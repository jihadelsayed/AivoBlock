"""Validate a paired Rust/Python QMLP research measurement without overclaiming.

Never combine reports for different committed model/input/results or numbers
of rows. Cross-language latency is descriptive and NOT blockchain finality.
"""
from __future__ import annotations

import argparse
import json
import math
import statistics
from pathlib import Path


def summarize(python: dict, rust: dict) -> dict:
    if python.get("mode") != "trusted_result":
        raise ValueError("Python comparison must use the trusted-result mode")
    if rust.get("mode") != "rust_reference_inference_only":
        raise ValueError("Rust report has the wrong mode")
    for key in ("model_commitment", "input_commitment", "result_commitment", "rows"):
        if python.get(key) != rust.get(key):
            raise ValueError(f"Cross-runtime workload mismatch: {key}")
    if type(python.get("rows")) is not int or not 1 <= python["rows"] <= 256:
        raise ValueError("Invalid QMLP row count")
    summaries = {}
    for name, report in (("python", python), ("rust", rust)):
        observations = report.get("latency_ms", {}).get("observations")
        if (not isinstance(observations, list) or len(observations) < 3
                or report.get("iterations") != len(observations)
                or any(type(x) not in (int, float) or not math.isfinite(x) or x <= 0
                       for x in observations)):
            raise ValueError(f"Invalid {name} latency observations")
        summaries[name] = {
            "iterations": len(observations),
            "mean_ms": statistics.mean(observations),
            "median_ms": statistics.median(observations),
            "sample_stddev_ms": statistics.stdev(observations),
            "rows_per_second_from_mean": python["rows"] * 1000 / statistics.mean(observations),
        }
    return {
        "schema_version": 1,
        "scope": "descriptive CPU Rust-vs-Python QMLP inference only; not blockchain throughput or BFT finality",
        "model_commitment": python["model_commitment"],
        "input_commitment": python["input_commitment"],
        "result_commitment": python["result_commitment"],
        "rows": python["rows"],
        "measurements": summaries,
        "rust_over_python_mean_latency_ratio": summaries["rust"]["mean_ms"] / summaries["python"]["mean_ms"],
        "limitations": [
            "Language runtime and implementation overhead differ",
            "Job sharing and CPU scheduling may affect observations",
            "No matched block-verification or network-finality timing",
            "Single process invocation per language is not a statistical replicate",
        ],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    report = summarize(json.loads(args.python.read_text()), json.loads(args.rust.read_text()))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    main()
