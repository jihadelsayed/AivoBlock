# AivoBlock Architecture

**Version:** 0.1 research prototype  
**Status:** Work in progress — not an operational blockchain.

## Purpose

AivoBlock is a **new, custom decentralized blockchain** designed to coordinate collaborative AI computation puzzles. It does not rely on Ethereum or another blockchain and does not propose creating an additional token. Sayed Token is the intended native ledger asset, subject to final protocol design.

## Monorepo components

| Directory | Language | Responsibility |
| --- | --- | --- |
| `core/` | Haskell | Pure ledger rules, block validation, voting primitives, proof structures and rewards |
| `node/` | Rust | Node orchestration, local storage, P2P networking, mining scheduling and IPC |
| `ai-engine/` | Python | AI puzzle execution, deterministic inference experiments and proof commitments |
| `protocol/` | Protobuf + Markdown | Wire messages, consensus specification and canonical encoding |
| `tests/` | Multiple | Cross-language and multi-node integration tests |
| `docker/` | Docker | Reproducible node packaging and local test deployment |

## Intended node architecture

```text
              AivoBlock peer-to-peer network
                         |
                Rust Node Runtime
              /        |         \
        Local DB   Haskell Core   Python Worker
        RocksDB    Validation     AI Puzzles
```

Every independently operated full node maintains its own ledger and data directory. Rust manages the node runtime and communicates locally with Haskell and Python through length-prefixed IPC messages. Independent Haskell checks must approve consensus-critical state transitions. Python worker outputs are untrusted until the relevant verification rules succeed.

**Decentralization is determined by consensus, validator control, peer discovery and participation rules, not by eliminating HTTP APIs or databases.**

## Proposed block processing

1. Obtain transactions and AI puzzle contributions from peers.
2. Validate input format, signatures, puzzle definitions and cryptographic commitments.
3. Recompute or otherwise independently verify AI results according to versioned puzzle rules.
4. Execute the deterministic ledger transition and calculate the resulting state commitment.
5. Propose and vote on blocks through a fully specified Byzantine fault-tolerant protocol.
6. After finality, atomically persist the block, state and accounted rewards locally.
7. Distribute the finalized block to peers for independent validation.

**Steps 2–7 are design targets, not a completed end-to-end implementation.**

## Development versus production design

- The initial research testnet uses a **fixed permissioned validator set**. A genuinely permissionless consensus and Sybil-resistance mechanism is an open research problem.
- Existing Haskell voting helpers only calculate quorum weights; they do not finalize blocks.
- The current Python puzzle uses deterministic integer linear inference. Verification currently repeats the computation, so inexpensive verification of arbitrary AI work is **not solved**.
- The Rust scheduler operates locally and does not guarantee decentralized task assignment.
- RocksDB storage is local per node; atomic finalized-block writes and recovery still need implementation.
- Protobuf describes transport messages. Consensus hashes require separate canonical encodings and cross-language test vectors.
- Docker Compose currently launches diagnostic commands, not connected mining nodes.

## Security boundaries

- Reject unauthenticated transactions, votes and worker identities.
- Never equate a result commitment with a correctness proof.
- Do not trust remote assertions that a block or AI result is verified.
- Bind signatures and proofs to chain ID, protocol version and exact task data.
- Limit untrusted input sizes, execution time and resource allocation.
- Keep consensus-critical input encodings deterministic across platforms.
- Never award rewards before the corresponding validated chain transition is finalized.

## Delivery milestones

1. Compile and test Haskell, Rust and Python independently.
2. Establish canonical encodings and shared test vectors.
3. Implement Rust-to-Haskell and Rust-to-Python typed IPC.
4. Build a complete block validation and persistence path.
5. Connect three independently configured P2P nodes.
6. Demonstrate a verified collaborative AI task and finalized block.
7. Adversarially test invalid proofs, forks, restarts and disconnected peers.

See [consensus](../protocol/spec/consensus.md), [puzzle rules](../protocol/spec/puzzle-rules.md) and [canonical encoding](../protocol/spec/canonical-encoding.md) for protocol details.
