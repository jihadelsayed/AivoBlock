//! RocksDB-backed local key-value storage for each AivoBlock node.
//!
//! Each node opens its own database directory. Storage persists bytes;
//! callers must verify consensus rules before committing chain state.
//! The initial API exposes individual writes, not atomic block commits.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use rocksdb::{Options, WriteBatch, WriteOptions, DB};
use sha2::{Digest, Sha256};

use super::ledger_state::{decode_ledger_state, encode_ledger_state, LedgerState};
use super::{encode_key, Column, KeyValueStore};

#[derive(Debug)]
pub enum StorageError {
    EmptyKey,
    InvalidBlockHash,
    InvalidParentHash,
    WrongParent,
    WrongHeight,
    CorruptChainState,
    InvalidSettlementRecord,
    DuplicateRewardBatch,
    AlreadyPaidReceipt,
    InvalidLedgerTransition,
    IncompatibleStorageMode,
    Database(rocksdb::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKey => write!(f, "storage key must not be empty"),
            Self::InvalidBlockHash => write!(f, "block hash must be exactly 32 bytes"),
            Self::InvalidParentHash => write!(f, "parent hash must be exactly 32 bytes"),
            Self::WrongParent => write!(f, "parent hash does not match the stored tip"),
            Self::WrongHeight => write!(f, "block height must follow the stored tip"),
            Self::CorruptChainState => write!(f, "stored chain metadata is missing or malformed"),
            Self::InvalidSettlementRecord => write!(f, "invalid receipt settlement record"),
            Self::DuplicateRewardBatch => write!(f, "reward batch already persisted"),
            Self::AlreadyPaidReceipt => write!(f, "reward receipt already persisted"),
            Self::InvalidLedgerTransition => write!(f, "invalid local ledger state transition"),
            Self::IncompatibleStorageMode => write!(f, "cannot mix legacy storage with atomic ledger blocks"),
            Self::Database(err) => write!(f, "RocksDB error: {err}"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(err) => Some(err),
            Self::EmptyKey
            | Self::InvalidBlockHash
            | Self::InvalidParentHash
            | Self::WrongParent
            | Self::WrongHeight
            | Self::CorruptChainState
            | Self::InvalidSettlementRecord
            | Self::DuplicateRewardBatch
            | Self::AlreadyPaidReceipt
            | Self::InvalidLedgerTransition
            | Self::IncompatibleStorageMode => None,
        }
    }
}

impl From<rocksdb::Error> for StorageError {
    fn from(err: rocksdb::Error) -> Self {
        Self::Database(err)
    }
}

fn storage_key(column: Column, key: &[u8]) -> Result<Vec<u8>, StorageError> {
    encode_key(column, key).map_err(|_| StorageError::EmptyKey)
}

/// Authenticated-by-caller settlement to be committed with a validated block.
/// The storage engine checks identifiers and atomicity, NOT proof correctness.
#[derive(Debug)]
pub struct SettlementBatch<'a> {
    pub batch_id: [u8; 32],
    pub receipt_ids: &'a [[u8; 32]],
    pub validated_response: &'a [u8],
}

const LOCAL_STATE_KEY: &[u8] = b"local-ledger-snapshot-v1";
const LOCAL_DIGEST_KEY: &[u8] = b"local-ledger-digest-v1";
const LEGACY_REPLAY_USED_KEY: &[u8] = b"legacy-replay-guard-used-v1";

fn marker_key(prefix: &[u8], id: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
    let mut suffix = prefix.to_vec();
    suffix.extend_from_slice(id);
    storage_key(Column::Metadata, &suffix)
}

fn local_supply(state: &LedgerState) -> u128 {
    state
        .accounts
        .values()
        .map(|account| u128::from(account.balance))
        .sum::<u128>()
        + u128::from(state.fee_pool)
}

/// Persistent database owned by a single node instance.
///
/// Open the same directory from only one live node process. Use different
/// directories for separate nodes in the private testnet.
pub struct RocksDbStore {
    db: DB,
    append_lock: Mutex<()>,
}

