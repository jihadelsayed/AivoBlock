# Strict signed-ledger IPC response parsing — experimental V1

**Status:** A local Rust↔Haskell protection boundary, not consensus or an
authentication scheme for remote peers.

## Threat addressed

Earlier diagnostic paths checked JSON responses by searching for raw text
fragments such as `"accepted":true`, `"state_hex"`, and Merkle-root strings.
Such checks are brittle when JSON fields are escaped, reordered or duplicated,
when the strings appear inside error messages, and when fields have wrong types.

The Rust node now uses typed `serde_json` responses for both
`ledger_signed_batch_fixture` and `ledger_signed_transfer_fixture`.

## Strict acceptance rules

- The framed response is nonempty and at most 4 MiB.
- JSON must parse as exactly the expected object schema, including boolean
  `ok` and `accepted`, numeric protocol version 1, exact message kind and
  scope, and `reason = "none"` on accepted results.
- Unknown fields, duplicate recognized object keys, wrong types, wrong
  versions and unsupported scopes are errors.
- An accepted response must have the canonical successor snapshot, the
  post-state commitment, the transaction Merkle root, and the required
  transaction ID(s). Null or missing commitments are errors.
- Transaction IDs must be *exactly* the signed IDs computed by Rust, in the
  *same order*, with the expected list length (1–64 for batches).
- All hex fields must be lowercase ASCII without whitespace, with exactly
  32 bytes for transaction IDs and roots. State hex has a 2 MiB character cap.
- State hex must decode to the existing canonical typed ledger snapshot
  (balances, nonces, fee pool, paid receipts, reward batches), and Haskell's
  reported state root must equal Rust's recomputed root.
- Parsing failure **prevents any RocksDB block/ledger write**. The historical
  one-transfer diagnostic and the multi-transfer replay path share the typed
  response checks.

The cross-language GitHub Actions tests continue to run the real Haskell IPC
executable, the signed transfer demos, and the full read-only chain audit.

## Explicit limitations

This is defense-in-depth against malformed **responses**, not proof that the
Haskell binary is trusted or that an IPC process cannot be compromised.
The IPC requests are still assembled as JSON text from locally typed
transfer fields. A full network input protocol, canonical messages signed
by peers, consensus quorum, chain finality, trusted genesis policy, AI-work
proof validation, and real rewards remain separate milestones.
