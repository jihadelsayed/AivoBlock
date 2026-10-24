"""Unit tests for link-safe standalone arXiv TeX packaging."""
import unittest
from pathlib import Path
import importlib.util

source_path = Path(__file__).with_name("package_source.py")
spec = importlib.util.spec_from_file_location("aivoblock_arxiv_package", source_path)
assert spec and spec.loader
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class UrlFormattingTests(unittest.TestCase):
    def test_wraps_plain_urls_only(self):
        source = "\\href{https://example.org/a}{article}\nhttps://example.org/file.pdf.\n"
        actual = module.wrap_plain_urls(source)
        self.assertIn("\\href{https://example.org/a}{article}", actual)
        self.assertIn("\\url{https://example.org/file.pdf}.", actual)

    def test_preserves_highlighted_shell_commands(self):
        src = "\\begin{Shaded}\n\\NormalTok{git clone https://example.org/repo.git}\n\\end{Shaded}\n"
        self.assertEqual(module.wrap_plain_urls(src), src)

    def test_real_title_block(self):
        original = (
            "\\author{}\n\\date{}\n\\begin{document}\n"
            "\\section{AivoBlock: Research Title}\\label{aivoblock-research-title}\n"
            "\\textbf{Author:} provisional credit.\n"
            "\\subsection{Abstract}\\label{abstract}\n"
            "Tested result: 730.760 ms.\n"
            "\\subsection{1. Introduction}\\label{introduction}\n"
        )
        # Synthetic miniature must include the complete number of sections to
        # test the full expected format without changing the fixed manuscript.
        section_parts = [f"\\subsection{{{i}. Section {i}}}\\label{{s{i}}}\n"
                         for i in range(2, 9)]
        subs = [f"\\subsubsection{{1.{i} Detail {i}}}\\label{{detail{i}}}\n"
                for i in range(1, 14)]
        synthetic = original + "".join(section_parts) + "".join(subs) + (
            "\\subsection{References --- checked selection}"
            "\\label{references-primary-source-checked}\n"
        )
        result = module.format_scholarly_title_block(
            synthetic, "AivoBlock: Research Title", "Jihad Elsayed",
            "University of Alabama at Birmingham",
        )
        self.assertIn("\\title{AivoBlock: Research Title}", result)
        self.assertIn("\\author{Jihad Elsayed\\\\", result)
        self.assertIn("\\begin{abstract}", result)
        self.assertIn("\\end{abstract}", result)
        self.assertIn("\\section{Introduction}", result)
        self.assertIn("\\section*{References}", result)
        self.assertEqual(result.count("730.760"), 1)
        self.assertNotIn("\\subsection{Abstract}", result)

    def test_repeatable_conversion(self):
        src = "https://example.org/reference\n"
        converted = module.wrap_plain_urls(src)
        self.assertEqual(module.wrap_plain_urls(converted), converted)


if __name__ == "__main__":
    unittest.main()
