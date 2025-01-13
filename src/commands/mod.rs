mod delete;
mod query;
mod runner;

pub use delete::cleanup;
pub use query::query;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Result;
use clap::Args;
use hoku_provider::{fvm_shared::address::Address, json_rpc::JsonRpcProvider};
use hoku_sdk::machine::{bucket::Bucket, Machine};
use hoku_sdk::{machine::bucket::QueryOptions, network::Network};
use hoku_signer::{AccountKind, Signer as _, Wallet};
use runner::TestRunner;
use tokio::task::JoinSet;
use tracing::{debug, error, info};

use crate::config::{self, Broadcast, DownloadTest, Target, TestConfig, TestRunConfig, UploadTest};
use crate::KeyData;

#[derive(Args, Debug, Clone)]
pub struct RunTestOpts {
    #[arg(short, long)]
    pub path: PathBuf,
}

#[derive(Args, Debug, Clone)]
/// Will list all keys from a bucket and then delete them
pub struct CleanupOpts {
    /// The prefix to remove objects inside, should end in a /
    #[arg(short, long)]
    pub prefix: String,
    /// The private key to use for the signer wallet
    #[arg(short, long, env = "HOKU_PRIVATE_KEY", hide_env_values = true)]
    pub key: String,
    /// The network to use (defaults to devnet)
    #[arg(short, long, env = "HOKU_NETWORK")]
    pub network: Option<Network>,
    /// The bucket machine address (fvm address string)
    #[arg(short = 'b', long, value_parser = hoku_provider::util::parse_address)]
    pub bucket: Address,
}

#[derive(Args, Debug, Clone)]
/// Will list all keys from a bucket and report time of each list operation
pub struct QueryOpts {
    /// Everything under foo/ by default in bucket
    /// The exact prefix passed is used
    #[arg(short, long, default_value = "foo/")]
    pub prefix: String,
    /// The private key to use for the signer wallet
    #[arg(short, long, env = "HOKU_PRIVATE_KEY", hide_env_values = true)]
    pub key: String,
    /// The network to use (defaults to devnet)
    #[arg(short, long, env = "HOKU_NETWORK")]
    pub network: Option<Network>,
    /// The bucket machine address (fvm address string)
    #[arg(short = 'b', long, value_parser = hoku_provider::util::parse_address)]
    pub bucket: Address,
}

#[derive(Args, Debug, Clone)]
pub struct BasicTestOpts {
    /// Everything under /foo by default in bucket. A / is prepended
    /// can use `date +"%s"` to get the unix epoch seconds for a 'random' value for the test
    #[arg(short, long, default_value = "foo")]
    pub prefix: String,
    /// The private key to use for the signer wallet
    #[arg(short, long, env = "HOKU_PRIVATE_KEY", hide_env_values = true)]
    pub key: String,
    /// The network to use (defaults to devnet)
    #[arg(short, long, env = "HOKU_NETWORK")]
    pub network: Option<Network>,
    /// The bucket machine address (fvm address string)
    #[arg(short = 'b', long, value_parser = hoku_provider::util::parse_address)]
    pub bucket: Option<Address>,
    /// The count of credits to buy before starting (defaults to not buying any)
    #[arg(long)]
    pub buy_credits: Option<u32>,
    /// If the test targets the SDK or S3 client.
    #[arg(long, default_value = "sdk")]
    pub target: Target,
    /// whether blobs should be deleted afterward
    #[arg(long, default_value = "false")]
    pub delete: bool,
    /// whether to query and download blobs after uploading them
    #[arg(long, default_value = "false")]
    pub download: bool,
    #[arg(short = 'c', long, default_value = "100")]
    pub blob_cnt: u32,
    /// blob size in mb (0.1 = 100 bytes, 1000 = 1gb)
    #[arg(short = 's', long, default_value = "1.0")]
    pub blob_size_mb: f64,
    /// Broadcast mode to use for uploads/deletes
    #[arg(long, default_value = "commit")]
    pub broadcast: Broadcast,
}

