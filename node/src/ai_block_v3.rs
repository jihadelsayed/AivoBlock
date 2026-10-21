//! V3 AI-work blocks for the private research testnet.
//!
//! A task is signed by the externally pinned task authority and commits to
//! the complete integer-inference model and input, the fee budget, the
//! settlement height (`sequence`) and an explicit partition plan naming the
//! registered worker key for every contiguous row range. Each worker signs
//! its partition result. Every validator recomputes every partition from
//! the bytes inside the block (no Python, no worker trust), checks the
//! scheduled issuance and executes settlement before voting.
//!
//! Two workloads are consensus-valid: the original integer linear model,
//! and QMLP-V1, an integer-only quantized two-layer MLP
//! (`protocol/spec/quantized-mlp-v1.md`). A QMLP task's canonical encoding
//! starts with its own tag; linear tasks keep their original bytes.
//!
//! This is full recomputation of tiny deterministic workloads. It is NOT a
//! succinct or zero-knowledge proof and does not scale to large models.

use std::collections::BTreeSet;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::ai_proof_v2::{result_commitment_for_rows_v1, rules_commitment_v1};
use crate::block_replay::decode_header_v1;
use crate::issuance::{ledger_supply, IssuancePolicyV1, IssuedSettlementV3};
use crate::mining::receipts::VerifiedWorkReceipt;
use crate::mining::scheduler::WorkRange;
use crate::qmlp::{qmlp_result_commitment_v1, qmlp_rules_commitment_v1, QmlpModelV1};
use crate::reward_block::RewardWorkV1;
use crate::settlement_v3::{SettledBlockV3, SettlementEngine};
use crate::storage::ledger_state::LedgerState;
use crate::transaction_commitment::{
    canonical_header_v1, ledger_state_root_v1, transaction_root_v1, CommittedHeaderV1,
};

