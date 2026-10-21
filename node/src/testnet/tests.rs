//! In-process multi-node tests: real loopback TCP, real RocksDB, real
//! consensus. Settlement uses Haskell when AIVOBLOCK_HASKELL_EXECUTABLE is
//! set (CI cross_language job) and the Rust mirror otherwise.

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::cli::{build_task, client_command, partition_rows, qmlp_workload, workload, TaskSpec};
use super::config::{verify_network_config, NetworkConfig, NodeKeys};
use super::control::{control_call, ControlRequest};
use super::init::{generate, write_testnet, GeneratedTestnet, InitOptions};
use super::node::{start_node, NodeHandle, NodeOptions};
use super::wire::WireMessage;
use super::worker::WorkerBackend;
use super::{hex, random_bytes32};
use crate::ai_block_v3::{encode_task_v3, expected_result_v3, sign_claim_v3, AiTaskV3};
use crate::ipc::haskell::HaskellValidatorClient;
use crate::issuance::IssuancePolicyV1;
use crate::settlement_v3::SettlementEngine;

fn engine() -> SettlementEngine {
    match std::env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE").filter(|v| !v.is_empty()) {
        Some(path) => SettlementEngine::Haskell(HaskellValidatorClient::new(path).unwrap()),
        None => SettlementEngine::RustMirror,
    }
}

struct Harness {
    generated: GeneratedTestnet,
    config: Arc<NetworkConfig>,
    dir: PathBuf,
    p2p: Vec<String>,
    control: Vec<String>,
    listeners: Vec<Option<(TcpListener, TcpListener)>>,
    nodes: Vec<Option<NodeHandle>>,
}

