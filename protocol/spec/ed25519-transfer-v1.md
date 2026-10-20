# Ed25519 transfer authorization V1 — local prototype

**Status:** Implemented and cross-checked in Rust and Haskell for a
local test chain with chain ID 1 and transfer version 1. **Not approved
for production consensus or real monetary settlement.**

## Key and ownership rule

For this specific V1 prototype, the **32-byte account address is exactly the
raw Ed25519 verification/public key**. A transfer MUST present that public key
as the sender and a valid Ed25519 signature under that key. An arbitrary
32-byte address, derived address, unrelated key, or forged authorization
cannot spend another account's balance.

This intentionally restricts the format to Ed25519 raw-key accounts: no
separate SHA-256/Blake2 address derivation, multisig, key rotation, smart
contracts or references are accepted. Other address schemes need a new,
explicit version and cross-language migration.

## Canonical V1 signing payload (118 bytes)

```text
ASCII("AIVOBLOCK-TRANSFER-V1") || 0x00
|| uint32_be(version = 1)
|| uint32_be(chain_id = 1)
|| raw32(sender_public_key)
|| raw32(recipient_address)
|| uint64_be(amount)
|| uint64_be(fee)
|| uint64_be(nonce)
```

- All integers are unsigned, fixed-width, big-endian.
- Amount must be positive. The fee can be zero.
- The sender address must equal the 32 raw public-key bytes.
- Unsupported chain IDs, versions and optional-reference extensions are
  rejected before signature validation.
- The preimage is signed with Ed25519 (64-byte detached signature).
  JSON text, hex strings, local timestamps and block IDs are not signed.
- The sender's **persisted account nonce** must equal the transaction nonce
  before Haskell can debit funds. The nonce increments on each accepted
  transfer and is durably stored in the atomic ledger snapshot.
- Chains using this V1 restriction must use a separate domain/chain ID
  for any new deployment rather than reusing these test-chain signatures.

### Cross-language fixture

Deterministic test-only Ed25519 seed: 32 bytes of `07`. Never fund or
reuse this seed on a real network.

Public key / sender:

```text
ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c
```

Recipient: 32 bytes of `02`, amount 10, fee 2, nonce 0,
version 1, chain ID 1.

Expected signature:

```text
f1cc31b91b3485d813bbe245ee742795eb94dcd7cfdba90d30d1a4da9ec18ca9822a6ef0a36c54a58a956de172eee42816ec51b2a0dfbcff7422d62deb952405
```

## Integration order

1. Rust builds V1 signing bytes and verifies the Ed25519 signature, owner
   binding, transaction version and chain ID. Failure aborts **before IPC**.
2. Rust obtains the previous canonical ledger from trusted local RocksDB.
3. The framed Haskell `ledger_signed_transfer_fixture` IPC endpoint
   parses fixed-size public key/signature fields and **independently verifies
   the Ed25519 signature and sender ownership** before its pure
   `applyVerifiedTransfer` function runs.
4. Haskell checks account balance/nonce, computes the new account balances
   and fee pool, and returns a canonical ledger snapshot.
5. Rust decodes the Haskell result and atomically commits it with local
   block bytes, tip, height and replay-marker sets to RocksDB.

No caller-controlled `caller_attested` flag is sufficient for acceptance
in the new signed-transfer endpoint. Any older, separately labeled fixture
endpoints remain test-only; they cannot be treated as trusted consensus
execution paths.

## Security boundaries still missing

This implementation does not authenticate genesis allocations or block roots,
prevent callers of the lower-level RocksDB API from proposing snapshots,
validate a canonical transaction ID or block-body inclusion, enforce
mempool/fee policy, implement validator consensus or chain forks/reorgs,
or prove AI work. The IPC client runs a locally configured executable
without sandboxing or timeouts; treat its executable path as privileged.

**No live native-token payouts, issuance or decentralized network finality
are activated by this milestone.**
