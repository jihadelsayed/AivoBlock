"""Deterministic puzzle descriptions and range checks for AivoBlock.

These are local worker-side checks, not consensus verification. Puzzle IDs
and commitments must eventually follow the canonical protocol specification.
"""

from __future__ import annotations

from dataclasses import dataclass

HASH_SIZE = 32
MAX_U64 = (1 << 64) - 1
MAX_U32 = (1 << 32) - 1


def _hash32(value: bytes, label: str) -> None:
    if not isinstance(value, bytes) or len(value) != HASH_SIZE:
        raise ValueError(f"{label} must be exactly 32 bytes")


def _unsigned(value: int, maximum: int, label: str) -> None:
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= maximum:
        raise ValueError(f"{label} must be an unsigned integer within range")


@dataclass(frozen=True, slots=True)
class Puzzle:
    puzzle_id: bytes
    version: int
    model_commitment: bytes
    input_commitment: bytes
    rules_commitment: bytes
    units: int
    deadline_height: int

    def __post_init__(self) -> None:
        for name in ("puzzle_id", "model_commitment", "input_commitment", "rules_commitment"):
            _hash32(getattr(self, name), name)
        _unsigned(self.version, MAX_U32, "version")
        _unsigned(self.units, MAX_U64, "units")
        _unsigned(self.deadline_height, MAX_U64, "deadline_height")
        if self.version == 0 or self.units == 0:
            raise ValueError("Puzzle version and units must be positive")


@dataclass(frozen=True, slots=True)
class WorkRange:
    start: int
    count: int

    def __post_init__(self) -> None:
        _unsigned(self.start, MAX_U64, "start")
        _unsigned(self.count, MAX_U64, "count")
        if self.count == 0:
            raise ValueError("Work range must cover at least one unit")

    def validate_for(self, puzzle: Puzzle) -> None:
        if self.start >= puzzle.units or self.count > puzzle.units - self.start:
            raise ValueError("Work range exceeds puzzle bounds")


def partition_puzzle(puzzle: Puzzle, batch_size: int) -> tuple[WorkRange, ...]:
    """Split a puzzle into contiguous non-overlapping chunks.

    For very large tasks prefer an iterator to avoid allocating every range.
    This helper is intended for bounded prototype tasks.
    """
    _unsigned(batch_size, MAX_U64, "batch_size")
    if batch_size == 0:
        raise ValueError("batch_size must be positive")
    return tuple(
        WorkRange(start, min(batch_size, puzzle.units - start))
        for start in range(0, puzzle.units, batch_size)
    )
