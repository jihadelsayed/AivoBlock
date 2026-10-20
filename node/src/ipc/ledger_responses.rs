//! Typed, strict parser for the two Haskell signed-ledger IPC responses.
//!
//! Avoid substring checks: a JSON value inside "reason", an escaped field
//! name, or a duplicate object key must NEVER authorize a state transition.
//! Parsing is bounded by the IPC frame limit and accepted fields are exact.
//! The caller must still trust the local executable and independently check
//! Ed25519 signatures, the trusted prior state, and block consensus.

use serde::Deserialize;

use super::MAX_IPC_PAYLOAD_BYTES;
use crate::storage::ledger_state::{decode_ledger_state, LedgerState};
use crate::transaction_commitment::ledger_state_root_v1;

const MAX_LEDGER_HEX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedBatchReply {
    #[serde(rename = "type")]
    kind: String,
    ok: bool,
    protocol_version: u32,
    accepted: bool,
    reason: String,
    scope: String,
    state_hex: Option<String>,
    transaction_ids: Option<Vec<String>>,
    transactions_root: Option<String>,
    state_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedTransferReply {
    #[serde(rename = "type")]
    kind: String,
    ok: bool,
    protocol_version: u32,
    accepted: bool,
    reason: String,
    scope: String,
    state_hex: Option<String>,
    transaction_id: Option<String>,
    transactions_root: Option<String>,
    state_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RewardBlockReply {
    #[serde(rename = "type")]
    kind: String,
    ok: bool,
    protocol_version: u32,
    accepted: bool,
    reason: String,
    scope: String,
    state_hex: Option<String>,
    state_root: Option<String>,
    receipt_ids: Option<Vec<String>>,
}

fn check_envelope(
    kind: &str,
    scope: &str,
    expected: (&str, &str),
    ok: bool,
    version: u32,
    accepted: bool,
    reason: &str,
) -> Result<(), String> {
    if kind != expected.0 || scope != expected.1 || !ok || version != 1 {
        return Err("Invalid Haskell signed-ledger response envelope".into());
    }
    if !accepted {
        return Err(format!(
            "Haskell rejected signed ledger transition: {reason}"
        ));
    }
    if reason != "none" {
        return Err("Accepted Haskell result carries a nonempty error reason".into());
    }
    Ok(())
}

fn parse_json<'a, T: Deserialize<'a>>(response: &'a [u8]) -> Result<T, String> {
    if response.is_empty() || response.len() > MAX_IPC_PAYLOAD_BYTES {
        return Err("Invalid or oversized signed-ledger IPC reply".into());
    }
    serde_json::from_slice(response)
        .map_err(|e| format!("Malformed signed-ledger JSON response: {e}"))
}

/// Decode only even-length lowercase ASCII hex, never an escaped string,
/// whitespace or a mix of interpretations of the same encoded bytes.
fn decode_hex(raw: &str, max_chars: usize) -> Result<Vec<u8>, String> {
    if raw.is_empty()
        || raw.len() % 2 != 0
        || raw.len() > max_chars
        || !raw
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("Noncanonical or oversized ledger hex field".into());
    }
    raw.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let a = (pair[0] as char).to_digit(16).ok_or("Invalid hex")?;
            let b = (pair[1] as char).to_digit(16).ok_or("Invalid hex")?;
            Ok(((a << 4) | b) as u8)
        })
        .collect::<Result<Vec<_>, &str>>()
        .map_err(str::to_owned)
}

fn parse_hash(raw: &str) -> Result<[u8; 32], String> {
    let decoded = decode_hex(raw, 64)?;
    decoded
        .try_into()
        .map_err(|_| "Commitment must be 32 bytes".into())
}

fn parse_ledger_and_state_root(
    raw: Option<String>,
    state_root: Option<String>,
) -> Result<LedgerState, String> {
    let raw = raw.ok_or("Missing accepted Haskell ledger snapshot")?;
    let bytes = decode_hex(&raw, MAX_LEDGER_HEX_BYTES)?;
    let ledger = decode_ledger_state(&bytes)
        .map_err(|e| format!("Noncanonical Haskell ledger snapshot: {e:?}"))?;
    let claimed = parse_hash(&state_root.ok_or("Missing Haskell state commitment")?)?;
    let calculated =
        ledger_state_root_v1(&ledger).map_err(|e| format!("Invalid successor ledger: {e:?}"))?;
    if claimed != calculated {
        return Err("Haskell response state commitment mismatch".into());
    }
    Ok(ledger)
}

