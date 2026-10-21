"""Off-chain PyTorch/ONNX path. Torch and ONNX tests are skipped unless the
optional ``research`` extra is installed (CI job "PyTorch / ONNX off-chain")."""

import pytest

from aivoblock_ai import digits, qmlp


def test_digit_dataset_is_deterministic_bounded_and_balanced() -> None:
    first = digits.make_dataset(400, 3)
    assert first == digits.make_dataset(400, 3)
    assert first != digits.make_dataset(400, 4)
    rows, labels = first
    assert all(len(row) == digits.PIXELS for row in rows)
    assert all(-127 <= value <= 127 for row in rows for value in row)
    counts = [labels.count(digit) for digit in range(digits.CLASSES)]
    assert min(counts) >= 20, counts
    # Every input is a valid QMLP-V1 row for a 32-input model.
    model = qmlp.QuantizedMLP(32, 1, 1, 1, 1, (1,) * 32, (0,), (1,), (0,))
    assert qmlp.qmlp_input_commitment(model, rows[:256])


def test_lcg_matches_its_specified_first_outputs() -> None:
    rng = digits.Lcg(0)
    state = 3037000493
    for _ in range(3):
        state = (state * 6364136223846793005 + 1442695040888963407) % (1 << 64)
        assert rng.below(1 << 31) == (state >> 33) % (1 << 31)



def test_pytorch_initialization_seed_is_explicit_and_reproducible() -> None:
    torch = pytest.importorskip("torch")
    from aivoblock_ai import torch_qmlp

    rows, labels = digits.make_dataset(100, 5)
    def weights(seed: int):
        model = torch_qmlp.train_float_mlp(rows, labels, epochs=2, seed=seed)
        return [p.detach().clone() for p in model.parameters()]

    first, again, different = weights(11), weights(11), weights(17)
    assert all(torch.equal(a, b) for a, b in zip(first, again))
    assert any(not torch.equal(a, b) for a, b in zip(first, different))


@pytest.fixture(scope="module")
def experiment():
    pytest.importorskip("torch")
    pytest.importorskip("onnx")
    pytest.importorskip("onnxruntime")
    from aivoblock_ai import research

    return research.run_experiment(train=1200, test=300, epochs=150)


def test_pytorch_model_quantizes_to_an_accurate_qmlp_model(experiment) -> None:
    report, quantized, _, _, _ = experiment
    assert report["float"]["accuracy"] >= 0.85, report["float"]
    assert report["qmlp"]["accuracy"] >= 0.8, report["qmlp"]
    assert report["qmlp"]["agreement_with_float"] >= 0.85, report["qmlp"]
    # The quantized model is a valid, committed QMLP-V1 model.
    assert qmlp.QuantizedMLP.from_json(quantized.to_json()) == quantized
    assert report["qmlp"]["model_commitment"] == quantized.commitment().hex()


def test_onnx_integer_graph_reproduces_consensus_logits_exactly(experiment) -> None:
    report, *_ = experiment
    assert report["onnx_integer"]["exact_match"] is True
    assert report["onnx_integer"]["mismatched_rows"] == 0


def test_onnx_float_graph_is_close_but_never_consensus_valid(experiment) -> None:
    report, *_ = experiment
    assert report["onnx_float"]["max_abs_diff_vs_torch"] < 1e-3
    assert report["onnx_float"]["prediction_agreement_with_torch"] >= 0.99
    assert report["onnx_float"]["consensus_valid"] is False


def test_exported_task_file_is_self_consistent(experiment) -> None:
    from aivoblock_ai import research

    _, quantized, _, _, (rows, labels) = experiment
    export = research.export_task_file(quantized, rows, labels)
    model = qmlp.QuantizedMLP.from_json(export["model"])
    assert model.commitment().hex() == export["model_commitment"]
    logits, commitment = qmlp.execute_partition(model, export["rows"], 0, len(export["rows"]))
    assert [list(row) for row in logits] == export["logits"]
    assert commitment.hex() == export["result_commitment"]
