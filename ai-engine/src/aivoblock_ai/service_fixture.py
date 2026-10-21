"""Turn a trained QMLP export into a commitment-checked service fixture.

This uses the real off-chain trained model; it is NOT the seeded synthetic
Rust verifier workload unless Rust explicitly consumes this same export.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from .qmlp import QuantizedMLP, qmlp_input_commitment, execute_partition


def build_fixture(export: dict) -> dict:
    if export.get("workload") != "qmlp-v1":
        raise ValueError("Expected qmlp-v1 export")
    model = QuantizedMLP.from_json(export["model"])
    rows = export["rows"]
    expected_model = model.commitment().hex()
    if export.get("model_commitment") != expected_model:
        raise ValueError("Export model commitment mismatch")
    inputs = qmlp_input_commitment(model, rows).hex()
    logits, result = execute_partition(model, rows, 0, len(rows))
    if export.get("result_commitment") != result.hex():
        raise ValueError("Export result commitment mismatch")
    if export.get("logits") != [list(row) for row in logits]:
        raise ValueError("Export logits mismatch")
    return {
        "schema_version": 1,
        "model": model.to_json(),
        "rows": [list(row) for row in rows],
        "model_commitment": expected_model,
        "input_commitment": inputs,
        "result_commitment": result.hex(),
        "source": "trained_qmlp_export",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qmlp-file", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    fixture = build_fixture(json.loads(args.qmlp_file.read_text()))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(fixture, indent=2, sort_keys=True) + "\n")
    print(json.dumps({
        "rows": len(fixture["rows"]),
        "model_commitment": fixture["model_commitment"],
        "input_commitment": fixture["input_commitment"],
    }, sort_keys=True))


if __name__ == "__main__":
    main()
