# AivoBlock — arXiv cs.DC Submission Form Worksheet

**DRAFT ONLY — NOT SUBMITTED, NOT ENDORSED.** This worksheet is NOT an arXiv ID or proof of eligibility. Compare the final values against the actual arXiv interface and the approved paper.

## Proposed values from the existing manuscript

- **Archive:** Computer Science.
- **Primary category:** `cs.DC` — Distributed, Parallel, and Cluster Computing.
- **Optional cross-list:** `cs.CR` — Cryptography and Security, *only if accepted and appropriate for the final technical content*. Do not assume cross-list eligibility.
- **Title:** AivoBlock: A Reproducible Permissioned Blockchain Prototype for Partitioned, Deterministically Verifiable AI Work
- **Proposed author:** Jihad Elsayed — **legal name confirmed by author; complete byline/contributor and publication approval remains pending**.
- **Proposed affiliation in the manuscript:** University of Alabama at Birmingham — **pending confirmation that it is appropriate for this independently developed research**.
- **ORCID:** Not provided; do not fabricate an identifier.
- **Keywords (where supported):** permissioned blockchain; verifiable AI inference; deterministic quantization; distributed systems; reproducible systems research.
- **Abstract:** Copy the entire `## Abstract` paragraph from [the canonical research manuscript](../../research/paper/aivoblock-manuscript.md) after approving its wording. Do not copy the shorter ACMSE poster abstract. Recheck measurement notation and any TeX markup when pasting into arXiv.
- **Report number, journal reference, DOI:** No report number, journal publication or article DOI is verified. Leave journal/DOI fields blank unless a real prior publication record exists.
- **Comments:** Optional. If used, accurately describe this as a research prototype with released experimental evidence; do not claim peer review or production deployment.
- **Article license:** **Not yet approved.** CC BY 4.0 was considered, but manuscript rights and publisher/conference compatibility must be confirmed before choosing an irrevocable license.
- **Source:** Only `aivoblock-arxiv-source.zip` from the *latest passing* `aivoblock-preprint-candidate` GitHub Actions artifact (its ZIP contains `main.tex`).
- **LaTeX processor:** XeLaTeX worked in GitHub CI; select/check the compatible arXiv processor during its own submission wizard and inspect the produced PDF.
- **Associated evidence:** https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype
- **Source evidence revision:** `ade6ff42737d972658e9c309f9a2d8cffbfb988c`, experiment CI run `39740779280`.

## Required checks before filling out the arXiv form

1. Open the actual submission page with the submitting arXiv account and confirm eligibility or complete the endorsement process. The account has previously shown **not endorsed for cs.DC**.
2. Ask ACMSE 2027 about identifiable public GitHub evidence and posting an identified arXiv preprint under double-anonymous poster review. Do **not** assume that public posting is automatically harmless.
3. Personally approve the complete paper, references, author credit, affiliation, contributions, funding/conflicts, AI-use disclosures, rights and article license.
4. Confirm arXiv's own compiled PDF is faithful: author byline, abstract, all pages/tables, URLs, references and no LaTeX errors. The locally compiled CI PDF is not a substitute.
5. Submit only after the author's final explicit authorization. Record the arXiv ID only after it is actually issued.

**Author/rights gates:** [research/paper/submission-metadata.json](../../research/paper/submission-metadata.json). All are deliberately pending; preparation alone must not change them.

arXiv guidance: https://info.arxiv.org/help/endorsement.html | https://info.arxiv.org/help/submit_tex.html | https://info.arxiv.org/help/license/index.html
