//! Candidate V1 signed transaction IDs, ordered Merkle commitments and
//! canonical block representation. This is local protocol validation, not
//! validator consensus, AI-proof validation or authenticated genesis.

use sha2::{Digest, Sha256};

use crate::storage::ledger_state::{encode_ledger_state, LedgerState};
use crate::storage::{Column, KeyValueStore, RocksDbStore};
use crate::transaction_auth::{
    transfer_signing_bytes_v1, verify_signed_transfer_v1, SignedTransferV1,
};

const TX_ID_DOMAIN: &[u8] = b"AIVOBLOCK-TX-ID-V1\0";
const EMPTY_DOMAIN: &[u8] = b"AIVOBLOCK-TX-EMPTY-V1\0";
const LEAF_DOMAIN: &[u8] = b"AIVOBLOCK-TX-LEAF-V1\0";
const NODE_DOMAIN: &[u8] = b"AIVOBLOCK-TX-NODE-V1\0";
const STATE_DOMAIN: &[u8] = b"AIVOBLOCK-STATE-ROOT-V1\0";
const HEADER_DOMAIN: &[u8; 26] = b"AIVOBLOCK-BLOCK-HEADER-V1\0";
const BODY_DOMAIN: &[u8] = b"AIVOBLOCK-TX-BODY-V1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommittedHeaderV1 {
    pub version: u32,
    pub chain_id: u32,
    pub height: u64,
    pub parent_hash: [u8; 32],
    pub transactions_root: [u8; 32],
    pub puzzle_proofs_root: [u8; 32],
    pub state_root: [u8; 32],
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitmentError {
    InvalidTransaction,
    DuplicateTransaction,
    TooManyTransactions,
    WrongTransactionRoot,
    WrongStateRoot,
    UnsupportedHeader,
    UnsupportedPuzzleProofs,
    WrongPriorState,
    InvalidBlockBytes,
    StorageFailure,
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Hash both canonical V1 signing bytes and the *exact* detached signature.
/// Do not hash JSON, ignore the signature, or permit invalid signed inputs.
pub fn signed_transaction_id_v1(transfer: &SignedTransferV1) -> Result<[u8; 32], CommitmentError> {
    verify_signed_transfer_v1(transfer).map_err(|_| CommitmentError::InvalidTransaction)?;
    let signing_bytes =
        transfer_signing_bytes_v1(transfer).map_err(|_| CommitmentError::InvalidTransaction)?;
    let mut bytes = Vec::with_capacity(TX_ID_DOMAIN.len() + 118 + 64);
    bytes.extend_from_slice(TX_ID_DOMAIN);
    bytes.extend_from_slice(&signing_bytes);
    bytes.extend_from_slice(&transfer.signature);
    Ok(hash(&bytes))
}

/// The leaves preserve block transaction ordering, and odd levels duplicate
/// their final leaf. Empty root is a distinct domain-separated constant.
pub fn transaction_root_v1(ids: &[[u8; 32]]) -> Result<[u8; 32], CommitmentError> {
    if ids.len() > 65_536 {
        return Err(CommitmentError::TooManyTransactions);
    }
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    if unique.len() != ids.len() {
        return Err(CommitmentError::DuplicateTransaction);
    }
    if ids.is_empty() {
        return Ok(hash(EMPTY_DOMAIN));
    }
    let mut level: Vec<[u8; 32]> = ids
        .iter()
        .map(|id| {
            let mut bytes = Vec::with_capacity(LEAF_DOMAIN.len() + 32);
            bytes.extend_from_slice(LEAF_DOMAIN);
            bytes.extend_from_slice(id);
            hash(&bytes)
        })
        .collect();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for pair in level.chunks(2) {
            let left = pair[0];
            let right = *pair.get(1).unwrap_or(&left);
            let mut bytes = Vec::with_capacity(NODE_DOMAIN.len() + 64);
            bytes.extend_from_slice(NODE_DOMAIN);
            bytes.extend_from_slice(&left);
            bytes.extend_from_slice(&right);
            next.push(hash(&bytes));
        }
        level = next;
    }
    Ok(level[0])
}

