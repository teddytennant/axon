use crate::protocol::{Capability, PeerInfo};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;

/// Peer liveness timeout — peers not seen within this window are considered dead.
const PEER_TIMEOUT_SECS: u64 = 60;

/// Manages discovery and tracking of peers in the mesh.
pub struct PeerTable {
    peers: HashMap<Vec<u8>, PeerInfo>,
    /// Local peer info.
    local_peer: PeerInfo,
}

impl PeerTable {
    pub fn new(local_peer: PeerInfo) -> Self {
        Self {
            peers: HashMap::new(),
            local_peer,
        }
    }

    /// Get the local peer info.
    pub fn local_peer(&self) -> &PeerInfo {
        &self.local_peer
    }

    /// Update local peer capabilities.
    pub fn set_local_capabilities(&mut self, caps: Vec<Capability>) {
        self.local_peer.capabilities = caps;
    }

    /// Update local peer's last_seen timestamp to now.
    pub fn touch_local(&mut self) {
        self.local_peer.last_seen = now_secs();
    }

    /// Add or update a peer. Returns true if this is a new peer.
    pub fn upsert(&mut self, info: PeerInfo) -> bool {
        // Don't add ourselves
        if info.peer_id == self.local_peer.peer_id {
            return false;
        }
        let is_new = !self.peers.contains_key(&info.peer_id);
        if is_new {
            info!(
                "Discovered new peer: {} at {}",
                short_id(&info.peer_id),
                info.addr
            );
        }
        self.peers.insert(info.peer_id.clone(), info);
        is_new
    }

    /// Remove a peer by ID.
    pub fn remove(&mut self, peer_id: &[u8]) {
        self.peers.remove(peer_id);
    }

    /// Get a peer by ID.
    pub fn get(&self, peer_id: &[u8]) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    /// Get all known peers.
    pub fn all_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Get all peers as owned PeerInfo vec (for gossip).
    pub fn all_peers_owned(&self) -> Vec<PeerInfo> {
        self.peers.values().cloned().collect()
    }

    /// Number of known peers.
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Find peers with a specific capability.
    pub fn find_by_capability(&self, capability: &Capability) -> Vec<&PeerInfo> {
        self.peers
            .values()
            .filter(|p| p.capabilities.iter().any(|c| c.matches(capability)))
            .collect()
    }

    /// Remove peers that haven't been seen within the timeout window.
    pub fn evict_stale(&mut self) -> Vec<PeerInfo> {
        let cutoff = now_secs().saturating_sub(PEER_TIMEOUT_SECS);
        let stale: Vec<Vec<u8>> = self
            .peers
            .iter()
            .filter(|(_, p)| p.last_seen < cutoff)
            .map(|(id, _)| id.clone())
            .collect();

        let mut evicted = Vec::new();
        for id in stale {
            if let Some(peer) = self.peers.remove(&id) {
                info!(
                    "Evicted stale peer: {} at {}",
                    short_id(&peer.peer_id),
                    peer.addr
                );
                evicted.push(peer);
            }
        }
        evicted
    }

