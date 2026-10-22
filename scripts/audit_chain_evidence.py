#!/usr/bin/env python3
"""Audit chain reports against measurement records; never infer on-chain hashes."""
import argparse
import json
from pathlib import Path


def audit(root):
    files = sorted(root.glob("launch-???.json"))
    if len(files) != 3:
        raise ValueError("three independent launch observations required")
    unique = set()
    observations = 0
    for path in files:
        samples = json.loads(path.read_text())
        report_path = root / path.stem / "three-node" / "report.json"
        report = json.loads(report_path.read_text())
        if report.get("result") != "PASS" or len(samples) != 30:
            raise ValueError("incomplete launch report or sample count")
        tasks = {item["task"]["task_id"]: item["task"]
                 for item in report["steps"] if item.get("step") == "task"}
        if len(tasks) != 30:
            raise ValueError("expected 30 separately submitted real tasks")
        for sample in samples:
            rid = sample["request_id"]
            if rid in unique:
                raise ValueError("duplicate measurement ID")
            unique.add(rid)
            task = tasks.get(sample["task_id"])
            if task is None or task["sequence"] != sample["finalized_height"]:
                raise ValueError("measurement does not map to submitted task")
            if task.get("workload") != "qmlp-v1" or task.get("rows") != sample["rows"]:
                raise ValueError("submission workload differs")
            if sample.get("fixture_source") != "trained_qmlp_export" or not sample.get("block_id"):
                raise ValueError("missing export provenance or finalized block ID")
            observations += 1
    return {"observations": observations, "verified": [
        "submitted_task_id_corresponds_to_report",
        "task_height_corresponds_to_recorded_finalized_height",
        "qmlp_workload_and_row_count",
        "finality_block_identifier_was_observed_by_collector"],
        "not_independently_verified": [
        "model_input_result_commitments_from_finalized_block",
        "validator_execution_equivalence",
        "equal_service_vs_finality_latency_boundaries"],
        "comparison_ready": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chain", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    result = audit(args.chain)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print("PASS: verified submit-to-observation links; on-chain commitments NOT independently verified")


if __name__ == "__main__":
    main()
