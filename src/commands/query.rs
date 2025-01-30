use anyhow::Context as _;
use recall_sdk::network::Network;
use std::sync::Arc;
use tracing::info;

use super::{list_bucket_items, setup_provider_wallet_bucket, QueryOpts};
use crate::parse_private_key;
use crate::targets::sdk::SdkTarget;

pub async fn query(opts: QueryOpts) -> anyhow::Result<()> {
    let key = parse_private_key(&opts.key)?;
    let prefix = opts.prefix.clone();
    let network = opts.network.unwrap_or(Network::Devnet);
    let bucket = opts.bucket;
    let (provider, signer, machine) = setup_provider_wallet_bucket(key, network, bucket)
        .await
        .context("failed to setup")?;

    let target = match opts.target {
        crate::config::Target::Sdk => Arc::new(SdkTarget {
            provider: provider.clone(),
            wallet: signer.clone(),
        }),
        crate::config::Target::S3 => unimplemented!(),
    };

    let (keys, durations) = list_bucket_items(target, &machine, &prefix)
        .await
        .context("failed to query bucket")?;

    info!(
        ?durations,
        "queried {} keys with {} operations",
        keys.len(),
        durations.len()
    );
    Ok(())
}
