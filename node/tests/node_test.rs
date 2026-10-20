//! Black-box integration tests for the AivoBlock node CLI.
//!
//! These test the current diagnostic interface only. They do not imply
//! the network, consensus engine, or AI workers are operational.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aivoblock-node"))
        .args(args)
        .output()
        .expect("AivoBlock node executable should start")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("CLI stdout must be UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("CLI stderr must be UTF-8")
}

#[test]
fn default_command_reports_initial_status() {
    let output = run(&[]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("Status: Initial development"));
    assert!(stdout(&output).contains("No node has been started."));
}

#[test]
fn explicit_status_reports_testnet_components() {
    let output = run(&["status"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("P2P networking: authenticated TCP private testnet"));
    assert!(text.contains("Persistent ledger: RocksDB finalized chain"));
}

#[test]
fn help_documents_supported_commands() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let text = stdout(&output);
    for command in [
        "status",
        "testnet-init",
        "run",
        "client",
        "bench",
        "--version",
        "--help",
    ] {
        assert!(text.contains(command), "missing command: {command}");
    }
}

#[test]
fn testnet_init_writes_signed_config_and_separates_private_keys() {
    let out = std::env::temp_dir().join(format!("aivoblock-init-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let target = out.to_str().expect("UTF-8 temp path");
    let output = run(&["testnet-init", "--out", target, "--validators", "4"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let network = std::fs::read_to_string(out.join("public/network.json")).unwrap();
    assert!(network.contains("\"config_signature\""));
    assert_eq!(network.matches("\"voting_power\"").count(), 4);
    for node in 1..=4 {
        let dir = out.join(format!("node-{node}"));
        assert!(dir.join("validator.key").exists());
        assert!(dir.join("worker.key").exists());
        assert_eq!(dir.join("authority.key").exists(), node == 1);
    }
    let env = std::fs::read_to_string(out.join("testnet.env")).unwrap();
    assert!(env.starts_with("AIVOBLOCK_AUTHORITY_PUBLIC_KEY="));
    // Refuse to overwrite an existing testnet (never clobber keys).
    assert!(!run(&["testnet-init", "--out", target]).status.success());
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn run_refuses_to_start_without_a_pinned_authority() {
    let output = Command::new(env!("CARGO_BIN_EXE_aivoblock-node"))
        .arg("run")
        .env_remove("AIVOBLOCK_AUTHORITY_PUBLIC_KEY")
        .output()
        .expect("node starts");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("AIVOBLOCK_AUTHORITY_PUBLIC_KEY"));
}

#[test]
fn version_matches_cargo_package() {
    let output = run(&["--version"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output).trim(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn unknown_command_returns_failure() {
    let output = run(&["start"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("Unknown command"));
}

#[test]
fn extra_arguments_return_failure() {
    let output = run(&["status", "unexpected"]);
    assert!(!output.status.success());
}
