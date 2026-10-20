# Ordered signed multi-transfer blocks — local V1 candidate

**Status:** Test-chain prototype, not decentralized consensus or real token
payments. The existing single-transfer format remains supported separately.

## Execution

The Rust node constructs a nonempty ordered list of 1–64 signed V1
transfers. Every transaction uses the canonical
`AIVOBLOCK-TRANSFER-V1\0` Ed25519 signing preimage (chain ID 1), and
the sender must equal the raw signing public key.

1. Rust checks every signature and computes canonical transaction IDs,
   rejecting exact duplicate IDs before contacting Haskell.
2. Rust transmits the prior typed ledger snapshot and ordered signed
   envelopes to the locally trusted Haskell `ledger_signed_batch_fixture`
   IPC endpoint.
3. Haskell independently verifies all Ed25519 signatures and rejects any
   duplicate IDs before beginning pure execution.
4. Haskell folds `applyVerifiedTransfer` **in order** over the snapshot,
   enforcing each sender nonce, balance, fee accounting and overflow rule
   against the ledger from the preceding transfer. If **any** transfer fails,
   no successor ledger is returned. There are no persistent intermediate
   writes.
5. Haskell returns the computed successor snapshot, ordered transaction
   IDs, transaction Merkle root, and state root. Rust independently checks
   all three commitment fields and canonical snapshot bytes.
6. Rust constructs a candidate 178-byte V1 block header containing that
   transaction root, post-ledger state root, verified parent, height,
   timestamp and an explicitly empty placeholder AI-proof root.
7. A checked Rust wrapper serializes **every signed envelope**, verifies
   the transaction/state roots, checks the preceding RocksDB ledger,
   and calls the existing synced RocksDB atomic block+ledger write.
   The block bytes, tip, height, balances/nonces and fees persist together.
8. After reopening RocksDB, the demo verifies the complete stored
   body, all signatures and transaction order, resulting state and tip.

## Signed batch body representation

```text
ASCII("AIVOBLOCK-TX-BATCH-BODY-V1") || 0x00
|| canonical_block_header_v1[178]
|| uint32_be(number_of_transactions)
|| for each signed transfer in order:
     transaction_id[32]
     transfer_signing_preimage[118]
     Ed25519_signature[64]
```

Merkle leaves and internal nodes reuse the domain-separated ordered V1
algorithm in `transaction-block-commitment-v1.md`. Duplicating a signed
transaction is rejected, rather than counted as two valid transfers.
This local executable limits accepted batches to **64**, smaller than the
pure Merkle function's 65,536-item capacity.

## Automated checks

- Two properly signed transactions in **one block**: Alice starts with
  100 units, pays 10+2 then 5+1, ends at balance 82 and nonce 2;
  Bob receives 15 and the fee pool holds 3; supply remains 100.
- Duplicate transaction ID and out-of-order nonce rejection.
- Validly signed later overspend rejection, and later signature tampering.
- Empty / oversized transaction batch rejection.
- Incorrect transaction Merkle root and ledger state root rejection
  without touching the stored ledger.
- Atomic RocksDB reopen and exact multi-transaction signed body verification.
- Existing single-transfer and AI-worker diagnostics remain tested.

## Explicit gaps

This implementation **does not** prove that the caller's initial genesis
allocation was authorized, nor prevent trusted local code from calling
low-level storage APIs with arbitrary data. The Haskell IPC endpoint is a
local fixture and currently uses a limited JSON response parser. There is
no consensus-authorized block proposer, validator voting, P2P propagation,
network fork choice, rollback/reorg handling, mempool, proof-of-useful-work
verification or actual mining rewards in these transfer blocks. Transaction
and state commitments alone do not confer consensus finality or value.
