//! AivoBlock Rust node CLI.
//!
//! The `ai-health` command is the first local Rust <-> Python IPC
//! integration. No blockchain network is started by these commands.

mod ipc;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use ipc::haskell::HaskellValidatorClient;
use ipc::python::PythonWorkerClient;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const CORE_HEALTH_REQUEST: &[u8] = br#"{\"protocol_version\":1,\"type\":\"health\"}"#;
const EXPECTED_CORE_HEALTH_RESPONSE: &str = r#"{\"ok\":true,\"type\":\"health\",\"protocol_version\":1,\"core\":\"aivoblock-core\",\"status\":\"ready_for_protocol_development\"}"#;
const HEALTH_REQUEST: &[u8] = br#"{"protocol_version":1,"type":"health"}"#;
const EXPECTED_HEALTH_RESPONSE: &str = r#"{"ok":true,"type":"health","protocol_version":1,"worker":"aivoblock-ai","status":"ready_for_protocol_development"}"#;

const VALID_GENESIS_REQUEST: &[u8] = br#"{"protocol_version":1,"type":"validate_block_shape","case":"valid_genesis"}"#;
const INVALID_GENESIS_REQUEST: &[u8] = br#"{"protocol_version":1,"type":"validate_block_shape","case":"invalid_genesis_parent"}"#;
const VALID_GENESIS_RESPONSE: &str = r#"{"ok":true,"type":"validate_block_shape","protocol_version":1,"valid":true,"reason":"none","scope":"structural_fixture_only"}"#;
const INVALID_GENESIS_RESPONSE: &str = r#"{"ok":true,"type":"validate_block_shape","protocol_version":1,"valid":false,"reason":"InvalidGenesisParent","scope":"structural_fixture_only"}"#;

const SAMPLE_PUZZLE_REQUEST: &str = r#"{"protocol_version":1,"type":"execute_integer_inference","worker_id":"0505050505050505050505050505050505050505050505050505050505050505","puzzle":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","version":1,"model_commitment":"0202020202020202020202020202020202020202020202020202020202020202","input_commitment":"0303030303030303030303030303030303030303030303030303030303030303","rules_commitment":"0404040404040404040404040404040404040404040404040404040404040404","units":3,"deadline_height":100},"start":0,"count":3,"rows":[[1,2],[3,4],[-2,5]],"weights":[2,3]}"#;

const SAMPLE_VERIFY_REQUEST: &str = r#"{"protocol_version":1,"type":"verify_integer_inference","puzzle":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","version":1,"model_commitment":"0202020202020202020202020202020202020202020202020202020202020202","input_commitment":"0303030303030303030303030303030303030303030303030303030303030303","rules_commitment":"0404040404040404040404040404040404040404040404040404040404040404","units":3,"deadline_height":100},"rows":[[1,2],[3,4],[-2,5]],"weights":[2,3],"proof":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","worker_id":"0505050505050505050505050505050505050505050505050505050505050505","start_unit":0,"unit_count":3,"result_commitment":"a22d7dc4fe5191094e273e20c50df061c516b244a030227957c61ed4cab8761f","witness_commitment":null}}"#;
const SAMPLE_TAMPERED_VERIFY_REQUEST: &str = r#"{"protocol_version":1,"type":"verify_integer_inference","puzzle":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","version":1,"model_commitment":"0202020202020202020202020202020202020202020202020202020202020202","input_commitment":"0303030303030303030303030303030303030303030303030303030303030303","rules_commitment":"0404040404040404040404040404040404040404040404040404040404040404","units":3,"deadline_height":100},"rows":[[1,2],[3,4],[-2,5]],"weights":[2,3],"proof":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","worker_id":"0505050505050505050505050505050505050505050505050505050505050505","start_unit":0,"unit_count":3,"result_commitment":"0000000000000000000000000000000000000000000000000000000000000000","witness_commitment":null}}"#;

