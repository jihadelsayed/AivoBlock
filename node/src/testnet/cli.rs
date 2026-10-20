//! Command-line entry points: `run`, `testnet-init` and `client ...`.

use std::collections::BTreeMap;
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use serde_json::{json, Value};

use super::config::{load_network_config, load_node_keys, read_key_file};
use super::control::{control_call, ControlRequest};
use super::init::{generate, write_testnet, InitOptions};
use super::node::{start_node, NodeOptions};
use super::wire::WireMessage;
use super::worker::WorkerBackend;
use super::{env_or, hex, now_ms, random_bytes32, unhex, unhex_array};
use crate::ai_block_v3::{
    decode_block_v3, encode_task_v3, expected_result_v3, input_commitment_v3, model_commitment_v3,
    sign_claim_v3, sign_task_v3, AiTaskV3, PartitionV3, MAX_BLOCK_BYTES,
};
use crate::ai_proof_v2::rules_commitment_v1;
use crate::ipc::haskell::HaskellValidatorClient;
use crate::ipc::python::PythonWorkerClient;
use crate::network::handshake::sign_hello;
use crate::network::tcp::{configure_stream, recv_frame, send_frame};
use crate::settlement_v3::SettlementEngine;
use crate::validator_certificate::sign_precommit_v1;

const DEFAULT_CONTROL: &str = "127.0.0.1:30334";
const HEALTH: &[u8] = br#"{"protocol_version":1,"type":"health"}"#;

fn flags(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut parsed = BTreeMap::new();
    let mut iter = args.iter();
    while let Some(name) = iter.next() {
        let key = name
            .strip_prefix("--")
            .ok_or_else(|| format!("Unexpected argument {name}"))?;
        let value = iter
            .next()
            .ok_or_else(|| format!("Missing value for {name}"))?;
        parsed.insert(key.to_owned(), value.clone());
    }
    Ok(parsed)
}

fn number<T: std::str::FromStr>(
    flags: &BTreeMap<String, String>,
    name: &str,
    default: T,
) -> Result<T, String> {
    match flags.get(name) {
        None => Ok(default),
        Some(raw) => raw.parse().map_err(|_| format!("Invalid --{name} value")),
    }
}

/// `aivoblock-node run`: configuration comes from the environment so the
/// same image serves every container.
pub fn run_command(args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("run takes no arguments; configure it with AIVOBLOCK_* variables".into());
    }
    let name = env_or("AIVOBLOCK_NODE_NAME", "node");
    let pinned: [u8; 32] = unhex_array(&env_or("AIVOBLOCK_AUTHORITY_PUBLIC_KEY", ""))
        .map_err(|_| "Set AIVOBLOCK_AUTHORITY_PUBLIC_KEY to the pinned authority key (hex)")?;
    let config_path = PathBuf::from(env_or("AIVOBLOCK_CONFIG", "/config/network.json"));
    let config = Arc::new(load_network_config(&config_path, &pinned)?);
    let keys = load_node_keys(&PathBuf::from(env_or("AIVOBLOCK_KEY_DIR", "/keys")))?;
    let engine = if env_or("AIVOBLOCK_SETTLEMENT_ENGINE", "haskell") == "rust-mirror" {
        eprintln!("WARNING: Haskell ledger disabled; Rust mirror settlement only (tests)");
        SettlementEngine::RustMirror
    } else {
        let path = env_or("AIVOBLOCK_HASKELL_EXECUTABLE", "");
        if path.is_empty() {
            return Err("Set AIVOBLOCK_HASKELL_EXECUTABLE to the aivoblock-core binary".into());
        }
        let core = HaskellValidatorClient::new(path).map_err(|e| format!("{e:?}"))?;
        if let Err(error) = core.exchange(HEALTH) {
            return Err(format!("Haskell core health check failed: {error:?}"));
        }
        SettlementEngine::Haskell(core)
    };
    let worker = if env_or("AIVOBLOCK_WORKER_BACKEND", "python") == "rust-reference" {
        WorkerBackend::RustReference
    } else {
        let python = env_or("AIVOBLOCK_PYTHON_EXECUTABLE", "python3");
        let script = env_or(
            "AIVOBLOCK_PYTHON_WORKER",
            "/opt/aivoblock/ai-engine/src/aivoblock_ai/worker.py",
        );
        let client = PythonWorkerClient::new(python, script).map_err(|e| format!("{e:?}"))?;
        if let Err(error) = client.exchange(HEALTH) {
            return Err(format!("Python AI worker health check failed: {error:?}"));
        }
        WorkerBackend::Python(client)
    };
    let p2p = env_or("AIVOBLOCK_P2P_LISTEN", "0.0.0.0:30333");
    let control = env_or("AIVOBLOCK_CONTROL_LISTEN", DEFAULT_CONTROL);
    let options = NodeOptions {
        name,
        config,
        keys,
        data_dir: PathBuf::from(env_or("AIVOBLOCK_DATA_DIR", "/var/lib/aivoblock")),
        p2p_listener: TcpListener::bind(&p2p).map_err(|e| format!("bind {p2p}: {e}"))?,
        control_listener: TcpListener::bind(&control)
            .map_err(|e| format!("bind {control}: {e}"))?,
        engine,
        worker,
    };
    start_node(options)?.wait()
}

