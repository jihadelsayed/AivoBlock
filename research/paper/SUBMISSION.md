# AivoBlock — author-facing preprint submission package

**Prepared:** 22 October 2026 · **Status: draft package, NOT SUBMITTED.**
**Candidate:** arXiv (subject to author registration, possible endorsement, and category moderation).
**Alternative:** Zenodo publication record for the manuscript and/or separate permanent research-data deposit (author controls license and DOI publication).

## Current artifacts

The canonical working text is [the manuscript](aivoblock-manuscript.md). Its source, the [targeted review](related-work-review.md), the [fixed v0.1.0 released experimental archive](https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype), and the [independent audit](https://github.com/jihadelsayed/AivoBlock/releases/download/v0.1.0-research-prototype/independent-audit.json) are linked with their separate provenance. All paper performance claims remain grounded to CI run `39740779280` at source revision `ade6ff42737d972658e9c309f9a2d8cffbfb988c`. Do not replace those fixed numbers with results from a later, unrelated workflow.

The CI **Preprint typesetting and author-approval gate** creates, on its own isolated runner, an archive named `aivoblock-preprint-candidate` containing:

- `aivoblock-preprint.pdf`: reviewable typeset paper;
- `aivoblock-preprint.tex`: LaTeX source converted from the same Markdown;
- `aivoblock-preprint-source.md`: original tracked Markdown;
- `submission-metadata.json`: proposed title, subjects and **unapproved** author/rights gates;
- `SHA256SUMS.txt` and `build-provenance.json`: file hashes and exact source SHA.

**This CI artifact is not an arXiv submission and is not a Zenodo DOI.** It is a build-and-review package only, never uploaded to a public preprint host by the workflow.

## Author approvals required before any public preprint submission

Each item is deliberately **false/pending** in `submission-metadata.json` until approved by the rights holders and human author. The author should:

1. Confirm the full name(s), byline order, accurate affiliations and any ORCID; consult all people who may merit authorship or acknowledgement.
2. Read and personally revise the manuscript, verify source quotations, algorithms, security claims, references, numeric tables and conflicts. CI validates consistency, not scientific truth or peer review.
3. Approve the explicit **substantial generative-AI assistance** disclosure; do not present an AI-written draft as solely human-authored work. A venue may decline predominantly AI-generated writing.
4. Confirm funding, institutional/sponsor disclosure rules and competing interests, rather than inferring that there were none.
5. Confirm ownership of *manuscript rights* and choose a paper distribution license. AivoBlock **software** remains source-available and all-rights-reserved; do not silently grant an MIT/Apache software license.
6. Decide whether patent/IP advice is needed **before further public disclosure**. The GitHub prototype is already public; the paper might be an additional disclosure.
7. Confirm the selected archive, category and submitter account/endorsement eligibility; personally accept the archive's irreversible distribution and submittal terms.
8. Provide **explicit approval** to publish this particular reviewed version. Without it, keep all submission gates false. This workflow deliberately has no publish action or archive credentials.

## arXiv source-only upload candidate (additional preflight)

The author-review bundle is **not** itself an arXiv upload: it includes a generated PDF, Markdown, internal JSON metadata and CI checksums. arXiv prefers the **TeX source only**, and typically rejects uploading a PDF already compiled from TeX. Source guidance: https://info.arxiv.org/help/submit_tex.html and https://info.arxiv.org/help/submit/index.html.

The CI artifact now also includes **`arxiv-upload-source.zip`**, which contains exactly one self-contained source file, `main.tex` — **no compiled PDF**, research data ZIP, author-approval metadata or extraneous log files. The source-only build converts the Pandoc-generated H1 to a conventional `\\title`, `\\author` and `\\maketitle` while preserving the body and release-grounded numeric tables. It includes the existing substantial AI-assistance disclosure. Because the candidate contains no figures, external `\\input` files or separate BibTeX sources, no additional files are required for this current snapshot.

GitHub CI **extracts that ZIP into a clean directory and compiles the sole `main.tex` using XeLaTeX**, then checks the generated PDF for the author name, disclosure, and main numerical result. This establishes an independent *technical source-build preflight*, not acceptance on arXiv's own TeX Live environment. arXiv offers XeLaTeX as a supported processor as of late 2025; at the submission screen select `xelatex` if arXiv does not infer it. **Review arXiv's own rebuilt PDF before publishing.** See https://info.arxiv.org/help/faq/texlive.html.

To verify the upload bundle locally:

```bash
python3 scripts/package_arxiv_source.py verify path/to/arxiv-upload-source.zip
unzip -l path/to/arxiv-upload-source.zip
```

Only upload the `arxiv-upload-source.zip` **after** a human author has reviewed the text, verified authorship/affiliation and third-party rights, chosen the paper distribution license, and authorized submission. The source candidate remains a draft with its name/metadata unconfirmed and must not be treated as a public preprint or an assigned identifier.

## arXiv preparation (candidate, not submission)

Primary source: [arXiv submission guidelines](https://info.arxiv.org/help/submit/index.html), [TeX source guidance](https://info.arxiv.org/help/submit_tex.html), [endorsement](https://info.arxiv.org/help/endorsement.html) and [licenses](https://info.arxiv.org/help/license/index.html).

**Suggested initial subject to review:** Computer Science / Distributed, Parallel, and Cluster Computing (cs.DC). Potential cross-list to cs.CR if the author and arXiv taxonomy agree. These are suggestions, **not** a verified account entitlement or a classification assigned by arXiv.

Use the generated `.tex` source (not solely a PDF if TeX is available), with only the supporting files required to build. The paper must be reproducible in the archive's LaTeX environment, and the author must inspect the PDF for missing characters, broken links, tables, and page overflow. The current generated source is a draft and still needs author editorial review. arXiv moderators decide subject fit; first-time/new-category submitters may need endorsement. arXiv requires a chosen **irrevocable paper license** and submitter rights certification; software and paper licensing are separate.

Suggested arXiv metadata (pending review):

- **Title:** AivoBlock: A Reproducible Permissioned Blockchain Prototype for Partitioned, Deterministically Verifiable AI Work
- **Author:** Sayed Jihad ElSayed — legal formatting, affiliation and contributor list **unconfirmed**.
- **Possible categories:** cs.DC (primary); cs.CR (cross-list).
- **Paper status:** research prototype characterization; *not* peer reviewed.
- **Keywords:** permissioned blockchain, proof of useful work, verifiable AI inference, deterministic quantization, reproducible experiments.
- **Comments (suggested):** Reproducible research prototype; source and research evidence: https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype

Do not label the first upload "version of record", add a DOI that does not exist, or call it accepted by a conference.

## Zenodo and permanent evidence archiving

Official: [new upload](https://help.zenodo.org/docs/deposit/create-new-upload/), [license selection](https://help.zenodo.org/docs/deposit/describe-records/licenses/), and [GitHub integration](https://help.zenodo.org/docs/github/enable-repository/).

A paper record and research-software/data record are different outputs and may use different rights statements. Zenodo normally assigns a DOI **when a record is published**, not merely when this file is created. You may also reserve a DOI in an unpublished Zenodo draft.

To give the already-published v0.1.0 software snapshot a Zenodo release DOI through GitHub integration, the author must connect and enable the repository and consider creating a **new** release after integration; prior GitHub releases are not necessarily imported retroactively. The new release must not overwrite or relabel v0.1.0's original experimental evidence. The author should choose suitable licenses or custom license terms rather than accepting Zenodo's default CC-BY grant for material they do not control.

## Venue and AI assistance policy

The repository has been extensively developed and the manuscript substantially drafted with ChatGPT assistance; that fact is disclosed in the paper. **OSF Preprints' August 2026 policy states that content completely or mostly generated by LLM tools is not appropriate for OSF Preprints.** We therefore do *not* recommend depositing this AI-drafted version with OSF. Review the actual selected venue's current policy, revise the paper under the human author's scientific judgment and disclose AI use as required.

## Final review and follow-up

Until approvals are complete, `submission-metadata.json` must preserve `publicly_submitted:false`, empty DOI/arXiv identifiers and all pending approval flags. The preprint build intentionally checks that unpublished status.

Once approved, the **human author** can open an arXiv account/submission or a Zenodo draft, provide the paper and correct metadata, and review the host's license/submission agreement. An actual public posting and DOI must be independently verified at its permanent public URL before milestone #111 can be marked as *submitted*. Passing CI means only that the review package built and passed its internal consistency checks.