    /// Touch a peer's last_seen timestamp.
    pub fn touch(&mut self, peer_id: &[u8]) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.last_seen = now_secs();
        }
    }

    /// Merge a list of peers from gossip.
    pub fn merge_gossip(&mut self, peers: Vec<PeerInfo>) -> usize {
        let mut new_count = 0;
        for peer in peers {
            if self.upsert(peer) {
                new_count += 1;
            }
        }
        new_count
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn short_id(id: &[u8]) -> String {
    id.iter()
        .take(4)
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_peer() -> PeerInfo {
        PeerInfo {
            peer_id: vec![0, 0, 0, 1],
            addr: "127.0.0.1:4242".to_string(),
            capabilities: vec![],
            last_seen: now_secs(),
        }
    }

    fn make_peer(id: u8) -> PeerInfo {
        PeerInfo {
            peer_id: vec![0, 0, 0, id],
            addr: format!("127.0.0.1:{}", 5000 + id as u16),
            capabilities: vec![],
            last_seen: now_secs(),
        }
    }

    fn make_peer_with_caps(id: u8, caps: Vec<Capability>) -> PeerInfo {
        PeerInfo {
            peer_id: vec![0, 0, 0, id],
            addr: format!("127.0.0.1:{}", 5000 + id as u16),
            capabilities: caps,
            last_seen: now_secs(),
        }
    }

    #[test]
    fn peer_table_starts_empty() {
        let table = PeerTable::new(local_peer());
        assert_eq!(table.len(), 0);
        assert!(table.is_empty());
    }

    #[test]
    fn peer_table_add_peer() {
        let mut table = PeerTable::new(local_peer());
        let is_new = table.upsert(make_peer(2));
        assert!(is_new);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn peer_table_update_peer() {
        let mut table = PeerTable::new(local_peer());
        table.upsert(make_peer(2));
        let is_new = table.upsert(make_peer(2));
        assert!(!is_new);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn peer_table_does_not_add_self() {
        let local = local_peer();
        let mut table = PeerTable::new(local.clone());
        let is_new = table.upsert(local);
        assert!(!is_new);
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn peer_table_remove_peer() {
        let mut table = PeerTable::new(local_peer());
        table.upsert(make_peer(2));
        table.remove(&[0, 0, 0, 2]);
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn peer_table_get_peer() {
        let mut table = PeerTable::new(local_peer());
        table.upsert(make_peer(2));
        let peer = table.get(&[0, 0, 0, 2]);
        assert!(peer.is_some());
        assert_eq!(peer.unwrap().addr, "127.0.0.1:5002");
    }

    #[test]
    fn peer_table_get_missing() {
        let table = PeerTable::new(local_peer());
        assert!(table.get(&[0, 0, 0, 99]).is_none());
    }

    #[test]
    fn peer_table_find_by_capability() {
        let mut table = PeerTable::new(local_peer());
        let cap = Capability::new("llm", "chat", 1);
        table.upsert(make_peer_with_caps(2, vec![cap.clone()]));
        table.upsert(make_peer_with_caps(
            3,
            vec![Capability::new("code", "review", 1)],
        ));
        table.upsert(make_peer_with_caps(4, vec![cap.clone()]));

        let found = table.find_by_capability(&cap);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn peer_table_find_no_capability() {
        let mut table = PeerTable::new(local_peer());
        table.upsert(make_peer_with_caps(
            2,
            vec![Capability::new("llm", "chat", 1)],
        ));
        let found = table.find_by_capability(&Capability::new("code", "review", 1));
        assert_eq!(found.len(), 0);
    }

    #[test]
    fn peer_table_evict_stale() {
        let mut table = PeerTable::new(local_peer());
        // Add a peer that's very old
        let mut old_peer = make_peer(2);
        old_peer.last_seen = 1000; // way in the past
        table.upsert(old_peer);

        // Add a fresh peer
        table.upsert(make_peer(3));

        let evicted = table.evict_stale();
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].peer_id, vec![0, 0, 0, 2]);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn peer_table_evict_none_stale() {
        let mut table = PeerTable::new(local_peer());
        table.upsert(make_peer(2));
        let evicted = table.evict_stale();
        assert_eq!(evicted.len(), 0);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn peer_table_touch() {
        let mut table = PeerTable::new(local_peer());
        let mut peer = make_peer(2);
        peer.last_seen = 1000;
        table.upsert(peer);

        table.touch(&[0, 0, 0, 2]);
        let updated = table.get(&[0, 0, 0, 2]).unwrap();
        assert!(updated.last_seen > 1000);
    }

    #[test]
    fn peer_table_merge_gossip() {
        let mut table = PeerTable::new(local_peer());
        table.upsert(make_peer(2));

        let gossip_peers = vec![make_peer(3), make_peer(4), make_peer(2)];
        let new_count = table.merge_gossip(gossip_peers);
        assert_eq!(new_count, 2); // 3 and 4 are new
        assert_eq!(table.len(), 3);
    }

    #[test]
    fn peer_table_all_peers() {
        let mut table = PeerTable::new(local_peer());
        table.upsert(make_peer(2));
        table.upsert(make_peer(3));
        assert_eq!(table.all_peers().len(), 2);
    }

    #[test]
    fn peer_table_set_local_capabilities() {
        let mut table = PeerTable::new(local_peer());
        assert!(table.local_peer().capabilities.is_empty());
        table.set_local_capabilities(vec![Capability::new("test", "echo", 1)]);
        assert_eq!(table.local_peer().capabilities.len(), 1);
    }

    #[test]
    fn peer_table_local_peer() {
        let local = local_peer();
        let table = PeerTable::new(local.clone());
        assert_eq!(table.local_peer().peer_id, local.peer_id);
    }
}
