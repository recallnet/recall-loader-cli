mod delete;
mod query;
mod runner;

pub use delete::cleanup;
pub use query::query;

use std::sync::Arc;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Result;
use clap::Args;
use hoku_provider::{fvm_shared::address::Address, json_rpc::JsonRpcProvider};
use hoku_sdk::machine::{bucket::Bucket, Machine};
use hoku_sdk::network::Network;
use hoku_signer::{AccountKind, Signer as _, Wallet};
use runner::TestRunner;
use tokio::task::JoinSet;
use tracing::{debug, error, info};

use crate::config::{self, Broadcast, DownloadTest, Target, TestConfig, TestRunConfig, UploadTest};
use crate::stats::collector::Collector;
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
    /// If the test targets the SDK or S3 client.
    #[arg(long, default_value = "sdk")]
    pub target: Target,
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
    /// If the test targets the SDK or S3 client.
    #[arg(long, default_value = "sdk")]
    pub target: Target,
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
    /// The private key to use for the funder wallet
    #[arg(short, long, env = "HOKU_FUNDER_PRIVATE_KEY", hide_env_values = true)]
    pub funder_private_key: String,
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
    /// blob size in bytes
    #[arg(short = 's', long, default_value = "1.0")]
    pub blob_size: i64,
    /// Broadcast mode to use for uploads/deletes
    #[arg(long, default_value = "commit")]
    pub broadcast: Broadcast,
}

impl From<BasicTestOpts> for TestConfig {
    fn from(opts: BasicTestOpts) -> Self {
        Self {
            funder_private_key: opts.funder_private_key,
            private_key: Some(opts.key),
            network: opts.network.unwrap_or(Network::Devnet),
            tests: vec![TestRunConfig {
                private_key: None,
                request_funds: None,
                buy_credit: opts.buy_credits,
                target: opts.target,
                test: config::Test {
                    upload: UploadTest {
                        bucket: opts.bucket,
                        blob_count: opts.blob_cnt,
                        prefix: opts.prefix,
                        blob_size: opts.blob_size,
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
    let collector = Arc::new(Collector::new());
    let tests = TestRunner::generate(config, collector.clone()).await?;
    let mut tasks = JoinSet::new();
    for test in tests.into_iter() {
        tasks.spawn(async move {
            match test.execute().await {
                Ok(_) => {}
                Err(e) => {
                    error!(error=?e, "Failed to run test");
                }
            }
        });
    }
    tasks.join_all().await;

    if let Ok(mut collector) = Arc::try_unwrap(collector) {
        collector.close().await;
        collector.display_aggregated()
    } else {
        error!("collector is still referenced");
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
    target: Arc<dyn crate::targets::Target>,
    bucket: &Bucket,
    prefix: &str,
) -> Result<(Vec<String>, Vec<Duration>)> {
    let mut query_durations = Vec::new();
    let mut results = Vec::new();

    let start = Instant::now();
    let (mut list, mut next_key) = target.list_objects(bucket, prefix, None).await?;
    query_durations.push(start.elapsed());
    debug!(?list, "queried objects");
    results.extend(list);

    while let Some(start_key) = next_key {
        let start = Instant::now();
        (list, next_key) = target.list_objects(bucket, prefix, Some(start_key)).await?;
        query_durations.push(start.elapsed());

        results.extend(list);
    }
    Ok((results, query_durations))
}
