"""Standard-library tests for controlled direct QMLP vs block-verifier metrics."""

import copy
import unittest

from analyze_standalone import analyze


def bench(run):
    def q(rows, partitions):
        return {
            "rows": rows,
            "partitions": partitions,
            "layers": "32 -> 64 -> 16",
            "model_commitment": "ab" * 32,
            "input_commitment": "cd" * 32,
            "standalone_qmlp_inference_us": 500.0 + rows + run,
            "full_block_verify_rust_mirror_us": 1000.0 + rows + 100 * partitions + run,
            "full_block_verify_haskell_persistent_us": 3000.0 + rows + 100 * partitions + run,
        }
    return {
        "iterations": 20,
        "qmlp_v1": q(256, 64),
        "sweeps": {
            "by_rows_8_partitions": [
                {"rows": rows, "qmlp_v1": q(rows, 8)} for rows in (16, 64, 256)
            ],
            "qmlp_32_64_16_by_partitions": [
                q(256, parts) for parts in (1, 2, 4, 8, 16, 32, 64)
            ],
        },
    }


class StandaloneComparisonTests(unittest.TestCase):
    def setUp(self):
        self.replicates = [bench(i) for i in (1, 2, 3)]

    def test_repeated_direct_vs_block_comparison(self):
        result = analyze(self.replicates)
        self.assertEqual(result["method"]["process_invocations"], 3)
        self.assertEqual(result["main_comparison"]["standalone_qmlp_inference_us"]["n"], 3)
        self.assertGreater(result["main_comparison"]["rust_verification_over_direct"]["mean"], 1)
        self.assertGreater(result["main_comparison"]["haskell_verification_over_direct"]["mean"],
                           result["main_comparison"]["rust_verification_over_direct"]["mean"])
        self.assertEqual([x["rows"] for x in result["row_sweep_one_invocation"]], [16, 64, 256])
        self.assertEqual([x["partitions"] for x in result["partition_sweep_one_invocation"]],
                         [1, 2, 4, 8, 16, 32, 64])

    def test_reject_nonpositive_or_nonfinite_times(self):
        for value in (0, -1, float("nan"), float("inf")):
            data = copy.deepcopy(self.replicates)
            data[1]["qmlp_v1"]["standalone_qmlp_inference_us"] = value
            with self.assertRaisesRegex(ValueError, "positive and finite"):
                analyze(data)

    def test_reject_mixed_workloads(self):
        data = copy.deepcopy(self.replicates)
        data[1]["qmlp_v1"]["rows"] = 64
        with self.assertRaisesRegex(ValueError, "identical QMLP"):
            analyze(data)

    def test_reject_changed_model_or_input_with_identical_dimensions(self):
        for field in ("model_commitment", "input_commitment"):
            data = copy.deepcopy(self.replicates)
            data[1]["qmlp_v1"][field] = "ff" * 32
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "identical QMLP"):
                analyze(data)

    def test_reject_invalid_commitment_encoding(self):
        data = copy.deepcopy(self.replicates)
        data[1]["qmlp_v1"]["model_commitment"] = "not-a-digest"
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            analyze(data)

    def test_reject_duplicate_or_missing_sweeps(self):
        for key, bad_index in (("by_rows_8_partitions", 0),
                               ("qmlp_32_64_16_by_partitions", 0)):
            data = copy.deepcopy(self.replicates)
            data[0]["sweeps"][key].pop(bad_index)
            with self.assertRaisesRegex(ValueError, "Missing"):
                analyze(data)

    def test_reject_fewer_than_three_invocations(self):
        with self.assertRaisesRegex(ValueError, "three independent"):
            analyze(self.replicates[:2])


if __name__ == "__main__":
    unittest.main()
