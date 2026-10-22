#!/usr/bin/env python3
"""Gate trained-export identity claims; not independent on-chain attestation."""
import argparse
import json
from pathlib import Path


def validate(chain_dir, service):
    files = sorted(chain_dir.glob("launch-???.json"))
    if len(files) != 3:
        raise ValueError("expected three trained-QMLP chain launches")
    canonical = service["canonical_commitments"]
    seen_requests, launch_ids = set(), set()
    for file in files:
        rows = json.loads(file.read_text())
        if len(rows) != 30:
            raise ValueError("expected 30 observations per launch")
        launches = {r["launch_id"] for r in rows}
        if len(launches) != 1 or next(iter(launches)) in launch_ids:
            raise ValueError("duplicate or inconsistent launch")
        launch_ids.update(launches)
        heights = set()
        for row in rows:
            if row.get("fixture_source") != "trained_qmlp_export" or row.get("workload") != "qmlp-v1":
                raise ValueError("seeded fixture cannot be compared to trained CPU model")
            if row.get("model_commitment") != canonical["model_commitment"]:
                raise ValueError("model commitment mismatch")
            if row.get("result_commitment") != canonical["result_commitment"]:
                raise ValueError("result commitment mismatch")
            if row.get("rows") != 12 or row.get("polling_interval_s") != 0.1:
                raise ValueError("workload configuration differs")
            if row["request_id"] in seen_requests or row["finalized_height"] in heights:
                raise ValueError("duplicate request or height")
            seen_requests.add(row["request_id"])
            heights.add(row["finalized_height"])
            if row["end_ns"] <= row["start_ns"] or row["elapsed_ms"] <= 0:
                raise ValueError("invalid timing")
    if service["samples"] != 90 or len(seen_requests) != 90:
        raise ValueError("insufficient real observations")
    return {
        "chain_samples": 90, "service_samples": 90,
        "shared_export_model_commitment": canonical["model_commitment"],
        "shared_export_result_commitment": canonical["result_commitment"],
        "identity_level": "shared export provenance, not independently verified on-chain commitment",
        "not_verified": ["chain-side input commitment", "chain-side executed result attestation",
                         "equivalent latency measurement boundary", "causal overhead"],
        "comparison_ready": False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chain", type=Path, required=True)
    parser.add_argument("--service", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    report = validate(args.chain, json.loads(args.service.read_text()))
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print("PASS: shared trained export provenance; NOT a verified matched performance comparison")


if __name__ == "__main__":
    main()
