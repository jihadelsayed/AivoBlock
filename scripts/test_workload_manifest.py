"""Regression tests for the fixed-workload benchmark manifest."""
import copy
import unittest
from workload_manifest import make_manifest


def sample():
    return {"iterations": 20, "qmlp_v1": {
        "rows": 256, "partitions": 64, "layers": "32 -> 64 -> 16",
        "model_commitment": "ab" * 32, "input_commitment": "cd" * 32,
    }}


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.data = [sample() for _ in range(3)]
        self.kwargs = {"runner": "github-actions", "revision": "a" * 40,
                       "machine": {"system": "Linux", "machine": "x86_64",
                                   "processor": "x86_64", "python": "3.12"}}

    def test_manifest(self):
        result = make_manifest(self.data, **self.kwargs)
        self.assertEqual(result["schema_version"], 1)
        self.assertEqual(result["replicates"], 3)
        self.assertEqual(result["workload"]["rows"], 256)

    def test_different_input_rejected(self):
        data = copy.deepcopy(self.data)
        data[1]["qmlp_v1"]["input_commitment"] = "ef" * 32
        with self.assertRaisesRegex(ValueError, "Mismatched"):
            make_manifest(data, **self.kwargs)

    def test_different_model_rejected(self):
        data = copy.deepcopy(self.data)
        data[2]["qmlp_v1"]["model_commitment"] = "ef" * 32
        with self.assertRaisesRegex(ValueError, "Mismatched"):
            make_manifest(data, **self.kwargs)

    def test_invalid_digest_rejected(self):
        data = copy.deepcopy(self.data)
        data[0]["qmlp_v1"]["input_commitment"] = "xyz"
        with self.assertRaisesRegex(ValueError, "Invalid input"):
            make_manifest(data, **self.kwargs)

    def test_protocol_limits_rejected(self):
        data = copy.deepcopy(self.data)
        for b in data:
            b["qmlp_v1"]["rows"] = 257
        with self.assertRaisesRegex(ValueError, "V3 task limits"):
            make_manifest(data, **self.kwargs)

    def test_invalid_model_dimensions_rejected(self):
        data = copy.deepcopy(self.data)
        for b in data:
            b["qmlp_v1"]["layers"] = "64 -> 64 -> 16"
        with self.assertRaisesRegex(ValueError, "layer dimensions"):
            make_manifest(data, **self.kwargs)

    def test_hidden_and_output_protocol_limits_rejected(self):
        for shape in ("32 -> 65 -> 16", "32 -> 64 -> 17"):
            data = copy.deepcopy(self.data)
            for b in data:
                b["qmlp_v1"]["layers"] = shape
            with self.subTest(shape=shape), self.assertRaisesRegex(ValueError, "layer dimensions"):
                make_manifest(data, **self.kwargs)

    def test_incorrect_operation_count_rejected(self):
        data = copy.deepcopy(self.data)
        for b in data:
            b["qmlp_v1"]["integer_multiply_adds_per_row"] = 1
        with self.assertRaisesRegex(ValueError, "operation count"):
            make_manifest(data, **self.kwargs)

    def test_correct_operation_count_accepted(self):
        data = copy.deepcopy(self.data)
        for b in data:
            b["qmlp_v1"]["integer_multiply_adds_per_row"] = 3072
        self.assertEqual(make_manifest(data, **self.kwargs)["replicates"], 3)

    def test_missing_runner_rejected(self):
        with self.assertRaisesRegex(ValueError, "Runner identity"):
            make_manifest(self.data, **{**self.kwargs, "runner": ""})

    def test_insufficient_repetitions_rejected(self):
        with self.assertRaisesRegex(ValueError, "three separate"):
            make_manifest(self.data[:2], **self.kwargs)


if __name__ == "__main__":
    unittest.main()
