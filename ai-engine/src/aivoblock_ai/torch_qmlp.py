"""Off-chain PyTorch training and post-training quantization to QMLP-V1.

Floating-point training and inference are NOT consensus operations: their
results depend on CPU features, kernels, thread counts and library versions.
This module only *produces* a committed integer model. Consensus executes
that model with the exact integer rules of ``qmlp.py`` and
``node/src/qmlp.rs``; the float network never touches a block.

Requires the optional ``research`` extra (``pip install -e ai-engine[research]``).
"""

from __future__ import annotations

import math
from typing import Sequence

from .qmlp import BIAS_LIMIT, INT8_LIMIT, MAX_MULTIPLIER, MAX_SHIFT, QuantizedMLP

# Float inputs are the int8 pixels scaled to roughly [-1, 1].
INPUT_SCALE = 1.0 / 127.0


def train_float_mlp(
    rows: Sequence[Sequence[int]],
    labels: Sequence[int],
    hidden: int = 32,
    classes: int = 10,
    epochs: int = 200,
    seed: int = 0,
    lr: float = 0.02,
):
    """Full-batch Adam training of ``Linear -> ReLU -> Linear`` on CPU."""
    import torch

    torch.manual_seed(seed)
    torch.use_deterministic_algorithms(True)
    torch.set_num_threads(1)
    x = torch.tensor(rows, dtype=torch.float32) * INPUT_SCALE
    y = torch.tensor(labels, dtype=torch.long)
    model = torch.nn.Sequential(
        torch.nn.Linear(x.shape[1], hidden),
        torch.nn.ReLU(),
        torch.nn.Linear(hidden, classes),
    )
    optimizer = torch.optim.Adam(model.parameters(), lr=lr)
    loss_fn = torch.nn.CrossEntropyLoss()
    for _ in range(epochs):
        optimizer.zero_grad()
        loss = loss_fn(model(x), y)
        loss.backward()
        optimizer.step()
    model.eval()
    return model


def float_logits(model, rows: Sequence[Sequence[int]]) -> list[list[float]]:
    import torch

    with torch.no_grad():
        x = torch.tensor(rows, dtype=torch.float32) * INPUT_SCALE
        return model(x).tolist()


def _round(value: float) -> int:
    """Round half up, the same rule the integer requantization uses."""
    return int(math.floor(value + 0.5))


def _requantization(ratio: float) -> tuple[int, int]:
    """Largest shift in 1..=40 whose multiplier still fits in 31 bits."""
    shift = MAX_SHIFT
    while shift > 1 and _round(ratio * 2**shift) > MAX_MULTIPLIER:
        shift -= 1
    multiplier = max(1, min(MAX_MULTIPLIER, _round(ratio * 2**shift)))
    return multiplier, shift


def quantize(model, calibration_rows: Sequence[Sequence[int]]) -> QuantizedMLP:
    """Symmetric per-tensor int8 post-training quantization.

    Real values: x = s_x * x_q, W = s_w * W_q, hidden h = s_h * h_q. The first
    layer accumulates in units of s_x * s_w1 and is requantized to s_h by the
    integer multiplier/shift pair; the second layer's logits are in units of
    s_h * s_w2, a positive scale that preserves the predicted class.
    """
    import torch

    first, second = model[0], model[2]
    w1 = first.weight.detach().double().tolist()
    b1 = first.bias.detach().double().tolist()
    w2 = second.weight.detach().double().tolist()
    b2 = second.bias.detach().double().tolist()
    s_w1 = max(abs(v) for row in w1 for v in row) / INT8_LIMIT
    s_w2 = max(abs(v) for row in w2 for v in row) / INT8_LIMIT
    with torch.no_grad():
        x = torch.tensor(calibration_rows, dtype=torch.float64) * INPUT_SCALE
        weights = torch.tensor(w1, dtype=torch.float64)
        bias = torch.tensor(b1, dtype=torch.float64)
        h_max = float(torch.relu(x @ weights.T + bias).max())
    s_h = max(h_max, 1e-9) / INT8_LIMIT
    acc_scale = INPUT_SCALE * s_w1
    multiplier, shift = _requantization(acc_scale / s_h)

    def int8(value: float) -> int:
        return max(-INT8_LIMIT, min(INT8_LIMIT, _round(value)))

    def int_bias(value: float) -> int:
        return max(-BIAS_LIMIT, min(BIAS_LIMIT, _round(value)))

    return QuantizedMLP(
        d_in=len(w1[0]),
        d_hidden=len(w1),
        d_out=len(w2),
        multiplier=multiplier,
        shift=shift,
        w1=tuple(int8(v / s_w1) for row in w1 for v in row),
        b1=tuple(int_bias(v / acc_scale) for v in b1),
        w2=tuple(int8(v / s_w2) for row in w2 for v in row),
        b2=tuple(int_bias(v / (s_h * s_w2)) for v in b2),
    )


def argmax(values: Sequence[float]) -> int:
    """Lowest index among the maximal values (QMLP-V1 class rule)."""
    best = max(values)
    return list(values).index(best)


def accuracy(predictions: Sequence[int], labels: Sequence[int]) -> float:
    return sum(p == y for p, y in zip(predictions, labels)) / len(labels)
