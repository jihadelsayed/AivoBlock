//! AI worker execution for one assigned task partition.
//!
//! The production backend runs the repository's Python worker
//! (`ai-engine/.../worker.py execute_integer_inference`). The worker node
//! refuses to sign if Python's commitment differs from the Rust reference
//! recomputation, so a broken executor cannot make an honest worker sign a
//! wrong result. Validators never use this module: they recompute in Rust.

use serde_json::{json, Value};

use super::{hex, unhex_array};
use crate::ai_block_v3::{expected_result_v3, AiTaskV3};
use crate::ipc::python::PythonWorkerClient;

#[derive(Debug, Clone)]
pub enum WorkerBackend {
    /// Python executor via framed IPC, cross-checked against Rust.
    Python(PythonWorkerClient),
    /// Rust reference only (unit tests without Python).
    RustReference,
}

impl WorkerBackend {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Python(_) => "python",
            Self::RustReference => "rust-reference",
        }
    }

    pub fn execute(
        &self,
        task: &AiTaskV3,
        partition: u32,
        worker_id: &[u8; 32],
    ) -> Result<[u8; 32], String> {
        let reference = expected_result_v3(task, partition)?;
        let client = match self {
            Self::RustReference => return Ok(reference),
            Self::Python(client) => client,
        };
        let entry = task
            .partitions
            .get(partition as usize)
            .ok_or("Nonexistent partition")?;
        let request = json!({
            "protocol_version": 1,
            "type": "execute_integer_inference",
            "worker_id": hex(worker_id),
            "puzzle": {
                "puzzle_id": hex(&task.task_id),
                "version": 1,
                "model_commitment": hex(&task.model_commitment),
                "input_commitment": hex(&task.input_commitment),
                "rules_commitment": hex(&task.rules_commitment),
                "units": task.rows.len(),
                "deadline_height": task.deadline_height,
            },
            "start": entry.start,
            "count": entry.count,
            "rows": task.rows,
            "weights": task.weights,
        });
        let bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        let response = client
            .exchange(&bytes)
            .map_err(|e| format!("Python worker IPC: {e:?}"))?;
        let value: Value =
            serde_json::from_slice(&response).map_err(|e| format!("Python reply: {e}"))?;
        if value["ok"] != json!(true) || value["type"] != json!("execute_integer_inference") {
            return Err(format!("Python worker refused the partition: {value}"));
        }
        let proof = &value["proof"];
        if proof["start_unit"] != json!(entry.start) || proof["unit_count"] != json!(entry.count) {
            return Err("Python worker answered a different range".into());
        }
        let commitment: [u8; 32] = unhex_array(
            proof["result_commitment"]
                .as_str()
                .ok_or("Missing Python result commitment")?,
        )?;
        if commitment != reference {
            return Err("Python result differs from the Rust reference; refusing to sign".into());
        }
        Ok(commitment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_block_v3::tests::fixture_task;

    #[test]
    fn rust_reference_matches_independent_recomputation() {
        let task = fixture_task(1);
        for partition in 0..3 {
            assert_eq!(
                WorkerBackend::RustReference
                    .execute(&task, partition, &[1; 32])
                    .unwrap(),
                expected_result_v3(&task, partition).unwrap()
            );
        }
        assert!(WorkerBackend::RustReference
            .execute(&task, 7, &[1; 32])
            .is_err());
    }

    /// Runs the real Python worker when it is installed (CI rust job).
    #[test]
    fn python_worker_agrees_with_rust_when_available() {
        let python = if cfg!(windows) { "python" } else { "python3" };
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../ai-engine/src/aivoblock_ai/worker.py");
        let client = PythonWorkerClient::new(python, &script).unwrap();
        let health = client.exchange(br#"{"protocol_version":1,"type":"health"}"#);
        if health.is_err() {
            eprintln!("SKIP: Python AI worker is not installed");
            return;
        }
        let task = fixture_task(1);
        let backend = WorkerBackend::Python(client);
        for partition in 0..3 {
            let worker = task.partitions[partition as usize].worker;
            assert_eq!(
                backend.execute(&task, partition, &worker).unwrap(),
                expected_result_v3(&task, partition).unwrap()
            );
        }
    }
}
