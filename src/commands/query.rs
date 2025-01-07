use anyhow::Context as _;
use hoku_sdk::network::Network;
use tracing::info;

use super::{list_bucket_items, setup_provider_wallet_bucket, QueryOpts};

use crate::parse_private_key;

pub async fn query(opts: QueryOpts) -> anyhow::Result<()> {
    let key = parse_private_key(&opts.key)?;
    let prefix = opts.prefix.clone();
    let network = opts.network.unwrap_or(Network::Devnet);
    let bucket = opts.bucket;
    let (provider, _signer, machine) = setup_provider_wallet_bucket(key, network, bucket)
        .await
        .context("failed to setup")?;

    let (keys, durations) = list_bucket_items(&provider, &machine, prefix.clone())
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
