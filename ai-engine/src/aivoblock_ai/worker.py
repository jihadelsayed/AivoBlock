"""Local framed-JSON worker for deterministic AivoBlock AI puzzle experiments.

One request per process, using a four-byte big-endian length prefix.
Worker results are UNTRUSTED: the Rust node must independently verify them
before any ledger or consensus action. This is not proof-of-useful-work.
"""

from __future__ import annotations

import json
import struct
import sys
from typing import BinaryIO

from aivoblock_ai.commitments import validate_commitments
from aivoblock_ai.executor import execute_integer_inference
from aivoblock_ai.proof import PuzzleProof, proof_from_result, verify_by_recomputation
from aivoblock_ai.puzzle import Puzzle, WorkRange

MAX_PAYLOAD = 4 * 1024 * 1024
MAX_REQUEST_ROWS = 1024
MAX_REQUEST_FEATURES = 4096


def read_exact(stream: BinaryIO, count: int) -> bytes:
    chunks = bytearray()
    while len(chunks) < count:
        chunk = stream.read(count - len(chunks))
        if not chunk:
            raise EOFError("Incomplete IPC frame")
        chunks.extend(chunk)
    return bytes(chunks)


def read_frame(stream: BinaryIO) -> bytes:
    size = struct.unpack(">I", read_exact(stream, 4))[0]
    if not 0 < size <= MAX_PAYLOAD:
        raise ValueError("Invalid IPC payload length")
    return read_exact(stream, size)


def write_frame(stream: BinaryIO, payload: bytes) -> None:
    if not 0 < len(payload) <= MAX_PAYLOAD:
        raise ValueError("Invalid IPC response length")
    stream.write(struct.pack(">I", len(payload)))
    stream.write(payload)
    stream.flush()


def _hex32(value: object, name: str) -> bytes:
    if not isinstance(value, str) or len(value) != 64:
        raise ValueError(f"{name} must be 64 hexadecimal characters")
    try:
        return bytes.fromhex(value)
    except ValueError as exc:
        raise ValueError(f"{name} must be hexadecimal") from exc


def _integer(value: object, name: str) -> int:
    if type(value) is not int:
        raise ValueError(f"{name} must be an integer")
    return value


def _integer_vector(value: object, name: str) -> list[int]:
    if not isinstance(value, list) or not 1 <= len(value) <= MAX_REQUEST_FEATURES:
        raise ValueError(f"{name} must be a nonempty bounded array")
    return [_integer(item, name) for item in value]


def _execute_request(request: dict) -> dict:
    """Run one bounded integer-inference partition and describe its claimed proof."""
    puzzle_data = request.get("puzzle")
    if not isinstance(puzzle_data, dict):
        raise ValueError("puzzle must be an object")

    task = Puzzle(
        puzzle_id=_hex32(puzzle_data.get("puzzle_id"), "puzzle_id"),
        version=_integer(puzzle_data.get("version"), "version"),
        model_commitment=_hex32(puzzle_data.get("model_commitment"), "model_commitment"),
        input_commitment=_hex32(puzzle_data.get("input_commitment"), "input_commitment"),
        rules_commitment=_hex32(puzzle_data.get("rules_commitment"), "rules_commitment"),
        units=_integer(puzzle_data.get("units"), "units"),
        deadline_height=_integer(puzzle_data.get("deadline_height"), "deadline_height"),
    )
    worker_id = _hex32(request.get("worker_id"), "worker_id")
    work = WorkRange(
        _integer(request.get("start"), "start"),
        _integer(request.get("count"), "count"),
    )
    work.validate_for(task)

    rows_data = request.get("rows")
    if not isinstance(rows_data, list) or not 1 <= len(rows_data) <= MAX_REQUEST_ROWS:
        raise ValueError("rows must be a nonempty bounded array")
    if len(rows_data) != task.units:
        raise ValueError("rows length must equal puzzle units")
    rows = [_integer_vector(row, "row") for row in rows_data]
    weights = _integer_vector(request.get("weights"), "weights")

    validate_commitments(task.model_commitment, task.input_commitment, task.rules_commitment, rows, weights)
    result = execute_integer_inference(rows, weights, work)
    proof = proof_from_result(task, worker_id, result)

    return {
        "ok": True,
        "type": "execute_integer_inference",
        "protocol_version": 1,
        "outputs": list(result.outputs),
        "proof": {
            "puzzle_id": proof.puzzle_id.hex(),
            "worker_id": proof.worker_id.hex(),
            "start_unit": proof.start_unit,
            "unit_count": proof.unit_count,
            "result_commitment": proof.result_commitment.hex(),
            "witness_commitment": None,
        },
        "verified": False,
    }



