#!/usr/bin/env python3
"""Validate a *draft* AivoBlock preprint package without authorizing publication.

A PASS means that the publication candidate is internally consistent and
blocked from submission, not that it is scientifically accepted, licensed,
endorsed, independent of AI authorship or actually deposited anywhere.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path

SOURCE="ade6ff42737d972658e9c309f9a2d8cffbfb988c"
RUN="39740779280"
PAPER="research/paper/aivoblock-manuscript.md"
META="research/paper/submission-metadata.json"
GUIDE="research/paper/SUBMISSION.md"

REQUIRED_APPROVALS={
    "author_identity_and_order",
    "affiliation_and_orcid",
    "all_coauthor_and_contributor_permissions",
    "rights_to_distribute",
    "manuscript_content_and_citations",
    "ai_assistance_disclosure",
    "funding_and_conflicts",
    "preprint_license",
    "arxiv_account_and_category_eligibility",
    "explicit_publish_authorization",
}

def require(ok: bool, reason: str) -> None:
    if not ok: raise ValueError(reason)

def audit(root: Path) -> dict:
    paper=(root/PAPER).read_text(encoding="utf-8")
    meta=json.loads((root/META).read_text(encoding="utf-8"))
    guide=(root/GUIDE).read_text(encoding="utf-8")
    require(meta.get("schema")=="aivoblock-preprint-candidate-v1", "wrong preprint metadata schema")
    require(meta.get("manuscript_path")==PAPER, "wrong tracked manuscript")
    require(meta.get("evidence_commit")==SOURCE and meta.get("evidence_run_id")==RUN,
            "immutable experimental reference changed")
    require(SOURCE in paper and RUN in paper and "v0.1.0-research-prototype" in paper,
            "fixed released research evidence missing")
    title=paper.splitlines()[0].removeprefix("# ").strip()
    require(title==meta.get("title"), "manuscript and submission title disagree")
    creators=meta.get("creators")
    require(isinstance(creators,list) and len(creators)>=1 and
            all(isinstance(p,dict) and p.get("name") == "Jihad Elsayed" and
                p.get("legal_name_confirmed_by_author") is True and
                p.get("confirmed_by_author") is False for p in creators),
            "unknown or falsely confirmed author identity")
    require(meta.get("publicly_submitted") is False and
            meta.get("preprint_doi") is None and
            meta.get("existing_paper_doi") is None and
            meta.get("arxiv_id") is None,
            "a submission or DOI cannot be claimed in the draft package")
    flags=meta.get("approval")
    require(isinstance(flags,dict) and set(flags)==REQUIRED_APPROVALS and
            all(value is False for value in flags.values()),
            "submission must stay gated until author approves and signs")
    required_disclosures=[
        "Generative AI assistance",
        "OpenAI ChatGPT (GPT-6)",
        "substantially assisted",
        "human author must",
        "funding",
        "competing interests",
        "not yet been confirmed",
        "manuscript-distribution license",
        "all-rights-reserved",
        "not a production",
        "not an independent rerun",
    ]
    for phrase in required_disclosures:
        require(phrase.lower() in paper.lower(), f"missing preprint disclosure: {phrase}")
    require("NOT SUBMITTED" in guide and "arXiv" in guide and
            "Zenodo" in guide and "OSF Preprints" in guide,
            "incomplete venue/submission guide")
    require("https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype"==
            meta.get("evidence_release"), "unverifiable release URL")
    require(len(meta.get("proposed_subjects",[]))>=1 and
            all(isinstance(v,str) for v in meta.get("keywords",[])),
            "missing subject or keywords")
    return {
        "status":"PASS",
        "scope":"draft package only; author approval and publication are still pending",
        "manuscript_sha256":hashlib.sha256(paper.encode("utf-8")).hexdigest(),
        "approval_count":0,
        "required_author_approval_count":len(REQUIRED_APPROVALS),
        "publicly_submitted":False,
        "preprint_doi":None,
        "evidence_commit":SOURCE,
        "evidence_run":RUN,
    }


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--root",type=Path,default=Path("."))
    p.add_argument("--out",type=Path)
    args=p.parse_args()
    info=audit(args.root)
    if args.out is not None:
        args.out.parent.mkdir(parents=True,exist_ok=True)
        args.out.write_text(json.dumps(info,sort_keys=True,indent=2)+"\n",encoding="utf-8")
    print(json.dumps(info,sort_keys=True,indent=2))


if __name__=="__main__":
    main()
