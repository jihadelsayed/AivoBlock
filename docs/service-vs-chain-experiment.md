# Preregistered QMLP service-versus-blockchain experiment

Status: **protocol only — no comparative result established by this document**.

## Question and scope

For the *same committed QMLP-V1 model and inference inputs*, what are the observed
latency and completion differences between (A) a persistent non-blockchain
QMLP service, (B) that service with optional same-implementation recomputation,
and (C) the private three-validator AivoBlock testnet?

This is an experimental comparison of **specified implementations and trust
semantics**, not a proof that blockchain makes AI inference faster or cheaper.
PR #71 measures one observed blockchain submission-to-finality sample; it is
not directly comparable to the Rust microbenchmark in PR #67 or to a Python
function call measured in isolation.

## Pre-execution controls

1. Pin the Git commit, container images, Python/Rust/Haskell versions and
   hardware/runner details for *all* scenarios; record CPU and memory limits.
2. Serialize the exact QMLP model and rows. Save and validate the workload
   manifest with model and input commitments, row count and partition count.
   Reject any run with mismatched commitments or unverifiable inputs.
3. Use the *same* 256-row, 64-partition workload if each implementation supports
   it; otherwise select one common supported workload **before observing
   results**, recording the reason. Never compare different model/row counts.
4. Confirm exact output/result commitments match between all implementations.
   A mismatch invalidates the comparison; do not substitute float PyTorch
   outputs for deterministic integer consensus logits.
5. Keep model loading/warmup outside measured requests in the persistent service.
   Stage the input file before timing blockchain submission, but include the
   signed CLI's file load, submission, worker, BFT and polling overhead.
   Document these *different operational boundaries* rather than claiming
   identical machinery.

## Measurements

Run at least three distinct process launches for each scenario. Per launch:
five unreported warmups, then at least 30 sequential requests. Each request
has a unique identifier and monotonic start/end timestamps.

- **A: Service execute.** Start before submission to a persistent
  service boundary; stop when the complete output is returned. If only
  in-process calls exist, label these specifically **in-process service-core
  latency**, not network request latency.
- **B: Service execute + recheck.** Same boundary and workload as A, with
  same-implementation recomputation enabled. This is *not* independent
  verification or Byzantine consensus.
- **C: Blockchain finality.** Start immediately before signed client CLI
  submission; stop when every validator has been *observed* at the target
  finalized height. Record timestamp precision and status polling interval.
  The measured value includes CLI, worker, network, settlement, BFT and polling.
  Use an otherwise identical fresh finalized workload for each sample and
  record the block height; avoid accidentally timing a previously submitted
  task.

Record each sample's elapsed milliseconds, commit hashes, execution status,
output commitment, warmed/cold state, runner label, process-launch ID and
scenario. Report failures separately, not as missing fast samples. Include
setup duration separately. If environment control is infeasible, label the
study *exploratory*, not a matched performance comparison.

## Analysis and acceptance gates

- Validate schema, positive finite durations, unique request IDs, committed
  workload equality and a minimum 90 measured samples per scenario.
- Publish raw per-request rows and per-launch median; summarize median, p95,
  mean, sample standard deviation and observed range **by scenario**.
- Publish latency ratios only with explicitly labeled numerator/denominator
  and different trust/operation semantics; never call them pure "consensus
  overhead" or a general speedup.
- Measure CPU time, peak RSS and request/response bytes where available, with
  collection methods. Do not claim cost, energy or throughput without direct
  instrumentation of those quantities. Sequential latency is not throughput.
- Explicitly bound observational uncertainty: status polling contributes up
  to roughly one polling period per validator to the observed completion time,
  plus sequential query overhead. Real finality occurred no later than the
  observation but its precise timestamp is unknown.
- Require passing deterministic-output tests, benchmark schema tests and
  CI runs before publishing numerical conclusions.

## Known architectural limitations

The blockchain is a private permissioned testnet, with all validators
recomputing deterministic QMLP inference. It supplies no succinct proof of
arbitrary AI computation, no independent guarantee of general model quality,
and no demonstrated permissionless incentive equilibrium. The non-blockchain
verification control reuses its execution implementation. Report these
limitations adjacent to any chart or performance headline.

## Relationship to existing artifacts

- `docs/service-baseline.md`: persistent Python service core and caveats.
- `docs/workload-manifest.md`: identity constraints.
- `docs/ai-workloads.md`: verifiable integer workload boundary.
- PR #71: observational client-to-all-validator finality; must complete CI
  and yield artifacts before including any numerical comparisons.

**Next implementation gate:** a real request boundary and matching repeated
samples for A/B/C, then a strictly validating analysis script. Do not label
this protocol as an executed benchmark.
