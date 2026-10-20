//! Signed private-testnet network configuration and node key files.
//!
//! Every consensus parameter (validator set and voting power, peer
//! addresses, registered workers, issuance policy, round timeout and the
//! genesis checkpoint) is signed by the task/genesis authority. Nodes accept
//! the file only under an authority key pinned out of band
//! (`AIVOBLOCK_AUTHORITY_PUBLIC_KEY`), never the key written inside it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{hex, unhex, unhex_array};
use crate::genesis_checkpoint::{verify_signed_genesis_checkpoint, SignedGenesisCheckpointV1};
use crate::issuance::{ledger_supply, IssuancePolicyV1};
use crate::storage::ledger_state::{decode_ledger_state, encode_ledger_state};
use crate::validator_certificate::{TrustedValidatorSetV1, ValidatorMemberV1};

const CONFIG_DOMAIN: &[u8] = b"AIVOBLOCK-TESTNET-CONFIG-V1\0";
pub const CHAIN_ID: u32 = 1;
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ValidatorEntry {
    pub name: String,
    pub public_key: String,
    pub voting_power: u64,
    pub p2p_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkerEntry {
    pub name: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IssuanceEntry {
    pub initial_reward: u64,
    pub halving_interval: u64,
    pub max_supply: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GenesisEntry {
    pub header_hex: String,
    pub ledger_hex: String,
    pub signature_hex: String,
}

/// On-disk JSON (`network.json`). Field order is irrelevant: the signature
/// covers canonical binary bytes, never JSON text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfigFile {
    pub version: u32,
    pub chain_id: u32,
    pub network_name: String,
    pub authority_public_key: String,
    pub validators: Vec<ValidatorEntry>,
    pub workers: Vec<WorkerEntry>,
    pub issuance: IssuanceEntry,
    pub round_timeout_ms: u64,
    pub genesis: GenesisEntry,
    pub config_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorInfo {
    pub name: String,
    pub public_key: [u8; 32],
    pub voting_power: u64,
    pub address: String,
}

/// Verified, typed configuration.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub network_name: String,
    pub authority: [u8; 32],
    /// Sorted by public key: the canonical leader rotation order.
    pub validators: Vec<ValidatorInfo>,
    pub validator_set: TrustedValidatorSetV1,
    pub workers: BTreeSet<[u8; 32]>,
    pub worker_names: BTreeMap<[u8; 32], String>,
    pub policy: IssuancePolicyV1,
    pub round_timeout_ms: u64,
    pub checkpoint: SignedGenesisCheckpointV1,
    pub genesis_id: [u8; 32],
    pub genesis_timestamp_ms: u64,
}

impl NetworkConfig {
    pub fn validator(&self, key: &[u8; 32]) -> Option<&ValidatorInfo> {
        self.validators.iter().find(|v| v.public_key == *key)
    }

    pub fn validator_keys(&self) -> BTreeSet<[u8; 32]> {
        self.validators.iter().map(|v| v.public_key).collect()
    }

    /// Round-robin proposer for (height, round).
    pub fn leader(&self, height: u64, round: u64) -> [u8; 32] {
        let n = self.validators.len() as u64;
        let index = (height % n + round % n) % n;
        self.validators[index as usize].public_key
    }

    /// Worker key registered under the same node name as a validator.
    pub fn worker_of(&self, validator_name: &str) -> Option<[u8; 32]> {
        self.worker_names
            .iter()
            .find(|(_, name)| name.as_str() == validator_name)
            .map(|(key, _)| *key)
    }
}

/// Canonical bytes signed by the authority.
pub fn config_signing_bytes(
    validators: &[ValidatorInfo],
    workers: &BTreeSet<[u8; 32]>,
    policy: &IssuancePolicyV1,
    round_timeout_ms: u64,
    genesis_id: &[u8; 32],
) -> Result<Vec<u8>, String> {
    let members: Vec<ValidatorMemberV1> = validators
        .iter()
        .map(|v| ValidatorMemberV1 {
            public_key: v.public_key,
            voting_power: v.voting_power,
        })
        .collect();
    let set = TrustedValidatorSetV1::new(&members)?;
    let mut sorted = validators.to_vec();
    sorted.sort_by_key(|v| v.public_key);
    let mut raw = CONFIG_DOMAIN.to_vec();
    raw.extend_from_slice(&CHAIN_ID.to_be_bytes());
    raw.extend_from_slice(&set.commitment());
    raw.extend_from_slice(&(sorted.len() as u32).to_be_bytes());
    for validator in &sorted {
        let address = validator.address.as_bytes();
        if address.is_empty() || address.len() > 255 {
            return Err("Validator address must be 1..255 bytes".into());
        }
        raw.extend_from_slice(&validator.public_key);
        raw.extend_from_slice(&validator.voting_power.to_be_bytes());
        raw.push(address.len() as u8);
        raw.extend_from_slice(address);
    }
    raw.extend_from_slice(&(workers.len() as u32).to_be_bytes());
    for worker in workers {
        raw.extend_from_slice(worker);
    }
    raw.extend_from_slice(&policy.canonical_bytes());
    raw.extend_from_slice(&round_timeout_ms.to_be_bytes());
    raw.extend_from_slice(genesis_id);
    Ok(raw)
}

fn verify_key(key: &[u8; 32], what: &str) -> Result<(), String> {
    VerifyingKey::from_bytes(key)
        .map(|_| ())
        .map_err(|_| format!("Invalid Ed25519 {what} key"))
}

/// Parse and authenticate a network configuration under the pinned key.
pub fn verify_network_config(
    file: &NetworkConfigFile,
    pinned_authority: &[u8; 32],
) -> Result<NetworkConfig, String> {
    if file.version != 1 || file.chain_id != CHAIN_ID {
        return Err("Unsupported network configuration version or chain".into());
    }
    let authority: [u8; 32] = unhex_array(&file.authority_public_key)?;
    if authority != *pinned_authority {
        return Err("Configuration authority differs from the pinned authority key".into());
    }
    if file.validators.is_empty() || file.validators.len() > 64 || file.workers.is_empty() {
        return Err("Configuration needs 1..64 validators and at least one worker".into());
    }
    if file.round_timeout_ms < 100 || file.round_timeout_ms > 600_000 {
        return Err("Round timeout must be 100..600000 ms".into());
    }
    let mut validators = Vec::with_capacity(file.validators.len());
    for entry in &file.validators {
        let public_key = unhex_array(&entry.public_key)?;
        verify_key(&public_key, "validator")?;
        validators.push(ValidatorInfo {
            name: entry.name.clone(),
            public_key,
            voting_power: entry.voting_power,
            address: entry.p2p_address.clone(),
        });
    }
    validators.sort_by_key(|v| v.public_key);
    let mut workers = BTreeSet::new();
    let mut worker_names = BTreeMap::new();
    for entry in &file.workers {
        let key = unhex_array(&entry.public_key)?;
        verify_key(&key, "worker")?;
        if !workers.insert(key) {
            return Err("Duplicate worker key".into());
        }
        worker_names.insert(key, entry.name.clone());
    }
    let issuance = file.issuance;
    let policy = IssuancePolicyV1::new(
        issuance.initial_reward,
        issuance.halving_interval,
        issuance.max_supply,
    )?;
    let header: [u8; 178] = unhex_array(&file.genesis.header_hex)?;
    let ledger_bytes = unhex(&file.genesis.ledger_hex, 1024 * 1024)?;
    let initial_ledger =
        decode_ledger_state(&ledger_bytes).map_err(|e| format!("Genesis ledger: {e:?}"))?;
    let checkpoint = SignedGenesisCheckpointV1 {
        genesis_header: header,
        initial_ledger,
        signature: unhex_array(&file.genesis.signature_hex)?,
    };
    let genesis_id = verify_signed_genesis_checkpoint(&checkpoint, pinned_authority)
        .map_err(|e| format!("Unauthenticated genesis checkpoint: {e:?}"))?;
    if ledger_supply(&checkpoint.initial_ledger) > u128::from(policy.max_supply) {
        return Err("Genesis supply exceeds the issuance cap".into());
    }
    let signed = config_signing_bytes(
        &validators,
        &workers,
        &policy,
        file.round_timeout_ms,
        &genesis_id,
    )?;
    let signature: [u8; 64] = unhex_array(&file.config_signature)?;
    VerifyingKey::from_bytes(pinned_authority)
        .map_err(|_| "Invalid pinned authority key")?
        .verify_strict(&signed, &Signature::from_bytes(&signature))
        .map_err(|_| "Network configuration signature is invalid")?;
    let members: Vec<ValidatorMemberV1> = validators
        .iter()
        .map(|v| ValidatorMemberV1 {
            public_key: v.public_key,
            voting_power: v.voting_power,
        })
        .collect();
    let validator_set = TrustedValidatorSetV1::new(&members)?;
    let mut stamp = [0u8; 8];
    stamp.copy_from_slice(&header[170..178]);
    let genesis_timestamp_ms = u64::from_be_bytes(stamp);
    Ok(NetworkConfig {
        network_name: file.network_name.clone(),
        authority,
        validators,
        validator_set,
        workers,
        worker_names,
        policy,
        round_timeout_ms: file.round_timeout_ms,
        checkpoint,
        genesis_id,
        genesis_timestamp_ms,
    })
}

pub fn load_network_config(
    path: &Path,
    pinned_authority: &[u8; 32],
) -> Result<NetworkConfig, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err("Network configuration file is too large".into());
    }
    let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file: NetworkConfigFile =
        serde_json::from_slice(&raw).map_err(|e| format!("Invalid network.json: {e}"))?;
    verify_network_config(&file, pinned_authority)
}

