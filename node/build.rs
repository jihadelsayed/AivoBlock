// Build metadata for the AivoBlock Rust node.
//
// This script intentionally performs no network requests, code generation,
// or external tool invocation. Consensus parameters must not be derived
// from the developer's machine or build timestamp.

fn main() {
    // Rebuild only when this build script changes.
    println!("cargo:rerun-if-changed=build.rs");

    // Embed a stable package version for diagnostic use only.
    // The blockchain protocol version is specified separately and must
    // not depend on Cargo package versions.
    println!(
        "cargo:rustc-env=AIVOBLOCK_BUILD_VERSION={}",
        env!("CARGO_PKG_VERSION")
    );
}
