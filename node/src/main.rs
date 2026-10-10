//! AivoBlock Rust node CLI.
//!
//! The `ai-health` command is the first local Rust <-> Python IPC
//! integration. No blockchain network is started by these commands.

mod ipc;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use ipc::python::PythonWorkerClient;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const HEALTH_REQUEST: &[u8] = br#"{"protocol_version":1,"type":"health"}"#;
const EXPECTED_HEALTH_RESPONSE: &str = r#"{"ok":true,"type":"health","protocol_version":1,"worker":"aivoblock-ai","status":"ready_for_protocol_development"}"#;

const SAMPLE_PUZZLE_REQUEST: &str = r#"{"protocol_version":1,"type":"execute_integer_inference","worker_id":"0505050505050505050505050505050505050505050505050505050505050505","puzzle":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","version":1,"model_commitment":"0202020202020202020202020202020202020202020202020202020202020202","input_commitment":"0303030303030303030303030303030303030303030303030303030303030303","rules_commitment":"0404040404040404040404040404040404040404040404040404040404040404","units":3,"deadline_height":100},"start":0,"count":3,"rows":[[1,2],[3,4],[-2,5]],"weights":[2,3]}"#;

fn print_help() {
    println!(
        "AivoBlock Node {VERSION}\n\n\
         Usage: aivoblock-node [COMMAND]\n\n\
         Commands:\n\
           status       Show implementation status (default)\n\
           ai-health    Check the local Python AI worker over framed IPC\n\
           ai-demo      Execute a sample AI puzzle using Python IPC\n\
           --version    Display package version\n\
           --help       Display this help"
    );
}

fn print_status() {
    println!("AivoBlock Node {VERSION}");
    println!("Status: Initial development");
    println!("Core validation: Haskell (not connected)");
    println!("AI puzzle worker: Python (health IPC available via ai-health)");
    println!("P2P networking: Not implemented");
    println!("Persistent ledger: Not implemented");
    println!("No node has been started.");
}

fn default_worker_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ai-engine/src/aivoblock_ai/worker.py")
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
        [command] if command == "ai-health" => ai_health(),
        [command] if command == "ai-demo" => ai_demo(),
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
        assert!(run(&["ai-demo".to_owned(), "extra".to_owned()]).is_err());
    }

    #[test]
    fn default_worker_location_is_under_ai_engine() {
        assert!(default_worker_path().ends_with("ai-engine/src/aivoblock_ai/worker.py"));
    }
}
