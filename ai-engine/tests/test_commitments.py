"""Regression tests for immutable puzzle model, input and rule bindings."""

import pytest

from aivoblock_ai.commitments import (
    input_commitment, model_commitment, rules_commitment,
)
from aivoblock_ai.worker import handle_request

ROWS = [[1, 2], [3, 4], [-2, 5]]
WEIGHTS = [2, 3]


def request(kind="execute_integer_inference"):
    puzzle = {
        "puzzle_id": "01" * 32,
        "version": 1,
        "model_commitment": model_commitment(WEIGHTS).hex(),
        "input_commitment": input_commitment(ROWS).hex(),
        "rules_commitment": rules_commitment().hex(),
        "units": 3,
        "deadline_height": 100,
    }
    return {
        "protocol_version": 1,
        "type": kind,
        "worker_id": "05" * 32,
        "puzzle": puzzle,
        "start": 0,
        "count": 3,
        "rows": [row[:] for row in ROWS],
        "weights": WEIGHTS[:],
    }


def test_valid_committed_execution_and_verification():
    execution = handle_request(request())
    assert execution["ok"] is True
    assert execution["outputs"] == [8, 18, 11]
    verification = request("verify_integer_inference")
    verification["proof"] = execution["proof"]
    assert handle_request(verification)["verified"] is True


@pytest.mark.parametrize("mutation", ["weights", "rows", "model_hash", "input_hash", "rules_hash"])
def test_mutated_puzzle_is_rejected(mutation):
    req = request()
    if mutation == "weights":
        req["weights"][0] += 1
    elif mutation == "rows":
        req["rows"][2][0] += 1
    elif mutation == "model_hash":
        req["puzzle"]["model_commitment"] = "00" * 32
    elif mutation == "input_hash":
        req["puzzle"]["input_commitment"] = "00" * 32
    else:
        req["puzzle"]["rules_commitment"] = "00" * 32
    assert handle_request(req) == {"ok": False, "error": "invalid_puzzle_request"}


def test_verifier_rejects_changed_trusted_inputs():
    execution = handle_request(request())
    req = request("verify_integer_inference")
    req["proof"] = execution["proof"]
    req["rows"][0][0] += 1
    assert handle_request(req) == {"ok": False, "error": "invalid_verification_request"}


@pytest.mark.parametrize("bad", [[[1, 2], [3]], [[True, 2]], [[1_000_001, 0]], []])
def test_bad_input_encodings(bad):
    with pytest.raises(ValueError):
        input_commitment(bad)


def test_commitments_are_deterministic_and_domain_separated():
    assert input_commitment(ROWS) == input_commitment([row[:] for row in ROWS])
    assert model_commitment(WEIGHTS) != input_commitment(ROWS)
    assert len(rules_commitment()) == 32
