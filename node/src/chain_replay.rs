//! Read-only full local-chain replay, starting from an explicitly trusted
//! genesis ID and ledger. This is an audit, NOT decentralized consensus.
//! It never modifies RocksDB, accepts network blocks or resolves forks.

use std::collections::HashSet;

use sha2::{Digest, Sha256};

use crate::batch_pipeline::execute_signed_batch_haskell;
use crate::block_replay::{decode_header_v1, decode_signed_batch_v1, DecodedSignedBatch};
use crate::ipc::haskell::HaskellValidatorClient;
use crate::reward_block::{
    decode_reward_block_v1, execute_reward_haskell, DecodedRewardBlock, REWARD_BODY_DOMAIN,
};
use crate::storage::ledger_state::LedgerState;
use crate::storage::{Column, KeyValueStore, RocksDbStore};
use crate::transaction_commitment::{ledger_state_root_v1, transaction_root_v1};

// Local bounded audit, not a network protocol height cap. The bounds avoid
// untrusted metadata allocating unbounded memory or causing endless replay.
pub const MAX_AUDIT_BLOCKS: usize = 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct ChainReplayReport {
    pub ledger: LedgerState,
    pub signed_blocks: usize,
    pub signed_transfers: usize,
    pub reward_blocks: usize,
    pub settled_receipts: usize,
    pub tip_id: [u8; 32],
}

enum ReplayBlock {
    Signed(DecodedSignedBatch),
    Reward(DecodedRewardBlock),
}

impl ReplayBlock {
    fn header(&self) -> &crate::transaction_commitment::CommittedHeaderV1 {
        match self {
            Self::Signed(block) => &block.header,
            Self::Reward(block) => &block.header,
        }
    }
}

fn id32(source: &[u8]) -> Result<[u8; 32], String> {
    source
        .try_into()
        .map_err(|_| "Stored block ID must be 32 bytes".to_owned())
}

/// Genesis bytes are a header only, with an empty transaction Merkle root.
/// The genesis ID *and ledger snapshot* must come from a trusted external
/// checkpoint. A mere internally consistent DB is not authoritative.
fn verify_trusted_genesis(
    stored: &[u8],
    expected_id: &[u8; 32],
    trusted_state: &LedgerState,
) -> Result<(), String> {
    let actual_id: [u8; 32] = Sha256::digest(stored).into();
    if &actual_id != expected_id {
        return Err("Genesis block ID differs from trusted checkpoint".into());
    }
    let header = decode_header_v1(stored)
        .map_err(|error| format!("Invalid stored genesis header: {error:?}"))?;
    let empty_root =
        transaction_root_v1(&[]).map_err(|error| format!("Empty transaction root: {error:?}"))?;
    if header.height != 0
        || header.parent_hash != [0; 32]
        || header.puzzle_proofs_root != [0; 32]
        || header.transactions_root != empty_root
        || header.state_root
            != ledger_state_root_v1(trusted_state)
                .map_err(|error| format!("Trusted genesis snapshot: {error:?}"))?
    {
        return Err("Genesis header disagrees with trusted checkpoint state".into());
    }
    Ok(())
}

