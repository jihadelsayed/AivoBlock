#!/usr/bin/env python3
"""Preflight ACMSE 2027 poster draft. Not a substitute for venue template/review."""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
IDENTIFIERS = ("Sayed Jihad", "Sayed Jihad ElSayed", "Jihad Elsayed", "Jihad ElSayed", "jihadelsayed", "University of Alabama", "uab.edu", "Neetechs LLC")
REQUIRED_VALUES = ("270", "730.760", "779.342", "0.742", "3.412", "8.282", "1.880", "0.566", "3.404", "18.757", "94.8")


def uncomment(source: str) -> str:
    return "\n".join(re.split(r"(?<!\\)%", line, maxsplit=1)[0] for line in source.splitlines())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-only", action="store_true", help="Skip PDF checks (for stdlib-only CI)")
    parser.add_argument("--pdf", type=Path, default=HERE / "poster-abstract.pdf")
    parser.add_argument("--repo-root", type=Path, default=REPO, help="Repo root (for local testing)")
    args = parser.parse_args()
    errors: list[str] = []
    warnings: list[str] = []
    tex = (HERE / "poster-abstract.tex").read_text(encoding="utf-8")
    bib = (HERE / "references.bib").read_text(encoding="utf-8")
    visible = uncomment(tex)

    def require(ok: bool, message: str) -> None:
        if not ok:
            errors.append(message)

    documentclasses = re.findall(r"\\documentclass(?:\[([^]]+)\])?\{acmart\}", visible)
    require(len(documentclasses) == 1 and {"sigconf", "anonymous"}.issubset(set(documentclasses[0].split(","))),
            "Document must use acmart with sigconf,anonymous review options")
    for tag in (r"\begin{abstract}", r"\end{abstract}", r"\ccsdesc", r"\keywords", r"\maketitle", r"\bibliography{references}"):
        require(tag in visible, f"Missing required TeX structure: {tag}")
    title = re.search(r"\\title\{([^}]+)\}", visible)
    require(bool(title and title.group(1).startswith("AivoBlock")), "Missing manuscript title")
    for cmd in (r"\author", r"\affiliation", r"\email", r"\orcid", r"\begin{acks}"):
        require(not re.search(re.escape(cmd) + r"\b|" + re.escape(cmd) + r"\s*\{", visible),
                f"Identifying review command present: {cmd}")
    for token in IDENTIFIERS:
        require(token.lower() not in visible.lower(), f"Potential author/affiliation identifier in review text: {token}")

    abstract = re.search(r"\\begin\{abstract\}(.*?)\\end\{abstract\}", visible, re.S)
    require(abstract is not None, "No abstract environment")
    if abstract:
        require(not re.search(r"\\cite|\\begin\{(?:figure|table|itemize|enumerate|align|equation)\}|\$", abstract.group(1)),
                "Abstract contains a prohibited citation, figure, table, list, or equation")
    keywords = re.search(r"\\keywords\{([^}]+)\}", visible)
    if keywords:
        words = [w.strip() for w in keywords.group(1).split(",")]
        require(len(words) >= 3 and all(w[:1].isupper() for w in words),
                "Expected comma-separated Title Case keywords")

    citations = [k.strip() for group in re.findall(r"\\cite(?:\[[^]]*\])*\{([^}]+)\}", visible) for k in group.split(",")]
    entries = set(re.findall(r"@\w+\s*\{\s*([^,\s]+)\s*,", bib))
    require(len(entries) >= 4, "At least four bibliography entries required for current draft")
    for key in set(citations):
        require(key in entries, f"Missing BibTeX entry for cited key {key}")
    require("Lihu, Andrei" in bib and "Du, Jincheng" in bib and "Barjaktarevic, Igor" in bib,
            "Lihu et al. bibliographic given names need correction")
    require("Wong, Wan Ki" in bib and "10.1007/978-3-031-15979-4_12" in bib,
            "Wong author or Ofelimos DOI missing from bibliography")

    canonical = (args.repo_root / "research/paper/aivoblock-manuscript.md").read_text(encoding="utf-8")
    for value in REQUIRED_VALUES:
        require(value in tex and value in canonical,
                f"Published-evidence number {value} absent from poster or canonical manuscript")
    for warning in ("permissionless", "fully recompute", "synthetic", "different boundaries"):
        require(warning.lower() in visible.lower(), f"Critical scope qualifier absent: {warning}")

    if not args.source_only:
        pdf = args.pdf
        require(pdf.is_file(), f"PDF missing: {pdf}")
        if pdf.is_file():
            for executable in ("pdfinfo", "pdftotext"):
                require(bool(shutil.which(executable)), f"{executable} not installed")
            if shutil.which("pdfinfo") and shutil.which("pdftotext"):
                info = subprocess.run(["pdfinfo", str(pdf)], capture_output=True, text=True, check=False)
                data = dict((k.strip(), v.strip()) for k, v in (line.split(":", 1) for line in info.stdout.splitlines() if ":" in line))
                require(info.returncode == 0, "pdfinfo unable to parse PDF")
                require(data.get("Pages") == "2", f"Expected exactly two pages; observed {data.get('Pages')}")
                require("612 x 792" in data.get("Page size", ""), "PDF must be US letter (612 x 792 pt)")
                require(data.get("Author", "").lower() in ("", "anonymous", "anon."), "Identifying PDF Author metadata")
                body = subprocess.run(["pdftotext", str(pdf), "-"], capture_output=True, text=True, check=False)
                require(body.returncode == 0, "pdftotext failed")
                for token in IDENTIFIERS:
                    require(token.lower() not in body.stdout.lower(), f"PDF visible identity: {token}")
                require("References" in body.stdout and "AivoBlock" in body.stdout,
                        "PDF does not include expected paper content and references")
                require("Andrei Lihu" in body.stdout and "Jincheng Du" in body.stdout,
                        "PDF bibliography is stale or has incorrect names; rebuild with corrected BibTeX")
    if not args.source_only:
        warnings.append("Manual review of the generated PDF against sample-sigconf.tex is still required")
    print(f"ACMSE poster preflight: {len(errors)} error(s), {len(warnings)} warning(s)")
    for warning in warnings:
        print(f"WARN: {warning}")
    for error in errors:
        print(f"FAIL: {error}")
    if not errors:
        print("PASS: automated checks only; venue-chair, rights and final author review remain mandatory")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
