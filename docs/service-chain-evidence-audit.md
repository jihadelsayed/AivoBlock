# PR #73 — QMLP service versus blockchain: evidence and comparability audit

Status: **exploratory observations; a matched comparison has NOT passed its acceptance gates**.

## Source artifacts

- [PR #71 CI run #404](https://github.com/jihadelsayed/AivoBlock/actions/runs/39615556041), artifact `testnet-reports`, files `three-node/report.json` and `workload-manifest.json`.
- [PR #72 CI run #419](https://github.com/jihadelsayed/AivoBlock/actions/runs/39618899653), artifact `pytorch-onnx-research`, file `service-replicates.json`.
- The source runs are **different CI runs**; runner equivalence has not been established.

## Descriptive results — NOT a common-latency comparison

| Path | Measurement boundary | Observations | Observed latency |
|---|---|---:|---:|
| Direct QMLP service core | Synchronous local in-process execution only | 90 requests, 3 Python process launches | median **2.040254 ms**, p95 **2.144648 ms** |
| Service core + recomputation | Same local in-process boundary, same-implementation recomputation | 90 requests, 3 Python process launches | median **3.9763035 ms**, p95 **4.140138 ms** |
| QMLP on three-validator private blockchain | Immediately before CLI submission through all validators **observed** finalized height 5 | **1** QMLP request, 12 rows, 3 partitions | **1707.822 ms** (single observation, **not** a median) |

The blockchain measurement includes CLI startup/submission, Python worker, network/BFT finalization, and sequential status polling at **one-second** intervals. Actual finality preceded or coincided with the observation; precise consensus-only finality latency was **not** measured. Service measurements exclude request IPC/HTTP, consensus, blockchain settlement, and polling. No throughput, energy, production performance, or cost claims follow from these results.

The `three-node/report.json` field `finality_summary_ms` summarizes **11 mixed testnet finality observations** (median 259 ms). It is **not** the 12-row QMLP client-observed-finality sample, and must not be substituted for it.

## Identified invalidating comparison gates

1. **Model commitment mismatch**: PR #71 `workload-manifest.json` identifies `8a72240443317649b8efec168291d02dc930074a46f4367f29020df059009c44` for a **separate 256-row, 64-partition microbenchmark**, not the QMLP finality sample. PR #72 service `service-replicates.json` identifies model `8173ad1657270b8b3d980461a36ad33d7371cbcd1730a7f72343db4727f8d69e`. This does **not establish identity** for the blockchain sample; the QMLP finality report does not contain the sample's model/input commitments.
2. **Input commitment not proven equal**: service artifact identifies `511074a4d2bdc53536105f7fba3c4e9162ae759db91d66688e3e0f38d599193a`; the QMLP finality record has row and partition counts but no input commitment. Equal row count (**12**) is insufficient.
3. **Incommensurate boundaries**: in-process service-core latency versus end-to-end CLI-to-observed-finality latency.
4. **Insufficient repetitions**: one QMLP finality sample versus 90 samples per service mode. Required minimum is 90 per scenario across at least three launches.
5. **Independent environment metadata**: no evidence both scenarios were measured under controlled equivalent runner resources or pinned builds.

## Required next experiment

- Submit the **exact identical committed model and input rows** through the service and blockchain, recording actual model, input and result commitments at the request level. Reject absent or unequal commitments.
- Implement a persistent **external request boundary** for the non-blockchain service (or explicitly retain an unmatched exploratory comparison); record process launch, monotonic per-request timestamps and setup separately.
- Collect three fresh launches of at least 30 sequential QMLP finality requests per launch, with distinct request IDs and finalized heights, matching the service workload.
- Preserve raw logs, all successes/failures, polling interval, runner details and output commitments; compute descriptive statistics only after automated provenance and sample-count gates pass.
- Publish request-boundary differences next to every result chart; do not label the difference “consensus overhead” or claim a general speedup.

No numerical ratio between the standalone service timing and the blockchain timing is reported because the workload identity and measurement-boundary requirements have not been satisfied.

## Machine-checked comparison gate

Run `python3 -m unittest discover -s scripts -p 'test_validate_service_chain.py' -v`
and `python3 scripts/validate_service_chain.py measured-bundle.json --out audit.json`.
The JSON bundle must have `scenarios` keys `service_direct`,
`service_recomputed`, and `chain_observed_finality`. Each scenario has
`model_commitment`, `input_commitment`, `result_commitment` (64 lowercase
hex digits) and `samples`: an array of objects containing `launch_id`,
`request_id`, `elapsed_ms`, `status: "success"`, and their own
`model_commitment`, `input_commitment`, and `result_commitment` fields,
which must match the scenario commitment values; chain samples also
require a unique positive `finalized_height`. Identity must match across
scenarios, with at least three launches and 30 positive finite measured
samples per launch. Duplicated request IDs, missing heights, failures and
commitment disagreement reject the entire comparison.

This normalized schema does not assert that separately acquired PR #71/#72
artifacts are comparable, nor does it verify the source artifacts' signature
or per-request cryptographic proof. A producer linking actual measured data
to these exact commitments is still required.

## PR #74 — Request-level provenance gate

The validator now rejects a benchmark sample unless **that sample** carries
all three expected commitments. A scenario-level declaration alone cannot
stand in for evidence of what a particular request executed. This is a schema
and consistency check, **not** independent cryptographic attestation: a future
measurement producer must derive these commitments from actual model bytes,
canonical input bytes, and result bytes, and retain raw per-request evidence.
The matched end-to-end request-boundary benchmark remains future work.
