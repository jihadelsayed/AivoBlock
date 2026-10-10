"""AI puzzle result commitments and local recomputation checks.

This is a prototype verification helper, NOT a succinct cryptographic
proof of useful work. A verifier must have trusted task inputs and repeat
the assigned computation. Consensus rules are not implemented here.
"""

from __future__ import annotations

from dataclasses import dataclass
from hmac import compare_digest
from typing import Sequence

from .executor import ExecutionResult, execute_integer_inference
from .puzzle import Puzzle, WorkRange

HASH_BYTES = 32


@dataclass(frozen=True, slots=True)
class PuzzleProof:
    puzzle_id: bytes
    worker_id: bytes
    start_unit: int
    unit_count: int
    result_commitment: bytes
    witness_commitment: bytes | None = None

    def __post_init__(self) -> None:
        for label, value in (
            ("puzzle_id", self.puzzle_id),
            ("worker_id", self.worker_id),
            ("result_commitment", self.result_commitment),
        ):
            if not isinstance(value, bytes) or len(value) != HASH_BYTES:
                raise ValueError(f"{label} must be exactly 32 bytes")
        if self.worker_id == bytes(HASH_BYTES):
            raise ValueError("Worker identity must not be zero")
        if self.witness_commitment is not None and (
            not isinstance(self.witness_commitment, bytes)
            or len(self.witness_commitment) != HASH_BYTES
        ):
            raise ValueError("witness_commitment must be 32 bytes")
        WorkRange(self.start_unit, self.unit_count)


def proof_from_result(
    puzzle: Puzzle, worker_id: bytes, result: ExecutionResult
) -> PuzzleProof:
    """Wrap a worker's result commitment without asserting its validity."""
    work = WorkRange(result.start, result.count)
    work.validate_for(puzzle)
    return PuzzleProof(
        puzzle_id=puzzle.puzzle_id,
        worker_id=worker_id,
        start_unit=work.start,
        unit_count=work.count,
        result_commitment=result.result_commitment,
    )


def verify_by_recomputation(
    puzzle: Puzzle,
    proof: PuzzleProof,
    rows: Sequence[Sequence[int]],
    weights: Sequence[int],
) -> bool:
    """Recompute the claimed range using independently trusted inputs.

    Returns False on an incorrect commitment or mismatched puzzle ID.
    Invalid ranges raise ValueError. This does not authenticate the worker,
    check the model/input commitments, or prove that work was performed.
    """
    if proof.puzzle_id != puzzle.puzzle_id:
        return False
    work = WorkRange(proof.start_unit, proof.unit_count)
    work.validate_for(puzzle)
    if len(rows) != puzzle.units:
        raise ValueError("Input row count differs from puzzle units")
    expected = execute_integer_inference(rows, weights, work)
    return compare_digest(expected.result_commitment, proof.result_commitment)
