//! `aivoblock-node bench`: verifier and settlement micro-benchmarks for the
//! largest V3 task the protocol accepts (256 rows x 32 features, 64 worker
//! partitions) and for quorum-certificate checks. Results are wall-clock
//! numbers on the current machine, printed as JSON.

use std::collections::BTreeSet;
use std::time::Instant;

use ed25519_dalek::SigningKey;
use serde_json::{json, Value};

use super::cli::{build_task, partition_rows, qmlp_workload_with, workload, TaskSpec};
use super::worker::WorkerBackend;
use crate::ai_block_v3::{
    build_block_v3, execute_block_v3, expected_result_v3, sign_claim_v3, verify_claim_v3, AiTaskV3,
    ParentContext, V3Rules, MAX_FEATURES, MAX_PARTITIONS, MAX_ROWS,
};
use crate::ipc::haskell::HaskellValidatorClient;
use crate::ipc::python::PythonWorkerClient;
use crate::issuance::IssuancePolicyV1;
use crate::settlement_v3::SettlementEngine;
use crate::storage::ledger_state::{LedgerState, LocalAccount};
use crate::validator_certificate::{
    sign_precommit_v1, verify_quorum_certificate_v1, QuorumCertificateV1, TrustedValidatorSetV1,
    ValidatorMemberV1,
};

fn per_op_us(started: Instant, operations: u32) -> f64 {
    started.elapsed().as_secs_f64() * 1e6 / f64::from(operations)
}

fn certificate_bench(validators: usize, iterations: u32) -> Result<f64, String> {
    let keys: Vec<SigningKey> = (0..validators)
        .map(|i| SigningKey::from_bytes(&[(i as u8).wrapping_add(1); 32]))
        .collect();
    let members: Vec<ValidatorMemberV1> = keys
        .iter()
        .map(|key| ValidatorMemberV1 {
            public_key: key.verifying_key().to_bytes(),
            voting_power: 1,
        })
        .collect();
    let set = TrustedValidatorSetV1::new(&members)?;
    let mut votes = keys
        .iter()
        .map(|key| sign_precommit_v1(key, set.commitment(), 1, 0, [7; 32]))
        .collect::<Result<Vec<_>, _>>()?;
    votes.sort_by_key(|vote| vote.public_key);
    let certificate = QuorumCertificateV1 {
        chain_id: 1,
        height: 1,
        round: 0,
        block_id: [7; 32],
        validator_set_commitment: set.commitment(),
        votes,
    };
    let started = Instant::now();
    for _ in 0..iterations {
        verify_quorum_certificate_v1(&set, &certificate)?;
    }
    Ok(per_op_us(started, iterations))
}

struct BlockTimings {
    block_bytes: usize,
    worker_us: f64,
    claim_us: f64,
    block_us: f64,
    haskell_us: Option<f64>,
}

/// Honest claims for every partition (Rust reference), then claim and
/// full-block verification timings with the Rust mirror and, when
/// configured, with Haskell settlement cross-checked by the mirror.
fn time_block(
    task: &AiTaskV3,
    workers: &[SigningKey],
    rules: &V3Rules<'_>,
    parent: &ParentContext<'_>,
    iterations: u32,
) -> Result<BlockTimings, String> {
    let started = Instant::now();
    let mut claims = Vec::new();
    for (index, partition) in task.partitions.iter().enumerate() {
        let key = workers
            .iter()
            .find(|key| key.verifying_key().to_bytes() == partition.worker)
            .ok_or("missing worker key")?;
        let result = expected_result_v3(task, index as u32)?;
        claims.push(sign_claim_v3(task, index as u32, key, result)?);
    }
    let worker_us = per_op_us(started, claims.len() as u32);
    let started = Instant::now();
    for _ in 0..iterations {
        for claim in &claims {
            verify_claim_v3(task, claim)?;
        }
    }
    let claim_us = per_op_us(started, iterations * claims.len() as u32);
    let mirror = SettlementEngine::RustMirror;
    let built = build_block_v3(parent, rules, &mirror, task, &claims)?;
    let started = Instant::now();
    for _ in 0..iterations {
        execute_block_v3(parent, rules, &mirror, &built.bytes, Some(&built.id))?;
    }
    let block_us = per_op_us(started, iterations);
    let haskell_us = match std::env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE") {
        Some(path) if !path.is_empty() => {
            let core = HaskellValidatorClient::new(path).map_err(|e| format!("{e:?}"))?;
            let engine = SettlementEngine::Haskell(core);
            let started = Instant::now();
            for _ in 0..iterations {
                execute_block_v3(parent, rules, &engine, &built.bytes, Some(&built.id))?;
            }
            Some(per_op_us(started, iterations))
        }
        _ => None,
    };
    Ok(BlockTimings {
        block_bytes: built.bytes.len(),
        worker_us,
        claim_us,
        block_us,
        haskell_us,
    })
}

