use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use hoku_provider::json_rpc::JsonRpcProvider;
use hoku_provider::tx::BroadcastMode;
use hoku_sdk::machine::bucket::{AddOptions, Bucket, DeleteOptions, GetOptions};
use hoku_sdk::machine::Machine;
use hoku_signer::Wallet;
use tokio::io::AsyncWrite;

use crate::targets::Target;

pub struct SdkTarget {
    pub provider: JsonRpcProvider,
    pub wallet: Wallet,
}

#[async_trait]
impl Target for SdkTarget {
    async fn create_bucket(&self) -> Result<Bucket> {
        let mut wallet = self.wallet.clone();
        let (machine, _) = Bucket::new(
            &self.provider,
            &mut wallet,
            None,
            HashMap::new(),
            Default::default(),
        )
        .await?;
        Ok(machine)
    }

    async fn list_objects(&self) {
        todo!()
    }

    async fn add_object(
        &self,
        bucket: &Bucket,
        key: &str,
        path: &Path,
        metadata: HashMap<String, String>,
        overwrite: bool,
    ) -> Result<()> {
        let mut wallet = self.wallet.clone();
        let opts = AddOptions {
            metadata: metadata.clone(),
            overwrite,
            ..Default::default()
        };
        let _ = bucket
            .add_from_path(&self.provider, &mut wallet, key, path, opts)
            .await?;

        Ok(())
    }

    async fn get_object(
        &self,
        bucket: &Bucket,
        key: &str,
        writer: Box<dyn AsyncWrite + Unpin + Send + 'static>,
        range: Option<String>,
    ) -> Result<()> {
        let opts = GetOptions {
            range,
            ..Default::default()
        };
        bucket.get(&self.provider, key, writer, opts).await
    }

    async fn delete_object(
        &self,
        bucket: &Bucket,
        key: &str,
        broadcast_mode: BroadcastMode,
    ) -> Result<()> {
        let mut wallet = self.wallet.clone();
        let opts = DeleteOptions {
            broadcast_mode,
            ..Default::default()
        };
        bucket
            .delete(&self.provider, &mut wallet, key, opts)
            .await?;
        Ok(())
    }
}
