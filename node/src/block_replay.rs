//! Independent V1 decoder for persisted signed multi-transaction blocks.
//!
//! Unlike the writer's original in-memory comparison, this parses the bytes
//! read back from RocksDB and authenticates each embedded signed envelope.
//! Full ledger replay is performed separately by trusted local Haskell IPC.
//! Neither layer establishes consensus or validates a trusted genesis.

use sha2::{Digest, Sha256};

use crate::batch_commitment::MAX_SIGNED_BLOCK_TRANSFERS;
use crate::transaction_auth::{transfer_signing_bytes_v1, SignedTransferV1};
use crate::transaction_commitment::{
    canonical_header_v1, signed_transaction_id_v1, transaction_root_v1,
    CommitmentError, CommittedHeaderV1,
};

const BODY_DOMAIN: &[u8] = b"AIVOBLOCK-TX-BATCH-BODY-V1\0";
const HEADER_DOMAIN: &[u8] = b"AIVOBLOCK-BLOCK-HEADER-V1\0";
const SIGNING_DOMAIN: &[u8] = b"AIVOBLOCK-TRANSFER-V1\0";
const HEADER_SIZE: usize = 178;
const RECORD_SIZE: usize = 32 + 118 + 64;

#[derive(Debug, PartialEq, Eq)]
pub struct DecodedSignedBatch {
    pub header: CommittedHeaderV1,
    pub transactions: Vec<SignedTransferV1>,
    pub transaction_ids: Vec<[u8; 32]>,
}

fn bytes<const N: usize>(source: &[u8], at: usize) -> Result<[u8; N], CommitmentError> {
    source
        .get(at..at.checked_add(N).ok_or(CommitmentError::InvalidBlockBytes)?)
        .ok_or(CommitmentError::InvalidBlockBytes)?
        .try_into()
        .map_err(|_| CommitmentError::InvalidBlockBytes)
}

fn u32_be(source: &[u8], at: usize) -> Result<u32, CommitmentError> {
    Ok(u32::from_be_bytes(bytes::<4>(source, at)?))
}

fn u64_be(source: &[u8], at: usize) -> Result<u64, CommitmentError> {
    Ok(u64::from_be_bytes(bytes::<8>(source, at)?))
}

/// Decode exactly the already-deployed 178-byte canonical header. Reject
/// unsupported versions and chains, and require a byte-identical round trip.
pub fn decode_header_v1(input: &[u8]) -> Result<CommittedHeaderV1, CommitmentError> {
    if input.len() != HEADER_SIZE || !input.starts_with(HEADER_DOMAIN) {
        return Err(CommitmentError::InvalidBlockBytes);
    }
    let header = CommittedHeaderV1 {
        version: u32_be(input, 26)?,
        chain_id: u32_be(input, 30)?,
        height: u64_be(input, 34)?,
        parent_hash: bytes::<32>(input, 42)?,
        transactions_root: bytes::<32>(input, 74)?,
        puzzle_proofs_root: bytes::<32>(input, 106)?,
        state_root: bytes::<32>(input, 138)?,
        timestamp_ms: u64_be(input, 170)?,
    };
    if canonical_header_v1(&header)?.as_slice() != input {
        return Err(CommitmentError::InvalidBlockBytes);
    }
    Ok(header)
}