/// Check a complete signed-batch reply, including the EXACT ordered list
/// of transaction IDs, the Merkle root, and the post-Haskell state root.
pub fn decode_signed_batch_reply(
    response: &[u8],
    expected_ids: &[[u8; 32]],
    expected_root: &[u8; 32],
) -> Result<LedgerState, String> {
    let reply: SignedBatchReply = parse_json(response)?;
    check_envelope(
        &reply.kind,
        &reply.scope,
        (
            "ledger_signed_batch_fixture",
            "ed25519_verified_ordered_batch_only",
        ),
        reply.ok,
        reply.protocol_version,
        reply.accepted,
        &reply.reason,
    )?;
    if expected_ids.is_empty() || expected_ids.len() > 64 {
        return Err("Unsupported signed-batch transaction count".into());
    }
    let reported = reply.transaction_ids.ok_or("Missing signed-batch IDs")?;
    if reported.len() != expected_ids.len() {
        return Err("Haskell reported wrong batch length".into());
    }
    for (reported_id, expected) in reported.iter().zip(expected_ids) {
        if parse_hash(reported_id)? != *expected {
            return Err("Haskell signed-batch ID or order mismatch".into());
        }
    }
    if parse_hash(&reply.transactions_root.ok_or("Missing Merkle root")?)? != *expected_root {
        return Err("Haskell signed-batch Merkle root mismatch".into());
    }
    parse_ledger_and_state_root(reply.state_hex, reply.state_root)
}

/// Check the historical one-transfer fixture with the same strict semantics
/// so the older CLI path cannot authorize malformed JSON by substring.
pub fn decode_signed_transfer_reply(
    response: &[u8],
    expected_id: &[u8; 32],
    expected_root: &[u8; 32],
) -> Result<LedgerState, String> {
    let reply: SignedTransferReply = parse_json(response)?;
    check_envelope(
        &reply.kind,
        &reply.scope,
        (
            "ledger_signed_transfer_fixture",
            "ed25519_verified_local_transfer_only",
        ),
        reply.ok,
        reply.protocol_version,
        reply.accepted,
        &reply.reason,
    )?;
    if parse_hash(&reply.transaction_id.ok_or("Missing transfer ID")?)? != *expected_id {
        return Err("Haskell single-transfer ID mismatch".into());
    }
    if parse_hash(&reply.transactions_root.ok_or("Missing Merkle root")?)? != *expected_root {
        return Err("Haskell single-transfer Merkle root mismatch".into());
    }
    parse_ledger_and_state_root(reply.state_hex, reply.state_root)
}

