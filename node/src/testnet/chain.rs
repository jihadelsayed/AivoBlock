//! Finalized chain state for one testnet node: signed-genesis
//! initialization, fail-closed full replay on every start, and atomic
//! persistence of quorum-certified blocks. Rewards are settled into the
//! ledger only inside `finalize`, i.e. after a valid quorum certificate.

use std::path::Path;

use super::config::NetworkConfig;
use super::hex;
use crate::ai_block_v3::{
    decode_block_v3, encode_block_v3, execute_block_v3, reward_batch_id_v3, ExecutedBlockV3,
    ParentContext, V3Rules,
};
use crate::genesis_checkpoint::initialize_signed_genesis;
use crate::issuance::ledger_supply;
use crate::mining::scheduler::WorkRange;
use crate::qmlp::qmlp_result_commitment_v1;
use crate::settlement_v3::SettlementEngine;
use crate::storage::ledger_state::LedgerState;
use crate::storage::rocksdb::{FinalizedBlockWrite, FinalizedRecord, SettlementBatch};
use crate::storage::{Column, KeyValueStore, RocksDbStore};
use crate::transaction_commitment::ledger_state_root_v1;
use crate::validator_certificate::{
    decode_quorum_certificate_v1, encode_quorum_certificate_v1, verify_quorum_certificate_v1,
    QuorumCertificateV1,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockSummary {
    pub height: u64,
    pub block_id: String,
    pub task_id: String,
    /// Commitments taken from the finalized, validator-recomputed task.
    pub model_commitment: Option<String>,
    pub input_commitment: Option<String>,
    /// Per-partition verified work-claim commitments, not a whole-batch result digest.
    pub claim_result_commitments: Option<Vec<String>>,
    /// Independently recomputed whole-batch result from the finalized task bytes.
    pub result_commitment: Option<String>,
    /// `integer-linear-v1` or `qmlp-v1`; both are recomputed by validators.
    pub workload: &'static str,
    pub fee_budget: u64,
    pub issuance: u64,
    pub partitions: Vec<PartitionSummary>,
    pub round: u64,
    pub signers: usize,
    pub state_root: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PartitionSummary {
    pub worker: String,
    pub start: u64,
    pub count: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct StartupReport {
    pub initialized_genesis: bool,
    pub replayed_blocks: u64,
    pub replay_ms: u64,
}

/// A validator's durable precommit lock for one height: block ID and bytes.
pub type VoteLock = ([u8; 32], Vec<u8>);

pub struct Chain {
    db: RocksDbStore,
    pub height: u64,
    pub tip: [u8; 32],
    pub tip_timestamp_ms: u64,
    pub ledger: LedgerState,
    pub history: Vec<BlockSummary>,
}

fn summary(executed: &ExecutedBlockV3, certificate: &QuorumCertificateV1) -> BlockSummary {
    let header = &executed.block.header;
    BlockSummary {
        height: header.height,
        block_id: hex(&executed.id),
        task_id: hex(&executed.block.task.task_id),
        model_commitment: executed
            .block
            .task
            .qmlp
            .as_ref()
            .map(|_| hex(&executed.block.task.model_commitment)),
        input_commitment: executed
            .block
            .task
            .qmlp
            .as_ref()
            .map(|_| hex(&executed.block.task.input_commitment)),
        result_commitment: executed.block.task.qmlp.as_ref().map(|model| {
            hex(&qmlp_result_commitment_v1(model, &executed.block.task.rows,
                WorkRange { start: 0, count: executed.block.task.rows.len() as u64 })
                .expect("already-validated finalized QMLP task must have a valid complete result"))
        }),
        claim_result_commitments: executed.block.task.qmlp.as_ref().map(|_| {
            executed
                .block
                .claims
                .iter()
                .map(|claim| hex(&claim.result_commitment))
                .collect()
        }),
        workload: match executed.block.task.qmlp {
            Some(_) => "qmlp-v1",
            None => "integer-linear-v1",
        },
        fee_budget: executed.block.task.fee_budget,
        issuance: executed.block.issuance,
        partitions: executed
            .block
            .task
            .partitions
            .iter()
            .map(|p| PartitionSummary {
                worker: hex(&p.worker),
                start: p.start,
                count: p.count,
            })
            .collect(),
        round: certificate.round,
        signers: certificate.votes.len(),
        state_root: hex(&header.state_root),
        timestamp_ms: header.timestamp_ms,
    }
}

pub fn rules(config: &NetworkConfig) -> V3Rules<'_> {
    V3Rules {
        task_authority: &config.authority,
        workers: &config.workers,
        policy: &config.policy,
    }
}

/// Verify a certificate against the configured validator set and bind it
/// to exactly this height and block.
pub fn check_certificate(
    config: &NetworkConfig,
    certificate: &QuorumCertificateV1,
    height: u64,
    block_id: &[u8; 32],
) -> Result<(), String> {
    let verified = verify_quorum_certificate_v1(&config.validator_set, certificate)?;
    if verified.height != height || verified.block_id != *block_id {
        return Err("Quorum certificate is for a different height or block".into());
    }
    Ok(())
}

impl Chain {
    /// Open (or initialize) the node database and replay every finalized
    /// block from the signed genesis: certificates, task/claim signatures,
    /// recomputed AI results, issuance and Haskell+Rust settlement. Any
    /// mismatch with the persisted snapshot aborts startup.
    pub fn open(
        path: &Path,
        config: &NetworkConfig,
        engine: &SettlementEngine,
    ) -> Result<(Self, StartupReport), String> {
        let started = std::time::Instant::now();
        let db = RocksDbStore::open(path).map_err(|e| format!("Open chain database: {e}"))?;
        let mut report = StartupReport::default();
        if db.chain_tip().map_err(|e| e.to_string())?.is_none() {
            initialize_signed_genesis(&db, &config.checkpoint, &config.authority)?;
            report.initialized_genesis = true;
        }
        let stored_genesis = db
            .get(Column::Blocks, &config.genesis_id)
            .map_err(|e| e.to_string())?;
        if stored_genesis.as_deref() != Some(config.checkpoint.genesis_header.as_slice()) {
            return Err("Database genesis differs from the signed network configuration".into());
        }
        let (stored_tip, stored_height) = db
            .chain_tip()
            .map_err(|e| e.to_string())?
            .ok_or("Missing chain tip after genesis")?;
        let mut chain = Chain {
            db,
            height: 0,
            tip: config.genesis_id,
            tip_timestamp_ms: config.genesis_timestamp_ms,
            ledger: config.checkpoint.initial_ledger.clone(),
            history: Vec::new(),
        };
        for height in 1..=stored_height {
            let record = chain
                .db
                .finalized_block_at(height)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Missing finalized block at height {height}"))?;
            let certificate = decode_quorum_certificate_v1(&record.certificate)?;
            check_certificate(config, &certificate, height, &record.block_id)?;
            let executed = execute_block_v3(
                &chain.parent(),
                &rules(config),
                engine,
                &record.bytes,
                Some(&record.block_id),
            )
            .map_err(|e| format!("Replay of height {height} failed: {e}"))?;
            chain.advance(&executed, &certificate);
            report.replayed_blocks += 1;
        }
        if chain.tip != stored_tip {
            return Err("Replayed tip differs from the stored tip".into());
        }
        let stored_ledger = chain
            .db
            .read_local_ledger_state()
            .map_err(|e| e.to_string())?
            .ok_or("Missing persisted ledger snapshot")?;
        if stored_ledger != chain.ledger {
            return Err("Persisted ledger differs from full replay".into());
        }
        report.replay_ms = started.elapsed().as_millis() as u64;
        Ok((chain, report))
    }

    pub fn parent(&self) -> ParentContext<'_> {
        ParentContext {
            parent_id: self.tip,
            parent_height: self.height,
            parent_timestamp_ms: self.tip_timestamp_ms,
            prior: &self.ledger,
        }
    }

    fn advance(&mut self, executed: &ExecutedBlockV3, certificate: &QuorumCertificateV1) {
        self.height = executed.block.header.height;
        self.tip = executed.id;
        self.tip_timestamp_ms = executed.block.header.timestamp_ms;
        self.ledger = executed.settled.post.clone();
        self.history.push(summary(executed, certificate));
    }

    /// Persist an executed block under a verified quorum certificate. This
    /// is the only place balances change.
    pub fn finalize(
        &mut self,
        config: &NetworkConfig,
        executed: &ExecutedBlockV3,
        certificate: &QuorumCertificateV1,
    ) -> Result<(), String> {
        let header = &executed.block.header;
        if header.parent_hash != self.tip || header.height != self.height + 1 {
            return Err("Finalized block does not extend the local tip".into());
        }
        check_certificate(config, certificate, header.height, &executed.id)?;
        let grown = ledger_supply(&executed.settled.post)
            .checked_sub(ledger_supply(&self.ledger))
            .ok_or("Finalized block destroyed supply")?;
        if grown != u128::from(executed.block.issuance) {
            return Err("Executed supply change differs from block issuance".into());
        }
        let issued = executed.block.issuance;
        let encoded = encode_quorum_certificate_v1(certificate);
        let settlement = SettlementBatch {
            batch_id: reward_batch_id_v3(&executed.block.task.task_id),
            receipt_ids: &executed.settled.receipt_ids,
            validated_response: &executed.settled.evidence,
        };
        self.db
            .append_finalized_block(&FinalizedBlockWrite {
                block_id: &executed.id,
                parent_id: &self.tip,
                height: header.height,
                block_bytes: &executed.bytes,
                post_state: &executed.settled.post,
                settlement: &settlement,
                issued,
                certificate: &encoded,
            })
            .map_err(|e| format!("Persist finalized block: {e}"))?;
        self.advance(executed, certificate);
        Ok(())
    }

    /// Fully verify a block received with its certificate (sync path).
    pub fn verify_commit(
        &self,
        config: &NetworkConfig,
        engine: &SettlementEngine,
        block: &[u8],
        certificate: &QuorumCertificateV1,
    ) -> Result<ExecutedBlockV3, String> {
        let decoded = decode_block_v3(block, None)?;
        let id = encode_block_v3(&decoded)?.0;
        check_certificate(config, certificate, decoded.header.height, &id)?;
        execute_block_v3(&self.parent(), &rules(config), engine, block, Some(&id))
    }

    pub fn record_at(&self, height: u64) -> Result<Option<FinalizedRecord>, String> {
        self.db
            .finalized_block_at(height)
            .map_err(|e| e.to_string())
    }

    pub fn state_root(&self) -> String {
        ledger_state_root_v1(&self.ledger)
            .map(|root| hex(&root))
            .unwrap_or_default()
    }

    /// Durable per-height vote lock: a validator never precommits two
    /// different blocks at one height, even across crashes and restarts.
    pub fn load_vote_lock(&self, height: u64) -> Result<Option<VoteLock>, String> {
        let raw = self
            .db
            .get_metadata(&lock_key(height))
            .map_err(|e| e.to_string())?;
        match raw {
            None => Ok(None),
            Some(raw) if raw.len() > 32 => {
                let mut id = [0u8; 32];
                id.copy_from_slice(&raw[..32]);
                Ok(Some((id, raw[32..].to_vec())))
            }
            Some(_) => Err("Corrupt vote lock".into()),
        }
    }

    pub fn store_vote_lock(&self, height: u64, id: &[u8; 32], block: &[u8]) -> Result<(), String> {
        let mut value = id.to_vec();
        value.extend_from_slice(block);
        self.db
            .put_metadata_synced(&lock_key(height), &value)
            .map_err(|e| e.to_string())
    }
}

fn lock_key(height: u64) -> Vec<u8> {
    let mut key = b"vote-lock-v1:".to_vec();
    key.extend_from_slice(&height.to_be_bytes());
    key
}
