#!/usr/bin/env python3
"""Static publication-risk and bibliography check for AivoBlock's scoped review.

This is an offline integrity and wording gate, *not* live literature retrieval,
plagiarism detection, novelty certification, peer review, or patent clearance.
Publication metadata was manually checked against primary sources 2026-10-22.
"""
import argparse
import re
from pathlib import Path

PAPER = "research/paper/aivoblock-manuscript.md"
REVIEW = "research/paper/related-work-review.md"
SOURCES = {
    3: ("2019", "10.3390/e21080723", "Coin.AI"),
    4: ("2020", "2001.09244", "A Proof of Useful Work for Artificial Intelligence"),
    6: ("2022", "Ofelimos", "CRYPTO"),
    7: ("2024", "2404.09005", "Proof-of-Learning"),
    8: ("2025", "2504.09971", "Matrix Multiplication"),
    9: ("2025", "10.1016/j.bcra.2025.100387", "Systematic Literature Review"),
    10: ("2026", "10.1145/3828717", "On Replacing Cryptopuzzles"),
    11: ("2026", "sedracoin.com/research/proof-of-useful-inference", "working paper"),
    12: ("2026", "2610.26713", "19 October 2026"),
}

REQUIRED_PHRASES = (
    "permissioned",
    "full recomputation",
    "not a production",
    "not a systematic literature review",
    "not an independent rerun",
    "must not be interpreted as a speedup or overhead ratio",
    "no inference tickets",
    "not peer reviewed",
    "22 October 2026",
)


def audit(paper: str, review: str) -> dict:
    for number, tokens in SOURCES.items():
        definition = re.search(r"^\[" + str(number) + r"\]\s+(.+)$",
                               paper, flags=re.MULTILINE)
        if not definition:
            raise ValueError(f"missing numbered source [{number}]")
        entry = definition.group(1)
        for token in tokens:
            if token.casefold() not in entry.casefold():
                raise ValueError(f"incorrect reference [{number}] metadata: {token}")
        if f"[{number}]" not in paper.split("## References")[0]:
            raise ValueError(f"reference [{number}] is not cited in manuscript")
    for phrase in REQUIRED_PHRASES:
        if phrase.casefold() not in (paper + "\n" + review).casefold():
            raise ValueError(f"omitted limitation: {phrase}")
    for heading in ("### 1.1 Related work", "## 6. Threats to validity",
                    "## References"):
        if heading not in paper:
            raise ValueError(f"missing manuscript section: {heading}")
    for row in ("Coin.AI", "Lihu et al.", "Ofelimos", "Zhao et al.",
                "Komargodski", "SEDRA", "Wong", "Bakhshi", "Merlina"):
        if row.casefold() not in review.casefold():
            raise ValueError(f"missing comparison of work: {row}")
    for row in ("# AivoBlock milestone #110", "## Primary-source comparison",
                "## Findings by scientific claim", "## Before milestone #111"):
        if row not in review:
            raise ValueError("incomplete scoped literature review: " + row)
    for incorrect in (
        "AivoBlock is the first AI blockchain",
        "AivoBlock invented AI mining",
        "AivoBlock is the first inference blockchain",
        "AivoBlock proves permissionless consensus",
        "patent granted to AivoBlock",
    ):
        if incorrect.casefold() in paper.casefold():
            raise ValueError("unsupported priority/security claim: " + incorrect)
    if ("v0.1.0-research-prototype" not in paper or
            "ade6ff42737d972658e9c309f9a2d8cffbfb988c" not in paper or
            "39740779280" not in paper):
        raise ValueError("research paper lost immutable numerical provenance")
    # Avoid incorrectly promoting an author's speculative working paper or an
    # arXiv upload to peer-reviewed journal status.
    for candidate in ("working paper", "preprint", "not peer reviewed"):
        if candidate.casefold() not in (paper + review).casefold():
            raise ValueError("not all publication types are disclosed")
    return {"verdict": "PASS", "review_type": "targeted, non-systematic",
            "checked_primary_records": len(SOURCES),
            "provenance": "manual primary-source review dated 2026-10-22",
            "limitations": "not an automated external citation or legal patent audit"}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--root", type=Path, default=Path("."))
    args = p.parse_args()
    import json
    print(json.dumps(audit((args.root / PAPER).read_text(encoding="utf-8"),
                           (args.root / REVIEW).read_text(encoding="utf-8")),
                     indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
