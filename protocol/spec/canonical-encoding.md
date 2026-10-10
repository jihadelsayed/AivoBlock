# AivoBlock Canonical Encoding — Draft v0.1

**Status:** Design draft. Integer-inference result and model commitments are implemented. The block-header encoding below is a newly specified candidate, **not yet implemented or approved for consensus**. Other formats MUST NOT be used for consensus until implemented and tested across languages.

## Why this matters

AivoBlock uses Rust, Haskell and Python. Independent nodes must hash **identical bytes** for identical protocol objects. Protobuf is suitable for transport but is not the consensus-canonical encoding.

## General rules (proposed)

- Every committed object has a unique ASCII domain tag and version.
- Integers are unsigned, fixed-width, big-endian unless explicitly specified.
- Fixed hashes/commitments are exactly 32 raw bytes.
- Variable-length byte strings receive an unsigned 32-bit big-endian byte-length prefix.
- No implicit Unicode normalization or locale-dependent formatting.
- Collections have an explicitly specified order; sets must be sorted by their canonical byte keys.
- Unknown fields and unsupported versions are rejected for consensus objects.
- Decode then re-encode before hashing to reject noncanonical representations.
- Never use floats, machine-native struct memory layout, generic JSON, or default Protobuf serialization for block IDs or signature preimages.

These are proposed conventions, not yet an implementation.

## Implemented local prototype: integer inference results

Defined in `ai-engine/src/aivoblock_ai/executor.py`:

```text
ASCII("AIVOBLOCK-INFERENCE-V1") || 0x00
|| uint64_be(start_unit)
|| uint64_be(unit_count)
|| repeat(unit_count times):
     uint32_be(length(signed_decimal_ascii(output)))
     || signed_decimal_ascii(output)
```

The output decimal representation has no leading plus, no leading zeros (except `0`), and uses `-` for negative values. The result commitment is:

```text
SHA256(encoded_inference_result)
```

This commitment binds an output sequence and its range. It **does not** bind the puzzle ID, model, or input by itself; those bindings must be checked separately before any proof is accepted.

## Model commitment (implemented local prototype)

Defined in `ai-engine/src/aivoblock_ai/models.py`:

```text
ASCII("AIVOBLOCK-INT-LINEAR-V1") || 0x00
|| uint32_be(model_version)
|| uint32_be(number_of_weights)
|| repeat(number_of_weights times):
     int64_be(weight)
```

The model commitment is `SHA256(encoded_model)`. The only supported model version is 1. Current bounds are 1–4096 weights and absolute weight values no greater than 1,000,000.

## Proposed block-header ID — candidate V1 (NOT implemented)

A block identifier is the **SHA-256 digest of the canonical block header bytes**.
The body is not serialized into the header ID; instead the transaction,
AI-proof, and state root fields are intended to commit to the relevant data.
Until those roots are independently recomputed and verified, a matching
header digest does **not** authenticate the body or ledger state.

The exact candidate byte layout, in field order, is:

```text
ASCII("AIVOBLOCK-BLOCK-HEADER-V1") || 0x00  (26 bytes total)
|| uint32_be(headerVersion)                  (4 bytes)
|| uint32_be(headerChainId)                  (4 bytes)
|| uint64_be(headerHeight)                   (8 bytes)
|| raw32(headerParentHash)                   (32 bytes)
|| raw32(headerTransactionsRoot)             (32 bytes)
|| raw32(headerPuzzleProofsRoot)             (32 bytes)
|| raw32(headerStateRoot)                    (32 bytes)
|| uint64_be(headerTimestampMs)              (8 bytes)
```

The total preimage is **178 bytes**. Hash algorithm: SHA-256, producing exactly
32 raw bytes. Hash values are represented as lowercase 64-character hex
**only for display**; hex text is never substituted for the raw bytes
in the preimage. Fields are unsigned and fixed-width, so negative numbers,
overflow, missing fields, and non-32-byte commitments must be rejected.
No JSON serialization, field separators, implicit padding, or machine
endianness are allowed. The domain tag above is case sensitive.

**Provisional test vector:** version=1, chain ID=1, height=0,
timestampMs=1000, and all four 32-byte hashes/roots equal to 32 zero bytes.
The expected SHA-256 digest is:

```text
c1b8e653731eeafeacb557e46669d277096ecc7ceff7ec911ab29b25a8a55c22
```

This vector was calculated from the specified 178-byte preimage; future
Rust and Haskell implementations must independently reproduce it.
Defining a header digest does not yet establish a valid genesis block,
valid Merkle roots, signatures, chain ancestry, or consensus acceptance.

## Remaining consensus encodings (NOT implemented yet)

Before enabling block or transaction acceptance, specify exact byte layouts, hashing algorithms, domain tags and test vectors for:

1. Chain identity and genesis configuration.
2. Independent implementations and cross-language test vectors for the block-header encoding proposed above.
3. Transaction signing payloads and transaction IDs.
4. Merkle leaves, internal nodes, empty-root rules and tree construction.
5. Ledger/account state roots.
6. AI puzzle IDs, input commitments, proof IDs and worker signatures.
7. Authenticated validator votes, rounds and quorum certificates.
8. Reward distributions and block state transitions.

Version changes affecting canonical bytes require explicitly defined protocol upgrades.

## Cross-language verification gate

Before these formats become consensus rules:

- Publish fixed positive and negative test vectors in `protocol/test-vectors/`.
- Implement encoders/decoders in the relevant Rust, Haskell and Python components.
- Check byte-for-byte equality and identical digests across independent implementations.
- Test malformed lengths, integer overflow, duplicate entries, ordering differences, unsupported versions and replay contexts.

**The candidate block-header layout is specified but not implemented or consensus-ready; complete block/transaction encodings remain outstanding.**
