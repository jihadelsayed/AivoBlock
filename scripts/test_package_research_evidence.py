"""Pure-stdlib checks for the public allowlisted research evidence archive."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

from package_research_evidence import create, file_list, verify, SCENARIOS


def sha(data):
    return hashlib.sha256(data).hexdigest()


class EvidenceBundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        self.output = Path(self.temp.name) / "bundle.zip"
        for path in file_list():
            dest = self.root / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_text("{}\n")
        for idx in range(3):
            base = f"testnet/generated/service-baseline/launch-{idx}"
            hashes = {}
            for name in ("model", "input", "result"):
                hashes[name] = sha((self.root / f"{base}-artifacts/{name}.json").read_bytes())
            requests = [
                {"request_id": f"service-{idx}-{n}",
                 **{f"artifact_{name}_sha256": hashes[name] for name in hashes}}
                for n in range(30)
            ]
            (self.root / f"{base}.jsonl").write_text(
                "".join(json.dumps(row) + "\n" for row in requests))
        for scenario, summary in (
            ("pilot-measurements", "phase-summary-1s-seeded.json"),
            ("fast-poll-measurements", "phase-summary-100ms-seeded.json"),
            ("trained-qmlp-chain", "phase-summary-100ms-trained.json"),
        ):
            sources = []
            for idx in range(3):
                launch = f"launch-{idx:03d}"
                prefix = f"testnet/generated/{scenario}/{launch}"
                rows = []
                for n in range(30):
                    row = {"request_id": f"{launch}-{n}",
                           "task_id": f"task-{idx}-{n}",
                           "block_id": f"block-{idx}-{n}"}
                    if scenario == "trained-qmlp-chain":
                        ref = (f"{launch}/three-node/finalized-block-evidence/"
                               f"{n:03d}.json")
                        row["finalized_block_evidence_path"] = ref
                        (self.root / f"testnet/generated/{scenario}/{ref}").write_text(
                            json.dumps({"task_id": row["task_id"],
                                        "block_id": row["block_id"]}))
                    rows.append(row)
                data = (json.dumps(rows, sort_keys=True) + "\n").encode()
                (self.root / f"{prefix}.json").write_bytes(data)
                sources.append({"source_file": launch + ".json",
                                "samples": 30, "sha256": sha(data)})
            (self.root / f"testnet/generated/{summary}").write_text(
                json.dumps({"method": {"total_observations": 90},
                            "per_launch": sources}))
    def make(self):
        return create(self.root, self.output, "a" * 40, "b" * 40, "39730631293")

    def test_complete_bundle_and_integrity_verification(self):
        result = self.make()
        verified = verify(self.output)
        self.assertEqual(result, verified)
        self.assertEqual(len(verified["entries"]), len(file_list()))
        self.assertEqual(verified["source_head_sha"], "a" * 40)
        with zipfile.ZipFile(self.output) as archive:
            self.assertEqual(archive.read("REPRODUCE.md").count(b"consensus-only"), 1)
            self.assertNotIn("private_keys", "\n".join(archive.namelist()))
            self.assertTrue(any("finalized-block-evidence/000.json" in x
                                for x in archive.namelist()))

    def test_deterministic_bytes(self):
        self.make()
        first = self.output.read_bytes()
        self.make()
        self.assertEqual(first, self.output.read_bytes())

    def test_missing_evidence_rejected(self):
        (self.root / "testnet/generated/trained-qmlp-chain/"
         "launch-000/three-node/finalized-block-evidence/001.json").unlink()
        with self.assertRaisesRegex(ValueError, "missing"):
            self.make()

    def test_mismatched_phase_summary_hash_rejected(self):
        path = self.root / "testnet/generated/pilot-measurements/launch-000.json"
        path.write_text("[]")
        with self.assertRaisesRegex(ValueError, "source-file digest"):
            self.make()

    def test_mismatched_finalized_block_rejected(self):
        path = (self.root / "testnet/generated/trained-qmlp-chain/launch-000/"
                "three-node/finalized-block-evidence/001.json")
        path.write_text('{"task_id":"wrong","block_id":"wrong"}')
        with self.assertRaisesRegex(ValueError, "finalized block identity"):
            self.make()

    def test_tampered_service_artifact_rejected(self):
        (self.root / "testnet/generated/service-baseline/"
         "launch-1-artifacts/result.json").write_text('{"tampered":true}')
        with self.assertRaisesRegex(ValueError, "artifact digest"):
            self.make()

    def test_symlink_avoidance(self):
        target = self.root / "research-out/report.json"
        target.unlink()
        target.symlink_to(self.root / "research-out/digits.qmlp.json")
        with self.assertRaisesRegex(ValueError, "linked"):
            self.make()

    def test_tampered_zip_rejected(self):
        self.make()
        with zipfile.ZipFile(self.output, "a") as archive:
            archive.writestr("unexpected.txt", "injected")
        with self.assertRaisesRegex(ValueError, "unlisted"):
            verify(self.output)

    def test_invalid_source_revision_rejected(self):
        with self.assertRaisesRegex(ValueError, "40-character"):
            create(self.root, self.output, "main", "b" * 40, "123")


if __name__ == "__main__":
    unittest.main()
