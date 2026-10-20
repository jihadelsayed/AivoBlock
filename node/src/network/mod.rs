//! Networking subsystem for independently operated AivoBlock nodes.
//!
//! This module defines protocol-level networking limits and exposes the
//! peer-management primitives in `p2p`. A transport, peer discovery,
//! encryption, and gossip implementation will be added separately.
//! Networking must never decide whether a block or AI proof is valid.

pub mod handshake;
pub mod p2p;

/// Identifies the wire protocol spoken by compatible nodes.
/// Increment when making incompatible network-message changes.
pub const NETWORK_PROTOCOL_VERSION: u32 = 1;

/// Upper bound for one incoming encoded message, before decoding.
/// This protects against unrestricted allocations from remote peers.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// Upper bound for simultaneous registered peers in the prototype.
pub const DEFAULT_MAX_PEERS: usize = 64;

/// Validate message framing limits before decoding an untrusted payload.
pub fn check_message_size(size: usize) -> Result<(), &'static str> {
    if size == 0 {
        return Err("Empty network message");
    }
    if size > MAX_MESSAGE_BYTES {
        return Err("Network message exceeds size limit");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_size_bounds() {
        assert!(check_message_size(1).is_ok());
        assert!(check_message_size(MAX_MESSAGE_BYTES).is_ok());
        assert!(check_message_size(0).is_err());
        assert!(check_message_size(MAX_MESSAGE_BYTES + 1).is_err());
    }
}