/// Parse the on-disk V1 format without any external transaction list.
/// A valid result verifies block ID, canonical header, every Ed25519
/// signature, canonical signing bytes, record IDs, and the ordered Merkle
/// root. State execution and the state root are checked during Haskell replay.
pub fn decode_signed_batch_v1(
    stored: &[u8],
    block_id: &[u8; 32],
) -> Result<DecodedSignedBatch, CommitmentError> {
    let header_at = BODY_DOMAIN.len();
    let count_at = header_at + HEADER_SIZE;
    let records_at = count_at + 4;
    if !stored.starts_with(BODY_DOMAIN) || stored.len() < records_at {
        return Err(CommitmentError::InvalidBlockBytes);
    }
    let raw_header = &stored[header_at..count_at];
    let computed_id: [u8; 32] = Sha256::digest(raw_header).into();
    if &computed_id != block_id {
        return Err(CommitmentError::InvalidBlockBytes);
    }
    let header = decode_header_v1(raw_header)?;
    if header.puzzle_proofs_root != [0; 32] {
        return Err(CommitmentError::UnsupportedPuzzleProofs);
    }
    let count = u32_be(stored, count_at)? as usize;
    if count == 0 || count > MAX_SIGNED_BLOCK_TRANSFERS {
        return Err(CommitmentError::TooManyTransactions);
    }
    let length = records_at
        .checked_add(count.checked_mul(RECORD_SIZE).ok_or(CommitmentError::InvalidBlockBytes)?)
        .ok_or(CommitmentError::InvalidBlockBytes)?;
    if stored.len() != length {
        return Err(CommitmentError::InvalidBlockBytes);
    }

    let mut transactions = Vec::with_capacity(count);
    let mut transaction_ids = Vec::with_capacity(count);
    for record in stored[records_at..].chunks_exact(RECORD_SIZE) {
        let claimed_id = bytes::<32>(record, 0)?;
        let raw_signing = &record[32..150];
        if !raw_signing.starts_with(SIGNING_DOMAIN) {
            return Err(CommitmentError::InvalidTransaction);
        }
        let version = u32_be(raw_signing, 22)?;
        let chain_id = u32_be(raw_signing, 26)?;
        if version != header.version || chain_id != header.chain_id {
            return Err(CommitmentError::InvalidTransaction);
        }
        let key = bytes::<32>(raw_signing, 30)?;
        let tx = SignedTransferV1 {
            version,
            chain_id,
            public_key: key,
            sender: key,
            recipient: bytes::<32>(raw_signing, 62)?,
            amount: u64_be(raw_signing, 94)?,
            fee: u64_be(raw_signing, 102)?,
            nonce: u64_be(raw_signing, 110)?,
            signature: bytes::<64>(record, 150)?,
        };
        if transfer_signing_bytes_v1(&tx)
            .map_err(|_| CommitmentError::InvalidTransaction)?
            .as_slice() != raw_signing
        {
            return Err(CommitmentError::InvalidTransaction);
        }
        let calculated_id = signed_transaction_id_v1(&tx)?;
        if claimed_id != calculated_id {
            return Err(CommitmentError::InvalidTransaction);
        }
        transaction_ids.push(calculated_id);
        transactions.push(tx);
    }
    if transaction_root_v1(&transaction_ids)? != header.transactions_root {
        return Err(CommitmentError::WrongTransactionRoot);
    }
    Ok(DecodedSignedBatch { header, transactions, transaction_ids })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch_commitment::encode_verified_batch_block;
    use crate::storage::{LedgerState, LocalAccount};
    use crate::transaction_auth::transfer_signing_bytes_v1;
    use crate::transaction_commitment::ledger_state_root_v1;
    use ed25519_dalek::{Signer, SigningKey};

    fn make_transfer(nonce: u64, amount: u64) -> SignedTransferV1 {
        let signer = SigningKey::from_bytes(&[7; 32]);
        let pubkey = signer.verifying_key().to_bytes();
        let mut tx = SignedTransferV1 {
            version: 1, chain_id: 1, sender: pubkey, public_key: pubkey,
            recipient: [2; 32], nonce, amount, fee: 1, signature: [0; 64],
        };
        tx.signature = signer.sign(&transfer_signing_bytes_v1(&tx).unwrap()).to_bytes();
        tx
    }

    fn sample() -> ([u8; 32], Vec<u8>, Vec<SignedTransferV1>) {
        let transfers = vec![make_transfer(0, 10), make_transfer(1, 5)];
        let mut ledger = LedgerState { fee_pool: 2, ..LedgerState::default() };
        ledger.accounts.insert(transfers[0].sender, LocalAccount { balance: 83, nonce: 2 });
        ledger.accounts.insert([2; 32], LocalAccount { balance: 15, nonce: 0 });
        let ids = transfers.iter().map(signed_transaction_id_v1)
            .collect::<Result<Vec<_>, _>>().unwrap();
        let header = CommittedHeaderV1 {
            version: 1, chain_id: 1, height: 1, parent_hash: [3; 32],
            transactions_root: transaction_root_v1(&ids).unwrap(),
            puzzle_proofs_root: [0; 32],
            state_root: ledger_state_root_v1(&ledger).unwrap(),
            timestamp_ms: 1001,
        };
        let (id, body) = encode_verified_batch_block(&header, &transfers, &ledger).unwrap();
        (id, body, transfers)
    }

    #[test]
    fn decodes_bytes_without_caller_provided_transactions() {
        let (id, body, expected) = sample();
        let decoded = decode_signed_batch_v1(&body, &id).unwrap();
        assert_eq!(decoded.transactions, expected);
        assert_eq!(decoded.header.height, 1);
        assert_eq!(decoded.header.parent_hash, [3; 32]);
        let expected_ids = expected.iter().map(signed_transaction_id_v1)
            .collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(decoded.transaction_ids, expected_ids);
        assert_eq!(decode_header_v1(&body[BODY_DOMAIN.len()..BODY_DOMAIN.len() + 178]).unwrap(), decoded.header);
    }

    #[test]
    fn rejects_truncation_and_trailing_bytes() {
        let (id, body, _) = sample();
        for len in 0..body.len() {
            assert!(decode_signed_batch_v1(&body[..len], &id).is_err());
        }
        let mut trailing = body;
        trailing.push(0);
        assert_eq!(decode_signed_batch_v1(&trailing, &id), Err(CommitmentError::InvalidBlockBytes));
    }

    #[test]
    fn rejects_wrong_id_invalid_count_and_changed_header() {
        let (id, body, _) = sample();
        assert_eq!(decode_signed_batch_v1(&body, &[0; 32]), Err(CommitmentError::InvalidBlockBytes));
        for count in [0, 65, u32::MAX] {
            let mut forged = body.clone();
            let start = BODY_DOMAIN.len() + HEADER_SIZE;
            forged[start..start + 4].copy_from_slice(&count.to_be_bytes());
            assert!(decode_signed_batch_v1(&forged, &id).is_err());
        }
        let mut forged = body;
        forged[BODY_DOMAIN.len() + 74] ^= 1;
        assert_eq!(decode_signed_batch_v1(&forged, &id), Err(CommitmentError::InvalidBlockBytes));
    }

    #[test]
    fn rejects_modified_id_signature_order_and_signing_fields() {
        let (id, body, _) = sample();
        let at = BODY_DOMAIN.len() + HEADER_SIZE + 4;
        for offset in [0, 32 + 94, 32 + 22, 32 + 30, 32 + 118] {
            let mut altered = body.clone();
            altered[at + offset] ^= 1;
            assert!(decode_signed_batch_v1(&altered, &id).is_err());
        }
        let mut swapped = body;
        let first = swapped[at..at + RECORD_SIZE].to_vec();
        let second = swapped[at + RECORD_SIZE..at + RECORD_SIZE * 2].to_vec();
        swapped[at..at + RECORD_SIZE].copy_from_slice(&second);
        swapped[at + RECORD_SIZE..at + RECORD_SIZE * 2].copy_from_slice(&first);
        assert_eq!(
            decode_signed_batch_v1(&swapped, &id),
            Err(CommitmentError::WrongTransactionRoot)
        );
    }
}
