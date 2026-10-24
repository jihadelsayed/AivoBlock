# ACMSE 2027 — Final Poster Submission Handoff

**Status: NOT SUBMITTED.** This is a practical checklist for the **Posters** track, not a statement of acceptance. The review PDF must remain anonymous; the submission system separately records true author metadata.

## 1. Confirm the GitHub build
- Open the [official-template review workflow](https://github.com/jihadelsayed/AivoBlock/actions/workflows/acmse-poster.yml).
- Require a **successful run on the latest main commit**. Ignore PDFs and artifacts from older unsuccessful runs.
- Download the uploaded `acmse-2027-anonymous-poster-review` artifact from that run.
- Use only `poster-abstract.pdf` from the successful official-template build (do not submit the earlier local preview).
- Confirm the artifact's logged official-template checksum and exact class version. The workflow must have downloaded the [mandatory ACMSE 2027 template](https://acmse.net/2027/papers-format/).

## 2. Independently inspect the PDF
- Open and visually inspect both pages: no clipped text, off-page citations, missing symbols, or content spilling beyond page two.
- Verify US letter size and exactly **two pages including references**.
- Verify only Linux Biolinum/Libertine fonts are used as specified by ACMSE.
- Confirm the author name, affiliation, email, institution and acknowledgments are absent from the **review PDF**, including PDF Author metadata.
- Confirm full given names in bibliography, corrected author ordering, live citations, CCS Concepts and Title Case keywords.
- Confirm the measurements remain descriptive and tied to fixed AivoBlock v0.1.0 experimental evidence.

## 3. Resolve the remaining scholarly and policy questions
- Ask ACMSE poster chairs how to reference a public, author-identifying AivoBlock GitHub repository and any arXiv preprint under double-anonymous review. Do **not** rewrite public Git history or claim independent prior publication.
- Personally read and approve the paper, all referenced sources and substantial generative-AI assistance disclosures; check the conference's exact AI disclosure policy.
- Confirm all contributors' credit, your genuine affiliation and your ORCID; none of these are automatically verified by CI.
- Confirm the draft constitutes original research and does not overclaim permissionless consensus, cryptographic proof of general AI computations, energy savings or cross-hardware replication.

## 4. Submit through EasyChair (author action only)
1. Open the verified [ACMSE 2027 EasyChair portal](https://easychair.org/conferences/?conf=acmse2027) (also linked by the [official poster call](https://acmse.net/2027/posters-call/)). Sign in or create an EasyChair account, then choose a new submission.
2. Select **Posters**, not the full/short paper track.
3. Enter **Jihad Elsayed** as the author's confirmed legal name in EasyChair (NOT in the anonymous PDF), plus an appropriate verified UAB affiliation and your real ORCID; review the separately tracked [submission metadata](submission-metadata.json). Other authors must be listed only when confirmed.
4. Paste the **same** title and abstract used in your reviewed PDF (if the system requests those fields).
5. Upload the **verified anonymous** `poster-abstract.pdf` from the successful official-template workflow.
6. Submit before **November 14, 2026**, save the receipt/submission ID, and record it only after the venue confirms submission.

Poster acceptance notification: **February 20, 2027**. Camera-ready date: **March 1, 2027**. Conference: **April 15–17, 2027**. An accepted poster must be registered and presented to appear in proceedings.

## 5. Publication identity

Anonymous is temporary and applies to the **review PDF**. ACMSE will record the author in the submission system; an accepted final camera-ready version includes the confirmed author name and affiliation. `submission-metadata.json` deliberately retains pending flags until verified human action.

**arXiv** is a separate research preprint repository. Do not treat a conference submission, GitHub file, or successful build as an arXiv publication.

## Current preparation status (updated 24 October 2026)

- The latest checked ACMSE official-template CI build passed; use a successful run on the *final* commit for the upload PDF, not an older artifact.
- Legal author name corrected to **Jihad Elsayed** in EasyChair metadata; preserve anonymity in the two-page review PDF.
- No EasyChair submission ID, acceptance, ORCID number, or camera-ready publication is claimed.

Official rules: [Call for Posters](https://acmse.net/2027/posters-call/) and [Papers Format](https://acmse.net/2027/papers-format/).
