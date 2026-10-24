#!/usr/bin/env python3
"""Audit the draft arXiv package. Success DOES NOT authorize submission."""
import argparse
from hashlib import sha256
import json
from pathlib import Path
from zipfile import ZipFile

ROOT = Path(__file__).resolve().parents[2]
BUILD = ROOT / "research/paper/build"

def inspect():
    meta = json.loads((ROOT / "research/paper/submission-metadata.json").read_text())
    source_report = json.loads((BUILD / "arxiv-source-report.json").read_text())
    zip_path = BUILD / "aivoblock-arxiv-source.zip"
    pdf_path = BUILD / "aivoblock-arxiv-source-preview.pdf"
    errors = []
    if not zip_path.is_file():
        errors.append("Missing arXiv source ZIP")
    else:
        if sha256(zip_path.read_bytes()).hexdigest() != source_report.get("archive_sha256"):
            errors.append("Source ZIP checksum mismatch")
        with ZipFile(zip_path) as archive:
            if archive.namelist() != ["main.tex"]:
                errors.append("Upload ZIP must contain only main.tex")
            elif sha256(archive.read("main.tex")).hexdigest() != source_report.get("source_sha256"):
                errors.append("main.tex hash mismatch")
    if not pdf_path.is_file() or pdf_path.stat().st_size < 1024:
        errors.append("Missing independently compiled PDF preview")
    if meta.get("publicly_submitted") is not False or meta.get("arxiv_id") is not None:
        errors.append("Draft incorrectly asserts a published paper")
    if source_report.get("publication_authorized") is not False or source_report.get("endorsement_verified") is not False:
        errors.append("Draft incorrectly asserts permission or endorsement")
    gates = meta.get("approval", {})
    if len(gates) != 10 or any(value is not False for value in gates.values()):
        errors.append("Draft author/rights approval gates must remain unapproved")
    creators = meta.get("creators", [])
    if not creators or any(c.get("name") != "Jihad Elsayed" or c.get("legal_name_confirmed_by_author") is not True or c.get("confirmed_by_author") is not False for c in creators):
        errors.append("Draft byline must remain unconfirmed pending human verification")
    return {
        "schema": "aivoblock-arxiv-readiness-v1",
        "status": "PREPARATION PASSED, PUBLICATION BLOCKED" if not errors else "PREPARATION FAILED",
        "publication_authorized": False,
        "arxiv_submitted": False,
        "technical_package_verified": not errors,
        "endorsement_verified": False,
        "acmse_compatibility_confirmed": False,
        "arxiv_hosted_pdf_reviewed": False,
        "proposed_author": creators[0].get("name") if creators else None,
        "proposed_affiliation": creators[0].get("affiliation") if creators else None,
        "pending_author_approvals": [key for key, value in gates.items() if value is False],
        "outstanding_human_checks": [
            "Obtain arXiv cs.DC endorsement or verify eligibility",
            "Get ACMSE poster-chair answer on public GitHub/arXiv and anonymous review",
            "Approve byline, UAB affiliation, ORCID and contributor credit",
            "Personally verify manuscript, scholarly citations and scientific limitations",
            "Confirm funding, conflicts, and substantial AI-assistance disclosure",
            "Confirm distribution rights and select irrevocable paper license",
            "Review the PDF compiled inside arXiv, not only the GitHub preview",
            "Authorize and personally submit the exact approved version",
        ],
        "errors": errors,
        "evidence_commit": meta.get("evidence_commit"),
        "evidence_run_id": meta.get("evidence_run_id"),
        "note": "Passing CI is not peer review, acceptance, endorsement or consent to publish."
    }

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, default=BUILD / "arxiv-submission-readiness.json")
    args = parser.parse_args()
    report = inspect()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    for error in report["errors"]:
        print("FAIL:", error)
    if report["errors"]:
        raise SystemExit(1)
    print("PASS: Source ZIP and preview validated.")
    print("BLOCKED: arXiv endorsement, venue coordination, rights, author approval and final submission are not verified.")

if __name__ == "__main__":
    main()
