"""Local Python worker entry point for AivoBlock AI puzzle tasks.

IPC contract: four-byte unsigned big-endian length, followed by UTF-8 JSON.
This is a development protocol, not consensus canonical serialization.
The Rust process owns scheduling; worker output is always untrusted.
"""

from __future__ import annotations

import json
import struct
import sys
from typing import BinaryIO

MAX_PAYLOAD = 4 * 1024 * 1024


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


def handle_request(request: dict) -> dict:
    """Respond to protocol diagnostics; actual AI work is not enabled yet."""
    if request.get("protocol_version") != 1:
        return {"ok": False, "error": "unsupported_protocol_version"}
    if request.get("type") == "health":
        return {
            "ok": True,
            "type": "health",
            "protocol_version": 1,
            "worker": "aivoblock-ai",
            "status": "ready_for_protocol_development",
        }
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
