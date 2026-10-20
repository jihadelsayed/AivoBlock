//! Experimental Haskell-led transfer -> Rust atomic block storage bridge.
//!
//! Only a trusted, local Haskell binary computes successor snapshots.
//! Transfer signature authentication, block header hashes, state roots,
//! consensus and chain rollback are deliberately NOT implemented here.

use ed25519_dalek::{Signer, SigningKey};
use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::ipc::haskell::HaskellValidatorClient;
use crate::storage::ledger_state::{decode_ledger_state, encode_ledger_state};
use crate::storage::{Column, KeyValueStore, LedgerState, LocalAccount, RocksDbStore};
use crate::transaction_auth::{
    transfer_signing_bytes_v1, verify_signed_transfer_v1, SignedTransferV1,
};

/// Fixed demo signing key: do NOT use this seed for real accounts.
fn fixture_transfer(
    key: &SigningKey,
    recipient: [u8; 32],
    amount: u64,
    fee: u64,
    nonce: u64,
) -> SignedTransferV1 {
    let sender = key.verifying_key().to_bytes();
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
    tx.signature = key
        .sign(&transfer_signing_bytes_v1(&tx).expect("valid local fixture"))
        .to_bytes();
    tx
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex(raw: &str) -> Result<Vec<u8>, String> {
    if raw.len() % 2 != 0 || raw.len() > 2 * 1024 * 1024 {
        return Err("Noncanonical or oversized Haskell snapshot hex".to_owned());
    }
    raw.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let a = (pair[0] as char)
                .to_digit(16)
                .ok_or("Invalid snapshot hex")?;
            let b = (pair[1] as char)
                .to_digit(16)
                .ok_or("Invalid snapshot hex")?;
            Ok(((a << 4) | b) as u8)
        })
        .collect::<Result<Vec<_>, &str>>()
        .map_err(str::to_owned)
}

