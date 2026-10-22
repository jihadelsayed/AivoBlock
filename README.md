# AivoBlock

**A custom blockchain for collaborative, verifiable AI computation.**

AivoBlock is an experimental decentralized blockchain backend. Its goal is to divide useful AI workloads among participating workers, verify submitted results, and record accepted work through a custom ledger and consensus protocol.

> **Development status:** Research prototype with a working **private Docker testnet** ([guide](docs/testnet.md)): authenticated nodes distribute a deterministic AI task across workers, every validator independently recomputes the results, a > 2/3 validator quorum finalizes the block, and native Sayed Token rewards are settled and recovered after restart. It is **not** a public network, and it does **not** cryptographically or succinctly prove arbitrary AI computation: verification is full recomputation of a small integer workload.

## What makes AivoBlock different?

AivoBlock investigates whether blockchain participants can contribute **useful AI computation** rather than relying exclusively on arbitrary computational puzzles. The design separates three problems:

1. **Computation:** distribute AI puzzle units and execute them.
2. **Verification:** independently check outputs and bind them to immutable task definitions.
3. **Consensus:** agree on finalized blocks despite faulty or malicious participants.

A correct AI result does **not**, by itself, provide Byzantine fault tolerance, finality, or Sybil resistance. These are active protocol design challenges.

AivoBlock is a **new blockchain**, not an integration with Ethereum or another chain. **Sayed Token** is the native ledger asset. The testnet uses a genesis-bound, capped halving issuance schedule ([spec](protocol/spec/sayed-token-issuance-v1.md)) whose parameters are illustrative, not a final economic design. This project does not propose an additional token.

## Technology stack

| Component | Technology | Responsibility |
| --- | --- | --- |
| Blockchain core | Haskell, Cabal | Deterministic ledger transitions, block validation, voting primitives, rewards |
| Node runtime | Rust, Cargo | Local node operation, planned P2P, RocksDB persistence, IPC |
| AI engine | Python | Puzzle workloads, deterministic inference, proof commitments |
| Transport contracts | Protocol Buffers | Versioned messages between nodes and components |
| Private testnet | Docker Compose | Three-node and four-validator networks with an end-to-end harness |
| Automated checks | GitHub Actions | Python, Haskell, Rust and Protobuf jobs |

No central PostgreSQL or FastAPI service is required.

## Repository layout

```text
AivoBlock/
├── core/                 # Haskell protocol and ledger
├── node/                 # Rust node runtime
├── ai-engine/            # Python AI workloads
├── protocol/
│   ├── proto/            # Transport message schemas
│   └── spec/             # Consensus and encoding drafts
├── docs/                 # Architecture, development, mining
├── docker/               # Node Dockerfile
├── scripts/              # Local setup and smoke tests
├── tests/                # Planned multi-component testing
├── compose.yaml
└── README.md
```

## Getting started

Clone:

```bash
git clone https://github.com/jihadelsayed/AivoBlock.git
cd AivoBlock
```

**Python AI tests** (Python 3.11+):

```bash
cd ai-engine
python -m pip install -e ".[dev]"
python -m pytest
```

**Haskell core** (GHC and Cabal required):

```bash
cd core
cabal update
cabal build all
cabal test all
```

**Rust node** (Rust/Cargo and RocksDB build prerequisites required):

```bash
cd node
cargo test
cargo run -- status
```

**Private testnet** (Docker + Python 3.11; builds Haskell, Rust and Python inside the image):

```bash
python3 scripts/testnet.py all
```

This runs the three-node end-to-end demonstration and the four-validator fault-tolerance scenario and verifies every balance independently. See the [testnet guide](docs/testnet.md) for what is checked, results and limitations, and [development instructions](docs/development.md) for native builds.

## Design documents

- [Private testnet](docs/testnet.md) — architecture diagrams, commands, verified behavior, limitations.
- [Measured research results (21 Oct 2026)](docs/research-results-2026-10-21.md) — benchmark evidence, quantized-model accuracy, Haskell IPC overhead, scaling limitations and reproduction commands.
- [Repeated-seed research methodology](docs/research-replicates.md) — multiple independent data/training seeds, repeated verifier measurements and statistical limitations.
- [Public research reproducibility package](docs/research-reproducibility-package.md) — curated experimental evidence, SHA-256 integrity checks, exact CI revision and commands for independently inspecting the data.
- [Research prototype release](docs/releases/v0.1.0-research-prototype.md) — scope, scientific limitations, release evidence and verification steps.
- [Scientific research-paper manuscript (draft)](research/paper/aivoblock-manuscript.md) — experiment-grounded methods, architecture, tables, limitations and provisional citations; not yet peer reviewed.
- [Standalone AI versus blockchain verifier](docs/standalone-comparison.md) — controlled integer inference baseline, matched tasks, validation overhead and explicit comparison limits.
- [AI workloads](docs/ai-workloads.md) — which AI computations are consensus-verifiable (integer linear, QMLP-V1) and the off-chain PyTorch → QMLP-V1 → ONNX path.
- [Architecture](docs/architecture.md) — components, node responsibilities, and trust boundaries.
- [Mining](docs/mining.md) — intended collaborative AI work lifecycle.
- [Consensus draft](protocol/spec/consensus.md) — initial validator model and open protocol questions.
- [Puzzle rules](protocol/spec/puzzle-rules.md) — deterministic integer inference prototype and verification.
- [Canonical encoding draft](protocol/spec/canonical-encoding.md) — byte encodings and cross-language consistency requirements.
- [AI-work block V3](protocol/spec/ai-work-block-v3.md), [testnet consensus](protocol/spec/testnet-consensus-v1.md), [Sayed Token issuance](protocol/spec/sayed-token-issuance-v1.md).

## Roadmap

1. **Blockchain core:** deterministic blocks, transactions and ledger rules.
2. **AI puzzles:** useful, partitionable tasks and reproducible execution.
3. **Verification and consensus:** authenticated proofs, Byzantine fault tolerance and finality.
4. **P2P collaboration:** decentralized node communication and work coordination.
5. **Native rewards:** Sayed Token accounting for verified contributions.
6. **Private testnet:** multi-node testing, adversarial checks and performance measurement.

**First milestone (reached on the private testnet):** three independent nodes collaboratively compute a puzzle, independently verify contributions, agree on a valid block, and record rewards; a four-validator network tolerates one crashed validator. Open problems: succinct verification of real AI workloads, permissionless validator selection, peer identity rotation, a fee market.

## Security and project limitations

- AI result hashes are commitments; correctness is established only by full recomputation of the small integer workload.
- The private-testnet validator model is permissioned; permissionless Sybil resistance remains unresolved.
- Validator P2P links mutually authenticate and derive ephemeral X25519 session keys. Frames are encrypted with ChaCha20-Poly1305 and remain signed and replay-checked. Peer encryption does **not** secure the local operator control API, protect endpoints, or prevent network-level traffic analysis and denial of service.
- Consensus-critical encodings and signature rules are versioned research formats, not finalized standards.
- Do not use this prototype to protect real funds or deploy a public mainnet.
- Build, test and testnet results are produced by CI; see the testnet guide for the latest verified run.

## Licensing and use

**Source-available research prototype; all rights reserved at this time.**
The repository is public for inspection and research discussion, but the current
[LICENSE](LICENSE) does **not** grant an open-source license or permission to
redistribute/modify the code. An open-source license may be chosen separately
by the rights holders. Third-party dependencies retain their own licenses.
