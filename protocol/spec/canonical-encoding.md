# AivoBlock Canonical Encoding — Draft v0.1

**Status:** Design draft. Only the integer-inference result commitment below is implemented. Other formats MUST NOT be used for consensus until implemented and tested across languages.

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

## Future consensus encodings (NOT defined yet)

Before enabling block or transaction acceptance, specify exact byte layouts, hashing algorithms, domain tags and test vectors for:

1. Chain identity and genesis configuration.
2. Block headers and block IDs.
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

**No completed canonical encoding for blocks or transactions is claimed by this draft.**
