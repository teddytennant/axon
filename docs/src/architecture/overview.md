# Architecture Overview

Axon is a three-crate Rust workspace designed for decentralized AI agent coordination.

## Layers

```
┌────────────────────────────────────────┐
│  Application (axon-cli)                │
│  - TUI Dashboard                       │
│  - CLI Commands                        │
│  - Built-in Agents (Echo, LLM, Sys)   │
│  - LLM Provider System                │
├────────────────────────────────────────┤
│  SDK (axon-sdk)                        │
│  - Agent trait re-exports              │
│  - Type-safe task handling             │
├────────────────────────────────────────┤
│  Core (axon-core)                      │
│  - Protocol (messages, capabilities)   │
│  - Transport (QUIC + TLS)             │
│  - Identity (Ed25519 keypairs)        │
│  - Discovery (peer table + mDNS)      │
│  - Gossip (peer propagation)          │
│  - Router (capability matching)       │
│  - Runtime (agent dispatch)           │
│  - CRDTs (GCounter, LWW, ORSet)     │
└────────────────────────────────────────┘
```

## Data Flow

1. Node boots, generates/loads Ed25519 identity
2. QUIC transport binds to a port with self-signed TLS
3. mDNS broadcasts the node's presence on the LAN
4. Discovered peers are added to the peer table
5. Gossip protocol shares peer lists periodically
6. Incoming `TaskRequest` messages are dispatched to matching agents
7. Responses flow back over the same QUIC connection

## Key Design Decisions

- **QUIC over TCP**: Multiplexed streams, built-in TLS, connection migration
- **Capability-based routing**: No hardcoded service names — agents declare what they can do
- **CRDTs for state**: No consensus protocol needed; eventual consistency via merge
- **Binary protocol**: `bincode` serialization for speed; length-prefixed framing
- **Trait-based agents**: Any struct implementing `Agent` can be registered at runtime
