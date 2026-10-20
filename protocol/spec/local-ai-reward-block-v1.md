# Local AI reward blocks — experimental V1 (Milestone #20)

**Status:** Executable local Rust / Python / Haskell / RocksDB fixture.
This is **not** validator consensus, remote verifiable AI mining,
a secure network protocol, finality, or live cryptocurrency.

## Checked local path

1. Rust authenticates an Ed25519-signed assignment from a pinned **test-only**
   issuer, recomputes expected deterministic AI outputs, compares the full
   Python worker response, and asks a separate Python process to recompute
   the proof. Only verified results enter the local receipt registry.
2. The reward builder hashes chain-specific receipt identities, checks
   nonzero commitments, bounded ranges, no overlapping work, and exact
   canonical ordering by puzzle ID and work-range start.
3. Haskell receives the **prior typed ledger**, recomputes receipt IDs,
   screens eligibility and settles using the ledger's **existing fee pool**.
4. Rust strictly parses the Haskell response, checking exact IDs, state root,
   replay markers, unchanged nonces, conservation and correct fee reduction.
5. A single synced RocksDB WriteBatch commits the block, balances, fees,
   receipts, batch marker, height and tip. Rejected operations leave state
   unchanged.
6. Signed-genesis chain replay decodes the on-disk reward block,
   recomputes receipt IDs and proof root, replays Haskell accounting from
   its real ancestor and checks the resulting state root and final ledger.

## Canonical block format

~~~text
ASCII("AIVOBLOCK-REWARD-BLOCK-V1") || 0x00
|| canonical_header_v1[178]
|| batch_id[32] || budget:u64_be || count:u32_be (1..64)
|| count * reward_record[160]

reward_record =
receipt_id[32] || puzzle_id[32] || worker_id[32]
|| result_commitment[32]
|| start:u64_be || count:u64_be
|| puzzle_units:u64_be || deadline_height:u64_be
~~~

The header's puzzle proof root is SHA-256 of:

~~~text
ASCII("AIVOBLOCK-REWARD-ROOT-V1") || 0x00
|| batch_id[32] || budget:u64_be || count:u32_be
|| concatenated reward records
~~~

The transaction root is the canonical empty root. The state root uses
the existing V1 domain-separated typed ledger root. The block ID is
SHA-256 of exactly the 178-byte canonical header. Ancestry and height
are checked on append and historical replay.

## End-to-end verification

The ai-reward-block-demo runs two signed local AI assignments and verifies
three integer-inference units, starts a genesis checkpoint authenticated
with an independently pinned **test-only** Ed25519 authority, and starts
with a funded 3-unit fee pool. It checks payouts 2 + 1, replay guards,
durable reopening, full signed-genesis history replay, and rejection of
duplicate payments, corrupted stored body and incorrect genesis authority.
The real Rust/Python/Haskell CI job exercises this path.

## Remaining trust boundaries

* The verified=true field in Haskell is still a trusted local Rust
  assertion. A receipt does NOT independently prove remote AI work;
  replay verifies accountable settlements but cannot reproduce arbitrary
  AI work from only a hash. NEVER authorize peer-provided receipts.
* No P2P consensus, network validation, finality, production authority
  key distribution, worker-to-wallet policy, data availability, governance
  or live cryptocurrency is implemented.
* Privileged low-level storage writers remain separate from the
  checked entrypoint. Forks and reorgs are unsupported.
* Next milestone: independently verifiable AI proof envelopes bound to
  authenticated model, input, and rules commitments for validator replay.
