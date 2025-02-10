use recall_provider::json_rpc::Url;
use recall_provider::{fvm_shared::address::Address, fvm_shared::chainid::ChainID, tx::BroadcastMode};
use recall_sdk::network::Network;
use rand::prelude::SliceRandom;
use rand::thread_rng;
use std::str::FromStr;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConfig {
    pub funder_private_key: String,
    pub network: Network,
    pub test: TestRunConfig,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunConfig {
    pub num_accounts: i32,
    pub request_funds: Option<u32>,
    pub buy_credit: Option<u32>,
    pub target: Target,
    pub upload: UploadTest,
    /// Whether to download the full object or use a range query.
    /// Only public for cli to set, should use getter
    pub download: Option<DownloadTest>,
    pub delete: bool,
}

fn deserialize_address<'de, D>(deserializer: D) -> Result<Option<Address>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let address: Option<&str> = serde::de::Deserialize::deserialize(deserializer)?;
    address
        .map(recall_provider::util::parse_address)
        .transpose()
        .map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadTest {
    /// Creates a new bucket if none
    #[serde(deserialize_with = "deserialize_address")]
    pub bucket: Option<Address>,
    /// How many blobs to upload
    pub blob_count: u32,
    /// Prefix blobs should be stored under (e.g. foo/bar). Should not end in /
    pub prefix: String,
    /// Size of each blob in bytes
    pub blob_size: i64,
    /// Overwrite the object if it already exists (true by default)
    #[serde(default = "true_bool")]
    pub overwrite: bool,
    /// Broadcast mode for the transactions in the tests
    #[serde(default)]
    pub broadcast_mode: Broadcast,
}

impl UploadTest {
    pub fn get_key_with_prefix(&self, name: &str) -> String {
        format!("{}/{name}", prefix_normalized(&self.prefix))
    }
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Broadcast {
    /// Return immediately after the transaction is broadcasted without waiting for check results.
    Async,
    /// Wait for the check results before returning from broadcast.
    Sync,
    /// Wait for the delivery results before returning from broadcast.
    #[default]
    Commit,
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Target {
    #[default]
    Sdk,
    S3,
}

impl From<Broadcast> for BroadcastMode {
    fn from(value: Broadcast) -> Self {
        match value {
            Broadcast::Commit => Self::Commit,
            Broadcast::Async => Self::Async,
            Broadcast::Sync => Self::Sync,
        }
    }
}

fn true_bool() -> bool {
    true
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTest {
    concurrency: i32,
}

impl DownloadTest {
    pub fn concurrency(&self) -> i32 {
        self.concurrency
    }
}

impl UploadTest {
    /// returns the size in bytes for each blob
    pub fn blob_size_bytes(&self) -> i64 {
        self.blob_size
    }
}

pub fn prefix_normalized(prefix: &str) -> String {
    if prefix.ends_with('/') {
        let mut res = prefix.to_string();
        res.pop();
        res
    } else {
        prefix.to_string()
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryTest {}

pub trait RandomizedNetwork {
    fn random_rpc_url(&self) -> Url;
    fn random_objects_api_url(&self) -> Url;
    fn chain_id(&self) -> ChainID;
}

impl RandomizedNetwork for Network {
    fn random_rpc_url(&self) -> Url {
        let urls = match self {
            Network::Mainnet => unimplemented!(),
            Network::Testnet => vec![
                "https://api.node-0.testnet.recall.network",
                "https://api.node-1.testnet.recall.network",
            ],
            Network::Localnet => vec![
                "http://localhost:26657",
                "http://localhost:26757",
                "http://localhost:26857",
            ],
            Network::Devnet => vec!["http://localhost:26657"],
        };
        let mut rng = thread_rng();
        let url = urls.choose(&mut rng).unwrap();
        Url::from_str(url).unwrap()
    }

    fn random_objects_api_url(&self) -> Url {
        let urls = match self {
            Network::Mainnet => unimplemented!(),
            Network::Testnet => vec![
                "https://objects.node-0.testnet.recall.network",
                "https://objects.node-1.testnet.recall.network",
            ],
            Network::Localnet => vec![
                "http://localhost:8001",
                "http://localhost:8002",
                "http://localhost:8003",
            ],
            Network::Devnet => vec!["http://localhost:8001"],
        };
        let mut rng = thread_rng();
        let url = urls.choose(&mut rng).unwrap();
        Url::from_str(url).unwrap()
    }

    fn chain_id(&self) -> ChainID {
        match self {
            Network::Mainnet => ChainID::from(24816),
            Network::Testnet => ChainID::from(2481632),
            Network::Localnet => ChainID::from(248163216),
            Network::Devnet => ChainID::from(248163216),
        }
    }
}
