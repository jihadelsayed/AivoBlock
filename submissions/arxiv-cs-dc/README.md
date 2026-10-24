# AivoBlock — arXiv cs.DC preprint submission

**Venue:** arXiv, Computer Science — Distributed, Parallel, and Cluster Computing (`cs.DC`)  
**Status:** PREPARATION ONLY — not uploaded, endorsed, announced, or moderated as accepted.

## Canonical source — one source of truth

- Full research manuscript: [research/paper/aivoblock-manuscript.md](../../research/paper/aivoblock-manuscript.md)
- Existing detailed preparation and approval guide: [research/paper/SUBMISSION.md](../../research/paper/SUBMISSION.md)
- Existing author/license approval state: [research/paper/submission-metadata.json](../../research/paper/submission-metadata.json)
- CI produces a **candidate** LaTeX/PDF preprint bundle; the presence of an artifact does *not* establish a public submission.
- Fixed research-evidence release: https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype
- Source revision `ade6ff42737d972658e9c309f9a2d8cffbfb988c`; evidence CI run `39740779280`.

## arXiv preparation gates

- [ ] Obtain `cs.DC` endorsement or confirm actual exemption/eligibility with arXiv (currently blocked for this category).
- [ ] Personally review and verify manuscript, cited sources, numerical claims, limitations and source artifacts.
- [ ] Confirm accurate author name, true affiliations, ORCID, contributor acknowledgement and author permissions.
- [ ] Confirm funding/conflict statements and disclosure of substantial AI assistance.
- [ ] Confirm manuscript copyright/rights and **explicitly choose** the irrevocable paper distribution license; paper and code licenses are separate. **CC BY 4.0 has been discussed as a preference**, but never silently mark rights approved or overwrite the canonical approval metadata.
- [ ] Verify the LaTeX source compiles, use only necessary source/figure files, inspect PDF, bibliography, tables and links.
- [ ] Confirm appropriate timing and public-preprint compatibility with ACMSE's double-anonymous review process.
- [ ] Obtain explicit final author authorization before public upload; only record an arXiv ID after successful posting.

Official guidance:
- https://info.arxiv.org/help/submit/index.html
- https://info.arxiv.org/help/submit_tex.html
- https://info.arxiv.org/help/endorsement.html
- https://info.arxiv.org/help/license/index.html

## Planned files (created in subsequent steps)

- `source/`: submission-ready LaTeX and necessary figures, sourced from the canonical manuscript/preprint workflow.
- `METADATA.md`: final title, authors, subject classification, abstract and submission comments (reviewed, not invented).
- `CHECKLIST.md`: independent final checks of metadata, compile, citation accuracy, rights and endorsement.

## Research scope

The work characterizes a permissioned prototype for signed, partitioned integer AI inference, full validator recomputation, quorum finality and deterministic test-ledger settlement. It **does not** claim novel or secure permissionless proof-of-useful-work, cryptographically succinct AI verification, realistic token economics, or independent cross-hardware replication.

This repository being public does not mean the article is posted on arXiv, peer reviewed, or guaranteed to pass moderation.

## Next work

1. Audit the existing preprint build and LaTeX source against arXiv's source requirements.
2. Fix paper metadata, bibliography, scientific statements and the author's disclosure checklist.
3. Build/inspect a clean source package and PDF; prepare the arXiv form metadata.
4. Wait for endorsement, venue coordination and final author approval before uploading.
