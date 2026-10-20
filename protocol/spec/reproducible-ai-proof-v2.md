# Milestone 21: reproducible AI inference proofs — V2 local chain

V2 is a **bounded deterministic-recomputation proof**, not a succinct proof
and not general useful-work consensus. Unlike legacy V1 local payouts,
normal replay of V2 blocks verifies the model, input, task authorization and
each signed result **from persisted block bytes** before Haskell can credit fees.

## Security policy and trust boundary

- A validator obtains an Ed25519 task-authority public key independently
  (in the local signed-genesis audit the previously pinned genesis key is
  used as the task-policy key for this test). Never trust a key supplied by
  the same block or fetched from local storage as its own trust root.
- The task authority signs the exact, domain-separated V2 task ID; that ID
  binds chain ID, model hash, input hash, rules hash, complete row count,
  deadline and **exact authorized reward budget**.
- The model/input/rules hashes are independently recalculated by the Rust
  validator from the complete, bounded signed task witness serialized in
  the block. The model and input encodings are byte-for-byte compatible
  with Python's existing integer-inference V1 encoders.
- Workers sign each V2 result claim with their own Ed25519 public key.
  A validator verifies ownership of the payout identity and independently
  evaluates every dot product in the claimed range; the exact result hash
  must match. These are **full work recomputations**, not a shortcut.
- Receipts must cover every task unit exactly once in ascending order.
  One deterministic settlement batch ID per canonical task stops payout
  replay even if a proposer changes partition boundaries or workers.
- Haskell only settles already validated receipts from existing fees,
  enforcing budget/supply/replay rules, and RocksDB atomically records
  balances, fee pool, paid receipts, batch ID, block bytes and chain tip.
  Read-only replay reconstructs the entire ledger from authenticated
  genesis and independently executes the proof checks again.

Normal signed-genesis replay now **rejects caller-attested V1 reward blocks**.
The V1 demonstration retains a separately named legacy replay path only
for historical tests and must never be used to validate peer blocks.

## Bounded V2 protocol

Chain ID is fixed to 1 for this local prototype. Rows and weights are
signed 64-bit big-endian integers between -1,000,000 and 1,000,000.
Maximum rows = 64; maximum features per row = 16; maximum claims = 64.
All arithmetic is deterministic integer-only. Every verifier uses the
same range and commitment calculation.

The block body is encoded in this exact order:

~~~text
ASCII("AIVOBLOCK-AI-REWARD-PROOF-BLOCK-V2") || 0x00
|| canonical_header_v1[178]
|| task_reward_batch_id[32]
|| puzzle_id[32]
|| model_commitment[32] || input_commitment[32] || rules_commitment[32]
|| deadline_height:u64_be || authorized_budget:u64_be
|| task_authority_signature[64]
|| row_count:u32_be || feature_count:u32_be
|| feature_count * signed_weight:i64_be
|| row_count * feature_count * signed_feature:i64_be
|| claim_count:u32_be (1..64)
|| claim_count * (
     receipt_id[32] || worker_public_key[32]
     || start:u64_be || count:u64_be
     || result_commitment[32] || worker_signature[64]
   )
~~~

The header's puzzle-proofs root equals SHA-256 over:

~~~text
ASCII("AIVOBLOCK-AI-PROOF-ROOT-V2") || 0x00
|| all body bytes after the canonical header
~~~

The block ID remains SHA-256 of its exact canonical 178-byte V1 header.
The transaction root is the existing canonical empty root for reward
blocks. The state root is the domain-separated canonical typed-ledger
root. All declared lengths are explicitly checked before allocation;
extra bytes, missing bytes, unsupported limits, duplicated or incomplete
ranges, expired tasks, and tampered commitments are rejected.

## Demonstration and CI

The ai-proof-v2-demo uses the pinned deterministic integer-inference task:
3 input rows, 2 integer weights, two signed worker ranges, Python
worker execution/recomputation, a signed task from the genesis authority,
Haskell fee-funded settlement 2 + 1, and persisted RocksDB signed-genesis
history. It checks incorrect authority, forged worker result, altered
model data, duplicate/replayed payouts, bad on-disk bytes, and full
read-only replay after a database reopen. Unit tests cover model, input,
rules, result vectors and tampered signatures.

## Hard limits and research gaps

* No succinct proof or cryptographically provable *cost of work* exists.
  A verifier repeats this small deterministic computation. A worker's
  signed valid answer shows possession of its signing key, not that it
  alone performed the work or that the work was economically valuable.
* No decentralized task issuance or validator-set governance, chain
  fork/reorganization processing, P2P ingress, finality, fees market,
  spending policy rotation, fault-tolerant consensus or real token payments.
* Rust performs the data/AI proof check before invoking Haskell. Haskell
  independently replays deterministic **accounting**, but is not itself a
  second independent AI-proof implementation yet. A production consensus
  core would need a precise implementation across validators, perhaps
  with independent cross-language differential vectors or a succinct proof
  verifier before networking is enabled.
* The V1 demonstration's explicitly legacy function exists to preserve
  backwards test coverage only. The default signed-genesis verifier
  rejects V1 caller-attested rewards.

Next logical milestone: validator-set authorization and a replayable
consensus check, only after specifying deterministic V2 proof policy for
multi-node agreement.