pub fn init_command(args: &[String]) -> Result<(), String> {
    let flags = flags(args)?;
    let out = PathBuf::from(flags.get("out").ok_or("testnet-init requires --out DIR")?);
    let defaults = InitOptions::default();
    let options = InitOptions {
        validators: number(&flags, "validators", defaults.validators)?,
        network_name: flags.get("name").cloned().unwrap_or(defaults.network_name),
        host_pattern: flags
            .get("host-pattern")
            .cloned()
            .unwrap_or(defaults.host_pattern),
        port: number(&flags, "port", defaults.port)?,
        initial_reward: number(&flags, "initial-reward", defaults.initial_reward)?,
        halving_interval: number(&flags, "halving-interval", defaults.halving_interval)?,
        max_supply: number(&flags, "max-supply", defaults.max_supply)?,
        genesis_fee_pool: number(&flags, "fee-pool", defaults.genesis_fee_pool)?,
        client_balance: number(&flags, "client-balance", defaults.client_balance)?,
        round_timeout_ms: number(&flags, "round-timeout-ms", defaults.round_timeout_ms)?,
    };
    let generated = generate(&options, None)?;
    write_testnet(&out, &generated)?;
    println!(
        "{}",
        json!({
            "out": out.display().to_string(),
            "validators": options.validators,
            "authority_public_key": hex(&generated.authority.verifying_key().to_bytes()),
            "issuance": generated.config.issuance,
            "genesis_fee_pool": options.genesis_fee_pool,
        })
    );
    Ok(())
}

fn call(address: &str, request: &ControlRequest) -> Result<Value, String> {
    let answer = control_call(address, request)?;
    if let Some(error) = answer.get("error") {
        return Err(format!("node error: {error}"));
    }
    Ok(answer)
}

fn status(address: &str) -> Result<Value, String> {
    call(address, &ControlRequest::Status)
}

/// The repository's original deterministic workload, or a seeded synthetic
/// integer matrix for larger performance runs.
pub fn workload(kind: &str, rows: usize, features: usize, seed: u64) -> (Vec<i64>, Vec<Vec<i64>>) {
    if kind == "fixture" {
        return (vec![2, 3], vec![vec![1, 2], vec![3, 4], vec![-2, 5]]);
    }
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) % 2001) as i64 - 1000
    };
    let weights = (0..features).map(|_| next()).collect();
    let matrix = (0..rows)
        .map(|_| (0..features).map(|_| next()).collect())
        .collect();
    (weights, matrix)
}

/// Split rows into contiguous partitions, one per worker (in key order).
pub fn partition_rows(rows: usize, workers: &[[u8; 32]]) -> Vec<PartitionV3> {
    let used = workers.len().min(rows).max(1);
    let base = rows / used;
    let extra = rows % used;
    let mut start = 0u64;
    workers
        .iter()
        .take(used)
        .enumerate()
        .map(|(index, worker)| {
            let count = (base + usize::from(index < extra)) as u64;
            let partition = PartitionV3 {
                start,
                count,
                worker: *worker,
            };
            start += count;
            partition
        })
        .collect()
}