const JSON_GENESIS_VALID: &str = r#"{"protocol_version":1,"type":"validate_block_shape","block":{"header":{"version":1,"chain_id":1,"height":0,"parent_hash":"0000000000000000000000000000000000000000000000000000000000000000","transactions_root":"0000000000000000000000000000000000000000000000000000000000000000","puzzle_proofs_root":"0000000000000000000000000000000000000000000000000000000000000000","state_root":"0000000000000000000000000000000000000000000000000000000000000000","timestamp_ms":0},"body":{"transaction_ids":[],"puzzle_proof_ids":[]}}}"#;
const JSON_GENESIS_INVALID: &str = r#"{"protocol_version":1,"type":"validate_block_shape","block":{"header":{"version":1,"chain_id":1,"height":0,"parent_hash":"0101010101010101010101010101010101010101010101010101010101010101","transactions_root":"0000000000000000000000000000000000000000000000000000000000000000","puzzle_proofs_root":"0000000000000000000000000000000000000000000000000000000000000000","state_root":"0000000000000000000000000000000000000000000000000000000000000000","timestamp_ms":0},"body":{"transaction_ids":[],"puzzle_proof_ids":[]}}}"#;

fn print_help() {
    println!(
        "AivoBlock Node {VERSION}\n\n\
         Usage: aivoblock-node [COMMAND]\n\n\
         Commands:\n\
           status       Show implementation status (default)\n\
           core-health  Check the local Haskell core over framed IPC\n\
           core-validate  Check valid and invalid Haskell block fixtures\n\
           core-json      Send full JSON block candidates to Haskell\n\
           ai-health    Check the local Python AI worker over framed IPC\n\
           ai-demo      Execute a sample AI puzzle using Python IPC\n\
           ai-verify    Verify a sample proof and reject a tampered proof\n\
           --version    Display package version\n\
           --help       Display this help"
    );
}

fn print_status() {
    println!("AivoBlock Node {VERSION}");
    println!("Status: Initial development");
    println!("Core validation: Haskell (diagnostic IPC via core-health; validation not connected)");
    println!("AI puzzle worker: Python (health IPC available via ai-health)");
    println!("P2P networking: Not implemented");
    println!("Persistent ledger: Not implemented");
    println!("No node has been started.");
}

fn default_worker_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ai-engine/src/aivoblock_ai/worker.py")
}


fn core_health() -> Result<(), String> {
    // Use a trusted, locally built core executable, not an arbitrary command.
    // On Windows set AIVOBLOCK_HASKELL_EXECUTABLE to the .exe path returned
    // by "cabal list-bin exe:aivoblock-core" from the core directory.
    let executable = env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            "Set AIVOBLOCK_HASKELL_EXECUTABLE to the compiled Haskell core executable (run cabal list-bin exe:aivoblock-core in core/).".to_owned()
        })?;
    let client = HaskellValidatorClient::new(executable)
        .map_err(|e| format!("Haskell core configuration error: {e:?}"))?;
    let bytes = client.exchange(CORE_HEALTH_REQUEST)
        .map_err(|e| format!("Haskell core IPC error: {e:?}"))?;
    let response = std::str::from_utf8(&bytes)
        .map_err(|_| "Haskell core response is not UTF-8".to_owned())?;
    // This is only an exact diagnostic contract, not proof of block validity.
    if response != EXPECTED_CORE_HEALTH_RESPONSE {
        return Err(format!("Unexpected Haskell core health response: {response}"));
    }
    println!("Haskell core IPC: healthy");
    println!("Core: aivoblock-core");
    println!("Protocol version: 1");
    println!("Block validation over IPC: Not implemented");
    Ok(())
}


