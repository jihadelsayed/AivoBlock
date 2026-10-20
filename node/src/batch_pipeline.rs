//! Rust -> Haskell ordered signed batch -> atomic RocksDB block demo.
//! The Haskell core computes the complete state before ANY block writes.
//! This is a local privileged IPC fixture; no validator consensus.

use ed25519_dalek::{Signer, SigningKey};
use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::batch_commitment::{
    commit_verified_batch_block, verify_stored_batch_block, MAX_SIGNED_BLOCK_TRANSFERS,
};
use crate::batch_replay::replay_stored_tip;
use crate::chain_replay::replay_local_chain;
use crate::genesis_checkpoint::{
    genesis_checkpoint_signing_bytes_v1, initialize_signed_genesis,
    replay_chain_from_signed_checkpoint, SignedGenesisCheckpointV1,
};
use crate::ipc::haskell::HaskellValidatorClient;
use crate::storage::ledger_state::{encode_ledger_state, LedgerState};
use crate::storage::{Column, KeyValueStore, LocalAccount, RocksDbStore};
use crate::transaction_auth::{
    transfer_signing_bytes_v1, verify_signed_transfer_v1, SignedTransferV1,
};
use crate::transaction_commitment::{
    canonical_header_v1, ledger_state_root_v1, signed_transaction_id_v1, transaction_root_v1,
    CommittedHeaderV1,
};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn signed(
    signer: &SigningKey,
    recipient: [u8; 32],
    amount: u64,
    fee: u64,
    nonce: u64,
) -> SignedTransferV1 {
    let sender = signer.verifying_key().to_bytes();
    let mut tx = SignedTransferV1 {
        chain_id: 1,
        version: 1,
        sender,
        recipient,
        amount,
        fee,
        nonce,
        public_key: sender,
        signature: [0; 64],
    };
    tx.signature = signer
        .sign(&transfer_signing_bytes_v1(&tx).unwrap())
        .to_bytes();
    tx
}

fn tx_json(tx: &SignedTransferV1) -> String {
    format!(
        r#"{{"sender":"{}","recipient":"{}","amount":{},"fee":{},"nonce":{},"public_key":"{}","signature":"{}"}}"#,
        hex(&tx.sender),
        hex(&tx.recipient),
        tx.amount,
        tx.fee,
        tx.nonce,
        hex(&tx.public_key),
        hex(&tx.signature)
    )
}

/// The complete batch is verified by Rust and Haskell; state is returned
/// only after Haskell's pure sequential fold succeeds. No state is written.
pub fn execute_signed_batch_haskell(
    core: &HaskellValidatorClient,
    previous: &LedgerState,
    transactions: &[SignedTransferV1],
) -> Result<LedgerState, String> {
    if transactions.is_empty() || transactions.len() > MAX_SIGNED_BLOCK_TRANSFERS {
        return Err("Signed batch count must be 1 to 64".into());
    }
    let mut ids = Vec::with_capacity(transactions.len());
    for tx in transactions {
        verify_signed_transfer_v1(tx).map_err(|e| format!("Invalid signed transaction: {e:?}"))?;
        ids.push(signed_transaction_id_v1(tx).map_err(|e| format!("Transaction ID: {e:?}"))?);
    }
    let root = transaction_root_v1(&ids)
        .map_err(|e| format!("Duplicate or invalid transaction root: {e:?}"))?;
    let prior =
        encode_ledger_state(previous).map_err(|e| format!("Encode previous ledger: {e:?}"))?;
    let request = format!(
        r#"{{"protocol_version":1,"type":"ledger_signed_batch_fixture","version":1,"chain_id":1,"state_hex":"{}","transactions":[{}]}}"#,
        hex(&prior),
        transactions
            .iter()
            .map(tx_json)
            .collect::<Vec<_>>()
            .join(",")
    );
    let response = core
        .exchange(request.as_bytes())
        .map_err(|e| format!("Haskell batch IPC: {e:?}"))?;
    crate::ipc::ledger_responses::decode_signed_batch_reply(&response, &ids, &root)
}

