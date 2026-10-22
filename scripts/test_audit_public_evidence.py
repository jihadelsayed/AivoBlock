"""Regression tests for independent ZIP observation reanalysis."""
import hashlib
import json
import math
from pathlib import Path
import statistics
import tempfile
import unittest
import zipfile

from audit_public_evidence import audit


def digest(data):
    return hashlib.sha256(data).hexdigest()


def stats(series):
    vals = sorted(series)
    return {
        "n": len(vals),
        "min_ms": vals[0], "median_ms": statistics.median(vals),
        "p95_nearest_rank_ms": vals[math.ceil(.95 * len(vals)) - 1],
        "mean_ms": statistics.mean(vals), "max_ms": vals[-1],
        "sample_stddev_ms": statistics.stdev(vals),
    }


class IndependentAuditTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.package = Path(self.tmp.name) / "evidence.zip"
        self.blobs = {}
        self.canonical = {
            "model_commitment": "a" * 64,
            "input_commitment": "b" * 64,
            "result_commitment": "c" * 64,
        }
        self.build_data()
        self.write()

    def add(self, name, value):
        raw = (json.dumps(value, sort_keys=True) + "\n").encode()
        self.blobs["evidence/" + name] = raw

    def build_data(self):
        base = "testnet/generated/"
        cpu_durations = []
        for i in range(3):
            stem = f"{base}service-baseline/launch-{i}"
            artifacts = {kind: json.dumps({"kind":kind}).encode()
                         for kind in ("model", "input", "result")}
            for kind, contents in artifacts.items():
                self.blobs[f"evidence/{stem}-artifacts/{kind}.json"] = contents
            observations = []
            for n in range(30):
                begin, finish = 100, 100 + 900000 + n * 3000
                cpu_durations.append((finish - begin) / 1e6)
                observations.append({
                    "request_id": f"service-{i}-{n}", "launch_id": f"service-launch-{i}",
                    "status": "success", "boundary": "synchronous_in_process_service_execute",
                    "start_ns": begin, "end_ns": finish, **self.canonical,
                    **{f"artifact_{kind}_sha256": digest(contents)
                       for kind, contents in artifacts.items()},
                })
            self.blobs[f"evidence/{stem}.jsonl"] = (
                "".join(json.dumps(row) + "\n" for row in observations).encode())
        self.add(f"{base}service-baseline/summary.json", {
            "launches": 3, "samples": 90, "canonical_commitments": self.canonical,
            "mean_ms": statistics.mean(cpu_durations),
            "median_ms": statistics.median(cpu_durations),
            "p95_ms": sorted(cpu_durations)[math.ceil(.95 * 90)-1],
        })
        for scenario, summary_name, cadence, source in (
            ("pilot-measurements", "phase-summary-1s-seeded.json", 1., "seeded_node_fixture"),
            ("fast-poll-measurements", "phase-summary-100ms-seeded.json", .1, "seeded_node_fixture"),
            ("trained-qmlp-chain", "phase-summary-100ms-trained.json", .1, "trained_qmlp_export"),
        ):
            phase_names = ("cli_submission_ms", "post_submission_to_observed_finality_ms", "total_ms")
            pooled = {name: [] for name in phase_names}
            details = []
            for i in range(3):
                launch = f"launch-{i:03d}"
                rows = []
                phases_per_launch = {name: [] for name in phase_names}
                for n in range(30):
                    start = (i * 31 + n) * 10_000_000
                    cli = 250_000 + n * 3000
                    post = 3_000_000 + n * 5000
                    phases = {
                        "clock": "time.perf_counter_ns",
                        "cli_submission_ms": cli / 1e6,
                        "post_submission_to_observed_finality_ms": post / 1e6,
                        "total_ms": (cli + post) / 1e6,
                        "consensus_only_ms": None, "ai_execution_only_ms": None,
                        "post_submission_includes": [
                            "worker_execution", "network_and_consensus", "status_polling"],
                    }
                    row = {
                        "request_id": f"{launch}-{n}", "launch_id": launch,
                        "task_id": f"task-{i}-{n}", "block_id": f"block-{i}-{n}",
                        "finalized_height": n+1, "workload": "qmlp-v1", "rows": 12,
                        "validators": 3, "fixture_source": source,
                        "polling_interval_s": cadence,
                        "start_ns": start, "end_ns": start+cli+post,
                        "elapsed_ms": (cli+post)/1e6, "clock": "time.perf_counter_ns",
                        "timing_phases": phases,
                    }
                    for name in phase_names:
                        pooled[name].append(phases[name])
                        phases_per_launch[name].append(phases[name])
                    if scenario == "trained-qmlp-chain":
                        row.update({name:self.canonical[name]
                                    for name in ("model_commitment","result_commitment")})
                        relative = f"{launch}/three-node/finalized-block-evidence/{n:03d}.json"
                        row["finalized_block_evidence_path"] = relative
                        self.add(f"{base}{scenario}/{relative}", {
                            "task_id":row["task_id"], "block_id":row["block_id"],
                            "height":n+1, **self.canonical,
                        })
                    rows.append(row)
                self.add(f"{base}{scenario}/{launch}.json", rows)
                file_data = self.blobs[f"evidence/{base}{scenario}/{launch}.json"]
                details.append({
                    "source_file":f"{launch}.json",
                    "sha256":digest(file_data), "samples":30,
                    "timing":{name:stats(phases_per_launch[name]) for name in phase_names},
                })
            summary = {
                "method":{
                    "launches":3, "observations_per_launch":30,
                    "total_observations":90, "rows_per_task":12,
                    "validators":3, "polling_interval_s":cadence,
                    "fixture_source":source, "workload":"qmlp-v1",
                    "clock":"time.perf_counter_ns",
                    "observation_boundary":"before_cli_submission_to_all_validator_height_poll",
                },
                "per_launch":details,
                "pooled":{name:stats(pooled[name]) for name in phase_names},
                "between_launch":{"timing":{
                    name:stats([entry["timing"][name]["median_ms"] for entry in details])
                    for name in phase_names}},
            }
            self.add(f"{base}{summary_name}", summary)

    def write(self):
        manifest = {
            "schema": "aivoblock-evidence-v1", "hash_algorithm": "sha256",
            "source_head_sha":"d"*40, "tested_checkout_sha":"e"*40,
            "ci_run_id":"12345", "entries":[
                {"path":path, "bytes":len(raw), "sha256":digest(raw)}
                for path, raw in sorted(self.blobs.items())
            ],
        }
        with zipfile.ZipFile(self.package,"w",zipfile.ZIP_DEFLATED) as z:
            z.writestr("manifest.json",json.dumps(manifest))
            z.writestr("REPRODUCE.md","Research test fixture")
            for path, raw in self.blobs.items():
                z.writestr(path,raw)

    def edit(self, name, callback):
        obj = json.loads(self.blobs["evidence/"+name])
        callback(obj)
        self.add(name,obj)
        self.write()

    def test_recompute_on_fresh_zip(self):
        report = audit(self.package, expected_run_id="12345")
        self.assertEqual(report["verdict"],"PASS")
        self.assertEqual(report["cpu_service_requests"],90)
        self.assertEqual({x["requests"] for x in report["chain"].values()},{90})
        self.assertEqual(len(report["chain"]),3)
        self.assertGreater(report["chain"]["trained-qmlp-chain"]["pooled_total_median_ms"],3)

    def test_wrong_run_id_rejected(self):
        with self.assertRaisesRegex(ValueError,"workflow provenance"):
            audit(self.package,expected_run_id="666")

    def test_recomputed_stat_catches_self_consistent_manifest_tampering(self):
        self.edit("testnet/generated/phase-summary-100ms-trained.json",
                  lambda r: r["pooled"]["total_ms"].update(mean_ms=900))
        with self.assertRaisesRegex(ValueError,"recomputed metric"):
            audit(self.package)

    def test_phase_misattribution_rejected_after_digest_update(self):
        path="testnet/generated/pilot-measurements/launch-000.json"
        self.edit(path,lambda r: r[0]["timing_phases"].update(consensus_only_ms=.5))
        self.edit("testnet/generated/phase-summary-1s-seeded.json",
                  lambda r: r["per_launch"][0].update(
                      sha256=digest(self.blobs["evidence/"+path])))
        with self.assertRaisesRegex(ValueError,"phase attribution"):
            audit(self.package)

    def test_trained_block_identity_rejected(self):
        path=("testnet/generated/trained-qmlp-chain/launch-000/"
              "three-node/finalized-block-evidence/001.json")
        self.edit(path,lambda r:r.update(block_id="unrelated"))
        with self.assertRaisesRegex(ValueError,"finalized block"):
            audit(self.package)

    def test_cpu_artifact_rejected(self):
        path="evidence/testnet/generated/service-baseline/launch-0-artifacts/result.json"
        self.blobs[path]=b"changed"
        self.write()
        with self.assertRaisesRegex(ValueError,"CPU artifact"):
            audit(self.package)

    def test_zip_rejects_unlisted_content(self):
        self.write()
        with zipfile.ZipFile(self.package,"a") as z:
            z.writestr("extra.txt","not accounted for")
        with self.assertRaisesRegex(ValueError,"unexpected archive members"):
            audit(self.package)


if __name__=="__main__":
    unittest.main()
