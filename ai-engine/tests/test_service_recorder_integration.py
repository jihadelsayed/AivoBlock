"""End-to-end evidence bridge: measured QMLP service -> artifact recorder.

These are real in-process CPU service calls, not chain finality or matched
service/chain performance measurements.
"""
import hashlib
import json
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
from record_benchmark import record  # noqa: E402

from aivoblock_ai.qmlp import FIXTURE_ROWS, execute_partition, fixture_model, qmlp_input_commitment
from aivoblock_ai.service_benchmark import measure


def fixture():
    model = fixture_model()
    _, result_digest = execute_partition(model, FIXTURE_ROWS, 0, len(FIXTURE_ROWS))
    return {
        "model": model.to_json(),
        "rows": [list(row) for row in FIXTURE_ROWS],
        "model_commitment": model.commitment().hex(),
        "input_commitment": qmlp_input_commitment(model, FIXTURE_ROWS).hex(),
        "result_commitment": result_digest.hex(),
    }


@pytest.mark.parametrize("verify,mode", [(False, "service_direct"), (True, "service_recomputed")])
def test_measured_service_artifacts_round_trip_into_recorder(tmp_path, verify, mode):
    report = measure(
        fixture(), iterations=3, verify=verify, launch_id=f"{mode}-launch",
        artifacts_dir=tmp_path / mode,
    )
    # Pass JSONL-serialized records, as the actual recorder CLI consumes them.
    observations = [json.loads(line) for line in
                    "".join(json.dumps(row) + "\n" for row in report["request_observations"]).splitlines()]
    recorded = record(observations, mode)
    assert len(recorded["samples"]) == 3
    assert len({row["request_id"] for row in recorded["samples"]}) == 3
    for field in ("model", "input", "result"):
        digest = report["artifact_sha256"][f"artifact_{field}_sha256"]
        assert recorded[f"{field}_commitment"] == digest
        assert recorded[f"canonical_{field}_commitment"] == report[f"{field}_commitment"]
        assert digest == hashlib.sha256(
            Path(report["artifact_paths"][f"{field}_path"]).read_bytes()
        ).hexdigest()
        assert all(row[f"{field}_commitment"] == digest for row in recorded["samples"])
    for i, sample in enumerate(recorded["samples"]):
        assert sample["elapsed_ms"] == pytest.approx(
            report["latency_ms"]["observations"][i], abs=1e-9
        )
        assert sample["canonical_result_commitment"] == report["result_commitment"]
    assert all("finalized_height" not in row for row in recorded["samples"])


def test_mutated_measured_artifact_is_detected_by_digest_reconciliation(tmp_path):
    report = measure(fixture(), iterations=3, launch_id="tamper-check",
                     artifacts_dir=tmp_path / "artifacts")
    path = Path(report["artifact_paths"]["result_path"])
    expected_digest = report["artifact_sha256"]["artifact_result_sha256"]
    path.write_bytes(path.read_bytes() + b"\n")
    recorded = record(report["request_observations"], "service_direct")
    assert recorded["result_commitment"] != expected_digest
    # SHA-256 is computed from present file bytes; no on-chain attestation claimed.


def test_service_observations_cannot_be_misrepresented_as_finality(tmp_path):
    report = measure(fixture(), iterations=3, launch_id="not-chain",
                     artifacts_dir=tmp_path / "artifacts")
    with pytest.raises(ValueError, match="finalized_height"):
        record(report["request_observations"], "chain_observed_finality")
