use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use hoku_provider::tx::BroadcastMode;
use hoku_sdk::machine::bucket::Bucket;
use tokio::io::AsyncWrite;

pub mod sdk;

#[async_trait]
pub trait Target: Send + Sync {
    async fn create_bucket(&self) -> Result<Bucket>;
    async fn list_objects(&self);
    async fn add_object(
        &self,
        bucket: &Bucket,
        key: &str,
        path: &Path,
        metadata: HashMap<String, String>,
        overwrite: bool,
    ) -> Result<()>;

    async fn get_object(
        &self,
        bucket: &Bucket,
        key: &str,
        writer: Box<dyn AsyncWrite + Unpin + Send + 'static>,
        range: Option<String>,
    ) -> Result<()>;
    async fn delete_object(
        &self,
        bucket: &Bucket,
        key: &str,
        broadcast_mode: BroadcastMode,
    ) -> Result<()>;
}
