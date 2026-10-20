//! Rust -> Haskell ordered signed batch -> atomic RocksDB block demo.
//! The Haskell core computes the complete state before ANY block writes.
//! This is a local privileged IPC fixture; no validator consensus.

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::batch_commitment::{
    commit_verified_batch_block, verify_stored_batch_block, MAX_SIGNED_BLOCK_TRANSFERS,
};
use crate::ipc::haskell::HaskellValidatorClient;
use crate::storage::ledger_state::{decode_ledger_state, encode_ledger_state, LedgerState};
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

/// Strict enough for a trusted local test executable only; replace with a
/// typed bounded JSON parser before exposing this IPC response to peers.
fn response_hex(response: &str, field: &str) -> Result<Vec<u8>, String> {
    let prefix = format!(r#""{field}":""#);
    if response.matches(&prefix).count() != 1 {
        return Err(format!("Missing/duplicate {field} in Haskell response"));
    }
    let tail = response
        .split_once(&prefix)
        .ok_or("Missing response field")?
        .1;
    let value = tail.split_once('"').ok_or("Unterminated hex response")?.0;
    if value.len() > 2 * 1024 * 1024
        || value.len() % 2 != 0
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(format!("Invalid {field} hex encoding"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let a = (pair[0] as char).to_digit(16).ok_or("Invalid hex")?;
            let b = (pair[1] as char).to_digit(16).ok_or("Invalid hex")?;
            Ok(((a << 4) | b) as u8)
        })
        .collect::<Result<Vec<_>, &str>>()
        .map_err(str::to_owned)
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
    let answer = std::str::from_utf8(&response).map_err(|_| "Non-UTF8 Haskell batch response")?;
    if !answer.contains(r#""ok":true"#)
        || !answer.contains(r#""type":"ledger_signed_batch_fixture""#)
        || !answer.contains(r#""protocol_version":1"#)
        || !answer.contains(r#""scope":"ed25519_verified_ordered_batch_only""#)
        || !answer.contains(r#""accepted":true"#)
    {
        return Err(format!("Haskell rejected signed batch: {answer}"));
    }
    let state_bytes = response_hex(answer, "state_hex")?;
    let successor =
        decode_ledger_state(&state_bytes).map_err(|e| format!("Invalid returned state: {e:?}"))?;
    let expected_ids = format!(
        r#""transaction_ids":[{}]"#,
        ids.iter()
            .map(|id| format!(r#""{}""#, hex(id)))
            .collect::<Vec<_>>()
            .join(",")
    );
    if answer.matches(&expected_ids).count() != 1
        || response_hex(answer, "transactions_root")? != root
        || response_hex(answer, "state_root")?
            != ledger_state_root_v1(&successor)
                .map_err(|e| format!("Invalid successor root: {e:?}"))?
    {
        return Err("Haskell and Rust batch commitments disagree".into());
    }
    Ok(successor)
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
    let genesis_id: [u8; 32] = Sha256::digest(genesis_bytes).into();
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        db.append_atomic_ledger_block(&genesis_id, &[0; 32], 0, &genesis_bytes, &genesis, None)
            .map_err(|e| format!("Genesis commit: {e}"))?;
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
        if db
            .get(Column::ChainState, b"tip")
            .map_err(|e| e.to_string())?
            != Some(block_id.to_vec())
        {
            return Err("Wrong reopened block tip".into());
        }
    }
    std::fs::remove_dir_all(&path).map_err(|e| format!("Remove batch test DB: {e}"))?;
    println!("Two Haskell-verified transfers committed in ONE Rust block: PASS");
    println!("Alice 82; Bob 15; fee pool 3; nonce 2; supply 100");
    println!("Duplicate, reordered, overspend and tampered batch rejection: PASS");
    println!("Atomic RocksDB reopen and signed block body verification: PASS");
    println!("Scope: synthetic genesis; no validator consensus or real payments");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_invalid_or_ambiguous_local_response_hex() {
        assert!(response_hex(r#"{"state_hex":"00"}"#, "state_hex").is_ok());
        assert!(response_hex(r#"{"state_hex":"0z"}"#, "state_hex").is_err());
        assert!(response_hex(r#"{"state_hex":"00","state_hex":"00"}"#, "state_hex").is_err());
        assert!(response_hex(r#"{"state_hex":"A0"}"#, "state_hex").is_err());
    }
}
