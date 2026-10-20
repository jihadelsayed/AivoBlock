//! Local persistent storage abstractions for an AivoBlock node.
//!
//! Storage is owned by each node independently. Persistence must not
//! substitute for cryptographic verification or consensus validation.

pub mod ledger_state;
pub mod rocksdb;

pub use ledger_state::{LedgerState, LocalAccount};
pub use rocksdb::RocksDbStore;

/// Namespaces prevent keys for different record types from colliding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Blocks,
    Headers,
    ChainState,
    Transactions,
    PuzzleProofs,
    Metadata,
}

impl Column {
    pub const fn prefix(self) -> u8 {
        match self {
            Self::Blocks => 1,
            Self::Headers => 2,
            Self::ChainState => 3,
            Self::Transactions => 4,
            Self::PuzzleProofs => 5,
            Self::Metadata => 6,
        }
    }
}

/// Construct a prefixed storage key. Empty user keys are not permitted.
pub fn encode_key(column: Column, key: &[u8]) -> Result<Vec<u8>, &'static str> {
    if key.is_empty() {
        return Err("Storage key must not be empty");
    }
    let mut encoded = Vec::with_capacity(key.len() + 1);
    encoded.push(column.prefix());
    encoded.extend_from_slice(key);
    Ok(encoded)
}

/// Interface for local key-value persistence.
///
/// Implementations must document their crash consistency and atomicity.
/// A successful write is not proof that the stored blockchain data is valid.
pub trait KeyValueStore {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get(&self, column: Column, key: &[u8]) -> Result<Option<Vec<u8>>, Self::Error>;
    fn put(&self, column: Column, key: &[u8], value: &[u8]) -> Result<(), Self::Error>;
    fn delete(&self, column: Column, key: &[u8]) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod atomic_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_namespaces_do_not_collide() {
        let block = encode_key(Column::Blocks, b"abc").unwrap();
        let header = encode_key(Column::Headers, b"abc").unwrap();
        assert_ne!(block, header);
        assert_eq!(block, vec![1, b'a', b'b', b'c']);
    }

    #[test]
    fn empty_keys_are_rejected() {
        assert!(encode_key(Column::Metadata, b"").is_err());
    }
}