/// The response must be from a **trusted local executable**, not network
/// input. This small prototype adapter recognizes one unescaped hex string,
/// validates the response envelope, then strictly decodes typed binary state.
/// Replace with a versioned JSON/Protobuf decoder for untrusted deployments.
fn decode_transition_response(response: &[u8]) -> Result<LedgerState, String> {
    let text = std::str::from_utf8(response).map_err(|_| "Non-UTF8 Haskell ledger response")?;
    if !text.contains(r#""type":"ledger_signed_transfer_fixture""#)
        || !text.contains(r#""scope":"ed25519_verified_local_transfer_only""#)
        || !text.contains(r#""protocol_version":1"#)
        || !text.contains(r#""ok":true"#)
        || !text.contains(r#""accepted":true"#)
    {
        return Err(format!("Haskell rejected ledger transition: {text}"));
    }
    let field = r#""state_hex":""#;
    if text.matches(field).count() != 1 {
        return Err("Missing or ambiguous Haskell ledger state".to_owned());
    }
    let suffix = text
        .split_once(field)
        .ok_or("Missing Haskell state field")?
        .1;
    let hex_value = suffix
        .split_once('"')
        .ok_or("Unterminated Haskell state hex")?
        .0;
    if !hex_value
        .bytes()
        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("Noncanonical Haskell hex encoding".to_owned());
    }
    let bytes = decode_hex(hex_value)?;
    decode_ledger_state(&bytes).map_err(|error| format!("Malformed Haskell ledger: {error:?}"))
}

/// Authenticate the canonical Ed25519 transfer locally, then invoke Haskell
/// to independently authenticate it and compute the ledger transition.
/// The caller must load prior state from trusted RocksDB; this function does
/// not authenticate blocks, genesis allocations or validator consensus.
pub fn compute_transfer_via_haskell(
    core: &HaskellValidatorClient,
    prior: &LedgerState,
    transfer: SignedTransferV1,
) -> Result<LedgerState, String> {
    // Cryptographic verification is mandatory before sending any state
    // transition to Haskell. Haskell repeats it independently.
    verify_signed_transfer_v1(&transfer)
        .map_err(|error| format!("Unauthenticated transfer: {error:?}"))?;
    let bytes = encode_ledger_state(prior).map_err(|e| format!("Snapshot encode: {e:?}"))?;
    let request = format!(
        r#"{{"protocol_version":1,"type":"ledger_signed_transfer_fixture","version":{},"chain_id":{},"state_hex":"{}","transfer":{{"sender":"{}","recipient":"{}","amount":{},"fee":{},"nonce":{},"public_key":"{}","signature":"{}"}}}}"#,
        transfer.version,
        transfer.chain_id,
        hex(&bytes),
        hex(&transfer.sender),
        hex(&transfer.recipient),
        transfer.amount,
        transfer.fee,
        transfer.nonce,
        hex(&transfer.public_key),
        hex(&transfer.signature)
    );
    let response = core
        .exchange(request.as_bytes())
        .map_err(|error| format!("Haskell ledger IPC failure: {error:?}"))?;
    decode_transition_response(&response)
}

/// End-to-end fixture: Haskell computes each successor, Rust atomically
/// stores the typed result and reloads it from RocksDB after process reopen.
pub fn demo() -> Result<(), String> {
    let executable = env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .ok_or("AIVOBLOCK_HASKELL_EXECUTABLE is required for this diagnostic")?;
    let core = HaskellValidatorClient::new(PathBuf::from(executable))
        .map_err(|error| format!("Haskell executable: {error:?}"))?;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Invalid system clock")?
        .as_nanos();
    let path = env::temp_dir().join(format!(
        "aivoblock-haskell-ledger-{}-{stamp}",
        std::process::id()
    ));

    let fixture_key = SigningKey::from_bytes(&[7u8; 32]);
    let alice = fixture_key.verifying_key().to_bytes();
    let bob = [2u8; 32];
    let mut initial = LedgerState::default();
    initial.accounts.insert(
        alice,
        LocalAccount {
            balance: 100,
            nonce: 0,
        },
    );

    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        db.append_atomic_ledger_block(&[1; 32], &[0; 32], 0, b"fixture-genesis", &initial, None)
            .map_err(|e| format!("Genesis commit: {e}"))?;
    }
    let after_first;
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        let prior = db
            .read_local_ledger_state()
            .map_err(|e| e.to_string())?
            .ok_or("Missing durable genesis")?;
        after_first = compute_transfer_via_haskell(
            &core,
            &prior,
            fixture_transfer(&fixture_key, bob, 10, 2, 0),
        )?;
        if after_first.fee_pool != 2
            || after_first
                .accounts
                .get(&alice)
                .map(|x| (x.balance, x.nonce))
                != Some((88, 1))
            || after_first.accounts.get(&bob).map(|x| x.balance) != Some(10)
        {
            return Err("Haskell transfer #1 returned unexpected balances".to_owned());
        }
        db.append_atomic_ledger_block(
            &[3; 32],
            &[1; 32],
            1,
            b"fixture-transfer-1",
            &after_first,
            None,
        )
        .map_err(|e| format!("First atomic ledger commit: {e}"))?;

        // An invalid nonce or overspend must be rejected by Haskell; neither
        // request may change the already-persisted chain state.
        for transfer in [
            fixture_transfer(&fixture_key, bob, 1, 0, 0),
            fixture_transfer(&fixture_key, bob, 100, 0, 1),
        ] {
            if compute_transfer_via_haskell(&core, &after_first, transfer).is_ok() {
                return Err("Haskell accepted invalid transfer".to_owned());
            }
        }
        // A tampered message and a wrong-key sender never reach Haskell.
        let signed = fixture_transfer(&fixture_key, bob, 1, 0, 1);
        for forged in [
            SignedTransferV1 {
                amount: 2,
                ..signed
            },
            SignedTransferV1 {
                sender: [4; 32],
                ..signed
            },
            SignedTransferV1 {
                signature: [0; 64],
                ..signed
            },
            SignedTransferV1 {
                chain_id: 2,
                ..signed
            },
        ] {
            if compute_transfer_via_haskell(&core, &after_first, forged).is_ok() {
                return Err("Invalid signature, owner or chain reached Haskell".to_owned());
            }
        }
        if db.read_local_ledger_state().map_err(|e| e.to_string())? != Some(after_first.clone()) {
            return Err("Rejected Haskell transfer changed stored ledger".to_owned());
        }
    }
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        let prior = db
            .read_local_ledger_state()
            .map_err(|e| e.to_string())?
            .ok_or("Missing first transfer after reopen")?;
        if prior != after_first {
            return Err("Recovered ledger mismatch after reopen".to_owned());
        }
        let next = compute_transfer_via_haskell(
            &core,
            &prior,
            fixture_transfer(&fixture_key, bob, 5, 1, 1),
        )?;
        if next.fee_pool != 3
            || next.accounts.get(&alice).map(|x| (x.balance, x.nonce)) != Some((82, 2))
            || next.accounts.get(&bob).map(|x| x.balance) != Some(15)
        {
            return Err("Haskell transfer #2 returned unexpected balances".to_owned());
        }
        db.append_atomic_ledger_block(&[4; 32], &[3; 32], 2, b"fixture-transfer-2", &next, None)
            .map_err(|e| format!("Second atomic ledger commit: {e}"))?;
    }
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        let final_state = db
            .read_local_ledger_state()
            .map_err(|e| e.to_string())?
            .ok_or("Missing final ledger")?;
        if final_state.fee_pool != 3
            || final_state.accounts.get(&alice).map(|x| x.balance) != Some(82)
            || final_state.accounts.get(&bob).map(|x| x.balance) != Some(15)
            || db
                .get(Column::ChainState, b"tip")
                .map_err(|e| e.to_string())?
                != Some(vec![4; 32])
        {
            return Err("Atomic Haskell ledger state not recovered".to_owned());
        }
    }
    std::fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
    println!("Haskell-computed transfer -> atomic RocksDB persistence: PASS");
    println!("Two transfers: alice=82, bob=15, fee pool=3; total funds=100");
    println!("Rejected nonce/overspend: PASS; reopen and state recovery: PASS");
    println!("Ed25519 signature, owner and chain checks: PASS (Rust + Haskell)");
    println!("Scope: local signed fixture; no validator consensus or real payments");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_and_forged_ipc_response_envelopes() {
        for response in [
            br#"{"accepted":true,"state_hex":"00"}"#.as_slice(),
            br#"{"ok":true,"type":"ledger_signed_transfer_fixture","protocol_version":1,"scope":"ed25519_verified_local_transfer_only","accepted":false,"state_hex":"00"}"#,
            br#"{"ok":true,"type":"ledger_signed_transfer_fixture","protocol_version":1,"scope":"ed25519_verified_local_transfer_only","accepted":true,"state_hex":"zz"}"#,
        ] {
            assert!(decode_transition_response(response).is_err());
        }
    }
}
