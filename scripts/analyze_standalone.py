#!/usr/bin/env python3
"""Compare direct CPU QMLP inference with verification of an identical AI-work block.

Direct inference computes the exact same committed integer model and rows in
one Rust process. It excludes claims/signatures, ledger, IPC, quorum, network
and finality. This is a narrow baseline, NOT an optimized conventional serving
system, AI-training comparison, or a network throughput measurement.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

from analyze_replicates import distribution


def positive(value, label: str) -> float:
    if not isinstance(value, (float, int)) or not math.isfinite(value) or value <= 0:
        raise ValueError(f"{label} must be positive and finite")
    return float(value)


def analyze(benchmarks: list[dict]) -> dict:
    if len(benchmarks) < 3:
        raise ValueError("Require three independent process-level benchmark invocations")
    signatures = {
        (b["iterations"], b["qmlp_v1"]["rows"],
         b["qmlp_v1"]["partitions"], b["qmlp_v1"]["layers"])
        for b in benchmarks
    }
    if len(signatures) != 1:
        raise ValueError("All comparisons must use identical QMLP tasks and iterations")

    controls = ["standalone_qmlp_inference_us", "full_block_verify_rust_mirror_us",
                "full_block_verify_haskell_persistent_us"]
    series = {k: [] for k in controls}
    series.update({"rust_verification_over_direct": [], "haskell_verification_over_direct": []})
    for b in benchmarks:
        q = b["qmlp_v1"]
        direct, rust, haskell = (positive(q[k], k) for k in controls)
        series[controls[0]].append(direct)
        series[controls[1]].append(rust)
        series[controls[2]].append(haskell)
        series["rust_verification_over_direct"].append(rust / direct)
        series["haskell_verification_over_direct"].append(haskell / direct)

    sweep = benchmarks[0].get("sweeps")
    if not sweep:
        raise ValueError("First benchmark must include the controlled scaling sweeps")
    rows_out = []
    seen_rows = set()
    for entry in sweep["by_rows_8_partitions"]:
        rows = entry["rows"]
        q = entry["qmlp_v1"]
        if rows in seen_rows or rows not in (16, 64, 256) or q["rows"] != rows or q["partitions"] != 8:
            raise ValueError("Unexpected or duplicate row-scaling workload")
        seen_rows.add(rows)
        direct = positive(q["standalone_qmlp_inference_us"], "row direct inference")
        verified = positive(q["full_block_verify_rust_mirror_us"], "row block verification")
        rows_out.append({"rows": rows, "partitions": 8,
                         "direct_inference_ms": round(direct / 1000, 6),
                         "rust_block_verification_ms": round(verified / 1000, 6),
                         "rust_verification_over_direct": round(verified / direct, 6)})
    if seen_rows != {16, 64, 256}:
        raise ValueError("Missing row-scaling workloads")
    parts_out = []
    seen_parts = set()
    for q in sweep["qmlp_32_64_16_by_partitions"]:
        n = q["partitions"]
        if n in seen_parts or n not in (1, 2, 4, 8, 16, 32, 64) or q["rows"] != 256:
            raise ValueError("Unexpected or duplicate partition-scaling workload")
        seen_parts.add(n)
        direct = positive(q["standalone_qmlp_inference_us"], "partition direct inference")
        verified = positive(q["full_block_verify_rust_mirror_us"], "partition block verification")
        parts_out.append({"partitions": n, "rows": 256,
                          "direct_inference_ms": round(direct / 1000, 6),
                          "rust_block_verification_ms": round(verified / 1000, 6),
                          "rust_verification_over_direct": round(verified / direct, 6)})
    if seen_parts != {1, 2, 4, 8, 16, 32, 64}:
        raise ValueError("Missing partition-scaling workloads")

    return {
        "method": {
            "comparison": "same integer QMLP-V1 model/rows, direct CPU logits vs full block validation",
            "iterations_per_invocation": benchmarks[0]["iterations"],
            "process_invocations": len(benchmarks),
            "rows": benchmarks[0]["qmlp_v1"]["rows"],
            "partitions": benchmarks[0]["qmlp_v1"]["partitions"],
            "layers": benchmarks[0]["qmlp_v1"]["layers"],
            "exclusions": [
                "no full network transport, BFT finality or P2P latency in either timing",
                "direct baseline is Rust integer CPU inference, not optimized GPU/ML serving",
                "block verification includes signatures, full recomputation and settlement",
            ],
            "uncertainty": "descriptive same-runner variation, not generalizable performance",
        },
        "main_comparison": {k: distribution(v) for k, v in series.items()},
        "row_sweep_one_invocation": sorted(rows_out, key=lambda x: x["rows"]),
        "partition_sweep_one_invocation": sorted(parts_out, key=lambda x: x["partitions"]),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmarks", type=Path, nargs="+", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    summary = analyze([json.loads(p.read_text()) for p in args.benchmarks])
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
