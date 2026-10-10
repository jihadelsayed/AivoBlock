#!/usr/bin/env bash
# AivoBlock local three-container smoke test.
# Run from any directory: bash scripts/test-network.sh
# This checks Docker Compose configuration and diagnostic output only.
# P2P connections, consensus and AI mining are NOT implemented yet.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v docker >/dev/null 2>&1; then
  echo "ERROR: Docker is not installed." >&2
  exit 1
fi

if ! docker compose version >/dev/null 2>&1; then
  echo "ERROR: Docker Compose v2 is required." >&2
  exit 1
fi

echo "Validating Compose configuration..."
docker compose config --quiet

cleanup() {
  docker compose down --remove-orphans
}
trap cleanup EXIT

echo "Building three AivoBlock diagnostic containers..."
docker compose build

echo "Running node status checks..."
for node in node-1 node-2 node-3; do
  echo "Checking $node..."
  output="$(docker compose run --rm --no-deps "$node" status)"
  printf '%s\n' "$output"
  grep -q "Status: Initial development" <<<"$output" || {
    echo "ERROR: $node did not report expected status." >&2
    exit 1
  }
  grep -q "No node has been started." <<<"$output" || {
    echo "ERROR: $node unexpectedly reported a running node." >&2
    exit 1
  }
done

echo "Smoke test passed: all three node configurations can execute."
echo "NOTE: No P2P network or consensus was tested."
