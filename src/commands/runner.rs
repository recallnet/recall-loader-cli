use std::sync::Arc;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context as _, Result};
use chrono::Utc;
use ethers::types::H160;
use hoku_provider::{fvm_shared::econ::TokenAmount, json_rpc::JsonRpcProvider};
use hoku_sdk::{
    credits::{BuyOptions, Credits},
    machine::{
        bucket::{AddOptions, Bucket, DeleteOptions, GetOptions},
        Machine,
    },
};
use hoku_signer::{AccountKind, Signer as _, Wallet};
use rand::{thread_rng, Rng as _};
use tokio::io::AsyncWriteExt as _;
use tracing::{debug, error, info};

use crate::config::{Target as ConfigTarget, Test, TestConfig};
use crate::funder::Funder;
use crate::parse_private_key;
use crate::stats::collector::Collector;
use crate::stats::ops::{Operation, OperationType};
use crate::targets::sdk::SdkTarget;
use crate::targets::Target;

pub struct TestRunner {
    target: Arc<dyn Target>,
    provider: JsonRpcProvider,
    wallet: Wallet,
    collector: Arc<Collector>,
    test: Test,
    id: String,
}

impl TestRunner {
    pub async fn execute(&self) -> Result<()> {
        let delete_opts = self.test.delete_opts();
        let upload_config = self.test.upload.clone();
        let download_config = self.test.download.clone();
        let wait_retries = match self.test.add_opts().broadcast_mode {
            hoku_provider::tx::BroadcastMode::Commit => 10,
            _v => 100,
        };
        let bucket = if let Some(bucket) = upload_config.bucket {
            let bucket = Bucket::attach(bucket)
                .await
                .context("failed to attach bucket")?;
            info!(%self.id, "using existing machine as bucket: {}", bucket.address());
            bucket
        } else {
            let bucket = self.target.clone().create_bucket().await?;
            info!(
                %self.id,
                addr=?self.wallet.address(),
                "created new bucket {}",
                bucket.address(),
            );
            bucket
        };

        let mut keys = Vec::with_capacity(upload_config.blob_count as usize);
        for i in 0..upload_config.blob_count {
            let key = self.test.get_key_with_prefix(&i.to_string());

            if self
                .upload_blob(
                    &bucket,
                    &key,
                    upload_config.blob_size_bytes(),
                    self.test.add_opts(),
                )
                .await
                .is_err()
            {
                // need to revert the sequence number since it was incremented by the sdk but failed
                // TODO: update SDK to be nicer here
                self.wallet.clone().init_sequence(&self.provider).await?;
                continue;
            }

            keys.push(key.clone());
        }

        if keys.is_empty() {
            error!(%self.id,"failed to upload any blobs");
            bail!("{} failed to upload blobs", self.id);
        }

        if download_config
            .as_ref()
            .is_some_and(|c| c.should_download())
        {
            let opts = download_config.expect("download config exists").get_opts();
            loop_until_blob_found(&keys, self.target.clone(), &bucket, wait_retries).await;
            for key in &keys {
                self.download_blob(&bucket, key, opts.clone(), upload_config.blob_size)
                    .await?;
            }
        }

        if let Some(opts) = delete_opts {
            for key in &keys {
                self.delete_blob(key, &bucket, opts.clone()).await?;
            }
        }

        Ok(())
    }

    pub async fn generate(config: TestConfig, collector: Arc<Collector>) -> Result<Vec<Self>> {
        let network = config.network;
        let network_cfg = network.get_config();
        let obj_api = network_cfg.object_api_url;
        info!("using network '{network}' and object api: {obj_api}");

        let mut results: Vec<TestRunner> = Vec::with_capacity(config.tests.len());
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

            if let Some(funds) = test.request_funds {
                Funder::fund(
                    &config.funder_private_key,
                    network_cfg.evm_rpc_url.as_ref(),
                    H160(key.eth_addr.0),
                    funds,
                )
                .await
                .context("failed to request funds")?;
            }

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

            let target = match test.target {
                ConfigTarget::Sdk => Arc::new(SdkTarget {
                    provider: provider.clone(),
                    wallet: wallet.clone(),
                }),
                ConfigTarget::S3 => unimplemented!(),
            };

            results.push(TestRunner {
                provider: provider.clone(),
                collector: collector.clone(),
                target,
                wallet,
                test: test.test,
                id: format!("{i}-{}", key.eth_addr),
            })
        }

