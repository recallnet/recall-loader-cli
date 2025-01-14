use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use hoku_provider::json_rpc::JsonRpcProvider;
use hoku_provider::tx::BroadcastMode;
use hoku_sdk::machine::bucket::{AddOptions, Bucket, DeleteOptions, GetOptions, QueryOptions};
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

    async fn list_objects(
        &self,
        bucket: &Bucket,
        prefix: &str,
        start_key: Option<Vec<u8>>,
    ) -> Result<(Vec<String>, Option<Vec<u8>>)> {
        let options = QueryOptions {
            prefix: prefix.to_string(),
            start_key,
            ..Default::default()
        };

        let result = bucket.query(&self.provider, options).await?;
        let mut results = Vec::new();

        for (key_bytes, _object) in result.objects {
            let key = String::from_utf8_lossy(&key_bytes).to_string();
            results.push(key);
        }

        Ok((results, result.next_key.clone()))
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
