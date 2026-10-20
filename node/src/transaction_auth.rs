//! Candidate V1 Ed25519 transfer authentication (not consensus).
//!
//! Native 32-byte accounts are raw Ed25519 verification keys in this
//! explicitly versioned prototype. An address cannot spend another key's
//! funds. Every signing field is bound to fixed-width domain-separated bytes.

use ed25519_dalek::{Signature, VerifyingKey};

const DOMAIN: &[u8] = b"AIVOBLOCK-TRANSFER-V1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedTransferV1 {
    pub chain_id: u32,
    pub version: u32,
    pub sender: [u8; 32],
    pub recipient: [u8; 32],
    pub amount: u64,
    pub fee: u64,
    pub nonce: u64,
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferAuthError {
    InvalidVersion,
    InvalidChain,
    ZeroAmount,
    SenderKeyMismatch,
    InvalidPublicKey,
    InvalidSignature,
}

/// 118 bytes: domain+NUL | version u32 | chain u32 | sender's public key
/// | recipient[32] | amount u64 | fee u64 | nonce u64. Big endian integers.
/// Omitted references and any non-V1 fields are forbidden for this codec.
pub fn transfer_signing_bytes_v1(tx: &SignedTransferV1) -> Result<Vec<u8>, TransferAuthError> {
    if tx.version != 1 {
        return Err(TransferAuthError::InvalidVersion);
    }
    if tx.chain_id != 1 {
        return Err(TransferAuthError::InvalidChain);
    }
    if tx.amount == 0 {
        return Err(TransferAuthError::ZeroAmount);
    }
    if tx.sender != tx.public_key {
        return Err(TransferAuthError::SenderKeyMismatch);
    }
    let mut bytes = Vec::with_capacity(DOMAIN.len() + 4 + 4 + 32 + 32 + 8 * 3);
    bytes.extend_from_slice(DOMAIN);
    bytes.extend_from_slice(&tx.version.to_be_bytes());
    bytes.extend_from_slice(&tx.chain_id.to_be_bytes());
    bytes.extend_from_slice(&tx.sender);
    bytes.extend_from_slice(&tx.recipient);
    bytes.extend_from_slice(&tx.amount.to_be_bytes());
    bytes.extend_from_slice(&tx.fee.to_be_bytes());
    bytes.extend_from_slice(&tx.nonce.to_be_bytes());
    Ok(bytes)
}

/// Checks key ownership and a canonical Ed25519 signature before invoking
/// the Haskell ledger. Haskell independently repeats these checks.
pub fn verify_signed_transfer_v1(tx: &SignedTransferV1) -> Result<(), TransferAuthError> {
    let payload = transfer_signing_bytes_v1(tx)?;
    let key = VerifyingKey::from_bytes(&tx.public_key)
        .map_err(|_| TransferAuthError::InvalidPublicKey)?;
    let signature = Signature::from_bytes(&tx.signature);
    key.verify_strict(&payload, &signature)
        .map_err(|_| TransferAuthError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed(amount: u64, fee: u64, nonce: u64) -> SignedTransferV1 {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public_key = key.verifying_key().to_bytes();
        let mut tx = SignedTransferV1 {
            chain_id: 1,
            version: 1,
            sender: public_key,
            public_key,
            recipient: [2; 32],
            amount,
            fee,
            nonce,
            signature: [0; 64],
        };
        tx.signature = key
            .sign(&transfer_signing_bytes_v1(&tx).unwrap())
            .to_bytes();
        tx
    }

    #[test]
    fn canonical_vector_matches_haskell_fixture() {
        let tx = signed(10, 2, 0);
        assert_eq!(transfer_signing_bytes_v1(&tx).unwrap().len(), 118);
        assert_eq!(
            hex(&tx.sender),
            "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"
        );
        assert_eq!(
            hex(&tx.signature),
            "f1cc31b91b3485d813bbe245ee742795eb94dcd7cfdba90d30d1a4da9ec18ca9822a6ef0a36c54a58a956de172eee42816ec51b2a0dfbcff7422d62deb952405"
        );
        verify_signed_transfer_v1(&tx).unwrap();
    }

    #[test]
    fn rejects_tampering_wrong_owner_and_forged_signatures() {
        let tx = signed(10, 2, 0);
        for altered in [
            SignedTransferV1 { amount: 11, ..tx },
            SignedTransferV1 { fee: 3, ..tx },
            SignedTransferV1 { nonce: 1, ..tx },
            SignedTransferV1 {
                recipient: [3; 32],
                ..tx
            },
        ] {
            assert_eq!(
                verify_signed_transfer_v1(&altered),
                Err(TransferAuthError::InvalidSignature)
            );
        }
        assert_eq!(
            verify_signed_transfer_v1(&SignedTransferV1 {
                sender: [4; 32],
                ..tx
            }),
            Err(TransferAuthError::SenderKeyMismatch)
        );
        assert_eq!(
            verify_signed_transfer_v1(&SignedTransferV1 {
                signature: [0; 64],
                ..tx
            }),
            Err(TransferAuthError::InvalidSignature)
        );
        assert_eq!(
            verify_signed_transfer_v1(&SignedTransferV1 { chain_id: 2, ..tx }),
            Err(TransferAuthError::InvalidChain)
        );
        assert_eq!(
            verify_signed_transfer_v1(&SignedTransferV1 { version: 2, ..tx }),
            Err(TransferAuthError::InvalidVersion)
        );
        assert_eq!(
            verify_signed_transfer_v1(&SignedTransferV1 { amount: 0, ..tx }),
            Err(TransferAuthError::ZeroAmount)
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
