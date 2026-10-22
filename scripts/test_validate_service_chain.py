import copy
import unittest

from validate_service_chain import validate


def fixture():
    digest = "a" * 64
    scenarios = {}
    for mode in ("service_direct", "service_recomputed", "chain_observed_finality"):
        rows = []
        for launch in range(3):
            for i in range(30):
                row = {"launch_id": f"launch-{launch}", "request_id": f"{mode}-{launch}-{i}",
                       "elapsed_ms": 1.5, "status": "success",
                       "model_commitment": digest, "input_commitment": "b" * 64,
                       "result_commitment": "c" * 64,
                       "canonical_model_commitment": "d" * 64,
                       "canonical_input_commitment": "e" * 64,
                       "canonical_result_commitment": "f" * 64}
                if mode == "chain_observed_finality":
                    row["finalized_height"] = 1 + launch * 30 + i
                rows.append(row)
        scenarios[mode] = {"model_commitment": digest, "input_commitment": "b" * 64,
                           "result_commitment": "c" * 64,
                           "canonical_model_commitment": "d" * 64,
                           "canonical_input_commitment": "e" * 64,
                           "canonical_result_commitment": "f" * 64, "samples": rows}
    return {"scenarios": scenarios}


class ValidateServiceChainTests(unittest.TestCase):
    def test_accepts_three_by_thirty(self):
        self.assertEqual(validate(fixture())["status"], "PASS")

    def test_marks_timing_boundaries_as_incompatible(self):
        report = validate(fixture())
        self.assertEqual(report["timing_comparability"]["status"], "NOT_COMPARABLE")
        self.assertNotEqual(report["timing_comparability"]["service_boundary"],
                            report["timing_comparability"]["chain_boundary"])

    def test_rejects_performance_claim_from_incompatible_boundaries(self):
        b = fixture()
        b["performance_claim"] = {"speedup": 100}
        with self.assertRaisesRegex(ValueError, "performance_claim prohibited"):
            validate(b)

    def test_rejects_missing_canonical_chain_identity(self):
        b = fixture()
        del b["scenarios"]["chain_observed_finality"]["canonical_model_commitment"]
        with self.assertRaisesRegex(ValueError, "canonical"):
            validate(b)

    def test_rejects_cross_scenario_canonical_mismatch(self):
        b = fixture()
        b["scenarios"]["chain_observed_finality"]["canonical_result_commitment"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "canonical"):
            validate(b)

    def test_rejects_missing_request_canonical_identity(self):
        b = fixture()
        del b["scenarios"]["service_direct"]["samples"][2]["canonical_input_commitment"]
        with self.assertRaisesRegex(ValueError, "canonical_input_commitment"):
            validate(b)

    def test_rejects_request_canonical_mismatch(self):
        b = fixture()
        b["scenarios"]["service_recomputed"]["samples"][10]["canonical_result_commitment"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "canonical_result_commitment"):
            validate(b)

    def test_returns_both_distinct_namespaces(self):
        result = validate(fixture())
        self.assertEqual(result["commitments"]["model_commitment"], "a" * 64)
        self.assertEqual(result["canonical_commitments"]["canonical_model_commitment"], "d" * 64)

    def test_rejects_missing_commitment(self):
        b = fixture()
        del b["scenarios"]["chain_observed_finality"]["input_commitment"]
        with self.assertRaises(ValueError):
            validate(b)

    def test_rejects_different_model(self):
        b = fixture()
        b["scenarios"]["chain_observed_finality"]["model_commitment"] = "d" * 64
        with self.assertRaises(ValueError):
            validate(b)

    def test_rejects_missing_request_input_commitment(self):
        b = fixture()
        del b["scenarios"]["chain_observed_finality"]["samples"][3]["input_commitment"]
        with self.assertRaisesRegex(ValueError, "missing or mismatched input_commitment"):
            validate(b)

    def test_rejects_mismatched_request_result_commitment(self):
        b = fixture()
        b["scenarios"]["service_direct"]["samples"][17]["result_commitment"] = "d" * 64
        with self.assertRaisesRegex(ValueError, "missing or mismatched result_commitment"):
            validate(b)

    def test_rejects_too_few_samples(self):
        b = fixture()
        b["scenarios"]["chain_observed_finality"]["samples"].pop()
        with self.assertRaises(ValueError):
            validate(b)

    def test_rejects_duplicate_request(self):
        b = fixture()
        b["scenarios"]["service_direct"]["samples"][1]["request_id"] = "service_direct-0-0"
        with self.assertRaises(ValueError):
            validate(b)

    def test_rejects_reused_finalized_height(self):
        b = fixture()
        b["scenarios"]["chain_observed_finality"]["samples"][1]["finalized_height"] = 1
        with self.assertRaises(ValueError):
            validate(b)

    def test_rejects_invalid_duration(self):
        b = fixture()
        b["scenarios"]["service_recomputed"]["samples"][0]["elapsed_ms"] = float("nan")
        with self.assertRaises(ValueError):
            validate(b)

    def test_rejects_non_success(self):
        b = fixture()
        b["scenarios"]["service_direct"]["samples"][0]["status"] = "failed"
        with self.assertRaises(ValueError):
            validate(b)


if __name__ == "__main__":
    unittest.main()
