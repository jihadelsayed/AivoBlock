# AivoBlock — arXiv cs.DC final-readiness checklist

**As of October 23, 2026: PREPARATION ONLY. NOT ENDORSED, NOT SUBMITTED, NOT ACCEPTED.**  
Canonical paper: [full AivoBlock manuscript](../../research/paper/aivoblock-manuscript.md)  
Source approval flags: [preprint submission metadata](../../research/paper/submission-metadata.json)

## What the repository has technically verified

- [x] Full paper, evidence and detailed scientific limitations remain in the canonical manuscript.
- [x] Results are tied to fixed evidence release `v0.1.0-research-prototype`, source revision `ade6ff42737d972658e9c309f9a2d8cffbfb988c` and run `39740779280`.
- [x] Bibliography and selected novelty boundaries audited; reference [6] title corrected to the CRYPTO 2022 publisher record, reference [8] now explicitly cites arXiv v4.
- [x] The Step 14 GitHub preprint job successfully built a clean ZIP containing `main.tex` and independently compiled it into a readable **10-page** PDF.
- [x] The arXiv and ACMSE submission packages remain separate; the ACMSE review PDF is anonymous and two pages, whereas arXiv contains the full-length identified paper.

**CI is only a technical gate.** After any new commit, use the latest successful preprint job instead of treating previous artifacts as current. Run `python3 submissions/arxiv-cs-dc/readiness.py` after building and compiling the source ZIP. It emits `research/paper/build/arxiv-submission-readiness.json`; publication remains blocked even when its technical checks pass.

## Human-led requirements still outstanding

- [ ] **arXiv cs.DC endorsement.** The user's arXiv account previously reported no endorsement for cs.DC. Recheck current account eligibility or obtain one from an eligible established researcher. Using an institutional email does not by itself certify eligibility. [Official policy](https://info.arxiv.org/help/endorsement.html).
- [ ] **ACMSE poster anonymity and preprint timing.** Ask the 2027 conference poster chairs whether the public, author-identifying GitHub project and a future identified arXiv preprint are compatible with double-anonymous poster review. The poster call does **not** expressly resolve this particular case. Get their response before making the preprint public. [Poster call](https://acmse.net/2027/posters-call/).
- [ ] **Author and affiliation.** Confirm exact legal/professional byline, appropriateness of UAB affiliation for independent work, ORCID, contributors, acknowledgments and coauthor permissions. A professor need not be a coauthor to endorse or review it.
- [ ] **Scientific review.** Read and revise the paper yourself, substantiate all data/citations/algorithmic statements, and ideally arrange an independent CS researcher review. CI does not validate novelty or independent scientific reproducibility.
- [ ] **Rights and disclosures.** Confirm funding, competing interests, substantial ChatGPT drafting/engineering assistance, and any university or sponsor obligations. Class homework rules are distinct from open research publication.
- [ ] **License.** The discussed preference is **CC BY 4.0 for the paper**, not a change to the all-rights-reserved source-code license. Confirm manuscript rights and explicitly choose the arXiv version's **irrevocable** license. [arXiv license policy](https://info.arxiv.org/help/license/index.html).
- [ ] **Exact arXiv processing.** Upload source after all prior gates, then check arXiv's auto-detected top-level TeX, compiler, rendered PDF, title, abstract, references, and author metadata before final submission. [TeX instructions](https://info.arxiv.org/help/submit_tex.html).
- [ ] **Explicit final authorization.** The human author must approve the exact version, complete the agreement and personally submit. Record an arXiv identifier only after arXiv issues one.

## ACMSE poster remains a different deliverable

**ACM Southeast 2027** requires an anonymous, exactly **two-page** poster extended abstract, including references, in its mandatory LaTeX template; it requires ORCID and uses EasyChair's Posters track. Poster abstract deadline: **November 14, 2026**. Do not upload the author-identified arXiv source as the ACMSE review manuscript. [Official poster call](https://acmse.net/2027/posters-call/).

## Suggested question to the ACMSE chairs

> I am preparing an ACMSE 2027 double-anonymous research poster about a permissioned blockchain prototype. The underlying software and data are already public on GitHub and identify the author. I am also considering a full-length arXiv preprint. May I post that preprint before or during poster review, and how should the public GitHub evidence be referenced in the anonymous PDF?

## Final sequence after human confirmation

1. Secure category endorsement/eligibility and ACMSE organizer clarification.
2. Author reviews and approves science, citations, collaborators, university affiliation, disclosures and rights.
3. Download the **latest passing** GitHub preprint artifact with `aivoblock-arxiv-source.zip`; match its SHA-256 report.
4. Upload `main.tex` through arXiv, inspect its TeX processing and PDF, and enter accurate submission metadata.
5. Select the approved irrevocable license, finish the agreement and submit personally.

**An arXiv preprint is not peer-reviewed acceptance, and neither venue guarantees publication.**
