"""Deterministic model definitions for AivoBlock's prototype AI workloads.

Models here are immutable, integer-only specifications. Floating-point
PyTorch execution may be added for research but must not silently become
consensus-valid without reproducible verification rules.
"""

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
import struct

MAX_FEATURES = 4096
MAX_ABSOLUTE_WEIGHT = 1_000_000


@dataclass(frozen=True, slots=True)
class IntegerLinearModel:
    """A versioned integer linear model with no bias term."""

    weights: tuple[int, ...]
    version: int = 1

    def __post_init__(self) -> None:
        if type(self.version) is not int or self.version != 1:
            raise ValueError("Unsupported integer model version")
        if not 1 <= len(self.weights) <= MAX_FEATURES:
            raise ValueError("Model must contain 1 to 4096 weights")
        if any(type(w) is not int or abs(w) > MAX_ABSOLUTE_WEIGHT for w in self.weights):
            raise ValueError("Weights must be bounded integers")

    def predict(self, features: tuple[int, ...]) -> int:
        """Calculate one exact integer dot product."""
        if len(features) != len(self.weights):
            raise ValueError("Feature count does not match model")
        if any(type(x) is not int or abs(x) > MAX_ABSOLUTE_WEIGHT for x in features):
            raise ValueError("Features must be bounded integers")
        return sum(x * w for x, w in zip(features, self.weights))

    def encode(self) -> bytes:
        """Provisional deterministic encoding; not yet the chain's canonical format."""
        header = b"AIVOBLOCK-INT-LINEAR-V1\x00"
        return (
            header
            + struct.pack(">II", self.version, len(self.weights))
            + b"".join(struct.pack(">q", w) for w in self.weights)
        )

    def commitment(self) -> bytes:
        """SHA-256 commitment to the exact encoded model definition."""
        return sha256(self.encode()).digest()


def model_from_weights(weights: list[int] | tuple[int, ...]) -> IntegerLinearModel:
    """Copy supplied weights into an immutable model specification."""
    return IntegerLinearModel(tuple(weights))