/// Domain-separated SHA-256 of exact typed Rust/Haskell ledger snapshot bytes.
pub fn ledger_state_root_v1(state: &LedgerState) -> Result<[u8; 32], CommitmentError> {
    let snapshot = encode_ledger_state(state).map_err(|_| CommitmentError::InvalidBlockBytes)?;
    let mut bytes = Vec::with_capacity(STATE_DOMAIN.len() + snapshot.len());
    bytes.extend_from_slice(STATE_DOMAIN);
    bytes.extend_from_slice(&snapshot);
    Ok(hash(&bytes))
}

/// Reuse the existing 178-byte canonical V1 header layout.
pub fn canonical_header_v1(header: &CommittedHeaderV1) -> Result<[u8; 178], CommitmentError> {
    if header.version != 1 || header.chain_id != 1 {
        return Err(CommitmentError::UnsupportedHeader);
    }
    let mut bytes = [0u8; 178];
    let mut at = 0usize;
    fn write(bytes: &mut [u8; 178], at: &mut usize, value: &[u8]) {
        bytes[*at..*at + value.len()].copy_from_slice(value);
        *at += value.len();
    }
    write(&mut bytes, &mut at, HEADER_DOMAIN);
    write(&mut bytes, &mut at, &header.version.to_be_bytes());
    write(&mut bytes, &mut at, &header.chain_id.to_be_bytes());
    write(&mut bytes, &mut at, &header.height.to_be_bytes());
    write(&mut bytes, &mut at, &header.parent_hash);
    write(&mut bytes, &mut at, &header.transactions_root);
    write(&mut bytes, &mut at, &header.puzzle_proofs_root);
    write(&mut bytes, &mut at, &header.state_root);
    write(&mut bytes, &mut at, &header.timestamp_ms.to_be_bytes());
    debug_assert_eq!(at, bytes.len());
    Ok(bytes)
}

/// Only one signed V1 transfer is supported in the present Haskell IPC
/// transition; rejecting all other counts prevents unexecuted transactions
/// from being committed under an apparently valid state root.
pub fn verify_single_transfer_block(
    header: &CommittedHeaderV1,
    transfer: &SignedTransferV1,
    post_state: &LedgerState,
) -> Result<([u8; 32], Vec<u8>), CommitmentError> {
    if header.puzzle_proofs_root != [0; 32] {
        return Err(CommitmentError::UnsupportedPuzzleProofs);
    }
    let tx_id = signed_transaction_id_v1(transfer)?;
    let expected_tx_root = transaction_root_v1(&[tx_id])?;
    if header.transactions_root != expected_tx_root {
        return Err(CommitmentError::WrongTransactionRoot);
    }
    if header.state_root != ledger_state_root_v1(post_state)? {
        return Err(CommitmentError::WrongStateRoot);
    }
    let encoded_header = canonical_header_v1(header)?;
    let block_id = hash(&encoded_header);
    let signing_bytes =
        transfer_signing_bytes_v1(transfer).map_err(|_| CommitmentError::InvalidTransaction)?;
    let mut body = Vec::with_capacity(BODY_DOMAIN.len() + 178 + 4 + 32 + 118 + 64);
    body.extend_from_slice(BODY_DOMAIN);
    body.extend_from_slice(&encoded_header);
    body.extend_from_slice(&1u32.to_be_bytes());
    body.extend_from_slice(&tx_id);
    body.extend_from_slice(&signing_bytes);
    body.extend_from_slice(&transfer.signature);
    Ok((block_id, body))
}

