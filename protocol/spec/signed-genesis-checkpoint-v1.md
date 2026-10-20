# Ed25519-signed genesis checkpoints — experimental V1

**Status:** An externally anchored local-chain initialization and audit
fixture. This is NOT validator consensus, an authorized real token launch, or
a production trust-distribution mechanism.

## Trust anchor

An operator must obtain the **32-byte Ed25519 verifying key** of the
genesis/checkpoint authority through an authenticated channel independent of
the checkpoint and RocksDB. The key **must not** be learned from the
untrusted manifest itself, a peer-supplied field, or the local chain history.

Rust's `verify_signed_genesis_checkpoint` verifies the supplied Ed25519
signature against that externally pinned key *before* it uses any account
allocation, chain ID, state commitment or block ID.

The separate `SignedGenesisCheckpointV1` value contains a canonical
178-byte genesis header, a typed initial ledger (accounts, balances, nonces,
fee pool and settlement replay sets), and a 64-byte Ed25519 signature.
No signing private key is stored with the checkpoint.

## Canonical signature preimage

```text
ASCII("AIVOBLOCK-GENESIS-CHECKPOINT-V1") || 0x00
|| genesis_header_v1[178]
|| uint32_be(length(typed_genesis_ledger_bytes))
|| typed_genesis_ledger_bytes
```

The typed bytes use the existing ordered
`AIVOBLOCK-LOCAL-LEDGER-V1\0` encoding. Length is capped at 1 MiB.

Before verifying the signature, the decoder requires the canonical header
and enforces version 1, chain ID 1, height 0, zero parent, the canonical
empty transaction root, an all-zero **placeholder** AI-proof root, and a
state root equal to the hash of the supplied typed genesis ledger.
The genesis block ID is SHA-256 of exactly the 178 header bytes.

Changing any signing field—including a nonce, allocation, fee pool,
timestamp, state root or header—invalidates the signature (or is rejected as
a malformed checkpoint). The signature is checked with Ed25519's strict
verification function.

## Local initialization and full history audit

`initialize_signed_genesis(db, checkpoint, external_authority_key)`
verifies the signature **before writing anything**, then atomically inserts
the canonical header, computed genesis ID and initial ledger into an
**empty** RocksDB chain. Older legacy storage modes and an already
initialized ledger fail closed. Invalid signatures must leave tip/state
absent.

`replay_chain_from_signed_checkpoint(db, haskell, checkpoint, key)`
checks that same external authority before using the signed genesis
allocation as the starting ledger for the existing complete historical
audit. The historical audit independently decodes signed transaction bodies,
runs Haskell execution and checks state roots, chain ancestry and the
persisted final ledger.

CI's `multi-transfer-demo` uses a **fixed test-only** Ed25519 authority
seed of 32 bytes of `11` (hexadecimal), checks a wrong authority key is
rejected before genesis storage, and audits two historical multi-transfer
blocks after reopening RocksDB. This key is deliberately unsafe for
production and **must never control valuable assets**.

Unit regressions test modified balances/nonces/fees, signature tampering,
changed header fields, wrong authority keys, non-genesis header rejection,
duplicate genesis initialization and reopen durability.

## Remaining limitations

- There is no decentralized decision on which authority public key is
  legitimate. A valid signature establishes only authorization by the key
  the local operator chose to trust.
- The demo generates its own fixed signing key; a real deployment requires
  an independently distributed network-specific trusted-key policy, securely
  signed release artifacts, authority-key rotation and recovery procedures.
- The Rust lower-level storage APIs and older audit entry point still
  accept locally trusted caller inputs. Signed-checkpoint verification is a
  **checked entry point**, not enforcement throughout every storage writer.
- Signed genesis does not provide P2P, validator voting, fork-choice,
  dynamic validator membership, AI-work proof validation, an authenticated
  rewards ledger, arbitrary crash fault injection, or actual cryptocurrency.
- No remote peer inputs may be considered trusted solely because they
  include a manifest and self-declared authority key.