fn core_validate() -> Result<(), String> {
    let executable = env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            "Set AIVOBLOCK_HASKELL_EXECUTABLE using cabal list-bin exe:aivoblock-core from core/.".to_owned()
        })?;
    let client = HaskellValidatorClient::new(executable)
        .map_err(|e| format!("Haskell core configuration error: {e:?}"))?;
    for (request, expected_response) in [
        (VALID_GENESIS_REQUEST, VALID_GENESIS_RESPONSE),
        (INVALID_GENESIS_REQUEST, INVALID_GENESIS_RESPONSE),
    ] {
        let result = client.exchange(request)
            .map_err(|e| format!("Haskell block validation IPC error: {e:?}"))?;
        let response = std::str::from_utf8(&result)
            .map_err(|_| "Haskell block validation response is not UTF-8".to_owned())?;
        if response != expected_response {
            return Err(format!("Unexpected Haskell block validation response: {response}"));
        }
    }
    println!("Haskell structural block validation demo: PASS");
    println!("Valid genesis fixture: accepted");
    println!("Invalid genesis parent fixture: rejected");
    println!("Scope: structural fixtures only; no full block validation or consensus");
    Ok(())
}


fn core_json() -> Result<(), String> {
    let executable = env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "Set AIVOBLOCK_HASKELL_EXECUTABLE using cabal list-bin exe:aivoblock-core from core/.".to_owned())?;
    let client = HaskellValidatorClient::new(executable)
        .map_err(|e| format!("Haskell core configuration error: {e:?}"))?;
    for (request, expected_valid, expected_reason) in [
        (JSON_GENESIS_VALID, true, "none"),
        (JSON_GENESIS_INVALID, false, "InvalidGenesisParent"),
    ] {
        let raw = client.exchange(request.as_bytes())
            .map_err(|e| format!("Haskell core IPC failed: {e:?}"))?;
        let response = std::str::from_utf8(&raw)
            .map_err(|_| "Haskell JSON block response is not UTF-8".to_owned())?;
        // The Haskell JSON encoder may reorder keys. Match all required
        // diagnostic fields, never treat the response as consensus approval.
        let required = [
            r#""ok":true"#,
            r#""type":"validate_block_shape""#,
            r#""protocol_version":1"#,
            r#""scope":"structural_only""#,
            if expected_valid { r#""valid":true"# } else { r#""valid":false"# },
        ];
        if !required.iter().all(|field| response.contains(field))
            || !response.contains(&format!(r#""reason":"{expected_reason}""#))
        {
            return Err(format!("Unexpected structural validation response: {response}"));
        }
    }
    println!("JSON block structural validation demo: PASS");
    println!("Valid JSON genesis candidate: accepted");
    println!("Invalid JSON genesis parent: rejected");
    println!("Scope: structural validation only; no signatures, roots, or consensus");
    Ok(())
}

fn ai_health() -> Result<(), String> {
    // Environment overrides are local configuration, not trusted network inputs.
    let executable = env::var_os("AIVOBLOCK_PYTHON_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }));
    let script = env::var_os("AIVOBLOCK_PYTHON_WORKER")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_worker_path);

    let client = PythonWorkerClient::new(&executable, &script)
        .map_err(|err| format!("AI worker configuration failed: {err:?}"))?;
    let bytes = client.exchange(HEALTH_REQUEST)
        .map_err(|err| format!("AI worker IPC failed: {err:?}. Check Python and the worker path."))?;
    let response = std::str::from_utf8(&bytes)
        .map_err(|_| "AI worker response is not UTF-8".to_owned())?;

    // Temporary strict diagnostic contract: reject any unexpected response.
    // Actual puzzle results will require typed decoding and verification.
    if response != EXPECTED_HEALTH_RESPONSE {
        return Err(format!("Unexpected AI worker health response: {response}"));
    }
    println!("AI worker IPC: healthy");
    println!("Worker: aivoblock-ai");
    println!("Protocol version: 1");
    println!("AI puzzles: Not wired to the Rust node yet");
    Ok(())
}

fn ai_demo() -> Result<(), String> {
    let executable = env::var_os("AIVOBLOCK_PYTHON_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }));
    let script = env::var_os("AIVOBLOCK_PYTHON_WORKER")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_worker_path);
    let client = PythonWorkerClient::new(executable, script)
        .map_err(|e| format!("Worker configuration error: {e:?}"))?;
    let response = client.exchange(SAMPLE_PUZZLE_REQUEST.as_bytes())
        .map_err(|e| format!("Worker IPC error: {e:?}"))?;
    let response = std::str::from_utf8(&response)
        .map_err(|_| "Worker response is not UTF-8".to_owned())?;
    // Strictly match this fixture's entire result, including the exact
    // SHA-256 commitment. Substring checks could accept a tampered response.
    // This is a diagnostic test vector, NOT general proof verification.
    const EXPECTED_DEMO_RESPONSE: &str = r#"{"ok":true,"type":"execute_integer_inference","protocol_version":1,"outputs":[8,18,11],"proof":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","worker_id":"0505050505050505050505050505050505050505050505050505050505050505","start_unit":0,"unit_count":3,"result_commitment":"a22d7dc4fe5191094e273e20c50df061c516b244a030227957c61ed4cab8761f","witness_commitment":null},"verified":false}"#;
    if response != EXPECTED_DEMO_RESPONSE {
        return Err(format!("Unexpected AI worker response: {response}"));
    }
    println!("AI puzzle demo completed");
    println!("Expected outputs: [8, 18, 11]");
    println!("Worker response: {response}");
    println!("Proof status: UNVERIFIED; no ledger update performed");
    Ok(())
}


fn ai_verify() -> Result<(), String> {
    let executable = env::var_os("AIVOBLOCK_PYTHON_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }));
    let script = env::var_os("AIVOBLOCK_PYTHON_WORKER")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_worker_path);
    let client = PythonWorkerClient::new(executable, script)
        .map_err(|e| format!("Worker configuration error: {e:?}"))?;
    // Two separate worker processes recompute the locally trusted fixtures.
    for (request, expected) in [
        (SAMPLE_VERIFY_REQUEST, true),
        (SAMPLE_TAMPERED_VERIFY_REQUEST, false),
    ] {
        let bytes = client.exchange(request.as_bytes())
            .map_err(|e| format!("Verifier IPC error: {e:?}"))?;
        let response = std::str::from_utf8(&bytes)
            .map_err(|_| "Verifier response is not UTF-8".to_owned())?;
        let expected_response = if expected {
            r#"{"ok":true,"type":"verify_integer_inference","protocol_version":1,"verified":true,"method":"full_recomputation"}"#
        } else {
            r#"{"ok":true,"type":"verify_integer_inference","protocol_version":1,"verified":false,"method":"full_recomputation"}"#
        };
        if response != expected_response {
            return Err(format!("Unexpected verifier response: {response}"));
        }
    }
    println!("AI verification demo: PASS");
    println!("Valid sample proof: accepted by recomputation");
    println!("Tampered sample proof: rejected by recomputation");
    println!("Verification uses local trusted fixtures; no ledger update performed");
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [] => {
            print_status();
            Ok(())
        }
        [command] if command == "status" => {
            print_status();
            Ok(())
        }
        [command] if command == "core-health" => core_health(),
        [command] if command == "core-validate" => core_validate(),
        [command] if command == "core-json" => core_json(),
        [command] if command == "ai-health" => ai_health(),
        [command] if command == "ai-demo" => ai_demo(),
        [command] if command == "ai-verify" => ai_verify(),
        [command] if command == "--help" || command == "-h" => {
            print_help();
            Ok(())
        }
        [command] if command == "--version" || command == "-V" => {
            println!("{VERSION}");
            Ok(())
        }
        _ => Err("Unknown command or arguments. Use --help.".to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{default_worker_path, run};

    #[test]
    fn basic_commands_are_accepted() {
        for args in [
            vec![],
            vec!["status".to_owned()],
            vec!["--help".to_owned()],
            vec!["--version".to_owned()],
        ] {
            assert!(run(&args).is_ok());
        }
    }

    #[test]
    fn unknown_commands_are_rejected() {
        assert!(run(&["start".to_owned()]).is_err());
        assert!(run(&["ai-health".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["core-health".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["core-validate".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["core-json".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["ai-demo".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["ai-verify".to_owned(), "extra".to_owned()]).is_err());
    }

    #[test]
    fn default_worker_location_is_under_ai_engine() {
        assert!(default_worker_path().ends_with("ai-engine/src/aivoblock_ai/worker.py"));
    }
}
