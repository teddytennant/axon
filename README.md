# axon

[![CI](https://github.com/teddytennant/axon/actions/workflows/ci.yml/badge.svg)](https://github.com/teddytennant/axon/actions/workflows/ci.yml)

A peer-to-peer runtime for AI agents to discover, communicate, and collaborate without central infrastructure. Built in Rust with QUIC transport, capability-based routing, CRDT shared state, and a full web dashboard.

## Why

Current AI agent frameworks are centralized — a single orchestrator dispatches tasks to workers. This creates single points of failure, bottlenecks, and doesn't scale. Axon takes a different approach: agents form a self-organizing mesh where any node can initiate work, discover capabilities, and route tasks without a coordinator.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   axon-cli                      │
│        TUI Dashboard + CLI Interface            │
│   Chat TUI │ Onboarding Wizard │ Trust CLI      │
├─────────────────────────────────────────────────┤
│                   axon-web                      │
│     Axum HTTP Server + React SPA Dashboard      │
│  WebSocket live updates │ Embedded static assets│
├─────────────────────────────────────────────────┤
│                  axon-core                      │
│  Discovery │ Router │ Agent Runtime             │
│  Transport (QUIC + mTLS) │ Identity (Ed25519)   │
│  Shared State (CRDTs) │ Negotiation             │
│  Trust │ MCP Gateway │ Orchestration            │
│  Task Queue (persistent) │ Gossip               │
└─────────────────────────────────────────────────┘
┌─────────────────────────────────────────────────┐
│                   axon-sdk                      │
│         Public API for custom agents            │
└─────────────────────────────────────────────────┘
```

**Core components:**

- **Identity** — Ed25519 keypairs for node authentication and message signing; identity-derived mTLS certificates
- **Transport** — QUIC via `quinn` with mutual TLS for multiplexed, encrypted peer communication
- **Protocol** — Binary message format (bincode): Ping/Pong, Announce, Discover, TaskRequest/TaskResponse, Gossip, StateSync, TaskOffer/TaskBid/BidAccept/BidReject, MCP tool messages
- **Discovery** — mDNS for LAN, gossip protocol for mesh-wide peer propagation
- **Router** — Capability-based routing with BestMatch, RoundRobin, Broadcast, and Negotiate strategies
- **Negotiation** — Agent-to-agent task bidding: TaskOffer → bid collection → winner dispatch; configurable scoring (latency, load, confidence, trust); pluggable bidding strategies
- **Trust** — Subjective, experience-based, decay-weighted reputation per peer; persistent sled-backed store; gossip propagation; trust-weighted bid scoring
- **Task Queue** — Persistent queue with crash recovery and automatic retries
- **MCP Gateway** — `axon serve-mcp` exposes aggregated MCP tools on stdio; `--mesh` joins the mesh and forwards remote tool calls via QUIC; budget-constrained tool selection for context window optimization; ToolCatalog gossip for decentralized discovery
- **Orchestration** — TOML-based agent definitions, shared blackboard state, capability-gated hooks, lifecycle management with heartbeat, workflow patterns (pipeline, fan-out, supervisor, swarm dispatch), workflow tracing with correlation IDs
- **Runtime** — Async agent executor with pluggable Agent trait
- **CRDTs** — GCounter, LWWRegister, ORSet for eventually-consistent shared state; peer trust and throughput history in sync tick

## Quick Start

```bash
# Interactive setup wizard (first run)
axon setup

# Start a mesh node with TUI dashboard
axon start

# Start with web UI at http://localhost:3000
axon start --web-port 3000

# Start on a specific port with a peer
axon start --listen 0.0.0.0:4242 --peer 192.168.1.100:4242

# Send a task to a peer
axon send --peer 127.0.0.1:4242 --namespace echo --name ping --data "hello"

# Interactive chat TUI
axon chat

# One-shot LLM prompt
axon ask "what is the capital of france"

# View your identity
axon identity
```

## Web Dashboard

Enable with `axon start --web-port 3000`, then open `http://localhost:3000`.

The React SPA connects via WebSocket for live updates and provides views for:

| Page | Description |
|------|-------------|
| Chat | Interactive chat with the LLM agent |
| Mesh | Live peer topology and capability map |
| Agents | Agent cards with status and capabilities |
| Tasks | Task log with status, latency, and routing |
| Workflows | Active and completed workflow runs |
| Blackboard | CRDT shared state viewer |
| Trust | Per-peer trust scores and observation history |
| Tools | MCP tool registry and search |
| Settings | Provider config, model picker |
| Logs | Structured node logs |

## Built-in Agents

| Agent | Capability | Description |
|-------|-----------|-------------|
| Echo | `echo:ping:v1` | Returns input payload (testing/diagnostics) |
| SystemInfo | `system:info:v1` | Returns hostname, OS, architecture |
| LLM | `llm:chat:v1` | Proxies to configured LLM provider |

## LLM Providers

Configure with `axon setup` or `axon auth <provider>`. All providers except Ollama require an API key.

| Provider | Notes |
|----------|-------|
| Ollama | Local inference, no key required |
| xAI | Grok models |
| OpenRouter | Unified gateway: Claude, GPT-4, Gemini, Mistral, Llama, and more |
| Custom | Any OpenAI-compatible endpoint |

```bash
axon auth openrouter       # save API key
axon models                # list available models
axon models --provider xai --filter grok
```

## Chat TUI

```bash
axon chat
```

Full-featured interactive chat with:

- Slash commands: `/model`, `/clear`, `/export`, `/agent`, `/orchestrate`, `/job`
- Async streaming responses
- Conversation history
- Background job scheduling
- Auto-agent mode (continuous background task execution)
- Multi-agent orchestration commands

## MCP Gateway

Axon can act as an MCP server, aggregating tools from multiple MCP servers and exposing them to any MCP-capable AI tool (Claude Code, Cursor, etc.).

```bash
# Serve local MCP tools on stdio
axon serve-mcp

# Serve local + remote mesh tools
axon serve-mcp --mesh

# Query tools on a running node
axon tools
axon tools --query "file operations" --budget 4000
axon tools --server filesystem --detail compact
```

Add to Claude Code config:
```json
{
  "mcpServers": {
    "axon": { "command": "axon", "args": ["serve-mcp", "--mesh"] }
  }
}
```

## Trust System

```bash
axon trust show                    # trust scores for all peers
axon trust show --peer <hex-id>    # specific peer
axon trust history --peer <hex-id> # observation log
axon trust demo                    # simulate scoring
```

Trust is subjective and experience-based — each node tracks its own observations (task success/failure, latency accuracy, optional quality signal) with exponential decay weighting. Recent outcomes matter more. The Negotiation router incorporates trust into bid scoring automatically.

## Orchestration

Define agents in TOML, orchestrate them with built-in workflow primitives:

```rust
use axon_core::orchestrate::{pipeline, fan_out, supervisor, WorkflowStep};

// Chain agents sequentially
pipeline(steps, payload, runtime).await?;

// Dispatch to multiple agents in parallel
fan_out(steps, payload, runtime).await?;

// Supervised retry with fallback
supervisor(primary, fallback, retries, runtime).await?;
```

The `Blackboard` provides CRDT-backed shared state across the workflow. `WorkflowSpan` tracks correlation IDs through distributed steps.

## Building Custom Agents

```rust
use axon_sdk::{async_trait, Agent, AgentError, Capability, TaskRequest, TaskResponse, TaskStatus};

struct MyAgent;

#[async_trait]
impl Agent for MyAgent {
    fn name(&self) -> &str { "my-agent" }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("custom", "task", 1)]
    }

    async fn handle(&self, req: TaskRequest) -> Result<TaskResponse, AgentError> {
        Ok(TaskResponse {
            request_id: req.id,
            status: TaskStatus::Success,
            payload: b"done".to_vec(),
            duration_ms: 0,
        })
    }
}
```

## TUI Dashboard

```
┌─ Axon ─────────────────────────────────────────┐
│ axon mesh  |  Peer: a3b7c9d1  |  Peers: 3      │
├─────────────────────────────────────────────────┤
│ 1 Mesh │ 2 Agents │ 3 Tasks │ 4 State │ 5 Logs │
├─────────────────────────────────────────────────┤
│ Peer ID   Address          Capabilities         │
│ a3b7c9d1  10.0.0.2:4242   llm:chat:v1          │
│ f2e8b4a0  10.0.0.3:4242   code:review:v1       │
│ 1d9c7e3f  10.0.0.4:4242   echo:ping:v1         │
├─────────────────────────────────────────────────┤
│ q: quit | Tab: switch | j/k: scroll | 1-5: tab │
└─────────────────────────────────────────────────┘
```

**Keybindings:** `q` quit, `Tab`/`Shift+Tab` switch tabs, `1-5` jump to tab, `j/k` scroll

## Build

```bash
# NixOS
nix-shell -p gcc pkg-config openssl --run "cargo build --release"

# Run tests
nix-shell -p gcc pkg-config openssl --run "cargo test"
```

## Tech Stack

| Component | Choice |
|-----------|--------|
| Language | Rust |
| Async | tokio |
| Transport | QUIC (quinn) + mTLS |
| Serialization | bincode + serde |
| Identity | Ed25519 (ed25519-dalek) |
| Discovery | mDNS (mdns-sd) |
| Persistence | sled |
| Web server | Axum |
| Web frontend | React + TypeScript |
| TUI | ratatui + crossterm |
| CLI | clap |