impl RocksDbStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let mut options = Options::default();
        options.create_if_missing(true);
        let db = DB::open(&options, path)?;
        Ok(Self {
            db,
            append_lock: Mutex::new(()),
        })
    }

    /// Legacy demo-only batch writer: does NOT check ancestry or height.
    /// Prefer append_checked_block for sequential local chain experiments.
    /// Atomically persist an already validated block and its chain tip.
    ///
    /// Callers MUST validate the block and derive its canonical hash before
    /// calling this method. The store does not authenticate either value.
    /// Both records are written together through a RocksDB WriteBatch.
    /// RocksDB's default WAL is enabled, but this does not request fsync.
    pub fn put_validated_block_and_tip(
        &self,
        block_hash: &[u8],
        block_bytes: &[u8],
    ) -> Result<(), StorageError> {
        if block_hash.len() != 32 {
            return Err(StorageError::InvalidBlockHash);
        }
        let block_key = storage_key(Column::Blocks, block_hash)?;
        let tip_key = storage_key(Column::ChainState, b"tip")?;
        let mut batch = WriteBatch::default();
        batch.put(block_key, block_bytes);
        batch.put(tip_key, block_hash);
        self.db.write(batch).map_err(StorageError::from)
    }

    /// Append a previously verified block to a single local chain.
    ///
    /// Ensures the incoming parent equals the current tip, and the incoming
    /// height is exactly current height + 1. Genesis requires an empty store,
    /// height 0, and a zero parent. The block, tip, and height are written in
    /// one RocksDB batch. This does NOT validate signatures, roots or consensus.
    pub fn append_checked_block(
        &self,
        block_hash: &[u8],
        parent_hash: &[u8],
        height: u64,
        block_bytes: &[u8],
    ) -> Result<(), StorageError> {
        if block_hash.len() != 32 {
            return Err(StorageError::InvalidBlockHash);
        }
        if parent_hash.len() != 32 {
            return Err(StorageError::InvalidParentHash);
        }
        let _guard = self
            .append_lock
            .lock()
            .map_err(|_| StorageError::CorruptChainState)?;
        let tip_key = storage_key(Column::ChainState, b"tip")?;
        let height_key = storage_key(Column::ChainState, b"height")?;
        let existing_tip = self.db.get(&tip_key)?;
        let existing_height = self.db.get(&height_key)?;
        match (existing_tip, existing_height) {
            (None, None) => {
                if height != 0 {
                    return Err(StorageError::WrongHeight);
                }
                if parent_hash != [0u8; 32] {
                    return Err(StorageError::WrongParent);
                }
            }
            (Some(tip), Some(bytes)) if tip.len() == 32 && bytes.len() == 8 => {
                if parent_hash != tip.as_slice() {
                    return Err(StorageError::WrongParent);
                }
                let mut hbytes = [0u8; 8];
                hbytes.copy_from_slice(&bytes);
                let previous = u64::from_be_bytes(hbytes);
                if previous.checked_add(1) != Some(height) {
                    return Err(StorageError::WrongHeight);
                }
            }
            _ => return Err(StorageError::CorruptChainState),
        }
        let block_key = storage_key(Column::Blocks, block_hash)?;
        if self.db.get(&block_key)?.is_some() {
            return Err(StorageError::CorruptChainState);
        }
        let mut batch = WriteBatch::default();
        batch.put(block_key, block_bytes);
        batch.put(tip_key, block_hash);
        batch.put(height_key, height.to_be_bytes());
        self.db.write(batch).map_err(StorageError::from)
    }

    /// Atomically remember the accepted response and all paid receipt IDs.
    ///
    /// This is a local demo replay guard, NOT chain reward finality. Caller must
    /// validate the Haskell settlement response and every receipt first.
    /// Commit uses a synced WAL. Real block execution must commit balances,
    /// receipts and tip in ONE transaction, including reorg handling.
    pub fn persist_settlement_replay_guard(
        &self,
        batch_id: &[u8; 32],
        receipt_ids: &[[u8; 32]],
        settlement_response: &[u8],
    ) -> Result<(), StorageError> {
        if *batch_id == [0; 32]
            || receipt_ids.is_empty()
            || settlement_response.is_empty()
            || receipt_ids.contains(&[0; 32])
        {
            return Err(StorageError::InvalidSettlementRecord);
        }
        let unique = receipt_ids.iter().collect::<BTreeSet<_>>();
        if unique.len() != receipt_ids.len() {
            return Err(StorageError::InvalidSettlementRecord);
        }
        let _guard = self
            .append_lock
            .lock()
            .map_err(|_| StorageError::CorruptChainState)?;

        let mut batch_suffix = b"reward-batch-v1:".to_vec();
        batch_suffix.extend_from_slice(batch_id);
        let batch_key = storage_key(Column::Metadata, &batch_suffix)?;
        if self.db.get(&batch_key)?.is_some() {
            return Err(StorageError::DuplicateRewardBatch);
        }
        let mut receipt_keys = Vec::with_capacity(receipt_ids.len());
        for receipt_id in receipt_ids {
            let mut suffix = b"paid-receipt-v1:".to_vec();
            suffix.extend_from_slice(receipt_id);
            let key = storage_key(Column::Metadata, &suffix)?;
            if self.db.get(&key)?.is_some() {
                return Err(StorageError::AlreadyPaidReceipt);
            }
            receipt_keys.push(key);
        }
        let mut batch = WriteBatch::default();
        batch.put(batch_key, settlement_response);
        for key in receipt_keys {
            batch.put(key, [1u8]);
        }
        let mut options = WriteOptions::default();
        options.set_sync(true);
        self.db
            .write_opt(batch, &options)
            .map_err(StorageError::from)
    }

    /// Read the latest local ledger snapshot only if both the saved digest
    /// and the chain tip/height are present and internally consistent.
    /// The checksum detects damage; it is NOT a consensus state root.
    pub fn read_local_ledger_state(&self) -> Result<Option<LedgerState>, StorageError> {
        let _guard = self
            .append_lock
            .lock()
            .map_err(|_| StorageError::CorruptChainState)?;
        let tip_key = storage_key(Column::ChainState, b"tip")?;
        let height_key = storage_key(Column::ChainState, b"height")?;
        let state_key = storage_key(Column::ChainState, LOCAL_STATE_KEY)?;
        let digest_key = storage_key(Column::ChainState, LOCAL_DIGEST_KEY)?;
        let tip = self.db.get(tip_key)?;
        let height = self.db.get(height_key)?;
        let state = self.db.get(state_key)?;
        let digest = self.db.get(digest_key)?;
        match (tip, height, state, digest) {
            (None, None, None, None) => Ok(None),
            (Some(tip), Some(height), Some(state), Some(digest))
                if tip.len() == 32 && height.len() == 8 && digest.len() == 32 =>
            {
                let block_key = storage_key(Column::Blocks, tip.as_ref())?;
                if self.db.get(block_key)?.is_none() {
                    return Err(StorageError::CorruptChainState);
                }
                let expected: [u8; 32] = Sha256::digest(&state).into();
                if digest.as_ref() != expected {
                    return Err(StorageError::CorruptChainState);
                }
                let decoded = decode_ledger_state(&state)
                    .map_err(|_| StorageError::CorruptChainState)?;
                Ok(Some(decoded))
            }
            _ => Err(StorageError::CorruptChainState),
        }
    }

    /// Append an already-authorized block AND its full post-execution ledger
    /// snapshot with optional reward settlement and replay markers.
    ///
    /// A single synced RocksDB WriteBatch commits block, tip, height, fee pool,
    /// account balances/nonces, settled batch IDs and paid receipt IDs. A
    /// failed precondition emits NO writes. After restart, the snapshot digest
    /// is verified before a successor can be appended.
    ///
    /// Consensus validation, true Haskell state transition verification,
    /// authenticated reward budget, header state-root binding and reorganizations
    /// are external, NOT implemented by this local storage API.
    pub fn append_atomic_ledger_block(
        &self,
        block_hash: &[u8; 32],
        parent_hash: &[u8; 32],
        height: u64,
        block_bytes: &[u8],
        post_state: &LedgerState,
        settlement: Option<&SettlementBatch<'_>>,
    ) -> Result<(), StorageError> {
        if *block_hash == [0; 32] || block_bytes.is_empty() {
            return Err(StorageError::InvalidBlockHash);
        }
        let _guard = self
            .append_lock
            .lock()
            .map_err(|_| StorageError::CorruptChainState)?;
        let tip_key = storage_key(Column::ChainState, b"tip")?;
        let height_key = storage_key(Column::ChainState, b"height")?;
        let state_key = storage_key(Column::ChainState, LOCAL_STATE_KEY)?;
        let digest_key = storage_key(Column::ChainState, LOCAL_DIGEST_KEY)?;
        let legacy_key = storage_key(Column::Metadata, LEGACY_REPLAY_USED_KEY)?;

        let tip = self.db.get(&tip_key)?;
        let existing_height = self.db.get(&height_key)?;
        let prior_snapshot = self.db.get(&state_key)?;
        let prior_digest = self.db.get(&digest_key)?;
        let prior_state = match (tip, existing_height, prior_snapshot, prior_digest) {
            (None, None, None, None) => {
                if height != 0 {
                    return Err(StorageError::WrongHeight);
                }
                if *parent_hash != [0; 32] {
                    return Err(StorageError::WrongParent);
                }
                if self.db.get(legacy_key)?.is_some() {
                    return Err(StorageError::IncompatibleStorageMode);
                }
                None
            }
            (Some(tip), Some(height_bytes), Some(snapshot), Some(digest))
                if tip.len() == 32 && height_bytes.len() == 8 && digest.len() == 32 =>
            {
                if parent_hash.as_slice() != tip.as_ref() {
                    return Err(StorageError::WrongParent);
                }
                let mut saved_height = [0; 8];
                saved_height.copy_from_slice(&height_bytes);
                if u64::from_be_bytes(saved_height).checked_add(1) != Some(height) {
                    return Err(StorageError::WrongHeight);
                }
                let expected: [u8; 32] = Sha256::digest(&snapshot).into();
                if digest.as_ref() != expected {
                    return Err(StorageError::CorruptChainState);
                }
                let state = decode_ledger_state(&snapshot)
                    .map_err(|_| StorageError::CorruptChainState)?;
                Some(state)
            }
            _ => return Err(StorageError::IncompatibleStorageMode),
        };

        let block_key = storage_key(Column::Blocks, block_hash)?;
        if self.db.get(&block_key)?.is_some() {
            return Err(StorageError::CorruptChainState);
        }

        // No settlement may silently drop a prior paid receipt or batch.
        // Exactly the new markers are allowed on every transition.
        let mut required_batches = prior_state
            .as_ref()
            .map(|state| state.settled_batches.clone())
            .unwrap_or_default();
        let mut required_receipts = prior_state
            .as_ref()
            .map(|state| state.paid_receipts.clone())
            .unwrap_or_default();
        let mut new_receipt_keys = Vec::new();
        let mut new_batch_key = None;

        if let Some(batch) = settlement {
            if prior_state.is_none()
                || batch.batch_id == [0; 32]
                || batch.validated_response.is_empty()
                || batch.receipt_ids.is_empty()
                || batch.receipt_ids.contains(&[0; 32])
            {
                return Err(StorageError::InvalidSettlementRecord);
            }
            if !required_batches.insert(batch.batch_id) {
                return Err(StorageError::DuplicateRewardBatch);
            }
            let key = marker_key(b"reward-batch-v1:", &batch.batch_id)?;
            if self.db.get(&key)?.is_some() {
                return Err(StorageError::DuplicateRewardBatch);
            }
            new_batch_key = Some(key);
            for receipt in batch.receipt_ids {
                if !required_receipts.insert(*receipt) {
                    return Err(StorageError::AlreadyPaidReceipt);
                }
                let key = marker_key(b"paid-receipt-v1:", receipt)?;
                if self.db.get(&key)?.is_some() {
                    return Err(StorageError::AlreadyPaidReceipt);
                }
                new_receipt_keys.push(key);
            }
        }

        if post_state.settled_batches != required_batches
            || post_state.paid_receipts != required_receipts
            || post_state.settled_batches.contains(&[0; 32])
            || post_state.paid_receipts.contains(&[0; 32])
        {
            return Err(StorageError::InvalidLedgerTransition);
        }
        if let Some(previous) = &prior_state {
            if local_supply(previous) != local_supply(post_state) {
                return Err(StorageError::InvalidLedgerTransition);
            }
            for (address, account) in &previous.accounts {
                match post_state.accounts.get(address) {
                    Some(next) if next.nonce >= account.nonce => {}
                    _ => return Err(StorageError::InvalidLedgerTransition),
                }
            }
        }

        let snapshot = encode_ledger_state(post_state)
            .map_err(|_| StorageError::InvalidLedgerTransition)?;
        let digest: [u8; 32] = Sha256::digest(&snapshot).into();
        let mut writes = WriteBatch::default();
        writes.put(block_key, block_bytes);
        writes.put(tip_key, block_hash);
        writes.put(height_key, height.to_be_bytes());
        writes.put(state_key, snapshot);
        writes.put(digest_key, digest);
        if let (Some(batch), Some(key)) = (settlement, new_batch_key) {
            writes.put(key, batch.validated_response);
            for key in new_receipt_keys {
                writes.put(key, [1u8]);
            }
        }
        let mut options = WriteOptions::default();
        options.set_sync(true);
        self.db
            .write_opt(writes, &options)
            .map_err(StorageError::from)
    }

    /// Flush the database's memtable. This is not a consensus operation.
    pub fn flush(&self) -> Result<(), StorageError> {
        self.db.flush().map_err(StorageError::from)
    }
}

