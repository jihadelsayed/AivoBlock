# Milestone 22 — Authenticated validator quorum certificates V1

**Status:** Bounded local, permissioned precommit-certificate verifier. This
implements only authenticated vote tallying against an **externally trusted
static validator set**. It is NOT a full BFT consensus or block-finality
protocol, and is not secure for network deployment.

## Trusted validator set

A validator is identified by its 32-byte Ed25519 public key and has a positive
unsigned-64-bit voting-power weight. A valid local set contains 1–64 distinct
valid Ed25519 public keys and its total voting power fits in u64. Members
are canonically sorted lexicographically by public-key bytes.

The validator-set commitment is:

~~~text
SHA256(
    ASCII("AIVOBLOCK-VALIDATOR-SET-V1") || 0x00
    || chain_id:u32_be (must be 1)
    || number_of_validators:u32_be
    || sorted(validator_public_key[32] || voting_power:u64_be)
)
~~~

The active set and its weights MUST come from independently authenticated
configuration such as a future genesis commitment / validator governance
transition. A validator set presented in a candidate block is untrusted.

## Signed precommit vote

Each signer authenticates:

~~~text
ASCII("AIVOBLOCK-PRECOMMIT-V1") || 0x00
|| chain_id:u32_be
|| validator_set_commitment[32]
|| height:u64_be (nonzero)
|| round:u64_be
|| block_id[32] (nonzero)
|| signer_ed25519_public_key[32]
~~~

Every signed vote contains a signer public key and its 64-byte Ed25519
signature. Votes in a certificate are strictly sorted by the signer's
public key. Duplicate, unknown, malformed, reordered and forged signatures
reject the entire certificate, as do changed chain, block, height, round and
validator-set commitments.

The quorum threshold is **floor(2 * total_voting_power / 3) + 1**.
All arithmetic uses checked or widened integers. This is the exact same
strict greater-than-two-thirds voting-power rule already specified by the
Haskell core consensus helper. For a validator set with powers 3, 2, 1,
a 3+2 certificate succeeds and a 3+1 certificate fails.

## Attesting an independently verified local tip

The new read-only helper accepts the output of signed-genesis full chain
replay (which rechecks the on-disk V2 AI proof and deterministic Haskell
ledger transitions). It then:

1. Authenticates the certificate's signatures against the externally
   trusted validator set and strict quorum requirement.
2. Requires the signed block ID to equal the independently replayed
   canonical tip.
3. Requires the persisted RocksDB tip and height metadata to match.
4. Requires the block to be present in RocksDB.

An attestation **does not itself validate a block's state**. The caller
must perform the independent signed-genesis replay first and must not
fabricate the audit report. A compromised local process can bypass the
helper; no node-network adversary model is covered yet.

The Rust/Python/Haskell V2 proof end-to-end diagnostic now exercises this
additional local attestation after independently validating and replaying
the AI proof block. It checks valid 5/6 signed voting power and rejects
certificates for a different block ID or height. A standalone CLI
validator-quorum-demo checks authentication, quorum and tampering.

## Remaining consensus requirements

No validator governance on chain, vote exchange/P2P, proposer selection,
lock/unlock rules, equivocation handling, persistent anti-double-sign
locks, round timeouts, view changes, fork-choice, finalization commits or
partition recovery exist. Signatures from >2/3 of a statically configured
set are an **authenticated certificate primitive**, not proof that any
running consensus algorithm is safe or live.

**Next milestone:** Define and test a deterministic validator-round state
machine with persistent signing locks and conflicting-vote rejection,
before any network or finality claims.
