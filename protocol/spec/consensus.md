# AivoBlock Consensus Protocol — Research Draft

**Status:** Draft v0.1, NOT a secure or deployable consensus specification.

## Purpose

AivoBlock is a custom blockchain intended to coordinate **collaborative useful AI computation**. This document separates three distinct problems:

1. **Task execution:** workers compute assigned AI puzzle ranges.
2. **Proof verification:** validators determine whether claimed results satisfy puzzle rules.
3. **Consensus:** independent nodes agree on a single ordered, finalized history in the presence of faults or malicious participants.

A valid AI result alone is **not** a consensus mechanism and does not prevent Sybil attacks.

## Roles

- **Worker:** executes AI work and submits a task-bound result commitment.
- **Proposer:** proposes a block with transactions and referenced puzzle proofs.
- **Validator:** independently checks blocks and casts authenticated votes.
- **Full node:** retains and validates the finalized ledger, without necessarily mining.

Roles may overlap, but a worker's claimed work **never automatically grants voting power**.

## Initial private-testnet trust model

For the first research prototype only:

- Genesis defines a **fixed, permissioned validator set**, with unique validator identities and positive voting weights.
- Every node validates the same genesis commitment, chain ID and protocol version.
- At most **less than one-third** of total voting power is Byzantine, under the usual assumptions of a correctly implemented BFT protocol and sufficient network synchrony.
- Vote authentication, validator membership changes, anti-equivocation evidence and recovery after partitions remain **unimplemented**.

**Do not interpret the Haskell `hasQuorum` helper as finality.** It only tallies already-authenticated votes and checks a strict >2/3 threshold.

## Tentative block proposal sequence

1. A validator obtains a task and previously submitted candidate proofs.
2. Workers process independent, non-overlapping work ranges.
3. Verification checks puzzle identity, immutable input/model/rules commitments, range correctness and the task-specific computation.
4. The proposer builds a candidate block referencing eligible proofs and transactions.
5. Each validator checks the block, canonical commitments, signatures, resulting ledger state and all puzzle proofs required by consensus.
6. Validators execute a **future fully specified** BFT proposal/vote/lock/finalization algorithm.
7. Only after finality do nodes commit the block and account/reward changes atomically.

Steps 3, 5 and 6 are **design targets**; they are not presently wired together.

## Voting and quorum primitives (implemented in Haskell)

- `Round` identifies a block height and round number.
- `ValidatorSet` maps validator IDs to positive integer voting power.
- The sum of voting weights must fit in `Word64`.
- `Vote` refers to one round and either a block commitment or nil.
- Duplicate votes, unknown validators and round mismatches cause tally failure.
- For total voting power `T`, the quorum threshold is `floor(2T / 3) + 1`.

These are **necessary building blocks, not an implemented consensus algorithm**.

## Safety requirements before block finalization exists

A future complete protocol must define:

- Authenticated proposal and vote formats with chain-ID and round domain separation.
- Validator-set activation and updates, and a Sybil-resistant membership mechanism if the network becomes permissionless.
- Proposer selection, prevote/precommit or equivalent phases, locks/unlocks and fork-choice/finality rules.
- Timeouts, round progression, retransmission and network partition recovery.
- Byzantine equivocation evidence and handling.
- Genesis parameters, timestamp validity, deterministic transaction execution and state-root calculation.
- Canonical byte encodings, block IDs, transaction IDs and Merkle-tree construction.
- Puzzle scheduling that cannot be monopolized by one coordinator.
- Objective proof verification and a strategy for costly or nondeterministic AI outputs.
- Reward calculation from *verified* contributions, with explicit supply and fee accounting.
- Persistence that atomically commits a finalized block and its state; recovery must never claim a half-written block is finalized.

## AI puzzle security limitations

The initial integer-linear inference experiment allows exact recomputation, which may cost roughly as much as execution. Its hashes **commit to** outputs; they do not prove correctness or that work was done by a particular worker. Production-capable useful-work verification remains a research problem. Never accept claimed proof status from a peer without checking it.

## Implementation boundary

| Component | Responsibility |
| --- | --- |
| Haskell `core/` | Pure deterministic validation, ledger and voting primitives |
| Rust `node/` | P2P, local persistence, scheduling and process integration |
| Python `ai-engine/` | Untrusted AI computation and experimental proof generation |
| `protocol/` | Shared message definitions and canonical consensus specification |

## Criteria to advance beyond the draft

1. Specify and test canonical encoding vectors across Haskell/Rust/Python.
2. Implement authentication and signature verification for transactions and votes.
3. Define and implement a complete BFT consensus state machine with safety tests.
4. Validate puzzles against independently trusted inputs, then enforce reward accounting.
5. Demonstrate three or more isolated nodes recovering from disconnects, invalid proofs and conflicting block proposals.
6. Obtain independent protocol/security review **before** claiming public-network security.

**No mainnet or economic security guarantee is made by this draft.**
