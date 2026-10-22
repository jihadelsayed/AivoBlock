#!/usr/bin/env python3
"""Fail closed on contradictions and missing release documentation.

This is a *documentation and disclosure* gate; it cannot certify an absence
of vulnerabilities, legal title, originality or scientific peer review.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

VERSION = "v0.1.0-research-prototype"
RELEASE_FILE = "docs/releases/v0.1.0-research-prototype.md"


def audit(root: Path) -> dict:
    def read(name: str) -> str:
        path = root / name
        if not path.is_file():
            raise ValueError(f"missing release input: {name}")
        return path.read_text(encoding="utf-8")

    license_text = read("LICENSE")
    readme = read("README.md")
    notes = read(RELEASE_FILE)
    testnet = read("docs/testnet.md")
    development = read("docs/development.md")
    guide = read("docs/research-reproducibility-package.md")

    if ("All rights reserved." not in license_text or
            "No open-source license has been granted" not in license_text or
            any(line.strip() == "MIT" for line in license_text.splitlines())):
        raise ValueError("inconsistent or mislabelled source rights")
    if ("**Source-available research prototype; all rights reserved" not in readme or
            "does **not** grant an open-source license" not in readme):
        raise ValueError("README must accurately disclose reserved rights")
    if ("transport is unencrypted" in testnet or
            "frames are encrypted" not in testnet or
            "permissioned" not in testnet):
        raise ValueError("testnet transport/validator scope misrepresented")
    if ("do not yet form a running decentralized network" in development or
            "Working permissioned private Docker testnet" not in development):
        raise ValueError("development guide understates or misstates testnet")
    claims = [
        "Research Prototype", "270 observed chain tasks",
        "90 CPU service requests", "Independent data reanalysis",
        "not a production blockchain", "full recomputation",
        "not an independent rerun", "rights reserved",
        "https://github.com/jihadelsayed/AivoBlock/releases/tag/" + VERSION,
    ]
    if any(claim.casefold() not in notes.casefold() for claim in claims):
        raise ValueError("release notes omit a required scope or provenance limitation")
    if ("independent-audit.json" not in notes or
            "aivoblock-research-evidence.zip" not in notes or
            "SHA-256" not in guide):
        raise ValueError("incomplete release asset/verification instructions")
    return {
        "status": "PASS",
        "tag": VERSION,
        "type": "source-available, all-rights-reserved research prototype prerelease",
        "checked": ["LICENSE", "README.md", "docs/testnet.md",
                    "docs/development.md", "docs/research-reproducibility-package.md",
                    RELEASE_FILE],
        "limitations": [
            "not a license/legal audit",
            "not a cybersecurity audit",
            "not an independent experiment reproduction",
            "not a peer-reviewed research publication",
        ],
    }


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--root", type=Path, default=Path("."))
    p.add_argument("--out", type=Path)
    args = p.parse_args()
    report = audit(args.root)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print("PASS: research prototype release claims, licensing and scope are internally consistent")


if __name__ == "__main__":
    main()
