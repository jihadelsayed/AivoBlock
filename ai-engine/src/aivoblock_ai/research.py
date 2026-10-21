"""Off-chain PyTorch -> QMLP-V1 -> ONNX experiment (machine-readable report).

    python -m aivoblock_ai.research --out research-out

1. Generate the deterministic noisy-digit dataset (pure Python).
2. Train a float MLP with PyTorch on CPU.
3. Quantize it to QMLP-V1 and evaluate the exact integer model.
4. Export the float MLP to ONNX and compare ONNX Runtime with PyTorch.
5. Export the integer model to ONNX and check ONNX Runtime reproduces the
   consensus logits exactly.
6. Write ``digits.qmlp.json`` (accepted by ``aivoblock-node client
   submit-task --qmlp-file``), both ONNX files and ``report.json``.

Every number in the report is measured by this run on this machine; nothing
is hard-coded. Requires the optional ``research`` extra.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import platform
import time

from . import digits, onnx_qmlp, qmlp, torch_qmlp

EXPORT_ROWS = 12


def run_experiment(
    train: int = 2000,
    test: int = 500,
    seed: int = 7,
    training_seed: int = 0,
    hidden: int = 32,
    epochs: int = 200,
):
    """Return (report, quantized model, float ONNX proto, integer ONNX proto, test set)."""
    import numpy as np
    import onnx
    import onnxruntime
    import torch

    train_rows, train_labels = digits.make_dataset(train, seed)
    test_rows, test_labels = digits.make_dataset(test, seed + 1)

    started = time.perf_counter()
    model = torch_qmlp.train_float_mlp(
        train_rows, train_labels, hidden=hidden, epochs=epochs, seed=training_seed
    )
    train_seconds = time.perf_counter() - started

    started = time.perf_counter()
    torch_out = torch_qmlp.float_logits(model, test_rows)
    torch_seconds = time.perf_counter() - started
    float_predictions = [torch_qmlp.argmax(row) for row in torch_out]

    quantized = torch_qmlp.quantize(model, train_rows)
    started = time.perf_counter()
    int_logits = [quantized.logits(row) for row in test_rows]
    int_seconds = time.perf_counter() - started
    int_predictions = [quantized.predict(row) for row in test_rows]

    float_proto = onnx_qmlp.float_graph(model)
    scaled = np.array(test_rows, dtype=np.float32) * np.float32(torch_qmlp.INPUT_SCALE)
    ort_float = onnx_qmlp.run(float_proto, scaled)
    reference = np.array(torch_out, dtype=np.float32)
    ort_float_predictions = [int(i) for i in np.argmax(ort_float, axis=1)]

    int_proto = onnx_qmlp.integer_graph(quantized)
    started = time.perf_counter()
    ort_int = onnx_qmlp.run(int_proto, np.array(test_rows, dtype=np.int64))
    ort_int_seconds = time.perf_counter() - started
    expected = np.array(int_logits, dtype=np.int64)
    mismatched_rows = int(np.sum(np.any(ort_int != expected, axis=1)))

    report = {
        "dataset": {
            "kind": "noisy 8x4 digit glyphs (aivoblock_ai.digits)",
            "train": train,
            "test": test,
            "seed": seed,
            "training_seed": training_seed,
            "features": digits.PIXELS,
            "classes": digits.CLASSES,
        },
        "environment": {
            "python": platform.python_version(),
            "machine": platform.machine(),
            "torch": torch.__version__,
            "onnx": onnx.__version__,
            "onnxruntime": onnxruntime.__version__,
            "numpy": np.__version__,
        },
        "float": {
            "layers": f"{digits.PIXELS} -> {hidden} -> {digits.CLASSES}",
            "epochs": epochs,
            "train_seconds": train_seconds,
            "inference_seconds": torch_seconds,
            "accuracy": torch_qmlp.accuracy(float_predictions, test_labels),
        },
        "qmlp": {
            "multiplier": quantized.multiplier,
            "shift": quantized.shift,
            "model_commitment": quantized.commitment().hex(),
            "parameter_bytes": len(quantized.encode_body()),
            "accuracy": torch_qmlp.accuracy(int_predictions, test_labels),
            "agreement_with_float": torch_qmlp.accuracy(int_predictions, float_predictions),
            "python_reference_seconds": int_seconds,
        },
        "onnx_float": {
            "max_abs_diff_vs_torch": float(np.max(np.abs(ort_float - reference))),
            "bitwise_identical_to_torch": bool(np.array_equal(ort_float, reference)),
            "prediction_agreement_with_torch": torch_qmlp.accuracy(
                ort_float_predictions, float_predictions
            ),
            "consensus_valid": False,
        },
        "onnx_integer": {
            "exact_match": mismatched_rows == 0,
            "mismatched_rows": mismatched_rows,
            "inference_seconds": ort_int_seconds,
            "consensus_valid": "only through the QMLP-V1 specification",
        },
    }
    return report, quantized, float_proto, int_proto, (test_rows, test_labels)


def export_task_file(quantized: qmlp.QuantizedMLP, rows, labels) -> dict:
    """Model plus a batch of inputs in the format the node CLI accepts."""
    rows = [list(row) for row in rows[:EXPORT_ROWS]]
    logits, commitment = qmlp.execute_partition(quantized, rows, 0, len(rows))
    return {
        "workload": "qmlp-v1",
        "model": quantized.to_json(),
        "model_commitment": quantized.commitment().hex(),
        "rows": rows,
        "labels": list(labels[:EXPORT_ROWS]),
        "logits": [list(row) for row in logits],
        "result_commitment": commitment.hex(),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path, default=Path("research-out"))
    parser.add_argument("--train", type=int, default=2000)
    parser.add_argument("--test", type=int, default=500)
    parser.add_argument("--epochs", type=int, default=200)
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--training-seed", type=int, default=0,
                        help="PyTorch initialization seed (default 0 preserves earlier CI experiments)")
    args = parser.parse_args()
    report, quantized, float_proto, int_proto, (rows, labels) = run_experiment(
        train=args.train, test=args.test, seed=args.seed,
        training_seed=args.training_seed, epochs=args.epochs
    )
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "float-mlp.onnx").write_bytes(float_proto.SerializeToString())
    (args.out / "qmlp-v1-int64.onnx").write_bytes(int_proto.SerializeToString())
    export = export_task_file(quantized, rows, labels)
    (args.out / "digits.qmlp.json").write_text(json.dumps(export, indent=2))
    (args.out / "report.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
    return 0 if report["onnx_integer"]["exact_match"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
