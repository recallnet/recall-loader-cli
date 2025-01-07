use anyhow::{bail, Context as _};
use hoku_sdk::{
    machine::{bucket::DeleteOptions, Machine},
    network::Network,
};
use tracing::{error, info};

use crate::{commands::runner::delete_blob, parse_private_key};

use super::{list_bucket_items, setup_provider_wallet_bucket, CleanupOpts};

pub async fn cleanup(opts: CleanupOpts) -> anyhow::Result<()> {
    let key = parse_private_key(&opts.key)?;
    let prefix = opts.prefix.clone();
    let network = opts.network.unwrap_or(Network::Devnet);
    let bucket = opts.bucket;
    let (provider, mut signer, machine) = setup_provider_wallet_bucket(key, network, bucket)
        .await
        .context("failed to setup")?;

    let (data, durations) = list_bucket_items(&provider, &machine, prefix.clone())
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
        match delete_blob(
            &key,
            &provider,
            &mut signer,
            &machine,
            DeleteOptions::default(),
        )
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
