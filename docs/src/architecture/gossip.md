# Gossip Protocol

The gossip protocol propagates peer information across the mesh beyond direct connections.

## How It Works

Every 10 seconds (configurable), each node:

1. Collects its current peer list (including itself)
2. Truncates to `max_peers_per_gossip` entries (default: 20)
3. Sends a `Gossip` message to all active connections
4. Pings all connections for liveness detection

## Peer Eviction

Every 30 seconds, stale peers (not seen for 60+ seconds) are evicted from the peer table.

## Configuration

```rust
struct GossipConfig {
    interval_secs: u64,           // default: 10
    max_peers_per_gossip: usize,  // default: 20
    eviction_interval_secs: u64,  // default: 30
}
```

## Convergence

Through repeated gossip rounds, all nodes eventually learn about all other nodes in the mesh. A new node joining through any single connection will have its information propagated to the entire mesh within a few gossip cycles.