/// Decode the minimal committed block shape, recomputing the header ID,
/// its transaction ID and Merkle root, and checking the stored state root.
/// The Haskell-produced state still must be verified by its transition rules.
pub fn verify_stored_single_transfer_block(
    stored: &[u8],
    stored_id: &[u8; 32],
    transaction: &SignedTransferV1,
    post_state: &LedgerState,
) -> Result<(), CommitmentError> {
    if !stored.starts_with(BODY_DOMAIN) {
        return Err(CommitmentError::InvalidBlockBytes);
    }
    let rest = &stored[BODY_DOMAIN.len()..];
    if rest.len() != 178 + 4 + 32 + 118 + 64 {
        return Err(CommitmentError::InvalidBlockBytes);
    }
    let header_bytes = &rest[..178];
    if hash(header_bytes) != *stored_id {
        return Err(CommitmentError::InvalidBlockBytes);
    }
    let count = &rest[178..182];
    if count != 1u32.to_be_bytes() {
        return Err(CommitmentError::InvalidBlockBytes);
    }
    let id = signed_transaction_id_v1(transaction)?;
    if rest[182..214] != id {
        return Err(CommitmentError::InvalidTransaction);
    }
    let signing_bytes =
        transfer_signing_bytes_v1(transaction).map_err(|_| CommitmentError::InvalidTransaction)?;
    if rest[214..332] != signing_bytes || rest[332..396] != transaction.signature {
        return Err(CommitmentError::InvalidTransaction);
    }
    let expected_root = transaction_root_v1(&[id])?;
    if header_bytes[26 + 4 + 4 + 8 + 32..26 + 4 + 4 + 8 + 32 + 32] != expected_root {
        return Err(CommitmentError::WrongTransactionRoot);
    }
    let state_start = 26 + 4 + 4 + 8 + 32 * 3;
    if header_bytes[state_start..state_start + 32] != ledger_state_root_v1(post_state)? {
        return Err(CommitmentError::WrongStateRoot);
    }
    Ok(())
}

