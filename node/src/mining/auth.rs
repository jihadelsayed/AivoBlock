//! Ed25519-authenticated local work assignments.
//!
//! A trusted chain configuration must supply the authorized issuer key.
//! This verifier does not establish decentralized authorization, worker
//! possession of its key, proof validity, finality, or token rewards.

use std::collections::BTreeSet;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use super::scheduler::Assignment;
use super::DEFAULT_MAX_BATCH_UNITS;

const DOMAIN: &[u8] = b"AIVOBLOCK-SIGNED-WORK-ASSIGNMENT-V1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedAssignment {
    pub chain_id: u32,
    pub assignment: Assignment,
    /// Worker identity is its 32-byte Ed25519 public key.
    pub worker_public_key: [u8; 32],
    pub issuer_public_key: [u8; 32],
    pub deadline_height: u64,
    pub nonce: u64,
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentAuthError {
    InvalidChain,
    UnauthorizedIssuer,
    InvalidWorkerIdentity,
    InvalidRange,
    Expired,
    InvalidIssuerKey,
    InvalidSignature,
    ReplayedNonce,
}

/// Canonical fixed-width, domain-separated bytes. Never sign JSON formatting.
pub fn assignment_signing_bytes(signed: &SignedAssignment) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(DOMAIN.len() + 4 + 32 * 3 + 8 * 4);
    bytes.extend_from_slice(DOMAIN);
    bytes.extend_from_slice(&signed.chain_id.to_be_bytes());
    bytes.extend_from_slice(&signed.issuer_public_key);
    bytes.extend_from_slice(&signed.worker_public_key);
    bytes.extend_from_slice(&signed.assignment.puzzle_id);
    bytes.extend_from_slice(&signed.assignment.range.start.to_be_bytes());
    bytes.extend_from_slice(&signed.assignment.range.count.to_be_bytes());
    bytes.extend_from_slice(&signed.deadline_height.to_be_bytes());
    bytes.extend_from_slice(&signed.nonce.to_be_bytes());
    bytes
}

/// Nonce tracking is in-memory only; production replay protection requires
/// persistent chain-state binding and a specified issuer rotation policy.
#[derive(Debug, Default)]
pub struct AssignmentAuthorizer {
    seen_nonces: BTreeSet<([u8; 32], u64)>,
}

