# AivoBlock — Independent Reviewer Response Template

**Blank form. No review has been conducted by virtue of this template.** A reviewer should write the answer from their own inspection/testing and may choose to remain unnamed in any public version.

| Field | Reviewer fills in |
| --- | --- |
| Reviewer name / pseudonym | |
| Organization / relationship to authors (optional) | |
| Review date and timezone | |
| Conflict of interest / prior contribution | |
| Manuscript/source commit audited | |
| Evidence archive SHA-256 verified | |
| Hardware / OS / Docker and Python versions | |
| Archived-data reanalysis command and results | |
| Live testnet command, raw logs and result location | |
| Tested consensus and adversarial scenarios | |
| Reviewer conclusion and confidence | |
| Permission to share review or excerpts | |

## Findings

**1. Scope of review.** Specify what you read or ran. Mark each as `Not attempted`, `Checked without discrepancy`, `Discrepancy`, or `Unable to assess`: archived data, trained model/quantization, Rust verification, voting/persistence, Haskell settlement, worker/task authentication, network timing, manuscript references.

**2. Reproduction detail.** For each attempted test, give the exact commit, commands, environment, exit code, relevant report path and independently computed numbers. When a metric was not rerun, label it *publisher's archived result* instead of *independently reproduced*.

**3. Critical findings.** Include issue severity (`critical/high/medium/low`), exact steps/input to reproduce, expected result, actual result, and relevant code lines. Do not include secret keys or sensitive private logs.

**4. Scientific claims.** Is the claim narrow enough about permissioned membership, full validator recomputation, lack of production scalability, lack of permissionless proof-of-work, poll-bound finality, and the CPU-baseline mismatch? Identify any overstatement.

**5. Literature and presentation.** Flag missing prior work, inaccurate citations, unclear timing methodology, inappropriate inference, or disclosure gaps. State whether the paper's contribution is distinguishable from existing useful-work/inference approaches.

**6. Decision.** Choose `major changes recommended`, `minor changes recommended`, `no major concerns in the reviewed scope`, or `unable to evaluate`; explain. This is external feedback only, not a formal conference or arXiv acceptance.

**7. Reviewer consent.** State whether the author may quote, publicly share, or link this report, and whether you want identification removed.

The AivoBlock maintainer should record any real report in STATUS.json only after receiving it, ensuring that received feedback and tested scope are represented accurately.
