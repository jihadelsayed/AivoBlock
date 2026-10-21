import json
import tempfile
import unittest
from pathlib import Path

from finality_evidence_manifest import manifest


class EvidenceManifestTests(unittest.TestCase):
    def write_fixtures(self, base):
        for index in range(3):
            rows = [{"launch_id": f"launch-{index:03d}",
                     "request_id": f"launch-{index:03d}-{n}",
                     "finalized_height": n + 1,
                     "task_id": f"task-{n}", "block_id": f"block-{n}",
                     "workload": "qmlp-v1", "elapsed_ms": 1.0}
                    for n in range(30)]
            (base / f"launch-{index:03d}.json").write_text(json.dumps(rows))

    def test_good_manifest_hashes_actual_files(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            self.write_fixtures(base)
            result = manifest(base, "a" * 40, "run-123", {"python": "test"})
            self.assertEqual(result["samples_total"], 90)
            self.assertEqual(len(result["files"]), 3)
            self.assertTrue(all(len(row["sha256"]) == 64 for row in result["files"]))

    def test_missing_launch_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            self.write_fixtures(base)
            (base / "launch-002.json").unlink()
            with self.assertRaisesRegex(ValueError, "exactly 3"):
                manifest(base, "sha", "run", {})

    def test_duplicate_cross_launch_requests_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            self.write_fixtures(base)
            path = base / "launch-001.json"
            rows = json.loads(path.read_text())
            rows[0]["request_id"] = "launch-000-0"
            path.write_text(json.dumps(rows))
            with self.assertRaisesRegex(ValueError, "duplicate"):
                manifest(base, "sha", "run", {})


if __name__ == "__main__":
    unittest.main()
