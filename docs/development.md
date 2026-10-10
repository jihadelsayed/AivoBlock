# AivoBlock Development Guide

**Status:** Experimental custom blockchain backend. Components are under development and do not yet form a running decentralized network.

## Repository layout

| Directory | Technology | Purpose |
| --- | --- | --- |
| `core/` | Haskell / Cabal | Ledger, validation, consensus primitives, reward rules |
| `node/` | Rust / Cargo | Node runtime, P2P, RocksDB, local IPC |
| `ai-engine/` | Python | Deterministic AI puzzle experiments and worker |
| `protocol/` | Protobuf / Markdown | Wire schemas and consensus specifications |
| `docker/` | Docker | Node image |
| `tests/` | Cross-component | Planned integration and end-to-end tests |

## Prerequisites

- Git and an appropriate Rust toolchain with Cargo
- GHC and Cabal for Haskell
- Python 3.11 or newer and pip
- Docker Desktop with Compose v2 for the local container smoke test
- A C/C++ build environment, Clang and libclang for RocksDB builds

On Windows, WSL2 is recommended for native Rust/RocksDB builds. Use a development environment with matching compilers and native dependencies. Do not commit machine-specific toolchain paths.

## Clone and set up

```bash
git clone https://github.com/jihadelsayed/AivoBlock.git
cd AivoBlock
```

On Windows PowerShell, run the prerequisite checker and Python setup:

```powershell
.\scripts\setup.ps1
```

This script checks for Python, Cargo, Cabal, GHC and Docker. It creates `ai-engine/.venv`, installs the Python package in editable mode with pytest, and runs Python tests. It does **not** install all system dependencies or launch nodes.

## Run component tests

### Python AI engine

```bash
cd ai-engine
python -m venv .venv
# Activate the virtual environment for your shell, then:
python -m pip install -e ".[dev]"
python -m pytest
```

### Haskell core

```bash
cd core
cabal update
cabal build all
cabal test all
cabal run aivoblock-core -- --help
```

### Rust node

```bash
cd node
cargo fmt --check
cargo test
cargo run -- status
```

The Haskell and Rust test commands are development targets; successful execution across supported environments has not yet been established.

## Local Docker diagnostic test

From the repository root:

```bash
docker compose config
docker compose up --build
```

The three services currently run the node's `status` command and exit. **They do not exchange blocks, perform mining, or establish P2P communication.**

Run the automated container smoke test in Bash:

```bash
bash scripts/test-network.sh
```

## IPC development contract

Rust-to-Haskell and Rust-to-Python adapters use one binary message per child process:

```text
uint32_be(payload_length) || payload
```

- Payload length must be from 1 to 4,194,304 bytes.
- Worker output is untrusted until independently verified.
- The Python worker accepts a framed JSON diagnostic request, such as `{"protocol_version":1,"type":"health"}`.
- The current Haskell CLI does **not** yet implement the binary validator IPC endpoint.
- Typed request/response schemas, supervision, and execution timeouts remain future work.

IPC framing is not blockchain canonical encoding.

## Development rules

1. Keep consensus-critical ledger state transitions deterministic and independently validated.
2. Never accept a submitted AI result commitment as proof of correctness by itself.
3. Distinguish local scheduling from decentralized network-wide work assignment.
4. Never persist a block as finalized until authenticated consensus checks succeed.
5. Add unit tests and shared cross-language vectors with protocol changes.
6. Never commit secrets, wallet keys, node identity private keys, virtual environments or node databases.

## Recommended next milestones

1. Ensure each language package compiles and its existing tests pass.
2. Finalize canonical block, transaction, puzzle and signature encodings.
3. Build versioned, typed Rust–Haskell and Rust–Python worker protocols.
4. Add authenticated P2P messaging, independent verification and atomic persistence.
5. Implement and adversarially test a complete consensus state machine.
6. Demonstrate three nodes accepting the same verified AI puzzle block.

See [architecture](architecture.md), [mining](mining.md), and the [consensus draft](../protocol/spec/consensus.md).