/// Build the signed configuration file (testnet-init and tests only).
pub fn sign_network_config(
    authority: &SigningKey,
    network_name: &str,
    validators: &[ValidatorInfo],
    workers: &[(String, [u8; 32])],
    policy: &IssuancePolicyV1,
    round_timeout_ms: u64,
    checkpoint: &SignedGenesisCheckpointV1,
) -> Result<NetworkConfigFile, String> {
    use ed25519_dalek::Signer;
    let authority_key = authority.verifying_key().to_bytes();
    let genesis_id = verify_signed_genesis_checkpoint(checkpoint, &authority_key)
        .map_err(|e| format!("Genesis checkpoint: {e:?}"))?;
    let worker_set: BTreeSet<[u8; 32]> = workers.iter().map(|(_, key)| *key).collect();
    let signed = config_signing_bytes(
        validators,
        &worker_set,
        policy,
        round_timeout_ms,
        &genesis_id,
    )?;
    let ledger = encode_ledger_state(&checkpoint.initial_ledger).map_err(|e| format!("{e:?}"))?;
    Ok(NetworkConfigFile {
        version: 1,
        chain_id: CHAIN_ID,
        network_name: network_name.to_owned(),
        authority_public_key: hex(&authority_key),
        validators: validators
            .iter()
            .map(|v| ValidatorEntry {
                name: v.name.clone(),
                public_key: hex(&v.public_key),
                voting_power: v.voting_power,
                p2p_address: v.address.clone(),
            })
            .collect(),
        workers: workers
            .iter()
            .map(|(name, key)| WorkerEntry {
                name: name.clone(),
                public_key: hex(key),
            })
            .collect(),
        issuance: IssuanceEntry {
            initial_reward: policy.initial_reward,
            halving_interval: policy.halving_interval,
            max_supply: policy.max_supply,
        },
        round_timeout_ms,
        genesis: GenesisEntry {
            header_hex: hex(&checkpoint.genesis_header),
            ledger_hex: hex(&ledger),
            signature_hex: hex(&checkpoint.signature),
        },
        config_signature: hex(&authority.sign(&signed).to_bytes()),
    })
}

/// A node's private keys, read from its own key directory only.
pub struct NodeKeys {
    pub validator: SigningKey,
    pub worker: SigningKey,
    /// Present only on the node operated by the task authority.
    pub authority: Option<SigningKey>,
}

pub fn read_key_file(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let seed: [u8; 32] = unhex_array(text.trim())?;
    Ok(SigningKey::from_bytes(&seed))
}

pub fn write_key_file(path: &Path, key: &SigningKey) -> Result<(), String> {
    std::fs::write(path, format!("{}\n", hex(&key.to_bytes())))
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load_node_keys(dir: &Path) -> Result<NodeKeys, String> {
    let authority_path = dir.join("authority.key");
    Ok(NodeKeys {
        validator: read_key_file(&dir.join("validator.key"))?,
        worker: read_key_file(&dir.join("worker.key"))?,
        authority: if authority_path.exists() {
            Some(read_key_file(&authority_path)?)
        } else {
            None
        },
    })
}

pub fn genesis_digest(header: &[u8; 178]) -> [u8; 32] {
    Sha256::digest(header).into()
}
