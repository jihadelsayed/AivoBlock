#!/usr/bin/env python3
"""Independently re-analyze a published AivoBlock evidence ZIP on a clean runner.

No imports from the bundle producer, timing summarizer or blockchain code.
This checks archived *observations*, not independent re-execution of AI tasks
or external replication of network performance.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import statistics
import zipfile
from pathlib import PurePosixPath, Path

PREFIX = "evidence/testnet/generated/"
SCENARIOS = (
    ("pilot-measurements", "phase-summary-1s-seeded.json", 1.0, "seeded_node_fixture"),
    ("fast-poll-measurements", "phase-summary-100ms-seeded.json", 0.1, "seeded_node_fixture"),
    ("trained-qmlp-chain", "phase-summary-100ms-trained.json", 0.1, "trained_qmlp_export"),
)
PHASES = ("cli_submission_ms", "post_submission_to_observed_finality_ms", "total_ms")
HEX = set("0123456789abcdef")


def require(ok: bool, message: str) -> None:
    if not ok:
        raise ValueError(message)


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def positive(value: object) -> float:
    require(type(value) in (int, float), "non-numeric timing")
    require(math.isfinite(value) and value > 0, "invalid timing value")
    return float(value)


def near(observed: object, actual: float, label: str) -> None:
    require(type(observed) in (int, float) and math.isfinite(observed) and
            math.isclose(observed, actual, rel_tol=1e-9, abs_tol=1e-6),
            f"recomputed metric differs: {label}")


def order_stats(values: list[float]) -> dict:
    require(len(values) >= 2, "insufficient samples for dispersion")
    ordered = sorted(values)
    return {
        "n": len(ordered),
        "min_ms": ordered[0],
        "median_ms": statistics.median(ordered),
        "p95_nearest_rank_ms": ordered[math.ceil(0.95 * len(ordered)) - 1],
        "mean_ms": statistics.mean(ordered),
        "max_ms": ordered[-1],
        "sample_stddev_ms": statistics.stdev(ordered),
    }


def check_stats(actual: list[float], claimed: dict, context: str) -> dict:
    computed = order_stats(actual)
    require(isinstance(claimed, dict) and
            type(claimed.get("n")) is int and claimed["n"] == computed["n"],
            f"sample count differs: {context}")
    for key, value in computed.items():
        if key != "n":
            near(claimed.get(key), value, f"{context}.{key}")
    return computed


def audit(path: Path, expected_run_id: str | None = None) -> dict:
    with zipfile.ZipFile(path) as archive:
        members = archive.namelist()
        require(len(set(members)) == len(members), "duplicate archive member")
        require(all(name and not PurePosixPath(name).is_absolute() and
                    not any(p in ("..", ".") for p in PurePosixPath(name).parts) and
                    "\\" not in name for name in members), "unsafe archive member")
        require("manifest.json" in members and "REPRODUCE.md" in members,
                "missing archive description")
        manifest = json.loads(archive.read("manifest.json"))
        require(manifest.get("schema") == "aivoblock-evidence-v1" and
                manifest.get("hash_algorithm") == "sha256", "invalid manifest schema")
        if expected_run_id is not None:
            require(str(manifest.get("ci_run_id")) == str(expected_run_id),
                    "unexpected workflow provenance")
        for field in ("source_head_sha", "tested_checkout_sha"):
            value = manifest.get(field)
            require(isinstance(value, str) and len(value) == 40 and
                    set(value) <= HEX, f"invalid {field}")
        entries = manifest.get("entries")
        require(isinstance(entries, list) and entries, "missing manifest entries")
        expected = {"manifest.json", "REPRODUCE.md"}
        total_size = 0
        for entry in entries:
            require(isinstance(entry, dict), "invalid manifest entry")
            member, sha, size = (entry.get(k) for k in ("path", "sha256", "bytes"))
            require(isinstance(member, str) and member.startswith("evidence/") and
                    member not in expected, "repeated or unlisted manifest path")
            require(isinstance(sha, str) and len(sha) == 64 and set(sha) <= HEX,
                    "invalid digest format")
            require(type(size) is int and 0 <= size <= 50_000_000, "unsafe entry size")
            expected.add(member)
            require(member in members, "missing archived source")
            raw = archive.read(member)
            require(len(raw) == size and digest(raw) == sha, "archive SHA-256 mismatch")
            total_size += size
            require(total_size < 100_000_000, "unexpected archive expansion")
        require(set(members) == expected, "unexpected archive members")
        # Exact paths must exist: missing data is not a valid zero-sample experiment.
        def read_json(name: str):
            member = "evidence/" + name
            require(member in expected, f"missing evidence: {name}")
            return json.loads(archive.read(member))

        svc = read_json("testnet/generated/service-baseline/summary.json")
        canonical = svc.get("canonical_commitments")
        require(isinstance(canonical, dict), "missing CPU canonical commitments")
        for name in ("model", "input", "result"):
            sha = canonical.get(f"{name}_commitment")
            require(isinstance(sha, str) and len(sha) == 64 and set(sha) <= HEX,
                    "invalid canonical CPU commitment")
        svc_times = []
        seen_cpu = set()
        artifact_sha = {}
        for i in range(3):
            base = PREFIX + f"service-baseline/launch-{i}"
            rows = [json.loads(line) for line in archive.read(
                f"{base}.jsonl").splitlines() if line.strip()]
            require(len(rows) == 30, "CPU service launch count differs")
            artifacts = {}
            for field in ("model", "input", "result"):
                name = f"{base}-artifacts/{field}.json"
                require(name in expected, "missing CPU artifact")
                artifacts[field] = digest(archive.read(name))
                if field in artifact_sha:
                    require(artifact_sha[field] == artifacts[field],
                            "CPU artifact changed between launches")
                artifact_sha[field] = artifacts[field]
            for row in rows:
                rid = row.get("request_id")
                require(isinstance(rid, str) and rid not in seen_cpu,
                        "duplicate/missing CPU request identifier")
                seen_cpu.add(rid)
                require(row.get("launch_id") == f"service-launch-{i}" and
                        row.get("status") == "success" and
                        row.get("boundary") == "synchronous_in_process_service_execute",
                        "invalid CPU execution boundary")
                a, b = row.get("start_ns"), row.get("end_ns")
                require(type(a) is int and type(b) is int and 0 <= a < b,
                        "invalid CPU monotonic timestamps")
                require(all(row.get(f"{n}_commitment") == canonical[f"{n}_commitment"]
                            for n in ("model", "input", "result")),
                        "CPU workload commitments disagree")
                for field in ("model", "input", "result"):
                    require(row.get(f"artifact_{field}_sha256") == artifacts[field],
                            "CPU artifact bytes disagree")
                svc_times.append((b - a) / 1_000_000)
        require(len(seen_cpu) == 90 and svc.get("samples") == 90 and
                svc.get("launches") == 3, "CPU sample/launch provenance differs")
        cpu_sorted = sorted(svc_times)
        for key, value in (
            ("mean_ms", statistics.mean(svc_times)),
            ("median_ms", statistics.median(svc_times)),
            ("p95_ms", cpu_sorted[math.ceil(0.95 * 90) - 1]),
        ):
            near(svc.get(key), value, "CPU." + key)

        scenario_results = {}
        for scenario, summary_name, cadence, fixture in SCENARIOS:
            summary = read_json("testnet/generated/" + summary_name)
            method = summary.get("method", {})
            require(all((method.get(k) == v) for k, v in (
                ("launches", 3), ("observations_per_launch", 30),
                ("total_observations", 90), ("rows_per_task", 12),
                ("validators", 3), ("polling_interval_s", cadence),
                ("fixture_source", fixture), ("workload", "qmlp-v1"),
                ("clock", "time.perf_counter_ns"),
                ("observation_boundary", "before_cli_submission_to_all_validator_height_poll"),
            )), f"invalid experimental setup: {scenario}")
            details = summary.get("per_launch")
            require(isinstance(details, list) and len(details) == 3, "missing launch summaries")
            indexed = {x.get("source_file"): x for x in details}
            require(set(indexed) == {f"launch-{i:03d}.json" for i in range(3)},
                    "missing/duplicate launch provenance")
            pooled = {key: [] for key in PHASES}
            per_launch = {key: [] for key in PHASES}
            seen = set()
            for i in range(3):
                launch = f"launch-{i:03d}"
                path_rel = f"testnet/generated/{scenario}/{launch}.json"
                raw = archive.read("evidence/" + path_rel)
                rows = json.loads(raw)
                require(isinstance(rows, list) and len(rows) == 30,
                        "missing raw chain observations")
                reported = indexed[launch + ".json"]
                require(reported.get("sha256") == digest(raw) and
                        reported.get("samples") == 30, "chain provenance hash/count mismatch")
                local = {key: [] for key in PHASES}
                heights = set()
                for seq, row in enumerate(rows):
                    rid = row.get("request_id")
                    height = row.get("finalized_height")
                    require(isinstance(rid, str) and rid not in seen, "duplicate chain request")
                    seen.add(rid)
                    require(type(height) is int and height > 0 and height not in heights,
                            "duplicate/missing finalized height")
                    heights.add(height)
                    require(row.get("launch_id") == launch and row.get("workload") == "qmlp-v1"
                            and row.get("fixture_source") == fixture and row.get("rows") == 12
                            and row.get("validators") == 3 and
                            row.get("polling_interval_s") == cadence,
                            "mixed chain workload")
                    start, end = row.get("start_ns"), row.get("end_ns")
                    require(type(start) is int and type(end) is int and
                            0 <= start < end and row.get("clock") == "time.perf_counter_ns",
                            "invalid chain monotonic timestamps")
                    total = (end - start) / 1_000_000
                    near(row.get("elapsed_ms"), total, "chain elapsed")
                    phases = row.get("timing_phases")
                    require(isinstance(phases, dict) and
                            phases.get("clock") == "time.perf_counter_ns" and
                            phases.get("consensus_only_ms", "missing") is None and
                            phases.get("ai_execution_only_ms", "missing") is None and
                            phases.get("post_submission_includes") ==
                            ["worker_execution", "network_and_consensus", "status_polling"],
                            "incorrect phase attribution")
                    durations = {key: positive(phases.get(key)) for key in PHASES}
                    near(durations["total_ms"], total, "phase total")
                    near(durations["cli_submission_ms"] +
                         durations["post_submission_to_observed_finality_ms"],
                         total, "phase split")
                    for key in PHASES:
                        local[key].append(durations[key])
                        pooled[key].append(durations[key])
                    if scenario == "trained-qmlp-chain":
                        for name in ("model", "result"):
                            require(row.get(f"{name}_commitment") ==
                                    canonical[f"{name}_commitment"],
                                    "trained workload identity disagreement")
                        block_path = (f"evidence/testnet/generated/{scenario}/{launch}/"
                                      f"three-node/finalized-block-evidence/{seq:03d}.json")
                        require(row.get("finalized_block_evidence_path") ==
                                f"{launch}/three-node/finalized-block-evidence/{seq:03d}.json",
                                "missing finalized block path")
                        block = json.loads(archive.read(block_path))
                        require(all(block.get(k) == row.get(k) for k in
                                    ("task_id", "block_id")) and
                                block.get("height") == row.get("finalized_height") and
                                all(block.get(f"{name}_commitment") ==
                                    canonical[f"{name}_commitment"] for name in
                                    ("model", "input", "result")),
                                "finalized block task/result identity mismatch")
                for key in PHASES:
                    stats = check_stats(local[key], reported["timing"][key],
                                        f"{scenario}.{launch}.{key}")
                    per_launch[key].append(stats["median_ms"])
            require(len(seen) == 90, "expected 90 unique chain requests")
            for key in PHASES:
                check_stats(pooled[key], summary["pooled"][key],
                            f"{scenario}.pooled.{key}")
                check_stats(per_launch[key], summary["between_launch"]["timing"][key],
                            f"{scenario}.between_launch.{key}")
            scenario_results[scenario] = {
                "requests": len(seen), "launches": 3,
                "pooled_total_median_ms": statistics.median(pooled["total_ms"]),
                "polling_interval_s": cadence, "fixture": fixture,
            }
        return {
            "verdict": "PASS",
            "scope": "independent reanalysis of archived observations on a separate CI runner",
            "archive_sha256": digest(path.read_bytes()),
            "manifest_run_id": str(manifest["ci_run_id"]),
            "source_head_sha": manifest["source_head_sha"],
            "tested_checkout_sha": manifest["tested_checkout_sha"],
            "validated_files": len(entries),
            "cpu_service_requests": len(seen_cpu),
            "chain": scenario_results,
            "limitations": [
                "not independent execution of new AI tasks",
                "not an external researcher or independent hardware replication",
                "not isolated consensus or AI inference latency",
                "not a matched end-to-end CPU-versus-blockchain performance experiment",
            ],
        }


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--archive", required=True, type=Path)
    p.add_argument("--expected-run-id")
    p.add_argument("--out", type=Path)
    args = p.parse_args()
    report = audit(args.archive, args.expected_run_id)
    serialized = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(serialized)
    print(serialized, end="")


if __name__ == "__main__":
    main()
