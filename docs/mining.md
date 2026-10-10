# AivoBlock Collaborative AI Mining

**Status:** Research design v0.1 — not an operational mining or consensus protocol.

## Objective

AivoBlock is a custom blockchain designed to reward **verified, useful AI computation**. Workers collaborate on puzzle tasks instead of performing arbitrary hash-search work. This document describes intended behavior, prototype functionality, and unresolved security challenges.

## Participants

- **Puzzle issuer:** Proposes a task with immutable model, input, and verification-rule commitments. Permissionless issuance rules remain unspecified.
- **Worker:** Executes one or more assigned work ranges using the Python AI engine.
- **Verifier:** Checks submitted results against the puzzle specification and independently trusted inputs.
- **Validator:** Authenticates consensus messages and participates in block agreement under a separately defined protocol.
- **Full node:** Stores and independently validates the finalized blockchain.

A worker may also operate a validator, but completing AI work does **not** automatically grant voting power.

## Puzzle lifecycle

1. **Define:** Publish a versioned puzzle ID, model and input commitments, rule commitment, number of work units, and deadline.
2. **Distribute:** Divide the task into contiguous, non-overlapping ranges. The current Rust scheduler does this **locally**, not across independent peers.
3. **Compute:** Workers perform the specified deterministic workload and compute result commitments.
4. **Submit:** Workers send task-bound result and witness commitments, accompanied by authenticated identity in a future protocol.
5. **Verify:** Independent verifiers check task identity, inputs, work ranges, output correctness, duplicates, and deadline.
6. **Include:** A block proposer references accepted contributions; validators recheck consensus-critical conditions.
7. **Finalize:** Only after block finality are rewards credited using the native Sayed Token ledger.

## First experimental puzzle

The Python prototype performs integer linear inference:

```text
For each assigned input row:
    output = sum(input[j] * weight[j])
```

Integer arithmetic avoids floating-point differences across common hardware. The implementation bounds model dimensions, input values, and execution batch sizes. Its SHA-256 result commitment is deterministic for identical output sequences and work ranges.

This workload is intended to demonstrate work splitting and checking. It does **not** demonstrate efficient verification of arbitrary neural-network training or inference.

## Reward accounting

The Haskell `distributeRewards` helper proportionally splits an existing reward budget among unique workers with positive, **previously verified** contribution weights.

Rules still to define include:

- Who funds the reward budget and how existing Sayed Token supply is represented.
- How work weights are measured fairly and independently.
- How replayed, copied, overlapping, fraudulent, and colluding submissions are rejected.
- How fees and rewards are applied atomically to the finalized ledger.
- What happens to incomplete or failed tasks.

A result hash alone never earns a reward.

## Security model and unresolved challenges

| Risk | Required mitigation or research |
| --- | --- |
| Fake AI output | Independently check committed input/model data and recompute or verify a sound proof |
| Copied or duplicate work | Authenticate workers, uniquely bind task and range, reject duplicate reward claims |
| Centralized task assignment | Design distributed puzzle selection and scheduling without a privileged coordinator |
| Malicious puzzle issuers | Validate resource bounds, task availability, eligibility, and immutable rules |
| Nondeterministic AI models | Specify reproducible execution or mathematically sound verification tolerances |
| Sybil miners | Define a separate Sybil-resistant participation and consensus policy |
| Chain forks | Implement authenticated consensus, locking, fork handling, and finality |
| Denial of service | Limit task size, proof size, worker execution time, and concurrent jobs |

**Crucial distinction:** useful computation, verified computation, and decentralized consensus are three different properties. Success in one does not establish the others.

## Current implementation

- **Rust:** local non-overlapping range scheduler with no P2P assignment or retry protocol.
- **Python:** deterministic integer inference, result commitments, and verification by full recomputation.
- **Haskell:** proof-envelope structures and proportional reward calculations.
- **Protocol:** draft puzzle message schemas and acceptance requirements.

No live multi-node mining, automatic rewards, cryptographically sound proof-of-useful-work system, or public consensus network is currently implemented.

## First mining demonstration target

Run three isolated development nodes with a shared, immutable puzzle. Assign different work ranges, execute them, validate each submission against independent inputs, and show that a single finalized block can reference the accepted results.

Completing this experiment would establish a **prototype demonstration**, not a secure permissionless blockchain.

Related specifications: [puzzle rules](../protocol/spec/puzzle-rules.md), [consensus](../protocol/spec/consensus.md), and [canonical encoding](../protocol/spec/canonical-encoding.md).
