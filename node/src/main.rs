//! AivoBlock Rust node CLI.
//!
//! The `ai-health` command is the first local Rust <-> Python IPC
//! integration. No blockchain network is started by these commands.

#[allow(dead_code)] // Some encoders are only exercised by tests and the testnet client.
mod ai_block_v3;
mod ai_pipeline;
mod ai_proof_v2;
mod batch_commitment;
mod batch_pipeline;
mod batch_replay;
mod block_replay;
mod chain_replay;
mod genesis_checkpoint;
#[allow(dead_code)]
// IPC error variants and frame helpers are part of the unfinished node protocol.
mod ipc;
#[allow(dead_code)] // Policy helpers are shared by the runtime and cross-language tests.
mod issuance;
mod ledger_bridge;
#[allow(dead_code)]
mod mining;
#[allow(dead_code)]
mod network;
mod proof_pipeline_v2;
mod qmlp;
mod reward_block;
mod reward_pipeline;
mod settlement_v3;
#[allow(dead_code)] // Columns reserved for ledger features are intentionally unused in the CLI.
mod storage;
mod testnet;
mod transaction_auth;
mod transaction_commitment;
#[allow(dead_code)] // Audit helpers are retained for the V2 diagnostic pipeline.
mod validator_certificate;

use sha2::{Digest, Sha256};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use ipc::haskell::HaskellValidatorClient;
use ipc::python::PythonWorkerClient;
use storage::{Column, KeyValueStore, RocksDbStore};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const CORE_HEALTH_REQUEST: &[u8] = br#"{"protocol_version":1,"type":"health"}"#;
const EXPECTED_CORE_HEALTH_RESPONSE: &str = r#"{"ok":true,"type":"health","protocol_version":1,"core":"aivoblock-core","status":"ready_for_protocol_development"}"#;
const HEALTH_REQUEST: &[u8] = br#"{"protocol_version":1,"type":"health"}"#;
const EXPECTED_HEALTH_RESPONSE: &str = r#"{"ok":true,"type":"health","protocol_version":1,"worker":"aivoblock-ai","status":"ready_for_protocol_development"}"#;

const VALID_GENESIS_REQUEST: &[u8] =
    br#"{"protocol_version":1,"type":"validate_block_shape","case":"valid_genesis"}"#;
const INVALID_GENESIS_REQUEST: &[u8] =
    br#"{"protocol_version":1,"type":"validate_block_shape","case":"invalid_genesis_parent"}"#;

const SAMPLE_PUZZLE_REQUEST: &str = r#"{"protocol_version":1,"type":"execute_integer_inference","worker_id":"0505050505050505050505050505050505050505050505050505050505050505","puzzle":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","version":1,"model_commitment":"841c1f0051800864f7189935a818c964893e1b96fe1c610136c23a1284a254bc","input_commitment":"548121e870e795733aaabbca753b290431797562a55a1301fcf6113f568c0e3d","rules_commitment":"d80998d00fe21a0127cc7edf3915cd5de202d702b919259bbf74aa19aad63902","units":3,"deadline_height":100},"start":0,"count":3,"rows":[[1,2],[3,4],[-2,5]],"weights":[2,3]}"#;

