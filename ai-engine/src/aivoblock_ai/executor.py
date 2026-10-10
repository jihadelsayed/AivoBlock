"""Bounded deterministic AI-work prototype executor.

The first workload evaluates integer dot products for assigned rows of a
fixed input matrix. It demonstrates divisible useful computation without
requiring an external AI model or nondeterministic floating-point kernels.
This is NOT proof of useful work or blockchain consensus verification.
"""

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
import struct
from typing import Sequence

from .puzzle import WorkRange

MAX_ROWS_PER_EXECUTION = 1024
MAX_FEATURES = 4096
MAX_ABSOLUTE_INPUT = 1_000_000


@dataclass(frozen=True, slots=True)
class ExecutionResult:
    start: int
    count: int
    outputs: tuple[int, ...]
    result_commitment: bytes


def _validate_vector(vector: Sequence[int], label: str) -> None:
    if not 1 <= len(vector) <= MAX_FEATURES:
        raise ValueError(f"{label} has invalid feature count")
    for value in vector:
        if type(value) is not int or abs(value) > MAX_ABSOLUTE_INPUT:
            raise ValueError(f"{label} contains an invalid integer")


def execute_integer_inference(
    rows: Sequence[Sequence[int]],
    weights: Sequence[int],
    work_range: WorkRange,
) -> ExecutionResult:
    """Evaluate integer linear inference on an assigned contiguous range.

    Outputs and commitments are reproducible across Python installations.
    The commitment encoding is provisional and must be included in the
    consensus specification before being used in accepted blocks.
    """
    _validate_vector(weights, "weights")
    if work_range.count > MAX_ROWS_PER_EXECUTION:
        raise ValueError("Work range exceeds execution limit")
    if work_range.start >= len(rows) or work_range.count > len(rows) - work_range.start:
        raise ValueError("Work range exceeds input rows")

    selected = rows[work_range.start:work_range.start + work_range.count]
    outputs = []
    for row in selected:
        if len(row) != len(weights):
            raise ValueError("Input row and weight dimensions differ")
        _validate_vector(row, "input row")
        outputs.append(sum(x * w for x, w in zip(row, weights)))

    # Versioned, unambiguous byte encoding for this local prototype.
    commitment_input = bytearray(b"AIVOBLOCK-INFERENCE-V1\x00")
    commitment_input.extend(struct.pack(">QQ", work_range.start, work_range.count))
    for result in outputs:
        encoded = str(result).encode("ascii")
        commitment_input.extend(struct.pack(">I", len(encoded)))
        commitment_input.extend(encoded)

    return ExecutionResult(
        start=work_range.start,
        count=work_range.count,
        outputs=tuple(outputs),
        result_commitment=sha256(commitment_input).digest(),
    )
