"""QMLP-V1: integer-only quantized two-layer MLP inference.

This is the consensus-safe AI workload specified in
``protocol/spec/quantized-mlp-v1.md``. Every operation is exact integer
arithmetic with explicit rounding and clamping, so any honest worker or
validator computes byte-identical results. Floating-point models (PyTorch,
ONNX) are only ever *quantized into* this format off-chain; they are never
executed in consensus.
"""

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
import struct
from typing import Sequence

from .commitments import input_commitment

MODEL_DOMAIN = b"AIVOBLOCK-QMLP-MODEL-V1\x00"
RULES_DOMAIN = b"AIVOBLOCK-QMLP-RULES-V1\x00"
RESULT_DOMAIN = b"AIVOBLOCK-QMLP-RESULT-V1\x00"

MAX_INPUT = 32
MAX_HIDDEN = 64
MAX_OUTPUT = 16
MAX_ROWS = 256
INT8_LIMIT = 127
BIAS_LIMIT = 1 << 24
MAX_MULTIPLIER = (1 << 31) - 1
MAX_SHIFT = 40


def _int(value: object, name: str) -> int:
    if type(value) is not int:
        raise ValueError(f"{name} must be an integer")
    return value


def _bounded(values: Sequence[object], count: int, limit: int, name: str) -> tuple[int, ...]:
    if not isinstance(values, (list, tuple)) or len(values) != count:
        raise ValueError(f"{name} must contain exactly {count} integers")
    checked = tuple(_int(value, name) for value in values)
    if any(abs(value) > limit for value in checked):
        raise ValueError(f"{name} is out of range")
    return checked


@dataclass(frozen=True, slots=True)
class QuantizedMLP:
    """Validated QMLP-V1 parameters. Weight matrices are flat and row-major."""

    d_in: int
    d_hidden: int
    d_out: int
    multiplier: int
    shift: int
    w1: tuple[int, ...]
    b1: tuple[int, ...]
    w2: tuple[int, ...]
    b2: tuple[int, ...]

    def __post_init__(self) -> None:
        for name, value, upper in (
            ("d_in", self.d_in, MAX_INPUT),
            ("d_hidden", self.d_hidden, MAX_HIDDEN),
            ("d_out", self.d_out, MAX_OUTPUT),
            ("multiplier", self.multiplier, MAX_MULTIPLIER),
            ("shift", self.shift, MAX_SHIFT),
        ):
            if not 1 <= _int(value, name) <= upper:
                raise ValueError(f"{name} must be in 1..={upper}")
        _bounded(self.w1, self.d_hidden * self.d_in, INT8_LIMIT, "w1")
        _bounded(self.b1, self.d_hidden, BIAS_LIMIT, "b1")
        _bounded(self.w2, self.d_out * self.d_hidden, INT8_LIMIT, "w2")
        _bounded(self.b2, self.d_out, BIAS_LIMIT, "b2")

    @classmethod
    def from_json(cls, data: object) -> "QuantizedMLP":
        if not isinstance(data, dict):
            raise ValueError("model must be an object")
        return cls(
            d_in=_int(data.get("d_in"), "d_in"),
            d_hidden=_int(data.get("d_hidden"), "d_hidden"),
            d_out=_int(data.get("d_out"), "d_out"),
            multiplier=_int(data.get("multiplier"), "multiplier"),
            shift=_int(data.get("shift"), "shift"),
            w1=tuple(data.get("w1") or ()),
            b1=tuple(data.get("b1") or ()),
            w2=tuple(data.get("w2") or ()),
            b2=tuple(data.get("b2") or ()),
        )

    def to_json(self) -> dict:
        return {
            "d_in": self.d_in,
            "d_hidden": self.d_hidden,
            "d_out": self.d_out,
            "multiplier": self.multiplier,
            "shift": self.shift,
            "w1": list(self.w1),
            "b1": list(self.b1),
            "w2": list(self.w2),
            "b2": list(self.b2),
        }

    def encode_body(self) -> bytes:
        """Canonical model body (spec: ``model_body``)."""
        out = bytearray(
            struct.pack(">IIIII", self.d_in, self.d_hidden, self.d_out, self.multiplier, self.shift)
        )
        out.extend(struct.pack(f">{len(self.w1)}b", *self.w1))
        out.extend(struct.pack(f">{len(self.b1)}i", *self.b1))
        out.extend(struct.pack(f">{len(self.w2)}b", *self.w2))
        out.extend(struct.pack(f">{len(self.b2)}i", *self.b2))
        return bytes(out)

    def commitment(self) -> bytes:
        return sha256(MODEL_DOMAIN + self.encode_body()).digest()

    def hidden(self, row: Sequence[int]) -> tuple[int, ...]:
        """Exact first layer: accumulate, requantize (round half up), ReLU, clamp."""
        x = _bounded(row, self.d_in, INT8_LIMIT, "input row")
        half = 1 << (self.shift - 1)
        out = []
        for j in range(self.d_hidden):
            weights = self.w1[j * self.d_in:(j + 1) * self.d_in]
            acc = self.b1[j] + sum(w * v for w, v in zip(weights, x))
            q = (acc * self.multiplier + half) >> self.shift  # floor shift
            out.append(min(max(q, 0), INT8_LIMIT))
        return tuple(out)

    def logits(self, row: Sequence[int]) -> tuple[int, ...]:
        h = self.hidden(row)
        return tuple(
            self.b2[k] + sum(w * v for w, v in zip(self.w2[k * self.d_hidden:(k + 1) * self.d_hidden], h))
            for k in range(self.d_out)
        )

    def predict(self, row: Sequence[int]) -> int:
        """Lowest index among the maximal logits."""
        values = self.logits(row)
        return values.index(max(values))


