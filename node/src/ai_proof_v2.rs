//! V2 on-chain deterministic AI-work evidence. All task inputs and worker
//! claims are inside the block; every validator recomputes each assigned
//! integer dot-product independently using only those bytes and an externally
//! pinned task-authority public key. This is NOT succinct PoW or consensus.

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::block_replay::decode_header_v1;
use crate::ipc::haskell::HaskellValidatorClient;
use crate::mining::receipts::{canonical_receipt_id_v1, VerifiedWorkReceipt};
use crate::mining::scheduler::WorkRange;
use crate::reward_block::{checked_receipt_ids, execute_reward_haskell, RewardWorkV1};
use crate::storage::ledger_state::LedgerState;
use crate::storage::rocksdb::SettlementBatch;
use crate::storage::{Column, KeyValueStore, RocksDbStore};
use crate::transaction_commitment::{
    canonical_header_v1, ledger_state_root_v1, transaction_root_v1, CommittedHeaderV1,
};

pub const BODY_DOMAIN_V2: &[u8] = b"AIVOBLOCK-AI-REWARD-PROOF-BLOCK-V2\0";
const ROOT_DOMAIN: &[u8] = b"AIVOBLOCK-AI-PROOF-ROOT-V2\0";
const TASK_DOMAIN: &[u8] = b"AIVOBLOCK-INFERENCE-TASK-ID-V2\0";
const TASK_SIGNATURE_DOMAIN: &[u8] = b"AIVOBLOCK-SIGNED-INFERENCE-TASK-V2\0";
const WORK_SIGNATURE_DOMAIN: &[u8] = b"AIVOBLOCK-SIGNED-AI-RESULT-V2\0";
const BATCH_DOMAIN: &[u8] = b"AIVOBLOCK-TASK-REWARD-BATCH-V2\0";
const MODEL_DOMAIN: &[u8] = b"AIVOBLOCK-INT-LINEAR-V1\0";
const INPUT_DOMAIN: &[u8] = b"AIVOBLOCK-INFERENCE-INPUT-V1\0";
const RULES_DOMAIN: &[u8] = b"AIVOBLOCK-INTEGER-INFERENCE-RULES-V1\0";
const RESULT_DOMAIN: &[u8] = b"AIVOBLOCK-INFERENCE-V1\0";
const MAX_ROWS: usize = 64;
const MAX_FEATURES: usize = 16;
const MAX_ABS: i64 = 1_000_000;
const MAX_RECEIPTS: usize = 64;
const CLAIM_BYTES: usize = 32 + 32 + 8 + 8 + 32 + 64;
const HEADER_BYTES: usize = 178;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceTaskV2 {
    pub puzzle_id: [u8; 32],
    pub model_commitment: [u8; 32],
    pub input_commitment: [u8; 32],
    pub rules_commitment: [u8; 32],
    pub deadline_height: u64,
    pub authorized_budget: u64,
    pub weights: Vec<i64>,
    pub rows: Vec<Vec<i64>>,
    pub authority_signature: [u8; 64],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedInferenceClaimV2 {
    pub receipt: VerifiedWorkReceipt,
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardProofV2 {
    pub batch_id: [u8; 32],
    pub task: InferenceTaskV2,
    pub claims: Vec<SignedInferenceClaimV2>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DecodedProofBlockV2 {
    pub header: CommittedHeaderV1,
    pub proof: RewardProofV2,
    pub works: Vec<RewardWorkV1>,
}

pub struct ProofBlockInput<'a> {
    pub expected_prior: &'a LedgerState,
    pub parent_id: &'a [u8; 32],
    pub height: u64,
    pub timestamp_ms: u64,
    pub proof: &'a RewardProofV2,
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn valid_data(task: &InferenceTaskV2) -> Result<(), String> {
    if task.rows.is_empty() || task.rows.len() > MAX_ROWS
        || task.weights.is_empty() || task.weights.len() > MAX_FEATURES
        || task.authorized_budget == 0 || task.deadline_height == 0
    {
        return Err("Unsupported task size or reward budget".into());
    }
    if task.weights.iter().any(|v| !(-MAX_ABS..=MAX_ABS).contains(v))
        || task.rows.iter().any(|row| {
            row.len() != task.weights.len()
                || row.iter().any(|v| !(-MAX_ABS..=MAX_ABS).contains(v))
        })
    {
        return Err("Invalid integer-inference values or dimensions".into());
    }
    Ok(())
}

pub fn model_commitment_v1(weights: &[i64]) -> Result<[u8; 32], String> {
    if weights.is_empty() || weights.len() > MAX_FEATURES
        || weights.iter().any(|v| !(-MAX_ABS..=MAX_ABS).contains(v))
    {
        return Err("Invalid model weights".into());
    }
    let mut bytes = MODEL_DOMAIN.to_vec();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&(weights.len() as u32).to_be_bytes());
    for value in weights {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    Ok(hash(&bytes))
}

pub fn input_commitment_v1(rows: &[Vec<i64>], width: usize) -> Result<[u8; 32], String> {
    if rows.is_empty() || rows.len() > MAX_ROWS || width == 0 || width > MAX_FEATURES
        || rows.iter().any(|row| {
            row.len() != width || row.iter().any(|v| !(-MAX_ABS..=MAX_ABS).contains(v))
        })
    {
        return Err("Invalid inference input rows".into());
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

pub fn rules_commitment_v1() -> [u8; 32] {
    hash(RULES_DOMAIN)
}

/// This task ID commits to the complete input and model hashes, deadline,
/// chain, row count and the exactly funded, authority-approved fee budget.
pub fn task_id_v2(task: &InferenceTaskV2) -> [u8; 32] {
    let mut bytes = TASK_DOMAIN.to_vec();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&task.model_commitment);
    bytes.extend_from_slice(&task.input_commitment);
    bytes.extend_from_slice(&task.rules_commitment);
    bytes.extend_from_slice(&(task.rows.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&task.deadline_height.to_be_bytes());
    bytes.extend_from_slice(&task.authorized_budget.to_be_bytes());
    hash(&bytes)
}

pub fn task_signing_bytes_v2(task: &InferenceTaskV2) -> Vec<u8> {
    let mut bytes = TASK_SIGNATURE_DOMAIN.to_vec();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&task.puzzle_id);
    bytes
}

/// A deterministic batch identity prohibits paying the same puzzle a second
/// time with different receipt partitions or different worker identities.
pub fn task_reward_batch_id_v2(puzzle_id: &[u8; 32]) -> [u8; 32] {
    let mut bytes = BATCH_DOMAIN.to_vec();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(puzzle_id);
    hash(&bytes)
}

pub fn worker_signing_bytes_v2(receipt: &VerifiedWorkReceipt) -> Vec<u8> {
    let mut bytes = WORK_SIGNATURE_DOMAIN.to_vec();
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&receipt.puzzle_id);
    bytes.extend_from_slice(&receipt.worker_id);
    bytes.extend_from_slice(&receipt.range.start.to_be_bytes());
    bytes.extend_from_slice(&receipt.range.count.to_be_bytes());
    bytes.extend_from_slice(&receipt.result_commitment);
    bytes
}

/// Matches Python executor.py byte-for-byte: signed decimal ASCII output
/// with big-endian length prefixes. Bounded operands keep i64 arithmetic safe.
pub fn expected_result_commitment_v1(
    task: &InferenceTaskV2,
    range: WorkRange,
) -> Result<[u8; 32], String> {
    valid_data(task)?;
    let start = usize::try_from(range.start).map_err(|_| "Range start overflow")?;
    let count = usize::try_from(range.count).map_err(|_| "Range count overflow")?;
    let end = start.checked_add(count).ok_or("Range overflow")?;
    if count == 0 || end > task.rows.len() {
        return Err("Claim exceeds signed task bounds".into());
    }
    let mut bytes = RESULT_DOMAIN.to_vec();
    bytes.extend_from_slice(&range.start.to_be_bytes());
    bytes.extend_from_slice(&range.count.to_be_bytes());
    for row in &task.rows[start..end] {
        // max 16 products of (1e6 * 1e6): at most 1.6e13.
        let sum: i64 = row.iter().zip(&task.weights).map(|(x, w)| x * w).sum();
        let ascii = sum.to_string();
        bytes.extend_from_slice(&(ascii.len() as u32).to_be_bytes());
        bytes.extend_from_slice(ascii.as_bytes());
    }
    Ok(hash(&bytes))
}

/// Full deterministic verification is independent of the original worker and
/// of Python/Haskell IPC. Only the externally trusted Ed25519 authority key
/// is policy input; no arbitrary signer shipped with a block is authoritative.
pub fn verify_proof_v2(
    proof: &RewardProofV2,
    height: u64,
    trusted_task_authority: &[u8; 32],
) -> Result<Vec<RewardWorkV1>, String> {
    let task = &proof.task;
    valid_data(task)?;
    if height == 0 || height > task.deadline_height
        || task.model_commitment != model_commitment_v1(&task.weights)?
        || task.input_commitment != input_commitment_v1(&task.rows, task.weights.len())?
        || task.rules_commitment != rules_commitment_v1()
        || task.puzzle_id != task_id_v2(task)
        || proof.batch_id != task_reward_batch_id_v2(&task.puzzle_id)
    {
        return Err("Task/budget identifier, commitments or deadline mismatch".into());
    }
    let issuer = VerifyingKey::from_bytes(trusted_task_authority)
        .map_err(|_| "Unrecognized task authority key")?;
    issuer.verify_strict(
        &task_signing_bytes_v2(task),
        &Signature::from_bytes(&task.authority_signature),
    ).map_err(|_| "Unauthorized task signature")?;

    if proof.claims.is_empty() || proof.claims.len() > MAX_RECEIPTS {
        return Err("Invalid number of signed inference claims".into());
    }
    let mut next_start = 0u64;
    let mut works = Vec::with_capacity(proof.claims.len());
    for claim in &proof.claims {
        let r = &claim.receipt;
        if r.puzzle_id != task.puzzle_id || r.range.start != next_start {
            return Err("Claims must cover the task exactly once, in order".into());
        }
        next_start = next_start.checked_add(r.range.count).ok_or("Work range overflow")?;
        if next_start > task.rows.len() as u64
            || r.result_commitment != expected_result_commitment_v1(task, r.range)?
        {
            return Err("Claimed AI result differs from independent recomputation".into());
        }
        let worker = VerifyingKey::from_bytes(&r.worker_id)
            .map_err(|_| "Invalid worker Ed25519 identity")?;
        worker.verify_strict(
            &worker_signing_bytes_v2(r),
            &Signature::from_bytes(&claim.signature),
        ).map_err(|_| "Invalid worker result signature")?;
        works.push(RewardWorkV1 {
            receipt: *r,
            puzzle_units: task.rows.len() as u64,
            deadline_height: task.deadline_height,
        });
    }
    if next_start != task.rows.len() as u64 {
        return Err("Partial task cannot receive a full funded reward".into());
    }
    checked_receipt_ids(&works, height)?;
    Ok(works)
}

fn serialize_payload(proof: &RewardProofV2) -> Vec<u8> {
    let task = &proof.task;
    let mut raw = Vec::new();
    raw.extend_from_slice(&proof.batch_id);
    raw.extend_from_slice(&task.puzzle_id);
    raw.extend_from_slice(&task.model_commitment);
    raw.extend_from_slice(&task.input_commitment);
    raw.extend_from_slice(&task.rules_commitment);
    raw.extend_from_slice(&task.deadline_height.to_be_bytes());
    raw.extend_from_slice(&task.authorized_budget.to_be_bytes());
    raw.extend_from_slice(&task.authority_signature);
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
    raw.extend_from_slice(&(proof.claims.len() as u32).to_be_bytes());
    for claim in &proof.claims {
        let id = canonical_receipt_id_v1(1, &claim.receipt)
            .expect("verified bounded V2 work receipt");
        raw.extend_from_slice(&id);
        raw.extend_from_slice(&claim.receipt.worker_id);
        raw.extend_from_slice(&claim.receipt.range.start.to_be_bytes());
        raw.extend_from_slice(&claim.receipt.range.count.to_be_bytes());
        raw.extend_from_slice(&claim.receipt.result_commitment);
        raw.extend_from_slice(&claim.signature);
    }
    raw
}

fn proof_root_from_payload(payload: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(ROOT_DOMAIN);
    h.update(payload);
    h.finalize().into()
}

fn take<const N: usize>(raw: &[u8], at: &mut usize) -> Result<[u8; N], String> {
    let end = at.checked_add(N).ok_or("Proof field overflow")?;
    let value = raw.get(*at..end).ok_or("Truncated AI proof block")?;
    *at = end;
    value.try_into().map_err(|_| "Malformed AI proof field".into())
}

fn u32_at(raw: &[u8], at: &mut usize) -> Result<u32, String> {
    Ok(u32::from_be_bytes(take::<4>(raw, at)?))
}

fn u64_at(raw: &[u8], at: &mut usize) -> Result<u64, String> {
    Ok(u64::from_be_bytes(take::<8>(raw, at)?))
}

fn i64_at(raw: &[u8], at: &mut usize) -> Result<i64, String> {
    Ok(i64::from_be_bytes(take::<8>(raw, at)?))
}

/// Exact V2 parser: bounded counts before allocation, fixed-width binary
/// fields, no trailing bytes, no duplicate/overlap, and all signatures plus
/// committed model/data/results independently verified before returning.
pub fn decode_proof_block_v2(
    raw: &[u8],
    claimed_id: &[u8; 32],
    trusted_task_authority: &[u8; 32],
) -> Result<DecodedProofBlockV2, String> {
    if !raw.starts_with(BODY_DOMAIN_V2) || raw.len() > 24 * 1024 {
        return Err("Wrong or oversized V2 AI proof body".into());
    }
    let mut at = BODY_DOMAIN_V2.len();
    let header_raw = take::<HEADER_BYTES>(raw, &mut at)?;
    if hash(&header_raw) != *claimed_id {
        return Err("Invalid AI reward block hash".into());
    }
    let header = decode_header_v1(&header_raw).map_err(|e| format!("{e:?}"))?;
    let payload_at = at;
    let batch_id = take::<32>(raw, &mut at)?;
    let puzzle_id = take::<32>(raw, &mut at)?;
    let model_commitment = take::<32>(raw, &mut at)?;
    let input_commitment = take::<32>(raw, &mut at)?;
    let rules_commitment = take::<32>(raw, &mut at)?;
    let deadline_height = u64_at(raw, &mut at)?;
    let authorized_budget = u64_at(raw, &mut at)?;
    let authority_signature = take::<64>(raw, &mut at)?;
    let rows_count = u32_at(raw, &mut at)? as usize;
    let width = u32_at(raw, &mut at)? as usize;
    if rows_count == 0 || rows_count > MAX_ROWS || width == 0 || width > MAX_FEATURES {
        return Err("V2 model dimensions exceed verifier limits".into());
    }
    let mut weights = Vec::with_capacity(width);
    for _ in 0..width {
        weights.push(i64_at(raw, &mut at)?);
    }
    let mut rows = Vec::with_capacity(rows_count);
    for _ in 0..rows_count {
        let mut row = Vec::with_capacity(width);
        for _ in 0..width {
            row.push(i64_at(raw, &mut at)?);
        }
        rows.push(row);
    }
    let count = u32_at(raw, &mut at)? as usize;
    if count == 0 || count > MAX_RECEIPTS
        || raw.len().saturating_sub(at) != count * CLAIM_BYTES
    {
        return Err("Invalid V2 receipt record length".into());
    }
    let mut claims = Vec::with_capacity(count);
    for _ in 0..count {
        let supplied_id = take::<32>(raw, &mut at)?;
        let worker_id = take::<32>(raw, &mut at)?;
        let start = u64_at(raw, &mut at)?;
        let count = u64_at(raw, &mut at)?;
        let result_commitment = take::<32>(raw, &mut at)?;
        let signature = take::<64>(raw, &mut at)?;
        let receipt = VerifiedWorkReceipt {
            puzzle_id,
            worker_id,
            range: WorkRange { start, count },
            result_commitment,
        };
        let calculated_id = canonical_receipt_id_v1(1, &receipt)
            .map_err(|e| format!("Malformed canonical receipt: {e:?}"))?;
        if supplied_id != calculated_id {
            return Err("Forged V2 receipt identity".into());
        }
        claims.push(SignedInferenceClaimV2 { receipt, signature });
    }
    if at != raw.len() || header.transactions_root != transaction_root_v1(&[])
        .map_err(|e| format!("{e:?}"))?
        || header.puzzle_proofs_root != proof_root_from_payload(&raw[payload_at..])
    {
        return Err("Wrong V2 proof-root commitment or noncanonical bytes".into());
    }
    let proof = RewardProofV2 {
        batch_id,
        task: InferenceTaskV2 {
            puzzle_id, model_commitment, input_commitment, rules_commitment,
            deadline_height, authorized_budget, weights, rows, authority_signature,
        },
        claims,
    };
    let works = verify_proof_v2(&proof, header.height, trusted_task_authority)?;
    if serialize_payload(&proof).as_slice() != &raw[payload_at..] {
        return Err("Noncanonical V2 AI proof body".into());
    }
    Ok(DecodedProofBlockV2 { header, proof, works })
}

pub fn encode_proof_block_v2(
    header: &CommittedHeaderV1,
    proof: &RewardProofV2,
    state: &LedgerState,
    trusted_task_authority: &[u8; 32],
) -> Result<([u8; 32], Vec<u8>), String> {
    verify_proof_v2(proof, header.height, trusted_task_authority)?;
    let payload = serialize_payload(proof);
    if header.transactions_root != transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?
        || header.puzzle_proofs_root != proof_root_from_payload(&payload)
        || header.state_root != ledger_state_root_v1(state).map_err(|e| format!("{e:?}"))?
    {
        return Err("V2 block commitments disagree with verified data".into());
    }
    let bytes = canonical_header_v1(header).map_err(|e| format!("{e:?}"))?;
    let id = hash(&bytes);
    let mut out = BODY_DOMAIN_V2.to_vec();
    out.extend_from_slice(&bytes);
    out.extend_from_slice(&payload);
    Ok((id, out))
}

/// New blocks require independently proven worker results and task authority.
/// The Haskell settlement remains the deterministic fee/balance engine,
/// but is never called until Rust has recomputed every result from the
/// authenticated model and all inputs committed in the block itself.
pub fn commit_proof_block_v2(
    db: &RocksDbStore,
    core: &HaskellValidatorClient,
    trusted_task_authority: &[u8; 32],
    input: ProofBlockInput<'_>,
) -> Result<[u8; 32], String> {
    let ProofBlockInput {
        expected_prior, parent_id, height, timestamp_ms, proof,
    } = input;
    let works = verify_proof_v2(proof, height, trusted_task_authority)?;
    if db.read_local_ledger_state().map_err(|e| e.to_string())?
        != Some(expected_prior.clone())
    {
        return Err("Stale or uninitialized prior ledger".into());
    }
    let (post, response) = execute_reward_haskell(
        core, expected_prior, &proof.batch_id, proof.task.authorized_budget,
        &works, height,
    )?;
    let header = CommittedHeaderV1 {
        version: 1, chain_id: 1, height, parent_hash: *parent_id,
        transactions_root: transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?,
        puzzle_proofs_root: proof_root_from_payload(&serialize_payload(proof)),
        state_root: ledger_state_root_v1(&post).map_err(|e| format!("{e:?}"))?,
        timestamp_ms,
    };
    let (id, raw) = encode_proof_block_v2(
        &header, proof, &post, trusted_task_authority
    )?;
    let receipt_ids = checked_receipt_ids(&works, height)?;
    let settlement = SettlementBatch {
        batch_id: proof.batch_id,
        receipt_ids: &receipt_ids,
        validated_response: &response,
    };
    db.append_atomic_ledger_block(
        &id, parent_id, height, &raw, &post, Some(&settlement),
    ).map_err(|e| format!("Persist independently validated AI proof: {e}"))?;
    let stored = db.get(Column::Blocks, &id).map_err(|e| e.to_string())?
        .ok_or("Missing stored V2 proof block")?;
    if decode_proof_block_v2(&stored, &id, trusted_task_authority)?.header != header {
        return Err("Stored AI proof block differs from verified data".into());
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    pub fn fixture() -> (RewardProofV2, [u8; 32]) {
        let authority = SigningKey::from_bytes(&[17; 32]);
        let mut task = InferenceTaskV2 {
            puzzle_id: [0; 32],
            model_commitment: model_commitment_v1(&[2, 3]).unwrap(),
            input_commitment: input_commitment_v1(
                &[vec![1, 2], vec![3, 4], vec![-2, 5]], 2,
            ).unwrap(),
            rules_commitment: rules_commitment_v1(),
            deadline_height: 100,
            authorized_budget: 3,
            weights: vec![2, 3],
            rows: vec![vec![1, 2], vec![3, 4], vec![-2, 5]],
            authority_signature: [0; 64],
        };
        task.puzzle_id = task_id_v2(&task);
        task.authority_signature = authority.sign(&task_signing_bytes_v2(&task)).to_bytes();
        let mut claims = Vec::new();
        for (seed, start, count) in [(5u8, 0, 2), (6u8, 2, 1)] {
            let key = SigningKey::from_bytes(&[seed; 32]);
            let receipt = VerifiedWorkReceipt {
                puzzle_id: task.puzzle_id,
                worker_id: key.verifying_key().to_bytes(),
                range: WorkRange { start, count },
                result_commitment: expected_result_commitment_v1(
                    &task, WorkRange { start, count },
                ).unwrap(),
            };
            claims.push(SignedInferenceClaimV2 {
                signature: key.sign(&worker_signing_bytes_v2(&receipt)).to_bytes(),
                receipt,
            });
        }
        (RewardProofV2 {
            batch_id: task_reward_batch_id_v2(&task.puzzle_id), task, claims,
        }, authority.verifying_key().to_bytes())
    }

    #[test]
    fn canonical_model_input_and_python_result_vectors_match() {
        let (proof, _) = fixture();
        let h = |bytes: &[u8; 32]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(h(&proof.task.model_commitment),
            "841c1f0051800864f7189935a818c964893e1b96fe1c610136c23a1284a254bc");
        assert_eq!(h(&proof.task.input_commitment),
            "548121e870e795733aaabbca753b290431797562a55a1301fcf6113f568c0e3d");
        assert_eq!(h(&proof.task.rules_commitment),
            "d80998d00fe21a0127cc7edf3915cd5de202d702b919259bbf74aa19aad63902");
        assert_eq!(h(&proof.claims[0].receipt.result_commitment),
            "edf2eb1a8f906b42910faa5dff56d8e6a3a54ece44d5369f62ead8f44acdd982");
    }

    #[test]
    fn proves_only_pinned_authority_signed_and_fully_recomputed_work() {
        let (proof, authority) = fixture();
        assert_eq!(verify_proof_v2(&proof, 1, &authority).unwrap().len(), 2);
        assert!(verify_proof_v2(&proof, 1, &[0; 32]).is_err());
        let mut altered = proof.clone();
        altered.task.rows[0][0] ^= 1;
        assert!(verify_proof_v2(&altered, 1, &authority).is_err());
        altered = proof.clone();
        altered.task.weights[0] ^= 1;
        assert!(verify_proof_v2(&altered, 1, &authority).is_err());
        altered = proof.clone();
        altered.task.authorized_budget = 4;
        assert!(verify_proof_v2(&altered, 1, &authority).is_err());
        altered = proof.clone();
        altered.task.authority_signature[0] ^= 1;
        assert!(verify_proof_v2(&altered, 1, &authority).is_err());
        altered = proof.clone();
        altered.claims[0].signature[0] ^= 1;
        assert!(verify_proof_v2(&altered, 1, &authority).is_err());
        altered = proof.clone();
        altered.claims[0].receipt.result_commitment[0] ^= 1;
        assert!(verify_proof_v2(&altered, 1, &authority).is_err());
        altered = proof.clone();
        altered.claims.swap(0, 1);
        assert!(verify_proof_v2(&altered, 1, &authority).is_err());
        altered = proof.clone();
        altered.claims.pop();
        assert!(verify_proof_v2(&altered, 1, &authority).is_err());
        altered = proof.clone();
        altered.batch_id = [7; 32];
        assert!(verify_proof_v2(&altered, 1, &authority).is_err());
        assert!(verify_proof_v2(&proof, 101, &authority).is_err());
    }

    #[test]
    fn stored_proof_roundtrip_and_mutations_fail() {
        let (proof, authority) = fixture();
        let state = LedgerState::default();
        let payload = serialize_payload(&proof);
        let header = CommittedHeaderV1 {
            version: 1, chain_id: 1, height: 1, parent_hash: [7; 32],
            transactions_root: transaction_root_v1(&[]).unwrap(),
            puzzle_proofs_root: proof_root_from_payload(&payload),
            state_root: ledger_state_root_v1(&state).unwrap(),
            timestamp_ms: 1001,
        };
        let (id, raw) = encode_proof_block_v2(&header, &proof, &state, &authority).unwrap();
        assert_eq!(decode_proof_block_v2(&raw, &id, &authority).unwrap().proof, proof);
        for offset in [0, BODY_DOMAIN_V2.len(), BODY_DOMAIN_V2.len() + HEADER_BYTES,
            raw.len() - 1, raw.len() - CLAIM_BYTES] {
            let mut corrupt = raw.clone();
            corrupt[offset] ^= 1;
            assert!(decode_proof_block_v2(&corrupt, &id, &authority).is_err());
        }
        let mut extended = raw.clone();
        extended.push(0);
        assert!(decode_proof_block_v2(&extended, &id, &authority).is_err());
        assert!(decode_proof_block_v2(&raw[..raw.len()-1], &id, &authority).is_err());
    }
}
