//! Fixed-validator-set, Ed25519-authenticated local quorum certificates.
//! This is an independently checked PRECOMMIT CERTIFICATE PRIMITIVE, not a
//! BFT state machine, fork-choice algorithm, durable vote lock, or finality.
//!
//! Validator membership and weights are supplied by an external trusted
//! configuration. A block self-declaring its own validator set is NOT trusted.

use std::collections::BTreeMap;

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

const VALIDATOR_SET_DOMAIN: &[u8] = b"AIVOBLOCK-VALIDATOR-SET-V1\0";
const PRECOMMIT_DOMAIN: &[u8] = b"AIVOBLOCK-PRECOMMIT-V1\0";
const MAX_VALIDATORS: usize = 64;
const CHAIN_ID: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatorMemberV1 {
    pub public_key: [u8; 32],
    pub voting_power: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedPrecommitV1 {
    pub public_key: [u8; 32],
    pub signature: [u8; 64],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuorumCertificateV1 {
    pub chain_id: u32,
    pub height: u64,
    pub round: u64,
    pub block_id: [u8; 32],
    pub validator_set_commitment: [u8; 32],
    pub votes: Vec<SignedPrecommitV1>,
}

#[derive(Debug)]
pub struct TrustedValidatorSetV1 {
    members: BTreeMap<[u8; 32], u64>,
    total_power: u64,
    commitment: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedQuorumV1 {
    pub height: u64,
    pub round: u64,
    pub block_id: [u8; 32],
    pub signed_power: u64,
    pub required_power: u64,
    pub validator_set_commitment: [u8; 32],
}

/// Constructed ONLY from independently trusted configuration. Reject
/// duplicate keys, zero voting power, total power overflow and invalid
/// public keys. Maps yield canonical lexicographic key ordering.
impl TrustedValidatorSetV1 {
    pub fn new(members: &[ValidatorMemberV1]) -> Result<Self, String> {
        if members.is_empty() || members.len() > MAX_VALIDATORS {
            return Err("Expected 1..64 configured validators".into());
        }
        let mut by_key = BTreeMap::new();
        let mut sum = 0u64;
        for validator in members {
            if validator.voting_power == 0 || validator.public_key == [0; 32] {
                return Err("Validator identity and power must be nonzero".into());
            }
            VerifyingKey::from_bytes(&validator.public_key)
                .map_err(|_| "Malformed validator Ed25519 key")?;
            sum = sum.checked_add(validator.voting_power)
                .ok_or("Total validator power overflows u64")?;
            if by_key.insert(validator.public_key, validator.voting_power).is_some() {
                return Err("Duplicate validator public key".into());
            }
        }
        let mut digest = Sha256::new();
        digest.update(VALIDATOR_SET_DOMAIN);
        digest.update(CHAIN_ID.to_be_bytes());
        digest.update((by_key.len() as u32).to_be_bytes());
        for (public_key, power) in &by_key {
            digest.update(public_key);
            digest.update(power.to_be_bytes());
        }
        Ok(Self {
            members: by_key,
            total_power: sum,
            commitment: digest.finalize().into(),
        })
    }

    pub fn commitment(&self) -> [u8; 32] {
        self.commitment
    }

    pub fn total_power(&self) -> u64 {
        self.total_power
    }

    /// Same formula as Haskell AivoBlock.Consensus.requiredQuorumPower:
    /// floor(2*T/3)+1. Use u128 to avoid overflow at u64::MAX.
    pub fn required_power(&self) -> u64 {
        ((u128::from(self.total_power) * 2) / 3 + 1) as u64
    }
}

/// Each vote signs its own identity as well as the validator set, exact
/// block ID, chain ID, height and round. This forbids cross-round,
/// cross-set, cross-chain and cross-block signature replay.
pub fn precommit_signing_bytes_v1(
    certificate: &QuorumCertificateV1,
    signer: &[u8; 32],
) -> Result<Vec<u8>, String> {
    if certificate.chain_id != CHAIN_ID
        || certificate.height == 0
        || certificate.block_id == [0; 32]
        || *signer == [0; 32]
    {
        return Err("Invalid precommit chain, height, signer or block".into());
    }
    let mut raw = Vec::with_capacity(PRECOMMIT_DOMAIN.len() + 4 + 32 * 3 + 8 * 2);
    raw.extend_from_slice(PRECOMMIT_DOMAIN);
    raw.extend_from_slice(&certificate.chain_id.to_be_bytes());
    raw.extend_from_slice(&certificate.validator_set_commitment);
    raw.extend_from_slice(&certificate.height.to_be_bytes());
    raw.extend_from_slice(&certificate.round.to_be_bytes());
    raw.extend_from_slice(&certificate.block_id);
    raw.extend_from_slice(signer);
    Ok(raw)
}

/// Fail closed: *every* provided vote must be valid and members must be
/// strictly sorted by public key to enforce one canonical certificate.
pub fn verify_quorum_certificate_v1(
    validators: &TrustedValidatorSetV1,
    certificate: &QuorumCertificateV1,
) -> Result<VerifiedQuorumV1, String> {
    if certificate.validator_set_commitment != validators.commitment {
        return Err("Quorum certificate uses an unauthorized validator set".into());
    }
    if certificate.votes.is_empty() || certificate.votes.len() > MAX_VALIDATORS {
        return Err("Missing or oversized precommit certificate".into());
    }
    let mut previous: Option<[u8; 32]> = None;
    let mut signed_power = 0u64;
    for vote in &certificate.votes {
        if previous.is_some_and(|key| key >= vote.public_key) {
            return Err("Duplicate, conflicting or unordered validator votes".into());
        }
        previous = Some(vote.public_key);
        let power = *validators.members.get(&vote.public_key)
            .ok_or("Precommit from unknown validator")?;
        let pub_key = VerifyingKey::from_bytes(&vote.public_key)
            .map_err(|_| "Invalid validator signature key")?;
        let payload = precommit_signing_bytes_v1(certificate, &vote.public_key)?;
        pub_key.verify_strict(&payload, &Signature::from_bytes(&vote.signature))
            .map_err(|_| "Invalid Ed25519 precommit signature")?;
        signed_power = signed_power.checked_add(power)
            .ok_or("Precommit power overflow")?;
    }
    let required = validators.required_power();
    if signed_power < required {
        return Err("Precommit certificate lacks strict two-thirds quorum".into());
    }
    Ok(VerifiedQuorumV1 {
        height: certificate.height,
        round: certificate.round,
        block_id: certificate.block_id,
        signed_power,
        required_power: required,
        validator_set_commitment: validators.commitment,
    })
}

/// Bind a quorum certificate to an *already independently replay-validated*
/// local tip. The report must originate from signed-genesis chain replay;
/// passing a fabricated report or using this without validating history
/// provides no proof of a valid block or consensus finality.
pub fn verify_audited_tip_certificate_v1(
    db: &crate::storage::RocksDbStore,
    report: &crate::chain_replay::ChainReplayReport,
    validators: &TrustedValidatorSetV1,
    certificate: &QuorumCertificateV1,
) -> Result<VerifiedQuorumV1, String> {
    use crate::storage::{Column, KeyValueStore};
    let confirmed = verify_quorum_certificate_v1(validators, certificate)?;
    if confirmed.block_id != report.tip_id {
        return Err("Certificate does not attest the independently replayed tip".into());
    }
    let stored = db.get(Column::ChainState, b"tip").map_err(|e| e.to_string())?
        .ok_or("Missing local tip metadata")?;
    if stored.as_slice() != confirmed.block_id {
        return Err("Local tip differs from audited certificate".into());
    }
    let height = db.get(Column::ChainState, b"height").map_err(|e| e.to_string())?
        .ok_or("Missing local height metadata")?;
    if height.as_slice() != confirmed.height.to_be_bytes() {
        return Err("Certificate height differs from replayed chain height".into());
    }
    if db.get(Column::Blocks, &confirmed.block_id).map_err(|e| e.to_string())?.is_none() {
        return Err("Certified block is missing from local storage".into());
    }
    Ok(confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn fixture(weights: &[u64]) -> (Vec<SigningKey>, TrustedValidatorSetV1) {
        let signers: Vec<SigningKey> = (1..=weights.len())
            .map(|i| SigningKey::from_bytes(&[i as u8; 32]))
            .collect();
        let members: Vec<ValidatorMemberV1> = signers.iter().zip(weights).map(|(key,power)| {
            ValidatorMemberV1 {
                public_key: key.verifying_key().to_bytes(),
                voting_power: *power,
            }
        }).collect();
        (signers, TrustedValidatorSetV1::new(&members).unwrap())
    }

    fn certificate(
        signers: &[SigningKey],
        set: &TrustedValidatorSetV1,
        height: u64,
        round: u64,
        block_id: [u8; 32],
    ) -> QuorumCertificateV1 {
        let mut cert = QuorumCertificateV1 {
            chain_id: CHAIN_ID,
            height,
            round,
            block_id,
            validator_set_commitment: set.commitment(),
            votes: Vec::new(),
        };
        for signer in signers {
            let key = signer.verifying_key().to_bytes();
            let signature = signer.sign(&precommit_signing_bytes_v1(&cert, &key).unwrap()).to_bytes();
            cert.votes.push(SignedPrecommitV1 { public_key: key, signature });
        }
        cert.votes.sort_by_key(|vote| vote.public_key);
        cert
    }

    #[test]
    fn quorum_weight_matches_haskell_rule() {
        for powers in [
            vec![1,1,1], vec![3,2,1], vec![2,3,3],
            vec![u64::MAX], vec![u64::MAX - 1, 1]
        ] {
            let (_, set) = fixture(&powers);
            let expected = (u128::from(set.total_power()) * 2) / 3 + 1;
            assert_eq!(u128::from(set.required_power()), expected);
        }
    }

    #[test]
    fn signed_quorum_allows_only_authorized_distinct_voters() {
        let (signers, set) = fixture(&[3,2,1]); // total 6, quorum 5
        let all = certificate(&signers, &set, 7, 2, [9; 32]);
        assert_eq!(verify_quorum_certificate_v1(&set, &all).unwrap().signed_power, 6);
        let mut weighted: QuorumCertificateV1 = all.clone();
        weighted.votes.retain(|v| {
            set.members.get(&v.public_key).is_some_and(|power| *power >= 2)
        });
        assert_eq!(verify_quorum_certificate_v1(&set, &weighted).unwrap().signed_power, 5);
        let mut minority = all.clone();
        minority.votes.retain(|v| {
            set.members.get(&v.public_key).is_some_and(|power| *power != 2)
        });
        assert!(verify_quorum_certificate_v1(&set, &minority).is_err());
        let mut impostor = all.clone();
        impostor.votes[0].signature[0] ^= 1;
        assert!(verify_quorum_certificate_v1(&set, &impostor).is_err());
        let mut duplicate = all.clone();
        duplicate.votes.insert(1, duplicate.votes[0]);
        assert!(verify_quorum_certificate_v1(&set, &duplicate).is_err());
        let mut reordered = all.clone();
        reordered.votes.reverse();
        assert!(verify_quorum_certificate_v1(&set, &reordered).is_err());
        let stranger = SigningKey::from_bytes(&[42; 32]);
        let wrong = certificate(&[stranger], &set, 7, 2, [9; 32]);
        assert!(verify_quorum_certificate_v1(&set, &wrong).is_err());
    }

    #[test]
    fn signatures_are_bound_to_height_round_block_chain_and_set() {
        let (keys, set) = fixture(&[1,1,1]);
        let cert = certificate(&keys, &set, 1, 0, [7; 32]);
        assert_eq!(verify_quorum_certificate_v1(&set, &cert).unwrap().signed_power, 3);
        let mut changed = cert.clone();
        changed.block_id = [8; 32];
        assert!(verify_quorum_certificate_v1(&set, &changed).is_err());
        changed = cert.clone();
        changed.height = 2;
        assert!(verify_quorum_certificate_v1(&set, &changed).is_err());
        changed = cert.clone();
        changed.round = 1;
        assert!(verify_quorum_certificate_v1(&set, &changed).is_err());
        changed = cert.clone();
        changed.chain_id = 2;
        assert!(verify_quorum_certificate_v1(&set, &changed).is_err());
        changed = cert.clone();
        changed.validator_set_commitment[0] ^= 1;
        assert!(verify_quorum_certificate_v1(&set, &changed).is_err());
        let (_, different) = fixture(&[1,1,2]);
        assert!(verify_quorum_certificate_v1(&different, &cert).is_err());
    }

    #[test]
    fn validator_config_rejects_duplicates_zero_power_and_overflow() {
        let key = SigningKey::from_bytes(&[1; 32]).verifying_key().to_bytes();
        let another = SigningKey::from_bytes(&[2; 32]).verifying_key().to_bytes();
        assert!(TrustedValidatorSetV1::new(&[]).is_err());
        assert!(TrustedValidatorSetV1::new(&[ValidatorMemberV1 {
            public_key: key, voting_power: 0
        }]).is_err());
        assert!(TrustedValidatorSetV1::new(&[
            ValidatorMemberV1 { public_key: key, voting_power: 1 },
            ValidatorMemberV1 { public_key: key, voting_power: 1 },
        ]).is_err());
        assert!(TrustedValidatorSetV1::new(&[
            ValidatorMemberV1 { public_key: key, voting_power: u64::MAX },
            ValidatorMemberV1 { public_key: another, voting_power: 1 },
        ]).is_err());
        assert!(TrustedValidatorSetV1::new(&[ValidatorMemberV1 {
            public_key: [0; 32], voting_power: 1,
        }]).is_err());
    }
}
