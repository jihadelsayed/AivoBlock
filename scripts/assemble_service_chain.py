#!/usr/bin/env python3
"""Assemble independently recorded scenario fragments; never invent measurements.

A PASS indicates consistency and sufficient samples, NOT a proven causal,
apples-to-apples performance advantage or independent chain attestation.
"""
import argparse
import json
from pathlib import Path

from validate_service_chain import SCENARIOS, validate


def assemble(fragments):
    if not isinstance(fragments, dict) or set(fragments) != set(SCENARIOS):
        raise ValueError("exactly three named scenario fragments required")
    scenarios = {}
    for name in SCENARIOS:
        fragment = fragments[name]
        if not isinstance(fragment, dict):
            raise ValueError(f"{name}: scenario fragment must be an object")
        scenarios[name] = fragment
    bundle = {"scenarios": scenarios}
    verdict = validate(bundle)
    return bundle, verdict


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in SCENARIOS:
        parser.add_argument("--" + name.replace("_", "-"), type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--verdict", type=Path, required=True)
    args = parser.parse_args()
    fragments = {
        name: json.loads(getattr(args, name).read_text())
        for name in SCENARIOS
    }
    bundle, verdict = assemble(fragments)
    for path, data in ((args.out, bundle), (args.verdict, verdict)):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")
    print("PASS: consistent measured fragment schema; performance comparison not established")


if __name__ == "__main__":
    main()