impl Harness {
    fn new(validators: usize) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "aivoblock-testnet-{}-{}",
            std::process::id(),
            hex(&random_bytes32()[..6])
        ));
        let mut p2p = Vec::new();
        let mut control = Vec::new();
        let mut listeners = Vec::new();
        for _ in 0..validators {
            let peer = TcpListener::bind("127.0.0.1:0").unwrap();
            let local = TcpListener::bind("127.0.0.1:0").unwrap();
            p2p.push(peer.local_addr().unwrap().to_string());
            control.push(local.local_addr().unwrap().to_string());
            listeners.push(Some((peer, local)));
        }
        let options = InitOptions {
            validators,
            round_timeout_ms: 700,
            ..InitOptions::default()
        };
        let generated = generate(&options, Some(&p2p)).unwrap();
        write_testnet(&dir, &generated).unwrap();
        let authority = generated.authority.verifying_key().to_bytes();
        let config = Arc::new(verify_network_config(&generated.config, &authority).unwrap());
        let mut harness = Harness {
            generated,
            config,
            dir,
            p2p,
            control,
            listeners,
            nodes: (0..validators).map(|_| None).collect(),
        };
        for index in 0..validators {
            harness.start(index);
        }
        harness
    }

    fn start(&mut self, index: usize) {
        let (peer, local) = self.listeners[index].take().unwrap_or_else(|| {
            (
                TcpListener::bind(&self.p2p[index]).unwrap(),
                TcpListener::bind(&self.control[index]).unwrap(),
            )
        });
        let keys = NodeKeys {
            validator: self.generated.validators[index].clone(),
            worker: self.generated.workers[index].clone(),
            authority: (index == 0).then(|| self.generated.authority.clone()),
        };
        let handle = start_node(NodeOptions {
            name: format!("node-{}", index + 1),
            config: Arc::clone(&self.config),
            keys,
            data_dir: self.dir.join(format!("data-{}", index + 1)),
            p2p_listener: peer,
            control_listener: local,
            engine: engine(),
            worker: WorkerBackend::RustReference,
        })
        .unwrap();
        self.nodes[index] = Some(handle);
    }

    fn stop(&mut self, index: usize) {
        if let Some(handle) = self.nodes[index].take() {
            handle.stop().unwrap();
        }
    }

    fn status(&self, index: usize) -> Value {
        control_call(&self.control[index], &ControlRequest::Status).unwrap()
    }

    fn wait(&self, what: &str, timeout_secs: u64, mut done: impl FnMut() -> bool) {
        let started = Instant::now();
        while !done() {
            assert!(
                started.elapsed() < Duration::from_secs(timeout_secs),
                "timed out waiting for {what}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_peers(&self, nodes: &[usize], peers: usize) {
        self.wait("peer connections", 30, || {
            nodes
                .iter()
                .all(|i| self.status(*i)["peers"].as_array().map(Vec::len) == Some(peers))
        });
    }

    fn wait_height(&self, nodes: &[usize], height: u64) {
        self.wait("finalized height", 60, || {
            nodes
                .iter()
                .all(|i| self.status(*i)["height"].as_u64() >= Some(height))
        });
    }

    /// Submit a seeded synthetic task through node 1, split among workers.
    fn submit(&self, sequence: u64, rows: usize, workers: &[[u8; 32]]) -> Value {
        let (weights, matrix) = workload("synthetic", rows, 3, sequence);
        let task = build_task(
            TaskSpec {
                sequence,
                deadline_height: sequence + 20,
                fee_budget: 30,
                weights: &weights,
                rows: &matrix,
                qmlp: None,
                partitions: partition_rows(matrix.len(), workers),
            },
            &self.generated.authority,
        )
        .unwrap();
        let request = ControlRequest::SubmitTask {
            task: hex(&encode_task_v3(&task)),
        };
        control_call(&self.control[0], &request).unwrap()
    }

    /// A seeded QMLP-V1 task, split among workers.
    fn qmlp_task(&self, sequence: u64, rows: usize, workers: &[[u8; 32]]) -> AiTaskV3 {
        let (model, matrix) = qmlp_workload(rows, sequence);
        build_task(
            TaskSpec {
                sequence,
                deadline_height: sequence + 20,
                fee_budget: 30,
                weights: &[],
                rows: &matrix,
                qmlp: Some(model),
                partitions: partition_rows(matrix.len(), workers),
            },
            &self.generated.authority,
        )
        .unwrap()
    }

    fn submit_task(&self, task: &AiTaskV3) -> Value {
        let request = ControlRequest::SubmitTask {
            task: hex(&encode_task_v3(task)),
        };
        control_call(&self.control[0], &request).unwrap()
    }

    fn inject(&self, message: WireMessage) -> Value {
        control_call(&self.control[0], &ControlRequest::Inject { message }).unwrap()
    }

    fn worker_key(&self, index: usize) -> [u8; 32] {
        self.generated.workers[index].verifying_key().to_bytes()
    }

    fn balance(status: &Value, worker: &[u8; 32]) -> u64 {
        status["accounts"][hex(worker)]["balance"]
            .as_u64()
            .unwrap_or(0)
    }

    fn assert_consistent(&self, nodes: &[usize]) {
        let first = self.status(nodes[0]);
        for index in nodes {
            let other = self.status(*index);
            assert_eq!(other["height"], first["height"]);
            assert_eq!(other["tip"], first["tip"]);
            assert_eq!(other["state_root"], first["state_root"]);
            assert_eq!(other["supply"], first["supply"]);
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        for index in 0..self.nodes.len() {
            self.stop(index);
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn four_validators_finalize_survive_a_crash_and_recover_after_restart() {
    let mut net = Harness::new(4);
    net.wait_peers(&[0, 1, 2, 3], 3);
    let all: Vec<[u8; 32]> = (0..4).map(|i| net.worker_key(i)).collect();
    let mut sorted = all.clone();
    sorted.sort();
    assert_eq!(net.submit(1, 8, &sorted)["verdict"], "accepted");
    net.wait_height(&[0, 1, 2, 3], 1);
    net.assert_consistent(&[0, 1, 2, 3]);
    let status = net.status(2);
    // fee budget 30 + scheduled issuance 50 = 80, split 2 rows each.
    for worker in &all {
        assert_eq!(Harness::balance(&status, worker), 20);
    }
    let policy = IssuancePolicyV1::new(50, 100, 2_000_000).unwrap();
    assert_eq!(policy.scheduled_issuance(1, 101_000).unwrap(), 50);
    assert_eq!(status["supply"], 101_050);

    // Crash one of four validators: 3 of 4 voting power still finalizes.
    net.stop(3);
    net.wait_peers(&[0, 1, 2], 2);
    let mut online: Vec<[u8; 32]> = all[..3].to_vec();
    online.sort();
    assert_eq!(net.submit(2, 6, &online)["verdict"], "accepted");
    net.wait_height(&[0, 1, 2], 2);
    net.assert_consistent(&[0, 1, 2]);
    assert_eq!(net.status(0)["blocks"].as_array().unwrap().len(), 2);

    // Restart: full local replay of height 1, then sync of height 2.
    net.start(3);
    net.wait_height(&[3], 2);
    net.assert_consistent(&[0, 1, 2, 3]);
    let restarted = net.status(3);
    assert_eq!(restarted["startup"]["replayed_blocks"], 1);
    assert!(restarted["counters"]["synced_blocks"].as_u64() >= Some(1));
    assert_eq!(restarted["blocks"].as_array().unwrap().len(), 2);
    let worker_four = net.worker_key(3);
    assert_eq!(Harness::balance(&restarted, &worker_four), 20);
}

#[test]
fn three_validators_finalize_quantized_mlp_work_and_reject_forged_results() {
    let net = Harness::new(3);
    net.wait_peers(&[0, 1, 2], 2);
    let mut workers: Vec<[u8; 32]> = (0..3).map(|i| net.worker_key(i)).collect();
    workers.sort();
    // A linear block, then QMLP blocks on top: both workloads share one chain.
    assert_eq!(net.submit(1, 3, &workers)["verdict"], "accepted");
    net.wait_height(&[0, 1, 2], 1);
    let before = net.status(0);
    let qmlp = net.qmlp_task(2, 12, &workers);
    assert_eq!(net.submit_task(&qmlp)["verdict"], "accepted");
    net.wait_height(&[0, 1, 2], 2);
    net.assert_consistent(&[0, 1, 2]);
    let after = net.status(1);
    let gained: Vec<u64> = workers
        .iter()
        .map(|w| Harness::balance(&after, w) - Harness::balance(&before, w))
        .collect();
    // Fee budget 30 plus issuance 50, split by 4 assigned rows per worker.
    assert_eq!(gained.iter().sum::<u64>(), 80);
    assert!(gained.iter().all(|g| (26..=27).contains(g)), "{gained:?}");
    assert_eq!(after["blocks"][1]["workload"], "qmlp-v1");

    // A pending QMLP task for height 4: forged and duplicate claims.
    let pending = net.qmlp_task(4, 6, &workers);
    assert_eq!(net.submit_task(&pending)["verdict"], "accepted");
    let own = net.worker_key(0);
    let index = pending
        .partitions
        .iter()
        .position(|p| p.worker == own)
        .unwrap() as u32;
    let key = &net.generated.workers[0];
    let correct = expected_result_v3(&pending, index).unwrap();
    let mut wrong = correct;
    wrong[0] ^= 1;
    let forged = sign_claim_v3(&pending, index, key, wrong).unwrap();
    assert_eq!(net.inject(WireMessage::claim(&forged))["verdict"], "rejected");
    let honest = sign_claim_v3(&pending, index, key, correct).unwrap();
    let first = net.inject(WireMessage::claim(&honest));
    assert!(first["verdict"] == "accepted" || first["verdict"] == "duplicate");
    assert_eq!(net.inject(WireMessage::claim(&honest))["verdict"], "duplicate");
    assert_eq!(net.submit(3, 3, &workers)["verdict"], "accepted");
    net.wait_height(&[0, 1, 2], 4);
    net.assert_consistent(&[0, 1, 2]);
    assert_eq!(net.status(2)["blocks"][3]["workload"], "qmlp-v1");
}

#[test]
fn three_validators_reject_adversarial_inputs_and_keep_consistent_state() {
    let net = Harness::new(3);
    net.wait_peers(&[0, 1, 2], 2);
    let mut workers: Vec<[u8; 32]> = (0..3).map(|i| net.worker_key(i)).collect();
    workers.sort();
    assert_eq!(net.submit(1, 3, &workers)["verdict"], "accepted");
    net.wait_height(&[0, 1, 2], 1);
    let key_dir = net.dir.join("node-1").display().to_string();
    let args: Vec<String> = [
        "adversarial",
        "--control",
        net.control[0].as_str(),
        "--key-dir",
        key_dir.as_str(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    client_command(&args).unwrap();
    // The suite left a legitimate task pending for height 3; fill height 2.
    assert_eq!(net.submit(2, 3, &workers)["verdict"], "accepted");
    net.wait_height(&[0, 1, 2], 3);
    net.assert_consistent(&[0, 1, 2]);
    let status = net.status(0);
    let counters = &status["counters"];
    assert!(counters["rejected_tasks"].as_u64() >= Some(6));
    assert!(counters["rejected_proposals"].as_u64() >= Some(2));
    let authority = hex(&net.generated.authority.verifying_key().to_bytes());
    assert_eq!(status["accounts"][authority.as_str()]["balance"], 100_000);
}
