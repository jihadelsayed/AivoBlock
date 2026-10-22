"""Regression tests for evidence-only QMLP client timing summaries."""
import copy
import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from summarize_chain_phases import summarize


def sample(launch, index):
    start = 1_000_000 + index * 10_000_000
    submitted = start + 1_000_000 + index * 1_000
    observed = submitted + 3_000_000 + index * 2_000
    cli_ms = (submitted - start) / 1_000_000
    poll_ms = (observed - submitted) / 1_000_000
    total_ms = (observed - start) / 1_000_000
    return {
        "launch_id": launch, "request_id": f"{launch}-{index}",
        "task_id": f"{launch}-task-{index}", "block_id": f"{launch}-block-{index}",
        "finalized_height": index + 1, "workload": "qmlp-v1",
        "fixture_source": "trained_qmlp_export", "rows": 12, "validators": 3,
        "model_commitment": "a" * 64, "result_commitment": "b" * 64,
        "polling_interval_s": 0.1,
        "observation_boundary": "before_cli_submission_to_all_validator_height_poll",
        "start_ns": start, "end_ns": observed, "elapsed_ms": total_ms,
        "clock": "time.perf_counter_ns",
        "timing_phases": {
            "clock": "time.perf_counter_ns",
            "cli_submission_ms": cli_ms,
            "post_submission_to_observed_finality_ms": poll_ms,
            "total_ms": total_ms,
            "post_submission_includes": [
                "worker_execution", "network_and_consensus", "status_polling"],
            "ai_execution_only_ms": None, "consensus_only_ms": None,
        },
    }


class PhaseSummaryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for launch_index in range(3):
            launch = f"launch-{launch_index:03d}"
            (self.root / f"{launch}.json").write_text(
                json.dumps([sample(launch, n) for n in range(30)]))

    def mutate(self, fn, launch="launch-001"):
        path = self.root / f"{launch}.json"
        observations = json.loads(path.read_text())
        fn(observations)
        path.write_text(json.dumps(observations))

    def test_three_launch_summary_retains_provenance(self):
        report = summarize(self.root)
        self.assertEqual(report["method"]["total_observations"], 90)
        self.assertEqual(report["method"]["fixture_source"], "trained_qmlp_export")
        self.assertEqual(report["pooled"]["total_ms"]["n"], 90)
        self.assertEqual(report["between_launch"]["timing"]["cli_submission_ms"]["n"], 3)
        self.assertGreater(report["pooled"]["total_ms"]["p95_nearest_rank_ms"], 4.0)
        for launch in report["per_launch"]:
            path = self.root / launch["source_file"]
            self.assertEqual(launch["sha256"], hashlib.sha256(path.read_bytes()).hexdigest())
        self.assertIn("consensus-only time", report["interpretation"]["not_measured"])
        self.assertIn("service-vs-chain speedup or overhead", report["interpretation"]["not_supported"])

    def test_reject_missing_launch(self):
        (self.root / "launch-002.json").unlink()
        with self.assertRaisesRegex(ValueError, "exactly three"):
            summarize(self.root)

    def test_reject_duplicate_request_id(self):
        self.mutate(lambda rows: rows[1].update(request_id=rows[0]["request_id"]))
        with self.assertRaisesRegex(ValueError, "duplicate request"):
            summarize(self.root)

    def test_reject_mixed_cadence(self):
        self.mutate(lambda rows: rows[0].update(polling_interval_s=1.0))
        with self.assertRaisesRegex(ValueError, "mixed workload"):
            summarize(self.root)

    def test_reject_legacy_record_without_phase_split(self):
        self.mutate(lambda rows: rows[0].pop("timing_phases"))
        with self.assertRaisesRegex(ValueError, "missing timing phases"):
            summarize(self.root)

    def test_reject_inconsistent_phase_sum(self):
        self.mutate(lambda rows: rows[0]["timing_phases"].update(cli_submission_ms=9.0))
        with self.assertRaisesRegex(ValueError, "phase duration"):
            summarize(self.root)

    def test_reject_wrong_clock_and_elapsed_tampering(self):
        self.mutate(lambda rows: rows[0].update(clock="time.time"))
        with self.assertRaisesRegex(ValueError, "monotonic"):
            summarize(self.root)
        self.mutate(lambda rows: rows[0].update(clock="time.perf_counter_ns", elapsed_ms=99))
        with self.assertRaisesRegex(ValueError, "elapsed duration"):
            summarize(self.root)

    def test_reject_misattributed_consensus_time(self):
        self.mutate(lambda rows: rows[0]["timing_phases"].update(consensus_only_ms=0.4))
        with self.assertRaisesRegex(ValueError, "unsupported AI-only"):
            summarize(self.root)

    def test_reject_changed_trained_model(self):
        self.mutate(lambda rows: rows[0].update(model_commitment="c" * 64))
        with self.assertRaisesRegex(ValueError, "mixed trained-model"):
            summarize(self.root)

    def test_reject_nonfinite_or_boolean_numbers(self):
        self.mutate(lambda rows: rows[0]["timing_phases"].update(total_ms=float("nan")))
        with self.assertRaisesRegex(ValueError, "invalid total_ms"):
            summarize(self.root)
        self.mutate(lambda rows: rows[0]["timing_phases"].update(total_ms=True))
        with self.assertRaisesRegex(ValueError, "invalid total_ms"):
            summarize(self.root)

    def test_seeded_fixture_has_no_trained_provenance(self):
        for path in sorted(self.root.glob("launch-???.json")):
            rows = json.loads(path.read_text())
            for row in rows:
                row["fixture_source"] = "seeded_node_fixture"
                del row["model_commitment"]
                del row["result_commitment"]
            path.write_text(json.dumps(rows))
        report = summarize(self.root)
        self.assertIsNone(report["method"]["trained_qmlp_commitments"])
        self.assertEqual(report["method"]["fixture_source"], "seeded_node_fixture")


if __name__ == "__main__":
    unittest.main()
