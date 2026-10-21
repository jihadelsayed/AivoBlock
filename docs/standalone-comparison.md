# Standalone integer AI inference versus AivoBlock verification

This research comparison isolates the **cost of validating an AI-work block**
relative to directly computing the **same QMLP-V1 integer neural network on
the same input rows** without a blockchain.

It is a limited computational-control experiment, **not** a comparison with
modern optimized PyTorch, GPU inference, production AI serving, or
non-blockchain AI *training*. Do not interpret the reported ratios as the
overall energy cost, consensus overhead, blockchain TPS, or network latency.

## Experimental conditions

The existing `aivoblock-node bench` command now reports
`standalone_qmlp_inference_us` for QMLP tasks. The baseline calls the
validated Rust QMLP-V1 logits API sequentially on all task rows,
using `std::hint::black_box` to retain the computation and a warm-up pass.
The model, weights, integer arithmetic and rows are **identical** to the
corresponding full-block verifier trial. Every repeated benchmark exports
SHA-256 model and input commitments from the signed task; the analyzer
requires both to match across all process invocations, not just tensor
sizes and layer counts.

The matched alternatives are:

| Path | Work measured |
| --- | --- |
| Standalone QMLP-V1 | Integer logits, model/input validation, per-row allocation, no commitments or blockchain |
| Rust full-block verification | Deserialize and validate task/claims/block; verify signatures and commitments; recompute AI results; apply Rust-mirror settlement |
| Haskell persistent full-block verification | The same block path with persistent Haskell settlement cross-checked by Rust |

None of the three timed paths includes actual multi-validator voting,
container networking, peer synchronization, fault recovery, or finality.
For end-to-end finality results, refer to the separate [research
report](research-results-2026-10-21.md) and the full testnet artifacts.

**Control configurations** follow the existing code:

- Main comparison: 256 rows, 32→64→16 QMLP, 64 worker partitions.
- Row sweep: 16 / 64 / 256 rows with eight worker partitions.
- Partition sweep: 1 / 2 / 4 / 8 / 16 / 32 / 64 partitions, keeping 256
  rows and the same 32→64→16 network.
- Model-size sweep: fixed 256 rows and eight partitions with three model sizes.

The input/model seeds remain fixed for the verifier sweep so that row and
partition effects are not confounded with random data. PyTorch multiseed
accuracy experiments run separately and do not change these verifier inputs.

## Reproduce

Run on the same machine and in the same Docker image used for the private
AivoBlock testnet. The first invocation includes the scaling sweeps and the
next two repeat the same main workload independently:

```bash
mkdir -p testnet/generated
docker run --rm aivoblock-node:testnet bench --iterations 20 --sweep \
  > testnet/generated/bench-replicate-1.json
for run in 2 3; do
  docker run --rm aivoblock-node:testnet bench --iterations 20 \
    > "testnet/generated/bench-replicate-$run.json"
done
python3 scripts/analyze_standalone.py \
  --benchmarks testnet/generated/bench-replicate-1.json \
               testnet/generated/bench-replicate-2.json \
               testnet/generated/bench-replicate-3.json \
  --out testnet/generated/standalone-comparison.json
```

GitHub Actions runs this after the existing PyTorch research, benchmark
repetitions, three-node trained-model demo and four-validator fault
scenarios. The **testnet-reports** artifact includes the three raw benchmark
JSON files and `standalone-comparison.json`. The stdlib analyzer rejects
missing, nonfinite, mismatched-size or non-positive samples and produces
mean, median, sample standard deviation, range and all original observations.

`rust_verification_over_direct` and
`haskell_verification_over_direct` are per-invocation **latency ratios**:
a value of 5 means a full verification pass took five times as long as a
direct local integer-inference pass for the tested workload. The ratios do
not establish that AI compute is inherently five times slower *because of*
a blockchain in other architectures.

## Limitations and follow-up experiments

- Direct Rust inference uses the public per-row API, including repeated
  validation and allocation, and is **not** a maximally optimized inference
  implementation. An optimized baseline could run faster.
- Full-block verification recomputes workers' results, whereas a
  conventional service might trust the producer. The difference includes
  those separate trust semantics, not only serialization overhead.
- The benchmark is single-process and sequential for its direct baseline;
  it does not measure a multi-worker job server or GPU accelerator.
- The three process-level measurements share one CI runner environment.
  Their sample variance is **descriptive only**, and row/partition
  sweeps are only a **single invocation** each.
- No real electricity usage, financial cost, broad-area transport or
  large-model proof compression was measured.
- A future stronger control should compare **equal trust assumptions**,
  independent machines, an optimized non-blockchain serving pipeline, and
  measured end-to-end useful-output throughput under the same workload.
