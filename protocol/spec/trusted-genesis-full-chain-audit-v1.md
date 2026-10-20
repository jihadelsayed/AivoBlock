# Trusted-genesis full-chain replay audit — experimental V1

**Status:** Read-only local audit. Neither the stored chain nor this replay
algorithm provides consensus, network finality, AI-work verification, or
cryptocurrency payments.

## Trusted input

`replay_local_chain(db, core, trusted_genesis_id, trusted_genesis_state)`
requires **two independently authenticated checkpoint inputs**:

- The expected 32-byte hash of a canonical genesis header
- The expected genesis ledger snapshot, including its initial accounts and
  balances, nonces, fee pool and replay-marker sets

Neither value is inferred from the untrusted local database. An internally
self-consistent database cannot authenticate its own genesis allocation.

## Independent verification

1. Read the exact 32-byte stored tip ID and unsigned 64-bit height. Reject
   missing, malformed, or unreasonable audit height (local bound: 1,024 blocks).
2. Traverse on-disk parent pointers from the tip toward the trusted genesis
   ID. Reject cycles, missing blocks, malformed signed bodies, wrong header
   hashes, nonconsecutive heights, zero parents and paths that do not
   reach the trusted genesis at height zero.
3. Verify genesis bytes are an **exact 178-byte canonical header**, with
   zero parent, zero placeholder AI-proof root, empty transaction Merkle
   root, and a state root matching the **trusted external genesis snapshot**.
   Verify its SHA-256 ID matches the external expected genesis ID.
4. Reverse the collected signed blocks into chain order. Decode each body
   from disk, verify every embedded Ed25519 signature, transaction ID,
   transaction count and ordered Merkle root. Sequentially ask trusted
   local Haskell to execute the entire signed batch from the *actual*
   predecessor ledger produced by replay, never from an arbitrary
   caller-supplied intermediate state.
5. For every block, check the Haskell-computed successor state root exactly
   matches its committed header. At the tip, compare the full Haskell
   ledger against the canonical RocksDB snapshot (including balances,
   nonces, fee pool, paid receipts and settled batch IDs), not merely a
   hash. The auditor never writes to storage.

The audit emits the rebuilt tip ledger, signed block count, signed
transaction count, and tip ID.

## End-to-end regression

The cross-language `multi-transfer-demo` now constructs a local chain
with **genesis plus two signed blocks**, each containing two
Ed25519-authenticated transfers. The fixture's final balances are:
Alice 73, Bob 22, fee pool 5, Alice nonce 4, total supply 100.

The regression rejects wrong genesis ID or genesis state, corrupted
historical signed transactions, an inconsistent tip height and state
mismatch. It restores test-only corruption and then closes/reopens
RocksDB and verifies the complete chain again.

## Trust and operational limits

- This is **bounded historical auditing**, not an ability to accept
  untrusted network blocks or choose a canonical fork.
- Only raw header-only genesis plus the current V1 signed multi-transfer
  child-body format is supported. Reward-settlement and AI-proof-containing
  blocks are not replayed by this implementation.
- No validator signatures, quorum voting, proof-of-useful-AI-work, mempool,
  reorganization handling, authenticated peer network, governance, or
  production token economy exists in this audit.
- The genesis checkpoint must come from a trusted external source. The
  repository's hard-coded fixture allocations are **not network-authorized
  genesis**.
- Legacy lower-level RocksDB mutation APIs can still write arbitrary
  fixture data. The audit is a *detective read-only control*, not an
  enforcement boundary for every writer.
- The IPC process remains a locally configured trusted executable and
  the prototype response parser is not robust enough for arbitrary peers.
- This code does not simulate power cuts, malicious OS storage corruption
  with cryptographic security guarantees, or multi-node agreement.
