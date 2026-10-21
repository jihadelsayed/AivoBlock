//! Testnet node: one event loop owns the chain, AI task pool and consensus
//! state; peer, dialer, control and worker threads only send it events.
//!
//! Consensus per height h (fixed validator set, n = 3f + 1 tolerates f
//! faulty validators):
//! - Proposals are deterministic: for a parent, task and verified claims
//!   every honest proposer builds the identical block, so leader changes
//!   cannot split honest votes.
//! - The leader of (h, r) is validators[(h + r) mod n] by public key.
//! - A validator precommits only after fully re-executing the block (task
//!   authority, assignment, worker signatures, Rust recomputation of every
//!   partition, issuance schedule, Haskell+Rust settlement, state root), and
//!   it precommits at most ONE block per height, ever: the vote lock is
//!   fsynced before the vote is sent and survives restarts.
//! - More than two thirds of voting power on (h, r, B) forms a quorum
//!   certificate. Only then is B persisted and are rewards credited.
//! - Without progress the round advances after a timeout and the next
//!   leader re-proposes (its locked block if it has one). A node also moves
//!   to a later round once validators holding more than one third of the
//!   power precommitted there; a single proposal never moves its round.
//! - Authenticated peers may still be Byzantine: proposals are accepted only
//!   from the round leader, identical bytes are executed once per height,
//!   and votes, rounds and early messages are bounded per validator.
//!
//! Limitations: no proposer signatures or equivocation evidence, no
//! validator-set changes, liveness assumes partial synchrony and a single
//! valid task per height. Safety does not depend on timing.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::bounds::{proposal_digest, ProposalAdmission, ProposalGate, VoteAdmission, VoteBook};
use super::chain::{rules, Chain, StartupReport, VoteLock};
use super::config::{NetworkConfig, NodeKeys, CHAIN_ID};
use super::control::{require_local_control_listener, spawn_control_server, ControlRequest};
use super::peer::{spawn_dialer, spawn_listener, AuthenticatedFrameLimiter, PeerLink, Transport};
use super::wire::{decode_block_hex, decode_claim, WireMessage};
use super::worker::WorkerBackend;
use super::{hex, unhex, unhex_array};
use crate::ai_block_v3::{
    build_block_v3, decode_task_v3, encode_task_v3, execute_block_v3, reward_batch_id_v3,
    sign_claim_v3, verify_claim_v3, verify_task_v3, AiTaskV3, ExecutedBlockV3, WorkClaimV3,
    MAX_BLOCK_BYTES,
};
use crate::issuance::ledger_supply;
use crate::settlement_v3::SettlementEngine;
use crate::validator_certificate::{
    decode_quorum_certificate_v1, encode_quorum_certificate_v1, sign_precommit_v1,
    verify_precommit_v1, QuorumCertificateV1, SignedPrecommitV1,
};

/// Bound the shared event backlog so authenticated peers cannot make memory
/// grow indefinitely when event producers outrun consensus processing.
const MAX_PENDING_NODE_EVENTS: usize = 128;
const MAX_PENDING_TASKS: usize = 256;
/// Avoid launching one expensive AI worker thread for every pending task.
const MAX_CONCURRENT_WORKER_EXECUTIONS: usize = 8;
const MAX_ORPHAN_CLAIMS: usize = 256;
const MAX_FUTURE_COMMITS: usize = 128;
const MAX_EARLY_MESSAGES: usize = 512;
/// Early (next-height) messages buffered per sender, so one validator
/// cannot crowd out the others or park megabytes of proposal bodies.
const MAX_EARLY_PROPOSALS_PER_SOURCE: usize = 4;
const MAX_EARLY_VOTES_PER_SOURCE: usize = 128;
/// Proposals further ahead than this many rounds are ignored. Rounds only
/// advance by timeout or by evidence from more than one third of the power.
const MAX_ROUND_LOOKAHEAD: u64 = 8;
const SYNC_BATCH: u64 = 64;
const TICK: Duration = Duration::from_millis(100);
const HEARTBEAT: Duration = Duration::from_millis(1_000);
const LOG_LINES: usize = 200;

pub enum Event {
    PeerUp(Box<PeerLink>),
    PeerDown {
        peer: [u8; 32],
        link_id: u64,
    },
    Inbound {
        peer: [u8; 32],
        link_id: u64,
        message: WireMessage,
    },
    HandshakeRejected {
        remote: String,
        reason: String,
    },
    MalformedMessage {
        peer: [u8; 32],
        reason: String,
    },
    Control {
        request: ControlRequest,
        reply: Sender<Value>,
    },
    WorkDone {
        task_id: [u8; 32],
        partition: u32,
        result: Result<[u8; 32], String>,
        elapsed_ms: u64,
    },
}

/// Outcome of processing one untrusted input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Accepted(String),
    Duplicate(String),
    Ignored(String),
    Rejected(String),
}

impl Verdict {
    pub fn to_json(&self) -> Value {
        let (verdict, reason) = match self {
            Self::Accepted(r) => ("accepted", r),
            Self::Duplicate(r) => ("duplicate", r),
            Self::Ignored(r) => ("ignored", r),
            Self::Rejected(r) => ("rejected", r),
        };
        json!({ "verdict": verdict, "reason": reason })
    }
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Counters {
    pub accepted_tasks: u64,
    pub rejected_tasks: u64,
    pub duplicate_tasks: u64,
    pub accepted_claims: u64,
    pub rejected_claims: u64,
    pub duplicate_claims: u64,
    pub accepted_proposals: u64,
    pub rejected_proposals: u64,
    pub votes_cast: u64,
    pub accepted_votes: u64,
    pub rejected_votes: u64,
    pub equivocating_votes: u64,
    pub refused_by_lock: u64,
    pub finalized_blocks: u64,
    pub synced_blocks: u64,
    pub stale_commits: u64,
    pub rejected_commits: u64,
    pub rejected_handshakes: u64,
    pub malformed_messages: u64,
    pub rounds_advanced: u64,
    pub worker_executions: u64,
    pub worker_failures: u64,
}

/// Track only workers that are actually executing or whose completion event
/// has not yet been handled. Failed tasks remain in `computing` to avoid
/// retries, but must still release a concurrency slot.
#[derive(Default)]
struct WorkerExecutionSlots {
    active: BTreeSet<([u8; 32], u32)>,
}

impl WorkerExecutionSlots {
    fn reserve(&mut self, task_id: [u8; 32], partition: u32) -> bool {
        if self.active.len() >= MAX_CONCURRENT_WORKER_EXECUTIONS {
            return false;
        }
        self.active.insert((task_id, partition))
    }

