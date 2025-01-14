use std::time::Duration;

use anyhow::{anyhow, Result};
use ethers::{
    core::types::TransactionRequest,
    middleware::SignerMiddleware,
    prelude::*,
    providers::{Http, Provider},
};
use tracing::info;

pub struct Funder {}

impl Funder {
    pub async fn fund(
        private_key: &str,
        provider_url: &str,
        to: Address,
        whole_amount: u32,
    ) -> Result<()> {
        let provider =
            Provider::<Http>::try_from(provider_url)?.interval(Duration::from_millis(10u64));

        let chain_id = provider.get_chainid().await?;
        let wallet: LocalWallet = private_key
            .parse::<LocalWallet>()?
            .with_chain_id(chain_id.as_u64());

        let amount_attos = U256::from(whole_amount) * U256::exp10(18usize);
        let gas_price = provider.get_gas_price().await?;

        let client = SignerMiddleware::new(provider, wallet);
        let tx = TransactionRequest::new()
            .to(to)
            .value(amount_attos)
            .gas_price(gas_price);
        let pending_tx = client.send_transaction(tx, None).await?;
        let _ = pending_tx
            .await?
            .ok_or_else(|| anyhow!("tx dropped from mempool"))?;

        info!("account {} funded", to);
        Ok(())
    }
}
