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
