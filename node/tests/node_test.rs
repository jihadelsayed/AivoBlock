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
fn explicit_status_reports_unimplemented_components() {
    let output = run(&["status"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("P2P networking: Not implemented"));
    assert!(text.contains("Persistent ledger: Not implemented"));
}

#[test]
fn help_documents_supported_commands() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let text = stdout(&output);
    for command in ["status", "--version", "--help"] {
        assert!(text.contains(command), "missing command: {command}");
    }
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
