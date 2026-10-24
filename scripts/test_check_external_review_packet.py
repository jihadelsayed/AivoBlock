"""Regression tests: a blank review tracker cannot claim completed external review."""
import json
from pathlib import Path
import tempfile
import unittest
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_external_review_packet as gate


class ExternalReviewPacketTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        original = gate.BASE
        for filename in ("README.md", "REVIEW_FORM.md", "STATUS.json", "UAB_OUTREACH.md", "EMAIL_TO_HASAN.md"):
            (self.base / filename).write_bytes((original / filename).read_bytes())

    def test_pending_packet_is_valid(self):
        self.assertTrue(gate.validate(self.base))

    def test_false_review_claim_fails_without_report(self):
        file = self.base / "STATUS.json"
        data = json.loads(file.read_text())
        data["external_review_report_received"] = True
        data["external_reviewer_contact_status"] = "response_received"
        file.write_text(json.dumps(data))
        with self.assertRaises(AssertionError):
            gate.validate(self.base)

    def test_false_cross_hardware_reproduction_fails_without_evidence(self):
        file = self.base / "STATUS.json"
        data = json.loads(file.read_text())
        data["independent_live_experiment_replication_verified"] = True
        file.write_text(json.dumps(data))
        with self.assertRaises(AssertionError):
            gate.validate(self.base)


if __name__ == "__main__":
    unittest.main()
