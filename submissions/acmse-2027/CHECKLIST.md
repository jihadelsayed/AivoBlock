# ACMSE 2027 Research Poster — Submission Preflight

**Status: DRAFT — NOT READY TO SUBMIT.** The checks below are based on the official [Call for Posters](https://acmse.net/2027/posters-call/) and [Papers Format](https://acmse.net/2027/papers-format/) pages, rechecked 2026-10-23.

## Automatically checkable
- [x] Source uses the `sigconf,anonymous` ACM class options.
- [x] Review source omits explicit author, affiliation, email, acknowledgments and repository-owner identifiers.
- [x] Abstract includes no citations, equations, lists, tables or figures.
- [x] CSS/CCS concepts and comma-separated keywords are present.
- [x] All citation keys resolve to BibTeX entries; Lihu et al. author given names corrected against arXiv.
- [x] Selected numerical findings match the canonical manuscript (not an independently verified experiment).
- [x] Local draft PDF visually inspected: **two US-letter pages**; PDF metadata author blank. The displayed PDF was compiled using locally available ACM class files.

Run source checks on any checkout:

    python3 submissions/acmse-2027/check_submission.py --source-only

Run additional PDF checks after generating a clean PDF:

    python3 submissions/acmse-2027/check_submission.py --pdf submissions/acmse-2027/poster-abstract.pdf

Source-only checks run within the existing manuscript GitHub Actions job. These mechanical checks do not independently reproduce the research or validate scholarly originality.

## Blocking manual checks before upload
- [ ] **Rebuild using ACMSE 2027's actual modified LaTeX distribution**, starting from its `sample-sigconf.tex`, not only the standard ACM `acmart` class. The official template ZIP was not accessible in the current automation environment.
- [ ] Ensure exactly **two pages INCLUDING REFERENCES**, PDF, US letter, conference-specified font usage, title capitalization, and the venue's ACM reference block.
- [ ] Rebuild the BibTeX bibliography from the corrected `references.bib`; a clean local build was blocked by an unavailable `bibtex` executable.
- [ ] Inspect every PDF page, figures if added, tables if added, links, citations, bibliography, and document metadata.
- [ ] Confirm anonymity handling for a publicly identifiable GitHub repo and manuscript with ACMSE poster chairs. Never falsify public history; keep true identity in EasyChair.
- [ ] Check conference submission overlap and preprint policy before public arXiv posting.
- [ ] Author personally verifies all scientific claims, references, evidence, contributor credit, and required generative-AI disclosure in line with ACM/venue rules.
- [ ] ORCID obtained, rights and permissions verified, author approves exact submission artifact.
- [ ] Submit via EasyChair under the **Posters** track by **November 14, 2026**. No GitHub CI process performs submission.

## Provenance
Canonical manuscript: `research/paper/aivoblock-manuscript.md`  
Fixed experimental release: `v0.1.0-research-prototype`  
Recorded revision: `ade6ff42737d972658e9c309f9a2d8cffbfb988c`  
Recorded experiment run: `39740779280`

Important: this research shows a small *permissioned* working prototype. It does not demonstrate permissionless proof-of-useful-work security, production scalability, or independent-hardware experimental replication. Acceptance cannot be guaranteed.
