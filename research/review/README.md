# AivoBlock — External Technical Review Packet (Step 17)

**Prepared: 23 October 2026. No independent researcher has been engaged or completed a review.** This package is a request for criticism, not evidence of peer review, endorsement, or replication.

## Research contribution and limits

The paper, [AivoBlock: A Reproducible Permissioned Blockchain Prototype for Partitioned, Deterministically Verifiable AI Work](../paper/aivoblock-manuscript.md), evaluates a small **permissioned** blockchain prototype that uses signed partition assignments for integer AI inference. Python workers produce claims, Rust validators **fully recompute** the inference, a greater-than-two-thirds fixed-set quorum finalizes blocks, and a Haskell core independently checks deterministic settlement.

Questions for a reviewer: *Does this implementation do what the paper says?* Are there security failures or missing baselines? Are experimental interpretations and novelty boundaries sufficiently narrow? It does **not** demonstrate permissionless proof-of-work, succinct proof of arbitrary ML computation, token-economic performance, or live-experiment replication on another machine.

The originally published evidence is **not updated by this review**:

- Release: https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype
- Source revision: `ade6ff42737d972658e9c309f9a2d8cffbfb988c`
- Producer CI run: `39740779280`
- Evidence archive SHA-256: `a489c37ef5f14c68e08b623eb01477cbc4507ed7b7326cddfcaf9a75745ef414`
- Independent *archived-data reanalysis* report: https://github.com/jihadelsayed/AivoBlock/releases/download/v0.1.0-research-prototype/independent-audit.json

## Review procedure A — Check the original evidence (no live network)

Prerequisites: Python 3.11+, Git, GitHub CLI authenticated if needed for release downloading; `sha256sum` or equivalent. Follow [the canonical reproducibility instructions](../../docs/research-reproducibility-package.md).

```bash
git clone https://github.com/jihadelsayed/AivoBlock.git
cd AivoBlock
git checkout ade6ff42737d972658e9c309f9a2d8cffbfb988c
mkdir -p publication
gh release download v0.1.0-research-prototype \
  -R jihadelsayed/AivoBlock \
  -p aivoblock-research-evidence.zip -D publication
sha256sum publication/aivoblock-research-evidence.zip
python3 scripts/package_research_evidence.py verify \
  publication/aivoblock-research-evidence.zip
python3 scripts/audit_public_evidence.py \
  --archive publication/aivoblock-research-evidence.zip \
  --out publication/reviewer-reanalysis.json
```

Compare the downloaded archive SHA-256 to the fixed value above. Record the audit output and any discrepancy. This work checks archived observations and manifests. **It is not an independent execution of the original 270 blockchain tasks.** If the audit produces a numerical mismatch, describe the exact archive member, statistic and code revision rather than replacing values silently.

## Review procedure B — Independently run the private protocol (new experiment)

Use a separate trusted environment with Docker Compose v2 and Python 3.11+ and the released revision. The Docker images build Rust, Haskell and Python components (see [testnet guide](../../docs/testnet.md)). Preserve local CPU architecture/model, OS, Docker/Compose versions, commit, UTC timestamp, resource limits, and exact commands.

```bash
git checkout ade6ff42737d972658e9c309f9a2d8cffbfb988c
python3 scripts/testnet.py demo
python3 scripts/testnet.py fault
# or both suites together:
python3 scripts/testnet.py all
```

Capture stdout/stderr and the generated report paths, redact test credentials from shared logs, record failures and local environment details. **Do not present these short smoke/fault scenarios as a reproduction of the paper's exact 270-task timed experiment.** A full independent timing study must reproduce the paper's three conditions, 3 × 30 observations each, with aligned poll rates, model fixtures, saved raw observations and hardware fingerprints, and should publish its own run data. CI commands for those experiments appear in [the source workflow](../../.github/workflows/ci.yml) and the [protocol/testnet documentation](../../docs/testnet.md).

## Review procedure C — Security and correctness questions

Evaluate the evidence for each issue, recording either a minimal failing test or an explicit *not tested* outcome:

1. **Membership/authentication:** Can an unauthorized peer, worker or task source inject claims, votes, or replayed transactions? Are identity and task commitments bound correctly?
2. **Inference correctness:** Do invalid integer outputs, swapped partitions or altered model/input commitments get rejected? Are overflows, quantization/rounding and canonicalization fully specified?
3. **Consensus safety:** Can one validator vote for conflicting blocks at the same height/round? Are persisted locks restored after restart? Is a >2/3 certificate enough under the actual assumptions? No formal safety theorem is supplied.
4. **Liveness:** What happens when a leader equivocates or validators are partitioned? The README documents crash/restart tests, not unconditional Byzantine liveness.
5. **Reward settlement:** Can duplicate task IDs, reordered partitions, inconsistent Haskell/Rust results, or bad fee/issuance fields cause double payments?
6. **Persistence/recovery:** Do restarts and long catch-up preserve historical commitments, state roots and one-block-per-height properties?
7. **Threat boundaries:** Peer frames are authenticated/encrypted, but the local control API and compromised endpoints are outside the claimed security boundary.

Relevant sources: [architecture](../../docs/architecture.md), [testnet](../../docs/testnet.md), [protocol specification](../../protocol/spec/testnet-consensus-v1.md), and [paper threat model](../paper/aivoblock-manuscript.md).

## Review procedure D — Scientific interpretation

Ask reviewers to challenge these claims rather than merely confirming that CI is green:

- The 90 trained-model chain observations gave **730.760 ms median** and **779.342 ms p95** *client-observed* request-to-finality with 100 ms polling. Do not interpret this as internal consensus latency.
- The **1.880 ms** CPU-only baseline measures a narrower interval. A direct speedup/overhead ratio is not supported.
- The **0.742 ms** direct-inference / **3.412 ms** Rust verifier values are single-process microbenchmarks with additional verifier work; no network comparison follows.
- Shared CI hardware and three fresh launches per scenario do not establish independent laboratory replication, cross-hardware performance or statistical independence of all tasks.
- Prior work includes training-based useful mining and newer inference-based proposals. The defensible contribution is the released *specific implementation and descriptive analysis*.

## Review outcomes and responsibility

Use [REVIEW_FORM.md](REVIEW_FORM.md) to document a reviewer identity/affiliation only with consent, exact source revision, review date, testing environment, reproduced data, specific concerns, and recommendation. A review may be private or public at the reviewer's discretion. **AivoBlock maintainers must not pre-fill a favorable verdict or impersonate outside review.**

[STATUS.json](STATUS.json) tracks *requests and evidence of external responses*, initially pending. A source-only checker ensures it does not silently assert completed review without a report. Changes to the status file must be based on actual communications/artifacts and should not be interpreted as arXiv endorsement or ACMSE acceptance. Human author sign-off, `cs.DC` endorsement, and [ACMSE anonymous-review guidance](../../submissions/acmse-2027/CHAIR_INQUIRY.md) are distinct additional blockers.
