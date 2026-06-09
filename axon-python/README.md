# axon-python

Axon Python — write Axon mesh agents in Python.

A Python agent SDK for the [Axon](../README.md) decentralized AI agent mesh. The Rust core handles transport, routing, and discovery; you write agent logic — LLM calls, tools, business logic — in Python.

## Why

Axon's core (`axon-core`) keeps everything performance- and protocol-sensitive in Rust: QUIC transport, capability-based routing, discovery, trust, CRDT shared state. Agent logic is a different problem — it's where you call LLMs, run tools, and encode business rules — and Python is a pragmatic fit for that.

This package lets a Python process plug into the mesh as an agent. There are two integration paths:

- **MCP stdio server (primary).** Works with Axon today. Your agent runs as an MCP server; axon-core's existing `McpBridge` spawns it and exposes each capability as a mesh tool. No Rust changes required.
- **Axon Agent Protocol / AAP (secondary, experimental).** A native stdio JSON-RPC protocol that maps directly onto Axon's task model. A draft for a future Rust `PythonAgentBridge` — not yet consumed by axon-core. See [below](#axon-agent-protocol-aap--experimental).

## Install

```bash
cd axon-python
pip install -e ".[dev]"
```

Python 3.10+. Zero runtime dependencies — the SDK is pure standard library. The `dev` extra pulls in the test toolchain.

## Quickstart — write an agent

```python
from axon import Agent, capability

class EchoAgent(Agent):
    name = "echo"

    @capability("echo", "ping", version=1)
    async def ping(self, payload: bytes) -> bytes:
        return payload
```

Subclass `Agent`, set a class-level `name`, and decorate `async def(self, payload: bytes) -> bytes` methods with `@capability(namespace, name, version)`. Each decorated method becomes one `namespace:name:vN` mesh capability — the example above advertises `echo:ping:v1`.

Handlers take raw `payload: bytes` and return `bytes`. An agent may declare any number of capabilities; see `examples/transform_agent.py` for one that exposes two.

## Run as an MCP server

This is the path that works with Axon today.

```bash
axon-serve-mcp --module examples.echo_agent:EchoAgent
```

The server speaks MCP JSON-RPC 2.0 (`protocolVersion` `2024-11-05`) over stdio, newline-delimited. It advertises each agent capability as an MCP tool, with the tool name derived from the capability tag as `namespace.name.vN` (dots, since MCP tool names don't allow colons) — e.g. `echo:ping:v1` becomes the tool `echo.ping.v1`.

Tool-call arguments map onto the payload bytes: if the arguments object carries a string `payload` field, that string is used directly as the payload; otherwise the whole arguments object is JSON-encoded into the payload.

## Wire into Axon

Add an `[[mcp.servers]]` entry to `~/.config/axon/config.toml`:

```toml
[[mcp.servers]]
name = "my-python-agent"
command = "axon-serve-mcp"
args = ["--module", "examples.echo_agent:EchoAgent"]
```

`command` must resolve on `PATH` — installing this package provides the `axon-serve-mcp` entry point. If it isn't on `PATH`, use an absolute path or invoke the module directly:

```toml
command = "python"
args = ["-m", "axon.cli", "serve-mcp", "--module", "examples.echo_agent:EchoAgent"]
```

The `--module` value is a `module.path:ClassName` spec resolved via `axon.load_agent`. On `axon start`, axon-core's `McpBridge` spawns this process, runs the MCP handshake, lists its tools, and exposes them as mesh capabilities.

The `[[mcp.servers]]` table maps onto axon-core's `McpServerConfig` (`axon-core/src/mcp/client.rs`), so the other fields are available too:

| Field | Meaning |
|-------|---------|
| `name` | Human-readable server name |
| `command` | Command to spawn |
| `args` | Command arguments |
| `env` | Extra environment variables (table of `KEY = "value"`) |
| `timeout_secs` | Per-request timeout (default `30`) |

## Axon Agent Protocol (AAP) — experimental

```bash
axon-serve --module examples.echo_agent:EchoAgent
```

AAP is a lightweight stdio JSON-RPC 2.0 protocol (newline-delimited) that maps directly onto Axon's task model, skipping the MCP tool indirection. It defines three methods:

**`initialize`** → handshake:

```json
{
  "protocolVersion": "aap/0.1",
  "agent": { "name": "echo" },
  "capabilities": { "task": {} }
}
```

**`capabilities/list`** → the agent's capabilities:

```json
[
  { "namespace": "echo", "name": "ping", "version": 1,
    "tag": "echo:ping:v1", "description": "..." }
]
```

**`task/handle`** → takes a `TaskRequest`, returns a `TaskResponse`:

```json
// params: TaskRequest
{
  "id": "…uuid…",
  "capability": { "namespace": "echo", "name": "ping", "version": 1 },
  "payload": "aGVsbG8=",
  "timeout_ms": 30000
}

// result: TaskResponse
{
  "request_id": "…uuid…",
  "status": "Success",
  "payload": "aGVsbG8=",
  "duration_ms": 1
}
```

`payload` is base64. `status` matches the serde form of the Rust `TaskStatus` enum: `"Success"`, `{"Error": "msg"}`, `"Timeout"`, or `"NoCapability"`.

This is a **draft** for a future Rust `PythonAgentBridge` and is **not yet consumed by axon-core**. Use the MCP path for anything real today.

## Type parity

The Python types in `src/axon/types.py` mirror `axon-core/src/protocol.rs` field-for-field, using serde's JSON field names so payloads cross the process boundary unchanged:

| Python | Rust | Fields |
|--------|------|--------|
| `Capability` | `Capability` | `namespace: str`, `name: str`, `version: int` (`u32`) |
| `TaskRequest` | `TaskRequest` | `id`, `capability`, `payload`, `timeout_ms` |
| `TaskResponse` | `TaskResponse` | `request_id`, `status`, `payload`, `duration_ms` |
| `TaskStatus` | `TaskStatus` | `"Success"` / `{"Error": msg}` / `"Timeout"` / `"NoCapability"` |

A capability's canonical tag is `namespace:name:vN` (`Capability.tag()`). Payload bytes are base64-encoded in JSON (matching serde's `Vec<u8>` handling under a human-readable encoding).

## Testing

```bash
pip install -e ".[dev]"
pytest -v
```

Beyond unit tests over the types and agent machinery, the suite includes subprocess integration tests that spawn the MCP and AAP servers and exchange JSON-RPC lines over stdio end to end.

## Project layout

```
axon-python/
├── src/axon/
│   ├── types.py        # Capability, TaskRequest, TaskResponse, TaskStatus
│   ├── agent.py        # Agent base class, @capability, load_agent
│   ├── mcp_server.py   # MCP JSON-RPC server (axon-serve-mcp)
│   ├── aap_server.py   # AAP JSON-RPC server (axon-serve)
│   └── cli.py          # serve-mcp / serve entry points
├── examples/           # echo_agent.py, transform_agent.py
└── tests/
```

## Follow-up Rust work

A future `PythonAgentBridge` in axon-core could consume AAP directly — spawn `axon-serve`, run `initialize` / `capabilities/list`, and dispatch tasks via `task/handle` — so Python agents register as first-class mesh agents with native capabilities, without the MCP tool name/argument indirection.

## License

MIT.
