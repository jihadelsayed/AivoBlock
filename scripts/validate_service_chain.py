#!/usr/bin/env python3
"""Fail closed on mismatched or insufficient service-vs-chain benchmark data.

Inputs are normalized JSON measurement bundles, not arbitrary source CI reports.
See docs/service-chain-evidence-audit.md for the expected bundle schema.
"""
import argparse
import json
import math
from collections import Counter
from pathlib import Path

FIELDS = ("model_commitment", "input_commitment", "result_commitment")
SCENARIOS = ("service_direct", "service_recomputed", "chain_observed_finality")


def validate(bundle, *, minimum_launches=3, minimum_per_launch=30):
    if not isinstance(bundle, dict) or not isinstance(bundle.get("scenarios"), dict):
        raise ValueError("Missing scenarios object")
    ref = None
    result = {}
    for mode in SCENARIOS:
        scenario = bundle["scenarios"].get(mode)
        if not isinstance(scenario, dict):
            raise ValueError(f"{mode}: missing scenario")
        identity = tuple(scenario.get(field) for field in FIELDS)
        if any(not isinstance(x, str) or len(x) != 64 or
               any(c not in "0123456789abcdef" for c in x) for x in identity):
            raise ValueError(f"{mode}: missing or invalid SHA-256 commitments")
        if ref is None:
            ref = identity
        elif identity != ref:
            raise ValueError(f"{mode}: model/input/result commitments differ")
        samples = scenario.get("samples")
        if not isinstance(samples, list):
            raise ValueError(f"{mode}: missing samples")
        counts = Counter()
        request_ids = set()
        heights = set()
        for s in samples:
            if not isinstance(s, dict):
                raise ValueError(f"{mode}: invalid sample")
            launch = s.get("launch_id")
            rid = s.get("request_id")
            ms = s.get("elapsed_ms")
            if not isinstance(launch, str) or not launch or not isinstance(rid, str) or not rid:
                raise ValueError(f"{mode}: launch_id/request_id required")
            for field, expected in zip(FIELDS, ref):
                supplied = s.get(field)
                if supplied != expected:
                    raise ValueError(f"{mode}: request {rid} missing or mismatched {field}")
            if rid in request_ids:
                raise ValueError(f"{mode}: duplicate request ID: {rid}")
            request_ids.add(rid)
            if isinstance(ms, bool) or not isinstance(ms, (int, float)) or not math.isfinite(ms) or ms <= 0:
                raise ValueError(f"{mode}: invalid elapsed_ms")
            if s.get("status") != "success":
                raise ValueError(f"{mode}: non-success sample; report failures separately")
            if mode == "chain_observed_finality":
                h = s.get("finalized_height")
                if isinstance(h, bool) or not isinstance(h, int) or h < 1 or h in heights:
                    raise ValueError(f"{mode}: missing/duplicate finalized height")
                heights.add(h)
            counts[launch] += 1
        if len(counts) < minimum_launches or any(n < minimum_per_launch for n in counts.values()):
            raise ValueError(f"{mode}: need >= {minimum_launches} launches with >= {minimum_per_launch} measured samples each")
        result[mode] = {"launches": len(counts), "samples": len(samples)}
    return {"status": "PASS", "commitments": dict(zip(FIELDS, ref)), "scenarios": result}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("bundle", type=Path)
    p.add_argument("--out", type=Path)
    args = p.parse_args()
    verified = validate(json.loads(args.bundle.read_text()))
    rendered = json.dumps(verified, indent=2, sort_keys=True) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(rendered)
    print(rendered, end="")


if __name__ == "__main__":
    main()
