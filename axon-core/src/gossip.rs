use crate::discovery::PeerTable;
use crate::mcp::McpToolSchema;
use crate::protocol::Message;
use crate::transport::Transport;
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
    /// How often to broadcast ToolCatalog (in gossip ticks). Default: 3 (~30s).
    pub tool_catalog_interval_ticks: u64,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            interval_secs: 10,
            max_peers_per_gossip: 20,
            eviction_interval_secs: 30,
            tool_catalog_interval_ticks: 3,
        }
    }
}

/// Local MCP tool catalog for gossip propagation.
/// When present, the gossip loop periodically broadcasts these tools to all peers.
pub struct LocalToolCatalog {
    pub peer_id: Vec<u8>,
    pub tools: Vec<McpToolSchema>,
}

/// Send a ToolCatalog message to a single connection.
pub async fn send_tool_catalog(
    conn: &Connection,
    peer_id: &[u8],
    tools: &[McpToolSchema],
) {
    if tools.is_empty() {
        return;
    }
    let msg = Message::ToolCatalog {
        peer_id: peer_id.to_vec(),
        tools: tools.to_vec(),
    };
    if let Err(e) = Transport::send(conn, &msg).await {
        debug!(
            "ToolCatalog send to {} failed: {}",
            conn.remote_address(),
            e
        );
    }
}

/// Broadcast a ToolCatalog message to all active connections.
pub async fn broadcast_tool_catalog(
    connections: &RwLock<Vec<(String, Connection)>>,
    peer_id: &[u8],
    tools: &[McpToolSchema],
) {
    if tools.is_empty() {
        return;
    }
    let msg = Message::ToolCatalog {
        peer_id: peer_id.to_vec(),
        tools: tools.to_vec(),
    };
    let conns = connections.read().await;
    for (addr, conn) in conns.iter() {
        if conn.close_reason().is_some() {
            continue;
        }
        if let Err(e) = Transport::send(conn, &msg).await {
            debug!("ToolCatalog broadcast to {} failed: {}", addr, e);
        }
    }
}

/// Runs the gossip protocol, periodically sharing peer lists and tool catalogs
/// with connected peers.
pub async fn run_gossip(
    peer_table: Arc<RwLock<PeerTable>>,
    _transport: Arc<Transport>,
    connections: Arc<RwLock<Vec<(String, Connection)>>>,
    config: GossipConfig,
    local_catalog: Option<LocalToolCatalog>,
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

        // Periodically broadcast ToolCatalog to all peers
        let catalog_interval = config.tool_catalog_interval_ticks.max(1);
        if gossip_tick.is_multiple_of(catalog_interval) {
            if let Some(ref catalog) = local_catalog {
                broadcast_tool_catalog(&connections, &catalog.peer_id, &catalog.tools).await;
            }
        }

        // Periodic stale peer eviction
        let eviction_every = if config.interval_secs > 0 {
            config.eviction_interval_secs / config.interval_secs
        } else {
            1
        };
        let eviction_every = eviction_every.max(1);
        if gossip_tick.is_multiple_of(eviction_every) {
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
        assert_eq!(config.tool_catalog_interval_ticks, 3);
    }

    #[test]
    fn local_tool_catalog_construction() {
        let catalog = LocalToolCatalog {
            peer_id: vec![1, 2, 3],
            tools: vec![McpToolSchema {
                name: "test-tool".to_string(),
                description: "A test tool".to_string(),
                input_schema: "{}".to_string(),
                server_name: "test-server".to_string(),
            }],
        };
        assert_eq!(catalog.tools.len(), 1);
        assert_eq!(catalog.peer_id, vec![1, 2, 3]);
    }

    #[test]
    fn local_tool_catalog_empty() {
        let catalog = LocalToolCatalog {
            peer_id: vec![1],
            tools: vec![],
        };
        assert!(catalog.tools.is_empty());
    }
}
