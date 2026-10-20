# Haskell ledger transition → Rust atomic storage bridge (experimental)

**Status: local caller-attested diagnostic only. No signature validation or
consensus, no production ledger, no actual tokens transferred.**

## Process

1. Rust stores a fixed genesis ledger using the existing atomic RocksDB
   storage operation. This initial allocation is a test fixture.
2. Rust reopens RocksDB and reads its typed, checksum-checked prior snapshot.
3. Rust sends that snapshot as lowercase hexadecimal in a bounded, framed IPC
   `ledger_transfer_fixture` request to the locally trusted Haskell executable,
   together with a transfer with sender, recipient, amount, fee and nonce.
4. Haskell independently decodes the Rust binary snapshot using
   `AivoBlock.LocalLedgerSnapshot` and calls the existing pure
   `applyVerifiedTransfer` ledger transition. Haskell rejects insufficient
   balance, nonce mismatch and 64-bit overflow and preserves past paid receipts
   and settled reward batches.
5. Only on an `accepted=true` response with the expected response scope,
   Rust decodes the returned canonical binary snapshot. Rust passes **only that
   Haskell-computed snapshot** to `append_atomic_ledger_block`.
6. RocksDB atomically commits block bytes, tip, height, the complete ledger,
   fee pool and replay identifiers with a synced WAL batch.

A Rust fixture exercises two Haskell-computed transfers from 100 initial
units: first 10 units + 2 fee and then 5 units + 1 fee. After reopening,
the balances are 82 and 15 with fee pool 3 and total supply 100. Negative
cases assert that a stale nonce and overspend are rejected before commit.

## Wire format

The prior and next states use
`AIVOBLOCK-LOCAL-LEDGER-V1\0` plus big-endian fee-pool u64, account-count
u32, ascending account records (address[32], balance u64, nonce u64), batch
count u32, ascending batch hashes[32], receipt count u32 and ascending
receipt hashes[32]. The Haskell codec verifies exact canonical re-encoding.
The Rust codec separately checks canonical state before storing.

Request JSON:

```json
{
  "protocol_version": 1,
  "type": "ledger_transfer_fixture",
  "caller_attested": true,
  "version": 1,
  "chain_id": 1,
  "state_hex": "<encoded prior ledger>",
  "transfer": {
    "sender": "<32-byte hexadecimal address>",
    "recipient": "<32-byte hexadecimal address>",
    "amount": 10,
    "fee": 2,
    "nonce": 0
  }
}
```

Successful responses are framed JSON including `accepted=true`,
`state_hex`, and scope `caller_attested_haskell_transfer_only`.
Failed transitions return `accepted=false` with a reason and no next
ledger. Rust rejects failures without changing persistent state.

## Critical limitations

- The caller-provided attestation flag is **not signature verification**:
  anyone who can call this local endpoint can set it.
- Rust must verify the transfer signature, account ownership, canonical
  transaction bytes and chain authorization *before* asking Haskell.
  This is not yet wired into the new diagnostic.
- The Rust legacy storage API still permits caller-supplied snapshots;
  only this new diagnostic routes the resulting state through Haskell.
- Block IDs are synthetic fixtures, not independently validated headers
  or block-body commitments, and the stored snapshot checksum is not bound
  to a validated consensus state root.
- This is a single local sequence: no P2P, validator voting, reorg handling,
  power-loss fault injection, finality or live token economy.
- The IPC adapter uses a trusted local executable and a deliberately limited
  response parser; it must be replaced by a fully typed, bounded protocol
  before accepting untrusted inputs.

**Next:** authenticate transfer signatures and chain state before dispatch,
then bind Haskell output to canonical block state roots and atomic storage.
