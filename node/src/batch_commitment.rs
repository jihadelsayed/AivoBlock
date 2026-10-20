//! Verified local multi-transfer blocks. Exactly the transactions executed
//! by Haskell are serialized and committed, in their execution order.
//! No validator consensus, AI proofs or authenticated genesis yet.

use sha2::{Digest, Sha256};

use crate::storage::ledger_state::LedgerState;
use crate::storage::{Column, KeyValueStore, RocksDbStore};
use crate::transaction_auth::{transfer_signing_bytes_v1, SignedTransferV1};
use crate::transaction_commitment::{
    canonical_header_v1, ledger_state_root_v1, signed_transaction_id_v1, transaction_root_v1,
    CommitmentError, CommittedHeaderV1,
};

const BODY_DOMAIN: &[u8] = b"AIVOBLOCK-TX-BATCH-BODY-V1\0";
pub const MAX_SIGNED_BLOCK_TRANSFERS: usize = 64;
const SIGNED_RECORD_SIZE: usize = 32 + 118 + 64;

/// This local V1 batch requires 1..=64 signed transfers and a placeholder
/// empty AI proof root. The signed records have fixed widths.
pub fn encode_verified_batch_block(
    header: &CommittedHeaderV1,
    transfers: &[SignedTransferV1],
    post_state: &LedgerState,
) -> Result<([u8; 32], Vec<u8>), CommitmentError> {
    if transfers.is_empty() || transfers.len() > MAX_SIGNED_BLOCK_TRANSFERS {
        return Err(CommitmentError::TooManyTransactions);
    }
    if header.puzzle_proofs_root != [0; 32] {
        return Err(CommitmentError::UnsupportedPuzzleProofs);
    }
    let identifiers = transfers
        .iter()
        .map(signed_transaction_id_v1)
        .collect::<Result<Vec<_>, _>>()?;
    let root = transaction_root_v1(&identifiers)?;
    if header.transactions_root != root {
        return Err(CommitmentError::WrongTransactionRoot);
    }
    if header.state_root != ledger_state_root_v1(post_state)? {
        return Err(CommitmentError::WrongStateRoot);
    }
    let header_bytes = canonical_header_v1(header)?;
    let block_id: [u8; 32] = Sha256::digest(header_bytes).into();

    let mut encoded =
        Vec::with_capacity(BODY_DOMAIN.len() + 178 + 4 + transfers.len() * SIGNED_RECORD_SIZE);
    encoded.extend_from_slice(BODY_DOMAIN);
    encoded.extend_from_slice(&header_bytes);
    encoded.extend_from_slice(&(transfers.len() as u32).to_be_bytes());
    for (tx, id) in transfers.iter().zip(&identifiers) {
        encoded.extend_from_slice(id);
        encoded.extend_from_slice(
            &transfer_signing_bytes_v1(tx).map_err(|_| CommitmentError::InvalidTransaction)?,
        );
        encoded.extend_from_slice(&tx.signature);
    }
    Ok((block_id, encoded))
}

/// Reconstruct the exact signed body and compare every byte, including
/// transaction order, IDs, signatures, count and header state commitment.
pub fn verify_stored_batch_block(
    stored: &[u8],
    block_id: &[u8; 32],
    header: &CommittedHeaderV1,
    transfers: &[SignedTransferV1],
    post_state: &LedgerState,
) -> Result<(), CommitmentError> {
    let (computed_id, expected) = encode_verified_batch_block(header, transfers, post_state)?;
    if computed_id != *block_id || stored != expected {
        return Err(CommitmentError::InvalidBlockBytes);
    }
    Ok(())
}

