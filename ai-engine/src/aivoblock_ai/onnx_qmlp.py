"""ONNX as a portable model format for AivoBlock AI work (off-chain).

Two graphs are built directly with ``onnx.helper`` (no exporter heuristics):

* ``float_graph``: the trained PyTorch MLP as ``Gemm -> Relu -> Gemm`` in
  float32. ONNX Runtime and PyTorch agree only approximately; float results
  are never consensus inputs.
* ``integer_graph``: a QMLP-V1 model using only int64 ONNX operators
  (MatMul, Add, Mul, Max, Div, Min). Because the ReLU clamps every negative
  requantization numerator to zero first, ONNX's truncating integer ``Div``
  equals the specification's floor shift, so ONNX Runtime can reproduce the
  consensus logits exactly. The ONNX file is a portable *description*:
  validators still execute the specification, not ONNX Runtime.

Requires the optional ``research`` extra.
"""

from __future__ import annotations

from .qmlp import INT8_LIMIT, QuantizedMLP

OPSET = 17
# IR version 8 matches opset 17 and loads in every supported ONNX Runtime.
IR_VERSION = 8


def _model(graph):
    import onnx
    from onnx import helper

    proto = helper.make_model(graph, opset_imports=[helper.make_opsetid("", OPSET)])
    proto.ir_version = IR_VERSION
    onnx.checker.check_model(proto)
    return proto


def float_graph(model):
    """The trained ``torch.nn.Sequential(Linear, ReLU, Linear)`` in float32."""
    import numpy as np
    from onnx import TensorProto, helper, numpy_helper

    first, second = model[0], model[2]

    def tensor(parameter, name: str):
        return numpy_helper.from_array(parameter.detach().numpy().astype(np.float32), name)

    d_in = int(first.weight.shape[1])
    d_out = int(second.weight.shape[0])
    graph = helper.make_graph(
        [
            helper.make_node("Gemm", ["X", "W1", "B1"], ["H0"], transB=1),
            helper.make_node("Relu", ["H0"], ["H"]),
            helper.make_node("Gemm", ["H", "W2", "B2"], ["Y"], transB=1),
        ],
        "aivoblock-float-mlp",
        [helper.make_tensor_value_info("X", TensorProto.FLOAT, [None, d_in])],
        [helper.make_tensor_value_info("Y", TensorProto.FLOAT, [None, d_out])],
        [
            tensor(first.weight, "W1"),
            tensor(first.bias, "B1"),
            tensor(second.weight, "W2"),
            tensor(second.bias, "B2"),
        ],
    )
    return _model(graph)


def integer_graph(q: QuantizedMLP):
    """QMLP-V1 with int64 ONNX operators only."""
    import numpy as np
    from onnx import TensorProto, helper, numpy_helper

    def const(name: str, value) -> object:
        return numpy_helper.from_array(np.array(value, dtype=np.int64), name)

    w1t = np.array(q.w1, dtype=np.int64).reshape(q.d_hidden, q.d_in).T.copy()
    w2t = np.array(q.w2, dtype=np.int64).reshape(q.d_out, q.d_hidden).T.copy()
    nodes = [
        helper.make_node("MatMul", ["X", "W1T"], ["P1"]),
        helper.make_node("Add", ["P1", "B1"], ["ACC"]),
        helper.make_node("Mul", ["ACC", "M"], ["SCALED"]),
        helper.make_node("Add", ["SCALED", "HALF"], ["NUM"]),
        helper.make_node("Max", ["NUM", "ZERO"], ["POSITIVE"]),
        helper.make_node("Div", ["POSITIVE", "POW2"], ["Q"]),
        helper.make_node("Min", ["Q", "CAP"], ["H"]),
        helper.make_node("MatMul", ["H", "W2T"], ["P2"]),
        helper.make_node("Add", ["P2", "B2"], ["Y"]),
    ]
    graph = helper.make_graph(
        nodes,
        "aivoblock-qmlp-v1",
        [helper.make_tensor_value_info("X", TensorProto.INT64, [None, q.d_in])],
        [helper.make_tensor_value_info("Y", TensorProto.INT64, [None, q.d_out])],
        [
            numpy_helper.from_array(w1t, "W1T"),
            const("B1", list(q.b1)),
            numpy_helper.from_array(w2t, "W2T"),
            const("B2", list(q.b2)),
            const("M", q.multiplier),
            const("HALF", 1 << (q.shift - 1)),
            const("ZERO", 0),
            const("POW2", 1 << q.shift),
            const("CAP", INT8_LIMIT),
        ],
    )
    return _model(graph)


def run(proto, inputs):
    """Run one graph on ONNX Runtime's CPU provider with a single thread."""
    import onnxruntime as ort

    options = ort.SessionOptions()
    options.intra_op_num_threads = 1
    options.inter_op_num_threads = 1
    # Run the graph exactly as written: no operator fusion or rewriting.
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_DISABLE_ALL
    session = ort.InferenceSession(
        proto.SerializeToString(), sess_options=options, providers=["CPUExecutionProvider"]
    )
    return session.run(None, {"X": inputs})[0]
