#!/usr/bin/env python3
"""Check arXiv manuscript internal consistency only (not scientific peer review)."""
import json
from pathlib import Path
import re

root = Path(__file__).resolve().parents[2]
paper = (root / "research/paper/aivoblock-manuscript.md").read_text(encoding="utf-8")
meta = json.loads((root / "research/paper/submission-metadata.json").read_text(encoding="utf-8"))
errors = []
def require(ok, description):
    if not ok:
        errors.append(description)

urls = {
    1:"https://bitcoin.org/bitcoin.pdf",
    2:"https://www.usenix.org/conference/osdi-99/practical-byzantine-fault-tolerance",
    3:"https://doi.org/10.3390/e21080723",
    4:"https://arxiv.org/abs/2001.09244",
    5:"https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype",
    6:"https://doi.org/10.1007/978-3-031-15979-4_12",
    7:"https://arxiv.org/abs/2404.09005",
    8:"https://arxiv.org/abs/2504.09971",
    9:"https://doi.org/10.1016/j.bcra.2025.100387",
    10:"https://doi.org/10.1145/3828717",
    11:"https://www.sedracoin.com/research/proof-of-useful-inference/",
    12:"https://arxiv.org/abs/2610.26713",
}
refs = re.findall(r"(?m)^\[(\d+)\]\s+(.+)$", paper)
require([int(n) for n, _ in refs] == list(range(1, 13)),
        "Expected twelve numbered bibliography entries in sequence")
for n, line in refs:
    require(urls[int(n)] in line, f"Reference [{n}] URL does not match the audited source")
require("LNCS vol. 13508, pp. 339–369" in paper and
        "Ofelimos: Combinatorial Optimization via Proof-of-Useful-Work.”" in paper,
        "Ref [6] must use the publisher's CRYPTO 2022 chapter metadata")
require("arXiv:2504.09971v4, revised 13 November 2025" in paper and
        "[8] I. Komargodski and O. Weinstein." in paper,
        "Ref [8] must cite the verified two-author arXiv v4 record")
require("J. Elsayed and AivoBlock contributors" in paper,
        "Ref [5] release attribution changed unexpectedly; author sign-off required")
require(paper.startswith("# " + meta["title"] + "\n"), "Manuscript title and metadata differ")
require(meta.get("evidence_commit") == "ade6ff42737d972658e9c309f9a2d8cffbfb988c",
        "Experimental source revision changed")
require(meta.get("evidence_run_id") == "39740779280", "Experimental evidence run changed")
require(meta.get("publicly_submitted") is False and meta.get("arxiv_id") is None,
        "This draft must not claim an arXiv submission")
creators = meta.get("creators", [])
require(len(creators) == 1 and creators[0].get("name") == "Jihad Elsayed"
        and creators[0].get("legal_name_confirmed_by_author") is True
        and creators[0].get("confirmed_by_author") is False,
        "Author confirmation is pending and cannot be set by CI")
approvals = meta.get("approval", {})
require(len(approvals) == 10 and all(x is False for x in approvals.values()),
        "Author/rights approval gates must remain unapproved in the candidate build")

abstract = paper.split("## Abstract\n", 1)[-1].split("\n## 1. Introduction", 1)[0]
for val in ("270", "730.760", "779.342", "1.880", "0.742", "3.412"):
    require(val in abstract, f"Abstract lost fixed measurement {val}")
for qualifier in (
    "full recomputation", "permissioned", "synthetic",
    "different trust and timing boundaries", "not an independent rerun",
    "Generative AI assistance", "not yet been confirmed", "all-rights-reserved",
):
    require(qualifier.lower() in paper.lower(), f"Missing limit/disclosure: {qualifier}")

if errors:
    for item in errors:
        print("FAIL:", item)
    raise SystemExit(1)
print("PASS: internal manuscript citations, evidence markers, and non-publication gates")
print("NOTE: No online source re-verification, experimental rerun, or author approval is implied.")
