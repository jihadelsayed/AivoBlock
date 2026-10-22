# AivoBlock: A Reproducible Permissioned Blockchain Prototype for Partitioned, Deterministically Verifiable AI Work

**Manuscript version:** 0.1, 22 October 2026 — working research-paper draft, **not** submitted or peer reviewed.  
**Author:** Sayed Jihad ElSayed (AivoBlock project); contributions and code history remain attributable to repository contributors.  
**Artifact release:** [AivoBlock v0.1.0 research prototype](https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype).  
**Evidence source:** GitHub Actions run [39740779280](https://github.com/jihadelsayed/AivoBlock/actions/runs/39740779280), commit `ade6ff42737d972658e9c309f9a2d8cffbfb988c`.  
**Keywords:** verifiable computation; useful AI work; permissioned blockchain; deterministic quantization; Byzantine fault tolerance; reproducible experimental systems.

> **Evidence policy.** All numeric experimental results in this draft refer to the above fixed **published release**, not an earlier measurement or future rerun. Values are descriptive. The manuscript does **not** establish a novel proof-of-work security mechanism, efficient proof of arbitrary AI computation, token economic viability, or production performance. Literature coverage and formal novelty claims require the separate #110 review.

## Abstract

Useful machine-learning computation is an appealing alternative to hash-puzzle expenditure, but reproducibility of floating-point inference, verification cost, and ledger agreement complicate proposals that couple AI work to blockchain incentives. We present **AivoBlock**, an implemented, permissioned research prototype in which authority-defined, row-partitioned integer inference tasks are executed by Python workers, independently recomputed by Rust validators, finalized by a fixed-set quorum protocol, and settled by deterministic Haskell ledger transitions cross-checked in Rust. Off-chain PyTorch training produces a quantized QMLP-V1 model whose exact integer inference is eligible for on-chain validation; floating-point PyTorch and ONNX results remain outside consensus. A published, integrity-checked research archive records 270 client-observed chain tasks across three 90-task conditions, 90 CPU-only service executions, benchmark repetitions, and 90 archived trained-model finalized-block records. In the trained-model condition, 90 requests across three fresh three-validator testnets had a pooled client-observed request-to-finality median of **730.760 ms** and nearest-rank p95 of **779.342 ms** under 100 ms status polling. The separately timed CPU service had a median of **1.880 ms**, but the two intervals have different trust and timing boundaries and **must not be interpreted as a speedup or overhead ratio**. For a matched, single-process 256-row/64-partition QMLP microbenchmark, direct integer inference averaged **0.742 ms** and Rust full-block verification averaged **3.412 ms** over three benchmark invocations. A separate CI runner independently reanalyzed archived data and passed. These findings demonstrate end-to-end integration of deterministic AI execution, permissioned finality and reward settlement for small workloads, while exposing verification and measurement costs; they do not demonstrate permissionless proof of useful work or independent hardware replication.

## 1. Introduction

Hash-based proof of work (PoW) consumes computation to make a ledger history costly to replace [1]. Proposals to reuse blockchain-related computation for externally useful machine learning are not new: Coin.AI [3] and the AI useful-work proposal by Lihu et al. [4] predate AivoBlock. A practical research question is narrower than whether AI can *replace* PoW generally: **can a small, explicitly verifiable AI task be assigned to workers, authenticated, checked independently, finalized, and rewarded without relying on agreement about nondeterministic floating-point output?**

AivoBlock is designed as a prototype and experimental instrument, not a deployed public network. It separates the worker's claimed computation from validators' verification and from quorum-based ledger agreement. In its current implementation the compute is genuinely executed as a deterministic integer-model task, yet validation is **full recomputation**, not cheaper cryptographic proof. The permissioned validator membership and fixed authority also mean that this task execution does not provide permissionless Sybil resistance or PoW-style difficulty.

This paper asks four research questions:

- **RQ1 (functional feasibility):** Can separate Rust, Haskell and Python components complete and agree upon a signed, partitioned inference task and its rewards on a multi-node private network?
- **RQ2 (determinism):** Can off-chain-trained model parameters be quantized into an inference representation whose outputs agree exactly between the Python reference, Rust validator and tested integer ONNX reference?
- **RQ3 (cost):** What computation and verification costs occur for fixed QMLP workloads and how do partition count and persistent settlement IPC affect measured microbenchmark latency?
- **RQ4 (observability):** What do repeated, *client-observed* blockchain finality timings show under explicitly bounded polling conditions, and which stronger causal or performance claims are unsupported?

**Contributions are limited to:** (i) a working cross-language, permissioned AI-work chain implementation; (ii) a deterministic QMLP task-to-finality pipeline; (iii) published testnet and CPU observation data with integrity and commitment checks; and (iv) independent *reanalysis* of the archived measurements on a separate CI runner. Novelty compared with earlier AI/PoUW systems and formal security claims are reserved for further analysis.

## 2. System design and threat model

### 2.1 Components and task lifecycle

AivoBlock contains a Rust node responsible for authenticated validator communication, task commitments, block construction, verification and durable RocksDB history; a Haskell core for deterministic issuance and settlement; and Python workers that execute assigned input-row partitions. Protobuf contracts exist for transport evolution, while the implemented private-testnet messaging uses bounded authenticated envelopes and versioned canonical data.

A fixed signed network configuration lists validators, voting power, allowed worker identities, issuance parameters and genesis information. An out-of-band pinned authority key authenticates the configuration. A client submits an authority-signed task with a committed model, committed batch of input rows, partition assignments, and fee context. Registered workers execute their assigned rows and return signed claims. The Rust validator verifies task/claim identities, signatures and commitments, **recomputes each assigned row's integer inference independently**, checks reward and Haskell-versus-Rust settlement equivalence, and votes on the proposed block and state transition. A quorum certificate and durable per-height vote lock guard finalization. RocksDB persists finalized blocks and state so nodes can recover after restart. The native **Sayed Token** is a test-ledger reward unit: workers receive assigned-row-based fee/issuance shares after finalization, not measured real-world compute payments.

The protocol is **permissioned**. It uses configured validators with a greater-than-two-thirds voting-power certificate; node recovery and leader crashes have been tested. The implementation does *not* provide open validator admission, stake/slashing, public difficulty adjustment, permissionless Sybil resistance, or unconditional Byzantine-leader liveness. Those properties should not be inferred from observed testnet convergence.

### 2.2 Deterministic AI representation

PyTorch trains a floating-point 32→32→10 two-layer multilayer perceptron on synthetic noisy 8×4 glyph inputs. Quantization produces QMLP-V1: int8 model weights/inputs with specified integer accumulation, bias, scaling, and requantization rules. Canonically encoded model/input/result commitments identify the exact inference workload. Workers' Python reference results are checked against a Rust implementation. The integer ONNX representation is tested for equivalence but validators execute the specified Rust integer routine rather than an ONNX runtime.

Ordinary floating-point PyTorch and float ONNX execution are **excluded** from consensus because different numerical kernels or execution orders can give different bit patterns. A network-wide requirement for exact consensus outputs is different from obtaining nearly identical prediction accuracy.

### 2.3 Trust and adversarial scope

The intended fault model includes malformed task or claim data, unregistered worker or peer identities, equivocation attempts, failed connections, crashed leaders and node restarts within the specified private testnet. Validator peers authenticate using configured Ed25519 identities and negotiate ephemeral encrypted sessions; encryption limits passive inspection of peer frames but does not secure compromised endpoints or the loopback-only operator control API. A successful local adversarial suite provides *tested behavior*, not a proof of all Byzantine executions. A task whose result is correct does not itself establish consensus security.

## 3. Experimental design and reproducibility

### 3.1 Fixed artifacts and hardware scope

The results are extracted from the published v0.1.0 artifact `aivoblock-research-evidence.zip`; its SHA-256 digest is:

```text
a489c37ef5f14c68e08b623eb01477cbc4507ed7b7326cddfcaf9a75745ef414
```

The bundle's `manifest.json` records source and tested checkout commit `ade6ff42737d972658e9c309f9a2d8cffbfb988c`, CI run `39740779280`, per-file hashes and byte sizes. Experiments ran on a GitHub Actions `ubuntu-24.04` x86-64 hosted runner using Python 3.12.15 and a Dockerized Rust/Haskell/Python node. Exact CPU SKU, uncontended CPU entitlement and cross-machine performance calibration were **not established**. Results are therefore specific to this environment.

The archive includes the base `research-out/report.json`, three independent model/data-seed reports (11, 17, 23), verifier microbenchmark JSON from three separate invocations, 90 CPU-service records, three 90-observation chain scenarios, three accompanying phase summaries, and the 90 trained-model finalized-block JSON records. Separate GitHub Actions jobs create the bundle and independently download/reanalyze it. The independent audit verifies file digests and experimental identities and recomputes descriptive statistics; **it does not rerun the experiment on independent machines**.

### 3.2 Microbenchmarks and training

The base model uses 2,000 synthetic training examples and 500 synthetic test examples, with 200 training epochs. We report accuracy and agreement only on that generated dataset. The three additional seed pairs vary dataset construction and training initialization, but not the generator family. The verifier microbenchmark uses **256 input rows, 64 partitions and a 32→64→16 integer QMLP**, with three separate benchmark invocations, each averaging 20 inner iterations. This verifier fixture is distinct from the trained glyph model and from the 12-row finality workload; their latencies should not be combined.

The matched single-process alternatives are: direct QMLP integer inference; Rust-mirror full-block verification (signatures, commitments, recomputation and Rust-only settlement mirror); and full-block verification with persistent Haskell settlement. These tests do **not** include live voting, network transit or finality. One-shot Haskell IPC comparisons separately evaluate fresh process creation on each verification call.

### 3.3 Observed chain finality and CPU service

Three chain scenarios each submit **30 QMLP tasks over three independently initialized three-validator private testnets** (90 observations per condition, 270 total). Each task has 12 rows:

1. Seeded-node QMLP fixture with **1.0 s** status polling.
2. Same fixture family with **0.1 s** status polling in separately launched testnets.
3. The off-chain **trained, quantized QMLP export** with **0.1 s** polling in separately launched testnets.

For every request, a monotonic `time.perf_counter_ns` clock records the interval before CLI submission, after the CLI returns, and after the client observes the target finalized height on all validators. Thus:

- **CLI submission phase:** before-submit → CLI-return.
- **Post-submit observation phase:** CLI-return → target finalized height observed.
- **Total client-observed duration:** sum of those phases.

The observation phase includes worker execution, networking/consensus and **status polling**. It is neither consensus-only latency nor the true internal finalization instant. The 1.0 s and 0.1 s experiments are sequential separate-network series rather than randomized matched trials; observed differences cannot establish causal effects of polling.

The separate CPU-service baseline records 30 executions in each of three launches (90 total) with monotonic timings around **in-process, synchronous service execution**. It excludes peer messaging and consensus and uses a *different timing boundary*. Even where the model/input/result identities match, comparing that number directly to client-observed finality would confound verification, protocol, process startup, scheduling, and measurement boundaries.

### 3.4 Analysis

For each raw phase, we compute the pooled mean, median, sample standard deviation and **nearest-rank p95** after checking unique request identifiers, finalized heights, timestamps, polling and workload metadata. We also report medians per fresh testnet launch. Pooled observations within one launch share resources and must not be treated as independent network deployments or used to claim a population-level confidence interval. Independent archive analysis on a second runner recomputes every reported descriptive statistic and checks observed finalized-block identities against saved records.

## 4. Results

### 4.1 Deterministic model and integration

| Property (published base model) | Observed result |
| --- | ---: |
| Synthetic training / test examples | 2,000 / 500 |
| PyTorch float-model test accuracy | **94.6%** |
| Quantized QMLP-V1 test accuracy | **94.8%** |
| Agreement between quantized and float predictions | **99.8%** |
| Integer ONNX vs QMLP-V1 reference mismatches | **0/500** |
| Maximum absolute float ONNX–PyTorch logit difference | **0.0000076294** |

These are not benchmark claims about real-world classification, other datasets, training acceleration or all floating-point hardware. Three additional seed-pair experiments recorded float accuracies **95.8%, 96.0%, 95.8%** and quantized accuracies **96.0%** in each run, with zero integer-ONNX mismatches in the archived analysis. The higher replicated accuracies should not be merged statistically with the original baseline as if they were one randomized experiment.

The private three-node integration demonstration finalized the trained model task and distributed **80 test Sayed Tokens** to three registered workers as **27, 27 and 26**, respectively, in the recorded release run. A four-validator scenario exercised crash/restart and leader recovery. In the released 90 trained-QMLP-task archive, all 90 finalized-block records expose model, input and result commitment fields, which the fresh-runner auditor checks against the workload and task identities. This establishes consistency of the *archived* evidence with the tested protocol rules, not independent replay by an unrelated laboratory.

### 4.2 Controlled, single-process verifier overhead

The **256-row / 64-partition** QMLP microbenchmark records three separate invocation means:

| Timed path | Mean of invocation means (ms) | Sample SD across three invocations (ms) |
| --- | ---: | ---: |
| Direct integer QMLP inference (same workload) | **0.742** | **0.0008** |
| Rust-mirror full-block verification | **3.412** | **0.012** |
| Persistent Haskell cross-checked full-block verification | **8.282** | **0.584** |
| Fresh Haskell process full-block verification | **18.757** | **0.345** |

Under this *local controlled workload*, Rust full-block verification averaged approximately **4.60×** direct integer inference; Haskell persistent cross-checked verification averaged **11.16×** direct inference. These ratios compare measured **in-process computational paths with different verification semantics**, not end-to-end blockchain overhead, speedup relative to ordinary AI services, electricity usage, or mining profitability.

Persistence avoids much process-creation cost. The three invocation-level Haskell one-shot/persistent latency ratios averaged **2.27×** for the QMLP benchmark. The partition sweep measured a Rust verification increase from approximately **0.566 ms** with one partition to **3.404 ms** with 64 partitions on a fixed 256-row QMLP fixture, roughly **6.02×**. This shows measurable signature/claim bookkeeping overhead, *not* that distributing work over more workers speeds useful computation.

### 4.3 Client-observed finality, 3 × 90 observations

All figures are milliseconds measured on the **published main-branch release run**.

| Workload / client polling | n (launches × tasks) | Mean total | Median total | p95 total | SD total |
| --- | ---: | ---: | ---: | ---: | ---: |
| Seeded QMLP, 1.0 s | 90 (3 × 30) | **1644.585** | **1639.678** | **1692.492** | 28.695 |
| Seeded QMLP, 0.1 s | 90 (3 × 30) | **718.529** | **724.717** | **778.375** | 44.390 |
| Trained QMLP, 0.1 s | 90 (3 × 30) | **728.115** | **730.760** | **779.342** | 45.925 |

For the trained 0.1 s series, the *pooled* median CLI phase was **132.936 ms** and the median post-submission observation phase was **587.252 ms**. Their medians need not sum to the median total, because medians are computed separately over non-identical orderings of tasks. The three **per-launch trained-model total medians** were **736.480**, **712.849**, and **738.813 ms**, showing variation even across fresh testnets on the same runner environment.

The seeded 1.0 s versus 0.1 s difference is descriptive only. Both intervals incorporate polling and trials occurred in different testnets/periods. A legitimate causal polling analysis would randomize run order, repeat matched tasks and control runner contention.

### 4.4 CPU-only execution: a different clock boundary

The separately measured trained-model CPU-only service executed **90 calls** with **mean 1.887 ms**, **median 1.880 ms** and **nearest-rank p95 1.960 ms**. The service timing excludes full request transport, consensus and finality observation; the chain timing includes them. Therefore this paper intentionally **does not divide chain duration by CPU duration** and does not call that quotient a blockchain penalty, speedup or PoUW efficiency estimate.

### 4.5 Evidence integrity and reproducibility

The published archive contains **147 ZIP entries**, including `manifest.json`, reproduction instructions and **145 evidence entries**; each evidence entry has a declared SHA-256 and byte size. The archive integrity builder checks recorded source hashes and service artifact bytes. The separate fresh-runner auditor recalculates the 270 chain observations' descriptive statistics, rechecks 90 CPU observations and verifies task, finalized height and commitment relationships. GitHub Actions run **39740779280** completed all core jobs, the independent audit and the prerelease publisher successfully.

These checks protect against inconsistent reports and detect tampering relative to the manifest. The manifest alone is not an external trusted timestamp or a substitute for public third-party review. The published GitHub release is a source snapshot and data artifact, **not** a journal publication or independent cross-laboratory replication.

## 5. Discussion

**Computational usefulness is conditional on a task consumer.** Executing a real integer model produces a legitimate, reproducible inference output, but this experiment does not show paying clients, market demand, power efficiency, improved model quality, or a security-economic incentive relative to conventional blockchains. The native reward schedule illustrates accounting, not cryptocurrency valuation.

**Verification cost is the main systems obstacle.** Full recomputation means every validator must perform the small AI workload. Partitioned workers increase the number of authentic claims and bookkeeping checks without eliminating validator inference cost. This may be acceptable for small deterministic jobs in a controlled federation; it does not solve scaling for large neural networks.

**Consensus security is independent of useful output.** Fixed validator identities, precommit locks and greater-than-two-thirds quorum certificates are a research consensus mechanism inspired by Byzantine state-machine replication [2], not an original replacement for hash-based open membership. Successful tests under a handful of nodes do not establish safety/liveness under all adversarial schedules or security of an unrestricted public chain.

**Quantization trades expressivity for reproducibility.** QMLP-V1 integer arithmetic permits exact checks in the tested pipeline. Floating-point inference outside consensus helps avoid numerical divergence, but the resulting model class and batch size are tightly bounded. The observed small synthetic-data accuracy differences do not generalize to larger ML models.

**A more meaningful performance question remains open.** A fair comparison would align trust and timing boundaries between an optimized non-blockchain distributed inference service and the full AivoBlock protocol. It would also measure separate worker time, validator recomputation, network and consensus phases using synchronized instrumentation, not infer them from client polling.

## 6. Threats to validity and scope limits

1. **External validity:** Small synthetic digits, small quantized models, 12-row finalized tasks and fixed 3/4-node permissioned networks are not production workloads or a geographically distributed deployment.
2. **Statistical validity:** 30 tasks share one network launch; three launches share a CI provider. The three microbenchmark invocations each average 20 iterations. No independent-machine confidence interval or broad population inference is justified.
3. **Measurement validity:** Client-observed finality includes subprocess startup and polling. CPU service latency uses a narrower, different boundary. End-to-end throughput, energy consumption, operating cost and network-only/consensus-only time were not measured.
4. **Security validity:** No permissionless Sybil resistance, difficulty mechanism equivalent to Bitcoin's PoW, independent formal verification, production audit, or guarantee of progress under arbitrary Byzantine leader behavior is shown.
5. **Experimental reproducibility:** Separate-runner audit verifies existing archived observations and computations; it did not independently regenerate models and retest the network on separate laboratory hardware.
6. **Originality:** Earlier AI/useful-work blockchain proposals exist [3,4], so the claim is a concrete reproducible systems artifact, not proof that the idea of AI-related useful-work mining was invented here.
7. **Publication and license:** A public all-rights-reserved source repository does not by itself grant software reuse rights. This manuscript draft has no peer-review status, DOI or accepted-paper citation.

## 7. Future work

A stronger follow-up should (i) run independent geographically/hardware-separated testnets and publish new data with system fingerprints; (ii) build a genuinely matched non-blockchain distributed service with equivalent verification trust assumptions; (iii) instrument actual consensus phases separately from client polling; (iv) evaluate larger models and succinct, probabilistic or trusted-hardware verification tradeoffs; (v) increase faulty/Byzantine validator coverage, admission decentralization and reward-model realism; and (vi) perform the formal related-work and novelty analysis before public preprint submission.

## 8. Conclusion

AivoBlock v0.1.0 demonstrates a **working permissioned prototype** integrating trained-and-quantized AI tasks, partitioned worker execution, independent integer verification, quorum-based finalization, and deterministic reward settlement. The published evidence documents repeatable local execution and client-observed timings with an independently verified analysis pipeline. In the trained QMLP condition, 90 finalized tasks had a **730.760 ms** median client-observed request-to-finality time under 100 ms polling, while a separately timed CPU-only service measured **1.880 ms** median under a narrower boundary. Neither number implies PoW-equivalent network security or a meaningful service-versus-chain performance ratio. The contribution is a bounded, reproducible implementation and characterization of its limits, providing a platform for rigorously comparing useful-work verification designs in subsequent research.

## Artifact availability and reproducibility

- **Tagged release:** https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype
- **Experiment and CI run:** https://github.com/jihadelsayed/AivoBlock/actions/runs/39740779280
- **Exact source revision:** `ade6ff42737d972658e9c309f9a2d8cffbfb988c`
- **Research evidence ZIP:** https://github.com/jihadelsayed/AivoBlock/releases/download/v0.1.0-research-prototype/aivoblock-research-evidence.zip
- **Independent archive audit:** https://github.com/jihadelsayed/AivoBlock/releases/download/v0.1.0-research-prototype/independent-audit.json
- **Checksum file:** https://github.com/jihadelsayed/AivoBlock/releases/download/v0.1.0-research-prototype/SHA256SUMS.txt
- **Reproduction guide:** https://github.com/jihadelsayed/AivoBlock/blob/v0.1.0-research-prototype/docs/research-reproducibility-package.md

To verify the downloaded release:

```bash
git clone https://github.com/jihadelsayed/AivoBlock.git
cd AivoBlock
git checkout v0.1.0-research-prototype
python3 scripts/package_research_evidence.py verify /path/to/aivoblock-research-evidence.zip
python3 scripts/audit_public_evidence.py --archive /path/to/aivoblock-research-evidence.zip
```

**Disclosure:** The source release currently carries an **all-rights-reserved** notice, not an MIT license. Prior work and the paper itself require a final bibliographic and license review before dissemination as a preprint.

## References — provisional; full systematic review in milestone #110

[1] S. Nakamoto. “Bitcoin: A Peer-to-Peer Electronic Cash System.” 2008. https://bitcoin.org/bitcoin.pdf

[2] M. Castro and B. Liskov. “Practical Byzantine Fault Tolerance.” *Proceedings of the Third Symposium on Operating Systems Design and Implementation (OSDI)*, 1999, pp. 173–186. https://www.usenix.org/conference/osdi-99/practical-byzantine-fault-tolerance

[3] A. Baldominos and Y. Sáez. “Coin.AI: A Proof-of-Useful-Work Scheme for Blockchain-based Distributed Deep Learning.” arXiv:1903.09800, 2019. https://arxiv.org/abs/1903.09800

[4] A. Lihu, J. Du, I. Barjaktarevic, P. Gerzanics, and M. Harvilla. “A Proof of Useful Work for Artificial Intelligence on the Blockchain.” arXiv:2001.09244, 2020. https://arxiv.org/abs/2001.09244

[5] S. J. ElSayed and AivoBlock contributors. *AivoBlock v0.1.0: Research Prototype for Verifiable Partitioned AI Work on a Permissioned Blockchain* [Software and research data], 2026. https://github.com/jihadelsayed/AivoBlock/releases/tag/v0.1.0-research-prototype
