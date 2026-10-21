# AI-work block V3

Status: implemented for the private research testnet (`node/src/ai_block_v3.rs`).

V3 replaces V2's "any worker may claim" model with an **authority-signed
assignment plan**: every partition of a task names the registered worker key
that must compute it. Each validator recomputes every partition from bytes in
the block itself before voting. Verification is full recomputation of a small
deterministic integer workload, **not** a succinct or zero-knowledge proof.

## Task

All integers are big-endian.

```text
task_body = sequence:u64 || created_ms:u64 || deadline_height:u64 || fee_budget:u64
          || model_commitment[32] || input_commitment[32] || rules_commitment[32]
          || rows:u32 || width:u32 || weights[width]:i64 || rows[rows][width]:i64
          || partitions:u32 || (start:u64 || count:u64 || worker_pubkey[32])*
task_id   = SHA256("AIVOBLOCK-AI-TASK-ID-V3\0" || chain_id:u32 || task_body)
signature = Ed25519(authority, "AIVOBLOCK-SIGNED-AI-TASK-V3\0" || chain_id:u32 || task_id)
encoding  = task_body || task_id || signature
```

Validity at settlement height `h`:

- `sequence == h`, `sequence <= deadline_height`, `h <= deadline_height` (else *expired*).
- 1..256 rows, 1..32 features, every value in `[-1e6, 1e6]`.
- Model/input commitments use the same encodings as the Python worker
  (`AIVOBLOCK-INT-LINEAR-V1`, `AIVOBLOCK-INFERENCE-INPUT-V1`); rules commitment
  is `SHA256("AIVOBLOCK-INTEGER-INFERENCE-RULES-V1\0")`.
- 1..64 partitions, contiguous from row 0, non-overlapping, covering every row
  exactly once, each worker registered in the signed network config, no worker
  repeated.
- `task_id` recomputes and the signature verifies under the **pinned** task
  authority key (never a key carried by the task).

### QMLP-V1 tasks

A task may instead carry a quantized two-layer MLP
(`quantized-mlp-v1.md`). Its body has its own tag, so it can never be parsed
as a linear task, and linear tasks keep their exact original bytes:

```text
task_body = "AIVOBLOCK-AI-TASK-QMLP-V1\0"
          || sequence:u64 || created_ms:u64 || deadline_height:u64 || fee_budget:u64
          || model_commitment[32] || input_commitment[32] || rules_commitment[32]
          || model_body || rows:u32 || rows[rows][d_in]:i8
          || partitions:u32 || (start:u64 || count:u64 || worker_pubkey[32])*
```

- `model_commitment = SHA256("AIVOBLOCK-QMLP-MODEL-V1\0" || model_body)`,
  `rules_commitment = SHA256("AIVOBLOCK-QMLP-RULES-V1\0")`, and the input
  commitment is the V1 input encoding (values widened to i64).
- Every input is in `[-127, 127]`; model limits are those of QMLP-V1.
- Claims commit to `AIVOBLOCK-QMLP-RESULT-V1` over the partition's logits;
  validators recompute them with `node/src/qmlp.rs`.
- Partitioning, assignment, signatures, block format, settlement and rewards
  (by assigned row count) are exactly as for linear tasks.

## Claim

```text
signed = "AIVOBLOCK-SIGNED-AI-RESULT-V3\0" || chain_id:u32 || task_id || partition:u32
       || worker_pubkey || start:u64 || count:u64 || result_commitment
```

A claim is accepted only if the worker is the one assigned to that partition,
the Ed25519 signature verifies (`verify_strict`), and `result_commitment`
equals the validator's own recomputation (`AIVOBLOCK-INFERENCE-V1` commitment,
byte-identical to `ai-engine/.../executor.py`).

## Block

```text
block   = "AIVOBLOCK-AI-WORK-BLOCK-V3\0" || header[178] || payload
payload = task_encoding || issuance:u64 || claims:u32 || (result[32] || signature[64])*
```

- `header` is the canonical V1 header; block ID = SHA256(header).
- `transactions_root` = empty transaction root.
- `puzzle_proofs_root` = SHA256("AIVOBLOCK-AI-WORK-ROOT-V3\0" || payload).
- `state_root` = state root of the settled successor ledger.
- `timestamp_ms = max(parent.timestamp_ms + 1, task.created_ms)` — fully
  deterministic, so every honest proposer builds the identical block.
- One claim per partition, in partition order (worker and range come from the
  assignment). Maximum 256 KiB.
- `issuance` must equal the genesis-bound schedule
  (`sayed-token-issuance-v1.md`); settlement must reproduce `state_root`.
- Reward batch ID = SHA256("AIVOBLOCK-AI-WORK-REWARD-BATCH-V3\0" || chain_id || task_id);
  receipt IDs are the canonical V1 receipt IDs. Ledger replay markers make a
  task or receipt payable at most once.

## Rejected inputs (tested)

Forged/foreign authority signature, tampered rows/weights, out-of-range values,
overlapping or gapped partitions, unregistered or repeated workers, expired or
out-of-sequence tasks, claims from the wrong worker, forged claim signatures,
manipulated results re-signed by the real worker, claims for other tasks,
nonexistent partitions, any flipped byte in a block, trailing bytes, wrong
parent, inflated issuance, wrong state root, and replay of a settled task.
For QMLP tasks additionally: a model changed after signing, inputs outside
int8 (even with a matching input commitment), linear weights or the linear
rules commitment on a QMLP task, manipulated logits re-signed by the assigned
worker, and claims computed for a different workload.
