#!/usr/bin/env python3
"""Normalize real request observations using SHA-256 of artifact bytes.

This does not measure requests: start_ns/end_ns must come from the caller's
same monotonic clock, around the declared request boundary. Emits a single
scenario fragment, never an automatically accepted matched experiment.
"""
import argparse
import hashlib
import json
from pathlib import Path

MODES = ("service_direct", "service_recomputed", "chain_observed_finality")
FIELDS = ("model", "input", "result")


def digest_file(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def record(observations, mode):
    if mode not in MODES:
        raise ValueError("unknown scenario")
    if not isinstance(observations, list) or not observations:
        raise ValueError("at least one observation required")
    samples, reference, ids, heights = [], None, set(), set()
    for index, row in enumerate(observations):
        if not isinstance(row, dict):
            raise ValueError(f"observation {index}: object required")
        rid, launch = row.get("request_id"), row.get("launch_id")
        if not isinstance(rid, str) or not rid or not isinstance(launch, str) or not launch:
            raise ValueError(f"observation {index}: valid request_id and launch_id required")
        if rid in ids:
            raise ValueError(f"duplicate request_id {rid}")
        ids.add(rid)
        start, end = row.get("start_ns"), row.get("end_ns")
        if any(isinstance(x, bool) or not isinstance(x, int) or x < 0 for x in (start, end)) or end <= start:
            raise ValueError(f"observation {index}: invalid monotonic timestamps")
        if row.get("status") != "success":
            raise ValueError(f"observation {index}: non-success request")
        identity = {f"{field}_commitment": digest_file(row[f"{field}_path"]) for field in FIELDS}
        if reference is None:
            reference = identity
        elif identity != reference:
            raise ValueError(f"observation {index}: inconsistent workload or result bytes")
        sample = {"request_id": rid, "launch_id": launch,
                  "elapsed_ms": (end - start) / 1_000_000,
                  "status": "success", **identity}
        if mode == "chain_observed_finality":
            height = row.get("finalized_height")
            if isinstance(height, bool) or not isinstance(height, int) or height <= 0 or height in heights:
                raise ValueError(f"observation {index}: missing or duplicate finalized_height")
            heights.add(height)
            sample["finalized_height"] = height
        samples.append(sample)
    return {**reference, "samples": samples}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", required=True, choices=MODES)
    parser.add_argument("--observations", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    rows = [json.loads(line) for line in args.observations.read_text().splitlines() if line.strip()]
    result = record(rows, args.scenario)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(f"Recorded {len(result['samples'])} observed samples for {args.scenario}; not a validated full comparison")


if __name__ == "__main__":
    main()
