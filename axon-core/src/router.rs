use crate::protocol::{Capability, PeerInfo};
use std::collections::HashMap;

/// Routing strategy for task dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Route to the best-scoring peer.
    BestMatch,
    /// Round-robin across matching peers.
    RoundRobin,
    /// Broadcast to all matching peers.
    Broadcast,
}

/// Tracks peer performance for routing decisions.
#[derive(Debug, Clone)]
pub struct PeerStats {
    pub total_tasks: u64,
    pub successful_tasks: u64,
    pub total_latency_ms: u64,
    pub last_seen: u64,
}

impl PeerStats {
    pub fn new() -> Self {
        Self {
            total_tasks: 0,
            successful_tasks: 0,
            total_latency_ms: 0,
            last_seen: 0,
        }
    }

    /// Average latency in ms, or u64::MAX if no successful tasks.
    pub fn avg_latency_ms(&self) -> u64 {
        if self.successful_tasks == 0 {
            return u64::MAX;
        }
        self.total_latency_ms / self.successful_tasks
    }

    /// Success rate as a float [0.0, 1.0].
    pub fn success_rate(&self) -> f64 {
        if self.total_tasks == 0 {
            return 0.0;
        }
        self.successful_tasks as f64 / self.total_tasks as f64
    }

    /// Composite score for routing (higher is better).
    pub fn score(&self) -> f64 {
        if self.total_tasks == 0 {
            // New peer gets a neutral score
            return 0.5;
        }
        let latency_factor = 1.0 / (1.0 + self.avg_latency_ms() as f64 / 1000.0);
        let success_factor = self.success_rate();
        // Weighted: 60% success, 40% latency
        0.6 * success_factor + 0.4 * latency_factor
    }

    pub fn record_success(&mut self, latency_ms: u64) {
        self.total_tasks = self.total_tasks.saturating_add(1);
        self.successful_tasks = self.successful_tasks.saturating_add(1);
        self.total_latency_ms = self.total_latency_ms.saturating_add(latency_ms);
    }

    pub fn record_failure(&mut self) {
        self.total_tasks = self.total_tasks.saturating_add(1);
    }
}

impl Default for PeerStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Routes tasks to peers based on capabilities and performance.
pub struct Router {
    /// Known peers and their info.
    peers: HashMap<Vec<u8>, PeerInfo>,
    /// Performance stats per peer.
    stats: HashMap<Vec<u8>, PeerStats>,
    /// Round-robin index per capability tag.
    rr_index: HashMap<String, usize>,
    /// Default routing strategy.
    pub strategy: Strategy,
}

impl Router {
    pub fn new(strategy: Strategy) -> Self {
        Self {
            peers: HashMap::new(),
            stats: HashMap::new(),
            rr_index: HashMap::new(),
            strategy,
        }
    }

    /// Register or update a peer.
    pub fn update_peer(&mut self, info: PeerInfo) {
        self.peers.insert(info.peer_id.clone(), info);
    }

    /// Remove a peer.
    pub fn remove_peer(&mut self, peer_id: &[u8]) {
        self.peers.remove(peer_id);
        self.stats.remove(peer_id);
    }

    /// Get all known peers.
    pub fn peers(&self) -> Vec<&PeerInfo> {
        self.peers.values().collect()
    }

    /// Find peers that match a requested capability.
    pub fn find_capable_peers(&self, capability: &Capability) -> Vec<&PeerInfo> {
        self.peers
            .values()
            .filter(|peer| peer.capabilities.iter().any(|c| c.matches(capability)))
            .collect()
    }

    /// Route a task to the best peer(s) for a given capability.
    pub fn route(&mut self, capability: &Capability) -> Vec<Vec<u8>> {
        let capable: Vec<Vec<u8>> = self
            .peers
            .values()
            .filter(|peer| peer.capabilities.iter().any(|c| c.matches(capability)))
            .map(|p| p.peer_id.clone())
            .collect();

        if capable.is_empty() {
            return vec![];
        }

        match self.strategy {
            Strategy::BestMatch => {
                let mut scored: Vec<(Vec<u8>, f64)> = capable
                    .into_iter()
                    .map(|id| {
                        let score = self
                            .stats
                            .get(&id)
                            .map(|s| s.score())
                            .unwrap_or(0.5);
                        (id, score)
                    })
                    .collect();
                scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                vec![scored[0].0.clone()]
            }
            Strategy::RoundRobin => {
                let tag = capability.tag();
                let idx = self.rr_index.entry(tag).or_insert(0);
                let chosen = capable[*idx % capable.len()].clone();
                *idx = (*idx + 1) % capable.len();
                vec![chosen]
            }
            Strategy::Broadcast => capable,
        }
    }

