//! End-to-end V2 independently verifiable integer inference reward fixture.
//! Python produces the task work; Rust verifies every stored witness without
//! Python or the submitting worker. Haskell settles from existing fees.
//! Full blockchain replay re-verifies V2 before recomputing any reward.

use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signer, SigningKey};

use crate::ai_pipeline::verify_demo_partitions_for;
use crate::ai_proof_v2::{
    commit_proof_block_v2, expected_result_commitment_v1, input_commitment_v1,
    model_commitment_v1, rules_commitment_v1, task_id_v2, task_reward_batch_id_v2,
    task_signing_bytes_v2, worker_signing_bytes_v2, InferenceTaskV2,
    ProofBlockInput, RewardProofV2, SignedInferenceClaimV2,
};
use crate::genesis_checkpoint::{
    genesis_checkpoint_signing_bytes_v1, initialize_signed_genesis,
    replay_chain_from_signed_checkpoint, SignedGenesisCheckpointV1,
};
use crate::ipc::haskell::HaskellValidatorClient;
use crate::mining::receipts::VerifiedWorkReceipt;
use crate::mining::scheduler::WorkRange;
use crate::storage::{Column, KeyValueStore, LedgerState, LocalAccount, RocksDbStore};
use crate::transaction_commitment::{
    canonical_header_v1, ledger_state_root_v1, transaction_root_v1, CommittedHeaderV1,
};

fn fixture_proof(authority: &SigningKey) -> Result<RewardProofV2, String> {
    let weights = vec![2, 3];
    let rows = vec![vec![1, 2], vec![3, 4], vec![-2, 5]];
    let mut task = InferenceTaskV2 {
        puzzle_id: [0; 32],
        model_commitment: model_commitment_v1(&weights)?,
        input_commitment: input_commitment_v1(&rows, weights.len())?,
        rules_commitment: rules_commitment_v1(),
        deadline_height: 100,
        authorized_budget: 3,
        weights, rows,
        authority_signature: [0; 64],
    };
    task.puzzle_id = task_id_v2(&task);
    task.authority_signature = authority.sign(&task_signing_bytes_v2(&task)).to_bytes();

    // The same two local worker identities used by the Python prototype.
    let mut claims = Vec::new();
    for (seed, start, count) in [(5u8, 0, 2), (6u8, 2, 1)] {
        let worker = SigningKey::from_bytes(&[seed; 32]);
        let range = WorkRange { start, count };
        let receipt = VerifiedWorkReceipt {
            puzzle_id: task.puzzle_id,
            worker_id: worker.verifying_key().to_bytes(),
            range,
            result_commitment: expected_result_commitment_v1(&task, range)?,
        };
        claims.push(SignedInferenceClaimV2 {
            signature: worker.sign(&worker_signing_bytes_v2(&receipt)).to_bytes(),
            receipt,
        });
    }
    Ok(RewardProofV2 {
        batch_id: task_reward_batch_id_v2(&task.puzzle_id),
        task,
        claims,
    })
}

fn block_input<'a>(
    proof: &'a RewardProofV2,
    prior: &'a LedgerState,
    parent: &'a [u8; 32],
) -> ProofBlockInput<'a> {
    ProofBlockInput {
        expected_prior: prior,
        parent_id: parent,
        height: 1,
        timestamp_ms: 1001,
        proof,
    }
}

