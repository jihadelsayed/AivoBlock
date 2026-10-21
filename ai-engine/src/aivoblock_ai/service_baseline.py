"""Bounded non-blockchain QMLP inference service for controlled research.

Persistent model, batched CPU inference, optional same-implementation recomputation.
No blockchain, signatures, networking, finality or ledger. Not an optimized
GPU serving baseline; all requests are processed synchronously.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Sequence

from .qmlp import MAX_ROWS, QuantizedMLP, qmlp_input_commitment, result_commitment


@dataclass(frozen=True)
class ServiceResult:
    logits: tuple[tuple[int, ...], ...]
    model_commitment: str
    input_commitment: str
    result_commitment: str
    recomputed_same_implementation: bool


class QmlpService:
    """Load the validated committed model once, and handle bounded batches."""

    def __init__(self, model: QuantizedMLP, *, max_batch: int = 64):
        if type(max_batch) is not int or not 1 <= max_batch <= MAX_ROWS:
            raise ValueError("max_batch must be 1..256")
        self._model = model
        self._max_batch = max_batch
        self._commitment = model.commitment()

    def execute(self, rows: Sequence[Sequence[int]], *,
                verify_independently: bool = False) -> ServiceResult:
        if type(verify_independently) is not bool:
            raise ValueError("verify_independently must be boolean")
        # Enforce exactly the same per-task size and row semantics as consensus.
        if not isinstance(rows, (tuple, list)):
            raise ValueError("rows must be an array")
        rows = tuple(tuple(row) if isinstance(row, (tuple, list)) else row for row in rows)
        inputs = qmlp_input_commitment(self._model, rows)
        outputs = []
        for offset in range(0, len(rows), self._max_batch):
            batch = rows[offset:offset + self._max_batch]
            outputs.extend(self._model.logits(row) for row in batch)
        logits = tuple(outputs)
        commitment = result_commitment(0, len(logits), self._model.d_out, logits)
        if verify_independently:
            # This control deliberately repeats all inference; it does not
            # claim independence across runtimes or implementations.
            checked = tuple(self._model.logits(row) for row in rows)
            if checked != logits or result_commitment(
                0, len(checked), self._model.d_out, checked
            ) != commitment:
                raise ValueError("independent recomputation mismatch")
        return ServiceResult(
            logits=logits,
            model_commitment=self._commitment.hex(),
            input_commitment=inputs.hex(),
            result_commitment=commitment.hex(),
            recomputed_same_implementation=verify_independently,
        )
