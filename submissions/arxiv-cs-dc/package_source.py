#!/usr/bin/env python3
"""Package the existing AivoBlock preprint LaTeX for *author review* on arXiv.

The output is NOT an arXiv submission, endorsement, or authorization to publish.
Requires scripts/build_preprint.sh to have generated the standalone TeX.
"""
from __future__ import annotations

import argparse
from hashlib import sha256
import json
from pathlib import Path
import re
import zipfile


def require(condition: bool, explanation: str) -> None:
    if not condition:
        raise ValueError(explanation)


def wrap_plain_urls(latex: str) -> str:
    """Make displayed HTTP(S) URLs line-breakable without altering code or hyperlinks."""
    lines = []
    in_code = False
    for line in latex.splitlines(keepends=True):
        if r"\begin{Shaded}" in line or r"\begin{Highlighting}" in line:
            in_code = True
        if not in_code and r"\href{" not in line and r"\url{" not in line and not line.lstrip().startswith("%"):
            def replace_url(match: re.Match[str]) -> str:
                raw = match.group(0)
                address = raw.rstrip(".,;")
                return r"\url{" + address + "}" + raw[len(address):]
            line = re.sub(r"https?://[^\s\\{}]+", replace_url, line)
        lines.append(line)
        if r"\end{Shaded}" in line:
            in_code = False
    return "".join(lines)


def package(repo: Path) -> dict:
    repo = repo.resolve()
    base = repo / "research/paper/build"
    tex_path = base / "aivoblock-preprint.tex"
    meta_path = repo / "research/paper/submission-metadata.json"
    require(tex_path.is_file(), "First run: bash scripts/build_preprint.sh")
    require(meta_path.is_file(), "Missing canonical preprint metadata")
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    require(meta.get("publicly_submitted") is False and meta.get("arxiv_id") is None,
            "This build must not assert arXiv publication")
    approvals = meta.get("approval", {})
    require(isinstance(approvals, dict) and approvals and all(v is False for v in approvals.values()),
            "Author rights and content approvals must remain pending in this draft workflow")
    tex = wrap_plain_urls(tex_path.read_text(encoding="utf-8"))
    for tag in (r"\documentclass", r"\begin{document}", r"\end{document}",
                "AivoBlock", "730.760", "270", "References", "Generative AI assistance"):
        require(tag.lower() in tex.lower(), f"Generated TeX missing expected marker: {tag}")
    require("Sayed Jihad ElSayed" in tex, "Public preprint must preserve proposed author credit")
    require(not re.search(r"\\(?:includegraphics|bibliography|addbibresource|input|include)\s*(?:\[.*?\])?\s*\{", tex),
            "The generated TeX references external files; package all required sources before uploading")
    require(not re.search(r"\\(?:documentclass)\s*(?:\[.*?\])?\s*\{acmart\}", tex),
            "The arXiv article must not silently inherit the ACMSE anonymous-review template")
    require("referee" not in tex[:3000].lower(), "Reject double-spaced referee layout")
    # The existing Pandoc-generated standalone TeX embeds its references;
    # there are no BibTeX inputs or included figures for this manuscript.
    payload = tex.encode("utf-8")
    output = base / "aivoblock-arxiv-source.zip"
    info = zipfile.ZipInfo("main.tex", date_time=(2026, 10, 23, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.external_attr = 0o644 << 16
    with zipfile.ZipFile(output, "w") as archive:
        archive.writestr(info, payload)
    with zipfile.ZipFile(output, "r") as archive:
        require(archive.namelist() == ["main.tex"], "The source ZIP must contain only main.tex")
        require(archive.read("main.tex") == payload, "The source ZIP does not match generated TeX")

    report = {
        "status": "DRAFT - NOT SUBMITTED",
        "archive": output.name,
        "contents": ["main.tex"],
        "processor_candidate": "XeLaTeX (requires confirmation in the arXiv interface)",
        "source_sha256": sha256(payload).hexdigest(),
        "archive_sha256": sha256(output.read_bytes()).hexdigest(),
        "manuscript_path": meta["manuscript_path"],
        "evidence_release": meta["evidence_release"],
        "evidence_commit": meta["evidence_commit"],
        "evidence_run_id": meta["evidence_run_id"],
        "endorsement_verified": False,
        "arxiv_id": None,
        "publication_authorized": False,
        "notes": [
            "Generated from canonical Pandoc LaTeX, not from the anonymous ACMSE poster",
            "Publication rights, affiliation, AI-disclosure, scholarly review and license gates remain pending",
            "Do not upload a source ZIP until the author reviews the TeX-rendered PDF and timing with ACMSE"
        ]
    }
    (base / "arxiv-source-report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    (base / "arxiv-source-sha256.txt").write_text(f"{report['archive_sha256']}  {output.name}\n", encoding="utf-8")
    print("PASS: arXiv source-only review ZIP created; NOT submitted")
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    info = package(args.repo_root)
    print(json.dumps({k: info[k] for k in ("status", "archive", "source_sha256", "archive_sha256")}, indent=2))


if __name__ == "__main__":
    main()
