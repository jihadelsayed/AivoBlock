# Matched workload manifest (research issue #68)

The manifest validates and records the exact committed QMLP-V1 model and
input reused by at least three separate AivoBlock verifier benchmark runs.
It records row count, partition count, model layers, per-run iteration count,
the source Git revision, runner identity, and Python/platform metadata.

**Prerequisite:** PR #67 exposes `model_commitment` and `input_commitment`
on the top-level `qmlp_v1` benchmark object. Until #67 merges and generates
three benchmark files, this is an independently tested validator, **not** a
completed performance experiment.

Usage:

```bash
python3 scripts/workload_manifest.py \
  --benchmarks testnet/generated/bench-replicate-1.json \
               testnet/generated/bench-replicate-2.json \
               testnet/generated/bench-replicate-3.json \
  --runner github-actions --revision "$(git rev-parse HEAD)" \
  --out testnet/generated/workload-manifest.json
python3 -m unittest discover -s scripts -p 'test_workload_manifest.py' -v
```

This is a workload-identity record for repeatability, not a benchmark report.
It does not measure actual CPU utilization, total network finality, or energy.
Hardware identity is self-reported metadata and not independently attested.
