#!/usr/bin/env bash
# Retry only transient Hackage tarball HTTP failures. Compiler and test errors
# must fail immediately; do not hide application regressions with generic retries.
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "Usage: ci-cabal-build.sh CABAL_BUILD_TARGET..." >&2
  exit 2
fi

log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT

for attempt in 1 2 3; do
  : > "$log_file"
  if cabal build "$@" 2>&1 | tee "$log_file"; then
    exit 0
  fi

  # Hackage's HTTP dream.io package mirror intermittently returns 403/404.
  # Cabal authenticates package hashes as usual; this only retries downloads.
  if ! grep -Eq 'Unexpected response (403|404|429|5[0-9][0-9]) for https?://[^ ]*(hackage|dream\.io)' "$log_file"; then
    echo "Build failure is not a transient Hackage HTTP download error; not retrying." >&2
    exit 1
  fi

  if [ "$attempt" -eq 3 ]; then
    echo "Hackage package download failed after $attempt attempts." >&2
    exit 1
  fi
  echo "Transient Hackage HTTP failure; retrying Cabal build after a short delay ($attempt/3)." >&2
  sleep "$((attempt * 5))"
done
