"""AAP host: expose a Python Axon :class:`Agent` over the Axon Agent Protocol.

EXPERIMENTAL — the Axon Agent Protocol is a draft for a future Rust
PythonAgentBridge. Wire format: newline-delimited JSON-RPC 2.0 over stdio.
Methods: initialize, capabilities/list, task/handle.

Runs a blocking, synchronous read loop on stdin: one JSON-RPC request per line,
one response line per request, flushed. Async handler dispatch is driven through
a private event loop. Standard library + asyncio only; no external dependencies.
"""

from __future__ import annotations

import asyncio
import base64
import binascii
import json
import sys
from typing import Any, Optional, TextIO

from .agent import Agent, CapabilityHandler
from .types import Capability, TaskRequest

PROTOCOL_VERSION = "aap/0.1"


def _ok(req_id: Any, result: Any) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": req_id, "result": result}


def _err(req_id: Any, code: int, message: str) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": req_id, "error": {"code": code, "message": message}}


def _capability_descriptor(handler: CapabilityHandler) -> dict[str, Any]:
    cap = handler.capability
    descriptor = cap.to_json()
    descriptor["tag"] = cap.tag()
    descriptor["description"] = handler.description
    return descriptor


def _coerce_payload(value: Any) -> bytes:
    """Decode a payload field tolerantly.

    Accepts raw bytes, a base64 string, or — failing that — plain text which is
    utf-8 encoded. The structured base64 form is canonical; the plain-text
    fallback exists only for the convenience param shape.
    """
    if isinstance(value, bytes):
        return value
    if value is None:
        return b""
    if isinstance(value, str):
        if not value:
            return b""
        try:
            return base64.b64decode(value, validate=True)
        except (binascii.Error, ValueError):
            return value.encode("utf-8")
    raise ValueError(f"cannot decode payload of type {type(value).__name__}")


def _build_task_request(params: dict[str, Any]) -> TaskRequest:
    """Build a :class:`TaskRequest` from ``task/handle`` params.

    Canonical form is a full TaskRequest object::

        {"id": ..., "capability": {"namespace": ..., "name": ..., "version": ...},
         "payload": <base64 str>, "timeout_ms": ...}

    A convenience form is also accepted where ``capability`` is a
    ``"namespace:name:vN"`` tag string and ``payload`` may be base64 or plain
    text. Prefer the structured form.
    """
    cap = params.get("capability")
    if isinstance(cap, str):
        capability = Capability.parse_tag(cap)
        payload = _coerce_payload(params.get("payload"))
        request = TaskRequest(capability=capability, payload=payload)
        if "id" in params:
            request.id = params["id"]
        if "timeout_ms" in params:
            request.timeout_ms = int(params["timeout_ms"])
        return request
    return TaskRequest.from_json(params)


def serve(
    agent: Agent,
    *,
    in_stream: Optional[TextIO] = None,
    out_stream: Optional[TextIO] = None,
) -> None:
    """Serve ``agent`` over AAP on stdio (blocking).

    Reads newline-delimited JSON-RPC 2.0 requests from ``in_stream`` (default
    :data:`sys.stdin`) and writes one response line per request to ``out_stream``
    (default :data:`sys.stdout`), flushing after each. Streams may be injected
    for testing. Malformed lines are skipped; notifications (no ``id``) get no
    response.
    """
    in_stream = in_stream if in_stream is not None else sys.stdin
    out_stream = out_stream if out_stream is not None else sys.stdout

    loop = asyncio.new_event_loop()
    try:
        for line in in_stream:
            stripped = line.strip()
            if not stripped:
                continue
            try:
                request = json.loads(stripped)
            except (json.JSONDecodeError, ValueError):
                continue
            if not isinstance(request, dict):
                continue

            # Notifications carry no id and expect no response.
            if "id" not in request:
                continue

            response = _dispatch(agent, request, loop)
            out_stream.write(json.dumps(response) + "\n")
            out_stream.flush()
    finally:
        loop.close()


def _dispatch(
    agent: Agent,
    request: dict[str, Any],
    loop: asyncio.AbstractEventLoop,
) -> dict[str, Any]:
    req_id = request.get("id")
    method = request.get("method", "")
    params = request.get("params") or {}
    if not isinstance(params, dict):
        params = {}

    if method == "initialize":
        return _ok(
            req_id,
            {
                "protocolVersion": PROTOCOL_VERSION,
                "agent": {"name": agent.name},
                "capabilities": {"task": {}},
            },
        )

    if method == "capabilities/list":
        return _ok(
            req_id,
            {"capabilities": [_capability_descriptor(h) for h in agent.handlers()]},
        )

    if method == "task/handle":
        return _task_handle(agent, req_id, params, loop)

    return _err(req_id, -32601, f"Method not found: {method}")


def _task_handle(
    agent: Agent,
    req_id: Any,
    params: dict[str, Any],
    loop: asyncio.AbstractEventLoop,
) -> dict[str, Any]:
    try:
        request = _build_task_request(params)
    except (KeyError, ValueError, TypeError) as exc:
        return _err(req_id, -32602, f"Invalid task request: {exc}")

    # agent.handle never raises: failures arrive as a structured TaskStatus.
    response = loop.run_until_complete(agent.handle(request))
    return _ok(req_id, response.to_json())
