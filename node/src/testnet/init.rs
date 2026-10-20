//! `testnet-init`: generate a private testnet directory.
//!
//! ```text
//! <out>/public/network.json      signed network configuration (shared)
//! <out>/public/authority.pub     pinned authority public key (hex)
//! <out>/testnet.env              AIVOBLOCK_AUTHORITY_PUBLIC_KEY=... for compose
//! <out>/node-N/validator.key     node N's private keys (mount ONLY into node N)
//! <out>/node-N/worker.key
//! <out>/node-1/authority.key     task/genesis authority (node 1 operator only)
//! ```
//!
//! Keys come from the OS random source. They are TEST keys for a private
//! research network and must never hold real value.

use std::path::Path;

use ed25519_dalek::{Signer, SigningKey};

use super::config::{sign_network_config, write_key_file, NetworkConfigFile, ValidatorInfo};
use super::{hex, now_ms, random_bytes32};
use crate::genesis_checkpoint::{genesis_checkpoint_signing_bytes_v1, SignedGenesisCheckpointV1};
use crate::issuance::IssuancePolicyV1;
use crate::storage::ledger_state::{LedgerState, LocalAccount};
use crate::transaction_commitment::{
    canonical_header_v1, ledger_state_root_v1, transaction_root_v1, CommittedHeaderV1,
};

#[derive(Debug, Clone)]
pub struct InitOptions {
    pub validators: usize,
    pub network_name: String,
    pub host_pattern: String,
    pub port: u16,
    pub initial_reward: u64,
    pub halving_interval: u64,
    pub max_supply: u64,
    pub genesis_fee_pool: u64,
    pub client_balance: u64,
    pub round_timeout_ms: u64,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            validators: 3,
            network_name: "aivoblock-private-testnet".into(),
            host_pattern: "node-{i}".into(),
            port: 30333,
            initial_reward: 50,
            halving_interval: 100,
            max_supply: 2_000_000,
            genesis_fee_pool: 1_000,
            client_balance: 100_000,
            round_timeout_ms: 2_000,
        }
    }
}

pub struct GeneratedTestnet {
    pub authority: SigningKey,
    pub validators: Vec<SigningKey>,
    pub workers: Vec<SigningKey>,
    pub config: NetworkConfigFile,
}

pub fn node_name(index: usize) -> String {
    format!("node-{index}")
}

/// Build keys, the signed genesis checkpoint and the signed configuration.
/// `addresses[i]` overrides the P2P address of validator i (tests).
pub fn generate(
    options: &InitOptions,
    addresses: Option<&[String]>,
) -> Result<GeneratedTestnet, String> {
    if options.validators == 0 || options.validators > 64 {
        return Err("Choose 1..64 validators".into());
    }
    let policy = IssuancePolicyV1::new(
        options.initial_reward,
        options.halving_interval,
        options.max_supply,
    )?;
    let authority = SigningKey::from_bytes(&random_bytes32());
    let validators: Vec<SigningKey> = (0..options.validators)
        .map(|_| SigningKey::from_bytes(&random_bytes32()))
        .collect();
    let workers: Vec<SigningKey> = (0..options.validators)
        .map(|_| SigningKey::from_bytes(&random_bytes32()))
        .collect();
    let mut ledger = LedgerState {
        fee_pool: options.genesis_fee_pool,
        ..LedgerState::default()
    };
    ledger.accounts.insert(
        authority.verifying_key().to_bytes(),
        LocalAccount {
            balance: options.client_balance,
            nonce: 0,
        },
    );
    let supply = u128::from(options.genesis_fee_pool) + u128::from(options.client_balance);
    if supply > u128::from(options.max_supply) {
        return Err("Genesis supply exceeds the maximum supply".into());
    }
    let header = CommittedHeaderV1 {
        version: 1,
        chain_id: 1,
        height: 0,
        parent_hash: [0; 32],
        transactions_root: transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?,
        puzzle_proofs_root: [0; 32],
        state_root: ledger_state_root_v1(&ledger).map_err(|e| format!("{e:?}"))?,
        timestamp_ms: now_ms(),
    };
    let header_bytes = canonical_header_v1(&header).map_err(|e| format!("{e:?}"))?;
    let signable = genesis_checkpoint_signing_bytes_v1(&header_bytes, &ledger)
        .map_err(|e| format!("{e:?}"))?;
    let checkpoint = SignedGenesisCheckpointV1 {
        genesis_header: header_bytes,
        initial_ledger: ledger,
        signature: authority.sign(&signable).to_bytes(),
    };
    let infos: Vec<ValidatorInfo> = validators
        .iter()
        .enumerate()
        .map(|(index, key)| {
            let number = (index + 1).to_string();
            let host = options.host_pattern.replace("{i}", &number);
            ValidatorInfo {
                name: node_name(index + 1),
                public_key: key.verifying_key().to_bytes(),
                voting_power: 1,
                address: addresses
                    .and_then(|list| list.get(index).cloned())
                    .unwrap_or_else(|| format!("{host}:{}", options.port)),
            }
        })
        .collect();
    let worker_entries: Vec<(String, [u8; 32])> = workers
        .iter()
        .enumerate()
        .map(|(index, key)| (node_name(index + 1), key.verifying_key().to_bytes()))
        .collect();
    let config = sign_network_config(
        &authority,
        &options.network_name,
        &infos,
        &worker_entries,
        &policy,
        options.round_timeout_ms,
        &checkpoint,
    )?;
    Ok(GeneratedTestnet {
        authority,
        validators,
        workers,
        config,
    })
}

/// Write a generated testnet to `out` (refuses to overwrite).
pub fn write_testnet(out: &Path, generated: &GeneratedTestnet) -> Result<(), String> {
    let public = out.join("public");
    if public.join("network.json").exists() {
        return Err(format!(
            "{} already contains a testnet; choose a new directory",
            out.display()
        ));
    }
    std::fs::create_dir_all(&public).map_err(|e| format!("{}: {e}", public.display()))?;
    let json = serde_json::to_string_pretty(&generated.config).map_err(|e| e.to_string())?;
    std::fs::write(public.join("network.json"), json + "\n").map_err(|e| e.to_string())?;
    let authority = hex(&generated.authority.verifying_key().to_bytes());
    std::fs::write(public.join("authority.pub"), format!("{authority}\n"))
        .map_err(|e| e.to_string())?;
    std::fs::write(
        out.join("testnet.env"),
        format!("AIVOBLOCK_AUTHORITY_PUBLIC_KEY={authority}\n"),
    )
    .map_err(|e| e.to_string())?;
    for (index, (validator, worker)) in generated
        .validators
        .iter()
        .zip(&generated.workers)
        .enumerate()
    {
        let dir = out.join(node_name(index + 1));
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        write_key_file(&dir.join("validator.key"), validator)?;
        write_key_file(&dir.join("worker.key"), worker)?;
        if index == 0 {
            write_key_file(&dir.join("authority.key"), &generated.authority)?;
        }
    }
    Ok(())
}
