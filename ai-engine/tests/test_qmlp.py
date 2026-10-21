import pytest

from aivoblock_ai import qmlp
from aivoblock_ai.worker import handle_request

# Shared cross-language vector (protocol/spec/quantized-mlp-v1.md, node/src/qmlp.rs).
MODEL_BODY_HEX = "000000020000000200000002000000030000000201fe030400000000fffffffb02fffd050000000100000000"
MODEL_COMMITMENT = "621e6ea400370f407c996e0aa2d5e96cf24a160a240d8c84f2bdc04b00f312e9"
RULES_COMMITMENT = "0dcdcf5a65f06c5ca3c60aa419867528ad0d5a4bfaf38c4610012b97a11642db"
INPUT_COMMITMENT = "bb5b8808a948c0c2c43650a35db38093d7e8fbd52263d9777eb8d7fa004f971f"
RESULT_ALL = "b864ae656be24b63335f81a5a3c1029ca2c906edff09de387513977b2eca9b05"
RESULT_ROWS_1_2 = "8ae5014a20a935bb5920ec1ea4f06ca0ab621403722faff0bf0822d52a2c0f87"
LOGITS = ((-4, 25), (17, -24), (-6, 35), (-126, 635))


def test_fixture_matches_the_specified_vector() -> None:
    model = qmlp.fixture_model()
    rows = qmlp.FIXTURE_ROWS
    assert [model.hidden(row) for row in rows] == [(0, 5), (8, 0), (0, 7), (0, 127)]
    assert tuple(model.logits(row) for row in rows) == LOGITS
    assert [model.predict(row) for row in rows] == [1, 0, 1, 1]
    assert model.encode_body().hex() == MODEL_BODY_HEX
    assert model.commitment().hex() == MODEL_COMMITMENT
    assert qmlp.rules_commitment().hex() == RULES_COMMITMENT
    assert qmlp.qmlp_input_commitment(model, rows).hex() == INPUT_COMMITMENT
    outputs, commitment = qmlp.execute_partition(model, rows, 0, 4)
    assert outputs == LOGITS and commitment.hex() == RESULT_ALL
    _, partial = qmlp.execute_partition(model, rows, 1, 2)
    assert partial.hex() == RESULT_ROWS_1_2


def test_requantization_rounds_half_up_and_clamps() -> None:
    # One hidden unit, identity output: logits equal the requantized hidden value.
    def single(weight: int, bias: int, multiplier: int, shift: int) -> qmlp.QuantizedMLP:
        return qmlp.QuantizedMLP(1, 1, 1, multiplier, shift, (weight,), (bias,), (1,), (0,))

    half = single(1, 0, 1, 1)  # (x + 1) >> 1
    assert [half.logits((x,))[0] for x in (0, 1, 2, 3, 127)] == [0, 1, 1, 2, 64]
    assert half.logits((-1,))[0] == 0  # ReLU
    big = single(127, 1 << 24, (1 << 31) - 1, 1)
    assert big.logits((127,))[0] == 127  # upper int8 clamp, no overflow


@pytest.mark.parametrize(
    "field, value",
    [
        ("d_in", 0),
        ("d_in", 33),
        ("d_hidden", 65),
        ("d_out", 17),
        ("multiplier", 0),
        ("multiplier", 1 << 31),
        ("shift", 0),
        ("shift", 41),
        ("w1", (1, -2, 3, 128)),
        ("w1", (1, -2, 3)),
        ("b1", (0, (1 << 24) + 1)),
        ("w2", (2, -1, -3, -128)),
        ("b2", (1,)),
        ("w1", (1, -2, 3, True)),
    ],
)
def test_rejects_out_of_range_models(field: str, value: object) -> None:
    params = qmlp.fixture_model().to_json()
    params[field] = list(value) if isinstance(value, tuple) else value
    with pytest.raises(ValueError):
        qmlp.QuantizedMLP.from_json(params)


def test_rejects_out_of_range_inputs_and_ranges() -> None:
    model = qmlp.fixture_model()
    for row in ((128, 0), (0, -128), (1,), (1, 2, 3), (1.0, 2)):
        with pytest.raises(ValueError):
            model.logits(row)
    with pytest.raises(ValueError):
        qmlp.execute_partition(model, qmlp.FIXTURE_ROWS, 3, 2)
    with pytest.raises(ValueError):
        qmlp.execute_partition(model, qmlp.FIXTURE_ROWS, 0, 0)
    with pytest.raises(ValueError):
        qmlp.qmlp_input_commitment(model, [(1, 2)] * 257)


def _request(**overrides: object) -> dict:
    model = qmlp.fixture_model()
    request = {
        "protocol_version": 1,
        "type": "execute_qmlp_inference",
        "task_id": "11" * 32,
        "worker_id": "22" * 32,
        "model": model.to_json(),
        "model_commitment": MODEL_COMMITMENT,
        "input_commitment": INPUT_COMMITMENT,
        "rules_commitment": RULES_COMMITMENT,
        "rows": [list(row) for row in qmlp.FIXTURE_ROWS],
        "start": 1,
        "count": 2,
    }
    request.update(overrides)
    return request


def test_worker_executes_a_committed_partition() -> None:
    response = handle_request(_request())
    assert response["ok"] is True
    assert response["result_commitment"] == RESULT_ROWS_1_2
    assert response["logits"] == [[17, -24], [-6, 35]]
    assert (response["start_unit"], response["unit_count"]) == (1, 2)
    assert response["verified"] is False


@pytest.mark.parametrize(
    "overrides",
    [
        {"model_commitment": "00" * 32},
        {"input_commitment": "00" * 32},
        {"rules_commitment": RESULT_ALL},
        {"rows": [[1, 2], [3, -4], [-2, 5], [127, 126]]},
        {"start": 3, "count": 2},
        {"count": 0},
        {"model": {"d_in": 2}},
        {"task_id": "zz"},
        {"rows": "not rows"},
    ],
)
def test_worker_refuses_mismatched_or_malformed_requests(overrides: dict) -> None:
    assert handle_request(_request(**overrides)) == {"ok": False, "error": "invalid_qmlp_request"}