impl From<BasicTestOpts> for TestConfig {
    fn from(opts: BasicTestOpts) -> Self {
        Self {
            private_key: Some(opts.key),
            network: opts.network.unwrap_or(Network::Devnet),
            tests: vec![TestRunConfig {
                private_key: None,
                buy_credit: opts.buy_credits,
                target: opts.target,
                test: config::Test {
                    upload: UploadTest {
                        bucket: opts.bucket,
                        blob_count: opts.blob_cnt,
                        prefix: opts.prefix,
                        blob_size_mb: opts.blob_size_mb,
                        overwrite: true,
                    },
                    download: opts.download.then_some(DownloadTest::Full(true)),
                    delete: opts.delete,
                    broadcast_mode: opts.broadcast,
                },
            }],
        }
    }
}

pub async fn run(config: TestConfig) -> Result<()> {
    let tests = TestRunner::generate(config).await?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    let mut tasks = JoinSet::new();
    for (i, test) in tests.into_iter().enumerate() {
        let tx = tx.clone();
        tasks.spawn(async move {
            match test.execute().await {
                Ok(res) => {
                    tx.send((i, res))
                        .await
                        .expect("should be able to send result");
                }
                Err(e) => {
                    error!(error=?e, "Failed to run test");
                }
            }
        });
    }
    drop(tx);
    tasks.join_all().await;
    while let Some((i, res)) = rx.recv().await {
        info!("got results for test index: {i}");
        res.display_stats();
    }
    Ok(())
}

pub(crate) async fn setup_provider_wallet_bucket(
    key: KeyData,
    network: Network,
    bucket: Address,
) -> anyhow::Result<(JsonRpcProvider, Wallet, Bucket)> {
    let network_cfg = network.get_config();
    let obj_api = network_cfg.object_api_url;
    info!("using network '{network}' and object api: {obj_api}");

    let provider = JsonRpcProvider::new_http(network_cfg.rpc_url, None, Some(obj_api))?;

    // Setup local wallet using private key from arg
    let mut wallet = Wallet::new_secp256k1(key.sk, AccountKind::Ethereum, network_cfg.subnet_id)?;
    wallet.init_sequence(&provider).await?;
    info!(
        "signer with address: {} on subnet id: {:?} ",
        wallet.address(),
        wallet.subnet_id()
    );

    let machine = Bucket::attach(bucket).await.unwrap();
    info!("using existing machine as bucket: {}", machine.address());

    Ok((provider, wallet, machine))
}

pub(crate) async fn list_bucket_items(
    provider: &JsonRpcProvider,
    machine: &Bucket,
    prefix: String,
) -> anyhow::Result<(Vec<String>, Vec<Duration>)> {
    let options = QueryOptions {
        prefix: prefix.clone(),
        ..Default::default()
    };

    let mut query_durations = Vec::new();
    let start = Instant::now();
    let mut list = machine.query(provider, options).await?;
    query_durations.push(start.elapsed());
    debug!(?list, "queried objects");

    let mut results = Vec::new();

    for (key_bytes, object) in list.objects {
        let key = String::from_utf8_lossy(&key_bytes).to_string();
        debug!("Query result for key {}: {}", key, object.hash);
        results.push(key);
    }
    while let Some(key) = list.next_key {
        let options = QueryOptions {
            prefix: prefix.clone(),
            start_key: Some(key),
            ..Default::default()
        };
        let start = Instant::now();
        list = machine.query(provider, options).await?;
        query_durations.push(start.elapsed());

        for (key_bytes, object) in list.objects {
            let key = String::from_utf8_lossy(&key_bytes).to_string();
            debug!("Query result for key {}: {}", key, object.hash);
            results.push(key);
        }
    }
    Ok((results, query_durations))
}
