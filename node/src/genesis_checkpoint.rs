//! Explicitly-authorized local V1 genesis checkpoint.
//!
//! The trust anchor is the *operator-supplied* Ed25519 authority public key,
//! NEVER a key read from RocksDB or from the untrusted checkpoint itself.
//! This is a prototype trust policy, not distributed validator consensus.

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::block_replay::decode_header_v1;
use crate::chain_replay::{replay_local_chain, ChainReplayReport};
use crate::ipc::haskell::HaskellValidatorClient;
use crate::storage::ledger_state::{encode_ledger_state, LedgerState};
use crate::storage::RocksDbStore;
use crate::transaction_commitment::{ledger_state_root_v1, transaction_root_v1};

const GENESIS_DOMAIN: &[u8] = b"AIVOBLOCK-GENESIS-CHECKPOINT-V1\0";
const MAX_CHECKPOINT_STATE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SignedGenesisCheckpointV1 {
    pub genesis_header: [u8; 178],
    pub initial_ledger: LedgerState,
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenesisCheckpointError {
    InvalidHeader,
    InvalidGenesisState,
    OversizedSnapshot,
    InvalidAuthorityKey,
    InvalidSignature,
}

/// Canonical bytes authenticated by the external genesis authority.
///
/// domain || exact canonical header[178] || snapshot-length:u32_be ||
/// canonical sorted ledger snapshot. The header includes chain ID 1 and the
/// snapshot state root; both are independently checked before signing.
/// A key supplied *with* the checkpoint must never become a trust anchor.
pub fn genesis_checkpoint_signing_bytes_v1(
    header_bytes: &[u8; 178],
    state: &LedgerState,
) -> Result<Vec<u8>, GenesisCheckpointError> {
    let header = decode_header_v1(header_bytes).map_err(|_| GenesisCheckpointError::InvalidHeader)?;
    let empty_root =
        transaction_root_v1(&[]).map_err(|_| GenesisCheckpointError::InvalidHeader)?;
    let state_root =
        ledger_state_root_v1(state).map_err(|_| GenesisCheckpointError::InvalidGenesisState)?;
    if header.version != 1
        || header.chain_id != 1
        || header.height != 0
        || header.parent_hash != [0; 32]
        || header.puzzle_proofs_root != [0; 32]
        || header.transactions_root != empty_root
        || header.state_root != state_root
    {
        return Err(GenesisCheckpointError::InvalidGenesisState);
    }
    let bytes =
        encode_ledger_state(state).map_err(|_| GenesisCheckpointError::InvalidGenesisState)?;
    if bytes.len() > MAX_CHECKPOINT_STATE_BYTES {
        return Err(GenesisCheckpointError::OversizedSnapshot);
    }
    let count = u32::try_from(bytes.len())
        .map_err(|_| GenesisCheckpointError::OversizedSnapshot)?;
    let mut payload = Vec::with_capacity(GENESIS_DOMAIN.len() + 178 + 4 + bytes.len());
    payload.extend_from_slice(GENESIS_DOMAIN);
    payload.extend_from_slice(header_bytes);
    payload.extend_from_slice(&count.to_be_bytes());
    payload.extend_from_slice(&bytes);
    Ok(payload)
}

/// A successful verification binds the genesis allocation, its exact header,
/// chain ID, accounts, balances, nonces, fee pool and replay sets to an
/// independently configured authority key. No private keys are stored.
pub fn verify_signed_genesis_checkpoint(
    checkpoint: &SignedGenesisCheckpointV1,
    trusted_authority_public_key: &[u8; 32],
) -> Result<[u8; 32], GenesisCheckpointError> {
    let payload = genesis_checkpoint_signing_bytes_v1(
        &checkpoint.genesis_header,
        &checkpoint.initial_ledger,
    )?;
    let authority = VerifyingKey::from_bytes(trusted_authority_public_key)
        .map_err(|_| GenesisCheckpointError::InvalidAuthorityKey)?;
    let signature = Signature::from_bytes(&checkpoint.signature);
    authority
        .verify_strict(&payload, &signature)
        .map_err(|_| GenesisCheckpointError::InvalidSignature)?;
    Ok(Sha256::digest(checkpoint.genesis_header).into())
}

/// Only initialize an empty local atomic-ledger chain after verifying
/// the exact signed checkpoint. RocksDB rejects a second genesis and
/// rejects mixing older legacy storage. No mutation occurs on failed auth.
pub fn initialize_signed_genesis(
    db: &RocksDbStore,
    checkpoint: &SignedGenesisCheckpointV1,
    trusted_authority_public_key: &[u8; 32],
) -> Result<[u8; 32], String> {
    let genesis_id = verify_signed_genesis_checkpoint(checkpoint, trusted_authority_public_key)
        .map_err(|e| format!("Unauthenticated genesis checkpoint: {e:?}"))?;
    db.append_atomic_ledger_block(
        &genesis_id,
        &[0; 32],
        0,
        &checkpoint.genesis_header,
        &checkpoint.initial_ledger,
        None,
    )
    .map_err(|e| format!("Atomic signed genesis initialization: {e}"))?;
    Ok(genesis_id)
}

/// Authenticates the checkpoint BEFORE reading chain history or trusting its
/// genesis allocations. Existing replay then verifies all stored blocks,
/// signatures, Merkle roots and Haskell-computed transfers against that
/// externally signed checkpoint. This does not choose a consensus fork.
pub fn replay_chain_from_signed_checkpoint(
    db: &RocksDbStore,
    core: &HaskellValidatorClient,
    checkpoint: &SignedGenesisCheckpointV1,
    trusted_authority_public_key: &[u8; 32],
) -> Result<ChainReplayReport, String> {
    let id = verify_signed_genesis_checkpoint(checkpoint, trusted_authority_public_key)
        .map_err(|e| format!("Unauthenticated genesis checkpoint: {e:?}"))?;
    replay_local_chain(db, core, &id, &checkpoint.initial_ledger)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Column, KeyValueStore, LocalAccount};
    use crate::transaction_commitment::{canonical_header_v1, CommittedHeaderV1};
    use ed25519_dalek::{Signer, SigningKey};

    fn fixture() -> (SignedGenesisCheckpointV1, [u8; 32]) {
        // Fixed TEST-ONLY seed, NEVER a production signing authority.
        let authority = SigningKey::from_bytes(&[17; 32]);
        let mut ledger = LedgerState::default();
        ledger.accounts.insert(
            [1; 32],
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
        let bytes = canonical_header_v1(&header).unwrap();
        let signing = genesis_checkpoint_signing_bytes_v1(&bytes, &ledger).unwrap();
        (
            SignedGenesisCheckpointV1 {
                genesis_header: bytes,
                initial_ledger: ledger,
                signature: authority.sign(&signing).to_bytes(),
            },
            authority.verifying_key().to_bytes(),
        )
    }

    #[test]
    fn verifies_only_operator_pinned_authority_key() {
        let (checkpoint, trusted) = fixture();
        let id = verify_signed_genesis_checkpoint(&checkpoint, &trusted).unwrap();
        let calculated_id: [u8; 32] = Sha256::digest(checkpoint.genesis_header).into();
        assert_eq!(id, calculated_id);
        assert!(verify_signed_genesis_checkpoint(&checkpoint, &[3; 32]).is_err());
        let impostor = SigningKey::from_bytes(&[18; 32]);
        assert_eq!(
            verify_signed_genesis_checkpoint(&checkpoint, &impostor.verifying_key().to_bytes()),
            Err(GenesisCheckpointError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_changed_allocations_fees_nonces_roots_and_signatures() {
        let (checkpoint, key) = fixture();
        let mut altered = checkpoint.clone();
        altered.initial_ledger.accounts.get_mut(&[1; 32]).unwrap().balance += 1;
        assert_eq!(
            verify_signed_genesis_checkpoint(&altered, &key),
            Err(GenesisCheckpointError::InvalidGenesisState)
        );
        altered = checkpoint.clone();
        altered.initial_ledger.accounts.get_mut(&[1; 32]).unwrap().nonce = 1;
        assert!(verify_signed_genesis_checkpoint(&altered, &key).is_err());
        altered = checkpoint.clone();
        altered.initial_ledger.fee_pool = 2;
        assert!(verify_signed_genesis_checkpoint(&altered, &key).is_err());
        altered = checkpoint.clone();
        altered.genesis_header[138] ^= 1; // State root
        assert!(verify_signed_genesis_checkpoint(&altered, &key).is_err());
        altered = checkpoint.clone();
        altered.genesis_header[170] ^= 1; // Time is signed, even when state root is unchanged
        assert_eq!(
            verify_signed_genesis_checkpoint(&altered, &key),
            Err(GenesisCheckpointError::InvalidSignature)
        );
        altered = checkpoint;
        altered.signature[0] ^= 1;
        assert_eq!(
            verify_signed_genesis_checkpoint(&altered, &key),
            Err(GenesisCheckpointError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_wrong_chain_and_non_genesis_parent() {
        let (checkpoint, _) = fixture();
        let mut altered = checkpoint.clone();
        altered.genesis_header[30] = 2; // Chain ID first byte
        assert!(genesis_checkpoint_signing_bytes_v1(
            &altered.genesis_header,
            &altered.initial_ledger,
        )
        .is_err());
        altered = checkpoint;
        altered.genesis_header[42] = 1; // Parent hash
        assert!(genesis_checkpoint_signing_bytes_v1(
            &altered.genesis_header,
            &altered.initial_ledger,
        )
        .is_err());
    }

    #[test]
    fn invalid_signature_does_not_write_and_signed_genesis_survives_reopen() {
        let dir = std::env::temp_dir().join(format!(
            "aivoblock-genesis-checkpoint-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let (checkpoint, trusted) = fixture();
        let id = verify_signed_genesis_checkpoint(&checkpoint, &trusted).unwrap();
        {
            let db = RocksDbStore::open(&dir).unwrap();
            assert!(initialize_signed_genesis(&db, &checkpoint, &[4; 32]).is_err());
            assert_eq!(db.get(Column::ChainState, b"tip").unwrap(), None);
            assert_eq!(db.read_local_ledger_state().unwrap(), None);
            assert_eq!(initialize_signed_genesis(&db, &checkpoint, &trusted).unwrap(), id);
            assert!(initialize_signed_genesis(&db, &checkpoint, &trusted).is_err());
        }
        {
            let db = RocksDbStore::open(&dir).unwrap();
            assert_eq!(db.get(Column::ChainState, b"tip").unwrap(), Some(id.to_vec()));
            assert_eq!(
                db.read_local_ledger_state().unwrap(),
                Some(checkpoint.initial_ledger)
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