pub struct TaskSpec<'a> {
    pub sequence: u64,
    pub deadline_height: u64,
    pub fee_budget: u64,
    pub weights: &'a [i64],
    pub rows: &'a [Vec<i64>],
    pub partitions: Vec<PartitionV3>,
}

pub fn build_task(spec: TaskSpec<'_>, authority: &SigningKey) -> Result<AiTaskV3, String> {
    let task = AiTaskV3 {
        sequence: spec.sequence,
        created_ms: now_ms(),
        deadline_height: spec.deadline_height,
        fee_budget: spec.fee_budget,
        model_commitment: model_commitment_v3(spec.weights)?,
        input_commitment: input_commitment_v3(spec.rows, spec.weights.len())?,
        rules_commitment: rules_commitment_v1(),
        weights: spec.weights.to_vec(),
        rows: spec.rows.to_vec(),
        partitions: spec.partitions,
        task_id: [0; 32],
        authority_signature: [0; 64],
    };
    Ok(sign_task_v3(task, authority))
}

fn task_json(task: &AiTaskV3) -> Value {
    json!({
        "task_id": hex(&task.task_id),
        "sequence": task.sequence,
        "deadline_height": task.deadline_height,
        "fee_budget": task.fee_budget,
        "rows": task.rows.len(),
        "features": task.weights.len(),
        "partitions": task.partitions.iter().map(|p| json!({
            "worker": hex(&p.worker),
            "start": p.start,
            "count": p.count,
        })).collect::<Vec<_>>(),
    })
}

fn online_workers(status: &Value) -> Result<Vec<[u8; 32]>, String> {
    status["online_workers"]
        .as_array()
        .ok_or("Status lacks online_workers")?
        .iter()
        .map(|w| unhex_array(w.as_str().unwrap_or_default()))
        .collect()
}