    fn release(&mut self, task_id: [u8; 32], partition: u32) {
        self.active.remove(&(task_id, partition));
    }
}

/// All peer, control, and worker threads share this bounded queue. A full
/// queue blocks senders rather than retaining unlimited unprocessed frames.
fn node_event_channel() -> (SyncSender<Event>, Receiver<Event>) {
    mpsc::sync_channel(MAX_PENDING_NODE_EVENTS)
}

pub struct NodeOptions {
    pub name: String,
    pub config: Arc<NetworkConfig>,
    pub keys: NodeKeys,
    pub data_dir: PathBuf,
    pub p2p_listener: TcpListener,
    pub control_listener: TcpListener,
    pub engine: SettlementEngine,
    pub worker: WorkerBackend,
}

pub struct NodeHandle {
    pub shutdown: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    main: Option<JoinHandle<Result<(), String>>>,
}

impl NodeHandle {
    /// Block until the node exits (it runs until shutdown).
    pub fn wait(mut self) -> Result<(), String> {
        let result = match self.main.take() {
            Some(main) => main.join().map_err(|_| "Node thread panicked".to_owned())?,
            None => Ok(()),
        };
        self.shutdown.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
        result
    }

    /// Stop all threads and release the database and sockets.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn stop(self) -> Result<(), String> {
        self.shutdown.store(true, Ordering::Relaxed);
        self.wait()
    }
}

pub fn start_node(options: NodeOptions) -> Result<NodeHandle, String> {
    let NodeOptions {
        name,
        config,
        keys,
        data_dir,
        p2p_listener,
        control_listener,
        engine,
        worker,
    } = options;
    // Reject an accidentally public operator API before opening chain storage
    // or starting network, worker and control threads.
    require_local_control_listener(&control_listener)?;
    let own_validator = keys.validator.verifying_key().to_bytes();
    if config.validator(&own_validator).is_none() {
        return Err("This node's validator key is not in the network configuration".into());
    }
    let own_worker = keys.worker.verifying_key().to_bytes();
    if !config.workers.contains(&own_worker) {
        return Err("This node's worker key is not registered in the configuration".into());
    }
    let chain_dir = data_dir.join("chain");
    std::fs::create_dir_all(&chain_dir).map_err(|e| format!("{}: {e}", chain_dir.display()))?;
    let (chain, startup) = Chain::open(&chain_dir, &config, &engine)?;
    let (events, receiver) = node_event_channel();
    let shutdown = Arc::new(AtomicBool::new(false));
    let connected = Arc::new(Mutex::new(BTreeSet::new()));
    let transport = Arc::new(Transport {
        chain_id: CHAIN_ID,
        key: keys.validator.clone(),
        allowed: config.validator_keys(),
        events: events.clone(),
        shutdown: Arc::clone(&shutdown),
        connected: Arc::clone(&connected),
        frame_limiter: Arc::new(Mutex::new(AuthenticatedFrameLimiter::default())),
    });
    let targets = config
        .validators
        .iter()
        .filter(|v| v.public_key > own_validator)
        .map(|v| (v.public_key, v.address.clone()))
        .collect();
    let threads = vec![
        spawn_listener(p2p_listener, Arc::clone(&transport)),
        spawn_dialer(Arc::clone(&transport), targets),
        spawn_control_server(control_listener, events.clone(), Arc::clone(&shutdown)),
    ];
    let mut node = Node {
        name,
        config,
        keys,
        own_validator,
        own_worker,
        chain,
        startup,
        engine,
        worker,
        events,
        connected,
        peers: BTreeMap::new(),
        peer_heights: BTreeMap::new(),
        tasks: BTreeMap::new(),
        task_seen_at: BTreeMap::new(),
        claims: BTreeMap::new(),
        orphan_claims: VecDeque::new(),
        computing: BTreeSet::new(),
        worker_slots: WorkerExecutionSlots::default(),
        round: 0,
        round_started: None,
        proposed_round: None,
        voted_rounds: BTreeSet::new(),
        lock: None,
        candidates: BTreeMap::new(),
        proposals: ProposalGate::default(),
        votes: VoteBook::default(),
        future_commits: BTreeMap::new(),
        early_messages: Vec::new(),
        last_status: Instant::now(),
        last_sync_request: None,
        counters: Counters::default(),
        log: VecDeque::new(),
        finality_ms: Vec::new(),
        started: Instant::now(),
    };
    node.lock = node.chain.load_vote_lock(node.chain.height + 1)?;
    node.note(format!(
        "started at height {} (replayed {} blocks in {} ms, genesis initialized: {}), engine {}, worker {}",
        node.chain.height,
        node.startup.replayed_blocks,
        node.startup.replay_ms,
        node.startup.initialized_genesis,
        node.engine.name(),
        node.worker.name()
    ));
    let loop_shutdown = Arc::clone(&shutdown);
    let main = thread::spawn(move || node.run(receiver, loop_shutdown));
    Ok(NodeHandle {
        shutdown,
        threads,
        main: Some(main),
    })
}