/// Two transfers are executed inside a SINGLE candidate block, with no
/// intermediate RocksDB commit. Rejected batches must leave tip unchanged.
pub fn demo() -> Result<(), String> {
    let executable = env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .ok_or("AIVOBLOCK_HASKELL_EXECUTABLE is required")?;
    let core = HaskellValidatorClient::new(PathBuf::from(executable))
        .map_err(|e| format!("Haskell binary: {e:?}"))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Bad system clock")?
        .as_nanos();
    let path = env::temp_dir().join(format!(
        "aivoblock-multi-transfer-{}-{stamp}",
        std::process::id(),
    ));
    let signer = SigningKey::from_bytes(&[7u8; 32]);
    let alice = signer.verifying_key().to_bytes();
    let bob = [2u8; 32];
    let mut genesis = LedgerState::default();
    genesis.accounts.insert(
        alice,
        LocalAccount {
            balance: 100,
            nonce: 0,
        },
    );
    let genesis_header = CommittedHeaderV1 {
        version: 1,
        chain_id: 1,
        height: 0,
        parent_hash: [0; 32],
        transactions_root: transaction_root_v1(&[]).map_err(|e| format!("{e:?}"))?,
        puzzle_proofs_root: [0; 32],
        state_root: ledger_state_root_v1(&genesis).map_err(|e| format!("{e:?}"))?,
        timestamp_ms: 1000,
    };
    let genesis_bytes = canonical_header_v1(&genesis_header).map_err(|e| format!("{e:?}"))?;
    // TEST-ONLY authority seed. Real nodes must obtain the trusted
    // verification key independently from checkpoint bytes or storage.
    let authority = SigningKey::from_bytes(&[17u8; 32]);
    let trusted_authority = authority.verifying_key().to_bytes();
    let checkpoint_payload = genesis_checkpoint_signing_bytes_v1(&genesis_bytes, &genesis)
        .map_err(|e| format!("Genesis checkpoint bytes: {e:?}"))?;
    let signed_checkpoint = SignedGenesisCheckpointV1 {
        genesis_header: genesis_bytes,
        initial_ledger: genesis.clone(),
        signature: authority.sign(&checkpoint_payload).to_bytes(),
    };
    let genesis_id;
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        let attacker = SigningKey::from_bytes(&[18u8; 32]);
        if initialize_signed_genesis(
            &db,
            &signed_checkpoint,
            &attacker.verifying_key().to_bytes(),
        )
        .is_ok()
        {
            return Err("Unauthorized genesis key initialized a ledger".into());
        }
        if db
            .get(Column::ChainState, b"tip")
            .map_err(|e| e.to_string())?
            .is_some()
        {
            return Err("Rejected checkpoint wrote a genesis tip".into());
        }
        genesis_id = initialize_signed_genesis(&db, &signed_checkpoint, &trusted_authority)?;
    }
    let batch = [
        signed(&signer, bob, 10, 2, 0),
        signed(&signer, bob, 5, 1, 1),
    ];
    let block_id;
    let block_header;
    let computed;
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        let prior = db
            .read_local_ledger_state()
            .map_err(|e| e.to_string())?
            .ok_or("Missing genesis state")?;
        let cases = [
            vec![batch[0], batch[0]],
            vec![batch[1], batch[0]],
            vec![batch[0], signed(&signer, bob, 500, 1, 1)],
            vec![
                batch[0],
                SignedTransferV1 {
                    amount: 6,
                    ..batch[1]
                },
            ],
        ];
        for rejected in cases {
            if execute_signed_batch_haskell(&core, &prior, &rejected).is_ok() {
                return Err("Invalid ordered batch was accepted".into());
            }
            if db.read_local_ledger_state().map_err(|e| e.to_string())? != Some(prior.clone())
                || db
                    .get(Column::ChainState, b"tip")
                    .map_err(|e| e.to_string())?
                    != Some(genesis_id.to_vec())
            {
                return Err("Rejected batch mutated ledger or tip".into());
            }
        }
        computed = execute_signed_batch_haskell(&core, &prior, &batch)?;
        if computed.fee_pool != 3
            || computed.accounts.get(&alice).map(|a| (a.balance, a.nonce)) != Some((82, 2))
            || computed.accounts.get(&bob).map(|a| a.balance) != Some(15)
        {
            return Err("Haskell batch balances did not match expected totals".into());
        }
        let ids = batch
            .iter()
            .map(signed_transaction_id_v1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("{e:?}"))?;
        block_header = CommittedHeaderV1 {
            version: 1,
            chain_id: 1,
            height: 1,
            parent_hash: genesis_id,
            transactions_root: transaction_root_v1(&ids).map_err(|e| format!("{e:?}"))?,
            puzzle_proofs_root: [0; 32],
            state_root: ledger_state_root_v1(&computed).map_err(|e| format!("{e:?}"))?,
            timestamp_ms: 1001,
        };
        let mut tampered_header = block_header;
        tampered_header.transactions_root = [4; 32];
        if commit_verified_batch_block(&db, &tampered_header, &batch, &prior, &computed).is_ok() {
            return Err("Wrong transaction Merkle root was accepted".into());
        }
        tampered_header = block_header;
        tampered_header.state_root = [5; 32];
        if commit_verified_batch_block(&db, &tampered_header, &batch, &prior, &computed).is_ok() {
            return Err("Wrong state root was accepted".into());
        }
        if db.read_local_ledger_state().map_err(|e| e.to_string())? != Some(prior.clone()) {
            return Err("Invalid block commitment changed prior ledger".into());
        }
        block_id = commit_verified_batch_block(&db, &block_header, &batch, &prior, &computed)
            .map_err(|e| format!("Atomic batch commit: {e:?}"))?;
    }
    let final_expected;
    let final_id;
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        if db.read_local_ledger_state().map_err(|e| e.to_string())? != Some(computed.clone()) {
            return Err("Block batch ledger did not survive restart".into());
        }
        let block = db
            .get(Column::Blocks, &block_id)
            .map_err(|e| e.to_string())?
            .ok_or("Missing stored block")?;
        verify_stored_batch_block(&block, &block_id, &block_header, &batch, &computed)
            .map_err(|e| format!("Stored block invalid: {e:?}"))?;
        // Unlike the older test above, this audit starts from the BLOCK
        // BYTES ON DISK, not the transaction list originally passed to
        // the encoder. It then independently reruns Haskell's ledger rules.
        let reconstructed = replay_stored_tip(&db, &core, &genesis)?;
        if reconstructed != computed {
            return Err("Independent stored-block replay gave different balances".into());
        }
        let mut wrong_prior = genesis.clone();
        wrong_prior.fee_pool = 1;
        if replay_stored_tip(&db, &core, &wrong_prior).is_ok() {
            return Err("Accepted predecessor state not committed by parent".into());
        }

        // Deliberately corrupt the on-disk signed body; read-only replay
        // MUST refuse to trust it. Restore the fixture after the test.
        let mut corrupted = block.clone();
        *corrupted.last_mut().ok_or("Empty stored body")? ^= 1;
        db.put(Column::Blocks, &block_id, &corrupted)
            .map_err(|e| format!("Corrupt test block: {e}"))?;
        if replay_stored_tip(&db, &core, &genesis).is_ok() {
            return Err("Corrupted signed block was accepted by replay".into());
        }
        db.put(Column::Blocks, &block_id, &block)
            .map_err(|e| format!("Restore test block: {e}"))?;
        if db.read_local_ledger_state().map_err(|e| e.to_string())? != Some(computed.clone()) {
            return Err("Failed replay unexpectedly mutated the ledger".into());
        }

        if db
            .get(Column::ChainState, b"tip")
            .map_err(|e| e.to_string())?
            != Some(block_id.to_vec())
        {
            return Err("Wrong reopened block tip".into());
        }

        // A second *distinct* signed batch proves history replay uses the
        // state calculated for each predecessor, not a supplied tip ledger.
        let subsequent = [signed(&signer, bob, 4, 1, 2), signed(&signer, bob, 3, 1, 3)];
        final_expected = execute_signed_batch_haskell(&core, &computed, &subsequent)?;
        if final_expected.fee_pool != 5
            || final_expected
                .accounts
                .get(&alice)
                .map(|account| (account.balance, account.nonce))
                != Some((73, 4))
            || final_expected
                .accounts
                .get(&bob)
                .map(|account| account.balance)
                != Some(22)
        {
            return Err("Second signed block Haskell balances mismatch".into());
        }
        let ids = subsequent
            .iter()
            .map(signed_transaction_id_v1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{error:?}"))?;
        let next_header = CommittedHeaderV1 {
            version: 1,
            chain_id: 1,
            height: 2,
            parent_hash: block_id,
            transactions_root: transaction_root_v1(&ids)
                .map_err(|error| format!("Followup transaction root: {error:?}"))?,
            puzzle_proofs_root: [0; 32],
            state_root: ledger_state_root_v1(&final_expected)
                .map_err(|error| format!("Followup state root: {error:?}"))?,
            timestamp_ms: 1002,
        };
        final_id =
            commit_verified_batch_block(&db, &next_header, &subsequent, &computed, &final_expected)
                .map_err(|error| format!("Second batch atomic commit: {error:?}"))?;

        let report = replay_chain_from_signed_checkpoint(
            &db,
            &core,
            &signed_checkpoint,
            &trusted_authority,
        )?;
        if report.ledger != final_expected
            || report.signed_blocks != 2
            || report.signed_transfers != 4
            || report.tip_id != final_id
        {
            return Err("Full trusted-genesis replay produced wrong chain state".into());
        }
        if replay_chain_from_signed_checkpoint(
            &db,
            &core,
            &signed_checkpoint,
            &SigningKey::from_bytes(&[18; 32]).verifying_key().to_bytes(),
        )
        .is_ok()
        {
            return Err("Full replay accepted untrusted genesis signer".into());
        }
        let mut changed_checkpoint = signed_checkpoint.clone();
        changed_checkpoint.initial_ledger.fee_pool += 1;
        if replay_chain_from_signed_checkpoint(&db, &core, &changed_checkpoint, &trusted_authority)
            .is_ok()
        {
            return Err("Full replay accepted altered signed allocation".into());
        }
        if replay_local_chain(&db, &core, &[9; 32], &genesis).is_ok() {
            return Err("Wrong trusted genesis ID was accepted".into());
        }
        let mut wrong_genesis = genesis.clone();
        wrong_genesis.fee_pool = 1;
        if replay_local_chain(&db, &core, &genesis_id, &wrong_genesis).is_ok() {
            return Err("Wrong trusted genesis state was accepted".into());
        }

        // Alter a *non-tip* signed ancestor and verify full replay refuses
        // to trust a valid-looking later tip whose history was corrupted.
        let mut damaged = block.clone();
        *damaged.last_mut().ok_or("Empty ancestor body")? ^= 1;
        db.put(Column::Blocks, &block_id, &damaged)
            .map_err(|error| format!("Corrupt ancestor fixture: {error}"))?;
        if replay_chain_from_signed_checkpoint(&db, &core, &signed_checkpoint, &trusted_authority)
            .is_ok()
        {
            return Err("Full replay accepted tampered historical block".into());
        }
        db.put(Column::Blocks, &block_id, &block)
            .map_err(|error| format!("Restore ancestor fixture: {error}"))?;

        db.put(Column::ChainState, b"height", &3u64.to_be_bytes())
            .map_err(|error| format!("Tamper height fixture: {error}"))?;
        if replay_chain_from_signed_checkpoint(&db, &core, &signed_checkpoint, &trusted_authority)
            .is_ok()
        {
            return Err("Full replay accepted invalid tip height".into());
        }
        db.put(Column::ChainState, b"height", &2u64.to_be_bytes())
            .map_err(|error| format!("Restore height fixture: {error}"))?;
        if replay_chain_from_signed_checkpoint(&db, &core, &signed_checkpoint, &trusted_authority)?
            .ledger
            != final_expected
        {
            return Err("Restored full chain failed Haskell replay".into());
        }
    }
    // Reopen once more and audit the whole chain using solely stored block
    // contents, a trusted genesis checkpoint and independent Haskell rules.
    {
        let db = RocksDbStore::open(&path).map_err(|error| error.to_string())?;
        let report = replay_chain_from_signed_checkpoint(
            &db,
            &core,
            &signed_checkpoint,
            &trusted_authority,
        )?;
        if report.tip_id != final_id
            || report.ledger != final_expected
            || report.signed_blocks != 2
            || report.signed_transfers != 4
        {
            return Err("Full replay across database reopen failed".into());
        }
    }
    std::fs::remove_dir_all(&path).map_err(|e| format!("Remove batch test DB: {e}"))?;
    println!("Two Haskell-verified transfers committed in ONE Rust block: PASS");
    println!("Alice 82; Bob 15; fee pool 3; nonce 2; supply 100");
    println!("Duplicate, reordered, overspend and tampered batch rejection: PASS");
    println!("Atomic RocksDB reopen and signed block body verification: PASS");
    println!("Decode-on-disk + Haskell replay + predecessor check: PASS");
    println!("Tampered stored signature and wrong prior ledger: REJECTED");
    println!("Full history: 2 signed blocks, 4 transfers replayed from signed genesis: PASS");
    println!("Forged genesis signer and modified allocations: REJECTED");
    println!("Tampered ancestor, wrong genesis and wrong height: REJECTED");
    println!("Scope: synthetic genesis; no validator consensus or real payments");
    Ok(())
}
