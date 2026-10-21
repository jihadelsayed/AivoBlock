"""Repeat committed QMLP service-core benchmarks in separate Python processes.

The subprocess boundary is outside measurement. No network, BFT or blockchain
finality is included; results must not be compared as equivalent boundaries.
"""
from __future__ import annotations

import argparse
import json
import math
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path


def aggregate(reports: list[dict], *, launches: int, iterations: int) -> dict:
    if len(reports) != 2 * launches:
        raise ValueError("missing per-launch reports")
    identities = {(r.get("model_commitment"), r.get("input_commitment"),
                   r.get("result_commitment"), r.get("rows")) for r in reports}
    if len(identities) != 1 or any(not all(identity[:3]) for identity in identities):
        raise ValueError("mismatched committed workloads")
    batches = {r.get("batch_size") for r in reports}
    if len(batches) != 1 or any(type(b) is not int or b < 1 for b in batches):
        raise ValueError("mismatched benchmark batch sizes")
    output = {}
    for mode in ("trusted_result", "same_implementation_recomputation"):
        subset = [r for r in reports if r.get("mode") == mode]
        if len(subset) != launches:
            raise ValueError(f"missing launches for {mode}")
        samples = []
        run_medians = []
        for r in subset:
            values = r.get("latency_ms", {}).get("observations")
            if r.get("warmups") != 5:
                raise ValueError("expected five warmups")
            if r.get("iterations") != iterations or not isinstance(values, list) or len(values) != iterations:
                raise ValueError("missing or mismatched observations")
            if any(isinstance(v, bool) or not isinstance(v, (int, float))
                   or not math.isfinite(v) or v <= 0 for v in values):
                raise ValueError("invalid latency observation")
            samples.extend(values)
            run_medians.append(statistics.median(values))
        ordered = sorted(samples)
        output[mode] = {
            "samples": samples,
            "per_launch_medians_ms": run_medians,
            "count": len(samples),
            "median_ms": statistics.median(samples),
            "mean_ms": statistics.mean(samples),
            "p95_ms": ordered[math.ceil(0.95 * len(ordered)) - 1],
            "sample_stddev_ms": statistics.stdev(samples),
            "min_ms": ordered[0],
            "max_ms": ordered[-1],
        }
    return {
        "schema_version": 1,
        "scope": "local synchronous in-process QMLP service-core latency only",
        "separate_process_launches": launches,
        "iterations_per_launch": iterations,
        "batch_size": next(iter(batches)),
        "warmups_per_launch": 5,
        "workload": dict(zip(("model_commitment", "input_commitment",
                             "result_commitment", "rows"), next(iter(identities)))),
        "modes": output,
        "limitations": "Same-implementation recomputation is not independent verification; no network, BFT, blockchain or throughput measurements.",
    }


def run(fixture: Path, *, launches: int = 3, iterations: int = 30,
        batch: int = 64) -> dict:
    if type(launches) is not int or launches < 3:
        raise ValueError("launches must be at least 3")
    if type(iterations) is not int or iterations < 30:
        raise ValueError("iterations must be at least 30")
    if type(batch) is not int or batch < 1:
        raise ValueError("batch must be positive")
    reports = []
    with tempfile.TemporaryDirectory() as tempdir:
        for mode in (False, True):
            for launch in range(launches):
                out = Path(tempdir) / f"{int(mode)}-{launch}.json"
                cmd = [sys.executable, "-m", "aivoblock_ai.service_benchmark",
                       "--fixture", str(fixture.resolve()), "--out", str(out),
                       "--iterations", str(iterations), "--batch", str(batch),
                       "--warmups", "5"]
                if mode:
                    cmd.append("--verify")
                subprocess.run(cmd, check=True, capture_output=True, text=True)
                reports.append(json.loads(out.read_text()))
    return aggregate(reports, launches=launches, iterations=iterations)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--launches", type=int, default=3)
    parser.add_argument("--iterations", type=int, default=30)
    parser.add_argument("--batch", type=int, default=64)
    args = parser.parse_args()
    report = run(args.fixture, launches=args.launches,
                 iterations=args.iterations, batch=args.batch)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"launches": args.launches, "modes": list(report["modes"])}, sort_keys=True))


if __name__ == "__main__":
    main()
