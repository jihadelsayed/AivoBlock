"""Standard-library regression tests for the research replicate analyzer."""

import copy
import unittest

from analyze_replicates import analyze, distribution


def model(seed):
    return {
        "dataset": {"seed": seed, "training_seed": seed, "train": 2000, "test": 500,
                    "features": 32, "classes": 10},
        "float": {"epochs": 200, "layers": "32 -> 32 -> 10", "accuracy": .9 + seed / 1000},
        "qmlp": {"model_commitment": f"commitment-{seed}", "accuracy": .88 + seed / 1000,
                 "agreement_with_float": .99},
        "onnx_integer": {"exact_match": True, "mismatched_rows": 0},
    }


def bench(n):
    return {
        "iterations": 20, "task": {"rows": 256, "partitions": 64},
        "qmlp_v1": {
            "rows": 256, "partitions": 64, "layers": "32 -> 64 -> 16",
            "full_block_verify_rust_mirror_us": 3000 + n * 100,
            "full_block_verify_haskell_cross_checked_us": 19000 + n * 100,
            "full_block_verify_haskell_persistent_us": 8500 + n * 100,
        },
        "full_block_verify_rust_mirror_us": 3500 + n * 100,
        "full_block_verify_haskell_cross_checked_us": 20000 + n * 100,
        "full_block_verify_haskell_persistent_us": 9000 + n * 100,
    }


class ReplicateReportTests(unittest.TestCase):
    def setUp(self):
        self.models = [model(s) for s in (11, 17, 23)]
        self.benchmarks = [bench(n) for n in (1, 2, 3)]

    def test_stats_and_number_of_runs(self):
        result = analyze(self.models, self.benchmarks)
        self.assertEqual(result["method"]["training_seeds"], [11, 17, 23])
        self.assertEqual(result["accuracy_percent"]["float"]["n"], 3)
        self.assertGreater(result["accuracy_percent"]["float"]["sample_stddev"], 0)
        self.assertEqual(result["integer_onnx_mismatches"], 0)
        self.assertGreater(result["benchmark"]["linear_persistent_speedup"]["mean"], 2)
        self.assertAlmostEqual(result["benchmark"]["linear_rust_mirror_ms"]["median"], 3.7)

    def test_reject_same_training_seed(self):
        self.models[1]["dataset"]["training_seed"] = 11
        with self.assertRaisesRegex(ValueError, "seeds must each be distinct"):
            analyze(self.models, self.benchmarks)

    def test_reject_inexact_integer_graph(self):
        self.models[1]["onnx_integer"]["mismatched_rows"] = 1
        with self.assertRaisesRegex(ValueError, "Integer ONNX"):
            analyze(self.models, self.benchmarks)

    def test_reject_mixed_benchmark_workloads(self):
        self.benchmarks[1]["iterations"] = 10
        with self.assertRaisesRegex(ValueError, "identical"):
            analyze(self.models, self.benchmarks)

    def test_reject_nonfinite_benchmark_data(self):
        self.benchmarks[1]["qmlp_v1"]["full_block_verify_rust_mirror_us"] = float("nan")
        with self.assertRaisesRegex(ValueError, "finite observations"):
            analyze(self.models, self.benchmarks)

    def test_reject_fewer_than_three_runs(self):
        with self.assertRaisesRegex(ValueError, "at least three"):
            analyze(self.models[:2], self.benchmarks)

    def test_distribution_is_sample_based(self):
        result = distribution([1, 2, 3])
        self.assertEqual(result["mean"], 2)
        self.assertEqual(result["sample_stddev"], 1)
        self.assertEqual(result["observations"], [1, 2, 3])


if __name__ == "__main__":
    unittest.main()
