"""Fail-closed regression tests for author-owned, unpublished preprint package."""
import json
import shutil
import tempfile
from pathlib import Path
import unittest
from check_preprint_package import audit

ROOT=Path(__file__).resolve().parents[1]
PAPER="research/paper/aivoblock-manuscript.md"
META="research/paper/submission-metadata.json"
GUIDE="research/paper/SUBMISSION.md"

class SubmissionGateTests(unittest.TestCase):
    def setUp(self):
        tmp=tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root=Path(tmp.name)
        for name in (PAPER,META,GUIDE):
            (self.root/name).parent.mkdir(parents=True,exist_ok=True)
            shutil.copyfile(ROOT/name,self.root/name)

    def mutate_metadata(self,change):
        path=self.root/META
        x=json.loads(path.read_text())
        change(x)
        path.write_text(json.dumps(x,indent=2))

    def test_unsubmitted_package_has_fixed_provenance(self):
        x=audit(self.root)
        self.assertEqual(x["status"],"PASS")
        self.assertEqual(x["approval_count"],0)
        self.assertFalse(x["publicly_submitted"])
        self.assertEqual(x["required_author_approval_count"],10)

    def test_rejects_falsely_approved_submission(self):
        self.mutate_metadata(lambda x: x.update(publicly_submitted=True))
        with self.assertRaisesRegex(ValueError,"submission or DOI"):
            audit(self.root)

    def test_rejects_invented_doi(self):
        self.mutate_metadata(lambda x: x.update(preprint_doi="10.1234/invented"))
        with self.assertRaisesRegex(ValueError,"submission or DOI"):
            audit(self.root)

    def test_rejects_unauthorized_license(self):
        self.mutate_metadata(lambda x: x["approval"].update(preprint_license=True))
        with self.assertRaisesRegex(ValueError,"submission must stay gated"):
            audit(self.root)

    def test_rejects_unauthorized_creator(self):
        self.mutate_metadata(lambda x: x["creators"][0].update(confirmed_by_author=True))
        with self.assertRaisesRegex(ValueError,"author identity"):
            audit(self.root)

    def test_rejects_missing_ai_assistance_disclosure(self):
        path=self.root/PAPER
        path.write_text(path.read_text().replace("OpenAI ChatGPT (GPT-6)",
                                                 "undisclosed technology"))
        with self.assertRaisesRegex(ValueError,"missing preprint disclosure"):
            audit(self.root)

    def test_rejects_changed_experimental_commit(self):
        self.mutate_metadata(lambda x: x.update(evidence_commit="0"*40))
        with self.assertRaisesRegex(ValueError,"immutable experimental"):
            audit(self.root)

    def test_rejects_missing_author_approval_gate(self):
        self.mutate_metadata(lambda x: x["approval"].pop("explicit_publish_authorization"))
        with self.assertRaisesRegex(ValueError,"submission must stay gated"):
            audit(self.root)

    def test_rejects_unreadable_or_missing_guide(self):
        (self.root/GUIDE).unlink()
        with self.assertRaises(FileNotFoundError):
            audit(self.root)

if __name__=="__main__":
    unittest.main()
