# Step 18 — Targeted UAB Research Review and arXiv Endorsement Outreach

**Status: Prepared but NOT SENT. No independent review, endorsement, or response has occurred.**

## Recommended first contact — Prof. Ragib Hasan

**Why:** UAB Computer Science professor, SECRETLab founder, research in trustworthy computing, cloud security, secure provenance and networked systems. AivoBlock needs criticism of its validator trust assumptions, authenticated work assignment, persistence and reproducibility. His published expertise overlaps those questions. **His current arXiv cs.DC endorsement eligibility is NOT VERIFIED.**

Official UAB faculty page: https://www.uab.edu/cas/computerscience/people/faculty-directory/faculty/ragib-hasan  
UAB advanced cyberinfrastructure research cluster: https://www.uab.edu/cas/computerscience/research/research-clusters/advanced-cyberinfrastructure  
Contact listed on Professor Hasan's own website: https://ragibhasan.com/contact/ (the site lists `ragib AT cis.uab.edu`; **verify the address is current before sending**, because older department aliases may change).

**Email prepared:** [EMAIL_TO_HASAN.md](EMAIL_TO_HASAN.md). Keep it an individualized request for independent scholarly feedback. Mention possible arXiv endorsement only as an optional separate question; never suggest it is an entitlement or a condition for commenting on the research.

## Secondary researcher — Prof. Yuliang Zheng

**Why:** UAB Computer Science Professor and Chair, with published research interests in blockchain, cryptocurrency and modern cryptography. Potential reviewer of fixed-validator quorum assumptions, signature protocols, useful-work security claims and novelty boundaries. **His cs.DC endorser eligibility is also UNVERIFIED.**

Official UAB faculty page: https://www.uab.edu/cas/computerscience/people/faculty-directory/faculty/yuliang-zheng  
UAB research areas: https://sites.uab.edu/yzheng/research-areas/

Do not send a bulk endorsement request to multiple faculty. Approach one person respectfully, and if necessary select another after receiving a response or allowing a reasonable interval. No follow-up has been scheduled.

## Attachments and claim boundaries

Before asking anyone to review, share:
- [The canonical paper](../paper/aivoblock-manuscript.md) — approximately 10 source-compiled pages, currently a **draft**, not a peer-reviewed publication.
- [External technical review guide](README.md) and [review form](REVIEW_FORM.md) — optional guide to code and experimental checks.
- [Released v0.1.0 experiment and data](https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype) — original source `ade6ff42737d972658e9c309f9a2d8cffbfb988c`, run `39740779280`.
- [Latest arXiv preparation checklist](../../submissions/arxiv-cs-dc/CHECKLIST.md).

Ask for specific feedback on **(1) protocol correctness and Byzantine assumptions, (2) the distinction between full integer recomputation and proof of useful work, and (3) whether the analysis and prior-work comparisons support a meaningful, modest research contribution**.

The paper states that GitHub CI reanalyzed archived measurements but **did not independently rerun** the 270-task live experiment on another machine. Do not describe any future faculty comments as a completed review or replication without their actual contribution and consent.

## arXiv endorsement: do not assume professor eligibility

Current arXiv policy: https://info.arxiv.org/help/endorsement.html

1. Sign in to arXiv and initiate a new submission under `cs.DC` to obtain **the actual endorsement email/request link**, if the account needs endorsement. Do not invent a six-character code.
2. arXiv permits checking potential endorsers through its **“Which authors of this paper are endorsers?”** link on relevant arXiv articles. A professor's title or publication record elsewhere does not prove active endorsement eligibility.
3. Provide an official arXiv-generated request only if the professor is eligible and willing to consider it. An endorsement is not coauthorship, scientific peer review, or automatic acceptance.
4. Respect arXiv's warning against mass endorsement emails and repeated follow-up requests.

## Tracking

- Main tracker: [STATUS.json](STATUS.json). The current value `external_reviewer_contact_status: not_requested` **must remain unchanged until a request is actually sent**.
- Planned candidates are recorded as *not contacted*, *endorsement eligibility unverified*, and *no responses*.
- If the author sends an email, record the actual date and an appropriate private reference. **Do not publish a professor's private reply or personal details to this public repository without permission.**
- The ACMSE 2027 double-anonymous preprint/GitHub-policy inquiry is separate; neither reviewer feedback nor arXiv endorsement resolves ACMSE policy.
