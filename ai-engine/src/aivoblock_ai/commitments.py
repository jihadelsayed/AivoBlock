"""Versioned, deterministic commitments for integer-inference puzzle inputs.

This is a local protocol v1 encoding, not yet a consensus-approved encoding.
All rows, including those outside a worker's assigned range, are committed.
"""

from __future__ import annotations

from hashlib import sha256
import struct
from typing import Sequence

from .models import IntegerLinearModel

MAX_ROWS = 1024
MAX_FEATURES = 4096
MAX_ABS_VALUE = 1_000_000
RULES_V1 = b"AIVOBLOCK-INTEGER-INFERENCE-RULES-V1\x00"


def input_commitment(rows: Sequence[Sequence[int]]) -> bytes:
    if not isinstance(rows, (list, tuple)) or not 1 <= len(rows) <= MAX_ROWS:
        raise ValueError("Input rows must be a nonempty bounded sequence")
    width = len(rows[0])
    if not 1 <= width <= MAX_FEATURES:
        raise ValueError("Invalid feature count")
    payload = bytearray(b"AIVOBLOCK-INFERENCE-INPUT-V1\x00")
    payload.extend(struct.pack(">II", len(rows), width))
    for row in rows:
        if not isinstance(row, (list, tuple)) or len(row) != width:
            raise ValueError("Rows must have identical dimensions")
        for value in row:
            if type(value) is not int or abs(value) > MAX_ABS_VALUE:
                raise ValueError("Input must contain bounded integers")
            payload.extend(struct.pack(">q", value))
    return sha256(payload).digest()


def model_commitment(weights: Sequence[int]) -> bytes:
    if not isinstance(weights, (list, tuple)):
        raise ValueError("Weights must be a sequence")
    return IntegerLinearModel(tuple(weights)).commitment()


def rules_commitment() -> bytes:
    return sha256(RULES_V1).digest()


def validate_commitments(
    model_hash: bytes, input_hash: bytes, rules_hash: bytes,
    rows: Sequence[Sequence[int]], weights: Sequence[int],
) -> None:
    if model_commitment(weights) != model_hash:
        raise ValueError("Model commitment mismatch")
    if input_commitment(rows) != input_hash:
        raise ValueError("Input commitment mismatch")
    if rules_commitment() != rules_hash:
        raise ValueError("Rules commitment mismatch")