struct Node {
    name: String,
    config: Arc<NetworkConfig>,
    keys: NodeKeys,
    own_validator: [u8; 32],
    own_worker: [u8; 32],
    chain: Chain,
    startup: StartupReport,
    engine: SettlementEngine,
    worker: WorkerBackend,
    events: SyncSender<Event>,
    connected: Arc<Mutex<BTreeSet<[u8; 32]>>>,
    peers: BTreeMap<[u8; 32], PeerLink>,
    peer_heights: BTreeMap<[u8; 32], u64>,
    tasks: BTreeMap<[u8; 32], AiTaskV3>,
    task_seen_at: BTreeMap<[u8; 32], Instant>,
    claims: BTreeMap<[u8; 32], BTreeMap<u32, WorkClaimV3>>,
    orphan_claims: VecDeque<WorkClaimV3>,
    computing: BTreeSet<([u8; 32], u32)>,
    worker_slots: WorkerExecutionSlots,
    round: u64,
    round_started: Option<Instant>,
    proposed_round: Option<u64>,
    voted_rounds: BTreeSet<u64>,
    lock: Option<VoteLock>,
    candidates: BTreeMap<[u8; 32], ExecutedBlockV3>,
    proposals: ProposalGate,
    votes: VoteBook,
    future_commits: BTreeMap<u64, (Vec<u8>, QuorumCertificateV1)>,
    /// Next-height messages with their authenticated sender (None: local).
    early_messages: Vec<(Option<[u8; 32]>, WireMessage)>,
    last_status: Instant,
    last_sync_request: Option<Instant>,
    counters: Counters,
    log: VecDeque<String>,
    finality_ms: Vec<u64>,
    started: Instant,
}

fn parse_vote(
    block_id: &str,
    validator: &str,
    signature: &str,
) -> Result<([u8; 32], SignedPrecommitV1), String> {
    let vote = SignedPrecommitV1 {
        public_key: unhex_array(validator)?,
        signature: unhex_array(signature)?,
    };
    Ok((unhex_array(block_id)?, vote))
}

fn short(id: &[u8; 32]) -> String {
    hex(&id[..6])
}

