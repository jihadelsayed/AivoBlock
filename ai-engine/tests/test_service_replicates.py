"""Validation tests for independently launched service-core replicate summaries."""
import pytest

from aivoblock_ai.service_replicates import aggregate, run


def report(mode, values):
    return {
        "mode": mode, "model_commitment": "aa", "input_commitment": "bb",
        "result_commitment": "cc", "rows": 4, "iterations": len(values),
        "latency_ms": {"observations": values},
    }


def suite(n=30):
    return [report(mode, [float(i + 1)] * n)
            for mode in ("trusted_result", "same_implementation_recomputation")
            for i in range(3)]


def test_replicates_summary():
    results = aggregate(suite(), launches=3, iterations=30)
    assert results["modes"]["trusted_result"]["count"] == 90
    assert results["modes"]["same_implementation_recomputation"]["p95_ms"] == 3.0
    assert results["modes"]["trusted_result"]["per_launch_medians_ms"] == [1.0, 2.0, 3.0]


def test_reject_cross_workload_substitution():
    reports = suite()
    reports[3]["input_commitment"] = "changed"
    with pytest.raises(ValueError, match="mismatched"):
        aggregate(reports, launches=3, iterations=30)


@pytest.mark.parametrize("bad", [0, float("nan"), float("inf"), -1, True])
def test_reject_invalid_samples(bad):
    reports = suite()
    reports[0]["latency_ms"]["observations"][0] = bad
    with pytest.raises(ValueError, match="invalid"):
        aggregate(reports, launches=3, iterations=30)


def test_reject_missing_launch():
    with pytest.raises(ValueError, match="missing"):
        aggregate(suite()[:-1], launches=3, iterations=30)


def test_reject_too_few_runs():
    with pytest.raises(ValueError, match="launches"):
        run(None, launches=2)


def test_reject_too_few_observations():
    with pytest.raises(ValueError, match="iterations"):
        run(None, iterations=29)
