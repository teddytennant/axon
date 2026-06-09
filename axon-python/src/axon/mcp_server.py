"""MCP host: expose a Python Axon :class:`Agent` over the Model Context Protocol.

Runs a blocking, newline-delimited JSON-RPC 2.0 loop on stdio. Each capability
handler on the agent is advertised as one MCP tool (``tools/list``) and invoked
through ``tools/call``. Implements protocol version ``2024-11-05``, matching the
wire shapes of the Rust ``axon-test-mcp-server``.

Standard library + asyncio only; no external dependencies.
"""

from __future__ import annotations

import asyncio
import json
import sys
from typing import Any, Optional, TextIO

from .agent import Agent, CapabilityHandler

PROTOCOL_VERSION = "2024-11-05"
SERVER_VERSION = "0.1.0"

_DEFAULT_INPUT_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "payload": {"type": "string", "description": "Raw task payload as text"}
    },
    "required": [],
}


def _tool_name(handler: CapabilityHandler) -> str:
    """MCP tool name for a handler: capability tag with ':' -> '.'."""
    return handler.capability.tag().replace(":", ".")


def _tool_descriptor(handler: CapabilityHandler) -> dict[str, Any]:
    schema = handler.input_schema if handler.input_schema is not None else _DEFAULT_INPUT_SCHEMA
    return {
        "name": _tool_name(handler),
        "description": handler.description,
        "inputSchema": schema,
    }


def _arguments_to_payload(arguments: dict[str, Any]) -> bytes:
    """Convert a tools/call arguments object to raw payload bytes."""
    payload = arguments.get("payload")
    if isinstance(payload, str):
        return payload.encode("utf-8")
    return json.dumps(arguments, separators=(",", ":")).encode("utf-8")


def _ok(req_id: Any, result: Any) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": req_id, "result": result}


def _err(req_id: Any, code: int, message: str) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": req_id, "error": {"code": code, "message": message}}


def serve(
    agent: Agent,
    *,
    in_stream: Optional[TextIO] = None,
    out_stream: Optional[TextIO] = None,
) -> None:
    """Serve ``agent``'s capabilities over MCP on stdio (blocking).

    Reads newline-delimited JSON-RPC 2.0 requests from ``in_stream`` (default
    :data:`sys.stdin`) and writes one response line per request to ``out_stream``
    (default :data:`sys.stdout`), flushing after each. Streams may be injected
    for testing.
    """
    in_stream = in_stream if in_stream is not None else sys.stdin
    out_stream = out_stream if out_stream is not None else sys.stdout

    tools = {_tool_name(h): h for h in agent.handlers()}

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

            response = _dispatch(agent, tools, request, loop)
            out_stream.write(json.dumps(response) + "\n")
            out_stream.flush()
    finally:
        loop.close()


def _dispatch(
    agent: Agent,
    tools: dict[str, CapabilityHandler],
    request: dict[str, Any],
    loop: asyncio.AbstractEventLoop,
) -> dict[str, Any]:
    req_id = request.get("id")
    method = request.get("method", "")
    params = request.get("params") or {}

    if method == "initialize":
        return _ok(
            req_id,
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": agent.name, "version": SERVER_VERSION},
            },
        )

    if method == "tools/list":
        return _ok(req_id, {"tools": [_tool_descriptor(h) for h in tools.values()]})

    if method == "tools/call":
        return _tools_call(agent, tools, req_id, params, loop)

    return _err(req_id, -32601, f"Method not found: {method}")


def _tools_call(
    agent: Agent,
    tools: dict[str, CapabilityHandler],
    req_id: Any,
    params: dict[str, Any],
    loop: asyncio.AbstractEventLoop,
) -> dict[str, Any]:
    name = params.get("name", "")
    handler = tools.get(name)
    if handler is None:
        return _err(req_id, -32602, f"Unknown tool: {name}")

    arguments = params.get("arguments") or {}
    if not isinstance(arguments, dict):
        arguments = {}
    payload = _arguments_to_payload(arguments)

    try:
        result = loop.run_until_complete(agent.invoke(handler.capability, payload))
        text = result.decode("utf-8", errors="replace")
        return _ok(req_id, {"content": [{"type": "text", "text": text}], "isError": False})
    except Exception as exc:  # noqa: BLE001 — MCP tool errors live in result.isError
        return _ok(req_id, {"content": [{"type": "text", "text": str(exc)}], "isError": True})
