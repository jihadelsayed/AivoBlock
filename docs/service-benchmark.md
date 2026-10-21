# Local QMLP service measurement

This companion to `service_baseline.py` measures a synchronous CPU process
that keeps a QMLP model loaded between requests. It compares **trusted result**
and **same-implementation recomputation** modes using identical committed
model and input fixtures. It intentionally does not report blockchain
throughput or pretend the verifier is an independent implementation.

A fixture JSON object must contain `model` (QMLP-V1 model JSON), `rows`,
`model_commitment` and `input_commitment`. Both commitments must match the
computed canonical values or the benchmark aborts.

```bash
python -m aivoblock_ai.service_benchmark --fixture /path/to/qmlp-fixture.json \
  --iterations 20 --batch 64 --out /tmp/service-trusted.json
python -m aivoblock_ai.service_benchmark --fixture /path/to/qmlp-fixture.json \
  --iterations 20 --batch 64 --verify --out /tmp/service-recomputed.json
```

Results record per-operation observations, mean, median, sample standard
deviation, and rows/sec from mean latency. The rows/sec value is local
single-process processing rate; it is **not** network TPS or finalized
throughput. Any comparison to the Rust benchmark requires the *exact same
model/input commitments*, equivalent hardware and explicit methodology.
Repeated invocations and broader resource/latency controls remain pending.

## Convert a trained model export into the service fixture

The existing PyTorch research job emits `digits.qmlp.json` with a trained
QMLP model, signed-off result logits and result commitment. Convert it using:

```bash
python -m aivoblock_ai.service_fixture \\
  --qmlp-file research-out/digits.qmlp.json \\
  --out research-out/service-fixture.json
python -m aivoblock_ai.service_benchmark \\
  --fixture research-out/service-fixture.json --iterations 20 \\
  --out research-out/service-trusted.json
```

This transformation validates the exported model, every input row, all logits,
and the result commitment before benchmarking. **Important:** the existing
Rust `bench` command in PR #67 uses its own generated synthetic QMLP model.
Its timing is therefore **not** a matched comparison with this trained-model
fixture. A Rust verifier mode accepting the *same export* and checking identical
model/input commitments remains required before a scientific performance ratio
can be reported.
