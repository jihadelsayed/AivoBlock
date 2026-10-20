# AivoBlock candidate V1 transaction and block commitments

**Status: local one-transfer-per-block prototype. Not ratified as network
consensus.** There is no validator voting, authenticated genesis distribution,
AI proof root verification, finality, or live cryptocurrency.

## Signed transaction ID

Only the exact Ed25519 signed transfer V1 of
[ed25519-transfer-v1.md](ed25519-transfer-v1.md) is accepted. A transaction
identifier is the SHA-256 digest of:

```text
ASCII("AIVOBLOCK-TX-ID-V1") || 0x00
|| transferSigningBytesV1[118]
|| Ed25519Signature[64]
```

The preimage is 201 bytes (19 + 118 + 64). Both Rust and Haskell
**verify the signature and sender ownership before returning any ID**.
In this V1 prototype the sender account is the raw Ed25519 public key.
Reusing the same signed bytes and nonce produces exactly the same ID; the
ledger also requires the account nonce to increment after execution.

Independent fixed vector (test-only key seed 32 bytes of `07`;
recipient 32 bytes of `02`; amount 10, fee 2, nonce 0, chain 1):

```text
a75cb64af25b97c4f7f06feb4a77827ba4d1159426eb45a9c99fded8dcb5b52d
```

The signature is included to bind the exact signed envelope. This identity
does not prove a transaction was included in a valid consensus block.

## Ordered transaction Merkle root

- Empty block: `SHA256(ASCII("AIVOBLOCK-TX-EMPTY-V1") || 0x00)`.
- Leaf for ID X:
  `SHA256(ASCII("AIVOBLOCK-TX-LEAF-V1") || 0x00 || raw32(X))`.
- Internal node:
  `SHA256(ASCII("AIVOBLOCK-TX-NODE-V1") || 0x00 || raw32(left) || raw32(right))`.
- Combine adjacent nodes in the original block transaction order.
- At each odd level, duplicate the last node and hash its pair.
- Reject duplicate IDs and more than 65,536 transactions.

The single-transfer Merkle root is its domain-separated leaf. A list of
multiple transactions can be committed and tested as a pure Merkle function,
but **the integrated Haskell state transition currently executes exactly
one transaction per child block**. An implementation MUST reject blocks
claiming other transactions unless all of them are validated and executed.

```text
empty root:
4cd0e95f1262d79dd9a479ac1cd9471a64552f5cf44a40ce4d8249b8d9f7f385
root for the fixed single transfer:
d0ea168dc83df1fd8bf95b46bf20d434d4bd2ae12b1a9ce7cf77cadac19283c3
```

Additional independent ordered Merkle vectors (raw 32-byte IDs made from
repeated bytes `01`, `02` and `03`; no signatures implied):

```text
two IDs [01,02]:
59cdd667d6549503267625933df3bfa85016948f59f8bb91af0b945f926a6477
three IDs [01,02,03] with last leaf duplicated:
0dbe0e823658b8f97440d5b79bc1b1aa7b0943ab43a1e9e930f1b4c881f2e822
```

## Ledger state root and candidate V1 header

The *local* ledger state root is

```text
SHA256(ASCII("AIVOBLOCK-STATE-ROOT-V1") || 0x00 ||
       encode_ledger_state_v1(updated_state))
```

The state codec includes accounts, balances, nonces, fee pool and reward
replay sets in sorted canonical order. This is **not** a sparse Merkle tree
or consensus-approved authenticated state proof.

The candidate canonical 178-byte block header from
[canonical-encoding.md](canonical-encoding.md) contains:

- version, chain ID, height and parent block ID
- transaction Merkle root above
- puzzle proofs root (all-zero placeholder, enforced for this fixture only)
- post-transaction local ledger state root above
- timestamp

Block ID is SHA-256 of those exact 178 header bytes. A local signed-transfer
body is:

```text
ASCII("AIVOBLOCK-TX-BODY-V1") || 0x00
|| canonical_block_header_v1[178]
|| uint32_be(transaction_count = 1)
|| transaction_id[32]
|| signing_bytes[118]
|| signature[64]
```

The Rust high-level `commit_signed_transfer_block` wrapper checks the
on-disk prior ledger snapshot, verified signature, transaction root, state
root, protocol version and zero puzzle-proof placeholder, then calls the
synced RocksDB atomic commit to write the block, post-state, tip and height.
The stored block is rehashed and rechecked after reopening. Haskell
independently returns its signed transaction ID, Merkle root and state root
as part of its signed-transfer IPC response; Rust compares these values to
its own computations before storing the successor.

The genesis state and its initial allocation remain a locally trusted
fixture. Its header uses the empty transaction root, and its calculated
hash is used as the actual parent ID of its first child.

## Remaining security gaps

- Low-level RocksDB mutation APIs still accept caller-supplied ledger/block
  data. The high-level wrapper is the checked path, not a sealed consensus
  verification boundary.
- No proof that state was computed by all validators, no authenticated genesis,
  AI-proof/body root validation or reward/receipt settlement under a real
  block. This milestone handles signed transfers only.
- No transaction inclusion proofs exported, persistent transaction index,
  reorg/fork handling, P2P networking, mempool, validator quorum or finality.
- The Haskell IPC response is from a trusted local process with a limited
  text-based JSON decoder, which must be replaced with fully typed decoding
  before accepting untrusted peer data.
- None of these fixtures create or transfer spendable tokens on a network.
