#!/usr/bin/env python3
"""Compare real polling-cadence observations; descriptive, not causal inference."""
import argparse
import json
import math
import statistics
from pathlib import Path


def load(directory, cadence):
    files = sorted(directory.glob("launch-???.json"))
    if len(files) != 3:
        raise ValueError(f"{directory}: expected 3 independent launch files")
    all_ids = set()
    launch_ids = set()
    durations = []
    for file in files:
        rows = json.loads(file.read_text())
        if len(rows) != 30:
            raise ValueError(f"{file}: expected 30 observations")
        ids = {r["launch_id"] for r in rows}
        if len(ids) != 1 or next(iter(ids)) in launch_ids:
            raise ValueError("invalid or repeated launch ID")
        launch_ids |= ids
        heights = set()
        for row in rows:
            rid = row["request_id"]
            if rid in all_ids or row["finalized_height"] in heights:
                raise ValueError("duplicate request or finalized height")
            all_ids.add(rid)
            heights.add(row["finalized_height"])
            if row.get("polling_interval_s") != cadence or row.get("workload") != "qmlp-v1" or row.get("rows") != 12 or row.get("validators") != 3:
                raise ValueError("inconsistent workload or observation cadence")
            duration = row.get("elapsed_ms")
            if isinstance(duration, bool) or not isinstance(duration, (int, float)) or not math.isfinite(duration) or duration <= 0:
                raise ValueError("invalid duration")
            durations.append(duration)
    return {"samples": len(durations), "launches": len(launch_ids),
            "mean_ms": statistics.mean(durations), "median_ms": statistics.median(durations),
            "min_ms": min(durations), "max_ms": max(durations),
            "p95_ms": sorted(durations)[math.ceil(len(durations) * .95)-1]}


def compare(slow, fast):
    a, b = load(slow, 1.0), load(fast, 0.1)
    return {"design": "sequential separate-testnet cadence comparison",
            "slow_1s": a, "fast_100ms": b,
            "mean_observed_difference_ms": a["mean_ms"] - b["mean_ms"],
            "warning": "Descriptive only: separate network launches and runner contention; observations include CLI, consensus and status polling. No isolated consensus latency or causal polling effect established."}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--slow", required=True, type=Path)
    p.add_argument("--fast", required=True, type=Path)
    p.add_argument("--out", required=True, type=Path)
    args = p.parse_args()
    args.out.write_text(json.dumps(compare(args.slow, args.fast), indent=2) + "\n")


if __name__ == "__main__":
    main()
