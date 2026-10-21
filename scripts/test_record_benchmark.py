import hashlib
import tempfile
import unittest
from pathlib import Path

from record_benchmark import record


class RecorderTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name, data in (("model", b"model bytes"), ("input", b"input bytes"), ("result", b"result bytes")):
            (self.root / name).write_bytes(data)

    def sample(self, rid="r1", **changes):
        row = {"request_id": rid, "launch_id": "launch-1",
               "start_ns": 100, "end_ns": 1000100, "status": "success",
               **{f"{name}_path": str(self.root / name) for name in ("model", "input", "result")}}
        row.update(changes)
        return row

    def test_hashes_real_bytes_and_duration(self):
        result = record([self.sample()], "service_direct")
        self.assertEqual(result["model_commitment"], hashlib.sha256(b"model bytes").hexdigest())
        self.assertEqual(result["samples"][0]["elapsed_ms"], 1.0)
        self.assertEqual(result["samples"][0]["result_commitment"], hashlib.sha256(b"result bytes").hexdigest())

    def test_rejects_changed_input(self):
        other = self.root / "other_input"
        other.write_bytes(b"different")
        with self.assertRaisesRegex(ValueError, "inconsistent"):
            record([self.sample(), self.sample("r2", input_path=str(other))], "service_direct")

    def test_rejects_bad_clock(self):
        with self.assertRaisesRegex(ValueError, "timestamps"):
            record([self.sample(end_ns=10)], "service_direct")

    def test_rejects_duplicate_request(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            record([self.sample(), self.sample()], "service_direct")

    def test_requires_chain_height(self):
        with self.assertRaisesRegex(ValueError, "finalized_height"):
            record([self.sample()], "chain_observed_finality")

    def test_chain_height_is_recorded(self):
        value = record([self.sample(finalized_height=7)], "chain_observed_finality")
        self.assertEqual(value["samples"][0]["finalized_height"], 7)

    def test_rejects_non_success(self):
        with self.assertRaisesRegex(ValueError, "non-success"):
            record([self.sample(status="failed")], "service_direct")

    def test_rejects_missing_file(self):
        with self.assertRaises(FileNotFoundError):
            record([self.sample(model_path=str(self.root / "missing"))], "service_direct")


if __name__ == "__main__":
    unittest.main()
