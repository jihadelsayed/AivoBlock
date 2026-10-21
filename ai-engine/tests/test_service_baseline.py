"""Bounded service baseline semantics; no network or throughput claims."""
import pytest

from aivoblock_ai.qmlp import FIXTURE_ROWS, execute_partition, fixture_model
from aivoblock_ai.service_baseline import QmlpService


def test_exact_logits_and_commitment_match_reference():
    model = fixture_model()
    expected, commitment = execute_partition(model, FIXTURE_ROWS, 0, len(FIXTURE_ROWS))
    service = QmlpService(model, max_batch=2)
    for independent in (False, True):
        result = service.execute(FIXTURE_ROWS, verify_independently=independent)
        assert result.logits == expected
        assert result.result_commitment == commitment.hex()
        assert result.input_commitment
        assert result.model_commitment == model.commitment().hex()
        assert result.recomputed_same_implementation is independent


def test_batch_sizes_preserve_identical_outputs():
    model = fixture_model()
    outputs = [QmlpService(model, max_batch=n).execute(FIXTURE_ROWS)
               for n in (1, 2, 4, 64, 256)]
    assert all(x == outputs[0] for x in outputs)


@pytest.mark.parametrize("max_batch", [0, -1, 257, True, 2.5])
def test_invalid_batch_size(max_batch):
    with pytest.raises(ValueError):
        QmlpService(fixture_model(), max_batch=max_batch)


def test_invalid_rows_rejected():
    service = QmlpService(fixture_model())
    with pytest.raises(ValueError):
        service.execute([])
    with pytest.raises(ValueError):
        service.execute([[1, 2]] * 257)
    with pytest.raises(ValueError):
        service.execute([[128, 0]])
    with pytest.raises(ValueError):
        service.execute([[1]])


def test_invalid_verifier_mode_rejected():
    with pytest.raises(ValueError):
        QmlpService(fixture_model()).execute(FIXTURE_ROWS, verify_independently=1)
