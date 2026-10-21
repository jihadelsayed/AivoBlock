# Quantized MLP inference v1 (QMLP-V1)

Status: research-testnet specification. QMLP-V1 is the second AI workload
whose results may enter AivoBlock consensus, after the original integer linear
workload (`reproducible-ai-proof-v2.md`, `ai-work-block-v3.md`).

It is verified by **full deterministic recomputation**: every validator
re-runs every partition with exact integer arithmetic. It is not a succinct
or zero-knowledge proof, and it does not make arbitrary PyTorch or ONNX
floating-point models verifiable.

## Why integers

Floating-point inference (PyTorch, ONNX Runtime) is not bit-reproducible
across CPUs, instruction sets, BLAS libraries, thread counts or library
versions, because summation order, fused multiply-add and vectorized kernels
change rounding. Consensus requires every honest validator to compute the
same bytes. QMLP-V1 therefore fixes **every** operation as integer
arithmetic with explicit widths, rounding and clamping. A float model may be
*trained* anywhere; only its committed integer quantization is executed in
consensus.

## Model

Two dense layers with a fused ReLU and int8 requantization in between:

| Field      | Type / range                                   |
|------------|------------------------------------------------|
| `d_in`     | 1..=32 input features                           |
| `d_hidden` | 1..=64 hidden units                             |
| `d_out`    | 1..=16 output logits                            |
| `w1`       | `d_hidden × d_in` int8 in `[-127, 127]`, row-major by hidden unit |
| `b1`       | `d_hidden` int32 in `[-2^24, 2^24]`             |
| `multiplier` | u32 in `1..=2^31 - 1`                        |
| `shift`    | u32 in `1..=40`                                 |
| `w2`       | `d_out × d_hidden` int8 in `[-127, 127]`, row-major by output |
| `b2`       | `d_out` int32 in `[-2^24, 2^24]`                |

Input rows have exactly `d_in` integers in `[-127, 127]`.

## Arithmetic (normative)

All intermediate values are mathematical integers; the stated bounds prove
that signed 64-bit arithmetic never overflows.

For each input row `x` and hidden unit `j`:

```text
acc_j = b1[j] + sum_i w1[j][i] * x[i]          |acc_j| <= 2^24 + 32*127*127 < 2^25
num_j = acc_j * multiplier + 2^(shift - 1)      |num_j| < 2^56 + 2^39
q_j   = floor(num_j / 2^shift)                  arithmetic right shift (floor)
h_j   = min(max(q_j, 0), 127)                   fused ReLU and int8 clamp
```

For each output `k`:

```text
logit_k = b2[k] + sum_j w2[k][j] * h_j          |logit_k| <= 2^24 + 64*127*127
```

The predicted class is the lowest index among the maximal logits; it is
derived from the logits and is not committed separately.

Note on rounding: `floor((acc*m + 2^(s-1)) / 2^s)` rounds half toward
positive infinity. Because negative `num_j` are clamped to 0 by the ReLU,
truncating and flooring division give identical `h_j`, which lets the model
be expressed exactly with ONNX integer operators (see below).

## Canonical encodings

All integers are big-endian.

```text
model_body   = u32 d_in || u32 d_hidden || u32 d_out
               || u32 multiplier || u32 shift
               || w1 as i8 bytes || b1 as i32 || w2 as i8 bytes || b2 as i32
model_commitment  = SHA-256("AIVOBLOCK-QMLP-MODEL-V1\0" || model_body)
input_commitment  = SHA-256("AIVOBLOCK-INFERENCE-INPUT-V1\0"
                    || u32 rows || u32 d_in || every value as i64)
rules_commitment  = SHA-256("AIVOBLOCK-QMLP-RULES-V1\0")
result_commitment = SHA-256("AIVOBLOCK-QMLP-RESULT-V1\0"
                    || u64 start || u64 count || u32 d_out
                    || for each row in [start, start + count): d_out logits as i64)
```

The input commitment is byte-identical to the V1 integer-linear encoding, so
existing commitment code is reused; QMLP adds the stricter int8 input range.

## Partitioning and distribution

A QMLP task is partitioned by **contiguous input rows** (data parallelism),
using the same signed partition plan as `ai-work-block-v3.md`: each partition
names one registered worker key, partitions are ordered, contiguous,
non-overlapping and cover every row, and a worker appears at most once. The
full model travels with the task, so a worker needs nothing else. Layer or
tensor (model-parallel) splitting is not implemented: at these sizes the
whole model is a few kilobytes and splitting would only add intermediate
activations that would themselves need commitments.

## Reference implementations

- Python: `ai-engine/src/aivoblock_ai/qmlp.py` (worker request
  `execute_qmlp_inference`).
- Rust: `node/src/qmlp.rs`, used by validators for recomputation.

Both are tested against the shared vector below.

## Test vector

```text
d_in = 2, d_hidden = 2, d_out = 2, multiplier = 3, shift = 2
w1 = [[1, -2], [3, 4]]   b1 = [0, -5]
w2 = [[2, -1], [-3, 5]]  b2 = [1, 0]
rows   = [[1, 2], [3, -4], [-2, 5], [127, 127]]
hidden = [[0, 5], [8, 0], [0, 7], [0, 127]]
logits = [[-4, 25], [17, -24], [-6, 35], [-126, 635]]
```

## What is and is not verifiable

| Workload | In consensus? | How it is checked |
|----------|---------------|-------------------|
| Integer linear model (V1) | yes | full Rust recomputation |
| QMLP-V1 (this document) | yes | full Rust recomputation, exact integers |
| Float PyTorch / ONNX models | **no** | off-chain only; outputs differ across platforms and are never put in blocks |
| Integer ONNX export of a QMLP-V1 model | not by itself | an ONNX file is a portable *description*; validators execute this specification, and CI checks that ONNX Runtime reproduces it exactly |
| Large models, GPUs, convolution, attention | no | not implemented; would need succinct proofs or trusted hardware |
