# AivoBlock AI Puzzle Rules — Research Draft v0.1

**Status:** Experimental specification. Not consensus-ready.

## Goal

AivoBlock divides a useful computational task among independent workers, records result commitments and verifies contributions before any reward is considered. The first workload is **deterministic integer linear inference**, used to test the protocol. This is not proof that arbitrary AI work can be cheaply verified.

## Puzzle definition

A puzzle contains:

| Field | Meaning |
| --- | --- |
| `puzzle_id` | 32-byte identifier binding the task definition (derivation TBD) |
| `version` | Positive protocol version |
| `model_commitment` | 32-byte commitment to fixed model parameters |
| `input_commitment` | 32-byte commitment to immutable task inputs |
| `rules_commitment` | 32-byte commitment to execution and verification rules |
| `units` | Positive count of independently assignable work units |
| `deadline_height` | Last eligible block height under future consensus rules |

All commitments must be validated against trusted task contents. **Hashes alone do not establish correctness or availability.** The canonical encoding, puzzle ID derivation, and input-availability protocol are not yet defined.

## Initial computation: integer inference

Each work unit processes one input row with fixed integer weights:

`output_i = sum(input_i[j] * weight[j])`

Rules implemented in the Python prototype:

- All arithmetic uses exact Python integers, not floating point.
- Model has 1–4096 weights; each weight and input feature has absolute value at most 1,000,000.
- Each execution accepts at most 1,024 rows; row length must match weight length.
- A range is a contiguous interval `[start, start + count)` with positive `count`, fully inside `[0, units)`.
- A node-local scheduler assigns non-overlapping ranges. **It is not a distributed assignment authority.**
- Results are committed using SHA-256 over the domain-separated encoding implemented in `ai-engine/src/aivoblock_ai/executor.py`.

The prototype commitment encoding is: ASCII `AIVOBLOCK-INFERENCE-V1`, followed by a zero byte; then unsigned 64-bit big-endian `start` and `count`; then for each output, unsigned 32-bit big-endian length followed by its signed decimal ASCII representation. This is a **provisional result encoding**, not a finalized chain-wide canonical serialization standard.

## Worker proof

A proof contains the puzzle ID, worker ID, start unit, count, result commitment, and optional witness commitment. All ID/commitment fields must be 32 bytes. A proof's worker ID is a claim until authenticated by a signature bound to the puzzle, range and result.

The worker's result **must not** be classified as verified simply because the worker submitted it or computed a hash.

## Verification requirements

For the present deterministic prototype:

1. Resolve the puzzle definition from a trusted, immutable source.
2. Check protocol version, commitments, deadline and work-range bounds.
3. Authenticate the worker and reject replayed or duplicate contribution claims.
4. Obtain the committed model and input values; recompute their hashes.
5. Independently recompute the assigned outputs with the exact integer rules.
6. Compare the result commitment in constant time.
7. Check overlapping contributions and record a verified status only after all required checks succeed.

Currently, `verify_by_recomputation` implements **only** range/input-count checks and step 5–6; it does not authenticate workers, validate input/model commitments, enforce deadlines, or handle replay protection. Full verification must be implemented before consensus uses puzzle results.

## Rewards and consensus

Verified contributions may be weighted by accepted work units, but rewards must be calculated from authenticated and nonduplicated work using a consensus-defined budget. The Haskell reward helper distributes an existing budget; it does not mint tokens.

**Puzzle success is separate from consensus finality.** Mining contributions do not inherently provide Sybil resistance, fair proposer selection or Byzantine safety. The private prototype uses a fixed validator set while these mechanisms are researched.

## Open research problems

- Permissionless puzzle issuance and assignment without coordinator control.
- Independent input/model availability and immutable content binding.
- Cross-language agreement on exact encodings and hashes.
- Reducing verification cost below full recomputation.
- Detecting copied proofs, collusion, duplicate or overlapping work.
- Handling adversarial workers, timeouts, reassignments and partial failures.
- Extending beyond deterministic integer inference to useful neural-network tasks while handling hardware nondeterminism.

## Minimum acceptance tests

- Partitioned work covers all task units without gaps or overlap.
- Equal inputs and rules yield identical result commitments.
- Tampering with a result, work range or task binding is rejected.
- Two nodes independently verify the same fixed test vectors.
- Failed, duplicate and forged contributions never receive rewards.

This document deliberately leaves unresolved mechanisms labeled as research tasks rather than claiming the prototype provides secure proof of useful AI work.
