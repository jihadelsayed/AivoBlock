//! RocksDB-backed local key-value storage for each AivoBlock node.
//!
//! Each node opens its own database directory. Storage persists bytes;
//! callers must verify consensus rules before committing chain state.
//! The initial API exposes individual writes, not atomic block commits.

use std::error::Error;
use std::fmt;
use std::path::Path;

use rocksdb::{Options, WriteBatch, DB};

use super::{encode_key, Column, KeyValueStore};

#[derive(Debug)]
pub enum StorageError {
    EmptyKey,
    InvalidBlockHash,
    Database(rocksdb::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKey => write!(f, "storage key must not be empty"),
            Self::InvalidBlockHash => write!(f, "block hash must be exactly 32 bytes"),
            Self::Database(err) => write!(f, "RocksDB error: {err}"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(err) => Some(err),
            Self::EmptyKey | Self::InvalidBlockHash => None,
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

/// Persistent database owned by a single node instance.
///
/// Open the same directory from only one live node process. Use different
/// directories for separate nodes in the private testnet.
pub struct RocksDbStore {
    db: DB,
}

impl RocksDbStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let mut options = Options::default();
        options.create_if_missing(true);
        let db = DB::open(&options, path)?;
        Ok(Self { db })
    }


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
            store.put_validated_block_and_tip(&hash, b"sample-block").unwrap();
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
    fn namespaces_produce_distinct_keys() {
        let block = storage_key(Column::Blocks, b"id").unwrap();
        let transaction = storage_key(Column::Transactions, b"id").unwrap();
        assert_ne!(block, transaction);
    }
}
