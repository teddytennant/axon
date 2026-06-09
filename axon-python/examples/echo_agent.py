"""Minimal Axon agent: echo a payload back unchanged.

The canonical "hello world" agent. Run it as a host via the CLI:

    python3 -m axon.cli serve-mcp --module examples.echo_agent:EchoAgent
    python3 -m axon.cli serve --module examples.echo_agent:EchoAgent
"""

from __future__ import annotations

from axon import Agent, capability


class EchoAgent(Agent):
    name = "echo"

    @capability("echo", "ping", version=1)
    async def ping(self, payload: bytes) -> bytes:
        """Return the payload unchanged."""
        return payload


if __name__ == "__main__":
    # The CLI hosts this agent; running the module directly is a no-op.
    pass
