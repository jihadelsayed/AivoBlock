# AivoBlock

**A custom blockchain for collaborative, verifiable AI computation.**

AivoBlock is an experimental decentralized blockchain backend. Its goal is to divide useful AI workloads among participating workers, verify submitted results, and record accepted work through a custom ledger and consensus protocol.

> **Development status:** Early research prototype. The repository contains initial types, local scheduling, validation helpers, and tests. It does **not** yet run a functioning P2P blockchain, mine blocks, establish consensus, or provide secure proof of useful AI work.

## What makes AivoBlock different?

AivoBlock investigates whether blockchain participants can contribute **useful AI computation** rather than relying exclusively on arbitrary computational puzzles. The design separates three problems:

1. **Computation:** distribute AI puzzle units and execute them.
2. **Verification:** independently check outputs and bind them to immutable task definitions.
3. **Consensus:** agree on finalized blocks despite faulty or malicious participants.

A correct AI result does **not**, by itself, provide Byzantine fault tolerance, finality, or Sybil resistance. These are active protocol design challenges.

AivoBlock is a **new blockchain**, not an integration with Ethereum or another chain. **Sayed Token** is the intended native ledger asset; the final supply, issuance, and compatibility rules are not yet specified. This project does not propose an additional token.

## Technology stack

| Component | Technology | Responsibility |
| --- | --- | --- |
| Blockchain core | Haskell, Cabal | Deterministic ledger transitions, block validation, voting primitives, rewards |
| Node runtime | Rust, Cargo | Local node operation, planned P2P, RocksDB persistence, IPC |
| AI engine | Python | Puzzle workloads, deterministic inference, proof commitments |
| Transport contracts | Protocol Buffers | Versioned messages between nodes and components |
| Local test environment | Docker Compose | Three isolated node configurations |
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

**Docker diagnostic containers** (from the repository root):

```bash
docker compose up --build
```

The Compose services currently print diagnostic status and exit. They do **not** establish a network. See [development instructions](docs/development.md) for installation details and caveats.

## Design documents

- [Architecture](docs/architecture.md) — components, node responsibilities, and trust boundaries.
- [Mining](docs/mining.md) — intended collaborative AI work lifecycle.
- [Consensus draft](protocol/spec/consensus.md) — initial validator model and open protocol questions.
- [Puzzle rules](protocol/spec/puzzle-rules.md) — deterministic integer inference prototype and verification.
- [Canonical encoding draft](protocol/spec/canonical-encoding.md) — byte encodings and cross-language consistency requirements.

## Roadmap

1. **Blockchain core:** deterministic blocks, transactions and ledger rules.
2. **AI puzzles:** useful, partitionable tasks and reproducible execution.
3. **Verification and consensus:** authenticated proofs, Byzantine fault tolerance and finality.
4. **P2P collaboration:** decentralized node communication and work coordination.
5. **Native rewards:** Sayed Token accounting for verified contributions.
6. **Private testnet:** multi-node testing, adversarial checks and performance measurement.

**First milestone:** Three independent nodes collaboratively compute a puzzle, independently verify contributions, agree on a valid block, and record rewards. This is a research goal, **not functionality that currently works**.

## Security and project limitations

- AI result hashes are commitments, not proofs of correctness.
- Prototype proof verification may require full recomputation.
- The private-testnet validator model is permissioned; permissionless Sybil resistance remains unresolved.
- Consensus-critical encodings and signature rules are not finalized.
- Do not use this prototype to protect real funds or deploy a public mainnet.
- Build and test results have not yet been independently verified across all components.

## License

See [LICENSE](LICENSE). The repository currently carries an all-rights-reserved notice; no open-source license has been selected.
