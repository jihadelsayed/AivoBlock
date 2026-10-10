"""Tests for AivoBlock puzzle specifications and deterministic partitioning."""

import pytest

from aivoblock_ai.puzzle import Puzzle, WorkRange, partition_puzzle


def make_puzzle(units: int = 10) -> Puzzle:
    return Puzzle(
        puzzle_id=b"\x01" * 32,
        version=1,
        model_commitment=b"\x02" * 32,
        input_commitment=b"\x03" * 32,
        rules_commitment=b"\x04" * 32,
        units=units,
        deadline_height=100,
    )


def test_valid_puzzle() -> None:
    puzzle = make_puzzle()
    assert puzzle.units == 10
    assert puzzle.version == 1


@pytest.mark.parametrize("units", [0, -1, 2**64, True])
def test_invalid_puzzle_units(units: int) -> None:
    with pytest.raises(ValueError):
        make_puzzle(units)


def test_invalid_hash_length() -> None:
    with pytest.raises(ValueError, match="puzzle_id"):
        Puzzle(
            puzzle_id=b"short",
            version=1,
            model_commitment=b"\x02" * 32,
            input_commitment=b"\x03" * 32,
            rules_commitment=b"\x04" * 32,
            units=10,
            deadline_height=100,
        )


@pytest.mark.parametrize("version", [0, -1, 2**32, True])
def test_invalid_version(version: int) -> None:
    with pytest.raises(ValueError):
        Puzzle(
            puzzle_id=b"\x01" * 32,
            version=version,
            model_commitment=b"\x02" * 32,
            input_commitment=b"\x03" * 32,
            rules_commitment=b"\x04" * 32,
            units=10,
            deadline_height=100,
        )


def test_partition_covers_puzzle_without_gaps_or_overlaps() -> None:
    puzzle = make_puzzle(10)
    ranges = partition_puzzle(puzzle, batch_size=3)
    assert ranges == (
        WorkRange(0, 3),
        WorkRange(3, 3),
        WorkRange(6, 3),
        WorkRange(9, 1),
    )
    covered = [unit for part in ranges for unit in range(part.start, part.start + part.count)]
    assert covered == list(range(puzzle.units))


@pytest.mark.parametrize("batch_size", [0, -1, 2**64, True])
def test_invalid_batch_size(batch_size: int) -> None:
    with pytest.raises(ValueError):
        partition_puzzle(make_puzzle(), batch_size)


def test_range_at_exact_boundary() -> None:
    WorkRange(8, 2).validate_for(make_puzzle(10))


@pytest.mark.parametrize(
    ("start", "count"),
    [(10, 1), (9, 2), (0, 11)],
)
def test_out_of_bounds_range(start: int, count: int) -> None:
    with pytest.raises(ValueError):
        WorkRange(start, count).validate_for(make_puzzle(10))


def test_zero_length_range_is_invalid() -> None:
    with pytest.raises(ValueError):
        WorkRange(0, 0)


@pytest.mark.parametrize("units,batch_size", [(1, 1), (7, 2), (32, 10), (100, 100)])
def test_partition_invariants(units: int, batch_size: int) -> None:
    parts = partition_puzzle(make_puzzle(units), batch_size)
    assert sum(part.count for part in parts) == units
    assert all(0 < part.count <= batch_size for part in parts)
    assert all(
        first.start + first.count == second.start
        for first, second in zip(parts, parts[1:])
    )
