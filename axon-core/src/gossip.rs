use crate::protocol::Message;
use crate::transport::Transport;
use crate::discovery::PeerTable;
use quinn::Connection;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Configuration for the gossip protocol.
pub struct GossipConfig {
    /// How often to gossip (in seconds).
    pub interval_secs: u64,
    /// Maximum number of peers to include in each gossip message.
    pub max_peers_per_gossip: usize,
    /// How often to evict stale peers (in seconds).
    pub eviction_interval_secs: u64,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            interval_secs: 10,
            max_peers_per_gossip: 20,
            eviction_interval_secs: 30,
        }
    }
}

/// Runs the gossip protocol, periodically sharing peer lists with connected peers.
pub async fn run_gossip(
    peer_table: Arc<RwLock<PeerTable>>,
    _transport: Arc<Transport>,
    connections: Arc<RwLock<Vec<(String, Connection)>>>,
    config: GossipConfig,
) {
    let mut gossip_tick = 0u64;

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(config.interval_secs)).await;
        gossip_tick += 1;

        // Get current peer list
        let peers = {
            let table = peer_table.read().await;
            let mut peers = table.all_peers_owned();
            // Include ourselves
            let local = table.local_peer().clone();
            peers.push(local);
            // Limit gossip size
            peers.truncate(config.max_peers_per_gossip);
            peers
        };

        // Send gossip to all connections
        let conns = connections.read().await;
        for (addr, conn) in conns.iter() {
            if conn.close_reason().is_some() {
                continue;
            }
            let msg = Message::Gossip {
                peers: peers.clone(),
            };
            if let Err(e) = Transport::send(conn, &msg).await {
                debug!("Gossip to {} failed: {}", addr, e);
            }
        }
        drop(conns);

        // Periodic stale peer eviction
        let eviction_every = if config.interval_secs > 0 {
            config.eviction_interval_secs / config.interval_secs
        } else {
            1
        };
        let eviction_every = eviction_every.max(1);
        if gossip_tick % eviction_every == 0 {
            let mut table = peer_table.write().await;
            table.touch_local();
            let evicted = table.evict_stale();
            if !evicted.is_empty() {
                info!("Evicted {} stale peers", evicted.len());
            }
        }

        // Ping all connections for liveness
        let conns = connections.read().await;
        for (addr, conn) in conns.iter() {
            if conn.close_reason().is_some() {
                continue;
            }
            let nonce = gossip_tick;
            let msg = Message::Ping { nonce };
            if let Err(e) = Transport::send(conn, &msg).await {
                debug!("Ping to {} failed: {}", addr, e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gossip_config_defaults() {
        let config = GossipConfig::default();
        assert_eq!(config.interval_secs, 10);
        assert_eq!(config.max_peers_per_gossip, 20);
        assert_eq!(config.eviction_interval_secs, 30);
    }
}