pub fn demo() -> Result<(), String> {
    let executable = env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE")
        .filter(|v| !v.is_empty())
        .ok_or("AIVOBLOCK_HASKELL_EXECUTABLE is required for V2 AI proof demo")?;
    let core = HaskellValidatorClient::new(PathBuf::from(executable))
        .map_err(|e| format!("Haskell executable: {e:?}"))?;
    let authority = SigningKey::from_bytes(&[17; 32]);
    let trusted_key = authority.verifying_key().to_bytes();
    let proof = fixture_proof(&authority)?;

    // This actually executes/recomputes the same committed task through
    // the Python worker and independently compares the exact receipts.
    let independently_executed = verify_demo_partitions_for(proof.task.puzzle_id)?;
    for claim in &proof.claims {
        if !independently_executed.contains_verified(&claim.receipt) {
            return Err("Python-produced result disagrees with independent Rust verifier".into());
        }
    }

    let mut initial = LedgerState {
        fee_pool: 3,
        ..LedgerState::default()
    };
    initial.accounts.insert([9; 32], LocalAccount { balance: 97, nonce: 0 });
    let genesis_header = CommittedHeaderV1 {
        version: 1, chain_id: 1, height: 0, parent_hash: [0; 32],
        transactions_root: transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?,
        puzzle_proofs_root: [0; 32],
        state_root: ledger_state_root_v1(&initial).map_err(|e| format!("{e:?}"))?,
        timestamp_ms: 1000,
    };
    let genesis_bytes = canonical_header_v1(&genesis_header).map_err(|e| format!("{e:?}"))?;
    let signable = genesis_checkpoint_signing_bytes_v1(&genesis_bytes, &initial)
        .map_err(|e| format!("{e:?}"))?;
    let checkpoint = SignedGenesisCheckpointV1 {
        genesis_header: genesis_bytes,
        initial_ledger: initial.clone(),
        signature: authority.sign(&signable).to_bytes(),
    };

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|_| "Clock before epoch")?.as_nanos();
    let path = env::temp_dir().join(format!(
        "aivoblock-proof-v2-{}-{timestamp}", std::process::id(),
    ));
    let block_id;
    let expected;
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        let genesis_id = initialize_signed_genesis(&db, &checkpoint, &trusted_key)?;
        let mut forged = proof.clone();
        forged.claims[0].receipt.result_commitment[0] ^= 1;
        if commit_proof_block_v2(&db, &core, &trusted_key, block_input(&forged, &initial, &genesis_id)).is_ok() {
            return Err("Forged AI result paid a worker".into());
        }
        forged = proof.clone();
        forged.task.weights[0] += 1;
        if commit_proof_block_v2(&db, &core, &trusted_key, input(&forged)).is_ok() {
            return Err("Changed model weights passed proof verification".into());
        }
        if commit_proof_block_v2(&db, &core,
            &SigningKey::from_bytes(&[18; 32]).verifying_key().to_bytes(),
            block_input(&proof, &initial, &genesis_id)).is_ok()
        {
            return Err("Untrusted work authority authorized a payout".into());
        }
        if db.read_local_ledger_state().map_err(|e| e.to_string())? != Some(initial.clone()) {
            return Err("Rejected AI proof mutated genesis balances".into());
        }

        block_id = commit_proof_block_v2(&db, &core, &trusted_key, input(&proof))?;
        expected = db.read_local_ledger_state().map_err(|e| e.to_string())?
            .ok_or("Missing V2 ledger")?;
        if expected.fee_pool != 0
            || expected.settled_batches.len() != 1
            || !expected.settled_batches.contains(&proof.batch_id)
            || expected.paid_receipts.len() != 2
            || expected.accounts.get(&proof.claims[0].receipt.worker_id)
                .map(|a| a.balance) != Some(2)
            || expected.accounts.get(&proof.claims[1].receipt.worker_id)
                .map(|a| a.balance) != Some(1)
        {
            return Err("Independent AI proof payouts or replay markers wrong".into());
        }
        // Even changing the worker and partition of this SAME signed task
        // cannot pay it again, because the batch ID is derived from the task.
        let mut differently_partitioned = proof.clone();
        differently_partitioned.claims.swap(0, 1);
        if commit_proof_block_v2(&db, &core, &trusted_key, ProofBlockInput {
            expected_prior: &expected, parent_id: &block_id, height: 2,
            timestamp_ms: 1002, proof: &differently_partitioned,
        }).is_ok() {
            return Err("Replayed or overlapping V2 task rewarded again".into());
        }
    }
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        let audited = replay_chain_from_signed_checkpoint(
            &db, &core, &checkpoint, &trusted_key,
        )?;
        if audited.ledger != expected || audited.tip_id != block_id
            || audited.reward_blocks != 1 || audited.settled_receipts != 2
        {
            return Err("Independent V2 history recomputation differs from state".into());
        }
        let raw = db.get(Column::Blocks, &block_id).map_err(|e| e.to_string())?
            .ok_or("Missing persisted V2 proof")?;
        let mut corrupted = raw.clone();
        *corrupted.last_mut().ok_or("Empty V2 proof block")? ^= 1;
        db.put(Column::Blocks, &block_id, &corrupted).map_err(|e| e.to_string())?;
        if replay_chain_from_signed_checkpoint(&db, &core, &checkpoint, &trusted_key).is_ok() {
            return Err("Tampered stored V2 proof passed independent replay".into());
        }
        db.put(Column::Blocks, &block_id, &raw).map_err(|e| e.to_string())?;
        if replay_chain_from_signed_checkpoint(&db, &core, &checkpoint, &trusted_key)?.ledger
            != expected
        {
            return Err("Restored V2 proof history failed independent replay".into());
        }
    }
    std::fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
    println!("AI proof V2: independently recomputed complete signed input/model: PASS");
    println!("Worker-signed claims, task issuer and canonical commitments: PASS");
    println!("Haskell funded 2 + 1 reward, RocksDB atomic restart/replay: PASS");
    println!("Forgery, changed model, untrusted issuer and corrupted storage: REJECTED");
    println!("Scope: tiny deterministic integer inference only; no succinct proofs, P2P or consensus");
    Ok(())
}
