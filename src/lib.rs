// Copyright 2025 Recall Contributors
// SPDX-License-Identifier: Apache-2.0, MIT
pub mod commands;
pub mod config;
pub mod funder;
pub mod stats;
pub mod targets;

use clap::{command, Parser, Subcommand};
use commands::{BasicTestOpts, CleanupOpts, QueryOpts, RunTestOpts};
use recall_signer::key::parse_secret_key;
use recall_signer::{key::SecretKey, EthAddress};

#[derive(Parser, Debug, Clone)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Clone, Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum Commands {
    /// Run a basic test using cli args
    #[clap(alias = "basic")]
    BasicTest(BasicTestOpts),
    /// Clean up (delete) data from a bucket
    #[clap(alias = "delete")]
    Cleanup(CleanupOpts),
    /// Query keys from a bucket with a prefix
    Query(QueryOpts),
    #[clap(alias = "run")]
    /// Run a more sophisticated test from a config file
    RunTest(RunTestOpts),
}

pub(crate) fn parse_private_key(sk: &str) -> anyhow::Result<KeyData> {
    let sk = parse_secret_key(sk)?;
    let eth_addr = EthAddress::from(sk.public_key());
    let key = KeyData { sk, eth_addr };
    Ok(key)
}

#[derive(Debug)]
#[allow(dead_code)]
struct KeyData {
    sk: SecretKey,
    eth_addr: EthAddress,
}
