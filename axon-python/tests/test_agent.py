"""Unit tests for axon.agent dispatch against the example agents."""

import asyncio
import os
import sys

# Make the examples/ directory importable.
REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if REPO not in sys.path:
    sys.path.insert(0, REPO)

from axon import Agent, capability  # noqa: E402
from axon.types import Capability, TaskRequest, TaskStatus  # noqa: E402

from examples.echo_agent import EchoAgent  # noqa: E402
from examples.transform_agent import TransformAgent  # noqa: E402


def test_echo_capabilities():
    agent = EchoAgent()
    tags = {c.tag() for c in agent.capabilities()}
    assert tags == {"echo:ping:v1"}


def test_transform_capabilities():
    agent = TransformAgent()
    tags = {c.tag() for c in agent.capabilities()}
    assert tags == {"json:upper:v1", "json:keys:v1"}


def test_handler_for_exact_tag():
    agent = EchoAgent()
    handler = agent.handler_for(Capability("echo", "ping", 1))
    assert handler is not None
    assert handler.capability.tag() == "echo:ping:v1"


def test_handler_for_version_match():
    # Provider is v1; a request for v1 resolves, a request for a higher
    # version does not.
    agent = EchoAgent()
    assert agent.handler_for(Capability("echo", "ping", 1)) is not None
    assert agent.handler_for(Capability("echo", "ping", 2)) is None


def test_invoke_echo_returns_payload():
    agent = EchoAgent()
    out = asyncio.run(agent.invoke(Capability("echo", "ping", 1), b"hi there"))
    assert out == b"hi there"


def test_invoke_transform_upper():
    agent = TransformAgent()
    out = asyncio.run(
        agent.invoke(Capability("json", "upper", 1), b'{"text": "hello"}')
    )
    assert out == b'{"text": "HELLO"}'


def test_invoke_transform_keys():
    agent = TransformAgent()
    out = asyncio.run(
        agent.invoke(Capability("json", "keys", 1), b'{"b": 1, "a": 2, "c": 3}')
    )
    assert out == b'["a", "b", "c"]'


def test_handle_success():
    agent = EchoAgent()
    req = TaskRequest(Capability("echo", "ping", 1), b"payload")
    resp = asyncio.run(agent.handle(req))
    assert resp.request_id == req.id
    assert resp.status == TaskStatus.success()
    assert resp.payload == b"payload"


def test_handle_unknown_capability():
    agent = EchoAgent()
    req = TaskRequest(Capability("echo", "missing", 1), b"x")
    resp = asyncio.run(agent.handle(req))
    assert resp.status == TaskStatus.no_capability()


class _FailingAgent(Agent):
    name = "failing"

    @capability("test", "boom", version=1)
    async def boom(self, payload: bytes) -> bytes:
        raise RuntimeError("kaboom")


def test_handle_error_status():
    agent = _FailingAgent()
    req = TaskRequest(Capability("test", "boom", 1), b"")
    resp = asyncio.run(agent.handle(req))
    assert resp.status.kind == "Error"
    assert resp.status == TaskStatus.error("kaboom")
