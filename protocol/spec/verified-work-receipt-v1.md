# Candidate V1 verified work receipt identity

**Status:** Local cross-language prototype. Not ratified for network consensus.
This identity is a deterministic commitment to a receipt's *claimed* fields;
it is not evidence the receipt has been authenticated or recomputed.

## Bytes and hash

`receipt_id = SHA-256(preimage)`, where the **151-byte** preimage concatenates
the following fields exactly, without JSON, separators, or length prefixes:

| Field | Length | Encoding |
| --- | ---: | --- |
| Domain | 35 bytes | ASCII `AIVOBLOCK-VERIFIED-WORK-RECEIPT-V1` followed by `00` |
| Chain ID | 4 bytes | Unsigned big-endian u32 |
| Puzzle ID | 32 bytes | Raw hash bytes |
| Worker ID | 32 bytes | Raw Ed25519 worker public key bytes |
| Start unit | 8 bytes | Unsigned big-endian u64 |
| Work unit count | 8 bytes | Unsigned big-endian u64 |
| Result commitment | 32 bytes | Raw hash bytes |

Reject zero chain ID, zero work-unit count, or a range that overflows u64.
The actual maximum puzzle units, deadlines, authorized issuer and worker,
verified result, and fee-budget authority are separately checked.

Reference vector (chain ID 1; puzzle 32 bytes of `01`, worker 32 bytes
of `02`, start 0, count 2, result commitment 32 bytes of `03`):

```
1085d690f142951fb5fc32a9979d2b8d0afb2861fcc7cfae095d1f2cf9c80db1
```

## Local diagnostic only

The Rust node sends already-recomputed fixture receipts to the Haskell
`settlement_fixture` IPC request. Haskell independently recalculates each
receipt ID, screens eligibility, and applies the pure fee-funded settlement
rules to a **synthetic fee pool supplied in that IPC request**. This is NOT a
trusted chain state and does not authorize any real payment.

After validating the diagnostic response, Rust atomically writes receipt and
batch replay markers into a RocksDB synced-WAL batch. They survive process
reopening. This is only **local replay bookkeeping**: a real chain requires
atomic storage of account balances, fee pool, receipt IDs, batch IDs, block
commitment and tip, with crash recovery and reorganization handling.

In addition, consensus must define canonical task identity, proof validation,
worker-to-address mapping, signed receipt attestation, and budget rules.
Do not expose the fixture IPC endpoint to untrusted peers.
