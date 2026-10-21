//! Bounded per-height consensus bookkeeping.
//!
//! Authenticated validators are not assumed honest: a Byzantine validator
//! (less than one third of the voting power) can sign unlimited distinct
//! precommits, re-send proposals, or vote in far-future rounds. These
//! structures bound what such a validator can make an honest node store or
//! re-execute, without changing what honest validators do.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::validator_certificate::{SignedPrecommitV1, TrustedValidatorSetV1};

/// Precommit rounds retained per validator and height. The lowest round is
/// evicted first: honest validators converge on recent rounds.
pub const MAX_VOTE_ROUNDS_PER_VALIDATOR: usize = 16;
/// Distinct block bodies one round leader may make this node execute per
/// height. Honest re-proposals of a locked block are byte-identical and are
/// answered from the cache without re-execution.
pub const MAX_DISTINCT_PROPOSALS_PER_LEADER: usize = 4;
/// Cached execution outcomes per height (operator injections included).
const MAX_REMEMBERED_PROPOSALS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteAdmission {
    Fresh,
    Duplicate,
    /// Older than every retained round of a validator with a full history.
    Stale,
    /// A second block from one validator at one height. Honest validators
    /// hold a durable one-block-per-height vote lock, so this is
    /// equivocation; the first block seen is kept.
    Equivocation([u8; 32]),
}

struct ValidatorVotes {
    block: [u8; 32],
    rounds: BTreeMap<u64, SignedPrecommitV1>,
}

/// Signature-verified precommits for the next height: one block and at most
/// `MAX_VOTE_ROUNDS_PER_VALIDATOR` rounds per validator.
#[derive(Default)]
pub struct VoteBook {
    validators: BTreeMap<[u8; 32], ValidatorVotes>,
}

impl VoteBook {
    /// `vote` must already be verified for (height, `round`, `block`).
    pub fn insert(
        &mut self,
        round: u64,
        block: [u8; 32],
        vote: SignedPrecommitV1,
    ) -> VoteAdmission {
        let entry = self
            .validators
            .entry(vote.public_key)
            .or_insert_with(|| ValidatorVotes {
                block,
                rounds: BTreeMap::new(),
            });
        if entry.block != block {
            return VoteAdmission::Equivocation(entry.block);
        }
        if entry.rounds.contains_key(&round) {
            return VoteAdmission::Duplicate;
        }
        let full = entry.rounds.len() >= MAX_VOTE_ROUNDS_PER_VALIDATOR;
        let lowest = entry.rounds.keys().next().copied();
        if full && lowest.is_some_and(|lowest| round < lowest) {
            return VoteAdmission::Stale;
        }
        entry.rounds.insert(round, vote);
        if entry.rounds.len() > MAX_VOTE_ROUNDS_PER_VALIDATOR {
            entry.rounds.pop_first();
        }
        VoteAdmission::Fresh
    }

    /// The lowest (round, block) whose signers hold the required power,
    /// with their votes in ascending public-key order (certificate order).
    pub fn quorum(
        &self,
        set: &TrustedValidatorSetV1,
    ) -> Option<(u64, [u8; 32], Vec<SignedPrecommitV1>)> {
        let mut tally: BTreeMap<(u64, [u8; 32]), u64> = BTreeMap::new();
        for (key, votes) in &self.validators {
            let power = set.power_of(key).unwrap_or(0);
            for round in votes.rounds.keys() {
                // Each member counts once per (round, block), so the sum is
                // bounded by the validated, non-overflowing total power.
                *tally.entry((*round, votes.block)).or_default() += power;
            }
        }
        let required = set.required_power();
        let ((round, block), _) = tally.into_iter().find(|(_, power)| *power >= required)?;
        let votes = self
            .validators
            .values()
            .filter(|votes| votes.block == block)
            .filter_map(|votes| votes.rounds.get(&round).copied())
            .collect();
        Some((round, block, votes))
    }