def rules_commitment() -> bytes:
    return sha256(RULES_DOMAIN).digest()


def qmlp_input_commitment(model: QuantizedMLP, rows: Sequence[Sequence[int]]) -> bytes:
    if not isinstance(rows, (list, tuple)) or not 1 <= len(rows) <= MAX_ROWS:
        raise ValueError("rows must be a nonempty bounded array")
    for row in rows:
        _bounded(row, model.d_in, INT8_LIMIT, "input row")
    return input_commitment(rows)


def result_commitment(start: int, count: int, d_out: int, logits: Sequence[Sequence[int]]) -> bytes:
    if count != len(logits) or any(len(row) != d_out for row in logits):
        raise ValueError("logit shape does not match the range")
    payload = bytearray(RESULT_DOMAIN)
    payload.extend(struct.pack(">QQI", start, count, d_out))
    for row in logits:
        payload.extend(struct.pack(f">{d_out}q", *row))
    return sha256(payload).digest()


def execute_partition(
    model: QuantizedMLP, rows: Sequence[Sequence[int]], start: int, count: int
) -> tuple[tuple[tuple[int, ...], ...], bytes]:
    """Run rows ``[start, start + count)`` and commit to their logits."""
    if type(start) is not int or type(count) is not int:
        raise ValueError("range must be integers")
    if start < 0 or count < 1 or start + count > len(rows):
        raise ValueError("range exceeds the input rows")
    outputs = tuple(model.logits(row) for row in rows[start:start + count])
    return outputs, result_commitment(start, count, model.d_out, outputs)


def fixture_model() -> QuantizedMLP:
    """The shared cross-language test vector from the specification."""
    return QuantizedMLP(
        d_in=2,
        d_hidden=2,
        d_out=2,
        multiplier=3,
        shift=2,
        w1=(1, -2, 3, 4),
        b1=(0, -5),
        w2=(2, -1, -3, 5),
        b2=(1, 0),
    )


FIXTURE_ROWS = ((1, 2), (3, -4), (-2, 5), (127, 127))
