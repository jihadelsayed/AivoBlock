//! Re-execute a persisted signed batch from its on-disk bytes in the
//! trusted Haskell core, without caller-supplied transaction envelopes.
//! This is a read-only LOCAL consistency audit, not consensus/finality.

use sha2::{Digest, Sha256};

use crate::batch_pipeline::execute_signed_batch_haskell;
use crate::block_replay::{decode_header_v1, decode_signed_batch_v1};
use crate::ipc::haskell::HaskellValidatorClient;
use crate::storage::ledger_state::LedgerState;
use crate::storage::{Column, KeyValueStore, RocksDbStore};
use crate::transaction_commitment::ledger_state_root_v1;

fn id32(bytes: &[u8]) -> Result<[u8; 32], String> {
    bytes
        .try_into()
        .map_err(|_| "Wrong stored block ID length".to_owned())
}

/// Validate the current local tip against a caller-provided trusted prior
/// snapshot, the parent header, and Haskell's actual signed batch execution.
///
/// Important: `trusted_prior` must come from independently verified history.
/// This routine DOES NOT authenticate genesis, enumerate chain history, or
/// enforce consensus. It never mutates storage, even on failure.
pub fn replay_stored_tip(
    db: &RocksDbStore,
    core: &HaskellValidatorClient,
    trusted_prior: &LedgerState,
) -> Result<LedgerState, String> {
    let tip = db
        .get(Column::ChainState, b"tip")
        .map_err(|e| format!("Read tip: {e}"))?
        .ok_or("No stored tip")?;
    let block_id = id32(&tip)?;
    let stored = db
        .get(Column::Blocks, &block_id)
        .map_err(|e| format!("Read current block: {e}"))?
        .ok_or("Missing stored tip block")?;
    let decoded = decode_signed_batch_v1(&stored, &block_id)
        .map_err(|e| format!("Invalid persisted signed batch: {e:?}"))?;

    let recorded_height = db
        .get(Column::ChainState, b"height")
        .map_err(|e| format!("Read chain height: {e}"))?
        .ok_or("Missing chain height")?;
    let height_bytes: [u8; 8] = recorded_height
        .as_slice()
        .try_into()
        .map_err(|_| "Corrupt stored chain height")?;
    if u64::from_be_bytes(height_bytes) != decoded.header.height {
        return Err("Stored height disagrees with signed block header".into());
    }

    let parent_id = decoded.header.parent_hash;
    let parent_bytes = db
        .get(Column::Blocks, &parent_id)
        .map_err(|e| format!("Read predecessor: {e}"))?
        .ok_or("Missing predecessor block")?;
    let parent_header = if parent_bytes.len() == 178 {
        let hash: [u8; 32] = Sha256::digest(&parent_bytes).into();
        if hash != parent_id {
            return Err("Predecessor header hash mismatch".into());
        }
        decode_header_v1(&parent_bytes).map_err(|e| format!("Invalid predecessor header: {e:?}"))?
    } else {
        decode_signed_batch_v1(&parent_bytes, &parent_id)
            .map_err(|e| format!("Invalid predecessor batch: {e:?}"))?
            .header
    };
    if parent_header.height.checked_add(1) != Some(decoded.header.height) {
        return Err("Nonconsecutive parent height".into());
    }
    let expected_prior_root = ledger_state_root_v1(trusted_prior)
        .map_err(|e| format!("Invalid trusted predecessor snapshot: {e:?}"))?;
    if parent_header.state_root != expected_prior_root {
        return Err("Trusted predecessor does not match parent state root".into());
    }

    // Haskell independently re-verifies Ed25519 envelopes, sequential
    // nonces, funds and fees from the exact decoded stored transactions.
    let recalculated = execute_signed_batch_haskell(core, trusted_prior, &decoded.transactions)?;
    let resulting_root =
        ledger_state_root_v1(&recalculated).map_err(|e| format!("Invalid replay state: {e:?}"))?;
    if resulting_root != decoded.header.state_root {
        return Err("Block state root disagrees with Haskell replay".into());
    }
    let on_disk = db
        .read_local_ledger_state()
        .map_err(|e| format!("Read current ledger: {e}"))?
        .ok_or("Missing persisted ledger")?;
    if on_disk != recalculated {
        return Err("Persisted ledger differs from Haskell replay".into());
    }
    Ok(recalculated)
}
