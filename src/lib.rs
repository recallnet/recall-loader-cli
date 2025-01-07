// Copyright 2024 Hoku Contributors
// SPDX-License-Identifier: Apache-2.0, MIT
pub mod commands;
pub mod config;

use std::path::PathBuf;
use std::vec;

use anyhow::Context as _;

use clap::{command, Parser, Subcommand};
use commands::{BasicTestOpts, CleanupOpts, QueryOpts, RunTestOpts};
use fendermint_crypto::PublicKey;
use hoku_signer::{
    key::{parse_secret_key, SecretKey},
    EthAddress,
};
use tracing::info;

const MB_F64: f64 = 1000_f64 * 1000_f64;

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
    let pk = sk.public_key();
    let eth_addr = EthAddress::from(pk);
    let key = KeyData { sk, pk, eth_addr };
    Ok(key)
}

#[derive(Debug)]
#[allow(dead_code)]
struct KeyData {
    sk: SecretKey,
    pk: PublicKey,
    eth_addr: EthAddress,
}

fn get_test_keys(dir: PathBuf) -> anyhow::Result<KeyData> {
    info!("reading keys from: {}", dir.display());
    let b64 = std::fs::read_to_string(dir).context("failed to read secret key")?;
    let bz = fendermint_crypto::from_b64(&b64)?;
    let sk = SecretKey::try_from(bz)?;
    let pk = sk.public_key();
    let eth_addr = EthAddress::from(pk);

    Ok(KeyData { sk, pk, eth_addr })
}

async fn load_devnet_keys() -> anyhow::Result<Vec<KeyData>> {
    let keys = vec!["alice.sk", "ellie.sk", "fonzi.sk", "grape.sk"];
    let dir = PathBuf::from("/Users/david/3box/hoku/ipc/test-network/keys/");
    let mut keys_to_use = Vec::with_capacity(keys.len());
    for key in keys {
        keys_to_use.push(get_test_keys(dir.join(key))?);
    }

    Ok(keys_to_use)
}
