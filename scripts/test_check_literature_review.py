"""Regression tests for the scope and bibliography gate in the reviewed paper."""
import tempfile
from pathlib import Path
import unittest

from check_literature_review import audit


ROOT = Path(__file__).resolve().parents[1]


class LiteratureReviewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.paper = (ROOT / "research/paper/aivoblock-manuscript.md").read_text()
        cls.review = (ROOT / "research/paper/related-work-review.md").read_text()

    def test_current_research_text_has_consistent_sources(self):
        report = audit(self.paper, self.review)
        self.assertEqual(report["verdict"], "PASS")
        self.assertEqual(report["checked_primary_records"], 9)
        self.assertEqual(report["review_type"], "targeted, non-systematic")

    def test_detects_forged_coai_publication_metadata(self):
        bad = self.paper.replace("10.3390/e21080723", "10.3390/e21080724")
        self.assertNotEqual(bad, self.paper)
        with self.assertRaisesRegex(ValueError, r"reference \[3\]"):
            audit(bad, self.review)

    def test_detects_uncited_recent_work(self):
        bad = self.paper.replace("2026 [12].", "2026.") if False else self.paper
        # The source still exists in the bibliography, but no longer appears
        # in body discussion.
        head, references = bad.split("## References", 1)
        head = head.replace("[12]", "(Wong et al., 2026)")
        with self.assertRaisesRegex(ValueError, r"not cited"):
            audit(head + "## References" + references, self.review)

    def test_prohibits_false_claims_of_first_ai_mining(self):
        bad = self.paper + "\nAivoBlock invented AI mining.\n"
        with self.assertRaisesRegex(ValueError, "unsupported priority"):
            audit(bad, self.review)

    def test_prohibits_missing_originality_boundary(self):
        bad = self.paper.replace("not a systematic literature review", "a full systematic literature review")
        with self.assertRaisesRegex(ValueError, "omitted limitation"):
            audit(bad, self.review)

    def test_rejects_missing_recent_contrast(self):
        bad = self.review.replace("Wong", "(name omitted)")
        with self.assertRaisesRegex(ValueError, "missing comparison"):
            audit(self.paper, bad)

    def test_does_not_relabel_working_paper_as_peer_reviewed(self):
        bad = self.paper.replace(
            "Exploratory working paper", "Peer-reviewed journal article")
        # Must retain clear publication type in the numbered citation.
        with self.assertRaisesRegex(ValueError, r"reference \[11\]"):
            audit(bad, self.review)


if __name__ == "__main__":
    unittest.main()