fn timings_json(timings: &BlockTimings, rows: usize, partitions: usize) -> Value {
    let worker_total_us = timings.worker_us * partitions as f64;
    json!({
        "rows": rows,
        "partitions": partitions,
        "block_bytes": timings.block_bytes,
        "worker_compute_and_sign_us_per_partition": timings.worker_us,
        "worker_compute_and_sign_us_all_partitions": worker_total_us,
        "claim_verify_us": timings.claim_us,
        "full_block_verify_rust_mirror_us": timings.block_us,
        "full_block_verify_haskell_cross_checked_us": timings.haskell_us,
        "block_verify_over_all_worker_compute": timings.block_us / worker_total_us,
        "rows_verified_per_second": rows as f64 * 1e6 / timings.block_us,
    })
}

/// Wall-clock time of one real Python worker execution of partition 0
/// (process spawn, JSON IPC, computation, Rust cross-check), if configured.
fn python_partition_us(task: &AiTaskV3, iterations: u32) -> Result<Option<f64>, String> {
    let Some(script) = std::env::var_os("AIVOBLOCK_PYTHON_WORKER").filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let python = std::env::var_os("AIVOBLOCK_PYTHON_EXECUTABLE").unwrap_or_else(|| "python3".into());
    let client = PythonWorkerClient::new(python, script).map_err(|e| format!("{e:?}"))?;
    let backend = WorkerBackend::Python(client);
    let worker = task.partitions[0].worker;
    let started = Instant::now();
    for _ in 0..iterations {
        backend.execute(task, 0, &worker)?;
    }
    Ok(Some(per_op_us(started, iterations)))
}

/// Scaling sweeps that change one variable at a time.
fn sweeps(
    authority: &SigningKey,
    workers: &[SigningKey],
    worker_ids: &[[u8; 32]],
    rules: &V3Rules<'_>,
    parent: &ParentContext<'_>,
    iterations: u32,
) -> Result<Value, String> {
    let qmlp_task = |rows: usize, dims: (usize, usize, usize), partitions: usize| {
        let (model, matrix) = qmlp_workload_with(rows, dims, 7);
        build_task(
            TaskSpec {
                sequence: 1,
                deadline_height: 10,
                fee_budget: 64,
                weights: &[],
                rows: &matrix,
                qmlp: Some(model),
                partitions: partition_rows(matrix.len(), &worker_ids[..partitions]),
            },
            authority,
        )
    };
    let linear_task = |rows: usize, partitions: usize| {
        let (weights, matrix) = workload("synthetic", rows, MAX_FEATURES, 42);
        build_task(
            TaskSpec {
                sequence: 1,
                deadline_height: 10,
                fee_budget: 64,
                weights: &weights,
                rows: &matrix,
                qmlp: None,
                partitions: partition_rows(matrix.len(), &worker_ids[..partitions]),
            },
            authority,
        )
    };
    let mut by_partitions = Vec::new();
    for partitions in [1, 2, 4, 8, 16, 32, 64] {
        let task = qmlp_task(MAX_ROWS, (32, 64, 16), partitions)?;
        let timings = time_block(&task, workers, rules, parent, iterations)?;
        by_partitions.push(timings_json(&timings, MAX_ROWS, partitions));
    }
    let mut by_model = Vec::new();
    for (d_in, d_hidden, d_out) in [(8, 16, 4), (16, 32, 10), (32, 64, 16)] {
        let task = qmlp_task(MAX_ROWS, (d_in, d_hidden, d_out), 8)?;
        let timings = time_block(&task, workers, rules, parent, iterations)?;
        let mut entry = timings_json(&timings, MAX_ROWS, 8);
        entry["layers"] = json!(format!("{d_in} -> {d_hidden} -> {d_out}"));
        entry["integer_multiply_adds_per_row"] = json!(d_in * d_hidden + d_hidden * d_out);
        by_model.push(entry);
    }
    let mut by_rows = Vec::new();
    for rows in [16, 64, 256] {
        let linear = time_block(&linear_task(rows, 8)?, workers, rules, parent, iterations)?;
        let task = qmlp_task(rows, (32, 64, 16), 8)?;
        let qmlp = time_block(&task, workers, rules, parent, iterations)?;
        by_rows.push(json!({
            "rows": rows,
            "integer_linear_v1": timings_json(&linear, rows, 8),
            "qmlp_v1": timings_json(&qmlp, rows, 8),
        }));
    }
    let mut by_validators = serde_json::Map::new();
    for validators in [4, 7, 16, 32, 64] {
        let us = certificate_bench(validators, iterations * 4)?;
        by_validators.insert(validators.to_string(), json!(us));
    }
    // Real worker processes are slow to spawn; a few runs suffice.
    let runs = iterations.clamp(1, 5);
    let linear_small = python_partition_us(&linear_task(MAX_ROWS, 64)?, runs)?;
    let qmlp_small = python_partition_us(&qmlp_task(MAX_ROWS, (32, 64, 16), 64)?, runs)?;
    let qmlp_large = python_partition_us(&qmlp_task(MAX_ROWS, (32, 64, 16), 1)?, runs)?;
    let python = json!({
        "integer_linear_v1_4_rows": linear_small,
        "qmlp_v1_4_rows": qmlp_small,
        "qmlp_v1_256_rows": qmlp_large,
    });
    Ok(json!({
        "qmlp_32_64_16_by_partitions": by_partitions,
        "qmlp_by_model_size_8_partitions": by_model,
        "by_rows_8_partitions": by_rows,
        "quorum_certificate_verify_us_by_validators": by_validators,
        "python_worker_partition_us": python,
    }))
}