    /// The highest round R such that validators holding more than one third
    /// of the voting power have precommitted in round R or later. At least
    /// one of them is honest, so a Byzantine minority cannot inflate R.
    pub fn honest_round_floor(&self, set: &TrustedValidatorSetV1) -> Option<u64> {
        let mut highest: Vec<(u64, u64)> = self
            .validators
            .iter()
            .filter_map(|(key, votes)| {
                let (round, _) = votes.rounds.last_key_value()?;
                Some((*round, set.power_of(key)?))
            })
            .collect();
        highest.sort_unstable_by_key(|(round, _)| Reverse(*round));
        let total = u128::from(set.total_power());
        let mut power = 0u128;
        for (round, weight) in highest {
            power += u128::from(weight);
            if power * 3 > total {
                return Some(round);
            }
        }
        None
    }
}

pub fn proposal_digest(raw: &[u8]) -> [u8; 32] {
    Sha256::digest(raw).into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalAdmission {
    /// Identical bytes were already executed and accepted as this block.
    Known([u8; 32]),
    /// Identical bytes were already executed and rejected.
    Rejected(String),
    /// This leader has exhausted its distinct-proposal budget.
    OverBudget,
    /// New bytes: execute them, then `record` the outcome.
    Execute,
}

/// Per-height cache of proposal execution outcomes keyed by the SHA-256 of
/// the block bytes, plus a distinct-execution budget per round leader.
#[derive(Default)]
pub struct ProposalGate {
    outcomes: BTreeMap<[u8; 32], Result<[u8; 32], String>>,
    executed_by: BTreeMap<[u8; 32], usize>,
}

impl ProposalGate {
    /// `leader` is the authenticated sender, or None for a local operator.
    pub fn admit(&self, digest: &[u8; 32], leader: Option<&[u8; 32]>) -> ProposalAdmission {
        match self.outcomes.get(digest) {
            Some(Ok(id)) => ProposalAdmission::Known(*id),
            Some(Err(reason)) => ProposalAdmission::Rejected(reason.clone()),
            None => {
                let used = leader
                    .and_then(|key| self.executed_by.get(key))
                    .copied()
                    .unwrap_or(0);
                if used >= MAX_DISTINCT_PROPOSALS_PER_LEADER {
                    ProposalAdmission::OverBudget
                } else {
                    ProposalAdmission::Execute
                }
            }
        }
    }

    pub fn record(
        &mut self,
        digest: [u8; 32],
        leader: Option<[u8; 32]>,
        outcome: Result<[u8; 32], String>,
    ) {
        if let Some(key) = leader {
            *self.executed_by.entry(key).or_default() += 1;
        }
        if self.outcomes.len() < MAX_REMEMBERED_PROPOSALS {
            self.outcomes.insert(digest, outcome);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validator_certificate::{sign_precommit_v1, verify_precommit_v1, ValidatorMemberV1};
    use ed25519_dalek::SigningKey;

    fn keys(count: u8) -> Vec<SigningKey> {
        (1..=count)
            .map(|i| SigningKey::from_bytes(&[i; 32]))
            .collect()
    }

    fn set(keys: &[SigningKey]) -> TrustedValidatorSetV1 {
        let members: Vec<ValidatorMemberV1> = keys
            .iter()
            .map(|key| ValidatorMemberV1 {
                public_key: key.verifying_key().to_bytes(),
                voting_power: 1,
            })
            .collect();
        TrustedValidatorSetV1::new(&members).unwrap()
    }

    fn vote(
        validators: &TrustedValidatorSetV1,
        key: &SigningKey,
        round: u64,
        block: [u8; 32],
    ) -> SignedPrecommitV1 {
        let vote = sign_precommit_v1(key, validators.commitment(), 5, round, block).unwrap();
        verify_precommit_v1(validators, 5, round, block, &vote).unwrap();
        vote
    }

    #[test]
    fn quorum_requires_two_thirds_in_one_round_and_orders_votes() {
        let keys = keys(4);
        let validators = set(&keys);
        let mut book = VoteBook::default();
        let block = [9; 32];
        // Three of four validators, but split across two rounds: no quorum.
        for (key, round) in keys.iter().zip([0, 0, 1]) {
            let signed = vote(&validators, key, round, block);
            assert_eq!(book.insert(round, block, signed), VoteAdmission::Fresh);
        }
        assert!(book.quorum(&validators).is_none());
        let late = vote(&validators, &keys[3], 1, block);
        assert_eq!(book.insert(1, block, late), VoteAdmission::Fresh);
        assert!(book.quorum(&validators).is_none());
        let again = vote(&validators, &keys[0], 1, block);
        assert_eq!(book.insert(1, block, again), VoteAdmission::Fresh);
        let (round, id, votes) = book.quorum(&validators).unwrap();
        assert_eq!((round, id, votes.len()), (1, block, 3));
        assert!(votes.windows(2).all(|w| w[0].public_key < w[1].public_key));
        assert_eq!(book.insert(1, block, again), VoteAdmission::Duplicate);
    }

    #[test]
    fn equivocating_validator_keeps_only_its_first_block() {
        let keys = keys(4);
        let validators = set(&keys);
        let mut book = VoteBook::default();
        let first = vote(&validators, &keys[0], 0, [1; 32]);
        assert_eq!(book.insert(0, [1; 32], first), VoteAdmission::Fresh);
        for round in 0..100 {
            let other = vote(&validators, &keys[0], round, [2; 32]);
            assert_eq!(
                book.insert(round, [2; 32], other),
                VoteAdmission::Equivocation([1; 32])
            );
        }
        let id = keys[0].verifying_key().to_bytes();
        assert_eq!(book.validators.len(), 1);
        assert_eq!(book.validators[&id].rounds.len(), 1);
    }

    #[test]
    fn round_spam_from_one_validator_is_bounded_and_evicts_lowest_rounds() {
        let keys = keys(4);
        let validators = set(&keys);
        let mut book = VoteBook::default();
        let block = [3; 32];
        for round in 0..1_000u64 {
            let signed = vote(&validators, &keys[0], round, block);
            assert_eq!(book.insert(round, block, signed), VoteAdmission::Fresh);
        }
        let id = keys[0].verifying_key().to_bytes();
        let rounds = &book.validators[&id].rounds;
        assert_eq!(rounds.len(), MAX_VOTE_ROUNDS_PER_VALIDATOR);
        assert_eq!(rounds.keys().next(), Some(&984));
        let old = vote(&validators, &keys[0], 3, block);
        assert_eq!(book.insert(3, block, old), VoteAdmission::Stale);
    }

    #[test]
    fn byzantine_minority_cannot_advance_the_honest_round_floor() {
        let keys = keys(4);
        let validators = set(&keys);
        let mut book = VoteBook::default();
        let block = [4; 32];
        // One of four validators (below one third) votes in round 1_000_000.
        let spam = vote(&validators, &keys[0], 1_000_000, block);
        book.insert(1_000_000, block, spam);
        assert_eq!(book.honest_round_floor(&validators), None);
        // A second validator in round 7: more than one third reached round 7.
        let honest = vote(&validators, &keys[1], 7, block);
        book.insert(7, block, honest);
        assert_eq!(book.honest_round_floor(&validators), Some(7));
    }

    #[test]
    fn proposal_gate_caches_outcomes_and_budgets_each_leader() {
        let mut gate = ProposalGate::default();
        let (a, b) = ([7; 32], [8; 32]);
        let ok = proposal_digest(b"good block");
        let bad = proposal_digest(b"bad block");
        assert_eq!(gate.admit(&ok, Some(&a)), ProposalAdmission::Execute);
        gate.record(ok, Some(a), Ok([1; 32]));
        gate.record(bad, Some(a), Err("invalid".into()));
        // Re-sent bytes never execute again, whoever sends them.
        let known = ProposalAdmission::Known([1; 32]);
        assert_eq!(gate.admit(&ok, Some(&b)), known);
        let rejected = ProposalAdmission::Rejected("invalid".into());
        assert_eq!(gate.admit(&bad, Some(&a)), rejected);
        for n in 2..MAX_DISTINCT_PROPOSALS_PER_LEADER {
            let digest = proposal_digest(&[n as u8]);
            assert_eq!(gate.admit(&digest, Some(&a)), ProposalAdmission::Execute);
            gate.record(digest, Some(a), Err("invalid".into()));
        }
        let fresh = proposal_digest(b"one more distinct body");
        let over = ProposalAdmission::OverBudget;
        assert_eq!(gate.admit(&fresh, Some(&a)), over);
        // Other leaders and local operator injections keep their own budget.
        assert_eq!(gate.admit(&fresh, Some(&b)), ProposalAdmission::Execute);
        assert_eq!(gate.admit(&fresh, None), ProposalAdmission::Execute);
    }
}
