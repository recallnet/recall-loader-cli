use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context as _, Result};
use hoku_provider::{
    fvm_shared::{address::Address, econ::TokenAmount},
    json_rpc::JsonRpcProvider,
    tx::{TxReceipt, TxStatus},
};
use hoku_sdk::{
    credits::{BuyOptions, Credits},
    machine::{
        bucket::{AddOptions, Bucket, DeleteOptions, GetOptions, Object},
        Machine,
    },
};
use hoku_signer::{AccountKind, Signer as _, Wallet};
use rand::{thread_rng, Rng as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tracing::{debug, error, info, trace, warn};

use crate::parse_private_key;

use crate::config::{Test, TestConfig};

pub struct TestRunner {
    provider: JsonRpcProvider,
    wallet: Wallet,
    test: Test,
    id: String,
}

impl TestRunner {
    pub async fn upload_test(self) -> Result<TestResult> {
        let TestRunner {
            provider,
            mut wallet,
            test,
            id,
        } = self;
        let delete_opts = test.delete_opts();
        let upload_config = test.upload.clone();
        let download_config = test.download.clone();
        let wait_retries = match test.add_opts().broadcast_mode {
            hoku_provider::tx::BroadcastMode::Commit => 10,
            _v => 100,
        };
        let machine = if let Some(bucket) = upload_config.bucket {
            let machine = Bucket::attach(bucket)
                .await
                .context("failed to attach bucket")?;
            info!(%id, "using existing machine as bucket: {}", machine.address());
            machine
        } else {
            let (machine, tx) = Bucket::new(
                &provider,
                &mut wallet,
                None,
                HashMap::new(),
                Default::default(),
            )
            .await?;
            info!(
                %id,
                addr=?wallet.address(),
                "Created new bucket {} in transaction hash: 0x{}",
                machine.address(),
                tx.hash
            );
            machine
        };
        let mut results = HashMap::with_capacity(upload_config.blob_count as usize);
        let mut tx_results = Vec::with_capacity(upload_config.blob_count as usize);
        for i in 0..upload_config.blob_count {
            let key = test.get_key_with_prefix(&i.to_string());
            match upload_blob(
                &provider,
                &mut wallet,
                &machine,
                &key,
                upload_config.blob_size_bytes(),
                test.add_opts(),
            )
            .await
            {
                Ok((time, tx)) => {
                    tx_results.push((key.clone(), tx));
                    results.insert(
                        key.clone(),
                        Timing {
                            bucket: machine.address(),
                            size: upload_config.blob_size_bytes(),
                            key,
                            upload_time: Some(time),
                            download_time: None,
                            delete_time: None,
                        },
                    );
                }
                Err(error) => {
                    error!(?error, %key, %id, "failed to upload");
                    // need to revert the sequence number since it was incremented by the sdk but failed
                    // TODO: update SDK to be nicer here
                    wallet.init_sequence(&provider).await?;
                    continue;
                }
            }
        }

        if results.is_empty() {
            error!(%id,"failed to upload any blobs");
            bail!("{id} failed to upload blobs");
        }

        if download_config
            .as_ref()
            .map_or(false, |c| c.should_download())
        {
            let opts = download_config.expect("download config exists").get_opts();
            loop_until_blob_found(tx_results, &provider, &machine, wait_retries).await;
            for (key, res) in results.iter_mut() {
                match download_blob(&provider, &machine, key, opts.clone(), true).await {
                    Ok(v) => {
                        res.download_time = Some(v);
                    }
                    Err(_error) => {
                        // logged in download_blob()
                        // error!(time=?error, key, "error downloading blob");
                    }
                }
            }
        } else {
            // not needed so give back the ram
            drop(tx_results);
        }

        if let Some(opts) = delete_opts {
            for (key, res) in results.iter_mut() {
                if let Ok(time) =
                    delete_blob(key, &provider, &mut wallet, &machine, opts.clone()).await
                {
                    res.delete_time = Some(time);
                }
            }
        }

        let results: Vec<Timing> = results.into_values().collect();
        debug!(?results, "upload");

        Ok(TestResult { times: results })
    }

    pub async fn generate(config: TestConfig) -> Result<Vec<Self>> {
        let network = config.network;
        let network_cfg = network.get_config();
        let obj_api = network_cfg.object_api_url;
        info!("using network '{network}' and object api: {obj_api}");

        let mut results = Vec::with_capacity(config.tests.len());
        let provider = JsonRpcProvider::new_http(network_cfg.rpc_url, None, Some(obj_api))
            .context("failed to setup json provider")?;
        // reuse wallets because we can't have multiple due to msg/actor sequence numbers getting out of sync
        // this wallet won't be able to be used concurrently as there's a mutex around the sequence number but it's better than errors
        let mut wallets: HashMap<Vec<u8>, Wallet> = HashMap::new();
        for (i, test) in config.tests.into_iter().enumerate() {
            let pk = test
                .private_key
                .or_else(|| config.private_key.clone())
                .ok_or_else(|| anyhow!("privateKey is required"))?;
            let key = parse_private_key(&pk)?;
            let sk_bytes = key.sk.serialize().to_vec();
            let mut wallet = if let Some(wallet) = wallets.get(&sk_bytes) {
                wallet.to_owned()
            } else {
                // Setup local wallet using private key from arg
                let mut wallet = Wallet::new_secp256k1(
                    key.sk,
                    AccountKind::Ethereum,
                    network_cfg.subnet_id.clone(),
                )
                .context("failed to create wallet")?;
                wallet.init_sequence(&provider).await.context(format!(
                    "does address exist on chain (eth={:?})",
                    key.eth_addr
                ))?;
                wallets.insert(sk_bytes, wallet.clone());
                wallet
            };
            info!(eth_address=?key.eth_addr, "using wallet for eth address");

            if let Some(credits) = test.buy_credit {
                let addr = wallet.address();
                let tx = Credits::buy(
                    &provider,
                    &mut wallet,
                    addr,
                    TokenAmount::from_whole(credits),
                    BuyOptions::default(),
                )
                .await
                .context("failed to buy credits")?;
                info!(eth_addr=?key.eth_addr, f_addr=?addr, "bought credits {credits} in tx {}", tx.hash);
            }

            results.push(TestRunner {
                provider: provider.clone(),
                wallet,
                test: test.test,
                id: format!("{i}-{}", key.eth_addr),
            })
        }

        Ok(results)
    }
}

#[derive(Debug)]
pub struct TestResult {
    times: Vec<Timing>,
}

#[derive(Debug)]
pub struct Timing {
    bucket: Address,
    #[allow(dead_code)]
    key: String,
    /// in bytes
    size: usize,
    upload_time: Option<Duration>,
    download_time: Option<Duration>,
    delete_time: Option<Duration>,
}

#[derive(Debug)]
pub struct TimeInfo {
    pub avg: f64,
    pub count: usize,
    pub total_time: Duration,
    pub max: Duration,
    pub min: Duration,
}

#[derive(Debug)]
pub struct BucketStats {
    pub address: Address,
    pub total_bytes: usize,
    pub time: TimeInfo,
}
impl BucketStats {
    /// mbps (megabits)
    pub fn mbps(&self) -> f64 {
        let bps = ((self.total_bytes * 8) as f64) / self.time.total_time.as_secs_f64();
        bps / 1_000_000_f64
    }

    /// MBps
    pub fn megabytes_per_second(&self) -> f64 {
        self.mbps() * 0.125
    }
}

#[derive(Debug)]
pub struct TestStats {
    upload: Option<BucketStats>,
    download: Option<BucketStats>,
    delete: Option<BucketStats>,
}

impl TestStats {
    pub fn from_upload(data: &TestResult) -> Self {
        let total_bytes = data.total_bytes();
        let address = data.bucket_address();

        let upload_stats = TimeInfo::try_from(data.uploads()).ok();
        let download_stats = TimeInfo::try_from(data.downloads()).ok();
        let delete_stats = TimeInfo::try_from(data.deletes()).ok();

        let upload = upload_stats.map(|time| BucketStats {
            address,
            total_bytes,
            time,
        });
        let download = download_stats.map(|time| BucketStats {
            address,
            total_bytes,
            time,
        });
        let delete = delete_stats.map(|time| BucketStats {
            address,
            total_bytes,
            time,
        });
        Self {
            upload,
            download,
            delete,
        }
    }
}

impl TryFrom<Vec<Duration>> for TimeInfo {
    type Error = anyhow::Error;

    fn try_from(value: Vec<Duration>) -> std::result::Result<Self, Self::Error> {
        if value.is_empty() {
            bail!("not supported for empty arrays")
        }
        let total_time = value.iter().sum::<Duration>();
        let count = value.len();
        let avg = total_time.as_secs_f64() / value.len() as f64;
        let min = *value.iter().min().expect("must have a min");
        let max = *value.iter().max().expect("must have a max");
        Ok(Self {
            avg,
            count,
            max,
            min,
            total_time,
        })
    }
}

impl TestResult {
    fn bucket_address(&self) -> Address {
        let upload_addresses: HashSet<Address> =
            HashSet::from_iter(self.times.iter().map(|t| t.bucket));
        let addresses: Vec<_> = upload_addresses.into_iter().collect();
        if addresses.len() > 1 {
            warn!(
                ?addresses,
                "test used multiple bucket addresses which is unexpected. picking first for reporting"
            )
        }
        addresses
            .first()
            .expect("test must have used a bucket")
            .to_owned()
    }

    fn uploads(&self) -> Vec<Duration> {
        self.times.iter().flat_map(|t| t.upload_time).collect()
    }

    fn downloads(&self) -> Vec<Duration> {
        self.times.iter().flat_map(|t| t.download_time).collect()
    }

    fn deletes(&self) -> Vec<Duration> {
        self.times.iter().flat_map(|t| t.delete_time).collect()
    }

    fn total_bytes(&self) -> usize {
        self.times.iter().map(|t| t.size).sum()
    }

    pub fn stats(&self) -> TestStats {
        TestStats::from_upload(self)
    }

    pub fn display_stats(&self) {
        let stats = self.stats();
        if let Some(s) = stats.upload {
            info!(address=%s.address, objects=%s.time.count, time=?s.time.total_time, avg_sec=%s.time.avg, max=?s.time.max, min=?s.time.min, mbps=%s.mbps(), MBps=%s.megabytes_per_second(), "upload stats");
        }
        if let Some(s) = stats.download {
            info!(address=%s.address, objects=%s.time.count, time=?s.time.total_time, avg_sec=%s.time.avg, max=?s.time.max, min=?s.time.min, mbps=%s.mbps(), MBps=%s.megabytes_per_second(), "download stats");
        }
        if let Some(s) = stats.delete {
            info!(address=%s.address, objects=%s.time.count, time=?s.time.total_time, avg_sec=%s.time.avg, max=?s.time.max, min=?s.time.min, mbps=%s.mbps(), MBps=%s.megabytes_per_second(), "delete stats");
        }
    }
}

pub(crate) async fn delete_blob(
    key: &str,
    provider: &JsonRpcProvider,
    wallet: &mut Wallet,
    machine: &Bucket,
    opts: DeleteOptions,
) -> Result<Duration> {
    let delete = Instant::now();
    match machine.delete(provider, wallet, key, opts).await {
        Ok(tx) => {
            let delete = delete.elapsed();
            trace!(key, time=?delete, "deleted in tx 0x{}", tx.hash);
            Ok(delete)
        }
        Err(e) => {
            error!(error=?e, %key, "failed to delete");
            Err(e)
        }
    }
}

async fn loop_until_blob_found(
    tx_results: Vec<(String, TxReceipt<Object>)>,
    provider: &JsonRpcProvider,
    machine: &Bucket,
    mut retries: u32,
) {
    info!(
        "waiting for network to resolve objects in bucket {}...",
        machine.address()
    );
    let last_uploaded = tx_results
        .last()
        .cloned()
        .expect("tx results can not be empty");

    let not_committed = tx_results
        .into_iter()
        .filter(|(_, tx)| !matches!(tx.status, TxStatus::Committed))
        .collect::<Vec<_>>();

    // would be nice to update the SDK to check tx status. for now just a hack to try a few
    let to_try = if not_committed.is_empty() {
        // all committed so will succeed
        vec![last_uploaded]
    } else {
        // often start failing and we wait for a really long time so we'll do the first and one from the middle
        vec![
            not_committed[0].clone(),
            not_committed[not_committed.len() / 2].clone(),
        ]
    };

    let start = Instant::now();

    for (i, (key, _tx)) in to_try.iter().enumerate() {
        while retries > 0 {
            if let Ok(time) = download_blob(
                provider,
                machine,
                key,
                GetOptions {
                    range: Some("0-99".to_string()),
                    ..Default::default()
                },
                false,
            )
            .await
            {
                let time_to_finish = start.elapsed();
                info!(
                    request_duration=?time,
                    "able to download/resolve object {}/{} in bucket {} (took {:?}).",
                    i+1, to_try.len(), machine.address(), time_to_finish
                );
                break;
            }
            debug!(
                "still waiting for network to resolve object in bucket {} with {key}...",
                machine.address()
            );
            retries -= 1;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}

async fn download_blob(
    provider: &JsonRpcProvider,
    machine: &Bucket,
    key: &str,
    opts: GetOptions,
    log_errors: bool,
) -> std::result::Result<Duration, Duration> {
    // Download the actual object at `foo/my_file`
    let mut obj_file = async_tempfile::TempFile::new().await.unwrap();
    let obj_path = obj_file.file_path().to_owned();
    trace!(?opts, "Downloading object to {}", obj_path.display());

    let open_file = obj_file.open_rw().await.unwrap();
    let now = Instant::now();
    let maybe_err = machine.get(provider, key, open_file, opts).await;
    let download = now.elapsed();
    match maybe_err {
        Ok(_) => {
            // Read the first 10 bytes of your downloaded 100 bytes
            let mut contents = vec![0; 10];
            obj_file.read_exact(&mut contents).await.unwrap();
            debug!("Successfully read first 10 bytes of {}", obj_path.display());
            Ok(download)
        }
        Err(e) => {
            if log_errors {
                warn!(error=?e, "failed to download data");
            } else {
                debug!(error=?e, "failed to download data");
            }
            Err(download)
        }
    }
}

async fn upload_blob(
    provider: &JsonRpcProvider,
    wallet: &mut Wallet,
    machine: &Bucket,
    key: &str,
    size: usize,
    mut opts: AddOptions,
) -> Result<(Duration, TxReceipt<Object>)> {
    let (file_path, size) = temp_file(size).await?;
    let bucket = machine.address();

    let mut metadata = HashMap::new();
    metadata.insert("upload bench test".to_string(), key.to_string());
    opts.metadata = metadata;

    let start = Instant::now();
    let tx = machine
        .add_from_path(provider, wallet, key, file_path.file_path(), opts)
        .await?;

    let time = start.elapsed();
    trace!(
        %bucket,
        key=%key,
        bytes=%size,
        "uploaded blob in {} ms in tx 0x{}",
        time.as_millis(),
        tx.hash,
    );

    Ok((time, tx))
}

async fn temp_file(size: usize) -> Result<(async_tempfile::TempFile, usize)> {
    let mut file = async_tempfile::TempFile::new().await?;

    let random_data = {
        let mut rng = thread_rng();
        let mut random_data = vec![0; size];
        rng.fill(&mut random_data[..]);
        random_data
    };
    file.write_all(&random_data).await?;
    file.flush().await?;
    Ok((file, size))
}
