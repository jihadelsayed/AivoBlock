#!/usr/bin/env python3
"""Describe between-run variability in off-chain model accuracy and verifier timings.

Three or more distinct dataset AND model-initialization seeds are required.
Benchmark samples come from separate local invocations, not independent
machines. This reports descriptive sample standard deviations, NOT a
confidence interval or a claim of production performance.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import statistics


def distribution(values: list[float]) -> dict:
    """Report unrounded observations plus descriptive between-run statistics."""
    if len(values) < 3 or not all(math.isfinite(v) for v in values):
        raise ValueError("At least three finite observations are required")
    return {
        "n": len(values),
        "observations": values,
        "mean": round(statistics.mean(values), 6),
        "median": round(statistics.median(values), 6),
        "sample_stddev": round(statistics.stdev(values), 6),
        "min": min(values),
        "max": max(values),
    }


def analyze(models: list[dict], benchmarks: list[dict]) -> dict:
    """Validate independent seeds and equal workloads before summarizing data."""
    if len(models) < 3 or len(benchmarks) < 3:
        raise ValueError("Need at least three seed runs and three benchmark invocations")

    data_seeds = [r["dataset"]["seed"] for r in models]
    model_seeds = [r["dataset"]["training_seed"] for r in models]
    if len(set(data_seeds)) != len(data_seeds) or len(set(model_seeds)) != len(model_seeds):
        raise ValueError("Dataset and PyTorch training seeds must each be distinct")
    dataset_settings = {
        (r["dataset"]["train"], r["dataset"]["test"], r["dataset"]["features"],
         r["dataset"]["classes"], r["float"]["epochs"], r["float"]["layers"])
        for r in models
    }
    if len(dataset_settings) != 1:
        raise ValueError("Model runs must use identical model/data dimensions and epochs")
    commitments = [r["qmlp"]["model_commitment"] for r in models]
    if len(set(commitments)) != len(commitments):
        raise ValueError("Independent training runs unexpectedly produced the same model")
    for r in models:
        if r["onnx_integer"]["mismatched_rows"] != 0 or not r["onnx_integer"]["exact_match"]:
            raise ValueError("Integer ONNX did not reproduce deterministic logits")
        for section, key in (("float", "accuracy"), ("qmlp", "accuracy"),
                             ("qmlp", "agreement_with_float")):
            value = r[section][key]
            if not isinstance(value, (int, float)) or not math.isfinite(value) or not 0 <= value <= 1:
                raise ValueError(f"Invalid accuracy: {section}.{key}={value}")

    signatures = {
        (b["iterations"], b["task"]["rows"], b["task"]["partitions"],
         b["qmlp_v1"]["rows"], b["qmlp_v1"]["partitions"], b["qmlp_v1"]["layers"])
        for b in benchmarks
    }
    if len(signatures) != 1:
        raise ValueError("Benchmark iterations and task sizes must be identical")
    timing_names = {
        "linear_rust_mirror_ms": lambda b: b["full_block_verify_rust_mirror_us"] / 1000,
        "linear_haskell_one_shot_ms": lambda b: b["full_block_verify_haskell_cross_checked_us"] / 1000,
        "linear_haskell_persistent_ms": lambda b: b["full_block_verify_haskell_persistent_us"] / 1000,
        "qmlp_rust_mirror_ms": lambda b: b["qmlp_v1"]["full_block_verify_rust_mirror_us"] / 1000,
        "qmlp_haskell_one_shot_ms": lambda b: b["qmlp_v1"]["full_block_verify_haskell_cross_checked_us"] / 1000,
        "qmlp_haskell_persistent_ms": lambda b: b["qmlp_v1"]["full_block_verify_haskell_persistent_us"] / 1000,
    }
    measurements = {}
    for label, extract in timing_names.items():
        values = [extract(b) for b in benchmarks]
        if any(v <= 0 for v in values):
            raise ValueError(f"Non-positive benchmark sample: {label}")
        measurements[label] = distribution(values)

    for label, section in (("linear", None), ("qmlp", "qmlp_v1")):
        def timing(b: dict, field: str) -> float:
            return (b if section is None else b[section])[field]
        measurements[label + "_persistent_speedup"] = distribution([
            timing(b, "full_block_verify_haskell_cross_checked_us") /
            timing(b, "full_block_verify_haskell_persistent_us") for b in benchmarks
        ])

    return {
        "method": {
            "dataset_seeds": data_seeds,
            "training_seeds": model_seeds,
            "model_runs": len(models),
            "benchmark_invocations": len(benchmarks),
            "iterations_per_benchmark_invocation": benchmarks[0]["iterations"],
            "scope": "descriptive within-runner variation only; NOT cross-machine or statistical significance",
        },
        "accuracy_percent": {
            "float": distribution([100 * r["float"]["accuracy"] for r in models]),
            "qmlp": distribution([100 * r["qmlp"]["accuracy"] for r in models]),
            "agreement_with_float": distribution(
                [100 * r["qmlp"]["agreement_with_float"] for r in models]
            ),
        },
        "integer_onnx_mismatches": sum(r["onnx_integer"]["mismatched_rows"] for r in models),
        "benchmark": measurements,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", type=Path, nargs="+", required=True)
    parser.add_argument("--benchmarks", type=Path, nargs="+", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    models = [json.loads(p.read_text()) for p in args.models]
    benchmarks = [json.loads(p.read_text()) for p in args.benchmarks]
    summary = analyze(models, benchmarks)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
