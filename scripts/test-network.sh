#!/usr/bin/env bash
# AivoBlock private testnet end-to-end check (Docker + Python 3.11+).
# Builds the node image, runs the three-node demonstration and the
# four-validator fault-tolerance scenario, and verifies every balance.
# Usage: bash scripts/test-network.sh [demo|fault|all] [extra testnet.py flags]

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v docker >/dev/null 2>&1 || ! docker compose version >/dev/null 2>&1; then
  echo "ERROR: Docker with Compose v2 is required." >&2
  exit 1
fi

scenario="${1:-all}"
shift || true
exec python3 scripts/testnet.py "$scenario" "$@"
