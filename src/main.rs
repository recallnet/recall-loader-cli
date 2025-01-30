// Copyright 2025 Recall Contributors
// SPDX-License-Identifier: Apache-2.0, MIT

use std::time::Instant;

use clap::Parser as _;
use recall_loader::{config::TestConfig, Cli};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(
                    "recall_loader=info"
                        .parse()
                        .expect("recall_loader=info is valid loglevel"),
                )
                .from_env_lossy(),
        )
        .init();
    let opts = Cli::parse();
    let start = Instant::now();
    let res = match opts.command {
        recall_loader::Commands::BasicTest(opts) => {
            let config = opts.into();
            recall_loader::commands::run(config).await
        }
        recall_loader::Commands::Cleanup(opts) => recall_loader::commands::cleanup(opts).await,
        recall_loader::Commands::RunTest(opts) => {
            let config = std::fs::read(opts.path)?;
            let config: TestConfig = serde_json::from_slice(&config)?;
            recall_loader::commands::run(config).await
        }
        recall_loader::Commands::Query(opts) => recall_loader::commands::query(opts).await,
    };
    let elapsed = start.elapsed();
    match res {
        Ok(_) => {
            info!(elapsed=?start.elapsed(), "completed");
        }
        Err(error) => {
            warn!(?error, ?elapsed, "completed with error");
        }
    }
    Ok(())
}
