use std::sync::Arc;

use anyhow::{bail, Context as _};
use hoku_provider::tx::BroadcastMode;
use hoku_sdk::{machine::Machine, network::Network};
use tracing::{error, info};

use super::{list_bucket_items, setup_provider_wallet_bucket, CleanupOpts};

use crate::config::Target as ConfigTarget;
use crate::parse_private_key;
use crate::targets::sdk::SdkTarget;
use crate::targets::Target;

pub async fn cleanup(opts: CleanupOpts) -> anyhow::Result<()> {
    let key = parse_private_key(&opts.key)?;
    let prefix = opts.prefix.clone();
    let network = opts.network.unwrap_or(Network::Devnet);
    let bucket = opts.bucket;
    let (provider, signer, machine) = setup_provider_wallet_bucket(key, network, bucket)
        .await
        .context("failed to setup")?;

    let target = match opts.target {
        ConfigTarget::Sdk => Arc::new(SdkTarget {
            provider: provider.clone(),
            wallet: signer.clone(),
        }),
        ConfigTarget::S3 => unimplemented!(),
    };

    let (data, durations) = list_bucket_items(target.clone(), &machine, &prefix)
        .await
        .context("failed to query bucket")?;

    info!(
        ?durations,
        "queried {} keys with {} operations",
        data.len(),
        durations.len()
    );

    let address = machine.address();
    if data.is_empty() {
        error!("found no data in bucket {address} with {prefix}");
        bail!("found no data to delete in bucket {address} with {prefix}");
    }

    for key in data {
        match target
            .clone()
            .delete_object(&machine, &key, BroadcastMode::Commit)
            .await
        {
            Ok(time) => {
                info!("deleted blob with {key} in {:?}", time);
            }
            Err(e) => {
                error!("failed to delete blob with {key}: {e}");
            }
        }
    }

    Ok(())
}
