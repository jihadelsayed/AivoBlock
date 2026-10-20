//! Experimental fee-funded AI work settlement blocks for a trusted local chain.
//! Receipt admission MUST follow signed local assignment and independent work
//! verification. This format makes the accepted inputs and state auditable;
//! it does NOT independently prove remote AI work or establish consensus.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

use crate::block_replay::decode_header_v1;
use crate::ipc::haskell::HaskellValidatorClient;
use crate::mining::receipts::{canonical_receipt_id_v1, ReceiptRegistry, VerifiedWorkReceipt};
use crate::mining::scheduler::WorkRange;
use crate::storage::ledger_state::{encode_ledger_state, LedgerState};
use crate::storage::rocksdb::SettlementBatch;
use crate::storage::{Column, KeyValueStore, RocksDbStore};
use crate::transaction_commitment::{
    canonical_header_v1, ledger_state_root_v1, transaction_root_v1, CommittedHeaderV1,
};

pub const REWARD_BODY_DOMAIN: &[u8] = b"AIVOBLOCK-REWARD-BLOCK-V1\0";
const REWARD_ROOT_DOMAIN: &[u8] = b"AIVOBLOCK-REWARD-ROOT-V1\0";
const MAX_REWARD_WORKS: usize = 64;
const RECORD_SIZE: usize = 32 + 32 + 32 + 32 + 8 + 8 + 8 + 8;
const HEADER_SIZE: usize = 178;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewardWorkV1 {
    pub receipt: VerifiedWorkReceipt,
    pub puzzle_units: u64,
    pub deadline_height: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DecodedRewardBlock {
    pub header: CommittedHeaderV1,
    pub batch_id: [u8; 32],
    pub budget: u64,
    pub works: Vec<RewardWorkV1>,
    pub receipt_ids: Vec<[u8; 32]>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn take<const N: usize>(source: &[u8], offset: &mut usize) -> Result<[u8; N], String> {
    let last = offset
        .checked_add(N)
        .ok_or("Reward block length overflow")?;
    let bytes = source.get(*offset..last).ok_or("Truncated reward block")?;
    *offset = last;
    bytes
        .try_into()
        .map_err(|_| "Invalid reward block field".into())
}

/// Verify canonical ordering, unique receipts, disjoint puzzle ranges,
/// and bounds. A valid ID does not authenticate the work: the caller must
/// pass values admitted by its trusted verifier, not network-supplied JSON.
pub fn checked_receipt_ids(works: &[RewardWorkV1], height: u64) -> Result<Vec<[u8; 32]>, String> {
    if works.is_empty() || works.len() > MAX_REWARD_WORKS || height == 0 {
        return Err("Reward block requires 1..64 works at a child height".into());
    }
    let mut ids = Vec::with_capacity(works.len());
    let mut seen = BTreeSet::new();
    let mut end_by_puzzle = BTreeMap::<[u8; 32], u64>::new();
    let mut previous_order: Option<([u8; 32], u64)> = None;
    for work in works {
        let r = &work.receipt;
        if r.puzzle_id == [0; 32]
            || r.worker_id == [0; 32]
            || r.result_commitment == [0; 32]
            || r.range.count == 0
            || work.puzzle_units == 0
            || height > work.deadline_height
        {
            return Err("Invalid work commitment, range or deadline".into());
        }
        let end = r
            .range
            .start
            .checked_add(r.range.count)
            .ok_or("Work range overflow")?;
        if end > work.puzzle_units {
            return Err("Work range exceeds puzzle units".into());
        }
        let order = (r.puzzle_id, r.range.start);
        if previous_order.is_some_and(|prior| prior >= order) {
            return Err("Reward receipts must be canonically ordered".into());
        }
        if end_by_puzzle
            .get(&r.puzzle_id)
            .is_some_and(|previous_end| r.range.start < *previous_end)
        {
            return Err("Duplicate or overlapping work range".into());
        }
        previous_order = Some(order);
        end_by_puzzle.insert(r.puzzle_id, end);
        let id =
            canonical_receipt_id_v1(1, r).map_err(|error| format!("Invalid work ID: {error:?}"))?;
        if !seen.insert(id) {
            return Err("Duplicate reward receipt ID".into());
        }
        ids.push(id);
    }
    Ok(ids)
}

fn reward_record_bytes(work: &RewardWorkV1, id: &[u8; 32]) -> Vec<u8> {
    let r = &work.receipt;
    let mut bytes = Vec::with_capacity(RECORD_SIZE);
    bytes.extend_from_slice(id);
    bytes.extend_from_slice(&r.puzzle_id);
    bytes.extend_from_slice(&r.worker_id);
    bytes.extend_from_slice(&r.result_commitment);
    bytes.extend_from_slice(&r.range.start.to_be_bytes());
    bytes.extend_from_slice(&r.range.count.to_be_bytes());
    bytes.extend_from_slice(&work.puzzle_units.to_be_bytes());
    bytes.extend_from_slice(&work.deadline_height.to_be_bytes());
    bytes
}

/// Commits to exact canonical receipt inputs AND the batch and fee budget.
pub fn reward_root_v1(
    batch_id: &[u8; 32],
    budget: u64,
    works: &[RewardWorkV1],
    height: u64,
) -> Result<[u8; 32], String> {
    if *batch_id == [0; 32] || budget == 0 {
        return Err("Missing reward batch or budget".into());
    }
    let ids = checked_receipt_ids(works, height)?;
    let mut digest = Sha256::new();
    digest.update(REWARD_ROOT_DOMAIN);
    digest.update(batch_id);
    digest.update(budget.to_be_bytes());
    digest.update((works.len() as u32).to_be_bytes());
    for (work, id) in works.iter().zip(&ids) {
        digest.update(reward_record_bytes(work, id));
    }
    Ok(digest.finalize().into())
}

pub fn encode_reward_block_v1(
    header: &CommittedHeaderV1,
    batch_id: &[u8; 32],
    budget: u64,
    works: &[RewardWorkV1],
    state: &LedgerState,
) -> Result<([u8; 32], Vec<u8>), String> {
    let ids = checked_receipt_ids(works, header.height)?;
    if header.transactions_root != transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?
        || header.puzzle_proofs_root != reward_root_v1(batch_id, budget, works, header.height)?
        || header.state_root != ledger_state_root_v1(state).map_err(|e| format!("{e:?}"))?
    {
        return Err("Reward block commitment mismatch".into());
    }
    let raw_header = canonical_header_v1(header).map_err(|e| format!("{e:?}"))?;
    let id: [u8; 32] = Sha256::digest(raw_header).into();
    let mut out = Vec::with_capacity(
        REWARD_BODY_DOMAIN.len() + HEADER_SIZE + 32 + 8 + 4 + works.len() * RECORD_SIZE,
    );
    out.extend_from_slice(REWARD_BODY_DOMAIN);
    out.extend_from_slice(&raw_header);
    out.extend_from_slice(batch_id);
    out.extend_from_slice(&budget.to_be_bytes());
    out.extend_from_slice(&(works.len() as u32).to_be_bytes());
    for (work, receipt_id) in works.iter().zip(&ids) {
        out.extend_from_slice(&reward_record_bytes(work, receipt_id));
    }
    Ok((id, out))
}

/// Reconstruct a reward block entirely from the bytes persisted on disk.
/// The local Haskell transition is replayed separately from trusted genesis.
pub fn decode_reward_block_v1(
    raw: &[u8],
    claimed_id: &[u8; 32],
) -> Result<DecodedRewardBlock, String> {
    if !raw.starts_with(REWARD_BODY_DOMAIN) {
        return Err("Wrong reward block domain".into());
    }
    let mut at = REWARD_BODY_DOMAIN.len();
    let header_bytes = take::<HEADER_SIZE>(raw, &mut at)?;
    let actual_id: [u8; 32] = Sha256::digest(header_bytes).into();
    if actual_id != *claimed_id {
        return Err("Reward block header hash mismatch".into());
    }
    let header = decode_header_v1(&header_bytes).map_err(|e| format!("{e:?}"))?;
    let batch_id = take::<32>(raw, &mut at)?;
    let budget = u64::from_be_bytes(take::<8>(raw, &mut at)?);
    let count = u32::from_be_bytes(take::<4>(raw, &mut at)?) as usize;
    if count == 0 || count > MAX_REWARD_WORKS {
        return Err("Unsupported reward receipt count".into());
    }
    if raw.len() != at + count * RECORD_SIZE {
        return Err("Reward block has truncated or trailing bytes".into());
    }
    let mut works = Vec::with_capacity(count);
    let mut claimed_receipts = Vec::with_capacity(count);
    for _ in 0..count {
        claimed_receipts.push(take::<32>(raw, &mut at)?);
        let puzzle_id = take::<32>(raw, &mut at)?;
        let worker_id = take::<32>(raw, &mut at)?;
        let result_commitment = take::<32>(raw, &mut at)?;
        let start = u64::from_be_bytes(take::<8>(raw, &mut at)?);
        let count = u64::from_be_bytes(take::<8>(raw, &mut at)?);
        let puzzle_units = u64::from_be_bytes(take::<8>(raw, &mut at)?);
        let deadline_height = u64::from_be_bytes(take::<8>(raw, &mut at)?);
        works.push(RewardWorkV1 {
            receipt: VerifiedWorkReceipt {
                puzzle_id,
                worker_id,
                result_commitment,
                range: WorkRange { start, count },
            },
            puzzle_units,
            deadline_height,
        });
    }
    let recomputed = checked_receipt_ids(&works, header.height)?;
    if recomputed != claimed_receipts
        || header.transactions_root != transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?
        || header.puzzle_proofs_root != reward_root_v1(&batch_id, budget, &works, header.height)?
    {
        return Err("Invalid stored reward receipt or proof-root commitment".into());
    }
    Ok(DecodedRewardBlock {
        header,
        batch_id,
        budget,
        works,
        receipt_ids: recomputed,
    })
}

/// Haskell starts from the exact prior typed ledger, recomputes receipt IDs,
/// checks eligibility and settles ONLY existing fees. The Rust reply parser
/// verifies the exact ids and state root before anything is written.
pub fn execute_reward_haskell(
    core: &HaskellValidatorClient,
    prior: &LedgerState,
    batch_id: &[u8; 32],
    budget: u64,
    works: &[RewardWorkV1],
    height: u64,
) -> Result<(LedgerState, Vec<u8>), String> {
    let ids = checked_receipt_ids(works, height)?;
    reward_root_v1(batch_id, budget, works, height)?;
    let previous = encode_ledger_state(prior).map_err(|e| format!("{e:?}"))?;
    let work_json = works.iter().zip(&ids).map(|(work, id)| {
        let r = &work.receipt;
        format!(
            r#"{{"puzzle_id":"{}","worker_id":"{}","result_commitment":"{}","puzzle_units":{},"deadline_height":{},"start":{},"count":{},"verified":true,"receipt_id":"{}"}}"#,
            hex(&r.puzzle_id),hex(&r.worker_id),hex(&r.result_commitment),
            work.puzzle_units,work.deadline_height,r.range.start,r.range.count,hex(id)
        )
    }).collect::<Vec<_>>().join(",");
    let request = format!(
        r#"{{"protocol_version":1,"type":"ledger_reward_block_fixture","chain_id":1,"height":{},"state_hex":"{}","budget":{},"batch_id":"{}","works":[{}]}}"#,
        height,
        hex(&previous),
        budget,
        hex(batch_id),
        work_json
    );
    let response = core
        .exchange(request.as_bytes())
        .map_err(|e| format!("Haskell reward IPC: {e:?}"))?;
    let state = crate::ipc::ledger_responses::decode_reward_block_reply(
        &response, batch_id, budget, &ids, prior,
    )?;
    Ok((state, response))
}

/// The caller MUST provide works admitted by a locally trusted verifier.
/// Persist the Haskell transition, canonical block, fee pool, receipts,
/// batch marker and height/tip under one synced RocksDB atomic write.
pub fn commit_reward_block(
    db: &RocksDbStore,
    core: &HaskellValidatorClient,
    verifier_registry: &ReceiptRegistry,
    expected_prior: &LedgerState,
    parent_id: &[u8; 32],
    height: u64,
    timestamp_ms: u64,
    batch_id: &[u8; 32],
    budget: u64,
    works: &[RewardWorkV1],
) -> Result<[u8; 32], String> {
    if works
        .iter()
        .any(|work| !verifier_registry.contains_verified(&work.receipt))
    {
        return Err("Reward claim has not passed the trusted local work verifier".into());
    }
    let on_disk = db
        .read_local_ledger_state()
        .map_err(|e| e.to_string())?
        .ok_or("Missing initialized ledger")?;
    if &on_disk != expected_prior {
        return Err("Stale or forged prior ledger".into());
    }
    let (post, response) =
        execute_reward_haskell(core, expected_prior, batch_id, budget, works, height)?;
    let header = CommittedHeaderV1 {
        version: 1,
        chain_id: 1,
        height,
        parent_hash: *parent_id,
        transactions_root: transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?,
        puzzle_proofs_root: reward_root_v1(batch_id, budget, works, height)?,
        state_root: ledger_state_root_v1(&post).map_err(|e| format!("{e:?}"))?,
        timestamp_ms,
    };
    let (id, body) = encode_reward_block_v1(&header, batch_id, budget, works, &post)?;
    let receipt_ids = checked_receipt_ids(works, height)?;
    let settlement = SettlementBatch {
        batch_id: *batch_id,
        receipt_ids: &receipt_ids,
        validated_response: &response,
    };
    db.append_atomic_ledger_block(&id, parent_id, height, &body, &post, Some(&settlement))
        .map_err(|e| format!("Atomic reward block failed: {e}"))?;
    let stored = db
        .get(Column::Blocks, &id)
        .map_err(|e| e.to_string())?
        .ok_or("Missing committed reward block")?;
    let decoded = decode_reward_block_v1(&stored, &id)?;
    if decoded.header != header || decoded.receipt_ids != receipt_ids {
        return Err("Committed reward block verification failed".into());
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalAccount;

    fn work(start: u64, count: u64) -> RewardWorkV1 {
        RewardWorkV1 {
            receipt: VerifiedWorkReceipt {
                puzzle_id: [1; 32],
                worker_id: [2; 32],
                range: WorkRange { start, count },
                result_commitment: [3; 32],
            },
            puzzle_units: 10,
            deadline_height: 100,
        }
    }
    fn state() -> LedgerState {
        let mut state = LedgerState::default();
        state.accounts.insert(
            [7; 32],
            LocalAccount {
                balance: 7,
                nonce: 0,
            },
        );
        state
    }
    fn header(works: &[RewardWorkV1], batch: &[u8; 32], budget: u64) -> CommittedHeaderV1 {
        CommittedHeaderV1 {
            version: 1,
            chain_id: 1,
            height: 1,
            parent_hash: [8; 32],
            transactions_root: transaction_root_v1(&[]).unwrap(),
            puzzle_proofs_root: reward_root_v1(batch, budget, works, 1).unwrap(),
            state_root: ledger_state_root_v1(&state()).unwrap(),
            timestamp_ms: 1001,
        }
    }
    #[test]
    fn rejects_duplicate_overlap_expiry_and_incorrect_order() {
        assert!(checked_receipt_ids(&[work(0, 2), work(0, 2)], 1).is_err());
        assert!(checked_receipt_ids(&[work(0, 2), work(1, 2)], 1).is_err());
        assert!(checked_receipt_ids(&[work(2, 1), work(0, 2)], 1).is_err());
        let mut expired = work(0, 2);
        expired.deadline_height = 0;
        assert!(checked_receipt_ids(&[expired], 1).is_err());
        let mut oversized = work(9, 2);
        oversized.puzzle_units = 10;
        assert!(checked_receipt_ids(&[oversized], 1).is_err());
    }
    #[test]
    fn roundtrip_detects_all_header_and_receipt_tampering() {
        let works = [work(0, 2), work(2, 1)];
        let batch = [9; 32];
        let header = header(&works, &batch, 3);
        let (id, bytes) = encode_reward_block_v1(&header, &batch, 3, &works, &state()).unwrap();
        assert_eq!(
            decode_reward_block_v1(&bytes, &id)
                .unwrap()
                .receipt_ids
                .len(),
            2
        );
        for offset in [
            0,
            REWARD_BODY_DOMAIN.len(),
            REWARD_BODY_DOMAIN.len() + HEADER_SIZE,
            bytes.len() - 1,
            bytes.len() - 30,
        ] {
            let mut tampered = bytes.clone();
            tampered[offset] ^= 1;
            assert!(decode_reward_block_v1(&tampered, &id).is_err());
        }
        let mut appended = bytes.clone();
        appended.push(0);
        assert!(decode_reward_block_v1(&appended, &id).is_err());
        assert!(decode_reward_block_v1(&bytes[..bytes.len() - 1], &id).is_err());
        assert!(encode_reward_block_v1(&header, &batch, 2, &works, &state()).is_err());
    }
}
