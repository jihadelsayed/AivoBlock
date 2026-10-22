# AivoBlock Development Guide

**Status:** Working permissioned private Docker testnet and cross-language research prototype; **no public permissionless blockchain or production network**. The original root-level `compose.yaml` remains a diagnostic and is not the complete testnet.

## Repository layout

| Directory | Technology | Purpose |
| --- | --- | --- |
| `core/` | Haskell / Cabal | Ledger, validation, consensus primitives, reward rules |
| `node/` | Rust / Cargo | Node runtime, P2P, RocksDB, local IPC |
| `ai-engine/` | Python | Deterministic AI puzzle experiments and worker |
| `protocol/` | Protobuf / Markdown | Wire schemas and consensus specifications |
| `docker/` | Docker | Node image |
| `scripts/` and per-language test suites | Python/Rust/Haskell | Unit, cross-language and Docker end-to-end verification |

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

These component commands are exercised in GitHub Actions on Ubuntu 24.04. Other hosts may require toolchain and dependency changes; passing CI does not establish portability to all operating systems.

## Rust-to-Python partitioned AI diagnostic

After installing the editable Python package (see above), run from `node/`:

```bash
cargo run -- ai-partition-demo
```

This CLI diagnostic has Rust locally assign two non-overlapping work ranges
to two worker identities (three input rows total). Each range is executed by
a separate local Python worker process. Rust independently calculates the
expected outputs and SHA-256 commitments for a fixed fixture, compares the
full worker response, requests a second recomputation, and confirms that a
tampered commitment is rejected. The `rust` CI job runs this command with
Python installed.

**Limitations:** The model and input hashes are placeholder fixture values,
worker identities are not authenticated, the recomputation uses caller-supplied
local inputs, and this test does not contact independent peers. This is
not decentralized mining, consensus, proof-of-useful-work security, or token
accounting. A live node must use authenticated, immutable puzzle inputs and a
versioned typed response protocol before accepting proofs.

## Local Docker diagnostic test

From the repository root:

```bash
docker compose config
docker compose up --build
```

The root-level `compose.yaml` is a **diagnostic only**: its three services run a status command and exit. It is distinct from `testnet/compose.three-node.yaml` and `testnet/compose.four-validator.yaml`, which start the working, mutually authenticated private validator network. See [the private-testnet guide](testnet.md).

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
- Haskell has an implemented, framed JSON IPC validator endpoint (including `ipc-serve`) used by the testnet and covered by Rust–Haskell CI integration tests. This does not provide generic production RPC interoperability.
- Every child request has a wall-clock deadline from spawn to the final byte
  (Haskell core 10 s, Python worker 30 s). A child that does not answer in
  time is killed and the request fails with `TimedOut`; a hung executable
  cannot block the consensus loop or hold a worker slot forever.
- The Haskell core also offers `aivoblock-core ipc-serve`, which answers
  framed requests until stdin closes. The testnet runtime uses it through
  `HaskellValidatorClient::persistent()`: requests are serialized, each has
  the same 10 s deadline, and any error, timeout or malformed reply kills the
  process so the next request starts a fresh one. Diagnostics keep one
  process per request.
- Typed request/response schemas remain future work.

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
3. Harden the implemented framed Rust–Haskell and Rust–Python worker protocols for wider deployments.
4. Extend the existing authenticated P2P, independent recomputation and atomic persistence with greater fault coverage.
5. Extend permissioned consensus adversarial testing to larger and more diverse networks.
6. Replicate the successful three-node testnet experiment independently on additional hardware.

See [architecture](architecture.md), [mining](mining.md), and the [consensus draft](../protocol/spec/consensus.md).

## Private testnet

With Docker (Compose v2) and Python 3.11+ only:

```bash
python3 scripts/testnet.py all
```

Native iteration without Docker: `cargo test testnet::` runs the in-process four-validator crash/restart and three-node adversarial tests with the Rust settlement mirror; export `AIVOBLOCK_HASKELL_EXECUTABLE=$(cd core && cabal list-bin exe:aivoblock-core)` to use the Haskell core. `cargo run -- testnet-init --out DIR` generates keys and a signed `network.json`; `cargo run -- run` starts a node from `AIVOBLOCK_*` variables (see `docker/Dockerfile.node` for defaults). See [testnet guide](testnet.md).