pub const BODY_DOMAIN_V3: &[u8] = b"AIVOBLOCK-AI-WORK-BLOCK-V3\0";
const TASK_ID_DOMAIN: &[u8] = b"AIVOBLOCK-AI-TASK-ID-V3\0";
const TASK_SIGNATURE_DOMAIN: &[u8] = b"AIVOBLOCK-SIGNED-AI-TASK-V3\0";
const CLAIM_SIGNATURE_DOMAIN: &[u8] = b"AIVOBLOCK-SIGNED-AI-RESULT-V3\0";
const BATCH_DOMAIN: &[u8] = b"AIVOBLOCK-AI-WORK-REWARD-BATCH-V3\0";
const ROOT_DOMAIN: &[u8] = b"AIVOBLOCK-AI-WORK-ROOT-V3\0";
const MODEL_DOMAIN: &[u8] = b"AIVOBLOCK-INT-LINEAR-V1\0";
const INPUT_DOMAIN: &[u8] = b"AIVOBLOCK-INFERENCE-INPUT-V1\0";
/// Prefix of a QMLP-V1 task encoding. A linear task starts with its u64
/// sequence instead; heights never approach these leading bytes.
const QMLP_TASK_TAG: &[u8] = b"AIVOBLOCK-AI-TASK-QMLP-V1\0";
pub const CHAIN_ID: u32 = 1;
pub const MAX_ROWS: usize = 256;
pub const MAX_FEATURES: usize = 32;
pub const MAX_PARTITIONS: usize = 64;
pub const MAX_ABS_VALUE: i64 = 1_000_000;
pub const MAX_BLOCK_BYTES: usize = 256 * 1024;
const HEADER_BYTES: usize = 178;
const PARTITION_BYTES: usize = 8 + 8 + 32;
const CLAIM_BYTES: usize = 32 + 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionV3 {
    pub start: u64,
    pub count: u64,
    pub worker: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiTaskV3 {
    /// The block height that must settle this task.
    pub sequence: u64,
    pub created_ms: u64,
    pub deadline_height: u64,
    pub fee_budget: u64,
    pub model_commitment: [u8; 32],
    pub input_commitment: [u8; 32],
    pub rules_commitment: [u8; 32],
    /// Integer linear model weights; empty for a QMLP task.
    pub weights: Vec<i64>,
    pub rows: Vec<Vec<i64>>,
    /// When set, the task runs QMLP-V1 inference instead of the linear model.
    pub qmlp: Option<QmlpModelV1>,
    pub partitions: Vec<PartitionV3>,
    pub task_id: [u8; 32],
    pub authority_signature: [u8; 64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkClaimV3 {
    pub task_id: [u8; 32],
    pub partition: u32,
    pub worker: [u8; 32],
    pub result_commitment: [u8; 32],
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiWorkBlockV3 {
    pub header: CommittedHeaderV1,
    pub task: AiTaskV3,
    pub issuance: u64,
    /// Exactly one claim per partition, in partition order.
    pub claims: Vec<WorkClaimV3>,
}

/// Consensus rules from the signed network configuration.
#[derive(Debug, Clone, Copy)]
pub struct V3Rules<'a> {
    pub task_authority: &'a [u8; 32],
    pub workers: &'a BTreeSet<[u8; 32]>,
    pub policy: &'a IssuancePolicyV1,
}

/// The finalized parent that a candidate block must extend.
#[derive(Debug, Clone, Copy)]
pub struct ParentContext<'a> {
    pub parent_id: [u8; 32],
    pub parent_height: u64,
    pub parent_timestamp_ms: u64,
    pub prior: &'a LedgerState,
}

/// A fully verified and executed block, ready to be voted on or finalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedBlockV3 {
    pub id: [u8; 32],
    pub bytes: Vec<u8>,
    pub block: AiWorkBlockV3,
    pub settled: SettledBlockV3,
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn in_bounds(value: &i64) -> bool {
    (-MAX_ABS_VALUE..=MAX_ABS_VALUE).contains(value)
}

/// Same byte encoding as the Python `IntegerLinearModel.commitment` and
/// `ai_proof_v2::model_commitment_v1`, with V3 size limits.
pub fn model_commitment_v3(weights: &[i64]) -> Result<[u8; 32], String> {
    if weights.is_empty() || weights.len() > MAX_FEATURES || !weights.iter().all(in_bounds) {
        return Err("Invalid V3 model weights".into());
    }
    let mut bytes = MODEL_DOMAIN.to_vec();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&(weights.len() as u32).to_be_bytes());
    for value in weights {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    Ok(hash(&bytes))
}

/// Same byte encoding as Python `input_commitment`, with V3 size limits.
pub fn input_commitment_v3(rows: &[Vec<i64>], width: usize) -> Result<[u8; 32], String> {
    if rows.is_empty()
        || rows.len() > MAX_ROWS
        || width == 0
        || width > MAX_FEATURES
        || rows
            .iter()
            .any(|row| row.len() != width || !row.iter().all(in_bounds))
    {
        return Err("Invalid V3 inference input rows".into());
    }
    let mut bytes = INPUT_DOMAIN.to_vec();
    bytes.extend_from_slice(&(rows.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&(width as u32).to_be_bytes());
    for row in rows {
        for value in row {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
    }
    Ok(hash(&bytes))
}

/// Canonical task fields preceding the task ID and signature.
fn task_body_bytes(task: &AiTaskV3) -> Vec<u8> {
    let mut raw = Vec::new();
    if task.qmlp.is_some() {
        raw.extend_from_slice(QMLP_TASK_TAG);
    }
    raw.extend_from_slice(&task.sequence.to_be_bytes());
    raw.extend_from_slice(&task.created_ms.to_be_bytes());
    raw.extend_from_slice(&task.deadline_height.to_be_bytes());
    raw.extend_from_slice(&task.fee_budget.to_be_bytes());
    raw.extend_from_slice(&task.model_commitment);
    raw.extend_from_slice(&task.input_commitment);
    raw.extend_from_slice(&task.rules_commitment);
    if let Some(model) = &task.qmlp {
        // model body, then u32 row count and every input as one int8 byte.
        raw.extend_from_slice(&model.encode_body());
        raw.extend_from_slice(&(task.rows.len() as u32).to_be_bytes());
        for row in &task.rows {
            raw.extend(row.iter().map(|value| *value as i8 as u8));
        }
    } else {
        raw.extend_from_slice(&(task.rows.len() as u32).to_be_bytes());
        raw.extend_from_slice(&(task.weights.len() as u32).to_be_bytes());
        for weight in &task.weights {
            raw.extend_from_slice(&weight.to_be_bytes());
        }
        for row in &task.rows {
            for value in row {
                raw.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
    raw.extend_from_slice(&(task.partitions.len() as u32).to_be_bytes());
    for partition in &task.partitions {
        raw.extend_from_slice(&partition.start.to_be_bytes());
        raw.extend_from_slice(&partition.count.to_be_bytes());
        raw.extend_from_slice(&partition.worker);
    }
    raw
}

/// The task ID commits to the chain and to every field above, including
/// the exact worker assignment plan, so no partition can be reassigned.
pub fn compute_task_id_v3(task: &AiTaskV3) -> [u8; 32] {
    let mut raw = TASK_ID_DOMAIN.to_vec();
    raw.extend_from_slice(&CHAIN_ID.to_be_bytes());
    raw.extend_from_slice(&task_body_bytes(task));
    hash(&raw)
}

pub fn task_signing_bytes_v3(task_id: &[u8; 32]) -> Vec<u8> {
    let mut raw = TASK_SIGNATURE_DOMAIN.to_vec();
    raw.extend_from_slice(&CHAIN_ID.to_be_bytes());
    raw.extend_from_slice(task_id);
    raw
}

/// Fill in the task ID and authority signature (task authority only).
pub fn sign_task_v3(mut task: AiTaskV3, authority: &SigningKey) -> AiTaskV3 {
    task.task_id = compute_task_id_v3(&task);
    task.authority_signature = authority
        .sign(&task_signing_bytes_v3(&task.task_id))
        .to_bytes();
    task
}

pub fn reward_batch_id_v3(task_id: &[u8; 32]) -> [u8; 32] {
    let mut raw = BATCH_DOMAIN.to_vec();
    raw.extend_from_slice(&CHAIN_ID.to_be_bytes());
    raw.extend_from_slice(task_id);
    hash(&raw)
}

/// Structural, commitment, partition and authority checks. `height` is the
/// height at which the task would settle; a task is expired once its
/// deadline is below it and is only valid at exactly `sequence`.
pub fn verify_task_v3(task: &AiTaskV3, rules: &V3Rules<'_>, height: u64) -> Result<(), String> {
    if task.sequence == 0 || task.deadline_height < task.sequence {
        return Err("Task sequence or deadline is invalid".into());
    }
    if height > task.deadline_height {
        return Err("Task expired: deadline is below the settlement height".into());
    }
    if height != task.sequence {
        return Err("Task sequence does not match the settlement height".into());
    }
    let (model_commitment, rules_commitment, width) = match &task.qmlp {
        None => (
            model_commitment_v3(&task.weights)?,
            rules_commitment_v1(),
            task.weights.len(),
        ),
        Some(model) => {
            if !task.weights.is_empty() {
                return Err("A QMLP task must not carry linear weights".into());
            }
            for row in &task.rows {
                model.check_row(row)?;
            }
            (
                model.commitment()?,
                qmlp_rules_commitment_v1(),
                model.d_in as usize,
            )
        }
    };
    if task.model_commitment != model_commitment
        || task.input_commitment != input_commitment_v3(&task.rows, width)?
        || task.rules_commitment != rules_commitment
    {
        return Err("Task model, input or rules commitment mismatch".into());
    }
    if task.partitions.is_empty() || task.partitions.len() > MAX_PARTITIONS {
        return Err("Task must have 1..64 partitions".into());
    }
    let mut next = 0u64;
    let mut seen = BTreeSet::new();
    for partition in &task.partitions {
        if partition.count == 0 || partition.start != next {
            return Err("Partitions must be contiguous, ordered and non-overlapping".into());
        }
        next = next
            .checked_add(partition.count)
            .ok_or("Partition range overflow")?;
        if !rules.workers.contains(&partition.worker) {
            return Err("Partition assigned to an unregistered worker".into());
        }
        if !seen.insert(partition.worker) {
            return Err("Worker assigned to more than one partition".into());
        }
    }
    if next != task.rows.len() as u64 {
        return Err("Partitions must cover every task row exactly once".into());
    }
    if task.task_id != compute_task_id_v3(task) {
        return Err("Task ID does not commit to the task contents".into());
    }
    let authority = VerifyingKey::from_bytes(rules.task_authority)
        .map_err(|_| "Invalid pinned task authority key")?;
    authority
        .verify_strict(
            &task_signing_bytes_v3(&task.task_id),
            &Signature::from_bytes(&task.authority_signature),
        )
        .map_err(|_| "Task is not signed by the pinned task authority".into())
}

pub fn claim_signing_bytes_v3(task: &AiTaskV3, claim: &WorkClaimV3) -> Result<Vec<u8>, String> {
    let partition = task
        .partitions
        .get(claim.partition as usize)
        .ok_or("Claim names a nonexistent partition")?;
    let mut raw = CLAIM_SIGNATURE_DOMAIN.to_vec();
    raw.extend_from_slice(&CHAIN_ID.to_be_bytes());
    raw.extend_from_slice(&claim.task_id);
    raw.extend_from_slice(&claim.partition.to_be_bytes());
    raw.extend_from_slice(&claim.worker);
    raw.extend_from_slice(&partition.start.to_be_bytes());
    raw.extend_from_slice(&partition.count.to_be_bytes());
    raw.extend_from_slice(&claim.result_commitment);
    Ok(raw)
}

/// Independent recomputation of one partition from the committed task data.
pub fn expected_result_v3(task: &AiTaskV3, partition: u32) -> Result<[u8; 32], String> {
    let entry = task
        .partitions
        .get(partition as usize)
        .ok_or("Nonexistent partition")?;
    let range = WorkRange {
        start: entry.start,
        count: entry.count,
    };
    match &task.qmlp {
        None => result_commitment_for_rows_v1(&task.weights, &task.rows, range),
        Some(model) => qmlp_result_commitment_v1(model, &task.rows, range),
    }
}

/// Worker-side: sign a result for the partition assigned to `worker_key`.
pub fn sign_claim_v3(
    task: &AiTaskV3,
    partition: u32,
    worker_key: &SigningKey,
    result_commitment: [u8; 32],
) -> Result<WorkClaimV3, String> {
    let mut claim = WorkClaimV3 {
        task_id: task.task_id,
        partition,
        worker: worker_key.verifying_key().to_bytes(),
        result_commitment,
        signature: [0; 64],
    };
    claim.signature = worker_key
        .sign(&claim_signing_bytes_v3(task, &claim)?)
        .to_bytes();
    Ok(claim)
}

/// Authenticate a claim against the (already verified) task and recompute
/// its result. Rejects forged signatures, wrong or unassigned workers, and
/// manipulated results.
pub fn verify_claim_v3(
    task: &AiTaskV3,
    claim: &WorkClaimV3,
) -> Result<VerifiedWorkReceipt, String> {
    if claim.task_id != task.task_id {
        return Err("Claim refers to a different task".into());
    }
    let partition = task
        .partitions
        .get(claim.partition as usize)
        .ok_or("Claim names a nonexistent partition")?;
    if partition.worker != claim.worker {
        return Err("Claim is not from the worker assigned to this partition".into());
    }
    let worker =
        VerifyingKey::from_bytes(&claim.worker).map_err(|_| "Invalid worker Ed25519 identity")?;
    worker
        .verify_strict(
            &claim_signing_bytes_v3(task, claim)?,
            &Signature::from_bytes(&claim.signature),
        )
        .map_err(|_| "Invalid worker result signature")?;
    if claim.result_commitment != expected_result_v3(task, claim.partition)? {
        return Err("Claimed AI result differs from independent recomputation".into());
    }
    Ok(VerifiedWorkReceipt {
        puzzle_id: task.task_id,
        worker_id: claim.worker,
        range: WorkRange {
            start: partition.start,
            count: partition.count,
        },
        result_commitment: claim.result_commitment,
    })
}

fn works_from_receipts(task: &AiTaskV3, receipts: Vec<VerifiedWorkReceipt>) -> Vec<RewardWorkV1> {
    receipts
        .into_iter()
        .map(|receipt| RewardWorkV1 {
            receipt,
            puzzle_units: task.rows.len() as u64,
            deadline_height: task.deadline_height,
        })
        .collect()
}

/// Verify the task at the block height and every claim, in partition order.
pub fn verify_block_work_v3(
    block: &AiWorkBlockV3,
    rules: &V3Rules<'_>,
) -> Result<Vec<RewardWorkV1>, String> {
    verify_task_v3(&block.task, rules, block.header.height)?;
    if block.claims.len() != block.task.partitions.len() {
        return Err("Block must carry exactly one claim per partition".into());
    }
    let mut receipts = Vec::with_capacity(block.claims.len());
    for (index, claim) in block.claims.iter().enumerate() {
        if claim.partition as usize != index {
            return Err("Claims must be in partition order".into());
        }
        receipts.push(verify_claim_v3(&block.task, claim)?);
    }
    Ok(works_from_receipts(&block.task, receipts))
}

pub fn encode_task_v3(task: &AiTaskV3) -> Vec<u8> {
    let mut raw = task_body_bytes(task);
    raw.extend_from_slice(&task.task_id);
    raw.extend_from_slice(&task.authority_signature);
    raw
}

fn payload_bytes(task: &AiTaskV3, issuance: u64, claims: &[WorkClaimV3]) -> Vec<u8> {
    let mut raw = encode_task_v3(task);
    raw.extend_from_slice(&issuance.to_be_bytes());
    raw.extend_from_slice(&(claims.len() as u32).to_be_bytes());
    for claim in claims {
        raw.extend_from_slice(&claim.result_commitment);
        raw.extend_from_slice(&claim.signature);
    }
    raw
}

pub fn proof_root_v3(task: &AiTaskV3, issuance: u64, claims: &[WorkClaimV3]) -> [u8; 32] {
    let mut raw = ROOT_DOMAIN.to_vec();
    raw.extend_from_slice(&payload_bytes(task, issuance, claims));
    hash(&raw)
}

/// Deterministic header timestamp: honest proposers at any round build the
/// identical block for the same parent, task and claims.
pub fn block_timestamp_v3(parent_timestamp_ms: u64, task: &AiTaskV3) -> u64 {
    parent_timestamp_ms.saturating_add(1).max(task.created_ms)
}

pub fn encode_block_v3(block: &AiWorkBlockV3) -> Result<([u8; 32], Vec<u8>), String> {
    let header = canonical_header_v1(&block.header).map_err(|e| format!("{e:?}"))?;
    let id = hash(&header);
    let mut raw = BODY_DOMAIN_V3.to_vec();
    raw.extend_from_slice(&header);
    raw.extend_from_slice(&payload_bytes(&block.task, block.issuance, &block.claims));
    if raw.len() > MAX_BLOCK_BYTES {
        return Err("V3 block exceeds size limit".into());
    }
    Ok((id, raw))
}

struct Reader<'a> {
    raw: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(count).ok_or("Field overflow")?;
        let bytes = self.raw.get(self.at..end).ok_or("Truncated V3 encoding")?;
        self.at = end;
        Ok(bytes)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        self.take(N)?
            .try_into()
            .map_err(|_| "Malformed V3 field".into())
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_be_bytes(self.array()?))
    }
    fn remaining(&self) -> usize {
        self.raw.len() - self.at
    }
}

type TaskPayload = (Vec<i64>, Vec<Vec<i64>>, Option<QmlpModelV1>);

/// QMLP task payload: model body, u32 row count, int8 inputs.
fn read_qmlp_payload(reader: &mut Reader<'_>) -> Result<TaskPayload, String> {
    let (model, used) = QmlpModelV1::decode_body(&reader.raw[reader.at..])?;
    reader.take(used)?;
    let row_count = reader.u32()? as usize;
    if row_count == 0 || row_count > MAX_ROWS {
        return Err("V3 task dimensions exceed verifier limits".into());
    }
    let width = model.d_in as usize;
    let rows = reader
        .take(row_count * width)?
        .chunks_exact(width)
        .map(|row| row.iter().map(|byte| i64::from(*byte as i8)).collect())
        .collect();
    Ok((Vec::new(), rows, Some(model)))
}

fn read_task(reader: &mut Reader<'_>) -> Result<AiTaskV3, String> {
    let qmlp_tagged = reader.raw[reader.at..].starts_with(QMLP_TASK_TAG);
    if qmlp_tagged {
        reader.take(QMLP_TASK_TAG.len())?;
    }
    let sequence = reader.u64()?;
    let created_ms = reader.u64()?;
    let deadline_height = reader.u64()?;
    let fee_budget = reader.u64()?;
    let model_commitment = reader.array()?;
    let input_commitment = reader.array()?;
    let rules_commitment = reader.array()?;
    let (weights, rows, qmlp) = if qmlp_tagged {
        read_qmlp_payload(reader)?
    } else {
        read_linear_payload(reader)?
    };
    let partition_count = reader.u32()? as usize;
    if partition_count == 0
        || partition_count > MAX_PARTITIONS
        || reader.remaining() < partition_count * PARTITION_BYTES
    {
        return Err("Invalid V3 partition count".into());
    }
    let mut partitions = Vec::with_capacity(partition_count);
    for _ in 0..partition_count {
        partitions.push(PartitionV3 {
            start: reader.u64()?,
            count: reader.u64()?,
            worker: reader.array()?,
        });
    }
    let task_id = reader.array()?;
    let authority_signature = reader.array()?;
    Ok(AiTaskV3 {
        sequence,
        created_ms,
        deadline_height,
        fee_budget,
        model_commitment,
        input_commitment,
        rules_commitment,
        weights,
        rows,
        qmlp,
        partitions,
        task_id,
        authority_signature,
    })
}

/// Original integer linear payload: u32 rows, u32 width, i64 weights, rows.
fn read_linear_payload(reader: &mut Reader<'_>) -> Result<TaskPayload, String> {
    let row_count = reader.u32()? as usize;
    let width = reader.u32()? as usize;
    if row_count == 0 || row_count > MAX_ROWS || width == 0 || width > MAX_FEATURES {
        return Err("V3 task dimensions exceed verifier limits".into());
    }
    if reader.remaining() < (row_count + 1) * width * 8 {
        return Err("Truncated V3 task matrix".into());
    }
    let mut weights = Vec::with_capacity(width);
    for _ in 0..width {
        weights.push(reader.i64()?);
    }
    let mut rows = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        let mut row = Vec::with_capacity(width);
        for _ in 0..width {
            row.push(reader.i64()?);
        }
        rows.push(row);
    }
    Ok((weights, rows, None))
}

/// Exact decoder for a gossiped task; authenticity is checked separately
/// by `verify_task_v3`.
pub fn decode_task_v3(raw: &[u8]) -> Result<AiTaskV3, String> {
    if raw.len() > MAX_BLOCK_BYTES {
        return Err("Oversized V3 task".into());
    }
    let mut reader = Reader { raw, at: 0 };
    let task = read_task(&mut reader)?;
    if reader.remaining() != 0 || encode_task_v3(&task) != raw {
        return Err("Noncanonical V3 task encoding".into());
    }
    Ok(task)
}

/// Exact structural decoder for a stored or received block. Checks the
/// header hash, empty transaction root, proof root and canonical bytes.
/// Signatures, recomputation and settlement are checked by
/// `execute_block_v3`.
pub fn decode_block_v3(raw: &[u8], claimed_id: Option<&[u8; 32]>) -> Result<AiWorkBlockV3, String> {
    if !raw.starts_with(BODY_DOMAIN_V3) || raw.len() > MAX_BLOCK_BYTES {
        return Err("Wrong or oversized V3 block body".into());
    }
    let mut reader = Reader {
        raw,
        at: BODY_DOMAIN_V3.len(),
    };
    let header_raw: [u8; HEADER_BYTES] = reader.array()?;
    if let Some(expected) = claimed_id {
        if hash(&header_raw) != *expected {
            return Err("V3 block header hash mismatch".into());
        }
    }
    let header = decode_header_v1(&header_raw).map_err(|e| format!("{e:?}"))?;
    let task = read_task(&mut reader)?;
    let issuance = reader.u64()?;
    let count = reader.u32()? as usize;
    if count != task.partitions.len() || reader.remaining() != count * CLAIM_BYTES {
        return Err("Invalid V3 claim record length".into());
    }
    let mut claims = Vec::with_capacity(count);
    for (index, partition) in task.partitions.iter().enumerate() {
        claims.push(WorkClaimV3 {
            task_id: task.task_id,
            partition: index as u32,
            worker: partition.worker,
            result_commitment: reader.array()?,
            signature: reader.array()?,
        });
    }
    let block = AiWorkBlockV3 {
        header,
        task,
        issuance,
        claims,
    };
    let empty_root = transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?;
    if block.header.transactions_root != empty_root
        || block.header.puzzle_proofs_root
            != proof_root_v3(&block.task, block.issuance, &block.claims)
    {
        return Err("V3 header roots disagree with the block payload".into());
    }
    if encode_block_v3(&block)?.1 != raw {
        return Err("Noncanonical V3 block encoding".into());
    }
    Ok(block)
}

/// Proposer: assemble the deterministic block for `parent`, `task` and the
/// verified `claims` (any order), executing settlement to obtain the root.
pub fn build_block_v3(
    parent: &ParentContext<'_>,
    rules: &V3Rules<'_>,
    engine: &SettlementEngine,
    task: &AiTaskV3,
    claims: &[WorkClaimV3],
) -> Result<ExecutedBlockV3, String> {
    let height = parent
        .parent_height
        .checked_add(1)
        .ok_or("Height overflow")?;
    let mut ordered = claims.to_vec();
    ordered.sort_by_key(|claim| claim.partition);
    let issuance = rules
        .policy
        .scheduled_issuance(height, ledger_supply(parent.prior))?;
    let mut block = AiWorkBlockV3 {
        header: CommittedHeaderV1 {
            version: 1,
            chain_id: CHAIN_ID,
            height,
            parent_hash: parent.parent_id,
            transactions_root: transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?,
            puzzle_proofs_root: proof_root_v3(task, issuance, &ordered),
            state_root: [0; 32],
            timestamp_ms: block_timestamp_v3(parent.parent_timestamp_ms, task),
        },
        task: task.clone(),
        issuance,
        claims: ordered,
    };
    let works = verify_block_work_v3(&block, rules)?;
    let input = settlement_input(&block, &works);
    let settled = engine.settle(rules.policy, parent.prior, &input)?;
    block.header.state_root = ledger_state_root_v1(&settled.post).map_err(|e| format!("{e:?}"))?;
    let (id, bytes) = encode_block_v3(&block)?;
    Ok(ExecutedBlockV3 {
        id,
        bytes,
        block,
        settled,
    })
}

fn settlement_input<'a>(
    block: &AiWorkBlockV3,
    works: &'a [RewardWorkV1],
) -> IssuedSettlementV3<'a> {
    IssuedSettlementV3 {
        height: block.header.height,
        batch_id: reward_batch_id_v3(&block.task.task_id),
        fee_budget: block.task.fee_budget,
        issuance: block.issuance,
        works,
    }
}

/// Validator: independently verify and execute received block bytes on top
/// of `parent`. Every check is recomputed from the bytes; nothing from the
/// proposer is trusted.
pub fn execute_block_v3(
    parent: &ParentContext<'_>,
    rules: &V3Rules<'_>,
    engine: &SettlementEngine,
    raw: &[u8],
    claimed_id: Option<&[u8; 32]>,
) -> Result<ExecutedBlockV3, String> {
    let block = decode_block_v3(raw, claimed_id)?;
    let header = &block.header;
    if header.parent_hash != parent.parent_id
        || Some(header.height) != parent.parent_height.checked_add(1)
    {
        return Err("Block does not extend the finalized parent".into());
    }
    if header.timestamp_ms != block_timestamp_v3(parent.parent_timestamp_ms, &block.task) {
        return Err("Block timestamp is not the deterministic V3 value".into());
    }
    let works = verify_block_work_v3(&block, rules)?;
    let expected_issuance = rules
        .policy
        .scheduled_issuance(header.height, ledger_supply(parent.prior))?;
    if block.issuance != expected_issuance {
        return Err("Block issuance differs from the genesis-bound schedule".into());
    }
    let input = settlement_input(&block, &works);
    let settled = engine.settle(rules.policy, parent.prior, &input)?;
    if header.state_root != ledger_state_root_v1(&settled.post).map_err(|e| format!("{e:?}"))? {
        return Err("Block state root differs from independent settlement".into());
    }
    let (id, bytes) = encode_block_v3(&block)?;
    Ok(ExecutedBlockV3 {
        id,
        bytes,
        block,
        settled,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::issuance::tests::test_policy;
    use crate::storage::LocalAccount;

    pub fn authority() -> SigningKey {
        SigningKey::from_bytes(&[17; 32])
    }

    pub fn worker_keys() -> Vec<SigningKey> {
        (0..3u8)
            .map(|i| SigningKey::from_bytes(&[40 + i; 32]))
            .collect()
    }

    pub fn registered() -> BTreeSet<[u8; 32]> {
        worker_keys()
            .iter()
            .map(|key| key.verifying_key().to_bytes())
            .collect()
    }

    /// The repository's original deterministic workload, one row per worker.
    pub fn fixture_task(sequence: u64) -> AiTaskV3 {
        let weights = vec![2, 3];
        let rows = vec![vec![1, 2], vec![3, 4], vec![-2, 5]];
        let partitions = worker_keys()
            .iter()
            .enumerate()
            .map(|(i, key)| PartitionV3 {
                start: i as u64,
                count: 1,
                worker: key.verifying_key().to_bytes(),
            })
            .collect();
        sign_task_v3(
            AiTaskV3 {
                sequence,
                created_ms: 1_000 + sequence,
                deadline_height: sequence + 10,
                fee_budget: 30,
                model_commitment: model_commitment_v3(&weights).unwrap(),
                input_commitment: input_commitment_v3(&rows, 2).unwrap(),
                rules_commitment: rules_commitment_v1(),
                weights,
                rows,
                qmlp: None,
                partitions,
                task_id: [0; 32],
                authority_signature: [0; 64],
            },
            &authority(),
        )
    }

    /// The QMLP-V1 cross-language vector: 4 rows split 2/1/1 across workers.
    pub fn qmlp_task(sequence: u64) -> AiTaskV3 {
        let model = crate::qmlp::tests::fixture_model();
        let rows = crate::qmlp::tests::fixture_rows();
        let partitions = worker_keys()
            .iter()
            .zip([(0, 2), (2, 1), (3, 1)])
            .map(|(key, (start, count))| PartitionV3 {
                start,
                count,
                worker: key.verifying_key().to_bytes(),
            })
            .collect();
        sign_task_v3(
            AiTaskV3 {
                sequence,
                created_ms: 1_000 + sequence,
                deadline_height: sequence + 10,
                fee_budget: 30,
                model_commitment: model.commitment().unwrap(),
                input_commitment: input_commitment_v3(&rows, 2).unwrap(),
                rules_commitment: qmlp_rules_commitment_v1(),
                weights: Vec::new(),
                rows,
                qmlp: Some(model),
                partitions,
                task_id: [0; 32],
                authority_signature: [0; 64],
            },
            &authority(),
        )
    }

    pub fn honest_claims(task: &AiTaskV3) -> Vec<WorkClaimV3> {
        worker_keys()
            .iter()
            .enumerate()
            .map(|(i, key)| {
                let result = expected_result_v3(task, i as u32).unwrap();
                sign_claim_v3(task, i as u32, key, result).unwrap()
            })
            .collect()
    }

    pub fn genesis_ledger() -> LedgerState {
        let mut ledger = LedgerState {
            fee_pool: 600,
            ..LedgerState::default()
        };
        ledger.accounts.insert(
            authority().verifying_key().to_bytes(),
            LocalAccount {
                balance: 200,
                nonce: 0,
            },
        );
        ledger
    }

    fn with_rules<T>(f: impl FnOnce(&V3Rules<'_>) -> T) -> T {
        let key = authority().verifying_key().to_bytes();
        let workers = registered();
        let policy = test_policy();
        f(&V3Rules {
            task_authority: &key,
            workers: &workers,
            policy: &policy,
        })
    }

    fn parent(prior: &LedgerState) -> ParentContext<'_> {
        ParentContext {
            parent_id: [5; 32],
            parent_height: 0,
            parent_timestamp_ms: 1_000,
            prior,
        }
    }

    #[test]
    fn v3_commitments_match_v2_and_python_vectors() {
        let task = fixture_task(1);
        assert_eq!(
            task.model_commitment,
            crate::ai_proof_v2::model_commitment_v1(&[2, 3]).unwrap()
        );
        assert_eq!(
            task.input_commitment,
            crate::ai_proof_v2::input_commitment_v1(&[vec![1, 2], vec![3, 4], vec![-2, 5]], 2)
                .unwrap()
        );
        let range = WorkRange { start: 0, count: 3 };
        let whole = result_commitment_for_rows_v1(&[2, 3], &task.rows, range).unwrap();
        let hex: String = whole.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "a22d7dc4fe5191094e273e20c50df061c516b244a030227957c61ed4cab8761f"
        );
    }

    #[test]
    fn task_and_claims_round_trip_and_verify() {
        with_rules(|rules| {
            let task = fixture_task(1);
            verify_task_v3(&task, rules, 1).unwrap();
            assert_eq!(decode_task_v3(&encode_task_v3(&task)).unwrap(), task);
            for claim in honest_claims(&task) {
                verify_claim_v3(&task, &claim).unwrap();
            }
        });
    }

    #[test]
    fn rejects_forged_expired_overlapping_and_unauthorized_tasks() {
        with_rules(|rules| {
            let task = fixture_task(1);
            assert!(verify_task_v3(&task, rules, 2).is_err());
            let mut expired = task.clone();
            expired.deadline_height = 1;
            expired.sequence = 2;
            let expired = sign_task_v3(expired, &authority());
            assert!(verify_task_v3(&expired, rules, 2).is_err());
            let mut forged = task.clone();
            forged.authority_signature[0] ^= 1;
            assert!(verify_task_v3(&forged, rules, 1).is_err());
            let impostor = sign_task_v3(task.clone(), &SigningKey::from_bytes(&[99; 32]));
            assert!(verify_task_v3(&impostor, rules, 1).is_err());
            let mut overlap = task.clone();
            overlap.partitions[1].start = 0;
            let overlap = sign_task_v3(overlap, &authority());
            assert!(verify_task_v3(&overlap, rules, 1).is_err());
            let mut gap = task.clone();
            gap.partitions.pop();
            let gap = sign_task_v3(gap, &authority());
            assert!(verify_task_v3(&gap, rules, 1).is_err());
            let mut stranger = task.clone();
            let outsider = SigningKey::from_bytes(&[98; 32]).verifying_key().to_bytes();
            stranger.partitions[0].worker = outsider;
            let stranger = sign_task_v3(stranger, &authority());
            assert!(verify_task_v3(&stranger, rules, 1).is_err());
            let mut duplicate = task.clone();
            duplicate.partitions[1].worker = duplicate.partitions[0].worker;
            let duplicate = sign_task_v3(duplicate, &authority());
            assert!(verify_task_v3(&duplicate, rules, 1).is_err());
            let mut tampered = task.clone();
            tampered.rows[0][0] += 1;
            assert!(verify_task_v3(&tampered, rules, 1).is_err());
            let mut out_of_range = task;
            out_of_range.weights[0] = MAX_ABS_VALUE + 1;
            assert!(verify_task_v3(&out_of_range, rules, 1).is_err());
        });
    }

    #[test]
    fn rejects_forged_manipulated_and_misassigned_claims() {
        let task = fixture_task(1);
        let claims = honest_claims(&task);
        let mut manipulated = claims[0];
        manipulated.result_commitment[0] ^= 1;
        // Re-signed by the real worker: the signature is valid, the result is not.
        let resigned =
            sign_claim_v3(&task, 0, &worker_keys()[0], manipulated.result_commitment).unwrap();
        assert!(verify_claim_v3(&task, &resigned).is_err());
        let mut forged = claims[1];
        forged.signature[3] ^= 1;
        assert!(verify_claim_v3(&task, &forged).is_err());
        let wrong_worker = sign_claim_v3(
            &task,
            0,
            &worker_keys()[1],
            expected_result_v3(&task, 0).unwrap(),
        )
        .unwrap();
        assert!(verify_claim_v3(&task, &wrong_worker).is_err());
        let mut other_task = claims[2];
        other_task.task_id[0] ^= 1;
        assert!(verify_claim_v3(&task, &other_task).is_err());
        let mut missing = claims[2];
        missing.partition = 9;
        assert!(verify_claim_v3(&task, &missing).is_err());
    }

    #[test]
    fn build_and_execute_block_agree_and_settle_rewards() {
        with_rules(|rules| {
            let prior = genesis_ledger();
            let task = fixture_task(1);
            let mut claims = honest_claims(&task);
            claims.reverse();
            let engine = SettlementEngine::RustMirror;
            let built = build_block_v3(&parent(&prior), rules, &engine, &task, &claims).unwrap();
            let executed = execute_block_v3(
                &parent(&prior),
                rules,
                &engine,
                &built.bytes,
                Some(&built.id),
            )
            .unwrap();
            assert_eq!(executed, built);
            assert_eq!(built.block.issuance, 50);
            assert_eq!(built.settled.post.fee_pool, 570);
            let accounts = &built.settled.post.accounts;
            let mut balances: Vec<u64> = worker_keys()
                .iter()
                .map(|key| accounts[&key.verifying_key().to_bytes()].balance)
                .collect();
            balances.sort();
            assert_eq!(balances, vec![26, 27, 27]);
            assert_eq!(
                decode_block_v3(&built.bytes, Some(&built.id)).unwrap(),
                built.block
            );
        });
    }

    #[test]
    fn qmlp_task_round_trips_and_settles_rewards_by_rows() {
        with_rules(|rules| {
            let task = qmlp_task(1);
            verify_task_v3(&task, rules, 1).unwrap();
            let encoded = encode_task_v3(&task);
            assert!(encoded.starts_with(QMLP_TASK_TAG));
            assert!(!encode_task_v3(&fixture_task(1)).starts_with(QMLP_TASK_TAG));
            assert_eq!(decode_task_v3(&encoded).unwrap(), task);
            assert!(decode_task_v3(&encoded[..encoded.len() - 1]).is_err());
            let mut extended = encoded;
            extended.push(0);
            assert!(decode_task_v3(&extended).is_err());
            let claims = honest_claims(&task);
            for claim in &claims {
                verify_claim_v3(&task, claim).unwrap();
            }
            let prior = genesis_ledger();
            let engine = SettlementEngine::RustMirror;
            let built = build_block_v3(&parent(&prior), rules, &engine, &task, &claims).unwrap();
            let executed = execute_block_v3(
                &parent(&prior),
                rules,
                &engine,
                &built.bytes,
                Some(&built.id),
            )
            .unwrap();
            assert_eq!(executed, built);
            assert_eq!(executed.block.task, task);
            let accounts = &built.settled.post.accounts;
            let balances: Vec<u64> = worker_keys()
                .iter()
                .map(|key| accounts[&key.verifying_key().to_bytes()].balance)
                .collect();
            // Fee budget 30 plus issuance 50, split by assigned rows 2:1:1.
            assert_eq!(balances, vec![40, 20, 20]);
        });
    }

    #[test]
    fn qmlp_rejects_tampered_models_inputs_rules_and_results() {
        with_rules(|rules| {
            let task = qmlp_task(1);
            let resigned = |task: AiTaskV3| sign_task_v3(task, &authority());
            let mut weight = task.clone();
            weight.qmlp.as_mut().unwrap().w1[0] += 1;
            assert!(verify_task_v3(&resigned(weight), rules, 1).is_err());
            // An out-of-int8 input is rejected even with a matching commitment.
            let mut wide = task.clone();
            wide.rows[0][0] = 128;
            wide.input_commitment = input_commitment_v3(&wide.rows, 2).unwrap();
            assert!(verify_task_v3(&resigned(wide), rules, 1).is_err());
            let mut mixed = task.clone();
            mixed.weights = vec![2, 3];
            assert!(verify_task_v3(&resigned(mixed), rules, 1).is_err());
            let mut linear_rules = task.clone();
            linear_rules.rules_commitment = rules_commitment_v1();
            assert!(verify_task_v3(&resigned(linear_rules), rules, 1).is_err());
            let mut wrong = expected_result_v3(&task, 0).unwrap();
            wrong[0] ^= 1;
            let manipulated = sign_claim_v3(&task, 0, &worker_keys()[0], wrong).unwrap();
            assert!(verify_claim_v3(&task, &manipulated).is_err());
            // A claim for the linear workload over the same rows is not valid.
            let linear = fixture_task(1);
            let foreign = honest_claims(&linear)[0];
            assert!(verify_claim_v3(&task, &foreign).is_err());
        });
    }

    #[test]
    fn execute_rejects_tampered_bytes_wrong_parent_and_issuance() {
        with_rules(|rules| {
            let prior = genesis_ledger();
            let task = fixture_task(1);
            let engine = SettlementEngine::RustMirror;
            let claims = honest_claims(&task);
            let built = build_block_v3(&parent(&prior), rules, &engine, &task, &claims).unwrap();
            for offset in [
                0,
                BODY_DOMAIN_V3.len() + 10,
                BODY_DOMAIN_V3.len() + HEADER_BYTES + 3,
                built.bytes.len() - 1,
                built.bytes.len() - CLAIM_BYTES,
            ] {
                let mut corrupt = built.bytes.clone();
                corrupt[offset] ^= 1;
                assert!(
                    execute_block_v3(&parent(&prior), rules, &engine, &corrupt, None).is_err(),
                    "offset {offset}"
                );
            }
            let mut extended = built.bytes.clone();
            extended.push(0);
            assert!(execute_block_v3(&parent(&prior), rules, &engine, &extended, None).is_err());
            let mut other_parent = parent(&prior);
            other_parent.parent_id = [6; 32];
            assert!(execute_block_v3(&other_parent, rules, &engine, &built.bytes, None).is_err());
            // A proposer inflating issuance must re-root the header; still rejected.
            let mut inflated = built.block.clone();
            inflated.issuance += 1;
            inflated.header.puzzle_proofs_root =
                proof_root_v3(&inflated.task, inflated.issuance, &inflated.claims);
            let (_, bytes) = encode_block_v3(&inflated).unwrap();
            assert!(execute_block_v3(&parent(&prior), rules, &engine, &bytes, None).is_err());
            // A different state root (e.g. extra payout) is rejected.
            let mut stolen = built.block.clone();
            stolen.header.state_root[0] ^= 1;
            let (_, bytes) = encode_block_v3(&stolen).unwrap();
            assert!(execute_block_v3(&parent(&prior), rules, &engine, &bytes, None).is_err());
            // A replay of the settled task on its own successor is rejected.
            let replay_parent = ParentContext {
                parent_id: built.id,
                parent_height: 1,
                parent_timestamp_ms: built.block.header.timestamp_ms,
                prior: &built.settled.post,
            };
            assert!(execute_block_v3(&replay_parent, rules, &engine, &built.bytes, None).is_err());
        });
    }
}