pub fn bench_command(args: &[String]) -> Result<(), String> {
    let mut iterations: u32 = 10;
    let mut sweep = false;
    let mut flags = args.iter();
    while let Some(flag) = flags.next() {
        match flag.as_str() {
            "--iterations" => {
                let value = flags.next().ok_or("Missing --iterations value")?;
                iterations = value.parse().map_err(|_| "Bad --iterations")?;
            }
            "--sweep" => sweep = true,
            _ => return Err("Usage: bench [--iterations N] [--sweep]".into()),
        }
    }
    if iterations == 0 {
        return Err("--iterations must be positive".into());
    }
    let authority = SigningKey::from_bytes(&[17; 32]);
    let workers: Vec<SigningKey> = (0..MAX_PARTITIONS)
        .map(|i| SigningKey::from_bytes(&[(i as u8).wrapping_add(100); 32]))
        .collect();
    let mut worker_ids: Vec<[u8; 32]> = workers
        .iter()
        .map(|key| key.verifying_key().to_bytes())
        .collect();
    worker_ids.sort();
    let registered: BTreeSet<[u8; 32]> = worker_ids.iter().copied().collect();
    let policy = IssuancePolicyV1::new(50, 100, 2_000_000)?;
    let authority_key = authority.verifying_key().to_bytes();
    let rules = V3Rules {
        task_authority: &authority_key,
        workers: &registered,
        policy: &policy,
    };
    let (weights, rows) = workload("synthetic", MAX_ROWS, MAX_FEATURES, 42);
    let task = build_task(
        TaskSpec {
            sequence: 1,
            deadline_height: 10,
            fee_budget: 64,
            weights: &weights,
            rows: &rows,
            qmlp: None,
            partitions: partition_rows(rows.len(), &worker_ids),
        },
        &authority,
    )?;
    // The largest QMLP-V1 task: 256 rows through 32 -> 64 -> 16, 64 workers.
    let (model, qmlp_rows) = qmlp_workload_with(MAX_ROWS, (32, 64, 16), 7);
    let qmlp_task = build_task(
        TaskSpec {
            sequence: 1,
            deadline_height: 10,
            fee_budget: 64,
            weights: &[],
            rows: &qmlp_rows,
            qmlp: Some(model),
            partitions: partition_rows(qmlp_rows.len(), &worker_ids),
        },
        &authority,
    )?;
    let mut prior = LedgerState {
        fee_pool: 1_000,
        ..LedgerState::default()
    };
    prior.accounts.insert(
        authority_key,
        LocalAccount {
            balance: 100_000,
            nonce: 0,
        },
    );
    let parent = ParentContext {
        parent_id: [1; 32],
        parent_height: 0,
        parent_timestamp_ms: 0,
        prior: &prior,
    };
    let linear = time_block(&task, &workers, &rules, &parent, iterations)?;
    let qmlp = time_block(&qmlp_task, &workers, &rules, &parent, iterations)?;
    let mut report = json!({
        "iterations": iterations,
        "task": {
            "rows": rows.len(),
            "features": weights.len(),
            "partitions": task.partitions.len(),
            "block_bytes": linear.block_bytes,
        },
        "worker_compute_and_sign_us_per_partition": linear.worker_us,
        "claim_verify_us": linear.claim_us,
        "full_block_verify_rust_mirror_us": linear.block_us,
        "full_block_verify_haskell_cross_checked_us": linear.haskell_us,
        "rows_verified_per_second": rows.len() as f64 * 1e6 / linear.block_us,
        "qmlp_v1": {
            "rows": qmlp_rows.len(),
            "layers": "32 -> 64 -> 16",
            "integer_multiply_adds_per_row": 32 * 64 + 64 * 16,
            "partitions": qmlp_task.partitions.len(),
            "block_bytes": qmlp.block_bytes,
            "worker_compute_and_sign_us_per_partition": qmlp.worker_us,
            "claim_verify_us": qmlp.claim_us,
            "full_block_verify_rust_mirror_us": qmlp.block_us,
            "full_block_verify_haskell_cross_checked_us": qmlp.haskell_us,
            "rows_verified_per_second": qmlp_rows.len() as f64 * 1e6 / qmlp.block_us,
        },
        "quorum_certificate_verify_us": {
            "4_validators": certificate_bench(4, iterations * 10)?,
            "64_validators": certificate_bench(64, iterations)?,
        },
    });
    if sweep {
        report["sweeps"] = sweeps(
            &authority,
            &workers,
            &worker_ids,
            &rules,
            &parent,
            iterations,
        )?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
    );
    Ok(())
}
