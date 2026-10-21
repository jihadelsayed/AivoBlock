# Non-blockchain QMLP service baseline (issue #68)

This baseline persists a validated integer QMLP model in a process, computes
bounded batches of up to 256 total rows, and optionally **recomputes the
results in the same implementation**. It produces exact QMLP model, input,
and result commitments using the consensus reference arithmetic.

`QmlpService` in `ai-engine/src/aivoblock_ai/service_baseline.py` has
no blockchain code, digital signatures, consensus, network transport or
ledger accounting. The verification mode is intended as a trust-semantics
control and **does not provide independent implementation assurance**.

This first milestone delivers a tested service core, **not a measured
performance comparison**. Queue concurrency, persistent IPC/HTTP transport,
an independent verifier, workload-manifest integration, and controlled
end-to-end throughput measurements are future tasks. Do not compare its
unmeasured performance to PR #67's Rust microbenchmark, and do not treat a
single Python process as optimized AI serving.

Run: `python -m pytest ai-engine/tests/test_service_baseline.py`.
