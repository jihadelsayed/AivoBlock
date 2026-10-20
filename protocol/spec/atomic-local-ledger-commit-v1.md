# Atomic local ledger/block commit — experimental V1

**Status: prototype storage boundary, not blockchain consensus.**

The Rust node now has `RocksDbStore::append_atomic_ledger_block`.
It accepts an **already validated by a trusted caller** post-execution local
`LedgerState`, candidate block hash/parent/height, and optionally a
`SettlementBatch` with receipt IDs and a settlement response.

## One synced atomic write

Once the preconditions pass, a single RocksDB `WriteBatch` with `sync=true`
commits together:

- Block bytes under the supplied candidate block ID
- Chain tip and height
- Deterministic, sorted binary ledger snapshot: account addresses, balances,
  nonces, and fee pool
- Settled reward batch and paid receipt sets in the snapshot
- Matching reward-batch and paid-receipt metadata records
- SHA-256 checksum of the serialized local snapshot (NOT a consensus root)

A predecessor is required for non-genesis blocks. The existing tip must match
the new parent and heights must be sequential. On a successor, the local sum
of all account balances plus fee pool must remain constant, prior account
nonces cannot decrease, and historical paid-receipt/batch markers must never
disappear. New marker IDs must exactly equal those in the settlement batch.
No duplicate or previously used IDs may enter a new batch.

The older `append_checked_block`, legacy block-tip writer, and standalone
receipt replay guard are rejected after entering the typed atomic storage
mode, preventing those demo APIs from accidentally bypassing its invariants.
A database with a previously used standalone replay guard cannot silently
start a new atomic chain without an explicit migration.

## Recovery and tests

Tests close and reopen RocksDB and read back block, tip, height, typed
balances, fee pool, and replay keys. They test malformed/truncated state
encoding, state-checksum mismatch, duplicate batch/receipt, bad parent,
wrong height, unintended supply change, and legacy writer rejection.
Rejected operations must leave all saved records unchanged.

The synced WAL improves durability across restart. **These tests simulate
reopening, not a power-cut at every possible instruction.** Hardware power-loss
and file-system failure injection are not implemented.

## Safety and limitations

The node still does **not** authenticate ledger snapshots, independently
verify AI proofs, bind the snapshot SHA-256 digest to a canonical signed block
header, independently verify actual Haskell ledger transitions, produce a
trusted genesis distribution, operate a consensus protocol, or handle
reorganizations and rollbacks. Local supply conservation is not a replacement
for full transaction and receipt authentication.

The current Haskell `settlement_fixture` runs with a synthetic fee pool.
There are **no live payments or newly issued tokens**. Integration of the
authenticated Haskell ledger transition with this atomic storage boundary is
a separate milestone, as is a fault-injection crash-recovery suite.
