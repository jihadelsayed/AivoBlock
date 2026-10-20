//! Genesis-bound native Sayed Token issuance policy and an independent Rust
//! settlement mirror of Haskell `AivoBlock.Issuance.settleIssuedRewards`.
//!
//! Haskell remains the ledger authority. The mirror lets every validator
//! cross-check the Haskell transition (N-version execution) and lets
//! Haskell-free unit tests exercise the full consensus pipeline. A block is
//! only accepted when both engines produce the identical successor ledger.
//! Neither engine verifies AI work: callers must pass only independently
//! recomputed, assignment-authenticated works.

use std::collections::{BTreeMap, BTreeSet};

use crate::reward_block::{checked_receipt_ids, RewardWorkV1};
use crate::storage::ledger_state::{LedgerState, LocalAccount};

const POLICY_DOMAIN: &[u8] = b"AIVOBLOCK-ISSUANCE-POLICY-V1\0";

/// Height h >= 1 issues `initial_reward >> ((h - 1) / halving_interval)`,
/// truncated so total supply (all balances plus the fee pool) never exceeds
/// `max_supply`. Matches Haskell `scheduledIssuance` exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IssuancePolicyV1 {
    pub initial_reward: u64,
    pub halving_interval: u64,
    pub max_supply: u64,
}

impl IssuancePolicyV1 {
    pub fn new(
        initial_reward: u64,
        halving_interval: u64,
        max_supply: u64,
    ) -> Result<Self, String> {
        if halving_interval == 0 {
            return Err("Halving interval must be positive".into());
        }
        if max_supply == 0 {
            return Err("Maximum supply must be positive".into());
        }
        if initial_reward > max_supply {
            return Err("Initial reward exceeds maximum supply".into());
        }
        Ok(Self {
            initial_reward,
            halving_interval,
            max_supply,
        })
    }

    /// Canonical bytes bound into the signed network configuration.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = POLICY_DOMAIN.to_vec();
        bytes.extend_from_slice(&self.initial_reward.to_be_bytes());
        bytes.extend_from_slice(&self.halving_interval.to_be_bytes());
        bytes.extend_from_slice(&self.max_supply.to_be_bytes());
        bytes
    }

    pub fn scheduled_issuance(&self, height: u64, supply: u128) -> Result<u64, String> {
        if height == 0 {
            return Err("Genesis does not issue rewards".into());
        }
        let cap = u128::from(self.max_supply);
        if supply > cap {
            return Err("Ledger supply exceeds issuance cap".into());
        }
        let halvings = (height - 1) / self.halving_interval;
        let base = if halvings >= 64 {
            0
        } else {
            self.initial_reward >> halvings
        };
        // cap - supply <= cap <= u64::MAX, so the minimum fits in u64.
        Ok(u128::from(base).min(cap - supply) as u64)
    }
}

/// Total native supply: every account balance plus the undistributed fees.
pub fn ledger_supply(state: &LedgerState) -> u128 {
    state
        .accounts
        .values()
        .map(|account| u128::from(account.balance))
        .sum::<u128>()
        + u128::from(state.fee_pool)
}

/// One finalized AI-work block's settlement parameters.
#[derive(Debug, Clone, Copy)]
pub struct IssuedSettlementV3<'a> {
    pub height: u64,
    pub batch_id: [u8; 32],
    pub fee_budget: u64,
    pub issuance: u64,
    pub works: &'a [RewardWorkV1],
}

/// Largest-remainder proportional split, identical to Haskell
/// `distributeRewards`: integer quotas, then one extra unit to the highest
/// fractional remainders with ties broken by ascending worker ID.
pub fn distribute_rewards(
    budget: u64,
    weights: &BTreeMap<[u8; 32], u64>,
) -> Result<BTreeMap<[u8; 32], u64>, String> {
    if weights.is_empty() {
        return Err("No reward recipients".into());
    }
    if weights.values().any(|weight| *weight == 0) {
        return Err("Contribution weights must be positive".into());
    }
    let total: u128 = weights.values().map(|weight| u128::from(*weight)).sum();
    let budget_wide = u128::from(budget);
    let mut quotas = Vec::with_capacity(weights.len());
    let mut allocated = 0u128;
    for (worker, weight) in weights {
        let numerator = budget_wide * u128::from(*weight);
        let whole = numerator / total;
        allocated += whole;
        quotas.push((*worker, whole, numerator % total));
    }
    let remaining = (budget_wide - allocated) as usize;
    let mut ranked: Vec<_> = quotas.iter().collect();
    ranked.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    let bonus: BTreeSet<[u8; 32]> = ranked.iter().take(remaining).map(|q| q.0).collect();
    let mut rewards = BTreeMap::new();
    for (worker, whole, _) in quotas {
        let amount = whole + u128::from(bonus.contains(&worker));
        rewards.insert(worker, amount as u64);
    }
    Ok(rewards)
}

