# Repeated service-core benchmark (PR #72)

The script `aivoblock_ai.service_replicates` starts a **new Python interpreter**
for each measured launch. It runs the same commitment-checked fixture in
trusted-result and same-implementation-recomputation modes. Each launch
executes one untimed fixture-validation/warmup call and 30 default measured
synchronous requests; the model stays loaded inside that subprocess.

Run from an environment with `pip install -e './ai-engine[dev]'`:

```sh
python -m aivoblock_ai.service_replicates \
  --fixture research-out/trained-service-fixture.json \
  --launches 3 --iterations 30 --batch 64 \
  --out research-out/service-replicates.json
python -m pytest ai-engine/tests/test_service_replicates.py
```

Generate the fixture first with `python -m aivoblock_ai.service_fixture
--qmlp-file research-out/digits.qmlp.json
--out research-out/trained-service-fixture.json`.

Output records the raw samples, per-launch medians, aggregate median,
nearest-rank p95, mean, sample standard deviation, and min/max. Inputs,
outputs, and model commitments must match across modes and launches.

**Important limits:** This is **in-process service-core timing**, not
request/response IPC/HTTP timing; separate process startup is excluded from
measured requests. Each launch performs five untimed warmup requests and 30 measured requests.
The full end-to-end comparison protocol remains incomplete until network
request boundaries and matched blockchain trials are implemented. Samples are sequential,
not a throughput or energy measurement. Independent verification, consensus,
network transport and blockchain finality are not measured here. The
same-implementation recomputation mode is not independent verification.

Do not compare these raw measurements directly against PR #71's
client-observed finality as if they share an equivalent measurement boundary.