/// This wrapper is only safe when `post_state` came from successful local
/// Haskell signed-batch execution and its returned IDs/roots were checked.
/// RocksDB checks height and parent under its append lock and atomically
/// stores all balances, fees and block bytes in a synced WriteBatch.
pub fn commit_verified_batch_block(
    db: &RocksDbStore,
    header: &CommittedHeaderV1,
    transfers: &[SignedTransferV1],
    expected_previous: &LedgerState,
    haskell_post_state: &LedgerState,
) -> Result<[u8; 32], CommitmentError> {
    let on_disk = db
        .read_local_ledger_state()
        .map_err(|_| CommitmentError::StorageFailure)?;
    if on_disk.as_ref() != Some(expected_previous) {
        return Err(CommitmentError::WrongPriorState);
    }
    let (block_id, body) = encode_verified_batch_block(header, transfers, haskell_post_state)?;
    db.append_atomic_ledger_block(
        &block_id,
        &header.parent_hash,
        header.height,
        &body,
        haskell_post_state,
        None,
    )
    .map_err(|_| CommitmentError::StorageFailure)?;
    let stored = db
        .get(Column::Blocks, &block_id)
        .map_err(|_| CommitmentError::StorageFailure)?
        .ok_or(CommitmentError::StorageFailure)?;
    verify_stored_batch_block(&stored, &block_id, header, transfers, haskell_post_state)?;
    Ok(block_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::LocalAccount;
    use crate::transaction_auth::transfer_signing_bytes_v1;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed(amount: u64, fee: u64, nonce: u64) -> SignedTransferV1 {
        let signer = SigningKey::from_bytes(&[7u8; 32]);
        let sender = signer.verifying_key().to_bytes();
        let mut tx = SignedTransferV1 {
            chain_id: 1,
            version: 1,
            sender,
            public_key: sender,
            recipient: [2; 32],
            amount,
            fee,
            nonce,
            signature: [0; 64],
        };
        tx.signature = signer
            .sign(&transfer_signing_bytes_v1(&tx).unwrap())
            .to_bytes();
        tx
    }

    fn post() -> LedgerState {
        let mut state = LedgerState::default();
        state.fee_pool = 3;
        state.accounts.insert(
            signed(10, 2, 0).sender,
            LocalAccount {
                balance: 82,
                nonce: 2,
            },
        );
        state.accounts.insert(
            [2; 32],
            LocalAccount {
                balance: 15,
                nonce: 0,
            },
        );
        state
    }

    fn header(transfers: &[SignedTransferV1], state: &LedgerState) -> CommittedHeaderV1 {
        let ids = transfers
            .iter()
            .map(signed_transaction_id_v1)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        CommittedHeaderV1 {
            version: 1,
            chain_id: 1,
            height: 1,
            parent_hash: [8; 32],
            transactions_root: transaction_root_v1(&ids).unwrap(),
            puzzle_proofs_root: [0; 32],
            state_root: ledger_state_root_v1(state).unwrap(),
            timestamp_ms: 1001,
        }
    }

    #[test]
    fn rejects_duplicate_tampered_and_overlong_batches() {
        let a = signed(10, 2, 0);
        let b = signed(5, 1, 1);
        let ledger = post();
        let h = header(&[a, b], &ledger);
        assert_eq!(
            encode_verified_batch_block(&h, &[a, a], &ledger),
            Err(CommitmentError::DuplicateTransaction)
        );
        assert_eq!(
            encode_verified_batch_block(&h, &[], &ledger),
            Err(CommitmentError::TooManyTransactions)
        );
        assert_eq!(
            encode_verified_batch_block(&h, &[a; 65], &ledger),
            Err(CommitmentError::TooManyTransactions)
        );
        let tampered = SignedTransferV1 { amount: 6, ..b };
        assert_eq!(
            encode_verified_batch_block(&h, &[a, tampered], &ledger),
            Err(CommitmentError::InvalidTransaction)
        );
        assert_eq!(
            encode_verified_batch_block(&h, &[b, a], &ledger),
            Err(CommitmentError::WrongTransactionRoot)
        );
    }

    #[test]
    fn full_body_commits_to_ordered_signatures_and_state() {
        let transfers = [signed(10, 2, 0), signed(5, 1, 1)];
        let ledger = post();
        let header = header(&transfers, &ledger);
        let (block_id, body) = encode_verified_batch_block(&header, &transfers, &ledger).unwrap();
        verify_stored_batch_block(&body, &block_id, &header, &transfers, &ledger).unwrap();
        let mut broken = body.clone();
        *broken.last_mut().unwrap() ^= 1;
        assert_eq!(
            verify_stored_batch_block(&broken, &block_id, &header, &transfers, &ledger),
            Err(CommitmentError::InvalidBlockBytes)
        );
        let mut bad_header = header;
        bad_header.state_root = [0; 32];
        assert_eq!(
            encode_verified_batch_block(&bad_header, &transfers, &ledger),
            Err(CommitmentError::WrongStateRoot)
        );
        bad_header = header;
        bad_header.transactions_root = [0; 32];
        assert_eq!(
            encode_verified_batch_block(&bad_header, &transfers, &ledger),
            Err(CommitmentError::WrongTransactionRoot)
        );
    }
}
