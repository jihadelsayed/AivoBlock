//! Local Rust scheduler -> Python worker -> recomputation diagnostic.
//!
//! This is a pinned, deterministic fixture only. A Python verifier receives
//! the same locally trusted inputs; no independent data availability,
//! network coordination, durable replay state, consensus or rewards are established.

use std::env;
use std::path::PathBuf;

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use crate::ipc::python::PythonWorkerClient;
use crate::mining::auth::{assignment_signing_bytes, AssignmentAuthorizer, SignedAssignment};
use crate::mining::receipts::{ReceiptRegistry, VerifiedWorkReceipt};
use crate::mining::{Assignment, MiningScheduler, PuzzleTask};

const ROWS: [[i64; 2]; 3] = [[1, 2], [3, 4], [-2, 5]];
const WEIGHTS: [i64; 2] = [2, 3];
const ROWS_JSON: &str = "[[1,2],[3,4],[-2,5]]";
const WEIGHTS_JSON: &str = "[2,3]";
const VERIFIED_RESPONSE: &str = r#"{"ok":true,"type":"verify_integer_inference","protocol_version":1,"verified":true,"method":"full_recomputation"}"#;
const REJECTED_RESPONSE: &str = r#"{"ok":true,"type":"verify_integer_inference","protocol_version":1,"verified":false,"method":"full_recomputation"}"#;

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex32(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn puzzle_json() -> String {
    format!(
        r#"{{"puzzle_id":"{}","version":1,"model_commitment":"{}","input_commitment":"{}","rules_commitment":"{}","units":3,"deadline_height":100}}"#,
        hex32(1),
        "841c1f0051800864f7189935a818c964893e1b96fe1c610136c23a1284a254bc",
        "548121e870e795733aaabbca753b290431797562a55a1301fcf6113f568c0e3d",
        "d80998d00fe21a0127cc7edf3915cd5de202d702b919259bbf74aa19aad63902"
    )
}

fn execution_request(assignment: &Assignment) -> String {
    format!(
        r#"{{"protocol_version":1,"type":"execute_integer_inference","worker_id":"{}","puzzle":{},"start":{},"count":{},"rows":{ROWS_JSON},"weights":{WEIGHTS_JSON}}}"#,
        hex_bytes(&assignment.worker_id),
        puzzle_json(),
        assignment.range.start,
        assignment.range.count,
    )
}

fn verification_request(proof_json: &str) -> String {
    format!(
        r#"{{"protocol_version":1,"type":"verify_integer_inference","puzzle":{},"rows":{ROWS_JSON},"weights":{WEIGHTS_JSON},"proof":{proof_json}}}"#,
        puzzle_json()
    )
}

/// Independently compute the Python prototype's result commitment bytes.
fn commitment_hex(start: u64, outputs: &[i64]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"AIVOBLOCK-INFERENCE-V1\0");
    hash.update(start.to_be_bytes());
    hash.update((outputs.len() as u64).to_be_bytes());
    for value in outputs {
        let ascii = value.to_string();
        hash.update((ascii.len() as u32).to_be_bytes());
        hash.update(ascii.as_bytes());
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn expected_outputs(assignment: &Assignment) -> Result<Vec<i64>, String> {
    let start = usize::try_from(assignment.range.start)
        .map_err(|_| "Assignment start is outside this fixture")?;
    let count = usize::try_from(assignment.range.count)
        .map_err(|_| "Assignment count is outside this fixture")?;
    let end = start
        .checked_add(count)
        .filter(|end| *end <= ROWS.len())
        .ok_or("Assignment exceeds the trusted fixture rows")?;
    Ok(ROWS[start..end]
        .iter()
        .map(|row| row[0] * WEIGHTS[0] + row[1] * WEIGHTS[1])
        .collect())
}

fn proof_json(assignment: &Assignment, commitment: &str) -> String {
    format!(
        r#"{{"puzzle_id":"{}","worker_id":"{}","start_unit":{},"unit_count":{},"result_commitment":"{commitment}","witness_commitment":null}}"#,
        hex_bytes(&assignment.puzzle_id),
        hex_bytes(&assignment.worker_id),
        assignment.range.start,
        assignment.range.count
    )
}

/// Run exactly two local assignments, comparing the complete worker reply
/// with a Rust-computed canonical commitment before asking Python to verify.
pub fn demo() -> Result<(), String> {
    let executable = env::var_os("AIVOBLOCK_PYTHON_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }));
    let script = env::var_os("AIVOBLOCK_PYTHON_WORKER")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../ai-engine/src/aivoblock_ai/worker.py")
        });
    let client = PythonWorkerClient::new(executable, script)
        .map_err(|error| format!("Worker configuration: {error:?}"))?;

    let puzzle_id = [1; 32];
    let mut scheduler = MiningScheduler::new();
    scheduler
        .add_puzzle(PuzzleTask {
            puzzle_id,
            total_units: ROWS.len() as u64,
        })
        .map_err(|error| format!("Puzzle scheduling: {error:?}"))?;

    // The signing keys below are public, fixed TEST VECTORS for this local demo.
    // Never reuse these keys for real workers or authority on a network.
    let issuer_key = SigningKey::from_bytes(&[7; 32]);
    let issuer_public_key = issuer_key.verifying_key().to_bytes();
    let mut authorizer = AssignmentAuthorizer::new();
    let mut receipts = ReceiptRegistry::new();
    for (index, worker_seed) in [[5; 32], [6; 32]].into_iter().enumerate() {
        let worker_id = SigningKey::from_bytes(&worker_seed)
            .verifying_key()
            .to_bytes();
        let assignment = scheduler
            .assign(puzzle_id, worker_id, 2)
            .map_err(|error| format!("Work assignment: {error:?}"))?;
        // Gate the execution request on the issuer signature and replay check.
        let mut signed = SignedAssignment {
            chain_id: 1,
            assignment,
            worker_public_key: worker_id,
            issuer_public_key,
            deadline_height: 100,
            nonce: index as u64,
            signature: [0; 64],
        };
        signed.signature = issuer_key
            .sign(&assignment_signing_bytes(&signed))
            .to_bytes();
        let authenticated = authorizer
            .verify_and_record(&signed, 1, issuer_public_key, 1)
            .map_err(|error| format!("Assignment authorization: {error:?}"))?;
        let outputs = expected_outputs(&authenticated)?;
        let commitment = commitment_hex(authenticated.range.start, &outputs);
        let proof = proof_json(&authenticated, &commitment);
        let outputs_json = format!(
            "[{}]",
            outputs
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        let expected = format!(
            r#"{{"ok":true,"type":"execute_integer_inference","protocol_version":1,"outputs":{outputs_json},"proof":{proof},"verified":false}}"#
        );

        let execution = client
            .exchange(execution_request(&authenticated).as_bytes())
            .map_err(|error| format!("Python execution IPC: {error:?}"))?;
        if execution.as_slice() != expected.as_bytes() {
            return Err(format!(
                "Worker {index} returned a result that disagrees with Rust's pinned fixture"
            ));
        }

        let verification = client
            .exchange(verification_request(&proof).as_bytes())
            .map_err(|error| format!("Python verification IPC: {error:?}"))?;
        if verification.as_slice() != VERIFIED_RESPONSE.as_bytes() {
            return Err(format!("Worker {index} proof failed local recomputation"));
        }

        if index == 0 {
            // A forged commitment must fail even with otherwise valid fields.
            let tampered = proof.replace(&commitment, &hex32(0));
            let rejected = client
                .exchange(verification_request(&tampered).as_bytes())
                .map_err(|error| format!("Tampered-proof IPC: {error:?}"))?;
            if rejected.as_slice() != REJECTED_RESPONSE.as_bytes() {
                return Err("Tampered result commitment was not rejected".to_owned());
            }
        }

        // Receipt admission is intentionally after both independent Rust
        // fixture comparison and the Python recomputation verifier.
        let mut result_commitment = [0u8; 32];
        for (position, byte) in result_commitment.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&commitment[position * 2..position * 2 + 2], 16)
                .map_err(|_| "Invalid result commitment encoding")?;
        }
        receipts
            .record_verified(
                &authenticated,
                VerifiedWorkReceipt {
                    puzzle_id: authenticated.puzzle_id,
                    worker_id: authenticated.worker_id,
                    range: authenticated.range,
                    result_commitment,
                },
            )
            .map_err(|error| format!("Receipt admission: {error:?}"))?;

        println!(
            "Partition {index}: range [{}..{}), outputs {:?}, proof recomputed",
            assignment.range.start,
            assignment.range.start + assignment.range.count,
            outputs
        );
    }

    if scheduler.remaining_units(&puzzle_id) != Some(0) {
        return Err("The local scheduler left some puzzle units unassigned".to_owned());
    }
    if receipts.receipts_for(puzzle_id).len() != 2 {
        return Err("Expected two locally verified work receipts".to_owned());
    }
    println!("AI partition IPC demo: PASS (3 units / 2 signed assignments / 2 verified receipts)");
    println!("Scope: fixture-only issuer signatures; no P2P, consensus or rewards");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_range_commitment_matches_published_python_vector() {
        assert_eq!(
            commitment_hex(0, &[8, 18, 11]),
            "a22d7dc4fe5191094e273e20c50df061c516b244a030227957c61ed4cab8761f"
        );
    }

    #[test]
    fn partitioned_commitments_are_range_bound() {
        assert_ne!(commitment_hex(0, &[8, 18]), commitment_hex(1, &[8, 18]));
        assert_ne!(commitment_hex(0, &[8, 18]), commitment_hex(0, &[8, 18, 11]));
    }

    #[test]
    fn scheduled_range_bounds_match_the_fixture() {
        let assignment = Assignment {
            worker_id: [5; 32],
            puzzle_id: [1; 32],
            range: crate::mining::WorkRange { start: 1, count: 2 },
        };
        assert_eq!(expected_outputs(&assignment).unwrap(), vec![18, 11]);
    }
}