impl Node {
    fn run(mut self, receiver: Receiver<Event>, shutdown: Arc<AtomicBool>) -> Result<(), String> {
        let mut last_tick = Instant::now();
        while !shutdown.load(Ordering::Relaxed) {
            match receiver.recv_timeout(TICK) {
                Ok(event) => self.handle(event),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if last_tick.elapsed() >= TICK {
                last_tick = Instant::now();
                self.tick();
            }
        }
        for peer in self.peers.values() {
            peer.close();
        }
        self.note("stopped".into());
        Ok(())
    }

    fn note(&mut self, line: String) {
        let stamped = format!(
            "[{}] +{}ms h={} r={} {line}",
            self.name,
            self.started.elapsed().as_millis(),
            self.chain.height,
            self.round
        );
        println!("{stamped}");
        if self.log.len() >= LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(stamped);
    }

    fn is_settled(&self, task_id: &[u8; 32]) -> bool {
        let batch = reward_batch_id_v3(task_id);
        self.chain.ledger.settled_batches.contains(&batch)
    }

    fn next_height(&self) -> u64 {
        self.chain.height + 1
    }

    fn gossip(&self, except: Option<[u8; 32]>, message: &WireMessage) {
        for (key, link) in &self.peers {
            if Some(*key) != except {
                let _ = link.send(message);
            }
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::PeerUp(link) => {
                let name = self
                    .config
                    .validator(&link.key)
                    .map(|v| v.name.clone())
                    .unwrap_or_default();
                if let Some(old) = self.peers.insert(link.key, (*link).clone()) {
                    old.close();
                }
                if let Ok(mut set) = self.connected.lock() {
                    set.insert(link.key);
                }
                let _ = link.send(&self.status_message());
                self.note(format!(
                    "authenticated peer {name} ({}) via {} {}",
                    short(&link.key),
                    if link.outbound {
                        "dial to"
                    } else {
                        "accept from"
                    },
                    link.remote
                ));
            }
            Event::PeerDown { peer, link_id } => {
                if self.peers.get(&peer).is_some_and(|link| link.id == link_id) {
                    self.peers.remove(&peer);
                    self.peer_heights.remove(&peer);
                    if let Ok(mut set) = self.connected.lock() {
                        set.remove(&peer);
                    }
                    self.note(format!("peer {} disconnected", short(&peer)));
                }
            }
            Event::Inbound {
                peer,
                link_id,
                message,
            } => {
                // Ignore frames still in flight from a replaced connection.
                if !self.peers.get(&peer).is_some_and(|link| link.id == link_id) {
                    return;
                }
                let kind = message.kind();
                let verdict = self.on_message(Some(peer), message);
                if let Verdict::Rejected(reason) = &verdict {
                    let line = format!("rejected {kind} from {}: {reason}", short(&peer));
                    self.note(line);
                }
            }
            Event::HandshakeRejected { remote, reason } => {
                self.counters.rejected_handshakes += 1;
                self.note(format!("rejected handshake from {remote}: {reason}"));
            }
            Event::MalformedMessage { peer, reason } => {
                self.counters.malformed_messages += 1;
                self.note(format!("malformed message from {}: {reason}", short(&peer)));
            }
            Event::Control { request, reply } => {
                let answer = self.on_control(request);
                let _ = reply.send(answer);
            }
            Event::WorkDone {
                task_id,
                partition,
                result,
                elapsed_ms,
            } => self.on_work_done(task_id, partition, result, elapsed_ms),
        }
    }

    fn status_message(&self) -> WireMessage {
        WireMessage::Status {
            height: self.chain.height,
            tip: hex(&self.chain.tip),
        }
    }

    fn on_message(&mut self, source: Option<[u8; 32]>, message: WireMessage) -> Verdict {
        match message {
            WireMessage::Status { height, .. } => {
                if let Some(peer) = source {
                    self.peer_heights.insert(peer, height);
                    if height > self.chain.height {
                        self.request_sync(peer);
                    }
                }
                Verdict::Ignored("status".into())
            }
            WireMessage::Task { task } => self.on_task(source, &task),
            WireMessage::Claim {
                task_id,
                partition,
                worker,
                result,
                signature,
            } => match decode_claim(&task_id, partition, &worker, &result, &signature) {
                Ok(claim) => self.on_claim(source, claim),
                Err(error) => {
                    self.counters.rejected_claims += 1;
                    Verdict::Rejected(format!("malformed claim: {error}"))
                }
            },
            WireMessage::Proposal {
                height,
                round,
                block,
            } => self.on_proposal(source, height, round, &block),
            WireMessage::Vote {
                height,
                round,
                block_id,
                validator,
                signature,
            } => self.on_vote(source, height, round, &block_id, &validator, &signature),
            WireMessage::Commit { block, certificate } => {
                self.on_commit(source, &block, &certificate)
            }
            WireMessage::GetBlocks { from_height } => {
                if let Some(peer) = source {
                    self.serve_blocks(peer, from_height);
                }
                Verdict::Ignored("block request".into())
            }
        }
    }

    fn request_sync(&mut self, peer: [u8; 32]) {
        if self
            .last_sync_request
            .is_some_and(|at| at.elapsed() < Duration::from_millis(750))
        {
            return;
        }
        self.last_sync_request = Some(Instant::now());
        if let Some(link) = self.peers.get(&peer) {
            let _ = link.send(&WireMessage::GetBlocks {
                from_height: self.chain.height + 1,
            });
        }
    }

    fn serve_blocks(&mut self, peer: [u8; 32], from_height: u64) {
        let Some(link) = self.peers.get(&peer).cloned() else {
            return;
        };
        let first = from_height.max(1);
        let last = self.chain.height.min(first.saturating_add(SYNC_BATCH - 1));
        for height in first..=last {
            match self.chain.record_at(height) {
                Ok(Some(record)) => {
                    let _ = link.send(&WireMessage::Commit {
                        block: hex(&record.bytes),
                        certificate: hex(&record.certificate),
                    });
                }
                _ => break,
            }
        }
    }

    fn on_task(&mut self, source: Option<[u8; 32]>, encoded: &str) -> Verdict {
        let task = match unhex(encoded, MAX_BLOCK_BYTES).and_then(|raw| decode_task_v3(&raw)) {
            Ok(task) => task,
            Err(error) => {
                self.counters.rejected_tasks += 1;
                return Verdict::Rejected(format!("malformed task: {error}"));
            }
        };
        if self.tasks.contains_key(&task.task_id) {
            self.counters.duplicate_tasks += 1;
            return Verdict::Duplicate(format!("task {} already pending", short(&task.task_id)));
        }
        let next = self.next_height();
        let reason = if self.is_settled(&task.task_id) {
            Some("task already settled (replay)".to_owned())
        } else if task.deadline_height < next {
            Some(format!(
                "task expired: deadline {} is below next height {next}",
                task.deadline_height
            ))
        } else if task.sequence < next {
            Some(format!(
                "stale task sequence {} (next height {next})",
                task.sequence
            ))
        } else if self.tasks.len() >= MAX_PENDING_TASKS {
            Some("task pool is full".to_owned())
        } else {
            verify_task_v3(&task, &rules(&self.config), task.sequence).err()
        };
        if let Some(reason) = reason {
            self.counters.rejected_tasks += 1;
            return Verdict::Rejected(reason);
        }
        let task_id = task.task_id;
        let summary = format!(
            "task {} for height {} ({} rows, {} partitions, fee budget {})",
            short(&task_id),
            task.sequence,
            task.rows.len(),
            task.partitions.len(),
            task.fee_budget
        );
        self.gossip(
            source,
            &WireMessage::Task {
                task: hex(&encode_task_v3(&task)),
            },
        );
        self.tasks.insert(task_id, task);
        self.task_seen_at.insert(task_id, Instant::now());
        self.counters.accepted_tasks += 1;
        self.note(format!("accepted {summary}"));
        let orphans: Vec<WorkClaimV3> = self.orphan_claims.drain(..).collect();
        for claim in orphans {
            self.on_claim(None, claim);
        }
        self.schedule_work();
        Verdict::Accepted(summary)
    }

    fn on_claim(&mut self, source: Option<[u8; 32]>, claim: WorkClaimV3) -> Verdict {
        if !self.tasks.contains_key(&claim.task_id) {
            if self.is_settled(&claim.task_id) {
                self.counters.rejected_claims += 1;
                return Verdict::Rejected("claim for an already settled task".into());
            }
            if self.orphan_claims.len() >= MAX_ORPHAN_CLAIMS {
                self.orphan_claims.pop_front();
            }
            self.orphan_claims.push_back(claim);
            return Verdict::Ignored("claim buffered until its task arrives".into());
        }
        let task = &self.tasks[&claim.task_id];
        if let Err(error) = verify_claim_v3(task, &claim) {
            self.counters.rejected_claims += 1;
            return Verdict::Rejected(error);
        }
        let partitions = task.partitions.len();
        let slot = self.claims.entry(claim.task_id).or_default();
        match slot.get(&claim.partition) {
            Some(existing) if *existing == claim => {
                self.counters.duplicate_claims += 1;
                return Verdict::Duplicate("claim already recorded".into());
            }
            Some(_) => {
                self.counters.rejected_claims += 1;
                return Verdict::Rejected("conflicting claim for a recorded partition".into());
            }
            None => {
                slot.insert(claim.partition, claim);
            }
        }
        let have = slot.len();
        self.counters.accepted_claims += 1;
        self.gossip(source, &WireMessage::claim(&claim));
        let line = format!(
            "verified claim task {} partition {} from worker {} ({have}/{partitions})",
            short(&claim.task_id),
            claim.partition,
            short(&claim.worker)
        );
        self.note(line.clone());
        Verdict::Accepted(line)
    }

    fn schedule_work(&mut self) {
        let next = self.next_height();
        for (task_id, task) in &self.tasks {
            if task.sequence < next {
                continue;
            }
            let Some(index) = task
                .partitions
                .iter()
                .position(|p| p.worker == self.own_worker)
            else {
                continue;
            };
            let partition = index as u32;
            let done = self
                .claims
                .get(task_id)
                .is_some_and(|claims| claims.contains_key(&partition));
            if done || self.computing.contains(&(*task_id, partition)) {
                continue;
            }
            // Reserve before launching a thread; anything beyond the cap
            // stays queued in `tasks` for the next scheduler tick.
            if !self.worker_slots.reserve(*task_id, partition) {
                break;
            }
            self.computing.insert((*task_id, partition));
            let backend = self.worker.clone();
            let task = task.clone();
            let worker = self.own_worker;
            let events = self.events.clone();
            thread::spawn(move || {
                let started = Instant::now();
                let result = backend.execute(&task, partition, &worker);
                let _ = events.send(Event::WorkDone {
                    task_id: task.task_id,
                    partition,
                    result,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                });
            });
        }
    }

    fn on_work_done(
        &mut self,
        task_id: [u8; 32],
        partition: u32,
        result: Result<[u8; 32], String>,
        elapsed_ms: u64,
    ) {
        // Release the execution slot on success, failure, or a stale result
        // whose task was finalized/removed while the worker was running.
        self.worker_slots.release(task_id, partition);
        let Some(task) = self.tasks.get(&task_id) else {
            self.computing.remove(&(task_id, partition));
            return;
        };
        let signed = result.and_then(|c| sign_claim_v3(task, partition, &self.keys.worker, c));
        match signed {
            // A failed partition stays marked as computing: no retry storm.
            Ok(claim) => {
                self.computing.remove(&(task_id, partition));
                self.counters.worker_executions += 1;
                self.note(format!(
                    "computed partition {partition} of task {} with {} in {elapsed_ms} ms",
                    short(&task_id),
                    self.worker.name()
                ));
                self.on_claim(None, claim);
            }
            Err(error) => {
                self.counters.worker_failures += 1;
                self.note(format!(
                    "worker failed on task {}: {error}",
                    short(&task_id)
                ));
            }
        }
    }

    fn complete_task(&self, height: u64) -> Option<(&AiTaskV3, Vec<WorkClaimV3>)> {
        self.tasks
            .values()
            .filter(|task| task.sequence == height)
            .find_map(|task| {
                let claims = self.claims.get(&task.task_id)?;
                (claims.len() == task.partitions.len())
                    .then(|| (task, claims.values().copied().collect()))
            })
    }

    fn has_pending_work(&self) -> bool {
        self.lock.is_some()
            || !self.candidates.is_empty()
            || self.complete_task(self.next_height()).is_some()
    }

    fn round_timeout(&self) -> Duration {
        Duration::from_millis(self.config.round_timeout_ms * (1 + self.round.min(3)))
    }

    fn tick(&mut self) {
        if self.last_status.elapsed() >= HEARTBEAT {
            self.last_status = Instant::now();
            self.gossip(None, &self.status_message());
            let behind = self
                .peer_heights
                .iter()
                .find(|(_, height)| **height > self.chain.height)
                .map(|(peer, _)| *peer);
            if let Some(peer) = behind {
                self.request_sync(peer);
            }
        }
        self.schedule_work();
        if !self.has_pending_work() {
            self.round_started = None;
            return;
        }
        let started = *self.round_started.get_or_insert_with(Instant::now);
        if started.elapsed() > self.round_timeout() {
            self.round = self.round.saturating_add(1);
            self.round_started = Some(Instant::now());
            self.counters.rounds_advanced += 1;
            let leader = self.config.leader(self.next_height(), self.round);
            self.note(format!("round timeout; new leader {}", short(&leader)));
        }
        self.maybe_propose();
    }

    fn maybe_propose(&mut self) {
        let height = self.next_height();
        let round = self.round;
        if self.config.leader(height, round) != self.own_validator
            || self.proposed_round == Some(round)
        {
            return;
        }
        let built = if let Some((id, bytes)) = &self.lock {
            match self.candidates.get(id) {
                Some(executed) => Ok(executed.clone()),
                None => execute_block_v3(
                    &self.chain.parent(),
                    &rules(&self.config),
                    &self.engine,
                    bytes,
                    Some(id),
                ),
            }
        } else if let Some((task, claims)) = self.complete_task(height) {
            build_block_v3(
                &self.chain.parent(),
                &rules(&self.config),
                &self.engine,
                task,
                &claims,
            )
        } else {
            return;
        };
        self.proposed_round = Some(round);
        let executed = match built {
            Ok(executed) => executed,
            Err(error) => {
                self.note(format!("could not build a proposal: {error}"));
                return;
            }
        };
        let id = executed.id;
        self.gossip(
            None,
            &WireMessage::Proposal {
                height,
                round,
                block: hex(&executed.bytes),
            },
        );
        self.note(format!("proposed block {} for round {round}", short(&id)));
        self.candidates.insert(id, executed);
        self.try_vote(round, id);
        self.check_quorum();
    }

    fn on_proposal(
        &mut self,
        source: Option<[u8; 32]>,
        height: u64,
        round: u64,
        block: &str,
    ) -> Verdict {
        if height <= self.chain.height {
            return Verdict::Ignored("proposal for an already finalized height".into());
        }
        if height > self.next_height() + 1 {
            return Verdict::Ignored("proposal for a future height".into());
        }
        if block.len() > 2 * MAX_BLOCK_BYTES {
            self.counters.rejected_proposals += 1;
            return Verdict::Rejected("oversized proposal".into());
        }
        // Proposals are never relayed, so an authenticated sender must lead
        // the proposal's round. Local operator injections carry no peer
        // identity and are still fully verified below.
        if source.is_some_and(|peer| self.config.leader(height, round) != peer) {
            self.counters.rejected_proposals += 1;
            return Verdict::Rejected(
                "proposal from a validator that does not lead its round".into(),
            );
        }
        if height == self.next_height() + 1 {
            let message = WireMessage::Proposal {
                height,
                round,
                block: block.to_owned(),
            };
            return self.buffer_early(source, message);
        }
        if round > self.round.saturating_add(MAX_ROUND_LOOKAHEAD) {
            return Verdict::Ignored("proposal round is too far ahead".into());
        }
        let raw = match decode_block_hex(block) {
            Ok(raw) => raw,
            Err(error) => {
                self.counters.rejected_proposals += 1;
                return Verdict::Rejected(format!("invalid proposal: {error}"));
            }
        };
        let digest = proposal_digest(&raw);
        let id = match self.proposals.admit(&digest, source.as_ref()) {
            ProposalAdmission::Known(id) => id,
            ProposalAdmission::Rejected(error) => {
                self.counters.rejected_proposals += 1;
                return Verdict::Rejected(format!("invalid proposal: {error}"));
            }
            ProposalAdmission::OverBudget => {
                return Verdict::Ignored("leader exceeded its distinct proposals".into());
            }
            ProposalAdmission::Execute => {
                let executed = execute_block_v3(
                    &self.chain.parent(),
                    &rules(&self.config),
                    &self.engine,
                    &raw,
                    None,
                );
                let outcome = executed.as_ref().map(|e| e.id).map_err(Clone::clone);
                self.proposals.record(digest, source, outcome);
                match executed {
                    Ok(executed) => {
                        let id = executed.id;
                        self.candidates.entry(id).or_insert(executed);
                        id
                    }
                    Err(error) => {
                        self.counters.rejected_proposals += 1;
                        return Verdict::Rejected(format!("invalid proposal: {error}"));
                    }
                }
            }
        };
        self.counters.accepted_proposals += 1;
        self.try_vote(round, id);
        self.check_quorum();
        Verdict::Accepted(format!(
            "independently verified block {} at height {height} round {round}",
            short(&id)
        ))
    }

    /// Buffer a next-height proposal or vote under a per-sender quota.
    fn buffer_early(&mut self, source: Option<[u8; 32]>, message: WireMessage) -> Verdict {
        let kind = message.kind();
        let proposal = matches!(message, WireMessage::Proposal { .. });
        let quota = if proposal {
            MAX_EARLY_PROPOSALS_PER_SOURCE
        } else {
            MAX_EARLY_VOTES_PER_SOURCE
        };
        let queued = self
            .early_messages
            .iter()
            .filter(|(from, queued)| {
                *from == source && matches!(queued, WireMessage::Proposal { .. }) == proposal
            })
            .count();
        if queued >= quota || self.early_messages.len() >= MAX_EARLY_MESSAGES {
            return Verdict::Ignored(format!("early {kind} buffer is full"));
        }
        self.early_messages.push((source, message));
        Verdict::Ignored(format!("{kind} for the next height buffered"))
    }

    fn try_vote(&mut self, round: u64, id: [u8; 32]) {
        if self.voted_rounds.contains(&round) {
            return;
        }
        let height = self.next_height();
        let locked = self.lock.as_ref().map(|(locked, _)| *locked);
        match locked {
            Some(locked) if locked != id => {
                self.counters.refused_by_lock += 1;
                let line = format!(
                    "refused to vote for {}: locked on {}",
                    short(&id),
                    short(&locked)
                );
                self.note(line);
                return;
            }
            Some(_) => {}
            None => {
                let Some(bytes) = self.candidates.get(&id).map(|e| e.bytes.clone()) else {
                    return;
                };
                if let Err(error) = self.chain.store_vote_lock(height, &id, &bytes) {
                    self.note(format!("cannot persist vote lock: {error}"));
                    return;
                }
                self.lock = Some((id, bytes));
            }
        }
        let commitment = self.config.validator_set.commitment();
        let vote = match sign_precommit_v1(&self.keys.validator, commitment, height, round, id) {
            Ok(vote) => vote,
            Err(error) => {
                self.note(format!("cannot sign precommit: {error}"));
                return;
            }
        };
        self.voted_rounds.insert(round);
        self.counters.votes_cast += 1;
        self.gossip(None, &WireMessage::vote(height, round, &id, &vote));
        self.votes.insert(round, id, vote);
        self.note(format!(
            "precommitted block {} in round {round}",
            short(&id)
        ));
    }

    fn on_vote(
        &mut self,
        source: Option<[u8; 32]>,
        height: u64,
        round: u64,
        block_id: &str,
        validator: &str,
        signature: &str,
    ) -> Verdict {
        let (id, vote) = match parse_vote(block_id, validator, signature) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.counters.rejected_votes += 1;
                return Verdict::Rejected(format!("malformed vote: {error}"));
            }
        };
        if height <= self.chain.height {
            return Verdict::Ignored("vote for an already finalized height".into());
        }
        if height == self.next_height() + 1 {
            return self.buffer_early(source, WireMessage::vote(height, round, &id, &vote));
        }
        if height > self.next_height() {
            if let Some(peer) = source {
                self.request_sync(peer);
            }
            return Verdict::Ignored("vote for a future height".into());
        }
        let set = &self.config.validator_set;
        if let Err(error) = verify_precommit_v1(set, height, round, id, &vote) {
            self.counters.rejected_votes += 1;
            return Verdict::Rejected(error);
        }
        match self.votes.insert(round, id, vote) {
            VoteAdmission::Fresh => {}
            VoteAdmission::Duplicate => {
                return Verdict::Duplicate("vote already counted".into());
            }
            VoteAdmission::Stale => {
                return Verdict::Ignored("vote older than retained rounds".into());
            }
            VoteAdmission::Equivocation(first) => {
                self.counters.rejected_votes += 1;
                self.counters.equivocating_votes += 1;
                return Verdict::Rejected(format!(
                    "equivocating precommit: validator already precommitted {}",
                    short(&first)
                ));
            }
        }
        self.counters.accepted_votes += 1;
        self.skip_to_honest_round();
        self.check_quorum();
        Verdict::Accepted(format!("authenticated precommit for {}", short(&id)))
    }

    /// Move to a later round only on precommits from validators holding
    /// more than one third of the power, so at least one honest validator
    /// is already there. A Byzantine minority cannot drive the round.
    fn skip_to_honest_round(&mut self) {
        let floor = self.votes.honest_round_floor(&self.config.validator_set);
        let Some(round) = floor.filter(|round| *round > self.round) else {
            return;
        };
        self.round = round;
        self.round_started = Some(Instant::now());
        self.counters.rounds_advanced += 1;
        self.note(format!(
            "advanced to round {round} on precommits from over one third of the power"
        ));
    }

    fn check_quorum(&mut self) {
        let Some((round, id, votes)) = self.votes.quorum(&self.config.validator_set) else {
            return;
        };
        let Some(executed) = self.candidates.get(&id).cloned() else {
            // Quorum without the block body: a peer's Commit will deliver it.
            return;
        };
        let certificate = QuorumCertificateV1 {
            chain_id: CHAIN_ID,
            height: executed.block.header.height,
            round,
            block_id: id,
            validator_set_commitment: self.config.validator_set.commitment(),
            votes,
        };
        if let Err(error) = self.chain.finalize(&self.config, &executed, &certificate) {
            self.note(format!("finalization failed: {error}"));
            return;
        }
        self.counters.finalized_blocks += 1;
        self.gossip(
            None,
            &WireMessage::Commit {
                block: hex(&executed.bytes),
                certificate: hex(&encode_quorum_certificate_v1(&certificate)),
            },
        );
        self.after_finalize(&executed, &certificate, "quorum");
    }

    fn on_commit(&mut self, source: Option<[u8; 32]>, block: &str, certificate: &str) -> Verdict {
        let parsed = decode_block_hex(block).and_then(|raw| {
            let qc =
                unhex(certificate, 64 * 1024).and_then(|c| decode_quorum_certificate_v1(&c))?;
            Ok((raw, qc))
        });
        let (raw, certificate) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                self.counters.rejected_commits += 1;
                return Verdict::Rejected(format!("malformed commit: {error}"));
            }
        };
        if certificate.height <= self.chain.height {
            self.counters.stale_commits += 1;
            return Verdict::Ignored("commit for an already finalized height".into());
        }
        if certificate.height > self.next_height() {
            if self.future_commits.len() < MAX_FUTURE_COMMITS {
                self.future_commits
                    .insert(certificate.height, (raw, certificate));
            }
            if let Some(peer) = source {
                self.request_sync(peer);
            }
            return Verdict::Ignored("buffered commit for a future height".into());
        }
        let verdict = self.apply_commit(source, raw, certificate);
        loop {
            let next = self.next_height();
            let Some((raw, certificate)) = self.future_commits.remove(&next) else {
                break;
            };
            let applied = self.apply_commit(None, raw, certificate);
            if !matches!(applied, Verdict::Accepted(_)) {
                break;
            }
        }
        self.future_commits
            .retain(|height, _| *height > self.chain.height);
        verdict
    }

    fn apply_commit(
        &mut self,
        source: Option<[u8; 32]>,
        raw: Vec<u8>,
        certificate: QuorumCertificateV1,
    ) -> Verdict {
        let verified = self
            .chain
            .verify_commit(&self.config, &self.engine, &raw, &certificate);
        let executed = match verified {
            Ok(executed) => executed,
            Err(error) => {
                self.counters.rejected_commits += 1;
                return Verdict::Rejected(format!("invalid commit: {error}"));
            }
        };
        if let Err(error) = self.chain.finalize(&self.config, &executed, &certificate) {
            self.counters.rejected_commits += 1;
            return Verdict::Rejected(format!("could not persist commit: {error}"));
        }
        self.counters.synced_blocks += 1;
        self.gossip(
            source,
            &WireMessage::Commit {
                block: hex(&raw),
                certificate: hex(&encode_quorum_certificate_v1(&certificate)),
            },
        );
        self.after_finalize(&executed, &certificate, "certified commit");
        Verdict::Accepted(format!(
            "finalized height {} from certificate",
            certificate.height
        ))
    }

    fn after_finalize(
        &mut self,
        executed: &ExecutedBlockV3,
        certificate: &QuorumCertificateV1,
        via: &str,
    ) {
        let height = executed.block.header.height;
        let task_id = executed.block.task.task_id;
        if let Some(seen) = self.task_seen_at.remove(&task_id) {
            self.finality_ms.push(seen.elapsed().as_millis() as u64);
        }
        self.tasks.remove(&task_id);
        self.claims.remove(&task_id);
        let next = height + 1;
        let stale: Vec<[u8; 32]> = self
            .tasks
            .iter()
            .filter(|(_, task)| task.sequence < next || task.deadline_height < next)
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.tasks.remove(&id);
            self.claims.remove(&id);
            self.task_seen_at.remove(&id);
        }
        self.round = 0;
        self.round_started = None;
        self.proposed_round = None;
        self.voted_rounds.clear();
        self.candidates.clear();
        self.proposals = ProposalGate::default();
        self.votes = VoteBook::default();
        self.lock = self.chain.load_vote_lock(next).unwrap_or(None);
        let rewards: Vec<String> = executed
            .block
            .task
            .partitions
            .iter()
            .map(|p| {
                let balance = self
                    .chain
                    .ledger
                    .accounts
                    .get(&p.worker)
                    .map(|a| a.balance)
                    .unwrap_or(0);
                format!("{}={balance}", short(&p.worker))
            })
            .collect();
        self.note(format!(
            "FINALIZED height {height} block {} via {via} (round {}, {} signers); issuance {}, fee budget {}; worker balances {}; state root {}",
            short(&executed.id),
            certificate.round,
            certificate.votes.len(),
            executed.block.issuance,
            executed.block.task.fee_budget,
            rewards.join(" "),
            short(&executed.block.header.state_root)
        ));
        self.gossip(None, &self.status_message());
        // Proposals and votes that arrived one height early are re-validated.
        for (source, message) in std::mem::take(&mut self.early_messages) {
            self.on_message(source, message);
        }
    }

    fn online_workers(&self) -> Vec<[u8; 32]> {
        let mut workers = vec![self.own_worker];
        for key in self.peers.keys() {
            if let Some(worker) = self
                .config
                .validator(key)
                .and_then(|v| self.config.worker_of(&v.name))
            {
                workers.push(worker);
            }
        }
        workers.sort();
        workers.dedup();
        workers
    }

    fn on_control(&mut self, request: ControlRequest) -> Value {
        match request {
            ControlRequest::Status => self.status_json(),
            ControlRequest::SubmitTask { task } => self.on_task(None, &task).to_json(),
            ControlRequest::Inject { message } => {
                let kind = message.kind();
                let verdict = self.on_message(None, message);
                let mut answer = verdict.to_json();
                answer["kind"] = json!(kind);
                answer
            }
            ControlRequest::GetBlock { height } => match self.chain.record_at(height) {
                Ok(Some(record)) => json!({
                    "height": height,
                    "block_id": hex(&record.block_id),
                    "block": hex(&record.bytes),
                    "certificate": hex(&record.certificate),
                }),
                Ok(None) => json!({ "error": "no finalized block at that height" }),
                Err(error) => json!({ "error": error }),
            },
        }
    }

    fn status_json(&self) -> Value {
        let accounts: serde_json::Map<String, Value> = self
            .chain
            .ledger
            .accounts
            .iter()
            .map(|(address, account)| {
                (
                    hex(address),
                    json!({ "balance": account.balance, "nonce": account.nonce }),
                )
            })
            .collect();
        let peers: Vec<Value> = self
            .peers
            .values()
            .map(|link| {
                json!({
                    "name": self.config.validator(&link.key).map(|v| v.name.clone()),
                    "validator": hex(&link.key),
                    "outbound": link.outbound,
                    "remote": link.remote,
                    "height": self.peer_heights.get(&link.key),
                })
            })
            .collect();
        let pending: Vec<Value> = self
            .tasks
            .values()
            .map(|task| {
                json!({
                    "task_id": hex(&task.task_id),
                    "sequence": task.sequence,
                    "partitions": task.partitions.len(),
                    "claims": self.claims.get(&task.task_id).map(|c| c.len()).unwrap_or(0),
                })
            })
            .collect();
        json!({
            "node": self.name,
            "network": self.config.network_name,
            "validator": hex(&self.own_validator),
            "worker": hex(&self.own_worker),
            "settlement_engine": self.engine.name(),
            "worker_backend": self.worker.name(),
            "holds_task_authority_key": self.keys.authority.is_some(),
            "height": self.chain.height,
            "tip": hex(&self.chain.tip),
            "state_root": self.chain.state_root(),
            "supply": ledger_supply(&self.chain.ledger) as u64,
            "fee_pool": self.chain.ledger.fee_pool,
            "accounts": accounts,
            "settled_batches": self.chain.ledger.settled_batches.len(),
            "paid_receipts": self.chain.ledger.paid_receipts.len(),
            "round": self.round,
            "next_leader": hex(&self.config.leader(self.next_height(), self.round)),
            "locked_block": self.lock.as_ref().map(|(id, _)| hex(id)),
            "pending_tasks": pending,
            "peers": peers,
            "online_workers": self.online_workers().iter().map(|w| hex(w.as_slice())).collect::<Vec<_>>(),
            "validator_count": self.config.validators.len(),
            "quorum_power": self.config.validator_set.required_power(),
            "validator_set_commitment": hex(&self.config.validator_set.commitment()),
            "counters": self.counters,
            "blocks": self.chain.history,
            "startup": self.startup,
            "finality_ms": self.finality_ms,
            "uptime_ms": self.started.elapsed().as_millis() as u64,
            "recent_events": self.log,
        })
    }
}

