"""Tests for the non-blockchain service microbenchmark."""
import pytest

from aivoblock_ai.qmlp import FIXTURE_ROWS, fixture_model, qmlp_input_commitment, execute_partition
from aivoblock_ai.service_benchmark import measure


def fixture():
    model = fixture_model()
    _, result = execute_partition(model, FIXTURE_ROWS, 0, len(FIXTURE_ROWS))
    return {
        "model": model.to_json(),
        "rows": [list(row) for row in FIXTURE_ROWS],
        "model_commitment": model.commitment().hex(),
        "input_commitment": qmlp_input_commitment(model, FIXTURE_ROWS).hex(),
        "result_commitment": result.hex(),
    }


def test_both_modes_preserve_identical_committed_results():
    trusted = measure(fixture(), iterations=3, batch=1)
    checked = measure(fixture(), iterations=3, batch=2, verify=True)
    assert trusted["result_commitment"] == checked["result_commitment"]
    assert trusted["input_commitment"] == checked["input_commitment"]
    assert checked["mode"] == "same_implementation_recomputation"
    assert len(checked["latency_ms"]["observations"]) == 3
    assert checked["rows_per_second_mean_latency"] > 0


@pytest.mark.parametrize("key", ["model_commitment", "input_commitment"])
def test_mismatched_fixture_rejected(key):
    data = fixture()
    data[key] = "00" * 32
    with pytest.raises(ValueError, match=key):
        measure(data, iterations=3)


@pytest.mark.parametrize("count", [0, 1, -1, True])
def test_invalid_iterations_rejected(count):
    with pytest.raises(ValueError, match="iterations"):
        measure(fixture(), iterations=count)


def test_mismatched_result_commitment_rejected():
    data = fixture()
    data["result_commitment"] = "00" * 32
    with pytest.raises(ValueError, match="result_commitment"):
        measure(data, iterations=3)


def test_request_observations_use_measured_boundaries_and_computed_commitments():
    report = measure(fixture(), iterations=3, launch_id="process-one")
    rows = report["request_observations"]
    assert len(rows) == 3
    assert len({row["request_id"] for row in rows}) == 3
    for i, row in enumerate(rows):
        assert row["launch_id"] == "process-one"
        assert row["end_ns"] > row["start_ns"]
        assert row["boundary"] == "synchronous_in_process_service_execute"
        assert row["status"] == "success"
        assert row["model_commitment"] == report["model_commitment"]
        assert row["input_commitment"] == report["input_commitment"]
        assert row["result_commitment"] == report["result_commitment"]
        assert abs((row["end_ns"] - row["start_ns"]) / 1e6 - report["latency_ms"]["observations"][i]) < 1e-9


def test_instrumentation_remains_opt_in():
    assert "request_observations" not in measure(fixture(), iterations=3)


def test_invalid_launch_id_rejected():
    with pytest.raises(ValueError, match="launch_id"):
        measure(fixture(), iterations=3, launch_id=" ")
