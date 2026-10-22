"""ArXiv upload candidate: source-only and unpublished-by-default tests."""
import json
from pathlib import Path
import tempfile
import unittest
import zipfile

from package_arxiv_source import make_tex, create, verify

ROOT=Path(__file__).resolve().parents[1]
TEX_EXAMPLE=(
    r"\documentclass{article}"+"\n"+r"\usepackage{lmodern}"+"\n"+
    r"\author{}"+"\n"+r"\date{}"+"\n"+
    r"\begin{document}"+"\n"+
    r"\section{AivoBlock: A Reproducible Permissioned Blockchain Prototype for"+"\n"+
    r" Partitioned, Deterministically Verifiable AI Work}\label{aivoblock-test}"+"\n"+
    "Generative AI assistance\n"+
    r"\href{https://github.com/jihadelsayed/AivoBlock}{AivoBlock repo}"+"\n"+
    "https://github.com/jihadelsayed/AivoBlock/releases/download/v0.1.0-research-prototype/SHA256SUMS.txt\n"+
    r"\texttt{ade6ff42737d972658e9c309f9a2d8cffbfb988c}"+"\n"+
    "ade6ff42737d972658e9c309f9a2d8cffbfb988c\n"+
    "AivoBlock experimental verification and additional evidence. "*250+"\n"+
    r"\end{document}"+"\n"
)
METADATA=ROOT/"research/paper/submission-metadata.json"

class ArxivCandidateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.meta=json.loads(METADATA.read_text(encoding="utf-8"))

    def test_converts_heading_to_explicit_title_and_author(self):
        ready=make_tex(TEX_EXAMPLE,self.meta)
        self.assertIn(r"\maketitle",ready)
        self.assertIn(r"\title{AivoBlock:",ready)
        self.assertIn(r"\author{Sayed Jihad ElSayed}",ready)
        self.assertNotIn(r"\section{AivoBlock:",ready)
        self.assertIn("not been submitted to arXiv",ready)
        self.assertIn("Generative AI assistance",ready)
        self.assertIn(r"\usepackage{xurl}",ready)
        self.assertIn(r"\path{ade6ff42737d972658e9c309f9a2d8cffbfb988c}",ready)
        self.assertIn(r"\url{https://github.com/jihadelsayed/AivoBlock/releases/",ready)
        self.assertIn(r"\href{https://github.com/jihadelsayed/AivoBlock}{AivoBlock repo}",ready)

    def test_rejects_missing_external_dependency(self):
        bad=TEX_EXAMPLE.replace(r"\end{document}",
                                r"\input{missing-figure.tex}"+"\n"+r"\end{document}")
        with self.assertRaisesRegex(ValueError,"external TeX dependencies"):
            make_tex(bad,self.meta)

    def test_rejects_publication_claim(self):
        meta=dict(self.meta,publicly_submitted=True)
        with self.assertRaisesRegex(ValueError,"public-submission"):
            make_tex(TEX_EXAMPLE,meta)

    def test_rejects_license_approval_without_human_signoff(self):
        meta=dict(self.meta,approval=dict(self.meta["approval"],preprint_license=True))
        with self.assertRaisesRegex(ValueError,"approvals"):
            make_tex(TEX_EXAMPLE,meta)

    def test_archive_contains_only_main_tex_and_is_deterministic(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            tex=root/"paper.tex"
            tex.write_text(TEX_EXAMPLE)
            out=root/"upload.zip"
            first=create(tex,METADATA,out)
            original=out.read_bytes()
            self.assertEqual(first["verdict"],"PASS")
            self.assertEqual(verify(out)["file"],"main.tex")
            with zipfile.ZipFile(out) as z:
                self.assertEqual(z.namelist(),["main.tex"])
                self.assertNotIn(b"%PDF",z.read("main.tex"))
            create(tex,METADATA,out)
            self.assertEqual(original,out.read_bytes())

    def test_wrong_zip_members_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            out=Path(d)/"bad.zip"
            with zipfile.ZipFile(out,"w") as z:
                z.writestr("main.tex",TEX_EXAMPLE)
                z.writestr("paper.pdf",b"%PDF-1.5")
            with self.assertRaisesRegex(ValueError,"exactly main.tex"):
                verify(out)

if __name__=="__main__":
    unittest.main()
