//! RocksDB-backed local key-value storage for each AivoBlock node.
//!
//! Each node opens its own database directory. Storage persists bytes;
//! callers must verify consensus rules before committing chain state.
//! The initial API exposes individual writes, not atomic block commits.

use std::error::Error;
use std::fmt;
use std::path::Path;

use rocksdb::{Options, DB};

use super::{encode_key, Column, KeyValueStore};

#[derive(Debug)]
pub enum StorageError {
    EmptyKey,
    Database(rocksdb::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKey => write!(f, "storage key must not be empty"),
            Self::Database(err) => write!(f, "RocksDB error: {err}"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(err) => Some(err),
            Self::EmptyKey => None,
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

    /// Make a sequence of changes atomically when committing a validated
    /// block. A WriteBatch-based API will be added with chain persistence.
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
    fn namespaces_produce_distinct_keys() {
        let block = storage_key(Column::Blocks, b"id").unwrap();
        let transaction = storage_key(Column::Transactions, b"id").unwrap();
        assert_ne!(block, transaction);
    }
}