/// Rebuild the entire local canonical branch from an externally trusted
/// genesis anchor using only blocks read from RocksDB. Every signed child
/// batch is decoded from disk, Ed25519-verified and then executed by Haskell
/// against the ledger computed for its actual parent.
///
/// Fail closed on a missing/cyclic parent, nonconsecutive height, modified
/// ancestor, forged state root, invalid signature, overspend or persisted
/// tip state different from Haskell replay. Never silently skip history.
pub fn replay_local_chain(
    db: &RocksDbStore,
    core: &HaskellValidatorClient,
    trusted_genesis_id: &[u8; 32],
    trusted_genesis_state: &LedgerState,
) -> Result<ChainReplayReport, String> {
    let tip = db
        .get(Column::ChainState, b"tip")
        .map_err(|error| format!("Read chain tip: {error}"))?
        .ok_or("Missing chain tip")?;
    let tip_id = id32(&tip)?;
    let height_bytes = db
        .get(Column::ChainState, b"height")
        .map_err(|error| format!("Read chain height: {error}"))?
        .ok_or("Missing chain height")?;
    let height_raw: [u8; 8] = height_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Malformed chain height")?;
    let height = u64::from_be_bytes(height_raw);
    if height > MAX_AUDIT_BLOCKS as u64 {
        return Err("Local replay exceeds configured safety bound".into());
    }

    let mut current = tip_id;
    let mut expected_height = height;
    let mut seen = HashSet::new();
    let mut reverse_children: Vec<ReplayBlock> = Vec::new();
    loop {
        if !seen.insert(current) {
            return Err("Cycle in stored chain ancestry".into());
        }
        let raw = db
            .get(Column::Blocks, &current)
            .map_err(|error| format!("Read ancestor: {error}"))?
            .ok_or("Missing ancestor block")?;
        if current == *trusted_genesis_id {
            if expected_height != 0 {
                return Err("Trusted genesis reached at nonzero height".into());
            }
            verify_trusted_genesis(&raw, trusted_genesis_id, trusted_genesis_state)?;
            break;
        }
        if expected_height == 0 || reverse_children.len() >= MAX_AUDIT_BLOCKS {
            return Err("Missing trusted genesis at height zero".into());
        }
        let decoded = if raw.starts_with(REWARD_BODY_DOMAIN) {
            ReplayBlock::Reward(decode_reward_block_v1(&raw, &current)
                .map_err(|error| format!("Invalid reward ancestor: {error}"))?)
        } else {
            ReplayBlock::Signed(decode_signed_batch_v1(&raw, &current)
                .map_err(|error| format!("Invalid signed ancestor: {error:?}"))?)
        };
        if decoded.header().height != expected_height || decoded.header().parent_hash == [0; 32] {
            return Err("Invalid signed/reward ancestor height or parent".into());
        }
        current = decoded.header().parent_hash;
        reverse_children.push(decoded);
        expected_height -= 1;
    }

    let mut ledger = trusted_genesis_state.clone();
    let mut total_transfers = 0usize;
    let mut signed_blocks = 0usize;
    let mut reward_blocks = 0usize;
    let mut settled_receipts = 0usize;
    for block in reverse_children.iter().rev() {
        let next = match block {
            ReplayBlock::Signed(signed) => {
                let state = execute_signed_batch_haskell(core, &ledger, &signed.transactions)?;
                total_transfers += signed.transactions.len();
                signed_blocks += 1;
                state
            }
            ReplayBlock::Reward(reward) => {
                // The stored caller-attested work is NOT independent proof of
                // AI computation. Haskell independently replays the fee-funded
                // effect, canonical receipt identities and replay markers.
                let (state, _) = execute_reward_haskell(
                    core, &ledger, &reward.batch_id, reward.budget,
                    &reward.works, reward.header.height,
                )?;
                reward_blocks += 1;
                settled_receipts += reward.works.len();
                state
            }
        };
        let computed_root = ledger_state_root_v1(&next)
            .map_err(|error| format!("Invalid replay ledger: {error:?}"))?;
        if computed_root != block.header().state_root {
            return Err("Ancestor state commitment differs from Haskell execution".into());
        }
        ledger = next;
    }

    let stored_tip_ledger = db
        .read_local_ledger_state()
        .map_err(|error| format!("Read persisted tip snapshot: {error}"))?
        .ok_or("Missing persisted tip snapshot")?;
    if stored_tip_ledger != ledger {
        return Err("Persisted tip ledger differs from entire Haskell replay".into());
    }
    Ok(ChainReplayReport {
        ledger,
        signed_blocks,
        signed_transfers: total_transfers,
        reward_blocks,
        settled_receipts,
        tip_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalAccount;
    use crate::transaction_commitment::{canonical_header_v1, CommittedHeaderV1};

    fn trusted_genesis() -> (LedgerState, [u8; 178], [u8; 32]) {
        let mut ledger = LedgerState::default();
        ledger.accounts.insert(
            [3; 32],
            LocalAccount {
                balance: 100,
                nonce: 0,
            },
        );
        let header = CommittedHeaderV1 {
            version: 1,
            chain_id: 1,
            height: 0,
            parent_hash: [0; 32],
            transactions_root: transaction_root_v1(&[]).unwrap(),
            puzzle_proofs_root: [0; 32],
            state_root: ledger_state_root_v1(&ledger).unwrap(),
            timestamp_ms: 1000,
        };
        let encoded = canonical_header_v1(&header).unwrap();
        let id: [u8; 32] = Sha256::digest(encoded).into();
        (ledger, encoded, id)
    }

    #[test]
    fn trusted_genesis_requires_exact_hash_empty_root_and_state() {
        let (ledger, encoded, id) = trusted_genesis();
        verify_trusted_genesis(&encoded, &id, &ledger).unwrap();
        assert!(verify_trusted_genesis(&encoded, &[7; 32], &ledger).is_err());
        let mut wrong_state = ledger;
        wrong_state.fee_pool = 1;
        assert!(verify_trusted_genesis(&encoded, &id, &wrong_state).is_err());
        let mut altered = encoded;
        altered[42] = 1;
        assert!(verify_trusted_genesis(&altered, &id, &wrong_state).is_err());
        assert!(verify_trusted_genesis(&encoded[..177], &id, &wrong_state).is_err());
    }
}
