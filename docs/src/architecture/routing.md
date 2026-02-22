# Routing

The router matches task requests to capable peers using capability-based routing.

## Strategies

### BestMatch

Selects the single best peer based on a composite score:

```
score = 0.6 * success_rate + 0.4 * latency_factor
```

Where:
- `success_rate` = successful_tasks / total_tasks
- `latency_factor` = 1.0 / (1.0 + avg_latency_ms / 1000.0)

Peers with no history get a neutral score of 0.5.

### RoundRobin

Cycles through capable peers sequentially.

### Broadcast

Returns all capable peers (for fan-out operations).

## Peer Stats

The router tracks per-peer statistics:

```rust
struct PeerStats {
    total_tasks: u64,
    successful_tasks: u64,
    total_latency_ms: u64,
}
```

Stats are updated after each task response.

## Version Matching

A capability request for version N matches any peer advertising version N or higher. This allows backward-compatible agent upgrades.