#[cfg(test)]
mod event_queue_tests {
    use super::*;

    #[test]
    fn worker_slots_enforce_concurrency_cap_and_release_capacity() {
        let mut slots = WorkerExecutionSlots::default();
        for id in 0..MAX_CONCURRENT_WORKER_EXECUTIONS {
            assert!(slots.reserve([id as u8; 32], 0));
        }
        assert_eq!(slots.active.len(), MAX_CONCURRENT_WORKER_EXECUTIONS);
        // A different task cannot launch more threads while the pool is full.
        assert!(!slots.reserve([100; 32], 0));
        // A task that finishes, fails, or becomes stale always frees its slot.
        slots.release([0; 32], 0);
        assert!(slots.reserve([100; 32], 0));
        assert!(!slots.reserve([100; 32], 0));
        slots.release([100; 32], 0);
        for id in 1..MAX_CONCURRENT_WORKER_EXECUTIONS {
            slots.release([id as u8; 32], 0);
        }
        assert!(slots.active.is_empty());
        assert!(slots.reserve([101; 32], 0));
    }

    #[test]
    fn shared_node_event_queue_rejects_overflow_and_reclaims_capacity() {
        let (sender, receiver) = node_event_channel();
        let event = || Event::HandshakeRejected {
            remote: "192.0.2.1:12345".into(),
            reason: "rejected".into(),
        };

        for _ in 0..MAX_PENDING_NODE_EVENTS {
            assert!(sender.try_send(event()).is_ok());
        }
        assert!(matches!(
            sender.try_send(event()),
            Err(mpsc::TrySendError::Full(_))
        ));

        assert!(matches!(
            receiver.try_recv(),
            Ok(Event::HandshakeRejected { .. })
        ));
        assert!(sender.try_send(event()).is_ok());
        drop(receiver);
        assert!(matches!(
            sender.try_send(event()),
            Err(mpsc::TrySendError::Disconnected(_))
        ));
    }
}
