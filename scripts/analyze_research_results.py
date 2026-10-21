#!/usr/bin/env python3
"""Summarize CI benchmark/research JSON without external Python dependencies.

Inputs are written by `aivoblock-node bench --iterations 20 --sweep`,
`python -m aivoblock_ai.research`, and `scripts/testnet.py demo/fault`.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def analyze(bench: dict, research: dict, three: dict, four: dict) -> dict:
    """Check artifact provenance and report descriptive (not causal) metrics."""
    if three.get("result") != "PASS" or four.get("result") != "PASS":
        raise ValueError("Both private testnet scenarios must have passed")
    if not research["onnx_integer"]["exact_match"] or research["onnx_integer"]["mismatched_rows"]:
        raise ValueError("Integer ONNX disagreed with the reference")
    exported = [step for step in three["steps"] if step["step"] == "pytorch_export"]
    paid = [step for step in three["steps"] if step["step"] == "pytorch_rewards"]
    if len(exported) != 1 or len(paid) != 1:
        raise ValueError("Exactly one trained-model export and reward audit are required")
    if exported[0]["model_commitment"] != research["qmlp"]["model_commitment"]:
        raise ValueError("Blockchain model commitment differs from training artifact")
    if sum(paid[0]["worker_gains"]) != paid[0]["tokens_paid"] or paid[0]["height"] != 5:
        raise ValueError("Trained-model reward audit disagrees with worker balances")
    if three["final"]["height"] != 5 or four["final"]["height"] < 5:
        raise ValueError("Expected testnet block heights were not finalized")

    linear_once = bench["full_block_verify_haskell_cross_checked_us"]
    linear_persist = bench["full_block_verify_haskell_persistent_us"]
    linear_mirror = bench["full_block_verify_rust_mirror_us"]
    qmlp = bench["qmlp_v1"]
    qmlp_once = qmlp["full_block_verify_haskell_cross_checked_us"]
    qmlp_persist = qmlp["full_block_verify_haskell_persistent_us"]
    qmlp_mirror = qmlp["full_block_verify_rust_mirror_us"]
    parts = bench["sweeps"]["qmlp_32_64_16_by_partitions"]
    endpoints = {entry["partitions"]: entry for entry in parts}
    if 1 not in endpoints or 64 not in endpoints:
        raise ValueError("Missing 1/64 partition comparison")
    return {
        "training": {
            "train_rows": research["dataset"]["train"],
            "test_rows": research["dataset"]["test"],
            "float_accuracy_pct": round(100 * research["float"]["accuracy"], 3),
            "quantized_accuracy_pct": round(100 * research["qmlp"]["accuracy"], 3),
            "prediction_agreement_pct": round(100 * research["qmlp"]["agreement_with_float"], 3),
            "float_onnx_max_abs_difference": research["onnx_float"]["max_abs_diff_vs_torch"],
            "integer_onnx_mismatched_rows": research["onnx_integer"]["mismatched_rows"],
            "model_commitment": research["qmlp"]["model_commitment"],
        },
        "benchmark": {
            "iterations": bench["iterations"],
            "linear_256_rows_64_partitions": {
                "rust_mirror_ms": round(linear_mirror / 1000, 3),
                "haskell_one_shot_ms": round(linear_once / 1000, 3),
                "haskell_persistent_ms": round(linear_persist / 1000, 3),
                "persistent_speedup_over_one_shot": round(linear_once / linear_persist, 3),
            },
            "qmlp_256_rows_64_partitions": {
                "rust_mirror_ms": round(qmlp_mirror / 1000, 3),
                "haskell_one_shot_ms": round(qmlp_once / 1000, 3),
                "haskell_persistent_ms": round(qmlp_persist / 1000, 3),
                "persistent_speedup_over_one_shot": round(qmlp_once / qmlp_persist, 3),
            },
            "qmlp_256_rows_partition_sweep": {
                "one_partition_rust_ms": round(endpoints[1]["full_block_verify_rust_mirror_us"] / 1000, 3),
                "sixty_four_partitions_rust_ms": round(endpoints[64]["full_block_verify_rust_mirror_us"] / 1000, 3),
                "verification_slowdown": round(endpoints[64]["full_block_verify_rust_mirror_us"] / endpoints[1]["full_block_verify_rust_mirror_us"], 3),
            },
        },
        "testnet": {
            "three_node": {
                "finalized_blocks": three["final"]["height"],
                "scenario_duration_s": three["duration_s"],
                "finality_median_ms": three["finality_summary_ms"]["median"],
                "finality_p95_ms": three["finality_summary_ms"]["p95"],
                "finality_samples": three["finality_summary_ms"]["count"],
                "issued_tokens": three["issued"],
                "paid_tokens": three["paid"],
                "trained_model_worker_rewards": paid[0]["worker_gains"],
                "trained_model_tokens_paid": paid[0]["tokens_paid"],
            },
            "four_validator": {
                "finalized_blocks": four["final"]["height"],
                "scenario_duration_s": four["duration_s"],
                "finality_median_ms": four["finality_summary_ms"]["median"],
                "finality_p95_ms": four["finality_summary_ms"]["p95"],
                "finality_samples": four["finality_summary_ms"]["count"],
                "issued_tokens": four["issued"],
                "paid_tokens": four["paid"],
            },
        },
    }


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    for name in ("bench", "research", "three", "four"):
        p.add_argument("--" + name, type=Path, required=True)
    p.add_argument("--out", type=Path, help="Save summary to this JSON file")
    args = p.parse_args()
    inputs = [json.loads(getattr(args, name).read_text()) for name in ("bench", "research", "three", "four")]
    summary = analyze(*inputs)
    encoded = json.dumps(summary, indent=2, sort_keys=True) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(encoded)
    print(encoded, end="")


if __name__ == "__main__":
    main()
