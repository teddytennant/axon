# Axon — Decentralized AI Agent Mesh Protocol

## Overview

Axon is a peer-to-peer runtime for AI agents to discover, communicate, and collaborate without central infrastructure. Each node in the mesh runs a lightweight Rust runtime that advertises capabilities, routes tasks to the best available agent, and maintains shared state via CRDTs.

Think of it as the nervous system for distributed AI — axons are the transmission pathways between neurons, and this project is the transmission layer between intelligent agents.

## Motivation

Current AI agent frameworks are centralized: a single orchestrator dispatches tasks to workers. This creates single points of failure, bottlenecks, and doesn't scale. Axon takes a different approach — agents form a self-organizing mesh where any node can initiate work, discover capabilities, and route tasks without a coordinator.

This is the infrastructure layer needed for Recursive Self-Improvement at scale: agents that can find and leverage each other's capabilities autonomously.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                    axon-cli                          │
│           TUI Dashboard + CLI Interface              │
├─────────────────────────────────────────────────────┤
│                    axon-core                         │
│  ┌──────────┐ ┌──────────┐ ┌───────────────────┐   │
│  │ Discovery │ │ Router   │ │  Agent Runtime    │   │
│  │ (mDNS)   │ │ (Cap-    │ │  (Task Executor)  │   │
│  │          │ │  based)  │ │                   │   │
│  └────┬─────┘ └────┬─────┘ └────────┬──────────┘   │
│       │            │               │               │
│  ┌────┴────────────┴───────────────┴────────────┐   │
│  │              Transport (QUIC)                 │   │
│  │         Identity (Ed25519 Keypairs)           │   │
│  └──────────────────────────────────────────────┘   │
│  ┌──────────────────────────────────────────────┐   │
│  │           Shared State (CRDTs)                │   │
│  │    GCounter · LWWRegister · ORSet             │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

## Core Components

### 1. Identity (`identity.rs`)
- Ed25519 keypair generation and management
- Peer IDs derived from public keys (base58-encoded)
- Message signing and verification
- Persistent identity stored in `~/.axon/identity.key`

### 2. Transport (`transport.rs`)
- QUIC-based connections via the `quinn` crate
- Bidirectional streaming between peers
- Self-signed TLS certificates derived from identity keys
- Connection pooling and automatic reconnection
- Multiplexed streams for concurrent message exchange

### 3. Protocol (`protocol.rs`)
- Binary message format using `bincode` + `serde`
- Message types:
  - `Ping` / `Pong` — liveness checks
  - `Announce` — broadcast capabilities to mesh
  - `Discover` — query mesh for specific capabilities
  - `TaskRequest` — send a task to a peer
  - `TaskResponse` — return task results
  - `StateSync` — CRDT state synchronization
  - `Gossip` — propagate peer/capability information

### 4. Discovery (`discovery.rs`)
- mDNS for zero-config LAN discovery via `mdns-sd`
- Gossip protocol for propagating peer information beyond LAN
- Peer table with heartbeat-based liveness tracking
- Capability index: maps capability tags to peer IDs

### 5. Router (`router.rs`)
- Capability-based task routing
- Agents advertise capabilities as typed tags (e.g., `llm:chat`, `code:review`, `embed:text`)
- Router scores peers by: capability match, latency, load, success rate
- Strategies: `BestMatch`, `RoundRobin`, `Broadcast`
- Fallback chain when primary peer is unavailable

### 6. Agent Runtime (`runtime.rs`)
- Async task executor built on tokio
- Agent trait:
  ```rust
  #[async_trait]
  pub trait Agent: Send + Sync {
      fn capabilities(&self) -> Vec<Capability>;
      async fn handle(&self, task: TaskRequest) -> Result<TaskResponse>;
  }
  ```
- Built-in agents:
  - `EchoAgent` — returns input (testing/diagnostics)
  - `LlmAgent` — proxies to local Ollama or OpenRouter
  - `SystemInfoAgent` — returns system metrics

### 7. Shared State (`crdt.rs`)
- CRDT implementations for eventually-consistent shared state:
  - `GCounter` — grow-only counter (task counts, metrics)
  - `LWWRegister<T>` — last-writer-wins register (config values)
  - `ORSet<T>` — observed-remove set (capability sets, peer lists)