/// Do not authorize a payout from string searches or caller-supplied balances.
/// Require a strict Haskell envelope, canonical exact receipt IDs and root,
/// then independently check replay markers and fee conservation in Rust.
pub fn decode_reward_block_reply(
    response: &[u8],
    batch_id: &[u8; 32],
    budget: u64,
    expected_receipts: &[[u8; 32]],
    prior: &LedgerState,
) -> Result<LedgerState, String> {
    let reply: RewardBlockReply = parse_json(response)?;
    check_envelope(
        &reply.kind,
        &reply.scope,
        (
            "ledger_reward_block_fixture",
            "caller_attested_fee_funded_reward_block_only",
        ),
        reply.ok,
        reply.protocol_version,
        reply.accepted,
        &reply.reason,
    )?;
    if *batch_id == [0; 32]
        || budget == 0
        || expected_receipts.is_empty()
        || expected_receipts.len() > 64
        || prior.fee_pool < budget
    {
        return Err("Invalid reward batch, receipt count or fee budget".into());
    }
    let reported = reply.receipt_ids.ok_or("Missing settled receipt IDs")?;
    if reported.len() != expected_receipts.len() {
        return Err("Haskell reward receipt count mismatch".into());
    }
    for (reported_id, expected) in reported.iter().zip(expected_receipts) {
        if parse_hash(reported_id)? != *expected {
            return Err("Haskell reward receipt ID or order mismatch".into());
        }
    }
    let state = parse_ledger_and_state_root(reply.state_hex, reply.state_root)?;
    if state.fee_pool != prior.fee_pool - budget {
        return Err("Funded reward changed the wrong amount of fees".into());
    }
    let mut batches = prior.settled_batches.clone();
    if !batches.insert(*batch_id) || state.settled_batches != batches {
        return Err("Reward batch markers were forged or replayed".into());
    }
    let mut paid = prior.paid_receipts.clone();
    for id in expected_receipts {
        if *id == [0; 32] || !paid.insert(*id) {
            return Err("Reward receipt reused or forged".into());
        }
    }
    if state.paid_receipts != paid {
        return Err("Reward receipt markers were not fully committed".into());
    }
    for (address, account) in &prior.accounts {
        match state.accounts.get(address) {
            Some(next) if next.nonce == account.nonce && next.balance >= account.balance => {}
            _ => return Err("Reward settlement modified existing account or nonce".into()),
        }
    }
    for (address, account) in &state.accounts {
        if !prior.accounts.contains_key(address) && account.nonce != 0 {
            return Err("New reward account has an invalid nonce".into());
        }
    }
    let supply = |ledger: &LedgerState| {
        ledger
            .accounts
            .values()
            .map(|a| u128::from(a.balance))
            .sum::<u128>()
            + u128::from(ledger.fee_pool)
    };
    if supply(&state) != supply(prior) {
        return Err("Reward settlement minted funds or destroyed supply".into());
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::ledger_state::encode_ledger_state;
    use crate::transaction_commitment::transaction_root_v1;
    use serde_json::{json, Value};

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn fixture_batch() -> (Vec<u8>, [u8; 32], [u8; 32]) {
        let id = [7; 32];
        let root = transaction_root_v1(&[id]).unwrap();
        let ledger = LedgerState::default();
        let state_bytes = encode_ledger_state(&ledger).unwrap();
        let state_root = ledger_state_root_v1(&ledger).unwrap();
        let response = json!({
            "ok": true,
            "type": "ledger_signed_batch_fixture",
            "protocol_version": 1,
            "accepted": true,
            "reason": "none",
            "scope": "ed25519_verified_ordered_batch_only",
            "state_hex": hex(&state_bytes),
            "transaction_ids": [hex(&id)],
            "transactions_root": hex(&root),
            "state_root": hex(&state_root)
        });
        (serde_json::to_vec(&response).unwrap(), id, root)
    }

    #[test]
    fn accepts_valid_typed_batch_and_whitespace_reordered_json() {
        let (bytes, id, root) = fixture_batch();
        assert_eq!(
            decode_signed_batch_reply(&bytes, &[id], &root),
            Ok(LedgerState::default())
        );
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let pretty = serde_json::to_vec_pretty(&value).unwrap();
        assert_eq!(
            decode_signed_batch_reply(&pretty, &[id], &root),
            Ok(LedgerState::default())
        );
    }

    #[test]
    fn rejects_duplicate_json_keys_even_when_first_value_is_valid() {
        let (bytes, id, root) = fixture_batch();
        let s = String::from_utf8(bytes).unwrap();
        let altered = s.replacen(
            r#""accepted":true"#,
            r#""accepted":true,"accepted":false"#,
            1,
        );
        assert!(decode_signed_batch_reply(altered.as_bytes(), &[id], &root).is_err());
    }

    #[test]
    fn rejects_status_embedded_in_reason_string_or_nested_fields() {
        let (bytes, id, root) = fixture_batch();
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value["accepted"] = json!(false);
        value["reason"] = json!(r#"mock \"accepted\":true and \"ok\":true"#);
        assert!(
            decode_signed_batch_reply(&serde_json::to_vec(&value).unwrap(), &[id], &root).is_err()
        );
        value["accepted"] = json!(true);
        value["reason"] = json!("none");
        value["extra"] = json!({"accepted":true});
        assert!(
            decode_signed_batch_reply(&serde_json::to_vec(&value).unwrap(), &[id], &root).is_err()
        );
    }

    #[test]
    fn rejects_wrong_order_types_roots_and_snapshot() {
        let (bytes, id, root) = fixture_batch();
        let original: Value = serde_json::from_slice(&bytes).unwrap();
        for (field, changed) in [
            ("protocol_version", json!(2)),
            ("ok", json!("true")),
            ("transaction_ids", json!([hex(&[8; 32])])),
            ("transaction_ids", json!([hex(&id), hex(&id)])),
            ("transactions_root", json!(hex(&[9; 32]))),
            ("state_root", json!(hex(&[9; 32]))),
            ("state_hex", json!("00")),
            ("state_hex", json!("ABCD")),
            ("scope", json!("other")),
            ("reason", json!("danger")),
        ] {
            let mut v = original.clone();
            v[field] = changed;
            assert!(
                decode_signed_batch_reply(&serde_json::to_vec(&v).unwrap(), &[id], &root).is_err(),
                "should reject {field}"
            );
        }
        assert!(decode_signed_batch_reply(&bytes, &[[5; 32]], &root).is_err());
        assert!(decode_signed_batch_reply(&bytes, &[id], &[6; 32]).is_err());
    }

    #[test]
    fn rejects_oversized_and_malformed_response() {
        let (bytes, id, root) = fixture_batch();
        assert!(
            decode_signed_batch_reply(&vec![32; MAX_IPC_PAYLOAD_BYTES + 1], &[id], &root).is_err()
        );
        assert!(decode_signed_batch_reply(&bytes[..bytes.len() - 1], &[id], &root).is_err());
        assert!(decode_signed_batch_reply(&[0xff, 0xfe], &[id], &root).is_err());
    }

    #[test]
    fn verifies_single_transfer_response_with_typed_fields() {
        let (bytes, id, root) = fixture_batch();
        let mut v: Value = serde_json::from_slice(&bytes).unwrap();
        v["type"] = json!("ledger_signed_transfer_fixture");
        v["scope"] = json!("ed25519_verified_local_transfer_only");
        v["transaction_id"] = json!(hex(&id));
        v.as_object_mut().unwrap().remove("transaction_ids");
        let accepted = serde_json::to_vec(&v).unwrap();
        assert_eq!(
            decode_signed_transfer_reply(&accepted, &id, &root),
            Ok(LedgerState::default())
        );
        v["transaction_id"] = json!(hex(&[0; 32]));
        assert!(
            decode_signed_transfer_reply(&serde_json::to_vec(&v).unwrap(), &id, &root).is_err()
        );
        assert!(decode_signed_transfer_reply(&bytes, &id, &root).is_err());
    }
}
