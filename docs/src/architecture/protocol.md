# Protocol

The Axon wire protocol uses binary serialization over QUIC streams.

## Message Types

```rust
enum Message {
    Ping { nonce: u64 },
    Pong { nonce: u64 },
    Announce(PeerInfo),
    Discover { capability: Capability },
    DiscoverResponse { peers: Vec<PeerInfo> },
    TaskRequest(TaskRequest),
    TaskResponse(TaskResponse),
    StateSync { key: String, value: Vec<u8> },
    Gossip { peers: Vec<PeerInfo> },
}
```

## Capabilities

Capabilities are the routing primitive. Each capability has:

| Field | Type | Example |
|-------|------|---------|
| `namespace` | String | `"llm"` |
| `name` | String | `"chat"` |
| `version` | u32 | `1` |

Capabilities are encoded as tags: `llm:chat:v1`

Version matching is forward-compatible: a request for `v1` matches agents with `v1` or higher.

## Task Lifecycle

```
Client                          Agent Node
  │                                 │
  │──── TaskRequest ───────────────>│
  │     (id, capability, payload)   │
  │                                 │── dispatch to agent
  │                                 │
  │<─── TaskResponse ──────────────│
  │     (status, payload, duration) │
```

## Framing

Messages are length-prefixed:

```
[4 bytes: payload length (u32 big-endian)] [N bytes: bincode-encoded Message]
```

Maximum message size: 16 MB.