const SAMPLE_VERIFY_REQUEST: &str = r#"{"protocol_version":1,"type":"verify_integer_inference","puzzle":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","version":1,"model_commitment":"841c1f0051800864f7189935a818c964893e1b96fe1c610136c23a1284a254bc","input_commitment":"548121e870e795733aaabbca753b290431797562a55a1301fcf6113f568c0e3d","rules_commitment":"d80998d00fe21a0127cc7edf3915cd5de202d702b919259bbf74aa19aad63902","units":3,"deadline_height":100},"rows":[[1,2],[3,4],[-2,5]],"weights":[2,3],"proof":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","worker_id":"0505050505050505050505050505050505050505050505050505050505050505","start_unit":0,"unit_count":3,"result_commitment":"a22d7dc4fe5191094e273e20c50df061c516b244a030227957c61ed4cab8761f","witness_commitment":null}}"#;
const SAMPLE_TAMPERED_VERIFY_REQUEST: &str = r#"{"protocol_version":1,"type":"verify_integer_inference","puzzle":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","version":1,"model_commitment":"841c1f0051800864f7189935a818c964893e1b96fe1c610136c23a1284a254bc","input_commitment":"548121e870e795733aaabbca753b290431797562a55a1301fcf6113f568c0e3d","rules_commitment":"d80998d00fe21a0127cc7edf3915cd5de202d702b919259bbf74aa19aad63902","units":3,"deadline_height":100},"rows":[[1,2],[3,4],[-2,5]],"weights":[2,3],"proof":{"puzzle_id":"0101010101010101010101010101010101010101010101010101010101010101","worker_id":"0505050505050505050505050505050505050505050505050505050505050505","start_unit":0,"unit_count":3,"result_commitment":"0000000000000000000000000000000000000000000000000000000000000000","witness_commitment":null}}"#;

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
           ai-partition-demo  Execute and verify two locally scheduled AI work ranges\n\
           ai-reward-block-demo  Commit verified local AI receipts in a fee-funded reward block\n\
           ai-proof-v2-demo  Independently verify a signed AI task and reward history\n\
           validator-quorum-demo  Verify signed weighted validator precommits\n\
           ledger-transition-demo  Verify transaction/state roots and store Haskell-executed blocks\n\
           multi-transfer-demo  Execute ordered signed transactions in one atomic block\n\
           storage-demo Persist and reopen a sample block in RocksDB\n\
           validated-storage-demo Check JSON block structure before saving a fixture\n\
           header-hash  Verify Rust canonical header SHA-256 test vector\n\
           two-block-demo  Test two-block RocksDB parent/height continuity\n\
           testnet-init --out DIR [--validators N]  Generate a private testnet\n\
           run          Run a private-testnet node (AIVOBLOCK_* environment)\n\
           client SUB   status|submit-task|wait-height|adversarial|probe-unauthorized\n\
           bench        Verifier, settlement and quorum-certificate benchmarks\n\
           --version    Display package version\n\
           --help       Display this help"
    );
}

fn print_status() {
    println!("AivoBlock Node {VERSION}");
    println!("Status: Initial development");
    println!("Core validation: Haskell (diagnostic IPC via core-health; validation not connected)");
    println!("AI puzzle worker: Python (health IPC available via ai-health)");
    println!("P2P networking: authenticated TCP private testnet (use the run command)");
    println!("Persistent ledger: RocksDB finalized chain with restart replay");
    println!("No node has been started.");
}

