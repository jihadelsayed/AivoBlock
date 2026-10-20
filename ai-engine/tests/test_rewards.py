import pytest

from aivoblock_ai.rewards import IssuancePolicy, distribute_rewards, expected_block_reward

POLICY = IssuancePolicy(initial_reward=50, halving_interval=100, max_supply=2_000_000)


def worker(byte: int) -> bytes:
    return bytes([byte]) * 32


def test_schedule_matches_haskell_and_rust_vectors() -> None:
    assert [POLICY.scheduled_issuance(h, 0) for h in (1, 100, 101, 200, 201, 301)] == [
        50, 50, 25, 25, 12, 6,
    ]
    assert POLICY.scheduled_issuance(64 * 100 + 1, 0) == 0
    assert POLICY.scheduled_issuance(2**64 - 1, 0) == 0
    assert POLICY.scheduled_issuance(1, 2_000_000 - 10) == 10
    assert POLICY.scheduled_issuance(1, 2_000_000) == 0
    with pytest.raises(ValueError):
        POLICY.scheduled_issuance(1, 2_000_001)
    with pytest.raises(ValueError):
        POLICY.scheduled_issuance(0, 0)


def test_invalid_policies_are_rejected() -> None:
    for args in ((50, 0, 100), (50, 1, 0), (101, 1, 100), (-1, 1, 100), (True, 1, 100)):
        with pytest.raises(ValueError):
            IssuancePolicy(*args)


def test_shared_cross_language_settlement_vector() -> None:
    # Ledger: one account of 200 plus a fee pool of 600 => supply 800.
    reward = expected_block_reward(POLICY, 1, 800, 30, {worker(1): 1, worker(2): 1, worker(3): 1})
    assert reward.issuance == 50
    assert reward.total_paid == 80
    assert reward.payouts == {worker(1): 27, worker(2): 27, worker(3): 26}
    assert 600 + reward.issuance - reward.total_paid == 570


def test_largest_remainder_conserves_budget() -> None:
    for budget in range(0, 200):
        payouts = distribute_rewards(budget, {worker(1): 3, worker(2): 5, worker(9): 1})
        assert sum(payouts.values()) == budget
    assert distribute_rewards(3, {worker(1): 2, worker(2): 1}) == {worker(1): 2, worker(2): 1}
    with pytest.raises(ValueError):
        distribute_rewards(1, {})
    with pytest.raises(ValueError):
        distribute_rewards(1, {worker(1): 0})


def test_zero_total_reward_is_rejected() -> None:
    with pytest.raises(ValueError):
        expected_block_reward(POLICY, 1, 2_000_000, 0, {worker(1): 1})