        Ok(results)
    }

    async fn upload_blob(
        &self,
        bucket: &Bucket,
        key: &str,
        size: i64,
        opts: AddOptions,
    ) -> Result<()> {
        let (temp_file, size) = temp_file(size).await?;

        let mut metadata = HashMap::new();
        metadata.insert("upload bench test".to_string(), key.to_string());

        let start = Utc::now();
        let mut operation = Operation {
            id: self.id.clone(),
            start,
            op_type: OperationType::Put,
            file: key.to_string(),
            size,
            ..Default::default()
        };

        return match self
            .target
            .add_object(bucket, key, temp_file.file_path(), metadata, opts.overwrite)
            .await
        {
            Ok(_) => {
                let end = Utc::now();
                operation.end = end;
                self.collector.collect(operation).await?;

                let time = end.signed_duration_since(start).num_milliseconds();
                let address = bucket.address();
                info!(
                    %address,
                    key=%key,
                    bytes=%size,
                    "uploaded blob in {} ms",
                    time,
                );
                Ok(())
            }
            Err(err) => {
                let end = Utc::now();
                operation.end = end;
                operation.error = err.to_string();
                self.collector.collect(operation).await?;

                error!(error=?err, %key, "failed to upload");
                Err(err)
            }
        };
    }

    async fn delete_blob(&self, key: &str, bucket: &Bucket, opts: DeleteOptions) -> Result<()> {
        let mut operation = Operation {
            id: self.id.clone(),
            op_type: OperationType::Delete,
            file: key.to_string(),
            error: "".to_string(),
            ..Default::default()
        };

        let start = Utc::now();
        operation.start = start;
        match self
            .target
            .delete_object(bucket, key, opts.broadcast_mode)
            .await
        {
            Ok(_) => {
                let end = Utc::now();
                operation.end = end;
                self.collector.collect(operation).await?;

                let time = end.signed_duration_since(start);
                debug!(key, time=?time, "deleted");
                Ok(())
            }
            Err(e) => {
                let end = Utc::now();
                operation.end = end;
                operation.error = e.to_string();
                self.collector.collect(operation).await?;
                error!(error=?e, %key, "failed to delete");
                Err(e)
            }
        }
    }

    async fn download_blob(
        &self,
        bucket: &Bucket,
        key: &str,
        opts: GetOptions,
        size: i64,
    ) -> Result<()> {
        let start = Utc::now();
        let mut operation = Operation {
            id: self.id.clone(),
            start,
            op_type: OperationType::Get,
            size,
            file: key.to_string(),
            ..Default::default()
        };

        let obj_file = async_tempfile::TempFile::new().await.unwrap();

        let result = self
            .target
            .get_object(bucket, key, Box::new(obj_file), opts.range)
            .await;

        match result {
            Ok(_) => {
                let end = Utc::now();
                operation.end = end;
                self.collector.collect(operation).await?;
                info!(
                    "successfully downloaded object {} (took {:?}).",
                    key,
                    end.signed_duration_since(start).num_milliseconds()
                );
                Ok(())
            }
            Err(e) => {
                operation.end = Utc::now();
                operation.error = e.to_string();
                self.collector.collect(operation).await?;
                error!(error=?e, "failed to download data");
                Err(e)
            }
        }
    }
}

async fn loop_until_blob_found(
    keys: &[String],
    target: Arc<dyn Target>,
    machine: &Bucket,
    mut retries: u32,
) {
    info!(
        "waiting for network to resolve objects in bucket {}...",
        machine.address()
    );

    let start = Instant::now();

    for (i, key) in keys.iter().enumerate() {
        while retries > 0 {
            let writer = tokio::io::sink();
            if let Ok(time) = target
                .get_object(machine, key, Box::new(writer), None)
                .await
            {
                let time_to_finish = start.elapsed();
                info!(
                    request_duration=?time,
                    "able to download/resolve object {}/{} in bucket {} (took {:?}).",
                    i+1, keys.len(), machine.address(), time_to_finish
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

async fn temp_file(size: i64) -> Result<(async_tempfile::TempFile, i64)> {
    let mut file = async_tempfile::TempFile::new().await?;

    let random_data = {
        let mut rng = thread_rng();
        let mut random_data = vec![0; size as usize];
        rng.fill(&mut random_data[..]);
        random_data
    };
    file.write_all(&random_data).await?;
    file.flush().await?;
    Ok((file, size))
}