/// Independent Rust re-implementation of Haskell `settleIssuedRewards`.
/// Returns the successor ledger and canonical receipt IDs in block order.
pub fn mirror_settle_issued_rewards(
    policy: &IssuancePolicyV1,
    prior: &LedgerState,
    input: &IssuedSettlementV3<'_>,
) -> Result<(LedgerState, Vec<[u8; 32]>), String> {
    let receipt_ids = checked_receipt_ids(input.works, input.height)?;
    let before = ledger_supply(prior);
    let expected = policy.scheduled_issuance(input.height, before)?;
    if input.issuance != expected {
        return Err("Issuance differs from policy schedule".into());
    }
    if input.fee_budget > prior.fee_pool {
        return Err("Insufficient fee pool".into());
    }
    let total = input
        .fee_budget
        .checked_add(input.issuance)
        .ok_or("Reward budget overflow")?;
    let pool = prior
        .fee_pool
        .checked_add(input.issuance)
        .ok_or("Reward budget overflow")?;
    if total == 0 {
        return Err("Reward budget must be positive".into());
    }
    if input.batch_id == [0; 32] {
        return Err("Zero reward batch ID".into());
    }
    if prior.settled_batches.contains(&input.batch_id) {
        return Err("Reward batch already settled".into());
    }
    let unique: BTreeSet<_> = receipt_ids.iter().collect();
    if unique.len() != receipt_ids.len() || receipt_ids.contains(&[0; 32]) {
        return Err("Duplicate or zero work receipt in reward batch".into());
    }
    if receipt_ids.iter().any(|id| prior.paid_receipts.contains(id)) {
        return Err("Work receipt already paid".into());
    }
    let mut weights = BTreeMap::<[u8; 32], u64>::new();
    for work in input.works {
        let entry = weights.entry(work.receipt.worker_id).or_insert(0);
        *entry = entry
            .checked_add(work.receipt.range.count)
            .ok_or("Worker weight overflow")?;
    }
    let rewards = distribute_rewards(total, &weights)?;
    let mut next = prior.clone();
    for (worker, amount) in rewards {
        if amount == 0 {
            continue;
        }
        let account = next.accounts.entry(worker).or_insert(LocalAccount {
            balance: 0,
            nonce: 0,
        });
        account.balance = account
            .balance
            .checked_add(amount)
            .ok_or("Unsigned 64-bit overflow")?;
    }
    next.fee_pool = pool - total;
    next.settled_batches.insert(input.batch_id);
    next.paid_receipts.extend(receipt_ids.iter().copied());
    let after = ledger_supply(&next);
    if after != before + u128::from(input.issuance) {
        return Err("Supply invariant violated".into());
    }
    if after > u128::from(policy.max_supply) {
        return Err("Issuance cap exceeded".into());
    }
    Ok((next, receipt_ids))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::mining::receipts::VerifiedWorkReceipt;
    use crate::mining::scheduler::WorkRange;

    pub fn test_policy() -> IssuancePolicyV1 {
        IssuancePolicyV1::new(50, 100, 2_000_000).unwrap()
    }

    /// Shared cross-language vector (Haskell IssuanceSpec, Python tests).
    pub fn vector_ledger() -> LedgerState {
        let mut ledger = LedgerState {
            fee_pool: 600,
            ..LedgerState::default()
        };
        ledger.accounts.insert(
            [11; 32],
            LocalAccount {
                balance: 200,
                nonce: 4,
            },
        );
        ledger
    }

    pub fn vector_works() -> Vec<RewardWorkV1> {
        (0..3u8)
            .map(|index| RewardWorkV1 {
                receipt: VerifiedWorkReceipt {
                    puzzle_id: [7; 32],
                    worker_id: [index + 1; 32],
                    range: WorkRange {
                        start: u64::from(index),
                        count: 1,
                    },
                    result_commitment: [9; 32],
                },
                puzzle_units: 3,
                deadline_height: 100,
            })
            .collect()
    }

    #[test]
    fn schedule_halves_truncates_and_matches_haskell_vectors() {
        let policy = test_policy();
        let got: Vec<u64> = [1, 100, 101, 200, 201, 301]
            .iter()
            .map(|h| policy.scheduled_issuance(*h, 0).unwrap())
            .collect();
        assert_eq!(got, vec![50, 50, 25, 25, 12, 6]);
        assert!(policy.scheduled_issuance(0, 0).is_err());
        assert_eq!(policy.scheduled_issuance(64 * 100 + 1, 0).unwrap(), 0);
        assert_eq!(policy.scheduled_issuance(u64::MAX, 0).unwrap(), 0);
        assert_eq!(policy.scheduled_issuance(1, 2_000_000 - 10).unwrap(), 10);
        assert_eq!(policy.scheduled_issuance(1, 2_000_000).unwrap(), 0);
        assert!(policy.scheduled_issuance(1, 2_000_001).is_err());
        assert!(IssuancePolicyV1::new(50, 0, 100).is_err());
        assert!(IssuancePolicyV1::new(50, 1, 0).is_err());
        assert!(IssuancePolicyV1::new(101, 1, 100).is_err());
    }

    #[test]
    fn largest_remainder_split_matches_haskell_tie_breaking() {
        let weights: BTreeMap<_, _> = [([1; 32], 1), ([2; 32], 1), ([3; 32], 1)].into();
        let split = distribute_rewards(80, &weights).unwrap();
        assert_eq!(split.values().copied().collect::<Vec<_>>(), vec![27, 27, 26]);
        let uneven: BTreeMap<_, _> = [([1; 32], 2), ([2; 32], 1)].into();
        let split = distribute_rewards(3, &uneven).unwrap();
        assert_eq!(split.values().copied().collect::<Vec<_>>(), vec![2, 1]);
        assert!(distribute_rewards(1, &BTreeMap::new()).is_err());
    }

    #[test]
    fn mirror_settlement_matches_shared_cross_language_vector() {
        let works = vector_works();
        let input = IssuedSettlementV3 {
            height: 1,
            batch_id: [90; 32],
            fee_budget: 30,
            issuance: 50,
            works: &works,
        };
        let (post, ids) =
            mirror_settle_issued_rewards(&test_policy(), &vector_ledger(), &input).unwrap();
        assert_eq!(post.fee_pool, 570);
        assert_eq!(post.accounts[&[1; 32]].balance, 27);
        assert_eq!(post.accounts[&[2; 32]].balance, 27);
        assert_eq!(post.accounts[&[3; 32]].balance, 26);
        assert_eq!(post.accounts[&[11; 32]].balance, 200);
        assert_eq!(post.accounts[&[11; 32]].nonce, 4);
        assert_eq!(ledger_supply(&post), 850);
        assert_eq!(ids.len(), 3);
        assert!(post.settled_batches.contains(&[90; 32]));
        assert_eq!(post.paid_receipts.len(), 3);
    }

    #[test]
    fn mirror_rejects_wrong_issuance_unfunded_fees_and_replays() {
        let works = vector_works();
        let policy = test_policy();
        let prior = vector_ledger();
        let base = IssuedSettlementV3 {
            height: 1,
            batch_id: [90; 32],
            fee_budget: 30,
            issuance: 50,
            works: &works,
        };
        for bad in [
            IssuedSettlementV3 {
                issuance: 51,
                ..base
            },
            IssuedSettlementV3 {
                height: 101,
                ..base
            },
            IssuedSettlementV3 {
                fee_budget: 601,
                ..base
            },
            IssuedSettlementV3 {
                batch_id: [0; 32],
                ..base
            },
        ] {
            assert!(mirror_settle_issued_rewards(&policy, &prior, &bad).is_err());
        }
        let (paid, _) = mirror_settle_issued_rewards(&policy, &prior, &base).unwrap();
        let again = IssuedSettlementV3 { height: 2, ..base };
        assert!(mirror_settle_issued_rewards(&policy, &paid, &again).is_err());
        let new_batch = IssuedSettlementV3 {
            height: 2,
            batch_id: [91; 32],
            ..base
        };
        assert!(mirror_settle_issued_rewards(&policy, &paid, &new_batch).is_err());
    }

    #[test]
    fn mirror_supply_grows_by_exactly_issuance_until_cap() {
        let works = vector_works();
        let policy = IssuancePolicyV1::new(50, 2, 1_000).unwrap();
        let mut state = vector_ledger();
        state.fee_pool = 700; // supply 900, room for 100 more units
        let mut issued_total = 0u128;
        for height in 1..=40u64 {
            let supply = ledger_supply(&state);
            let issuance = policy.scheduled_issuance(height, supply).unwrap();
            let shifted: Vec<RewardWorkV1> = works
                .iter()
                .map(|work| {
                    let mut work = *work;
                    work.receipt.puzzle_id = [height as u8; 32];
                    work.deadline_height = 1_000;
                    work
                })
                .collect();
            let input = IssuedSettlementV3 {
                height,
                batch_id: [height as u8 + 100; 32],
                fee_budget: 1,
                issuance,
                works: &shifted,
            };
            let (next, _) = mirror_settle_issued_rewards(&policy, &state, &input).unwrap();
            assert_eq!(ledger_supply(&next), supply + u128::from(issuance));
            assert!(ledger_supply(&next) <= 1_000);
            issued_total += u128::from(issuance);
            state = next;
        }
        assert_eq!(issued_total, 100);
        assert_eq!(ledger_supply(&state), 1_000);
    }
}
