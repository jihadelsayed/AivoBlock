"""Tests for deterministic AivoBlock AI work execution and proof checks."""

import pytest

from aivoblock_ai.executor import execute_integer_inference
from aivoblock_ai.models import IntegerLinearModel
from aivoblock_ai.proof import proof_from_result, verify_by_recomputation
from aivoblock_ai.puzzle import Puzzle, WorkRange


def puzzle(units: int = 3) -> Puzzle:
    return Puzzle(
        puzzle_id=b"\x01" * 32,
        version=1,
        model_commitment=b"\x02" * 32,
        input_commitment=b"\x03" * 32,
        rules_commitment=b"\x04" * 32,
        units=units,
        deadline_height=100,
    )


ROWS = ((1, 2), (3, 4), (-2, 5))
WEIGHTS = (2, 3)


def test_integer_inference_outputs() -> None:
    result = execute_integer_inference(ROWS, WEIGHTS, WorkRange(0, 3))
    assert result.outputs == (8, 18, 11)
    assert result.start == 0
    assert result.count == 3
    assert len(result.result_commitment) == 32


def test_partitioned_execution_matches_whole_execution() -> None:
    full = execute_integer_inference(ROWS, WEIGHTS, WorkRange(0, 3))
    first = execute_integer_inference(ROWS, WEIGHTS, WorkRange(0, 1))
    second = execute_integer_inference(ROWS, WEIGHTS, WorkRange(1, 2))
    assert first.outputs + second.outputs == full.outputs


def test_same_inputs_produce_same_commitment() -> None:
    first = execute_integer_inference(ROWS, WEIGHTS, WorkRange(0, 3))
    second = execute_integer_inference(ROWS, WEIGHTS, WorkRange(0, 3))
    assert first.result_commitment == second.result_commitment


def test_different_ranges_produce_different_commitments() -> None:
    first = execute_integer_inference(ROWS, WEIGHTS, WorkRange(0, 1))
    second = execute_integer_inference(ROWS, WEIGHTS, WorkRange(1, 1))
    assert first.result_commitment != second.result_commitment


def test_proof_verifies_against_trusted_inputs() -> None:
    task = puzzle()
    result = execute_integer_inference(ROWS, WEIGHTS, WorkRange(0, 3))
    proof = proof_from_result(task, b"\x05" * 32, result)
    assert verify_by_recomputation(task, proof, ROWS, WEIGHTS)


def test_tampered_result_commitment_fails() -> None:
    from dataclasses import replace

    task = puzzle()
    result = execute_integer_inference(ROWS, WEIGHTS, WorkRange(0, 3))
    proof = proof_from_result(task, b"\x05" * 32, result)
    altered = replace(proof, result_commitment=b"\x09" * 32)
    assert not verify_by_recomputation(task, altered, ROWS, WEIGHTS)


def test_modified_inputs_fail_proof_check() -> None:
    task = puzzle()
    result = execute_integer_inference(ROWS, WEIGHTS, WorkRange(0, 3))
    proof = proof_from_result(task, b"\x05" * 32, result)
    changed_rows = ((2, 2),) + ROWS[1:]
    assert not verify_by_recomputation(task, proof, changed_rows, WEIGHTS)


@pytest.mark.parametrize("rows,weights,work", [
    (ROWS, (1,), WorkRange(0, 1)),
    (ROWS, WEIGHTS, WorkRange(2, 2)),
    (ROWS, (True, 3), WorkRange(0, 1)),
    (ROWS, (2, 1_000_001), WorkRange(0, 1)),
])
def test_invalid_execution_inputs(rows, weights, work) -> None:
    with pytest.raises(ValueError):
        execute_integer_inference(rows, weights, work)


def test_integer_model_predicts_and_commits_deterministically() -> None:
    model = IntegerLinearModel(WEIGHTS)
    assert model.predict((3, 4)) == 18
    assert model.commitment() == IntegerLinearModel(WEIGHTS).commitment()
    assert len(model.commitment()) == 32


def test_integer_model_rejects_invalid_features() -> None:
    model = IntegerLinearModel(WEIGHTS)
    with pytest.raises(ValueError):
        model.predict((1,))
    with pytest.raises(ValueError):
        model.predict((True, 2))