def _verify_request(request: dict) -> dict:
    """Recompute a claimed proof against caller-supplied trusted puzzle inputs.

    The verifier must obtain the puzzle definition, rows and weights from an
    independent trusted source; values sent by a prover cannot authenticate
    themselves. This local operation is not consensus verification.
    """
    puzzle_data = request.get("puzzle")
    proof_data = request.get("proof")
    if not isinstance(puzzle_data, dict) or not isinstance(proof_data, dict):
        raise ValueError("puzzle and proof must be objects")

    puzzle = Puzzle(
        puzzle_id=_hex32(puzzle_data.get("puzzle_id"), "puzzle_id"),
        version=_integer(puzzle_data.get("version"), "version"),
        model_commitment=_hex32(puzzle_data.get("model_commitment"), "model_commitment"),
        input_commitment=_hex32(puzzle_data.get("input_commitment"), "input_commitment"),
        rules_commitment=_hex32(puzzle_data.get("rules_commitment"), "rules_commitment"),
        units=_integer(puzzle_data.get("units"), "units"),
        deadline_height=_integer(puzzle_data.get("deadline_height"), "deadline_height"),
    )
    if puzzle.units > MAX_REQUEST_ROWS:
        raise ValueError("puzzle exceeds local verification limit")
    rows_data = request.get("rows")
    if not isinstance(rows_data, list) or len(rows_data) != puzzle.units:
        raise ValueError("rows must match trusted puzzle units")
    rows = [_integer_vector(row, "row") for row in rows_data]
    weights = _integer_vector(request.get("weights"), "weights")

    witness = proof_data.get("witness_commitment")
    if witness is not None:
        witness = _hex32(witness, "witness_commitment")
    proof = PuzzleProof(
        puzzle_id=_hex32(proof_data.get("puzzle_id"), "proof.puzzle_id"),
        worker_id=_hex32(proof_data.get("worker_id"), "proof.worker_id"),
        start_unit=_integer(proof_data.get("start_unit"), "start_unit"),
        unit_count=_integer(proof_data.get("unit_count"), "unit_count"),
        result_commitment=_hex32(proof_data.get("result_commitment"), "result_commitment"),
        witness_commitment=witness,
    )
    if proof.unit_count > MAX_REQUEST_ROWS:
        raise ValueError("proof exceeds verification limit")
    validate_commitments(puzzle.model_commitment, puzzle.input_commitment, puzzle.rules_commitment, rows, weights)
    valid = verify_by_recomputation(puzzle, proof, rows, weights)
    return {
        "ok": True,
        "type": "verify_integer_inference",
        "protocol_version": 1,
        "verified": valid,
        "method": "full_recomputation",
    }

def handle_request(request: dict) -> dict:
    """Dispatch one request; unsupported or invalid tasks return framed errors."""
    if request.get("protocol_version") != 1:
        return {"ok": False, "error": "unsupported_protocol_version"}
    kind = request.get("type")
    if kind == "health":
        return {
            "ok": True,
            "type": "health",
            "protocol_version": 1,
            "worker": "aivoblock-ai",
            "status": "ready_for_protocol_development",
        }
    if kind == "execute_integer_inference":
        try:
            return _execute_request(request)
        except (ValueError, TypeError, KeyError):
            return {"ok": False, "error": "invalid_puzzle_request"}
    if kind == "verify_integer_inference":
        try:
            return _verify_request(request)
        except (ValueError, TypeError, KeyError):
            return {"ok": False, "error": "invalid_verification_request"}
    return {"ok": False, "error": "unsupported_request_type"}


def main() -> int:
    try:
        raw = read_frame(sys.stdin.buffer)
        request = json.loads(raw.decode("utf-8"))
        if not isinstance(request, dict):
            raise ValueError("Request must be a JSON object")
        response = handle_request(request)
        payload = json.dumps(response, separators=(",", ":"), allow_nan=False).encode("utf-8")
        write_frame(sys.stdout.buffer, payload)
        return 0
    except (EOFError, ValueError, UnicodeError, json.JSONDecodeError) as exc:
        print(f"AivoBlock worker protocol error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
