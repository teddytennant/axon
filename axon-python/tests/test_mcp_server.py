"""Integration tests: drive the MCP stdio host as a subprocess."""

import json
import os
import subprocess
import sys

import pytest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

_TIMEOUT = 10


def _env():
    env = dict(os.environ)
    env["PYTHONPATH"] = os.pathsep.join(
        [os.path.join(REPO, "src"), REPO, env.get("PYTHONPATH", "")]
    )
    env["PYTHONUNBUFFERED"] = "1"
    return env


def _spawn():
    return subprocess.Popen(
        [
            sys.executable,
            "-m",
            "axon.cli",
            "serve-mcp",
            "--module",
            "examples.echo_agent:EchoAgent",
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=_env(),
        cwd=REPO,
        text=True,
    )


def rpc(proc, obj):
    """Write one JSON-RPC request line and read one response line."""
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    if not line:
        stderr = proc.stderr.read() if proc.stderr else ""
        raise AssertionError(f"no response from server; stderr:\n{stderr}")
    return json.loads(line)


def test_mcp_server_flow():
    proc = _spawn()
    try:
        init = rpc(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize"})
        result = init["result"]
        assert result["protocolVersion"] == "2024-11-05"
        assert result["serverInfo"]["name"] == "echo"

        listed = rpc(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        tools = listed["result"]["tools"]
        assert len(tools) == 1
        tool_name = tools[0]["name"]
        assert "echo" in tool_name and "ping" in tool_name

        called = rpc(
            proc,
            {
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": tool_name, "arguments": {"payload": "hello"}},
            },
        )
        content = called["result"]["content"]
        assert content[0]["text"] == "hello"
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=_TIMEOUT)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=_TIMEOUT)
