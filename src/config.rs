use hoku_provider::{fvm_shared::address::Address, tx::BroadcastMode};
use hoku_sdk::{
    machine::bucket::{AddOptions, DeleteOptions, GetOptions},
    network::Network,
};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConfig {
    pub funder_private_key: String,
    pub private_key: Option<String>,
    pub network: Network,
    pub tests: Vec<TestRunConfig>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunConfig {
    pub private_key: Option<String>,
    pub request_funds: Option<u32>,
    pub buy_credit: Option<u32>,
    pub target: Target,
    pub test: Test,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub struct Test {
    pub upload: UploadTest,
    /// Whether to download the full object or use a range query.
    /// Only public for cli to set, should use getter
    pub download: Option<DownloadTest>,
    pub delete: bool,
    /// Broadcast mode for the transactions in the tests
    /// TODO: shared for now, could be per tx type in the future if it's interesting
    #[serde(default)]
    pub broadcast_mode: Broadcast,
}

impl Test {
    pub fn delete_opts(&self) -> Option<DeleteOptions> {
        if self.delete {
            Some(DeleteOptions {
                broadcast_mode: self.broadcast_mode.into(),
                ..Default::default()
            })
        } else {
            None
        }
    }

    pub fn add_opts(&self) -> AddOptions {
        AddOptions {
            overwrite: self.upload.overwrite,
            broadcast_mode: self.broadcast_mode.into(),
            ..Default::default()
        }
    }

    pub fn get_key_with_prefix(&self, name: &str) -> String {
        format!("{}/{name}", prefix_normalized(&self.upload.prefix))
    }
}

fn deserialize_address<'de, D>(deserializer: D) -> Result<Option<Address>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let address: Option<&str> = serde::de::Deserialize::deserialize(deserializer)?;
    address
        .map(hoku_provider::util::parse_address)
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
#[serde(untagged)]
pub enum DownloadTest {
    /// For now, if set to false, will not download
    /// should value be ignored? just needed something to deserialize
    Full(bool),
    Range(String),
}

impl DownloadTest {
    pub fn should_download(&self) -> bool {
        !matches!(self, DownloadTest::Full(false))
    }

    pub fn get_opts(&self) -> GetOptions {
        GetOptions {
            range: self.range(),
            ..Default::default()
        }
    }

    fn range(&self) -> Option<String> {
        match self {
            DownloadTest::Range(s) => Some(s.to_owned()),
            DownloadTest::Full(_) => None,
        }
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
