#!/usr/bin/env python3
"""Inspect real finalized-block records for the QMLP identity fields."""
import argparse
import json
from pathlib import Path

FIELDS = ("model_commitment", "input_commitment", "result_commitment")


def inspect(root):
    files = sorted(root.glob("launch-???/three-node/finalized-block-evidence/*.json"))
    if len(files) != 90:
        raise ValueError("expected 90 finalized blocks")
    missing = {field: 0 for field in FIELDS}
    for path in files:
        block = json.loads(path.read_text())
        if block.get("workload") != "qmlp-v1" or not block.get("block_id"):
            raise ValueError("invalid finalized QMLP block")
        for field in FIELDS:
            if field not in block:
                missing[field] += 1
    return {"blocks_audited": len(files),
            "missing_commitment_fields": missing,
            "independently_verified_triple": all(n == 0 for n in missing.values())}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    result = inspect(args.root)
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    print("Finalized block audit:", result)


if __name__ == "__main__":
    main()