- Automatic state sync via gossip protocol
- Merge function for each CRDT type

### 8. TUI Dashboard (`tui.rs`)
- Built with `ratatui` + `crossterm`
- Tabs:
  - **Mesh** — visual map of connected peers, connection status
  - **Agents** — registered agents and their capabilities
  - **Tasks** — live task feed with status, routing info, latency
  - **State** — CRDT shared state viewer
  - **Logs** — scrollable log output
- Vim-style navigation (j/k scroll, tab switching, q quit)

## Tech Stack

| Component | Choice | Justification |
|-----------|--------|---------------|
| Language | Rust | Memory safety, async performance, type system |
| Async Runtime | tokio | Industry standard, mature ecosystem |
| Transport | quinn (QUIC) | Multiplexed streams, built-in encryption, UDP-based |
| Serialization | serde + bincode | Fast binary serialization, zero-copy where possible |
| Identity | ed25519-dalek | Standard EdDSA, compact signatures |
| Discovery | mdns-sd | Zero-config LAN discovery |
| TUI | ratatui + crossterm | Proven TUI stack, cross-platform |
| CLI | clap | Standard Rust CLI framework |
| Logging | tracing | Structured, async-aware logging |

## Module Structure

```
axon/
├── Cargo.toml              # Workspace definition
├── SPEC.md
├── README.md
├── axon-core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # Public API re-exports
│       ├── identity.rs     # Ed25519 keypair management
│       ├── transport.rs    # QUIC transport layer
│       ├── protocol.rs     # Message types and serialization
│       ├── discovery.rs    # mDNS + gossip discovery
│       ├── router.rs       # Capability-based routing
│       ├── runtime.rs      # Agent runtime and task execution
│       └── crdt.rs         # CRDT implementations
├── axon-cli/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs         # CLI entry point
│       ├── tui.rs          # TUI dashboard
│       └── agents.rs       # Built-in agent implementations
└── axon-sdk/
    ├── Cargo.toml
    └── src/
        └── lib.rs          # Public SDK for building custom agents
```

## API Contracts

### Agent Trait (SDK)
```rust
#[async_trait]
pub trait Agent: Send + Sync + 'static {
    /// Unique name for this agent type
    fn name(&self) -> &str;

    /// Capabilities this agent provides
    fn capabilities(&self) -> Vec<Capability>;

    /// Handle an incoming task request
    async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError>;
}
```

### Core Types
```rust
pub struct PeerId(pub [u8; 32]); // Ed25519 public key

pub struct Capability {
    pub namespace: String,  // e.g., "llm", "code", "embed"
    pub name: String,       // e.g., "chat", "review", "text"
    pub version: u32,       // capability version
}

pub struct TaskRequest {
    pub id: Uuid,
    pub capability: Capability,
    pub payload: Vec<u8>,
    pub timeout_ms: u64,
}

pub struct TaskResponse {
    pub request_id: Uuid,
    pub status: TaskStatus,
    pub payload: Vec<u8>,
    pub duration_ms: u64,
}

pub enum TaskStatus {
    Success,
    Error(String),
    Timeout,
    NoCapability,
}
```

### Node Configuration
```rust
pub struct NodeConfig {
    pub listen_addr: SocketAddr,     // default: 0.0.0.0:4242
    pub identity_path: PathBuf,      // default: ~/.axon/identity.key
    pub enable_mdns: bool,           // default: true
    pub bootstrap_peers: Vec<SocketAddr>,
    pub max_connections: usize,      // default: 64
    pub task_timeout_ms: u64,        // default: 30000
}
```

## MVP Scope

### In scope:
- Ed25519 identity generation and persistence
- QUIC transport with connection management
- Binary protocol with all message types
- mDNS local discovery
- Capability-based routing (BestMatch strategy)
- Agent trait and runtime
- 3 built-in agents (Echo, LLM proxy, SystemInfo)
- CRDT shared state (GCounter, LWWRegister, ORSet)
- TUI dashboard with all 5 tabs
- CLI with `start`, `status`, `peers`, `send` commands

### Out of scope (post-MVP):
- Internet-scale DHT discovery
- Persistent task queues
- Agent hot-reloading
- Multi-hop routing
- Encryption at rest
- Web dashboard
