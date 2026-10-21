# PR #75 — Request observation recorder

`scripts/record_benchmark.py` turns **already observed** request measurements
into a single normalized scenario fragment accepted by the PR #74 provenance
schema. The recorder computes SHA-256 commitments by reading the actual model,
input, and result files: it does not accept user-supplied digest strings.

Input is UTF-8 JSONL, with one object per request:

```json
{"launch_id":"launch-1","request_id":"r-001","start_ns":1000,"end_ns":2001000,"status":"success","model_path":"artifacts/model.bin","input_path":"artifacts/input.bin","result_path":"artifacts/result.bin","finalized_height":42}
```

Use `finalized_height` only with `--scenario chain_observed_finality`.
Example:

```sh
python scripts/record_benchmark.py --scenario service_direct --observations raw/service.jsonl --out normalized/service-direct.json
```

**Measurement requirements and limitations**

- The caller must record `start_ns` and `end_ns` using the **same monotonic clock** around a clearly stated request boundary. The recorder cannot authenticate timestamps or certify that they were collected from real requests.
- Model, input and result artifact files must correspond to the exact request; the recorder reads their current bytes and detects differing hashes within one scenario.
- Keep original JSONL, artifacts, failures, environment/build metadata and any chain receipts as independent evidence. Rejected requests are not silently converted to successes.
- Every scenario launch is currently recorded as user-provided `launch_id`; the recorder does not itself launch processes or run blockchain transactions.
- Output is **one scenario fragment** and cannot independently pass the full matched comparison gate. Repeated matched measurements, actual blockchain submissions, service external request boundary, and controlled runner comparability are **not yet implemented**.

Run `python -m unittest discover -s scripts -p 'test_record_benchmark.py' -v`.
