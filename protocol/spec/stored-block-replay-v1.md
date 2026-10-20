# Stored signed block decoding and Haskell replay — experimental V1

**Scope:** Local diagnostic for AivoBlock's **ordered multi-transfer blocks**.
It is **not** a consensus validator, general chain importer, P2P handler,
fork-choice algorithm, or proof of useful AI work.

## Independent storage-to-execution audit

The previous verification rebuilt a block from an **in-memory transaction
list**. The new decoder instead starts from the exact bytes obtained through
`RocksDbStore::get(Column::Blocks, block_id)` and rejects malformed or
tampered records before Haskell is called.

A stored V1 batch must have exactly:

```text
"AIVOBLOCK-TX-BATCH-BODY-V1" || 00
|| 178-byte V1 canonical block header
|| big-endian u32 transaction count (1..64)
|| count * (32-byte transaction ID
            || 118-byte canonical signing payload
            || 64-byte detached Ed25519 signature)
```

The decoder checks the domain bytes, exact length (no trailing data),
canonical header format, V1 chain and version, SHA-256 block ID, zero
placeholder puzzle-proof root, bounded count and each signed record. It
reconstructs sender ownership directly from the 32-byte Ed25519 public key,
verifies the signature, regenerates each ID, rejects duplicate IDs and
checks the ordered transaction Merkle root.

The **read-only replay audit** then:

1. Reads the tip, signed block bytes and height from RocksDB.
2. Loads the exact parent block, recomputes its canonical block ID, and
   verifies consecutive heights.
3. Compares a caller-provided **independently trusted predecessor ledger**
   against the parent block's committed ledger-state root.
4. Passes the **decoded, verified transactions from disk** to the trusted
   Haskell signed-batch executor. Haskell independently checks signatures,
   order, nonces, balances, fees and arithmetic.
5. Compares the Haskell-computed successor root with the stored header's
   state root and compares the complete successor ledger snapshot with
   RocksDB's saved ledger.
6. Returns the reconstructed ledger without writing to storage.

Tests reject malformed/truncated/trailing block data, changed IDs,
altered signing payloads/signatures, reordered records, mismatched
block ID, invalid counts, wrong predecessor ledger and deliberately
corrupted on-disk records. The end-to-end demo checks that failure
does not change ledger balances or the block tip.

## Limitations and next steps

The predecessor ledger must already be trusted. This audit does not
recursively reconstruct chain history from an independently authenticated
genesis; it can audit only a current single batch against a known
predecessor. Genesis allocations, proposer identity, puzzle proofs and
validator votes are not checked. Low-level storage writes remain
available to trusted local code, and no rollback/reorg handling or
network finality is implemented. The Haskell IPC response decoder is
still fixture-oriented.

**Next suggested step:** replay a complete local chain from a pinned
genesis checkpoint, with bounded history and explicit tamper detection.
