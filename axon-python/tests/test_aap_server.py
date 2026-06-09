"""Integration tests: drive the AAP stdio host as a subprocess."""

import base64
import json
import os
import subprocess
import sys

from axon.types import Capability, TaskRequest

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
            "serve",
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


def test_aap_server_flow():
    proc = _spawn()
    try:
        init = rpc(proc, {"jsonrpc": "2.0", "id": 1, "method": "initialize"})
        result = init["result"]
        assert result["protocolVersion"] == "aap/0.1"
        assert result["agent"]["name"] == "echo"

        listed = rpc(
            proc, {"jsonrpc": "2.0", "id": 2, "method": "capabilities/list"}
        )
        caps = listed["result"]["capabilities"]
        tags = {c["tag"] for c in caps}
        assert "echo:ping:v1" in tags

        params = TaskRequest(Capability("echo", "ping", 1), b"hi").to_json()
        handled = rpc(
            proc,
            {"jsonrpc": "2.0", "id": 3, "method": "task/handle", "params": params},
        )
        task_result = handled["result"]
        assert task_result["status"] == "Success"
        assert base64.b64decode(task_result["payload"]) == b"hi"
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=_TIMEOUT)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=_TIMEOUT)
