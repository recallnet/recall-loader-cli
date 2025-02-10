use crate::commands::downloader::Downloader;
use crate::config::{
    Broadcast, RandomizedNetwork, Target as ConfigTarget, TestConfig, TestRunConfig,
};
use crate::funder::Funder;
use crate::stats::collector::Collector;
use crate::stats::ops::{Operation, OperationType};
use crate::targets::sdk::SdkTarget;
use crate::targets::Target;
use crate::KeyData;
use anyhow::{bail, Context as _, Result};
use chrono::Utc;
use ethers::types::H160;
use recall_provider::{
    fvm_shared::econ::TokenAmount,
    json_rpc::JsonRpcProvider
};
use recall_sdk::{
    credits::{BuyOptions, Credits},
    machine::{bucket::Bucket, Machine},
};
use recall_signer::key::random_secretkey;
use recall_signer::{AccountKind, EthAddress, Signer as _, Wallet};
use rand::{thread_rng, Rng as _};
use std::sync::Arc;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt as _;
use tracing::{debug, error, info, warn};

pub struct TestRunner {
    target: Arc<dyn Target>,
    provider: JsonRpcProvider,
    wallet: Wallet,
    collector: Arc<Collector>,
    test: TestRunConfig,
    thread_id: String,
}

impl TestRunner {
    pub async fn execute(&self) -> Result<()> {
        let upload_config = self.test.upload.clone();
        let download_config = self.test.download.clone();

        let bucket = if let Some(bucket) = upload_config.bucket {
            let bucket = Bucket::attach(bucket)
                .await
                .context("failed to attach bucket")?;
            info!(%self.thread_id, "using existing machine as bucket: {}", bucket.address());
            bucket
        } else {
            let bucket = self.target.clone().create_bucket().await?;
            info!(
                %self.thread_id,
                addr=?self.wallet.address(),
                "created new bucket {}",
                bucket.address(),
            );
            bucket
        };

        let mut keys = Vec::with_capacity(upload_config.blob_count as usize);
        for i in 0..upload_config.blob_count {
            let key = self.test.upload.get_key_with_prefix(&i.to_string());

            if self
                .upload_blob(
                    &bucket,
                    &key,
                    upload_config.blob_size_bytes(),
                    upload_config.broadcast_mode,
                    upload_config.overwrite,
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
            error!(%self.thread_id,"failed to upload any blobs");
            bail!("{} failed to upload blobs", self.thread_id);
        }

        if let Some(config) = download_config {
            loop_until_blob_found(&keys, self.target.clone(), &bucket, 10).await;
            let mut downloader = Downloader::new(
                self.target.clone(),
                self.collector.clone(),
                self.thread_id.clone(),
                bucket.address(),
                config.concurrency(),
                upload_config.blob_size,
            );
            downloader.download(&keys).await?;
            downloader.close().await;
        }

        if self.test.delete {
            for key in &keys {
                self.delete_blob(key, &bucket).await?;
            }
        }

        Ok(())
    }

    pub async fn prepare(config: TestConfig, collector: Arc<Collector>) -> Result<Vec<Self>> {
        let network = config.network;
        let network_cfg = network.get_config();
        info!("using network '{network}'");

        let mut results: Vec<TestRunner> = Vec::with_capacity(config.test.num_accounts as usize);
        let provider = JsonRpcProvider::new_http(
            network.random_rpc_url(),
            network.chain_id(),
            None,
            Some(network.random_objects_api_url()),
        )
        .context("failed to setup json provider")?;
        // reuse wallets because we can't have multiple due to msg/actor sequence numbers getting out of sync
        // this wallet won't be able to be used concurrently as there's a mutex around the sequence number but it's better than errors
        let mut wallets: HashMap<Vec<u8>, Wallet> = HashMap::new();
        for i in 0..config.test.num_accounts {
            // create random account
            let sk = random_secretkey();
            let eth_addr = EthAddress::from(sk.public_key());
            let key = KeyData { sk, eth_addr };

            info!("account created {}", eth_addr.to_string());

            if let Some(funds) = config.test.request_funds {
                if let Err(err) = Funder::fund(
                    &config.funder_private_key,
                    network_cfg.evm_rpc_url.as_ref(),
                    H160(key.eth_addr.0),
                    funds,
                )
                .await {
                    warn!("failed to request funds. err = {}", err);
                    continue
                }
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
                match wallet.init_sequence(&provider).await {
                    Ok(_) => {
                        wallets.insert(sk_bytes, wallet.clone());
                        wallet
                    }
                    Err(err) => {
                        warn!(
                            "does address exist on chain (eth={:?}), err = {}",
                            key.eth_addr,
                            err
                        );
                        continue
                    }
                }
            };
            info!(eth_address=?key.eth_addr, "using wallet for eth address");

            if let Some(credits) = config.test.buy_credit {
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
                info!(eth_addr=?key.eth_addr, f_addr=?addr, "bought credits {credits} in tx {}", tx.hash());
            }

            let target = match config.test.target {
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
                test: config.test.clone(),
                thread_id: format!("{i}-{}", key.eth_addr),
            })
        }

        Ok(results)
    }

    async fn upload_blob(
        &self,
        bucket: &Bucket,
        key: &str,
        size: i64,
        broadcast_mode: Broadcast,
        overwrite: bool,
    ) -> Result<()> {
        let (temp_file, size) = temp_file(size).await?;
        let mut metadata = HashMap::new();
        metadata.insert("upload bench test".to_string(), key.to_string());

        let start = Utc::now();
        let mut operation = Operation {
            id: self.thread_id.clone(),
            start,
            op_type: OperationType::Put,
            file: key.to_string(),
            size,
            ..Default::default()
        };

        return match self
            .target
            .add_object(
                bucket,
                key,
                temp_file.file_path(),
                metadata,
                overwrite,
                broadcast_mode,
            )
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

    async fn delete_blob(&self, key: &str, bucket: &Bucket) -> Result<()> {
        let mut operation = Operation {
            id: self.thread_id.clone(),
            op_type: OperationType::Delete,
            file: key.to_string(),
            error: "".to_string(),
            ..Default::default()
        };

        let start = Utc::now();
        operation.start = start;
        match self.target.delete_object(bucket, key).await {
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

    // We wait for the objects from start, middle and end to be resolvable.
    // It's highly likely the other objects are resolvable too.
    let keys = if keys.len() > 10 {
        vec![keys[0].clone(), keys[keys.len() / 2].clone(), keys[keys.len() - 1].clone()]
    } else {
        keys.to_vec()
    };

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
