# Sayed Token issuance and AI-work settlement V1

Status: implemented for the **private research testnet** only. No real funds.

## Supply

Native supply is `sum(account balances) + fee_pool`, in indivisible base units
(`u64` per balance, unbounded integer for the sum). Genesis supply is fixed by
the signed genesis checkpoint and must not exceed `max_supply`.

## Policy (genesis-bound)

```text
IssuancePolicyV1 { initial_reward: u64, halving_interval: u64 > 0, max_supply: u64 > 0 }
initial_reward <= max_supply
canonical bytes = "AIVOBLOCK-ISSUANCE-POLICY-V1\0" || initial_reward:u64be
                  || halving_interval:u64be || max_supply:u64be
```

The policy is bound into the signed testnet network configuration, so it is a
consensus parameter, not a per-node setting.

## Schedule

For a block at height `h >= 1` with prior supply `S` (`S <= max_supply`):

```text
halvings  = (h - 1) div halving_interval
base      = 0                      if halvings >= 64
          = initial_reward >> halvings otherwise
issuance  = min(base, max_supply - S)
```

Genesis never issues. A ledger with `S > max_supply` is invalid.

## Settlement of one finalized AI-work block

Inputs: prior ledger, height, `batch_id`, task `fee_budget`, claimed
`issuance`, and the independently recomputed works (one per assigned
partition). The transition (Haskell `settleIssuedRewards`, mirrored in Rust
`mirror_settle_issued_rewards`) is:

1. `issuance` must equal the schedule for `(h, S)`.
2. `fee_budget <= fee_pool` — fees must already exist; issuance cannot fund them.
3. `total = fee_budget + issuance > 0`.
4. Mint `issuance` into the fee pool, then pay `total` with the existing
   fee-funded rule: weights are verified work units per worker, split by
   largest remainder with ties to the smaller worker ID; replay markers for
   the batch and every receipt ID are added; nonces are unchanged.
5. Post-supply must equal `S + issuance` and must not exceed `max_supply`.

The fee pool therefore decreases by exactly `fee_budget`. Any failure leaves
the ledger untouched (pure function; RocksDB writes happen only afterwards).

## Cross-checks

Every validator executes the transition in Haskell **and** in the Rust mirror
and rejects the block unless the successor ledgers are byte-identical. The
Rust reply decoder additionally re-checks fee, supply, cap, nonce and replay
invariants. `ai-engine/src/aivoblock_ai/rewards.py` is a third, independent
reference used by the testnet harness to predict balances.

Shared vector (Haskell `IssuanceSpec`, Rust `issuance::tests`, Python
`test_rewards.py`): policy `(50, 100, 2_000_000)`, prior ledger `{alice: 200,
fee_pool: 600}`, height 1, fee budget 30, three workers with one unit each
→ issuance 50, payouts `27, 27, 26` (worker IDs `01..`, `02..`, `03..`),
fee pool 570, supply 850.

## Limitations

- Testnet parameters are illustrative, not an economic proposal.
- There is no fee market; task fee budgets come from a genesis-prefunded pool.
- No slashing, staking or validator rewards.
