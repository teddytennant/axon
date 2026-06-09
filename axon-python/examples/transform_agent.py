"""Axon agent with two JSON-transforming capabilities.

    python3 -m axon.cli serve-mcp --module examples.transform_agent:TransformAgent
    python3 -m axon.cli serve --module examples.transform_agent:TransformAgent
"""

from __future__ import annotations

import json

from axon import Agent, capability

_UPPER_SCHEMA = {
    "type": "object",
    "properties": {"text": {"type": "string"}},
    "required": ["text"],
}


class TransformAgent(Agent):
    name = "transform"

    @capability("json", "upper", version=1, input_schema=_UPPER_SCHEMA)
    async def upper(self, payload: bytes) -> bytes:
        """Uppercase the 'text' field of a JSON object."""
        obj = json.loads(payload.decode("utf-8"))
        result = {"text": str(obj["text"]).upper()}
        return json.dumps(result).encode("utf-8")

    @capability("json", "keys", version=1)
    async def keys(self, payload: bytes) -> bytes:
        """Return the sorted keys of a JSON object as a JSON array."""
        obj = json.loads(payload.decode("utf-8"))
        return json.dumps(sorted(obj.keys())).encode("utf-8")


if __name__ == "__main__":
    pass
