# arXiv cs.DC — Proposed Submission Metadata

**Status:** Candidate information for author review only. No paper has been submitted to arXiv.

## Proposed fields

**Title:** AivoBlock: A Reproducible Permissioned Blockchain Prototype for Partitioned, Deterministically Verifiable AI Work

**Author:** Jihad Elsayed — legal name confirmed by author on 24 October 2026; verify contributor credit and final submission authorization before upload.

**Affiliation:** University of Alabama at Birmingham (UAB) — the author has identified UAB as the current institution; confirm the appropriate research-affiliation wording and contributor credits for the manuscript.

**Primary subject:** `cs.DC` — Distributed, Parallel, and Cluster Computing.  
**Possible cross-list:** `cs.CR` only if the completed manuscript and arXiv moderation justify it.

**Keywords:** permissioned blockchain; deterministic AI inference; useful computation; validator recomputation; reproducibility.

**License discussed with author:** CC BY 4.0. **Do not automatically select or finalize it:** arXiv licenses are irrevocable for the posted version, and the repository's canonical rights/approval gates have not yet been signed off.

**Journal reference and DOI:** leave blank until actually assigned or applicable.

**Evidence:** https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype

**Evidence source commit:** `ade6ff42737d972658e9c309f9a2d8cffbfb988c`  
**Evidence CI run:** `39740779280`

## Abstract

The authoritative abstract is the one in [the canonical research manuscript](../../research/paper/aivoblock-manuscript.md), under `## Abstract`. Do not maintain an independently edited copy here. Copy it into the arXiv form only **after** verifying that the final rendered submission source contains the same claims and numbers.

## Source and compilation

The existing `scripts/build_preprint.sh` pipeline converts the canonical Markdown manuscript to standalone LaTeX/PDF. Run `python3 submissions/arxiv-cs-dc/package_source.py` after the preprint build. It produces a **review-only** source ZIP under `research/paper/build/aivoblock-arxiv-source.zip`.

The upload ZIP contains **only `main.tex`**. No draft metadata, compiled PDF, conference template, log files, or unrelated source code belongs in the arXiv upload. The accompanying report and hashes are outside the upload ZIP for independent review.

arXiv's current TeX processing accepts source ZIPs and automatically handles the chosen TeX processor; the author must inspect the resulting PDF. The proposed processor is XeLaTeX because the canonical draft uses Pandoc's XeLaTeX render path. Verify it in the actual interface. See https://info.arxiv.org/help/submit_tex.html.

## Explicit publication gates

- [ ] Confirm author byline, UAB research affiliation, contributor acknowledgments and permissions.
- [ ] Read and correct the paper personally; validate all citations, mathematical descriptions, experimental statements, and substantial generative-AI assistance disclosure.
- [ ] Confirm rights, funding/conflicts and actual arXiv paper license.
- [ ] Obtain eligibility/endorsement for `cs.DC`.
- [ ] Coordinate the timing of public arXiv publication with ACMSE's double-anonymous review policy, including the publicly identifiable GitHub repo.
- [ ] Inspect the arXiv-processed PDF and authorize this exact version for posting.

**A successful ZIP build cannot provide endorsement, certify originality, or publish the paper.**
