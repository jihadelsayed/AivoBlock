# AivoBlock paper workspace

The [manuscript draft](aivoblock-manuscript.md) is a **working scientific-paper draft**, not yet peer reviewed, accepted, assigned a DOI or uploaded as a public preprint.

## Immutable numbers

The quantitative claims are tied to the **published** [v0.1.0 research release](https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype), specifically the SHA-256 verified data ZIP from CI run **39740779280** on commit `ade6ff42737d972658e9c309f9a2d8cffbfb988c`. They must not be updated from a different CI run without changing the manuscript's evidence version and re-validating all results.

Download and verify with Python 3.11+:

```bash
gh release download v0.1.0-research-prototype -R jihadelsayed/AivoBlock \
  -p aivoblock-research-evidence.zip -D manuscript-release
python3 scripts/package_research_evidence.py verify \
  manuscript-release/aivoblock-research-evidence.zip
python3 scripts/check_manuscript_evidence.py \
  --archive manuscript-release/aivoblock-research-evidence.zip
```

GitHub Actions also runs this check and `scripts/check_literature_review.py` with independent regression tests in the **Research paper figures against published v0.1.0 data** job. It validates numeric table cells, release revision, archive checksum and essential research limitations. It is a **mechanical consistency check**, not validation of novelty, formal security, economic viability or scientific conclusions.

## Remaining before public preprint

- Milestone #110: completed a **targeted, non-systematic** [primary-source related-work review](related-work-review.md), including AI training mining, permissionless useful-work schemes and close 2026 inference proposals. This does not certify novelty, freedom to operate or security.
- Milestone #111: review title/author and affiliation approval, resolve software/paper reuse rights and preprint venue, generate a final typeset version, archive data permanently with a DOI where appropriate, and submit only after the author chooses an archive.

The prototype GitHub release is public but currently **all rights reserved**. Publishing the paper and licensing code are separate decisions.