    /// Record a successful task completion.
    pub fn record_success(&mut self, peer_id: &[u8], latency_ms: u64) {
        self.stats
            .entry(peer_id.to_vec())
            .or_default()
            .record_success(latency_ms);
    }

    /// Record a task failure.
    pub fn record_failure(&mut self, peer_id: &[u8]) {
        self.stats
            .entry(peer_id.to_vec())
            .or_default()
            .record_failure();
    }

    /// Get stats for a peer.
    pub fn get_stats(&self, peer_id: &[u8]) -> Option<&PeerStats> {
        self.stats.get(peer_id)
    }

    /// Number of known peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_peer(id: u8, caps: Vec<Capability>) -> PeerInfo {
        PeerInfo {
            peer_id: vec![id],
            addr: format!("127.0.0.1:{}", 4242 + id as u16),
            capabilities: caps,
            last_seen: 1000,
        }
    }

    #[test]
    fn router_empty_has_no_peers() {
        let r = Router::new(Strategy::BestMatch);
        assert_eq!(r.peer_count(), 0);
    }

    #[test]
    fn router_add_and_count_peers() {
        let mut r = Router::new(Strategy::BestMatch);
        r.update_peer(make_peer(1, vec![]));
        r.update_peer(make_peer(2, vec![]));
        assert_eq!(r.peer_count(), 2);
    }

    #[test]
    fn router_remove_peer() {
        let mut r = Router::new(Strategy::BestMatch);
        r.update_peer(make_peer(1, vec![]));
        r.remove_peer(&[1]);
        assert_eq!(r.peer_count(), 0);
    }

    #[test]
    fn router_update_peer_replaces() {
        let mut r = Router::new(Strategy::BestMatch);
        let cap = Capability::new("llm", "chat", 1);
        r.update_peer(make_peer(1, vec![]));
        r.update_peer(make_peer(1, vec![cap.clone()]));
        assert_eq!(r.peer_count(), 1);
        let peers = r.find_capable_peers(&cap);
        assert_eq!(peers.len(), 1);
    }

    #[test]
    fn router_find_capable_peers() {
        let mut r = Router::new(Strategy::BestMatch);
        let llm = Capability::new("llm", "chat", 1);
        let code = Capability::new("code", "review", 1);

        r.update_peer(make_peer(1, vec![llm.clone()]));
        r.update_peer(make_peer(2, vec![code.clone()]));
        r.update_peer(make_peer(3, vec![llm.clone(), code.clone()]));

        let llm_peers = r.find_capable_peers(&llm);
        assert_eq!(llm_peers.len(), 2);

        let code_peers = r.find_capable_peers(&code);
        assert_eq!(code_peers.len(), 2);
    }

    #[test]
    fn router_find_no_capable_peers() {
        let mut r = Router::new(Strategy::BestMatch);
        r.update_peer(make_peer(1, vec![Capability::new("llm", "chat", 1)]));
        let peers = r.find_capable_peers(&Capability::new("code", "review", 1));
        assert_eq!(peers.len(), 0);
    }

    #[test]
    fn router_route_best_match_empty() {
        let mut r = Router::new(Strategy::BestMatch);
        let result = r.route(&Capability::new("llm", "chat", 1));
        assert!(result.is_empty());
    }

    #[test]
    fn router_route_best_match_single() {
        let mut r = Router::new(Strategy::BestMatch);
        let cap = Capability::new("llm", "chat", 1);
        r.update_peer(make_peer(1, vec![cap.clone()]));
        let result = r.route(&cap);
        assert_eq!(result, vec![vec![1]]);
    }

    #[test]
    fn router_route_best_match_prefers_successful() {
        let mut r = Router::new(Strategy::BestMatch);
        let cap = Capability::new("llm", "chat", 1);
        r.update_peer(make_peer(1, vec![cap.clone()]));
        r.update_peer(make_peer(2, vec![cap.clone()]));

        // Peer 2 has great stats, peer 1 has failures
        r.record_success(&[2], 100);
        r.record_success(&[2], 150);
        r.record_failure(&[1]);
        r.record_failure(&[1]);

        let result = r.route(&cap);
        assert_eq!(result, vec![vec![2]]);
    }

    #[test]
    fn router_route_round_robin() {
        let mut r = Router::new(Strategy::RoundRobin);
        let cap = Capability::new("llm", "chat", 1);
        r.update_peer(make_peer(1, vec![cap.clone()]));
        r.update_peer(make_peer(2, vec![cap.clone()]));

        let r1 = r.route(&cap);
        let r2 = r.route(&cap);
        // Should alternate between the two peers
        assert_ne!(r1, r2);
    }

    #[test]
    fn router_route_broadcast() {
        let mut r = Router::new(Strategy::Broadcast);
        let cap = Capability::new("llm", "chat", 1);
        r.update_peer(make_peer(1, vec![cap.clone()]));
        r.update_peer(make_peer(2, vec![cap.clone()]));
        r.update_peer(make_peer(3, vec![Capability::new("code", "review", 1)]));

        let result = r.route(&cap);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn peer_stats_new_defaults() {
        let s = PeerStats::new();
        assert_eq!(s.total_tasks, 0);
        assert_eq!(s.success_rate(), 0.0);
        assert_eq!(s.avg_latency_ms(), u64::MAX);
        assert!((s.score() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn peer_stats_success_rate() {
        let mut s = PeerStats::new();
        s.record_success(100);
        s.record_success(200);
        s.record_failure();
        assert!((s.success_rate() - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn peer_stats_avg_latency() {
        let mut s = PeerStats::new();
        s.record_success(100);
        s.record_success(300);
        assert_eq!(s.avg_latency_ms(), 200);
    }

    #[test]
    fn peer_stats_score_perfect() {
        let mut s = PeerStats::new();
        s.record_success(50);
        s.record_success(50);
        // 100% success, 50ms avg latency
        // success_factor = 1.0
        // latency_factor = 1/(1+0.05) ≈ 0.952
        // score = 0.6*1.0 + 0.4*0.952 ≈ 0.981
        assert!(s.score() > 0.9);
    }

    #[test]
    fn peer_stats_score_all_failures() {
        let mut s = PeerStats::new();
        s.record_failure();
        s.record_failure();
        // 0% success, no successful tasks so avg_latency = u64::MAX
        // success_factor = 0.0
        // latency_factor = 1/(1 + u64::MAX/1000) ≈ 0.0
        // score = 0.6*0 + 0.4*~0 ≈ 0.0
        assert!(s.score() < 0.5);
    }

    #[test]
    fn peer_stats_avg_latency_ignores_failures() {
        let mut s = PeerStats::new();
        s.record_success(100);
        s.record_success(300);
        s.record_failure(); // should not affect avg latency
        // avg = (100+300)/2 = 200, not (100+300)/3 = 133
        assert_eq!(s.avg_latency_ms(), 200);
    }

    #[test]
    fn peer_stats_saturating_arithmetic() {
        let mut s = PeerStats::new();
        s.total_tasks = u64::MAX;
        s.record_failure();
        assert_eq!(s.total_tasks, u64::MAX);

        let mut s2 = PeerStats::new();
        s2.total_latency_ms = u64::MAX;
        s2.record_success(1000);
        assert_eq!(s2.total_latency_ms, u64::MAX);
    }

    #[test]
    fn router_record_and_get_stats() {
        let mut r = Router::new(Strategy::BestMatch);
        r.record_success(&[1], 100);
        r.record_success(&[1], 200);
        r.record_failure(&[1]);

        let stats = r.get_stats(&[1]).unwrap();
        assert_eq!(stats.total_tasks, 3);
        assert_eq!(stats.successful_tasks, 2);
    }

    #[test]
    fn router_get_stats_unknown_peer() {
        let r = Router::new(Strategy::BestMatch);
        assert!(r.get_stats(&[99]).is_none());
    }

    #[test]
    fn router_capability_version_matching() {
        let mut r = Router::new(Strategy::BestMatch);
        // Peer has v2, request is for v1 — should match
        r.update_peer(make_peer(1, vec![Capability::new("llm", "chat", 2)]));
        let result = r.route(&Capability::new("llm", "chat", 1));
        assert_eq!(result.len(), 1);

        // Request for v3 — should NOT match v2
        let result = r.route(&Capability::new("llm", "chat", 3));
        assert!(result.is_empty());
    }
}
