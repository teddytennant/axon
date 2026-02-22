# Test Suite

## Running Tests

```bash
# NixOS
nix-shell -p gcc pkg-config openssl --run "cargo test"

# Ubuntu / standard Linux
cargo test
```

## Test Breakdown

### axon-core — 108 unit tests

| Module | Tests | What's Covered |
|--------|-------|----------------|
| `protocol` | 11 | Message encode/decode roundtrips, capability matching, version semantics, invalid data handling |
| `identity` | 11 | Key generation, serialization roundtrip, sign/verify, wrong key rejection, file persistence |
| `crdt` | 28 | GCounter increment/merge/idempotency, LWWRegister timestamp ordering, ORSet add/remove/concurrent merge |
| `router` | 16 | Peer add/remove/update, capability search, BestMatch scoring, RoundRobin cycling, Broadcast, stats tracking |
| `runtime` | 8 | Agent registration, capability dispatch, no-match handling, failing agent, duration tracking |
| `discovery` | 15 | Peer table CRUD, capability lookup, stale eviction, gossip merge, self-exclusion |
| `mdns` | 6 | Capability tag parsing (empty, single, multiple, malformed), hex decode |
| `gossip` | 1 | Config defaults |
| `transport` | 4 | Bind, connect+exchange, connection counting, bidirectional communication |

### axon-core — 4 integration tests

| Test | What It Does |
|------|-------------|
| `two_node_task_exchange` | Two QUIC nodes connect, announce, send a task, receive response |
| `peer_table_full_workflow` | Gossip merge, capability search, stale peer eviction |
| `runtime_multi_agent_dispatch` | Echo + Uppercase agents dispatched by capability, no-match returns NoCapability |
| `crdt_convergence_simulation` | 3-node GCounter + ORSet converge after gossip-style merges |

### axon-cli — 9 unit tests

| Test | What It Does |
|------|-------------|
| `provider_kind_from_str` | Parses "ollama", "openai", "xai", "grok", "openrouter", "custom", rejects invalid |
| `provider_kind_display` | Display formatting for all provider kinds |
| `default_models_not_empty` | Every provider has a non-empty default model |
| `default_endpoints_not_empty` | Every provider has a non-empty default endpoint |
| `build_ollama_no_key_needed` | Ollama builds without API key |
| `build_openai_requires_key` | OpenAI rejects missing key |
| `build_xai_requires_key` | XAI rejects missing key |
| `build_openrouter_requires_key` | OpenRouter rejects missing key |
| `build_with_valid_keys` | All key-based providers build successfully with keys |

## Test Properties

- **No network required**: All tests use loopback (`127.0.0.1:0`) with ephemeral ports
- **No external services**: No Ollama, no API keys, no mDNS daemon needed
- **Deterministic**: No flaky tests, no timeouts under normal conditions
- **Fast**: Full suite runs in <1 second (excluding compile time)

## Adding Tests

Unit tests live alongside the code in `#[cfg(test)] mod tests` blocks. Integration tests go in `axon-core/tests/integration.rs`.

Pattern for a new agent test:

```rust
#[tokio::test]
async fn my_agent_test() {
    let rt = Runtime::new();
    rt.register(Arc::new(MyAgent)).await;

    let req = TaskRequest {
        id: Uuid::new_v4(),
        capability: Capability::new("my", "cap", 1),
        payload: b"input".to_vec(),
        timeout_ms: 1000,
    };

    let resp = rt.dispatch(req).await;
    assert_eq!(resp.status, TaskStatus::Success);
}
```
