"""Independent Python reference for Sayed Token issuance and reward splits.

This mirrors Haskell ``AivoBlock.Issuance`` / ``AivoBlock.Rewards`` and the
Rust ``issuance`` module so that test harnesses can predict every worker
balance without trusting any node. It is NOT a ledger and never moves funds.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Mapping

MAX_U64 = (1 << 64) - 1


@dataclass(frozen=True, slots=True)
class IssuancePolicy:
    initial_reward: int
    halving_interval: int
    max_supply: int

    def __post_init__(self) -> None:
        for name in ("initial_reward", "halving_interval", "max_supply"):
            value = getattr(self, name)
            if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= MAX_U64:
                raise ValueError(f"{name} must be an unsigned 64-bit integer")
        if self.halving_interval == 0:
            raise ValueError("Halving interval must be positive")
        if self.max_supply == 0:
            raise ValueError("Maximum supply must be positive")
        if self.initial_reward > self.max_supply:
            raise ValueError("Initial reward exceeds maximum supply")

    def scheduled_issuance(self, height: int, supply: int) -> int:
        """Units minted by the block at ``height`` given the prior supply."""
        if height <= 0:
            raise ValueError("Genesis does not issue rewards")
        if supply < 0 or supply > self.max_supply:
            raise ValueError("Ledger supply exceeds issuance cap")
        halvings = (height - 1) // self.halving_interval
        base = 0 if halvings >= 64 else self.initial_reward >> halvings
        return min(base, self.max_supply - supply)


def distribute_rewards(budget: int, weights: Mapping[bytes, int]) -> dict[bytes, int]:
    """Largest-remainder split; ties go to the smaller worker ID."""
    if not weights:
        raise ValueError("No reward recipients")
    if any(weight <= 0 for weight in weights.values()):
        raise ValueError("Contribution weights must be positive")
    total = sum(weights.values())
    quotas = []
    for worker in sorted(weights):
        numerator = budget * weights[worker]
        quotas.append((worker, numerator // total, numerator % total))
    remaining = budget - sum(whole for _, whole, _ in quotas)
    ranked = sorted(quotas, key=lambda quota: (-quota[2], quota[0]))
    bonus = {worker for worker, _, _ in ranked[:remaining]}
    return {worker: whole + (1 if worker in bonus else 0) for worker, whole, _ in quotas}


@dataclass(frozen=True, slots=True)
class BlockReward:
    """Expected outcome of settling one AI-work block."""

    issuance: int
    total_paid: int
    payouts: dict[bytes, int]


def expected_block_reward(
    policy: IssuancePolicy,
    height: int,
    supply_before: int,
    fee_budget: int,
    units_by_worker: Mapping[bytes, int],
) -> BlockReward:
    issuance = policy.scheduled_issuance(height, supply_before)
    total = fee_budget + issuance
    if total == 0:
        raise ValueError("Reward budget must be positive")
    return BlockReward(issuance, total, distribute_rewards(total, units_by_worker))
