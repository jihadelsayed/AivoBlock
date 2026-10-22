#!/usr/bin/env python3
"""Validate observed, separate-process QMLP CPU baseline without claiming chain parity."""
import argparse
import hashlib
import json
import math
import statistics
from pathlib import Path


FIELDS = ("model_commitment", "input_commitment", "result_commitment")
ARTIFACTS = ("model", "input", "result")


def validate(directory):
    files = sorted(directory.glob("launch-*.jsonl"))
    if len(files) != 3:
        raise ValueError("expected exactly three CPU service launches")
    all_requests = set()
    launches = set()
    reference = None
    artifact_reference = None
    durations = []
    for path in files:
        rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
        if len(rows) != 30:
            raise ValueError(f"{path.name}: expected 30 real observations")
        launch_ids = {row["launch_id"] for row in rows}
        if len(launch_ids) != 1 or next(iter(launch_ids)) in launches:
            raise ValueError("missing or duplicate launch identifier")
        launches.update(launch_ids)
        for row in rows:
            if row.get("status") != "success" or row.get("boundary") != "synchronous_in_process_service_execute":
                raise ValueError("invalid service execution boundary")
            rid = row["request_id"]
            if rid in all_requests:
                raise ValueError("duplicate service request identifier")
            all_requests.add(rid)
            start, end = row["start_ns"], row["end_ns"]
            if type(start) is not int or type(end) is not int or start < 0 or end <= start:
                raise ValueError("invalid service monotonic timestamps")
            identity = tuple(row[field] for field in FIELDS)
            if any(not isinstance(x, str) or len(x) != 64 or any(c not in "0123456789abcdef" for c in x) for x in identity):
                raise ValueError("invalid QMLP commitments")
            if reference is None:
                reference = identity
            elif reference != identity:
                raise ValueError("QMLP model/input/result mismatch across launches")
            digests = []
            for name in ARTIFACTS:
                stored = row[f"artifact_{name}_sha256"]
                actual = hashlib.sha256(Path(row[f"{name}_path"]).read_bytes()).hexdigest()
                if stored != actual:
                    raise ValueError("service artifact digest mismatch")
                digests.append(actual)
            if artifact_reference is None:
                artifact_reference = tuple(digests)
            elif artifact_reference != tuple(digests):
                raise ValueError("service artifact mismatch across launches")
            durations.append((end-start)/1_000_000)
    if len(durations) != 90 or any(not math.isfinite(ms) or ms <= 0 for ms in durations):
        raise ValueError("invalid observation set")
    return {"scope": "local in-process CPU execution only, no network/consensus",
            "launches": len(launches), "samples": len(durations),
            "canonical_commitments": dict(zip(FIELDS, reference)),
            "artifact_sha256": dict(zip(ARTIFACTS, artifact_reference)),
            "mean_ms": statistics.mean(durations), "median_ms": statistics.median(durations),
            "p95_ms": sorted(durations)[math.ceil(len(durations)*0.95)-1],
            "warning": "Not a matched chain comparison: chain observations currently lack verified matching QMLP commitments and the timing boundaries differ."}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--directory", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    args = p.parse_args()
    report = validate(args.directory)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True)+"\n")
    print(f"PASS: {report['samples']} real CPU observations; matched chain parity NOT established")


if __name__ == "__main__":
    main()
