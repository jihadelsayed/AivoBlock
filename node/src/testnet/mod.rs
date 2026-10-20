//! Private research testnet runtime: authenticated TCP peers, deterministic
//! AI-work proposals, durable-lock precommit voting with quorum
//! certificates, block sync, restart recovery and a local control API.
//!
//! Scope and limits: a fixed, permissioned validator set from a signed
//! network configuration; no transport encryption; no stake, slashing or
//! validator rotation; full recomputation of a tiny integer workload. Never
//! use this to protect real funds or to run a public network.

pub mod bench;
pub mod chain;
pub mod cli;
pub mod config;
pub mod control;
pub mod init;
pub mod node;
pub mod peer;
pub mod wire;
pub mod worker;

#[cfg(test)]
mod tests;

use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Strict lowercase/uppercase hex decoding with an explicit size bound.
pub fn unhex(raw: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    if raw.len() % 2 != 0 || raw.len() / 2 > max_bytes {
        return Err("Invalid or oversized hex field".into());
    }
    (0..raw.len())
        .step_by(2)
        .map(|i| {
            raw.get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| "Invalid hex digit".to_owned())
        })
        .collect()
}

pub fn unhex_array<const N: usize>(raw: &str) -> Result<[u8; N], String> {
    unhex(raw, N)?
        .try_into()
        .map_err(|_| format!("Expected {N} hex-encoded bytes"))
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 32 bytes from the OS (`/dev/urandom`). The fallback hashes time, process
/// and a counter: it is unique but NOT cryptographically unpredictable and
/// exists only so Windows unit tests can run; containers use the OS source.
pub fn random_bytes32() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        if file.read_exact(&mut bytes).is_ok() && bytes != [0; 32] {
            return bytes;
        }
    }
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut digest = Sha256::new();
    digest.update(b"AIVOBLOCK-FALLBACK-RANDOM-V1\0");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    digest.update(nanos.to_be_bytes());
    digest.update(std::process::id().to_be_bytes());
    digest.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_be_bytes());
    digest.update(format!("{:?}", std::thread::current().id()).as_bytes());
    digest.finalize().into()
}

pub fn env_or(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_owned())
}
