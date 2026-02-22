# axon

A peer-to-peer runtime for AI agents to discover, communicate, and collaborate without central infrastructure. Built in Rust with QUIC transport, capability-based routing, and CRDT shared state.

## Why

Current AI agent frameworks are centralized — a single orchestrator dispatches tasks to workers. This creates single points of failure, bottlenecks, and doesn't scale. Axon takes a different approach: agents form a self-organizing mesh where any node can initiate work, discover capabilities, and route tasks without a coordinator.

## Architecture

```
┌─────────────────────────────────────────┐
│              axon-cli                   │
│       TUI Dashboard + CLI Interface     │
├─────────────────────────────────────────┤
│              axon-core                  │
│  Discovery │ Router │ Agent Runtime     │
│  Transport (QUIC) │ Identity (Ed25519)  │
│  Shared State (CRDTs)                   │
└─────────────────────────────────────────┘
```

**Core components:**

- **Identity** — Ed25519 keypairs for node authentication and message signing
- **Transport** — QUIC via `quinn` for multiplexed, encrypted peer communication
- **Protocol** — Binary message format (bincode) with Ping/Pong, Announce, Discover, TaskRequest/TaskResponse, Gossip, StateSync
- **Discovery** — mDNS for LAN, gossip protocol for mesh-wide peer propagation
- **Router** — Capability-based routing with BestMatch, RoundRobin, and Broadcast strategies
- **Runtime** — Async agent executor with pluggable Agent trait
- **CRDTs** — GCounter, LWWRegister, ORSet for eventually-consistent shared state

## Quick Start

```bash
# Start a mesh node with TUI dashboard
axon start

# Start on a specific port
axon start --listen 0.0.0.0:4242

# Connect to an existing node
axon start --peer 192.168.1.100:4242

# Send a task to a peer
axon send --peer 127.0.0.1:4242 --namespace echo --name ping --data "hello"

# View your identity
axon identity
```

## Built-in Agents

| Agent | Capability | Description |
|-------|-----------|-------------|
| Echo | `echo:ping:v1` | Returns input payload (testing/diagnostics) |
| SystemInfo | `system:info:v1` | Returns hostname, OS, architecture |
| LLM | `llm:chat:v1` | Proxies to local Ollama instance |

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

# Run tests (101 tests)
nix-shell -p gcc pkg-config openssl --run "cargo test"
```

## Tech Stack

| Component | Choice |
|-----------|--------|
| Language | Rust |
| Async | tokio |
| Transport | QUIC (quinn) |
| Serialization | bincode + serde |
| Identity | Ed25519 (ed25519-dalek) |
| Discovery | mDNS (mdns-sd) |
| TUI | ratatui + crossterm |
| CLI | clap |
