# AivoBlock research results — 21 October 2026

**Status:** One successful CI run of a *permissioned private research testnet*. These results do not establish public-chain scalability, permissionless security, or cryptographic proofs of general AI execution.

## Evidence and reproducibility

- **Source:** [GitHub Actions run #367](https://github.com/jihadelsayed/AivoBlock/actions/runs/39592962720), PR [#64](https://github.com/jihadelsayed/AivoBlock/pull/64), tested commit `f53a10928bc55a9831c9d6b65e3b508f25916132`, subsequently merged as `6c5f4472b5556b889de9948ab54c34ed2ba9f633`.
- **Artifacts:** `testnet-reports` (GitHub artifact ID `12343485427`, files `bench.json`, `three-node/report.json`, `four-validator/report.json`) and `pytorch-onnx-research` (artifact ID `12343610296`, files `report.json`, `digits.qmlp.json`, `float-mlp.onnx`, `qmlp-v1-int64.onnx`).
- **Method:** `aivoblock-node bench --iterations 20 --sweep` in the Docker node image; off-chain CPU PyTorch experiment; separate Docker 3-node demo and 4-validator crash/restart scenario.
- **Environment:** GitHub Actions `ubuntu-24.04` x86-64 hosted runner, Dockerized Rust/Haskell/Python node; training with Python 3.12.15, PyTorch 2.14.1+cpu, ONNX 1.23.2, ONNX Runtime 1.31.0, NumPy 2.5.4. Exact CPU SKU, guaranteed resource isolation, frequency, and cross-machine calibration were *not* recorded.
- **Evidence quality:** **one machine/run**. Microbenchmarks use 20 iterations rather than independent repeated CI runs. Finality percentiles contain only 11 or 15 node-side samples. Values below are descriptive, not uncertainty-bounded population estimates.

Download the two artifacts and recompute the summary with Python's standard library:

~~~bash
gh run download 39592962720 -R jihadelsayed/AivoBlock -n testnet-reports -D testnet/generated
gh run download 39592962720 -R jihadelsayed/AivoBlock -n pytorch-onnx-research -D research-out
python3 scripts/analyze_research_results.py \
  --bench testnet/generated/bench.json \
  --research research-out/report.json \
  --three testnet/generated/three-node/report.json \
  --four testnet/generated/four-validator/report.json \
  --out testnet/generated/research-summary.json
~~~

The analyzer also checks that the trained model commitment in the research artifact matches the model finalized by the three-node testnet, that the integer ONNX output matched its reference, and that the audited worker rewards sum to the paid tokens. The CI Docker job now invokes the same analyzer and uploads `research-summary.json`.

## 1. Training and deterministic-model verification

| Measurement | Observed |
| --- | ---: |
| Dataset | Synthetic noisy 8×4 digit glyphs; 2,000 training / 500 test samples |
| Float PyTorch MLP | 32 → 32 → 10; 200 epochs |
| Float test accuracy | **94.6%** |
| QMLP-V1 int8 test accuracy | **94.8%** |
| Quantized vs float prediction agreement | **99.8%** |
| Float ONNX maximum absolute logit deviation vs PyTorch | 0.0000076294 |
| Integer ONNX mismatches against reference | **0/500** |

Float outputs were *not* bitwise identical. The model committed to the chain was the quantized integer QMLP-V1 model, not the float or ONNX runtime. Agreement on this dataset and machine does not prove general, cross-hardware reproducibility of float models.

The quantized model commitment in both artifacts was:

~~~text
8173ad1657270b8b3d980461a36ad33d7371cbcd1730a7f72343db4727f8d69e
~~~

## 2. Settlement and validator-verification latency

**Same 256-row / 64-partition benchmark inputs**. The first workload is integer-linear V1 with 32 features; the second is QMLP-V1 with 32 → 64 → 16 layers. All entries are microbenchmark mean milliseconds per full block verification, not observed consensus finality time.

| Path | Integer linear (ms) | QMLP-V1 (ms) |
| --- | ---: | ---: |
| Rust mirror only | 3.541 | 3.391 |
| Spawn a Haskell process per block, cross-checked | 18.809 | 19.157 |
| Supervised persistent Haskell process, cross-checked | **8.733** | **8.314** |
| Persistent / one-shot speedup | **2.15×** | **2.30×** |

The persistent Haskell IPC approach reduced this measured latency by **53.6% for integer-linear** and **56.6% for QMLP**, versus one process per block. However, it remained roughly **2.47× / 2.45×** the Rust-mirror-only diagnostic latency. The mirror-only mode is *not* equivalent to the cross-checked settlement configuration of the full testnet.

For the 256-row, 64-partition measurements, Rust-only verification rates calculated by the benchmark were approximately **72,305 rows/s (linear)** and **75,493 rows/s (QMLP)**. These *in-process verifier rates* are **not** network transaction throughput or consensus block-finalization throughput.

## 3. Scaling and verification overhead

**QMLP-V1, 32 → 64 → 16 model, 256 rows, varying partitions** (Rust-mirror full-block verifier):

| Partitions | Verification (ms) | Rows verified per second |
| ---: | ---: | ---: |
| 1 | 0.565 | 453,140 |
| 2 | 0.627 | 408,221 |
| 4 | 0.775 | 330,386 |
| 8 | 0.971 | 263,627 |
| 16 | 1.218 | 210,100 |
| 32 | 1.929 | 132,727 |
| 64 | 3.363 | 76,128 |

With the same 256 rows, changing from one to 64 partitions increased measured verification latency **5.95×**. Partition signatures, claims, encoding, and per-partition coordination have nontrivial overhead. This does not show a scalability benefit from more partitions: these sweeps benchmark sequential verifier work, **not parallel worker throughput**.

Verifier quorum-certificate checks also increased with signer count: **0.176 ms with four validators** and **2.679 ms with 64 validators** in the recorded sweep. This is a *signature-verification microbenchmark*, not an observed 64-node live consensus trial. The private live testnet contained at most four validators.

## 4. Private-testnet finality, recovery, and rewards

| CI scenario | Three-node demo | Four-validator fault scenario |
| --- | ---: | ---: |
| Scenario passed | Yes | Yes |
| Finalized block height | 5 | 5 |
| Entire scenario elapsed time | 23.0 s | 25.6 s |
| Finality median | 196 ms | 196 ms |
| Finality p95 (nearest-rank) | 3,228 ms | 2,191 ms |
| Finality samples | 11 | 15 |
| Total native tokens issued | 250 | 250 |
| Total fee + issuance paid to workers | 392 | 400 |

The **trained PyTorch-derived task** was submitted in the three-node scenario and finalized at height 5. The independent reward audit confirmed **80 Sayed Tokens paid** across three workers in increments of **27, 27 and 26** (30 task-fee tokens plus 50 scheduled issuance tokens). All validators agreed on the final chain and state root. The four-validator scenario finalized with one node absent, recovered it, and finalized after a leader crash with a round change.

The finality distribution is measured in a small local Docker network and includes distinct roles/rounds; it is *not a latency SLA*. The complete scenario duration includes initialization, task submissions, adversarial checks, restart/recovery, and harness waits, not just block production.

## 5. Limitations and next research questions

1. **Verifier workload:** correct small-model results are established by full independent recomputation, not a succinct proof. Larger models would scale validation costs.
2. **Economic incentives:** workers currently earn from assigned row counts and an illustrative genesis-prefunded fee budget plus scheduled issuance; rewards are not proportional to measured compute cost or useful model quality.
3. **Network design:** validator identities are permissioned; tests do not establish Sybil resistance, open-validator security, broad-area network resilience, or public-token economics.
4. **Experimental validity:** the digit dataset is synthetic; the accuracy comparison is a single seeded run, with no multi-seed statistical interval or external task distribution.
5. **Scaling:** repeat benchmarks on fixed, documented hardware and multiple runners; report median/dispersion across runs, CPU/memory profiles and end-to-end throughput under controlled worker/task loads. Test many validators in actual network deployments, not only certificate microbenchmarks.

### Research conclusion

The evidence supports a *working small, private, deterministically verifiable AI-work blockchain prototype*: a CPU-trained model can be quantized, assigned to Python workers, recomputed by Rust validators, finalized by a permissioned BFT testnet, and rewarded through Haskell ledger settlement. The performance data also identifies settlement-process start-up and per-partition verification overhead as meaningful bottlenecks. It does **not** establish that arbitrary AI training can be verified economically or that the system can operate as a permissionless production blockchain.
