#!/usr/bin/env python3
"""Build a versioned, validated workload manifest from AivoBlock benchmark JSON.

The SHA-256 task commitments are produced by the Rust benchmark (PR #67).
This tool records reproducibility metadata, not proof of performance or finality.
"""
from __future__ import annotations

import argparse
import json
import platform
import re
import sys
from pathlib import Path

_HEX64 = re.compile(r"^[0-9a-f]{64}$")
_LAYERS = re.compile(r"^[1-9][0-9]* -> [1-9][0-9]* -> [1-9][0-9]*$")


def positive_int(value: object, name: str) -> int:
    if type(value) is not int or value <= 0:
        raise ValueError(f"{name} must be a positive integer")
    return value


def make_manifest(benchmarks: list[dict], *, runner: str, revision: str,
                  machine: dict[str, str]) -> dict:
    if len(benchmarks) < 3:
        raise ValueError("At least three separate benchmark invocations required")
    if not runner.strip() or not revision.strip():
        raise ValueError("Runner identity and source revision are required")
    for key in ("system", "machine", "processor", "python"):
        if not isinstance(machine.get(key), str) or not machine[key].strip():
            raise ValueError(f"Missing machine metadata: {key}")
    entries = []
    for index, bench in enumerate(benchmarks, 1):
        q = bench["qmlp_v1"]
        model = q["model_commitment"]
        data = q["input_commitment"]
        for label, digest in (("model", model), ("input", data)):
            if not isinstance(digest, str) or not _HEX64.fullmatch(digest):
                raise ValueError(f"Invalid {label} commitment in invocation {index}")
        layer = q["layers"]
        if not isinstance(layer, str) or not _LAYERS.fullmatch(layer):
            raise ValueError(f"Invalid QMLP layer dimensions in invocation {index}")
        layer_sizes = tuple(int(n) for n in layer.split(" -> "))
        if layer_sizes[0] > 32 or layer_sizes[1] > 64 or layer_sizes[2] > 16:
            raise ValueError("QMLP layer dimensions exceed supported benchmark bounds")
        if "integer_multiply_adds_per_row" in q:
            expected_macs = layer_sizes[0] * layer_sizes[1] + layer_sizes[1] * layer_sizes[2]
            if q["integer_multiply_adds_per_row"] != expected_macs:
                raise ValueError("QMLP model operation count disagrees with layer dimensions")
        entries.append({
            "invocation": index,
            "iterations": positive_int(bench["iterations"], "iterations"),
            "rows": positive_int(q["rows"], "rows"),
            "partitions": positive_int(q["partitions"], "partitions"),
            "layers": layer,
            "model_commitment": model,
            "input_commitment": data,
        })
    first = {k: v for k, v in entries[0].items() if k != "invocation"}
    for entry in entries[1:]:
        if {k: v for k, v in entry.items() if k != "invocation"} != first:
            raise ValueError("Mismatched workloads or invocation settings")
    if first["rows"] > 256 or first["partitions"] > 64 or first["partitions"] > first["rows"]:
        raise ValueError("Workload exceeds V3 task limits")
    return {
        "schema_version": 1,
        "workload": first,
        "replicates": len(entries),
        "invocations": entries,
        "environment": {"runner": runner, "revision": revision, **machine},
        "scope": "single-host benchmark identity only; no BFT finality, service latency or energy claims",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--benchmarks", type=Path, nargs="+", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--runner", required=True)
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    machine = {
        "system": platform.platform(),
        "machine": platform.machine() or "unknown",
        "processor": platform.processor() or platform.machine() or "unknown",
        "python": sys.version.split()[0],
    }
    report = make_manifest([json.loads(p.read_text()) for p in args.benchmarks],
                           runner=args.runner, revision=args.revision, machine=machine)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    main()
