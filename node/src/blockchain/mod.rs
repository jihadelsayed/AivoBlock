//! Blockchain subsystem for the AivoBlock Rust node.
//!
//! The Haskell core is responsible for consensus-critical validation.
//! Rust coordinates validated blocks, chain state, storage and networking.
//! Never treat a locally received block as valid before independent checks.

pub mod chain;

pub use chain::{ChainError, ChainHead, ChainState};

/// Protocol parameters required for identifying a distinct chain.
///
/// This is metadata, not a complete set of consensus rules. The genesis
/// commitment must be fixed by the finalized protocol specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainIdentity {
    pub chain_id: u32,
    pub genesis_hash: [u8; 32],
}

impl ChainIdentity {
    pub fn new(chain_id: u32, genesis_hash: [u8; 32]) -> Result<Self, &'static str> {
        if chain_id == 0 {
            return Err("Chain ID must be nonzero");
        }
        if genesis_hash == [0; 32] {
            return Err("Genesis hash must not be zero");
        }
        Ok(Self {
            chain_id,
            genesis_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_chain_identity() {
        assert!(ChainIdentity::new(0, [1; 32]).is_err());
        assert!(ChainIdentity::new(1, [0; 32]).is_err());
    }

    #[test]
    fn accepts_nonzero_chain_identity() {
        let id = ChainIdentity::new(1, [7; 32]).unwrap();
        assert_eq!(id.chain_id, 1);
        assert_eq!(id.genesis_hash, [7; 32]);
    }
}
