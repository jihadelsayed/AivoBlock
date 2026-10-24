# AivoBlock — ACM Southeast 2027 submission

**Venue:** ACM Southeast Conference (ACMSE 2027)  
**Target track:** Research Poster (two-page extended abstract, including references)  
**Status:** PREPARATION ONLY — not submitted, accepted, or reviewed.

## Submission requirements (verified 2026-10-23)

- Deadline: **November 14, 2026**; decisions: February 20, 2027; camera-ready: March 1, 2027.
- The **review PDF must be double-anonymous**: no author names or affiliations. Enter the true author information in the EasyChair submission system, not the review PDF.
- Use the **mandatory ACMSE 2027 LaTeX template** (modified ACM `sigconf`), PDF format, letter paper; Word is not accepted. An incorrect format can be rejected before review.
- An **ORCID** is required. Accepted posters require an in-person presenter to register and present (April 15–17, 2027, Decatur, Alabama).
- Posters are intended for preliminary/exploratory results; acceptance is determined by review. The poster track is our **first target**, not a claim of acceptance.

Official instructions:
- https://acmse.net/2027/posters-call/
- https://acmse.net/2027/papers-format/
- https://acmse.net/2027/papers-call/

## Canonical research and evidence — do not fork the numbers

- Manuscript: [research/paper/aivoblock-manuscript.md](../../research/paper/aivoblock-manuscript.md)
- Related-work analysis: [research/paper/related-work-review.md](../../research/paper/related-work-review.md)
- Published evidence: https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype
- Fixed evidence source revision: `ade6ff42737d972658e9c309f9a2d8cffbfb988c`, GitHub Actions run `39740779280`.
- This paper demonstrates **small, permissioned, deterministic inference and full validator recomputation**. It does *not* establish a permissionless proof-of-useful-work consensus protocol.

## Planned files (created in subsequent steps)

- `poster-abstract.tex`: review-anonymous two-page ACMSE poster extended abstract.
- `references.bib`: source-checked citations used in the poster.
- `figures/`: only necessary, source-supported figures.
- `CHECKLIST.md`: format, author-anonymity, provenance, citations, PDF and approval checks.
- `camera-ready/`: **only after acceptance**, with author names and affiliation.

## Critical anonymity and disclosure rules

The public GitHub repository identifies the project and author. **Do not include an identifying repository URL, personal name, institution, acknowledgement, or PDF author metadata in the review manuscript** without following ACMSE's author guidelines. Check with the program chairs about referencing public artifacts/preprints; do not rewrite or falsify the public history. Author identity is retained in EasyChair and restored in camera-ready materials if accepted.

Follow current ACM/venue rules on generative-AI assistance and paper authorship. The author must personally review every claim and reference; CI checks do not replace scholarly review. No submission is authorized by this folder.

## Next work

1. Build the exact ACMSE poster LaTeX structure using the mandatory template.
2. Extract one specific research question, contributions, experimental design, and numbers from the fixed evidence.
3. Produce and inspect the two-page anonymous review PDF; verify source consistency and PDF metadata.
4. Obtain the author's final approval before any upload.

**Acceptance is not guaranteed.** This folder is an auditable preparation workspace.
