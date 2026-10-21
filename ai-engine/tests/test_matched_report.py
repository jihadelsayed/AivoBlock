import copy
import pytest
from aivoblock_ai.matched_report import summarize


def fixture(mode):
    return {
        "mode": mode, "model_commitment": "ab" * 32,
        "input_commitment": "cd" * 32, "result_commitment": "ef" * 32,
        "rows": 12, "iterations": 3,
        "latency_ms": {"observations": [1.0, 2.0, 3.0]},
    }


def pair():
    return fixture("trusted_result"), fixture("rust_reference_inference_only")


def test_matched_summaries():
    py, rust = pair()
    report = summarize(py, rust)
    assert report["rows"] == 12
    assert report["rust_over_python_mean_latency_ratio"] == 1.0


@pytest.mark.parametrize("key", ["model_commitment", "input_commitment", "result_commitment", "rows"])
def test_mismatch_rejected(key):
    py, rust = pair()
    rust[key] = 13 if key == "rows" else "00" * 32
    with pytest.raises(ValueError, match=key):
        summarize(py, rust)


@pytest.mark.parametrize("bad", [[1.0, 0.0, 2.0], [1.0, float("nan"), 2.0], [1.0, 2.0]])
def test_invalid_latencies_rejected(bad):
    py, rust = pair()
    rust["latency_ms"]["observations"] = bad
    rust["iterations"] = len(bad)
    with pytest.raises(ValueError, match="latency"):
        summarize(py, rust)


def test_untrusted_mode_rejected():
    py, rust = pair()
    py["mode"] = "same_implementation_recomputation"
    with pytest.raises(ValueError, match="trusted"):
        summarize(py, rust)