/// Validated wrapper around the low-level atomic storage API. It binds a
/// *single Haskell-executed* transfer to exact signing bytes, Merkle root,
/// resulting ledger snapshot, block ID and parent in the same atomic write.
/// It rejects stale prestate: the raw RocksDB API remains unsafe for
/// untrusted use and is intentionally not treated as consensus.
pub fn commit_signed_transfer_block(
    store: &RocksDbStore,
    header: &CommittedHeaderV1,
    tx: &SignedTransferV1,
    expected_prior: &LedgerState,
    haskell_post_state: &LedgerState,
) -> Result<[u8; 32], CommitmentError> {
    let on_disk = store
        .read_local_ledger_state()
        .map_err(|_| CommitmentError::StorageFailure)?;
    if on_disk.as_ref() != Some(expected_prior) {
        return Err(CommitmentError::WrongPriorState);
    }
    let (block_id, block_bytes) = verify_single_transfer_block(header, tx, haskell_post_state)?;
    store
        .append_atomic_ledger_block(
            &block_id,
            &header.parent_hash,
            header.height,
            &block_bytes,
            haskell_post_state,
            None,
        )
        .map_err(|_| CommitmentError::StorageFailure)?;
    let saved = store
        .get(Column::Blocks, &block_id)
        .map_err(|_| CommitmentError::StorageFailure)?
        .ok_or(CommitmentError::StorageFailure)?;
    verify_stored_single_transfer_block(&saved, &block_id, tx, haskell_post_state)?;
    Ok(block_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn tx(amount: u64, nonce: u64) -> SignedTransferV1 {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = key.verifying_key().to_bytes();
        let mut tx = SignedTransferV1 {
            chain_id: 1,
            version: 1,
            sender: public,
            recipient: [2; 32],
            amount,
            fee: 2,
            nonce,
            public_key: public,
            signature: [0; 64],
        };
        tx.signature = key
            .sign(&transfer_signing_bytes_v1(&tx).unwrap())
            .to_bytes();
        tx
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn cross_language_transaction_id_and_leaf_vector() {
        let id = signed_transaction_id_v1(&tx(10, 0)).unwrap();
        assert_eq!(
            hex(&id),
            "a75cb64af25b97c4f7f06feb4a77827ba4d1159426eb45a9c99fded8dcb5b52d"
        );
        assert_eq!(
            hex(&transaction_root_v1(&[id]).unwrap()),
            "d0ea168dc83df1fd8bf95b46bf20d434d4bd2ae12b1a9ce7cf77cadac19283c3"
        );
        assert_eq!(
            hex(&transaction_root_v1(&[]).unwrap()),
            "4cd0e95f1262d79dd9a479ac1cd9471a64552f5cf44a40ce4d8249b8d9f7f385"
        );
    }

    #[test]
    fn merkle_order_and_double_spends_change_root() {
        let a = signed_transaction_id_v1(&tx(10, 0)).unwrap();
        let b = signed_transaction_id_v1(&tx(11, 0)).unwrap();
        let c = signed_transaction_id_v1(&tx(12, 0)).unwrap();
        assert_ne!(
            transaction_root_v1(&[a, b]).unwrap(),
            transaction_root_v1(&[b, a]).unwrap()
        );
        assert_ne!(
            transaction_root_v1(&[a, b]).unwrap(),
            transaction_root_v1(&[a, b, c]).unwrap()
        );
        assert_ne!(
            transaction_root_v1(&[a, b, c]).unwrap(),
            transaction_root_v1(&[a, c, b]).unwrap()
        );
        assert_eq!(
            transaction_root_v1(&[a, a]),
            Err(CommitmentError::DuplicateTransaction)
        );
    }

    #[test]
    fn even_and_odd_merkle_vectors_match_haskell() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let c = [3u8; 32];
        assert_eq!(
            hex(&transaction_root_v1(&[a, b]).unwrap()),
            "59cdd667d6549503267625933df3bfa85016948f59f8bb91af0b945f926a6477"
        );
        assert_eq!(
            hex(&transaction_root_v1(&[a, b, c]).unwrap()),
            "0dbe0e823658b8f97440d5b79bc1b1aa7b0943ab43a1e9e930f1b4c881f2e822"
        );
    }

    #[test]
    fn unsigned_or_tampered_tx_has_no_valid_identifier() {
        let original = tx(10, 0);
        assert_eq!(
            signed_transaction_id_v1(&SignedTransferV1 {
                amount: 11,
                ..original
            }),
            Err(CommitmentError::InvalidTransaction)
        );
        assert_eq!(
            signed_transaction_id_v1(&SignedTransferV1 {
                signature: [0; 64],
                ..original
            }),
            Err(CommitmentError::InvalidTransaction)
        );
    }

    #[test]
    fn stored_block_binds_transaction_and_post_state() {
        let signed = tx(10, 0);
        let mut post = LedgerState::default();
        post.accounts.insert(
            signed.sender,
            crate::storage::LocalAccount {
                balance: 88,
                nonce: 1,
            },
        );
        post.accounts.insert(
            [2; 32],
            crate::storage::LocalAccount {
                balance: 10,
                nonce: 0,
            },
        );
        post.fee_pool = 2;
        let header = CommittedHeaderV1 {
            version: 1,
            chain_id: 1,
            height: 1,
            parent_hash: [3; 32],
            transactions_root: transaction_root_v1(&[signed_transaction_id_v1(&signed).unwrap()])
                .unwrap(),
            puzzle_proofs_root: [0; 32],
            state_root: ledger_state_root_v1(&post).unwrap(),
            timestamp_ms: 1001,
        };
        let (block_id, block_bytes) =
            verify_single_transfer_block(&header, &signed, &post).unwrap();
        verify_stored_single_transfer_block(&block_bytes, &block_id, &signed, &post).unwrap();
        let mut altered = post.clone();
        altered.fee_pool = 3;
        assert_eq!(
            verify_single_transfer_block(&header, &signed, &altered),
            Err(CommitmentError::WrongStateRoot)
        );
        let mut wrong = header;
        wrong.transactions_root = [0; 32];
        assert_eq!(
            verify_single_transfer_block(&wrong, &signed, &post),
            Err(CommitmentError::WrongTransactionRoot)
        );
        let mut changed = block_bytes.clone();
        changed[0] ^= 1;
        assert_eq!(
            verify_stored_single_transfer_block(&changed, &block_id, &signed, &post),
            Err(CommitmentError::InvalidBlockBytes)
        );
    }
}
