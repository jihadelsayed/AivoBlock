"""Measure bounded non-blockchain QMLP service latency from committed fixtures.

This is local synchronous CPU service timing, not BFT finality or a GPU baseline.
"""
from __future__ import annotations

import argparse
import json
import statistics
import time
from pathlib import Path

from .qmlp import QuantizedMLP
from .service_baseline import QmlpService


def measure(payload: dict, *, iterations: int = 20, batch: int = 64,
            verify: bool = False) -> dict:
    if type(iterations) is not int or iterations < 3:
        raise ValueError("iterations must be at least three")
    if type(verify) is not bool:
        raise ValueError("verify must be boolean")
    model = QuantizedMLP.from_json(payload["model"])
    rows = payload["rows"]
    service = QmlpService(model, max_batch=batch)
    # Verify the fixture before timing: no comparison is valid for a
    # mismatched input/model even if dimensions happen to agree.
    warm = service.execute(rows, verify_independently=verify)
    for key in ("model_commitment", "input_commitment"):
        if payload.get(key) != getattr(warm, key):
            raise ValueError(f"Fixture {key} does not match computed commitment")
    if payload.get("result_commitment") != warm.result_commitment:
        raise ValueError("Fixture result_commitment does not match computed commitment")
    observations = []
    for _ in range(iterations):
        started = time.perf_counter_ns()
        result = service.execute(rows, verify_independently=verify)
        observations.append((time.perf_counter_ns() - started) / 1e6)
        if result.result_commitment != warm.result_commitment:
            raise ValueError("Nondeterministic service result")
    return {
        "schema_version": 1,
        "scope": "local synchronous CPU service; excludes network, ledger and BFT finality",
        "mode": "same_implementation_recomputation" if verify else "trusted_result",
        "rows": len(rows),
        "batch_size": batch,
        "iterations": iterations,
        "model_commitment": warm.model_commitment,
        "input_commitment": warm.input_commitment,
        "result_commitment": warm.result_commitment,
        "latency_ms": {
            "observations": observations,
            "median": statistics.median(observations),
            "mean": statistics.mean(observations),
            "sample_stddev": statistics.stdev(observations),
        },
        "rows_per_second_mean_latency": len(rows) * 1000 / statistics.mean(observations),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--iterations", type=int, default=20)
    parser.add_argument("--batch", type=int, default=64)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    report = measure(json.loads(args.fixture.read_text()),
                     iterations=args.iterations, batch=args.batch, verify=args.verify)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    main()
