#!/usr/bin/env python3
"""Validate and describe real repeated QMLP finality timing.

Strictly descriptive: no isolated AI/consensus latency, speedup, or causal
polling-cadence claim is supported by these client-observed intervals.
Runs on saved raw JSON observations, without Docker or synthetic measurements.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import statistics
from pathlib import Path

PHASES = ("cli_submission_ms", "post_submission_to_observed_finality_ms", "total_ms")
EXPECTED_INCLUDES = ["worker_execution", "network_and_consensus", "status_polling"]
CLOCK = "time.perf_counter_ns"
BOUNDARY = "before_cli_submission_to_all_validator_height_poll"


def number(value: object, field: str, *, positive: bool = True) -> float:
    if type(value) not in (int, float) or not math.isfinite(value) or (positive and value <= 0):
        raise ValueError(f"invalid {field}")
    return float(value)


def close(observed: float, expected: float) -> bool:
    # Serialization uses unrounded nanoseconds / 1_000_000.
    return math.isclose(observed, expected, rel_tol=1e-12, abs_tol=1e-7)


def stats(samples: list[float]) -> dict:
    if not samples or any(not math.isfinite(x) or x <= 0 for x in samples):
        raise ValueError("invalid timing samples")
    ordered = sorted(samples)
    return {
        "n": len(ordered),
        "min_ms": ordered[0],
        "median_ms": statistics.median(ordered),
        "p95_nearest_rank_ms": ordered[math.ceil(len(ordered) * 0.95) - 1],
        "mean_ms": statistics.mean(ordered),
        "max_ms": ordered[-1],
        "sample_stddev_ms": statistics.stdev(ordered) if len(ordered) > 1 else None,
    }


def summarize(directory: Path) -> dict:
    files = sorted(directory.glob("launch-???.json"))
    if len(files) != 3:
        raise ValueError("expected exactly three independent launch files")
    identifiers: set[str] = set()
    launches: set[str] = set()
    configuration = None
    canonical = None
    pooled = {name: [] for name in PHASES}
    details = []

    for path in files:
        raw = path.read_bytes()
        samples = json.loads(raw)
        if not isinstance(samples, list) or len(samples) != 30:
            raise ValueError(f"{path.name}: expected 30 actual observations")
        launch = path.stem
        if launch in launches:
            raise ValueError("duplicate launch")
        launches.add(launch)
        heights: set[int] = set()
        block_ids: set[str] = set()
        local = {name: [] for name in PHASES}
        for index, row in enumerate(samples):
            if not isinstance(row, dict) or row.get("launch_id") != launch:
                raise ValueError(f"{path.name}: inconsistent launch ID")
            rid = row.get("request_id")
            if not isinstance(rid, str) or not rid or rid in identifiers:
                raise ValueError(f"{path.name}: missing or duplicate request ID")
            identifiers.add(rid)
            height = row.get("finalized_height")
            if type(height) is not int or height < 1 or height in heights:
                raise ValueError(f"{path.name}: missing or duplicate finality height")
            heights.add(height)
            block_id = row.get("block_id")
            if not isinstance(block_id, str) or not block_id or block_id in block_ids:
                raise ValueError(f"{path.name}: missing or duplicate block ID")
            block_ids.add(block_id)
            if not isinstance(row.get("task_id"), str) or not row["task_id"]:
                raise ValueError(f"{path.name}: missing task ID")
            polling = number(row.get("polling_interval_s"), "polling cadence")
            if polling > 10:
                raise ValueError("invalid polling cadence")
            rows = row.get("rows")
            if type(rows) is not int or rows <= 0:
                raise ValueError("invalid row count")
            candidate = (row.get("workload"), row.get("fixture_source"), rows,
                         row.get("validators"), polling, row.get("observation_boundary"))
            if (candidate[0] != "qmlp-v1" or candidate[1] not in
                    ("trained_qmlp_export", "seeded_node_fixture")
                    or candidate[3] != 3 or candidate[5] != BOUNDARY):
                raise ValueError(f"{path.name}: invalid testnet configuration")
            if configuration is None:
                configuration = candidate
            if candidate != configuration:
                raise ValueError("mixed workload, fixture, validator count or polling cadence")
            if candidate[1] == "trained_qmlp_export":
                ids = (row.get("model_commitment"), row.get("result_commitment"))
                if any(not isinstance(v, str) or len(v) != 64
                       or any(ch not in "0123456789abcdef" for ch in v) for v in ids):
                    raise ValueError("missing or invalid trained QMLP commitments")
            else:
                ids = None
                if "model_commitment" in row or "result_commitment" in row:
                    raise ValueError("seeded fixture carries trained commitments")
            if index == 0 and not details and canonical is None:
                canonical = ids
            if ids != canonical:
                raise ValueError("mixed trained-model commitments")
            start, end = row.get("start_ns"), row.get("end_ns")
            if (type(start) is not int or type(end) is not int or start < 0 or end <= start
                    or row.get("clock") != CLOCK):
                raise ValueError("invalid monotonic timing boundary")
            total = number(row.get("elapsed_ms"), "elapsed_ms")
            if not close(total, (end - start) / 1_000_000):
                raise ValueError("elapsed duration disagrees with raw timestamps")
            phases = row.get("timing_phases")
            if not isinstance(phases, dict) or phases.get("clock") != CLOCK:
                raise ValueError("missing timing phases or wrong clock")
            values = {name: number(phases.get(name), name) for name in PHASES}
            if not close(values["total_ms"], total) or not close(
                    values["cli_submission_ms"] + values["post_submission_to_observed_finality_ms"], total):
                raise ValueError("phase duration does not equal total duration")
            if (phases.get("ai_execution_only_ms", "missing") is not None or
                    phases.get("consensus_only_ms", "missing") is not None or
                    phases.get("post_submission_includes") != EXPECTED_INCLUDES):
                raise ValueError("unsupported AI-only/consensus-only timing attribution")
            for key in PHASES:
                pooled[key].append(values[key])
                local[key].append(values[key])
        details.append({
            "launch_id": launch,
            "source_file": path.name,
            "sha256": hashlib.sha256(raw).hexdigest(),
            "samples": len(samples),
            "timing": {key: stats(local[key]) for key in PHASES},
        })
    if len(identifiers) != 90:
        raise ValueError("expected exactly 90 distinct finalized requests")

    return {
        "method": {
            "workload": configuration[0], "fixture_source": configuration[1],
            "rows_per_task": configuration[2], "validators": configuration[3],
            "polling_interval_s": configuration[4],
            "observation_boundary": configuration[5], "clock": CLOCK,
            "launches": 3, "observations_per_launch": 30,
            "total_observations": len(identifiers),
            "trained_qmlp_commitments": (
                {"model": canonical[0], "result": canonical[1]} if canonical else None),
        },
        "pooled": {key: stats(pooled[key]) for key in PHASES},
        "per_launch": details,
        "between_launch": {
            "metric": "median duration in each fresh network launch",
            "timing": {key: stats([row["timing"][key]["median_ms"] for row in details])
                       for key in PHASES},
        },
        "interpretation": {
            "scope": "descriptive client-observed durations for this CI runner and three testnet launches",
            "post_submission_includes": EXPECTED_INCLUDES,
            "not_measured": ["AI-only time", "network-only time", "consensus-only time",
                             "true internal finalization instant"],
            "not_supported": ["service-vs-chain speedup or overhead",
                              "causal polling-cadence effect", "cross-machine generalization",
                              "confidence interval based on 90 independent network trials"],
            "note": "Tasks within each launch share nodes and runner resources; polling adds measurement delay.",
        },
    }


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--directory", required=True, type=Path)
    p.add_argument("--out", required=True, type=Path)
    args = p.parse_args()
    report = summarize(args.directory)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(f"PASS: described {report['method']['total_observations']} real client-observed QMLP timings; no isolated consensus or speedup claims")


if __name__ == "__main__":
    main()
