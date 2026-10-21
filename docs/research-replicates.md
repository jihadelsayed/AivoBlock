# Repeated AivoBlock research experiments

The [21 October baseline](research-results-2026-10-21.md) came from one CI
environment and one seeded PyTorch run. That baseline remains intact.

This change adds **three independent synthetic-data / model-initialization
seed pairs** and **three separate verifier benchmark invocations**.
Do not infer independent machines or a production-network throughput bound
from three runs on one GitHub-hosted CI environment.

## Reproduce the model experiments

The primary `research-out/report.json` and `digits.qmlp.json` remain the
original baseline; they are the exact artifact used by the three-node
blockchain end-to-end test. The experiment's `--training-seed` is explicit,
defaulting to `0` to preserve prior published results. For the new
replicates, both the dataset seed and PyTorch model initialization vary:

```bash
pip install --index-url https://download.pytorch.org/whl/cpu "torch>=2.2,<3"
pip install -e "./ai-engine[dev,research]"
for seed in 11 17 23; do
  python -m aivoblock_ai.research \
    --seed "$seed" --training-seed "$seed" \
    --out "research-out/replicates/seed-$seed"
done
```

Each replicate trains a 32→32→10 float network for 200 epochs on 2,000
synthetic 8×4 glyphs and evaluates it on 500 examples, then quantizes and
checks the full 500-row integer ONNX output against QMLP-V1. Both seeds are
recorded; the analyzer rejects duplicates, mixed hyperparameters, or integer
disagreements. The commitment is also checked to differ across experiments.

## Reproduce the verifier benchmark measurements

Use the same Docker node image and fixed verifier workload in three
*separate* process invocations. Each process repeats the microbenchmark
operations 20 times. The first run includes the scaling sweeps needed by
the baseline report; the next two focus on the same top-level workloads:

```bash
mkdir -p testnet/generated
docker run --rm aivoblock-node:testnet bench --iterations 20 --sweep \
  > testnet/generated/bench-replicate-1.json
for run in 2 3; do
  docker run --rm aivoblock-node:testnet bench --iterations 20 \
    > "testnet/generated/bench-replicate-$run.json"
done
```

The CI summary is generated with:

```bash
python3 scripts/analyze_replicates.py \
  --models research-out/replicates/seed-11/report.json \
           research-out/replicates/seed-17/report.json \
           research-out/replicates/seed-23/report.json \
  --benchmarks testnet/generated/bench-replicate-1.json \
               testnet/generated/bench-replicate-2.json \
               testnet/generated/bench-replicate-3.json \
  --out testnet/generated/variability-summary.json
```

Find `variability-summary.json`, the raw replicate benchmark JSON, and
the model reports under the **testnet-reports** and
**pytorch-onnx-research** artifacts attached to each successful GitHub CI
run. The report includes individual observations, mean, median, minimum,
maximum and **sample standard deviation** of accuracies, full-block Rust
mirror and Haskell one-shot/persistent verifier times, and speedup ratios.

## Limits on interpretation

- Different seeds give **three experiments on one synthetic dataset
  generator**, not a scientifically diverse task benchmark.
- Repeating a command on one shared CI runner measures within-environment
  variation, not variation across machines, workloads, versions, or regions.
- A 20-iteration microbenchmark mean is a single observation per
  invocation; its repeated statistics do **not** represent independent
  per-operation samples.
- With only three replicates, sample standard deviations are descriptive:
  no confidence intervals, significance claims, robustness guarantee or
  inference about general AI workloads.
- Blockchain correctness continues to rely on independent integer
  recomputation. Floating-point ONNX stays off-consensus.
