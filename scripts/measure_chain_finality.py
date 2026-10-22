#!/usr/bin/env python3
"""Measure repeated real QMLP testnet submissions; no synthetic observations.

Each launch generates fresh private-testnet keys and storage. CLI submission,
consensus, and polling all validators are included in the timed boundary.
Output is raw evidence, NOT a matched three-scenario benchmark fragment.
"""
import argparse
import json
import time
from pathlib import Path

from testnet import Testnet, observed_finality_timing, run, ROOT, IMAGE


def measure_launch(net, *, samples, rows, polling_interval_s=1.0):
    if samples < 1 or rows < 1:
        raise ValueError("samples and rows must be positive")
    if not isinstance(polling_interval_s, (int, float)) or isinstance(polling_interval_s, bool) or not (0 < polling_interval_s <= 10):
        raise ValueError("polling interval must be between 0 and 10 seconds")
    nodes = net.nodes()
    net.up()
    measurements = []
    launch_id = net.out.parent.name
    for index in range(samples):
        before = net.status("node-1")["height"]
        start = time.perf_counter_ns()
        task = net.submit("--workload", "qmlp", "--rows", str(rows))
        target = int(task["sequence"])
        if target <= before:
            raise ValueError("submitted task did not advance target height")
        # Explicit bounded polling cadence, included in the observation boundary.
        net.wait(f"height {target} finalized on {nodes}", 180,
                 lambda: all(net.status(n)["height"] >= target for n in nodes),
                 interval=polling_interval_s)
        timing = observed_finality_timing(start, time.perf_counter_ns())
        status = net.verify(nodes)
        if status["height"] < target:
            raise ValueError("target height not observed")
        block = next((b for b in status["blocks"] if b["height"] == target), None)
        if block is None or block["workload"] != "qmlp-v1":
            raise ValueError("QMLP task not included in observed finalized block")
        measurements.append({
            "launch_id": launch_id, "request_id": f"{launch_id}-{index}",
            "task_id": task["task_id"], "finalized_height": target,
            "block_id": block["block_id"], "workload": "qmlp-v1",
            "rows": task["rows"], "validators": len(nodes),
            "observation_boundary": "before_cli_submission_to_all_validator_height_poll",
            "polling_interval_s": polling_interval_s, **timing,
        })
        net.report["measurements"] = measurements
        net.save_report()
    return measurements


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--launches", type=int, default=3)
    ap.add_argument("--samples", type=int, default=30)
    ap.add_argument("--rows", type=int, default=12)
    ap.add_argument("--polling-interval-s", type=float, default=1.0)
    ap.add_argument("--skip-build", action="store_true")
    ap.add_argument("--out", type=Path, default=ROOT / "testnet" / "generated" / "measurements")
    args = ap.parse_args()
    if min(args.launches, args.samples, args.rows) < 1:
        ap.error("launches, samples and rows must be positive")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if not args.skip_build:
        run(["docker", "build", "-f", str(ROOT / "docker" / "Dockerfile.node"),
             "-t", IMAGE, str(ROOT)])
    for launch in range(args.launches):
        # Unique per-launch output folder, fresh key material and ledger.
        net = Testnet("three-node", out / f"launch-{launch:03d}")
        try:
            net.init()
            observations = measure_launch(net, samples=args.samples, rows=args.rows,
                                          polling_interval_s=args.polling_interval_s)
            net.report["result"] = "PASS"
        except Exception as exc:
            net.report["result"] = f"FAIL: {type(exc).__name__}: {exc}"
            net.save_report()
            raise
        finally:
            try:
                net.down()
            finally:
                net.save_report()
        path = out / f"launch-{launch:03d}.json"
        path.write_text(json.dumps(observations, indent=2) + "\n")
    print(f"Recorded {args.launches} independent launches in {out}; raw chain evidence only")


if __name__ == "__main__":
    main()
