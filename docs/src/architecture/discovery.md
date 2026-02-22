# Discovery

Axon uses two discovery mechanisms: mDNS for the local network and gossip for the wider mesh.

## mDNS

On startup, each node registers an mDNS service:

- Service type: `_axon._udp.local.`
- Instance name: `axon-{short_peer_id}`
- Properties: `peer_id`, `caps` (comma-separated capability tags)

When a new mDNS service is resolved, the node:
1. Adds the peer to the peer table
2. Opens a QUIC connection
3. Sends an `Announce` message

mDNS is zero-configuration — nodes on the same LAN discover each other automatically.

## Peer Table

The peer table tracks all known peers:

```rust
struct PeerTable {
    local_peer: PeerInfo,
    peers: HashMap<Vec<u8>, PeerInfo>,
}
```

Each `PeerInfo` contains:
- `peer_id`: Public key bytes
- `addr`: Socket address string
- `capabilities`: Vector of capabilities
- `last_seen`: Unix timestamp

## Stale Peer Eviction

Peers not seen for 60+ seconds are evicted during periodic cleanup. The local node's timestamp is refreshed before each eviction cycle.

## Bootstrap Peers

For nodes not on the same LAN, use `--peer` to specify bootstrap addresses:

```bash
axon-cli start --peer 203.0.113.1:4242
```