/// Next free settlement height given the chain tip and pending tasks.
fn next_sequence(status: &Value) -> u64 {
    let height = status["height"].as_u64().unwrap_or(0);
    let pending = status["pending_tasks"]
        .as_array()
        .map(|tasks| {
            tasks
                .iter()
                .filter_map(|t| t["sequence"].as_u64())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    (height + 1).max(pending + 1)
}

fn submit_task_command(address: &str, flags: &BTreeMap<String, String>) -> Result<(), String> {
    let key_dir = PathBuf::from(flags.get("key-dir").map(String::as_str).unwrap_or("/keys"));
    let authority = read_key_file(&key_dir.join("authority.key"))?;
    let current = status(address)?;
    let sequence = number(flags, "sequence", next_sequence(&current))?;
    let kind = flags
        .get("workload")
        .map(String::as_str)
        .unwrap_or("fixture");
    let (weights, rows) = workload(
        kind,
        number(flags, "rows", 12)?,
        number(flags, "features", 4)?,
        number(flags, "seed", sequence)?,
    );
    let mut workers = online_workers(&current)?;
    let limit = number(flags, "max-workers", workers.len())?;
    workers.truncate(limit.max(1));
    let task = build_task(
        TaskSpec {
            sequence,
            deadline_height: sequence + number(flags, "deadline-delta", 20u64)?,
            fee_budget: number(flags, "fee-budget", 30u64)?,
            weights: &weights,
            rows: &rows,
            partitions: partition_rows(rows.len(), &workers),
        },
        &authority,
    )?;
    let verdict = call(
        address,
        &ControlRequest::SubmitTask {
            task: hex(&encode_task_v3(&task)),
        },
    )?;
    let mut output = task_json(&task);
    output["verdict"] = verdict["verdict"].clone();
    output["reason"] = verdict["reason"].clone();
    println!("{output}");
    if verdict["verdict"] != json!("accepted") {
        return Err(format!("Task was not accepted: {verdict}"));
    }
    Ok(())
}

fn wait_height_command(address: &str, flags: &BTreeMap<String, String>) -> Result<(), String> {
    let target: u64 = number(flags, "height", 1)?;
    let timeout = Duration::from_secs(number(flags, "timeout-secs", 60)?);
    let started = Instant::now();
    loop {
        if let Ok(current) = status(address) {
            if current["height"].as_u64().unwrap_or(0) >= target {
                println!(
                    "{}",
                    json!({ "height": current["height"], "state_root": current["state_root"] })
                );
                return Ok(());
            }
        }
        if started.elapsed() > timeout {
            return Err(format!("Timed out waiting for height {target}"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Connect to a peer port with an unregistered key; an honest node must
/// close the connection instead of admitting us.
fn probe_unauthorized(flags: &BTreeMap<String, String>) -> Result<(), String> {
    let peer = flags
        .get("peer")
        .ok_or("probe-unauthorized requires --peer")?;
    let addr = peer
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("No peer address")?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| format!("connect {peer}: {e}"))?;
    configure_stream(&stream).map_err(|e| e.to_string())?;
    let intruder = SigningKey::from_bytes(&random_bytes32());
    send_frame(&mut stream, &random_bytes32()).map_err(|e| e.to_string())?;
    let challenge: [u8; 32] = recv_frame(&mut stream)
        .map_err(|e| e.to_string())?
        .as_slice()
        .try_into()
        .map_err(|_| "Peer sent a malformed challenge")?;
    send_frame(&mut stream, &sign_hello(1, &intruder, &challenge)).map_err(|e| e.to_string())?;
    let _their_hello = recv_frame(&mut stream);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let admitted = recv_frame(&mut stream).is_ok();
    println!(
        "{}",
        json!({ "peer": peer, "intruder": hex(&intruder.verifying_key().to_bytes()), "admitted": admitted })
    );
    if admitted {
        return Err("Unregistered peer was admitted".into());
    }
    Ok(())
}

struct Suite {
    cases: Vec<Value>,
    passed: bool,
}

impl Suite {
    fn expect(&mut self, case: &str, answer: Result<Value, String>, allowed: &[&str]) -> Value {
        let answer = answer.unwrap_or_else(|error| json!({ "verdict": "error", "reason": error }));
        let verdict = answer["verdict"].as_str().unwrap_or("none").to_owned();
        let pass = allowed.contains(&verdict.as_str());
        self.passed &= pass;
        self.cases.push(json!({
            "case": case,
            "expected": allowed,
            "verdict": verdict,
            "reason": answer["reason"],
            "pass": pass,
        }));
        answer
    }
}

fn inject(address: &str, message: WireMessage) -> Result<Value, String> {
    call(address, &ControlRequest::Inject { message })
}

fn submit(address: &str, task: &AiTaskV3) -> Result<Value, String> {
    call(
        address,
        &ControlRequest::SubmitTask {
            task: hex(&encode_task_v3(task)),
        },
    )
}

fn fixture_task(
    authority: &SigningKey,
    sequence: u64,
    deadline_height: u64,
    fee_budget: u64,
    partitions: Vec<PartitionV3>,
) -> Result<AiTaskV3, String> {
    let (weights, rows) = workload("fixture", 0, 0, 0);
    build_task(
        TaskSpec {
            sequence,
            deadline_height,
            fee_budget,
            weights: &weights,
            rows: &rows,
            partitions,
        },
        authority,
    )
}

/// Adversarial integration suite against a running node. Requires at least
/// one finalized block, no pending tasks, and this node's authority, worker
/// and validator keys. Every forged, duplicated, expired, overlapping or
/// manipulated input must be refused without changing finalized state.
///
/// The legitimate task it submits settles at `next_free_sequence + 1`, so it
/// stays pending (no finalization race) until a task for
/// `next_free_sequence` is submitted.
fn adversarial_command(address: &str, flags: &BTreeMap<String, String>) -> Result<(), String> {
    let key_dir = PathBuf::from(flags.get("key-dir").map(String::as_str).unwrap_or("/keys"));
    let authority = read_key_file(&key_dir.join("authority.key"))?;
    let worker_key = read_key_file(&key_dir.join("worker.key"))?;
    let validator_key = read_key_file(&key_dir.join("validator.key"))?;
    let fee_budget = number(flags, "fee-budget", 12u64)?;
    let before = status(address)?;
    let height = before["height"].as_u64().unwrap_or(0);
    if height == 0 {
        return Err("Run the adversarial suite after at least one finalized block".into());
    }
    let own_worker = worker_key.verifying_key().to_bytes();
    let mut workers = online_workers(&before)?;
    if workers.len() < 2 || !workers.contains(&own_worker) {
        return Err("The suite needs this node's worker plus one more online worker".into());
    }
    workers.truncate(3);
    let next = next_sequence(&before);
    let future = next + 1;
    let deadline = future + 20;
    let honest = partition_rows(3, &workers);
    let mut suite = Suite {
        cases: Vec::new(),
        passed: true,
    };

    // Attacks on finalized history first, while the height is stable.
    let record = call(address, &ControlRequest::GetBlock { height })?;
    let block_hex = record["block"]
        .as_str()
        .ok_or("Missing block bytes")?
        .to_owned();
    let certificate = record["certificate"]
        .as_str()
        .ok_or("Missing certificate")?
        .to_owned();
    let finalized_bytes = unhex(&block_hex, MAX_BLOCK_BYTES)?;
    let mut tampered = finalized_bytes.clone();
    let middle = tampered.len() / 2;
    tampered[middle] ^= 1;
    let tampered_proposal = WireMessage::Proposal {
        height: height + 1,
        round: 0,
        block: hex(&tampered),
    };
    suite.expect(
        "tampered_block_proposal",
        inject(address, tampered_proposal),
        &["rejected"],
    );
    let replayed_proposal = WireMessage::Proposal {
        height: height + 1,
        round: 0,
        block: block_hex.clone(),
    };
    suite.expect(
        "finalized_block_replayed_as_new_proposal",
        inject(address, replayed_proposal),
        &["rejected"],
    );
    let replayed_commit = WireMessage::Commit {
        block: block_hex.clone(),
        certificate,
    };
    suite.expect(
        "finalized_commit_replayed",
        inject(address, replayed_commit),
        &["ignored"],
    );
    let settled = decode_block_v3(&finalized_bytes, None)?.task;
    suite.expect(
        "settled_task_resubmitted",
        submit(address, &settled),
        &["rejected"],
    );
    let commitment_hex = before["validator_set_commitment"]
        .as_str()
        .unwrap_or_default();
    let commitment: [u8; 32] = unhex_array(commitment_hex)?;
    let target = random_bytes32();
    let mut vote = sign_precommit_v1(&validator_key, commitment, height + 1, 0, target)?;
    vote.signature[0] ^= 1;
    suite.expect(
        "forged_vote_signature",
        inject(address, WireMessage::vote(height + 1, 0, &target, &vote)),
        &["rejected"],
    );
    let outsider = SigningKey::from_bytes(&random_bytes32());
    let foreign_vote = sign_precommit_v1(&outsider, commitment, height + 1, 0, target)?;
    let foreign_message = WireMessage::vote(height + 1, 0, &target, &foreign_vote);
    suite.expect(
        "vote_from_unknown_validator",
        inject(address, foreign_message),
        &["rejected"],
    );

    // Attacks on task admission.
    let mut forged = fixture_task(&authority, future, deadline, fee_budget, honest.clone())?;
    forged.authority_signature[0] ^= 1;
    suite.expect(
        "forged_task_signature",
        submit(address, &forged),
        &["rejected"],
    );
    let impostor = SigningKey::from_bytes(&random_bytes32());
    let foreign = fixture_task(&impostor, future, deadline, fee_budget, honest.clone())?;
    suite.expect(
        "untrusted_task_authority",
        submit(address, &foreign),
        &["rejected"],
    );
    let expired = fixture_task(&authority, height, height, fee_budget, honest.clone())?;
    suite.expect("expired_task", submit(address, &expired), &["rejected"]);
    let overlapping = vec![
        PartitionV3 {
            start: 0,
            count: 2,
            worker: workers[0],
        },
        PartitionV3 {
            start: 1,
            count: 2,
            worker: workers[1],
        },
    ];
    let overlap = fixture_task(&authority, future, deadline, fee_budget, overlapping)?;
    suite.expect(
        "overlapping_partitions",
        submit(address, &overlap),
        &["rejected"],
    );
    let intruder = SigningKey::from_bytes(&random_bytes32());
    let mut stranger = honest.clone();
    stranger[0].worker = intruder.verifying_key().to_bytes();
    let unregistered = fixture_task(&authority, future, deadline, fee_budget, stranger)?;
    suite.expect(
        "unregistered_worker",
        submit(address, &unregistered),
        &["rejected"],
    );
    let malformed = ControlRequest::SubmitTask {
        task: "00ff".into(),
    };
    suite.expect(
        "malformed_task_bytes",
        call(address, &malformed),
        &["rejected"],
    );

    // Attacks on work claims for a pending, legitimate task.
    let task = fixture_task(&authority, future, deadline, fee_budget, honest)?;
    suite.expect(
        "future_legitimate_task",
        submit(address, &task),
        &["accepted"],
    );
    suite.expect("duplicate_task", submit(address, &task), &["duplicate"]);
    let own_position = task
        .partitions
        .iter()
        .position(|p| p.worker == own_worker)
        .ok_or("Own worker missing from partitions")?;
    let own_index = own_position as u32;
    let other_index = u32::from(own_index == 0);
    let correct = expected_result_v3(&task, own_index)?;
    let mut wrong = correct;
    wrong[0] ^= 1;
    let manipulated = sign_claim_v3(&task, own_index, &worker_key, wrong)?;
    suite.expect(
        "manipulated_result_signed_by_assigned_worker",
        inject(address, WireMessage::claim(&manipulated)),
        &["rejected"],
    );
    let mut forged_claim = sign_claim_v3(&task, own_index, &worker_key, correct)?;
    forged_claim.signature[5] ^= 1;
    suite.expect(
        "forged_claim_signature",
        inject(address, WireMessage::claim(&forged_claim)),
        &["rejected"],
    );
    let other_result = expected_result_v3(&task, other_index)?;
    let misassigned = sign_claim_v3(&task, other_index, &worker_key, other_result)?;
    suite.expect(
        "claim_for_partition_assigned_to_another_worker",
        inject(address, WireMessage::claim(&misassigned)),
        &["rejected"],
    );
    let honest_claim = sign_claim_v3(&task, own_index, &worker_key, correct)?;
    suite.expect(
        "first_copy_of_honest_claim",
        inject(address, WireMessage::claim(&honest_claim)),
        &["accepted", "duplicate"],
    );
    suite.expect(
        "duplicate_claim",
        inject(address, WireMessage::claim(&honest_claim)),
        &["duplicate"],
    );

    let after = status(address)?;
    let stable = after["height"].as_u64() == Some(height);
    let passed = suite.passed && stable;
    println!(
        "{}",
        json!({
            "passed": passed,
            "height_unchanged": stable,
            "cases": suite.cases,
            "pending_future_task": task_json(&task),
            "next_free_sequence": next,
            "height": height,
        })
    );
    if !passed {
        return Err("Adversarial suite failed".into());
    }
    Ok(())
}

pub fn client_command(args: &[String]) -> Result<(), String> {
    let (command, rest) = args.split_first().ok_or("client requires a subcommand")?;
    let flags = flags(rest)?;
    let address = flags
        .get("control")
        .cloned()
        .unwrap_or_else(|| env_or("AIVOBLOCK_CONTROL_ADDRESS", DEFAULT_CONTROL));
    match command.as_str() {
        "status" => {
            let answer = status(&address)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&answer).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "submit-task" => submit_task_command(&address, &flags),
        "wait-height" => wait_height_command(&address, &flags),
        "adversarial" => adversarial_command(&address, &flags),
        "probe-unauthorized" => probe_unauthorized(&flags),
        other => Err(format!("Unknown client subcommand {other}")),
    }
}
