"""Tests for trained export -> standalone service fixture conversion."""
import copy
import pytest

from aivoblock_ai.qmlp import FIXTURE_ROWS, execute_partition, fixture_model
from aivoblock_ai.service_fixture import build_fixture
from aivoblock_ai.service_benchmark import measure


def source():
    model = fixture_model()
    logits, result = execute_partition(model, FIXTURE_ROWS, 0, len(FIXTURE_ROWS))
    return {"workload": "qmlp-v1", "model": model.to_json(),
            "rows": [list(r) for r in FIXTURE_ROWS],
            "model_commitment": model.commitment().hex(),
            "logits": [list(row) for row in logits],
            "result_commitment": result.hex()}


def test_round_trip_into_benchmark():
    fixture = build_fixture(source())
    report = measure(fixture, iterations=3, verify=True)
    assert fixture["result_commitment"] == report["result_commitment"]
    assert fixture["model_commitment"] == report["model_commitment"]
    assert fixture["input_commitment"] == report["input_commitment"]


@pytest.mark.parametrize("field", ["model_commitment", "result_commitment", "logits"])
def test_tampering_rejected(field):
    exported = copy.deepcopy(source())
    exported[field] = [] if field == "logits" else "00" * 32
    with pytest.raises(ValueError, match="mismatch"):
        build_fixture(exported)


def test_reject_wrong_workload():
    exported = source()
    exported["workload"] = "linear"
    with pytest.raises(ValueError, match="qmlp-v1"):
        build_fixture(exported)
