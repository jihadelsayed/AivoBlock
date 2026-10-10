//! In-memory canonical chain-head tracking for the AivoBlock node.
//!
//! This module does not verify signatures, puzzle proofs, state roots,
//! finality or fork choice. A consensus-validated header must be supplied
//! by the Haskell verification component before calling `append_verified`.
//! It deliberately does not invent a block-hashing algorithm.

use super::ChainIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainHead {
    pub hash: [u8; 32],
    pub height: u64,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    WrongChain,
    InvalidBlockHash,
    WrongParent,
    WrongHeight,
    HeightOverflow,
    NonIncreasingTimestamp,
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ChainError {}

#[derive(Debug, Clone)]
pub struct ChainState {
    identity: ChainIdentity,
    head: ChainHead,
}

impl ChainState {
    /// Initialize from an externally validated genesis commitment.
    pub fn new(identity: ChainIdentity, genesis_timestamp_ms: u64) -> Self {
        Self {
            head: ChainHead {
                hash: identity.genesis_hash,
                height: 0,
                timestamp_ms: genesis_timestamp_ms,
            },
            identity,
        }
    }

    pub fn identity(&self) -> ChainIdentity {
        self.identity
    }

    pub fn head(&self) -> ChainHead {
        self.head
    }

    /// Extend the tracked canonical tip by one already-verified block.
    ///
    /// The caller MUST perform full consensus validation beforehand.
    /// This only checks local linkage and never resolves competing forks.
    pub fn append_verified(
        &mut self,
        chain_id: u32,
        parent_hash: [u8; 32],
        candidate: ChainHead,
    ) -> Result<(), ChainError> {
        if chain_id != self.identity.chain_id {
            return Err(ChainError::WrongChain);
        }
        if candidate.hash == [0; 32] {
            return Err(ChainError::InvalidBlockHash);
        }
        if parent_hash != self.head.hash {
            return Err(ChainError::WrongParent);
        }
        let next_height = self.head.height.checked_add(1)
            .ok_or(ChainError::HeightOverflow)?;
        if candidate.height != next_height {
            return Err(ChainError::WrongHeight);
        }
        if candidate.timestamp_ms <= self.head.timestamp_ms {
            return Err(ChainError::NonIncreasingTimestamp);
        }

        self.head = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> ChainState {
        ChainState::new(ChainIdentity::new(1, [1; 32]).unwrap(), 1000)
    }

    fn next_head() -> ChainHead {
        ChainHead { hash: [2; 32], height: 1, timestamp_ms: 1001 }
    }

    #[test]
    fn initializes_at_genesis() {
        assert_eq!(chain().head().height, 0);
        assert_eq!(chain().head().hash, [1; 32]);
    }

    #[test]
    fn extends_a_prevalidated_head() {
        let mut state = chain();
        state.append_verified(1, [1; 32], next_head()).unwrap();
        assert_eq!(state.head(), next_head());
    }

    #[test]
    fn rejects_invalid_links_without_mutation() {
        let cases = [
            (2, [1; 32], next_head(), ChainError::WrongChain),
            (1, [3; 32], next_head(), ChainError::WrongParent),
            (1, [1; 32], ChainHead { height: 2, ..next_head() }, ChainError::WrongHeight),
            (1, [1; 32], ChainHead { timestamp_ms: 1000, ..next_head() }, ChainError::NonIncreasingTimestamp),
            (1, [1; 32], ChainHead { hash: [0; 32], ..next_head() }, ChainError::InvalidBlockHash),
        ];
        for (id, parent, candidate, expected) in cases {
            let mut state = chain();
            assert_eq!(state.append_verified(id, parent, candidate), Err(expected));
            assert_eq!(state.head().height, 0);
        }
    }
}