impl AssignmentAuthorizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn verify_and_record(
        &mut self,
        signed: &SignedAssignment,
        expected_chain_id: u32,
        authorized_issuer: [u8; 32],
        current_height: u64,
    ) -> Result<Assignment, AssignmentAuthError> {
        if expected_chain_id == 0 || signed.chain_id != expected_chain_id {
            return Err(AssignmentAuthError::InvalidChain);
        }
        if signed.issuer_public_key != authorized_issuer {
            return Err(AssignmentAuthError::UnauthorizedIssuer);
        }
        if signed.worker_public_key == [0; 32]
            || signed.assignment.worker_id != signed.worker_public_key
        {
            return Err(AssignmentAuthError::InvalidWorkerIdentity);
        }
        if signed.assignment.puzzle_id == [0; 32]
            || signed.assignment.range.count == 0
            || signed.assignment.range.count > DEFAULT_MAX_BATCH_UNITS
            || signed
                .assignment
                .range
                .start
                .checked_add(signed.assignment.range.count)
                .is_none()
        {
            return Err(AssignmentAuthError::InvalidRange);
        }
        if current_height > signed.deadline_height {
            return Err(AssignmentAuthError::Expired);
        }
        let key = VerifyingKey::from_bytes(&signed.issuer_public_key)
            .map_err(|_| AssignmentAuthError::InvalidIssuerKey)?;
        let signature = Signature::from_bytes(&signed.signature);
        key.verify(&assignment_signing_bytes(signed), &signature)
            .map_err(|_| AssignmentAuthError::InvalidSignature)?;
        if !self
            .seen_nonces
            .insert((signed.issuer_public_key, signed.nonce))
        {
            return Err(AssignmentAuthError::ReplayedNonce);
        }
        Ok(signed.assignment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed_fixture() -> (SigningKey, SignedAssignment) {
        let key = SigningKey::from_bytes(&[7; 32]);
        let worker = SigningKey::from_bytes(&[9; 32]).verifying_key().to_bytes();
        let mut signed = SignedAssignment {
            chain_id: 1,
            assignment: Assignment {
                puzzle_id: [1; 32],
                worker_id: worker,
                range: super::super::scheduler::WorkRange { start: 0, count: 2 },
            },
            worker_public_key: worker,
            issuer_public_key: key.verifying_key().to_bytes(),
            deadline_height: 100,
            nonce: 17,
            signature: [0; 64],
        };
        signed.signature = key.sign(&assignment_signing_bytes(&signed)).to_bytes();
        (key, signed)
    }

    #[test]
    fn valid_signature_binds_worker_puzzle_range_chain_and_deadline() {
        let (_, signed) = signed_fixture();
        let mut auth = AssignmentAuthorizer::new();
        assert_eq!(
            auth.verify_and_record(&signed, 1, signed.issuer_public_key, 100),
            Ok(signed.assignment)
        );
    }

    #[test]
    fn rejects_tampered_fields() {
        let (_, original) = signed_fixture();
        let variants = [
            SignedAssignment {
                chain_id: 2,
                ..original
            },
            SignedAssignment {
                worker_public_key: [5; 32],
                ..original
            },
            SignedAssignment {
                assignment: Assignment {
                    range: super::super::scheduler::WorkRange { start: 1, count: 2 },
                    ..original.assignment
                },
                ..original
            },
            SignedAssignment {
                deadline_height: 101,
                ..original
            },
            SignedAssignment {
                nonce: 18,
                ..original
            },
        ];
        for candidate in variants {
            assert!(AssignmentAuthorizer::new()
                .verify_and_record(&candidate, 1, original.issuer_public_key, 99)
                .is_err());
        }
    }

    #[test]
    fn rejects_wrong_issuer_expiration_and_replay() {
        let (_, signed) = signed_fixture();
        let mut auth = AssignmentAuthorizer::new();
        assert_eq!(
            auth.verify_and_record(&signed, 1, [4; 32], 1),
            Err(AssignmentAuthError::UnauthorizedIssuer)
        );
        assert_eq!(
            auth.verify_and_record(&signed, 1, signed.issuer_public_key, 101),
            Err(AssignmentAuthError::Expired)
        );
        auth.verify_and_record(&signed, 1, signed.issuer_public_key, 10)
            .unwrap();
        assert_eq!(
            auth.verify_and_record(&signed, 1, signed.issuer_public_key, 10),
            Err(AssignmentAuthError::ReplayedNonce)
        );
    }

    #[test]
    fn rejects_oversized_assignment_even_when_signed() {
        let (key, mut signed) = signed_fixture();
        signed.assignment.range.count = DEFAULT_MAX_BATCH_UNITS + 1;
        signed.signature = key.sign(&assignment_signing_bytes(&signed)).to_bytes();
        assert_eq!(
            AssignmentAuthorizer::new().verify_and_record(
                &signed,
                1,
                signed.issuer_public_key,
                10,
            ),
            Err(AssignmentAuthError::InvalidRange)
        );
    }

    #[test]
    fn invalid_signature_does_not_consume_nonce() {
        let (_, signed) = signed_fixture();
        let mut auth = AssignmentAuthorizer::new();
        let mut invalid = signed;
        invalid.signature[0] ^= 1;
        assert_eq!(
            auth.verify_and_record(&invalid, 1, signed.issuer_public_key, 10),
            Err(AssignmentAuthError::InvalidSignature)
        );
        assert_eq!(
            auth.verify_and_record(&signed, 1, signed.issuer_public_key, 10),
            Ok(signed.assignment)
        );
    }
}
