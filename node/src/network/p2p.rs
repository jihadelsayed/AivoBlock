//! Local peer registry for AivoBlock's future libp2p transport.
//!
//! This is bookkeeping only: it does not connect peers, authenticate
//! identities, discover nodes, or synchronize blocks.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use super::DEFAULT_MAX_PEERS;

/// Fixed-size, protocol-neutral peer identity for the prototype.
/// A real libp2p PeerId binding will be specified with the wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId([u8; 32]);

impl PeerId {
    pub fn new(bytes: [u8; 32]) -> Result<Self, PeerRegistryError> {
        if bytes == [0; 32] {
            return Err(PeerRegistryError::InvalidIdentity);
        }
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerRegistryError {
    InvalidIdentity,
    DuplicatePeer,
    UnknownPeer,
    CapacityReached,
    InvalidCapacity,
}

#[derive(Debug, Clone, Default)]
pub struct PeerRegistry {
    peers: BTreeMap<PeerId, SocketAddr>,
    max_peers: usize,
}

impl PeerRegistry {
    pub fn new(max_peers: usize) -> Result<Self, PeerRegistryError> {
        if max_peers == 0 {
            return Err(PeerRegistryError::InvalidCapacity);
        }
        Ok(Self {
            peers: BTreeMap::new(),
            max_peers,
        })
    }

    pub fn with_default_limit() -> Self {
        Self::new(DEFAULT_MAX_PEERS).expect("default peer limit is nonzero")
    }

    pub fn register(
        &mut self,
        id: PeerId,
        address: SocketAddr,
    ) -> Result<(), PeerRegistryError> {
        if self.peers.contains_key(&id) {
            return Err(PeerRegistryError::DuplicatePeer);
        }
        if self.peers.len() >= self.max_peers {
            return Err(PeerRegistryError::CapacityReached);
        }
        self.peers.insert(id, address);
        Ok(())
    }

    pub fn unregister(&mut self, id: &PeerId) -> Result<SocketAddr, PeerRegistryError> {
        self.peers.remove(id).ok_or(PeerRegistryError::UnknownPeer)
    }

    pub fn address_of(&self, id: &PeerId) -> Option<SocketAddr> {
        self.peers.get(id).copied()
    }

    pub fn contains(&self, id: &PeerId) -> bool {
        self.peers.contains_key(id)
    }

    pub fn count(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(value: u8) -> PeerId {
        PeerId::new([value; 32]).unwrap()
    }

    fn address(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    #[test]
    fn rejects_zero_peer_identity() {
        assert_eq!(PeerId::new([0; 32]), Err(PeerRegistryError::InvalidIdentity));
    }

    #[test]
    fn registers_and_removes_peers() {
        let mut registry = PeerRegistry::new(2).unwrap();
        registry.register(peer(1), address(30333)).unwrap();
        assert_eq!(registry.address_of(&peer(1)), Some(address(30333)));
        assert!(registry.contains(&peer(1)));
        assert_eq!(registry.unregister(&peer(1)), Ok(address(30333)));
        assert!(registry.is_empty());
    }

    #[test]
    fn rejects_duplicates_and_enforces_capacity() {
        let mut registry = PeerRegistry::new(1).unwrap();
        registry.register(peer(1), address(30333)).unwrap();
        assert_eq!(
            registry.register(peer(1), address(30334)),
            Err(PeerRegistryError::DuplicatePeer)
        );
        assert_eq!(
            registry.register(peer(2), address(30334)),
            Err(PeerRegistryError::CapacityReached)
        );
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn rejects_invalid_capacity_and_unknown_peer() {
        assert!(matches!(
            PeerRegistry::new(0),
            Err(PeerRegistryError::InvalidCapacity)
        ));
        let mut registry = PeerRegistry::with_default_limit();
        assert_eq!(
            registry.unregister(&peer(1)),
            Err(PeerRegistryError::UnknownPeer)
        );
    }
}
