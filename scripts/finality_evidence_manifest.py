#!/usr/bin/env python3
"""Create a provenance manifest for real finality observations; do not invent data."""
import argparse
import hashlib
import json
import platform
import subprocess
from pathlib import Path


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def manifest(directory, revision, run_id, environment):
    paths = sorted(directory.glob("launch-???.json"))
    if len(paths) != 3:
        raise ValueError("need exactly 3 real launch evidence files")
    entries = []
    all_ids = set()
    for path in paths:
        observations = json.loads(path.read_text())
        if not isinstance(observations, list) or len(observations) != 30:
            raise ValueError(f"{path.name}: expected 30 observations")
        launch_ids = {x["launch_id"] for x in observations}
        if len(launch_ids) != 1:
            raise ValueError(f"{path.name}: inconsistent launch IDs")
        heights = set()
        for row in observations:
            request_id, height = row["request_id"], row["finalized_height"]
            if request_id in all_ids or height in heights:
                raise ValueError(f"{path.name}: duplicate request ID or height")
            if row.get("workload") != "qmlp-v1" or row.get("elapsed_ms", 0) <= 0:
                raise ValueError(f"{path.name}: invalid observation")
            if not row.get("task_id") or not row.get("block_id"):
                raise ValueError(f"{path.name}: missing task/block identity")
            all_ids.add(request_id)
            heights.add(height)
        entries.append({"file": path.name, "sha256": sha256(path),
                        "samples": len(observations), "launch_id": next(iter(launch_ids))})
    if len({x["launch_id"] for x in entries}) != 3:
        raise ValueError("launch IDs must be unique across runs")
    if not revision or not run_id:
        raise ValueError("revision and run ID required")
    return {"kind": "observed-qmlp-finality-only", "git_revision": revision,
            "ci_run_id": run_id, "environment": environment,
            "samples_total": len(all_ids), "files": entries,
            "interpretation": "Raw chain-only timing; not a matched service comparison or independent attestation"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    env = {"python": platform.python_version(), "system": platform.platform()}
    for command, key in ((["docker", "--version"], "docker"),
                         (["docker", "compose", "version"], "docker_compose")):
        env[key] = subprocess.check_output(command, text=True).strip()
    result = manifest(args.directory, args.revision, args.run_id, env)
    args.out.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print("PASS: provenance recorded for 90 raw blockchain observations")


if __name__ == "__main__":
    main()
