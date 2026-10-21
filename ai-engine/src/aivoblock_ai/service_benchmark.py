"""Measure bounded non-blockchain QMLP service latency from committed fixtures.

This is local synchronous CPU service timing, not BFT finality or a GPU baseline.
"""
from __future__ import annotations

import argparse
import json
import statistics
import time
import uuid
from pathlib import Path

from .qmlp import QuantizedMLP
from .service_baseline import QmlpService


def measure(payload: dict, *, iterations: int = 20, batch: int = 64,
            verify: bool = False, warmups: int = 1,
            launch_id: str | None = None) -> dict:
    if type(iterations) is not int or iterations < 3:
        raise ValueError("iterations must be at least three")
    if type(verify) is not bool:
        raise ValueError("verify must be boolean")
    if type(warmups) is not int or warmups < 1:
        raise ValueError("warmups must be positive")
    if launch_id is not None and (not isinstance(launch_id, str) or not launch_id.strip()):
        raise ValueError("launch_id must be a nonempty string")
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
    for _ in range(warmups - 1):
        preflight = service.execute(rows, verify_independently=verify)
        if preflight.result_commitment != warm.result_commitment:
            raise ValueError("Nondeterministic warmup result")
    observations = []
    request_observations = []
    for index in range(iterations):
        started = time.perf_counter_ns()
        result = service.execute(rows, verify_independently=verify)
        ended = time.perf_counter_ns()
        if result.result_commitment != warm.result_commitment:
            raise ValueError("Nondeterministic service result")
        observations.append((ended - started) / 1e6)
        if launch_id is not None:
            request_observations.append({
                "launch_id": launch_id,
                "request_id": f"{launch_id}-{index}-{uuid.uuid4().hex}",
                "start_ns": started,
                "end_ns": ended,
                "status": "success",
                "model_commitment": result.model_commitment,
                "input_commitment": result.input_commitment,
                "result_commitment": result.result_commitment,
                "boundary": "synchronous_in_process_service_execute",
            })
    report = {
        "schema_version": 1,
        "scope": "local synchronous CPU service; excludes network, ledger and BFT finality",
        "mode": "same_implementation_recomputation" if verify else "trusted_result",
        "rows": len(rows),
        "batch_size": batch,
        "iterations": iterations,
        "warmups": warmups,
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
    if launch_id is not None:
        report["request_observations"] = request_observations
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--iterations", type=int, default=20)
    parser.add_argument("--batch", type=int, default=64)
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--warmups", type=int, default=1)
    parser.add_argument("--observations-out", type=Path,
                        help="Optional JSONL of measured, committed service requests")
    parser.add_argument("--launch-id", type=str,
                        help="Required with --observations-out; unique per process launch")
    args = parser.parse_args()
    if bool(args.observations_out) != bool(args.launch_id):
        parser.error("--observations-out and --launch-id must be provided together")
    report = measure(json.loads(args.fixture.read_text()),
                     iterations=args.iterations, batch=args.batch, verify=args.verify,
                     warmups=args.warmups, launch_id=args.launch_id)
    if args.observations_out:
        args.observations_out.parent.mkdir(parents=True, exist_ok=True)
        args.observations_out.write_text("".join(json.dumps(row, sort_keys=True) + "\n"
                                            for row in report["request_observations"]))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    main()
