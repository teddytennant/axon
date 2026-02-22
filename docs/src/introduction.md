# Axon

**Decentralized AI Agent Mesh Protocol**

Axon is a peer-to-peer runtime for AI agents. Nodes discover each other on the local network via mDNS, form a mesh through gossip protocol, and route tasks to agents based on capabilities.

## What Axon Does

- **Peer-to-peer mesh**: Nodes connect over QUIC with mutual TLS, forming a self-organizing network
- **Capability-based routing**: Each agent advertises what it can do; tasks are routed to the right agent automatically
- **Zero-config discovery**: mDNS finds peers on the LAN without any central server
- **Gossip protocol**: Peer information propagates across the mesh beyond direct connections
- **CRDTs**: Eventually-consistent shared state without coordination
- **Multi-provider LLM**: Route AI tasks to Ollama, OpenAI, XAI (Grok), OpenRouter, or any OpenAI-compatible endpoint

## Architecture at a Glance

```
┌─────────────────────────────────────┐
│            Axon Node                │
│  ┌───────────────────────────────┐  │
│  │          Runtime              │  │
│  │  ┌─────┐ ┌─────┐ ┌────────┐  │  │
│  │  │Echo │ │ LLM │ │SysInfo │  │  │
│  │  └─────┘ └─────┘ └────────┘  │  │
│  └───────────────────────────────┘  │
│  ┌──────────┐  ┌─────────────────┐  │
│  │ Router   │  │  Peer Table     │  │
│  └──────────┘  └─────────────────┘  │
│  ┌──────────┐  ┌─────────────────┐  │
│  │Transport │  │ mDNS + Gossip   │  │
│  │  (QUIC)  │  │                 │  │
│  └──────────┘  └─────────────────┘  │
└─────────────────────────────────────┘
```

## Crate Structure

| Crate | Purpose |
|-------|---------|
| `axon-core` | Protocol, transport, identity, routing, CRDTs, discovery, runtime |
| `axon-cli` | CLI binary with TUI dashboard and built-in agents |
| `axon-sdk` | Public SDK for building custom agents |