impl KeyValueStore for RocksDbStore {
    type Error = StorageError;

    fn get(&self, column: Column, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error> {
        let key = storage_key(column, key)?;
        self.db.get(key).map_err(StorageError::from)
    }

    fn put(&self, column: Column, key: &[u8], value: &[u8]) -> Result<(), Self::Error> {
        let key = storage_key(column, key)?;
        self.db.put(key, value).map_err(StorageError::from)
    }

    fn delete(&self, column: Column, key: &[u8]) -> Result<(), Self::Error> {
        let key = storage_key(column, key)?;
        self.db.delete(key).map_err(StorageError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_settlement_guard_survives_reopen_and_rejects_replay_atomically() {
        let path = std::env::temp_dir().join(format!(
            "aivoblock-reward-guard-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        let id1 = [1u8; 32];
        let id2 = [2u8; 32];
        {
            let db = RocksDbStore::open(&path).unwrap();
            db.persist_settlement_replay_guard(&[9; 32], &[id1, id2], b"accepted")
                .unwrap();
            assert!(matches!(
                db.persist_settlement_replay_guard(&[9; 32], &[id1], b"again"),
                Err(StorageError::DuplicateRewardBatch)
            ));
            assert!(matches!(
                db.persist_settlement_replay_guard(&[10; 32], &[id2, [3; 32]], b"again"),
                Err(StorageError::AlreadyPaidReceipt)
            ));
        }
        {
            let db = RocksDbStore::open(&path).unwrap();
            assert!(matches!(
                db.persist_settlement_replay_guard(&[11; 32], &[id1], b"again"),
                Err(StorageError::AlreadyPaidReceipt)
            ));
            // Failed batch containing an already-paid receipt must not have
            // persisted its other receipt or the new batch marker.
            db.persist_settlement_replay_guard(&[10; 32], &[[3; 32]], b"accepted2")
                .unwrap();
            assert!(matches!(
                db.persist_settlement_replay_guard(&[10; 32], &[[4; 32]], b"again"),
                Err(StorageError::DuplicateRewardBatch)
            ));
            assert!(matches!(
                db.persist_settlement_replay_guard(&[12; 32], &[[3; 32]], b"again"),
                Err(StorageError::AlreadyPaidReceipt)
            ));
        }
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn settlement_guard_rejects_duplicate_and_zero_identifiers() {
        let path = std::env::temp_dir().join(format!(
            "aivoblock-reward-invalid-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        {
            let db = RocksDbStore::open(&path).unwrap();
            assert!(matches!(
                db.persist_settlement_replay_guard(&[9; 32], &[[1; 32], [1; 32]], b"record"),
                Err(StorageError::InvalidSettlementRecord)
            ));
            assert!(matches!(
                db.persist_settlement_replay_guard(&[0; 32], &[[1; 32]], b"record"),
                Err(StorageError::InvalidSettlementRecord)
            ));
            assert!(matches!(
                db.persist_settlement_replay_guard(&[9; 32], &[[0; 32]], b"record"),
                Err(StorageError::InvalidSettlementRecord)
            ));
        }
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn rejects_empty_key_before_database_access() {
        assert!(matches!(
            storage_key(Column::Blocks, b""),
            Err(StorageError::EmptyKey)
        ));
    }

    #[test]
    fn validates_block_hash_size_without_writing() {
        let path = std::env::temp_dir().join(format!(
            "aivoblock-rocksdb-invalid-hash-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        {
            let store = RocksDbStore::open(&path).unwrap();
            assert!(matches!(
                store.put_validated_block_and_tip(b"short", b"block"),
                Err(StorageError::InvalidBlockHash)
            ));
            assert_eq!(store.get(Column::ChainState, b"tip").unwrap(), None);
        }
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn persists_block_and_tip_across_reopen() {
        let path = std::env::temp_dir().join(format!(
            "aivoblock-rocksdb-batch-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        let hash = [42u8; 32];
        {
            let store = RocksDbStore::open(&path).unwrap();
            store
                .put_validated_block_and_tip(&hash, b"sample-block")
                .unwrap();
        }
        {
            let store = RocksDbStore::open(&path).unwrap();
            assert_eq!(
                store.get(Column::Blocks, &hash).unwrap(),
                Some(b"sample-block".to_vec())
            );
            assert_eq!(
                store.get(Column::ChainState, b"tip").unwrap(),
                Some(hash.to_vec())
            );
        }
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn checked_chain_rejects_wrong_parent_and_height_without_advancing() {
        let path = std::env::temp_dir().join(format!(
            "aivoblock-checked-chain-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        let genesis = [1u8; 32];
        let child = [2u8; 32];
        {
            let store = RocksDbStore::open(&path).unwrap();
            store
                .append_checked_block(&genesis, &[0u8; 32], 0, b"genesis")
                .unwrap();
            assert!(matches!(
                store.append_checked_block(&child, &[9u8; 32], 1, b"bad-parent"),
                Err(StorageError::WrongParent)
            ));
            assert!(matches!(
                store.append_checked_block(&child, &genesis, 3, b"bad-height"),
                Err(StorageError::WrongHeight)
            ));
            assert_eq!(
                store.get(Column::ChainState, b"tip").unwrap(),
                Some(genesis.to_vec())
            );
            store
                .append_checked_block(&child, &genesis, 1, b"child")
                .unwrap();
        }
        {
            let store = RocksDbStore::open(&path).unwrap();
            assert_eq!(
                store.get(Column::ChainState, b"tip").unwrap(),
                Some(child.to_vec())
            );
            assert_eq!(
                store.get(Column::ChainState, b"height").unwrap(),
                Some(1u64.to_be_bytes().to_vec())
            );
            assert_eq!(
                store.get(Column::Blocks, &child).unwrap(),
                Some(b"child".to_vec())
            );
        }
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn namespaces_produce_distinct_keys() {
        let block = storage_key(Column::Blocks, b"id").unwrap();
        let transaction = storage_key(Column::Transactions, b"id").unwrap();
        assert_ne!(block, transaction);
    }
}
