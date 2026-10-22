"""Release disclosure and licensing gate regression tests."""
from pathlib import Path
import shutil
import tempfile
import unittest

from check_release_readiness import audit


ROOT = Path(__file__).resolve().parents[1]
PATHS = (
    "LICENSE", "README.md", "docs/testnet.md",
    "docs/development.md", "docs/research-reproducibility-package.md",
    "docs/releases/v0.1.0-research-prototype.md",
)


class ResearchReleaseGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for relative in PATHS:
            dest = self.root / relative
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, dest)

    def change(self, relative, transform):
        path = self.root / relative
        path.write_text(transform(path.read_text()))

    def test_release_disclosures_are_present(self):
        result = audit(self.root)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["tag"], "v0.1.0-research-prototype")

    def test_fake_mit_grant_fails(self):
        self.change("LICENSE", lambda s: s + "\nMIT\n")
        with self.assertRaisesRegex(ValueError, "source rights"):
            audit(self.root)

    def test_readme_licensing_disagreement_fails(self):
        self.change("README.md", lambda s: s.replace(
            "**Source-available research prototype; all rights reserved",
            "MIT License"))
        with self.assertRaisesRegex(ValueError, "README"):
            audit(self.root)

    def test_reintroduced_plaintext_claim_fails(self):
        self.change("docs/testnet.md", lambda s: s + "\ntransport is unencrypted")
        with self.assertRaisesRegex(ValueError, "testnet transport"):
            audit(self.root)

    def test_reintroduced_no_network_claim_fails(self):
        self.change("docs/development.md", lambda s:
                    s.replace("Working permissioned private Docker testnet",
                              "do not yet form a running decentralized network"))
        with self.assertRaisesRegex(ValueError, "development guide"):
            audit(self.root)

    def test_missing_independent_reproduction_caveat_fails(self):
        self.change("docs/releases/v0.1.0-research-prototype.md",
                    lambda s: s.replace("not an independent rerun",
                                        "a proven independent rerun"))
        with self.assertRaisesRegex(ValueError, "release notes"):
            audit(self.root)

    def test_missing_release_asset_fails(self):
        self.change("docs/releases/v0.1.0-research-prototype.md",
                    lambda s: s.replace("independent-audit.json",
                                        "unstated-evidence.json"))
        with self.assertRaisesRegex(ValueError, "release asset"):
            audit(self.root)

    def test_missing_file_fails(self):
        (self.root / "LICENSE").unlink()
        with self.assertRaisesRegex(ValueError, "missing release input"):
            audit(self.root)


if __name__ == "__main__":
    unittest.main()
