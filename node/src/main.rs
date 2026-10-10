//! AivoBlock Rust node entry point.
//!
//! This executable provides initial diagnostics and deliberately does not
//! advertise a functional blockchain network until networking, storage,
//! and the Haskell/Python integrations have been implemented.

use std::env;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!(
        "AivoBlock Node {VERSION}\n\n\
         Usage: aivoblock-node [COMMAND]\n\n\
         Commands:\n\
           status       Show implementation status (default)\n\
           --version    Display package version\n\
           --help       Display this help"
    );
}

fn print_status() {
    println!("AivoBlock Node {VERSION}");
    println!("Status: Initial development");
    println!("Core validation: Haskell (not connected)");
    println!("AI puzzle worker: Python (not connected)");
    println!("P2P networking: Not implemented");
    println!("Persistent ledger: Not implemented");
    println!("No node has been started.");
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
    use super::run;

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
    }
}