fn default_worker_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ai-engine/src/aivoblock_ai/worker.py")
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
    let bytes = client
        .exchange(CORE_HEALTH_REQUEST)
        .map_err(|e| format!("Haskell core IPC error: {e:?}"))?;
    let response =
        std::str::from_utf8(&bytes).map_err(|_| "Haskell core response is not UTF-8".to_owned())?;
    // This is only an exact diagnostic contract, not proof of block validity.
    if response != EXPECTED_CORE_HEALTH_RESPONSE {
        return Err(format!(
            "Unexpected Haskell core health response: {response}"
        ));
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
            "Set AIVOBLOCK_HASKELL_EXECUTABLE using cabal list-bin exe:aivoblock-core from core/."
                .to_owned()
        })?;
    let client = HaskellValidatorClient::new(executable)
        .map_err(|e| format!("Haskell core configuration error: {e:?}"))?;
    for request in [VALID_GENESIS_REQUEST, INVALID_GENESIS_REQUEST] {
        let result = client
            .exchange(request)
            .map_err(|e| format!("Haskell block validation IPC error: {e:?}"))?;
        let response = std::str::from_utf8(&result)
            .map_err(|_| "Haskell block validation response is not UTF-8".to_owned())?;
        // Aeson serializes JSON object keys independently of insertion order.
        // Verify all expected fixture fields without requiring a byte-for-byte
        // order match. This remains a diagnostic check, not consensus parsing.
        let required_fields = [
            r#""ok":true"#,
            r#""type":"validate_block_shape""#,
            r#""protocol_version":1"#,
            r#""scope":"structural_fixture_only""#,
        ];
        let expected_verdict = if request == VALID_GENESIS_REQUEST {
            r#""valid":true"#
        } else {
            r#""valid":false"#
        };
        let expected_reason = if request == VALID_GENESIS_REQUEST {
            r#""reason":"none""#
        } else {
            r#""reason":"InvalidGenesisParent""#
        };
        if !required_fields.iter().all(|field| response.contains(field))
            || !response.contains(expected_verdict)
            || !response.contains(expected_reason)
        {
            return Err(format!(
                "Unexpected Haskell block validation response: {response}; fixture contract mismatch"
            ));
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
        .ok_or_else(|| {
            "Set AIVOBLOCK_HASKELL_EXECUTABLE using cabal list-bin exe:aivoblock-core from core/."
                .to_owned()
        })?;
    let client = HaskellValidatorClient::new(executable)
        .map_err(|e| format!("Haskell core configuration error: {e:?}"))?;
    for (request, expected_valid, expected_reason) in [
        (JSON_GENESIS_VALID, true, "none"),
        (JSON_GENESIS_INVALID, false, "InvalidGenesisParent"),
    ] {
        let raw = client
            .exchange(request.as_bytes())
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
            if expected_valid {
                r#""valid":true"#
            } else {
                r#""valid":false"#
            },
        ];
        if !required.iter().all(|field| response.contains(field))
            || !response.contains(&format!(r#""reason":"{expected_reason}""#))
        {
            return Err(format!(
                "Unexpected structural validation response: {response}"
            ));
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
    let bytes = client.exchange(HEALTH_REQUEST).map_err(|err| {
        format!("AI worker IPC failed: {err:?}. Check Python and the worker path.")
    })?;
    let response =
        std::str::from_utf8(&bytes).map_err(|_| "AI worker response is not UTF-8".to_owned())?;

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
    let response = client
        .exchange(SAMPLE_PUZZLE_REQUEST.as_bytes())
        .map_err(|e| format!("Worker IPC error: {e:?}"))?;
    let response =
        std::str::from_utf8(&response).map_err(|_| "Worker response is not UTF-8".to_owned())?;
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
        let bytes = client
            .exchange(request.as_bytes())
            .map_err(|e| format!("Verifier IPC error: {e:?}"))?;
        let response =
            std::str::from_utf8(&bytes).map_err(|_| "Verifier response is not UTF-8".to_owned())?;
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

fn storage_demo() -> Result<(), String> {
    // Dedicated diagnostic path, never a node's real chain database.
    let path = env::var_os("AIVOBLOCK_STORAGE_DEMO_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("storage-demo-data"));
    let block_key = b"demo-block-v1";
    let block_bytes = b"AivoBlock storage fixture; NOT a validated blockchain block";
    {
        let db = RocksDbStore::open(&path)
            .map_err(|e| format!("Opening storage demo database failed: {e}"))?;
        db.put(Column::Blocks, block_key, block_bytes)
            .map_err(|e| format!("Writing sample block failed: {e}"))?;
        db.flush()
            .map_err(|e| format!("Flushing storage demo failed: {e}"))?;
    }
    let db = RocksDbStore::open(&path)
        .map_err(|e| format!("Reopening storage demo database failed: {e}"))?;
    let recovered = db
        .get(Column::Blocks, block_key)
        .map_err(|e| format!("Reading sample block failed: {e}"))?;
    if recovered.as_deref() != Some(block_bytes.as_slice()) {
        return Err("Persisted sample block did not match written bytes".to_owned());
    }
    println!("RocksDB storage demo: PASS");
    println!("Sample bytes persisted and recovered after reopen");
    println!("Database: {}", path.display());
    println!("Scope: raw storage only; no validation, chain-tip update, or consensus");
    Ok(())
}

fn validated_storage_demo() -> Result<(), String> {
    // Experimental structural check, not full block consensus validation.
    // Never save this fixture into a production chain directory.
    let executable = env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            "Set AIVOBLOCK_HASKELL_EXECUTABLE to the compiled Haskell core binary.".to_owned()
        })?;
    let client = HaskellValidatorClient::new(executable)
        .map_err(|e| format!("Haskell core configuration failed: {e:?}"))?;
    let request = JSON_GENESIS_VALID.as_bytes();
    let response = client
        .exchange(request)
        .map_err(|e| format!("Haskell structural validation IPC failed: {e:?}"))?;
    let response = std::str::from_utf8(&response)
        .map_err(|_| "Haskell validation response was not UTF-8".to_owned())?;
    // The response is diagnostic JSON. Requiring all essential fields avoids
    // accepting a generic "ok" message as a structural verdict.
    for field in [
        r#""ok":true"#,
        r#""type":"validate_block_shape""#,
        r#""protocol_version":1"#,
        r#""valid":true"#,
        r#""reason":"none""#,
        r#""scope":"structural_only""#,
    ] {
        if !response.contains(field) {
            return Err(format!(
                "Haskell did not accept block structure: {response}"
            ));
        }
    }
    let path = env::var_os("AIVOBLOCK_VALIDATED_STORAGE_DEMO_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("validated-storage-demo-data")
        });
    // These fields correspond exactly to JSON_GENESIS_VALID above.
    // This is a candidate header ID, not an authenticated block or body.
    let header = CanonicalHeaderV1 {
        version: 1,
        chain_id: 1,
        height: 0,
        parent_hash: [0; 32],
        transactions_root: [0; 32],
        puzzle_proofs_root: [0; 32],
        state_root: [0; 32],
        timestamp_ms: 0,
    };
    let block_id = hash_header_v1(&header).map_err(str::to_owned)?;
    {
        let db =
            RocksDbStore::open(&path).map_err(|e| format!("Opening demo database failed: {e}"))?;
        db.put_validated_block_and_tip(&block_id, request)
            .map_err(|e| format!("Atomically storing demo block and tip failed: {e}"))?;
        db.flush()
            .map_err(|e| format!("Flushing demo database failed: {e}"))?;
    }
    let db =
        RocksDbStore::open(&path).map_err(|e| format!("Reopening demo database failed: {e}"))?;
    if db
        .get(Column::Blocks, &block_id)
        .map_err(|e| format!("Reading stored fixture failed: {e}"))?
        .as_deref()
        != Some(request)
    {
        return Err("Reopened block fixture differs from stored bytes".to_owned());
    }
    let recovered_tip = db
        .get(Column::ChainState, b"tip")
        .map_err(|e| format!("Reading stored chain tip failed: {e}"))?;
    if recovered_tip.as_deref() != Some(block_id.as_slice()) {
        return Err("Reopened chain tip does not match candidate header hash".to_owned());
    }
    println!("Haskell structural validation → atomic RocksDB demo: PASS");
    println!("Block bytes and chain-tip reference recovered after database reopen");
    println!("Database: {}", path.display());
    println!(
        "Canonical header SHA-256 key: {}",
        block_id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    println!("Demo tip is stored, not consensus-validated; block roots, signatures and consensus remain unverified");
    Ok(())
}

#[derive(Clone, Copy)]
struct CanonicalHeaderV1 {
    version: u32,
    chain_id: u32,
    height: u64,
    parent_hash: [u8; 32],
    transactions_root: [u8; 32],
    puzzle_proofs_root: [u8; 32],
    state_root: [u8; 32],
    timestamp_ms: u64,
}

fn encode_header_v1(header: &CanonicalHeaderV1) -> Result<[u8; 178], &'static str> {
    if header.version != 1 {
        return Err("Unsupported V1 header version");
    }
    if header.chain_id == 0 {
        return Err("Chain ID must not be zero");
    }
    let mut bytes = [0u8; 178];
    let mut offset = 0;
    fn append<const N: usize>(target: &mut [u8; 178], offset: &mut usize, value: &[u8; N]) {
        target[*offset..*offset + N].copy_from_slice(value);
        *offset += N;
    }
    append(&mut bytes, &mut offset, b"AIVOBLOCK-BLOCK-HEADER-V1\0");
    append(&mut bytes, &mut offset, &header.version.to_be_bytes());
    append(&mut bytes, &mut offset, &header.chain_id.to_be_bytes());
    append(&mut bytes, &mut offset, &header.height.to_be_bytes());
    append(&mut bytes, &mut offset, &header.parent_hash);
    append(&mut bytes, &mut offset, &header.transactions_root);
    append(&mut bytes, &mut offset, &header.puzzle_proofs_root);
    append(&mut bytes, &mut offset, &header.state_root);
    append(&mut bytes, &mut offset, &header.timestamp_ms.to_be_bytes());
    debug_assert_eq!(offset, bytes.len());
    Ok(bytes)
}

fn hash_header_v1(header: &CanonicalHeaderV1) -> Result<[u8; 32], &'static str> {
    let bytes = encode_header_v1(header)?;
    Ok(Sha256::digest(bytes).into())
}

fn header_hash_demo() -> Result<(), String> {
    let header = CanonicalHeaderV1 {
        version: 1,
        chain_id: 1,
        height: 0,
        parent_hash: [0; 32],
        transactions_root: [0; 32],
        puzzle_proofs_root: [0; 32],
        state_root: [0; 32],
        timestamp_ms: 1000,
    };
    let digest = hash_header_v1(&header).map_err(str::to_owned)?;
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    const EXPECTED: &str = "c1b8e653731eeafeacb557e46669d277096ecc7ceff7ec911ab29b25a8a55c22";
    if hex != EXPECTED {
        return Err(format!("Canonical header SHA-256 mismatch: {hex}"));
    }
    println!("Canonical Rust header hash: PASS");
    println!("Preimage: 178 bytes");
    println!("SHA-256: {hex}");
    println!("Scope: header ID only; body roots and consensus are not verified");
    Ok(())
}

fn two_block_demo() -> Result<(), String> {
    // Separate demonstration database; never use this as a consensus ledger.
    let path = env::var_os("AIVOBLOCK_TWO_BLOCK_DEMO_PATH")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("two-block-demo-data"));
    let genesis = CanonicalHeaderV1 {
        version: 1,
        chain_id: 1,
        height: 0,
        parent_hash: [0; 32],
        transactions_root: [0; 32],
        puzzle_proofs_root: [0; 32],
        state_root: [0; 32],
        timestamp_ms: 1000,
    };
    let genesis_id = hash_header_v1(&genesis).map_err(str::to_owned)?;
    let child = CanonicalHeaderV1 {
        height: 1,
        parent_hash: genesis_id,
        timestamp_ms: 1001,
        ..genesis
    };
    let child_id = hash_header_v1(&child).map_err(str::to_owned)?;
    let genesis_bytes = encode_header_v1(&genesis).map_err(str::to_owned)?;
    let child_bytes = encode_header_v1(&child).map_err(str::to_owned)?;
    {
        let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
        // Require a fresh demo database to avoid overwriting existing state.
        if db
            .get(Column::ChainState, b"tip")
            .map_err(|e| e.to_string())?
            .is_some()
            || db
                .get(Column::ChainState, b"height")
                .map_err(|e| e.to_string())?
                .is_some()
        {
            return Err(format!("Demo database already contains a chain: {}. Set AIVOBLOCK_TWO_BLOCK_DEMO_PATH to a new directory.", path.display()));
        }
        db.append_checked_block(&genesis_id, &[0; 32], 0, &genesis_bytes)
            .map_err(|e| format!("Genesis append failed: {e}"))?;
        // Invalid parent must be rejected without changing the tip.
        if db
            .append_checked_block(&child_id, &[9; 32], 1, &child_bytes)
            .is_ok()
        {
            return Err("Incorrect parent was unexpectedly accepted".to_owned());
        }
        if db
            .append_checked_block(&child_id, &genesis_id, 2, &child_bytes)
            .is_ok()
        {
            return Err("Incorrect height was unexpectedly accepted".to_owned());
        }
        db.append_checked_block(&child_id, &genesis_id, 1, &child_bytes)
            .map_err(|e| format!("Child append failed: {e}"))?;
    }
    let db = RocksDbStore::open(&path).map_err(|e| e.to_string())?;
    if db
        .get(Column::ChainState, b"tip")
        .map_err(|e| e.to_string())?
        .as_deref()
        != Some(child_id.as_slice())
        || db
            .get(Column::ChainState, b"height")
            .map_err(|e| e.to_string())?
            .as_deref()
            != Some(1u64.to_be_bytes().as_slice())
        || db
            .get(Column::Blocks, &genesis_id)
            .map_err(|e| e.to_string())?
            .as_deref()
            != Some(genesis_bytes.as_slice())
        || db
            .get(Column::Blocks, &child_id)
            .map_err(|e| e.to_string())?
            .as_deref()
            != Some(child_bytes.as_slice())
    {
        return Err("Two-block chain did not survive database reopening".to_owned());
    }
    println!("Two-block RocksDB continuity demo: PASS");
    println!("Genesis height: 0; child height: 1");
    println!("Invalid parent and height rejected");
    println!("Both headers, tip and height persisted after reopen");
    println!("Database: {}", path.display());
    println!("Scope: local header continuity only; no Haskell consensus verification");
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
        [command] if command == "ai-partition-demo" => ai_pipeline::demo(),
        [command] if command == "ai-reward-block-demo" => reward_pipeline::demo(),
        [command] if command == "ai-proof-v2-demo" => proof_pipeline_v2::demo(),
        [command] if command == "validator-quorum-demo" => validator_certificate::demo(),
        [command] if command == "ledger-transition-demo" => ledger_bridge::demo(),
        [command] if command == "multi-transfer-demo" => batch_pipeline::demo(),
        [command] if command == "storage-demo" => storage_demo(),
        [command] if command == "validated-storage-demo" => validated_storage_demo(),
        [command] if command == "header-hash" => header_hash_demo(),
        [command] if command == "two-block-demo" => two_block_demo(),
        [command, rest @ ..] if command == "run" => testnet::cli::run_command(rest),
        [command, rest @ ..] if command == "testnet-init" => testnet::cli::init_command(rest),
        [command, rest @ ..] if command == "client" => testnet::cli::client_command(rest),
        [command, rest @ ..] if command == "bench" => testnet::bench::bench_command(rest),
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
        assert!(run(&["ai-partition-demo".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["ai-reward-block-demo".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["ai-proof-v2-demo".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["validator-quorum-demo".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["ledger-transition-demo".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["storage-demo".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["validated-storage-demo".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["header-hash".to_owned(), "extra".to_owned()]).is_err());
        assert!(run(&["two-block-demo".to_owned(), "extra".to_owned()]).is_err());
    }

    // Cross-language tests run when the compiled Haskell executable is supplied.
    // Set AIVOBLOCK_HASKELL_EXECUTABLE before cargo test. Without it the
    // test is skipped, rather than depending on a Cabal installation in CI.
    #[test]
    fn haskell_json_ipc_validation_cases() {
        use super::{CORE_HEALTH_REQUEST, JSON_GENESIS_INVALID, JSON_GENESIS_VALID};
        use crate::ipc::haskell::HaskellValidatorClient;
        use std::env;

        let Some(executable) =
            env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE").filter(|v| !v.is_empty())
        else {
            eprintln!("SKIP: set AIVOBLOCK_HASKELL_EXECUTABLE for Haskell IPC integration");
            return;
        };
        let client = HaskellValidatorClient::new(executable).expect("valid core executable path");
        for (request, required) in [
            (
                CORE_HEALTH_REQUEST,
                vec![r#""ok":true"#, r#""core":"aivoblock-core""#],
            ),
            (
                JSON_GENESIS_VALID.as_bytes(),
                vec![r#""valid":true"#, r#""scope":"structural_only""#],
            ),
            (
                JSON_GENESIS_INVALID.as_bytes(),
                vec![r#""valid":false"#, r#""reason":"InvalidGenesisParent""#],
            ),
        ] {
            let response = client
                .exchange(request)
                .expect("Haskell framed IPC exchange");
            let response = std::str::from_utf8(&response).expect("UTF-8 JSON response");
            for field in required {
                assert!(response.contains(field), "missing {field} in {response}");
            }
        }
        // Valid JSON, but the required block is missing: the core must not approve it.
        let malformed_block = br#"{"protocol_version":1,"type":"validate_block_shape","block":{}}"#;
        let response = client
            .exchange(malformed_block)
            .expect("malformed block response");
        let response = std::str::from_utf8(&response).expect("UTF-8 error response");
        assert!(response.contains(r#""ok":false"#), "{response}");
        assert!(
            response.contains(r#""error":"invalid_block""#),
            "{response}"
        );
    }

    // Execute malformed raw frames against the real Haskell binary.
    // These tests are opt-in because CI may not have Cabal/GHC installed.
    #[test]
    fn haskell_rejects_malformed_ipc_frames() {
        use std::env;
        use std::io::Write;
        use std::process::{Command, Stdio};

        let Some(executable) =
            env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE").filter(|value| !value.is_empty())
        else {
            eprintln!("SKIP: set AIVOBLOCK_HASKELL_EXECUTABLE for malformed IPC tests");
            return;
        };

        let oversized = ((4 * 1024 * 1024 + 1) as u32).to_be_bytes().to_vec();
        let cases: [(&str, Vec<u8>); 6] = [
            ("empty input", vec![]),
            ("short header", vec![0, 0, 0]),
            ("zero-length frame", vec![0, 0, 0, 0]),
            ("oversized header", oversized),
            ("truncated body", vec![0, 0, 0, 5, b'{', b'}']),
            ("trailing byte", {
                let mut frame = (2u32).to_be_bytes().to_vec();
                frame.extend_from_slice(b"{}!");
                frame
            }),
        ];

        for (name, frame) in cases {
            let mut child = Command::new(&executable)
                .arg("ipc-health")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("launch Haskell core");
            child
                .stdin
                .take()
                .expect("child stdin")
                .write_all(&frame)
                .expect("write raw frame");
            let output = child.wait_with_output().expect("wait for Haskell core");
            assert!(
                !output.status.success(),
                "{name}: malformed frame unexpectedly succeeded"
            );
            assert!(
                output.stdout.is_empty(),
                "{name}: malformed frame returned output"
            );
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("IPC error"),
                "{name}: missing diagnostic: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn canonical_header_matches_published_sha256_vector() {
        use super::{encode_header_v1, hash_header_v1, CanonicalHeaderV1};
        let mut header = CanonicalHeaderV1 {
            version: 1,
            chain_id: 1,
            height: 0,
            parent_hash: [0; 32],
            transactions_root: [0; 32],
            puzzle_proofs_root: [0; 32],
            state_root: [0; 32],
            timestamp_ms: 1000,
        };
        assert_eq!(encode_header_v1(&header).unwrap().len(), 178);
        let result = hash_header_v1(&header).unwrap();
        let hex = result
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(
            hex,
            "c1b8e653731eeafeacb557e46669d277096ecc7ceff7ec911ab29b25a8a55c22"
        );
        header.timestamp_ms += 1;
        assert_ne!(hash_header_v1(&header).unwrap(), result);
        header.version = 2;
        assert!(hash_header_v1(&header).is_err());
    }

    #[test]
    fn default_worker_location_is_under_ai_engine() {
        assert!(default_worker_path().ends_with("ai-engine/src/aivoblock_ai/worker.py"));
    }
}
