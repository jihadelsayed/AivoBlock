# Step 4 — Exact ACMSE Conference Template Build

ACMSE 2027 supplies a mandatory modified `sigconf` template. An ordinary installed ACM `acmart.cls` is **not sufficient** to certify final compliance. The [official download link](https://acmse.net/2027/wp-content/2027-templates/acmart-master-acmse2027-v0.1.zip) is linked from the [conference formatting page](https://acmse.net/2027/papers-format/).

## Workflow

1. [ACMSE poster GitHub Actions build](../../.github/workflows/acmse-poster.yml) runs when this submission folder changes.
2. [`build_official.sh`](build_official.sh) downloads the conference ZIP on a fresh runner and verifies it contains `sample-sigconf.tex`, `acmart.cls` and `ACM-Reference-Format.bst`.
3. It compiles `poster-abstract.tex` and `references.bib` using the actual downloaded conference class and bibliography style.
4. [`check_submission.py`](check_submission.py) validates review anonymity, two US-letter pages, expected citations, metadata, and documented experimental values. The PDF bibliography **must** show the corrected given names Andrei Lihu and Jincheng Du (not the stale preview's older names).
5. The workflow checks the conference's Linux Libertine/Biolinum fonts and uploads PDF, compilation log, bibliography and template download checksum as **review artifacts only**.

**No submission or camera-ready publication occurs automatically.**

## Build Locally

With curl, unzip, latexmk, BibTeX, PDF tools, and the Linux Libertine font family installed:

```bash
python3 submissions/acmse-2027/check_submission.py --source-only
bash submissions/acmse-2027/build_official.sh
```

The workflow uses the downloaded official template **on the build runner**; it does not vendor it into the repository. Confirm any venue-specific content in `sample-sigconf.tex` manually before using the generated PDF for EasyChair.

## Explicit blockers

- Remote conference template URL must be reachable and the GitHub build must pass. Container-side access to the conference ZIP has been blocked; this is not a completed local official-template comparison.
- Final author review and validation of the paper's novelty, references, experimental claims, and disclosures are necessary.
- ACMSE chairs should be asked how to handle links to the existing publicly identifiable AivoBlock code and preprint.
- The PDF must remain anonymous during review; real author information belongs in the EasyChair metadata. The original public Git history is never rewritten.
- Submission to EasyChair requires a separate deliberate action by the human author, an ORCID, and attention to the November 14, 2026 deadline.

Even a fully passing technical preflight cannot guarantee conference acceptance.
