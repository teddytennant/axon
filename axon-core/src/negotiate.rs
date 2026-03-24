//! Agent-to-agent task negotiation protocol.
//!
//! Instead of the router unilaterally picking a peer based on stale stats,
//! agents **bid** on tasks. The requestor broadcasts a [`TaskOffer`], peers
//! respond with [`TaskBid`]s reporting their actual load, estimated latency,
//! and confidence. A [`Negotiator`] scores bids and selects a winner.
//!
//! This turns axon from a routing mesh into a compute marketplace.
//!
//! # Protocol flow
//!
//! 1. Requestor sends `TaskOffer` to all capable peers
//! 2. Peers evaluate the offer via their [`BiddingStrategy`] and respond with `TaskBid`
//! 3. Requestor collects bids until deadline (or all peers respond)
//! 4. [`Negotiator::select_winner`] picks the best bid
//! 5. Winner gets `BidAccept` + the actual `TaskRequest`
//! 6. Losers get `BidReject`

use crate::protocol::{Capability, TaskRequest};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Bid scoring
// ---------------------------------------------------------------------------

/// How to rank competing bids.
#[derive(Debug, Clone, PartialEq)]
pub enum BidScoring {
    /// Pick the peer with lowest estimated latency.
    LowestLatency,
    /// Pick the least-loaded peer.
    LeastLoaded,
    /// Pick the most confident peer.
    HighestConfidence,
    /// Weighted combination (weights are relative, not required to sum to 1).
    Weighted {
        latency_weight: f64,
        load_weight: f64,
        confidence_weight: f64,
    },
}

impl Default for BidScoring {
    fn default() -> Self {
        Self::Weighted {
            latency_weight: 0.4,
            load_weight: 0.3,
            confidence_weight: 0.3,
        }
    }
}

/// A received bid (local representation, mirrors the protocol message fields).
#[derive(Debug, Clone)]
pub struct ReceivedBid {
    pub request_id: Uuid,
    pub peer_id: Vec<u8>,
    pub estimated_latency_ms: u64,
    pub load_factor: f64,
    pub confidence: f64,
    pub received_at: Instant,
}

impl ReceivedBid {
    pub fn new(
        request_id: Uuid,
        peer_id: Vec<u8>,
        estimated_latency_ms: u64,
        load_factor: f64,
        confidence: f64,
    ) -> Self {
        Self {
            request_id,
            peer_id,
            estimated_latency_ms,
            load_factor: load_factor.clamp(0.0, 1.0),
            confidence: confidence.clamp(0.0, 1.0),
            received_at: Instant::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Negotiator
// ---------------------------------------------------------------------------

/// Scores and selects winning bids for task negotiations.
pub struct Negotiator {
    /// How long to wait for bids before selecting a winner.
    pub bid_timeout: Duration,
    /// Scoring strategy.
    pub scoring: BidScoring,
    /// Minimum confidence threshold — bids below this are discarded.
    pub min_confidence: f64,
    /// Maximum load factor — bids above this are discarded.
    pub max_load: f64,
}

impl Negotiator {
    pub fn new(bid_timeout: Duration, scoring: BidScoring) -> Self {
        Self {
            bid_timeout,
            scoring,
            min_confidence: 0.0,
            max_load: 1.0,
        }
    }

    /// Set minimum confidence threshold.
    pub fn with_min_confidence(mut self, min: f64) -> Self {
        self.min_confidence = min.clamp(0.0, 1.0);
        self
    }

    /// Set maximum load factor threshold.
    pub fn with_max_load(mut self, max: f64) -> Self {
        self.max_load = max.clamp(0.0, 1.0);
        self
    }

    /// Score a single bid. Higher is better.
    pub fn score_bid(&self, bid: &ReceivedBid) -> f64 {
        match &self.scoring {
            BidScoring::LowestLatency => {
                // Invert latency: lower latency → higher score
                1.0 / (1.0 + bid.estimated_latency_ms as f64 / 1000.0)
            }
            BidScoring::LeastLoaded => {
                // Invert load: lower load → higher score
                1.0 - bid.load_factor
            }
            BidScoring::HighestConfidence => bid.confidence,
            BidScoring::Weighted {
                latency_weight,
                load_weight,
                confidence_weight,
            } => {
                let latency_score = 1.0 / (1.0 + bid.estimated_latency_ms as f64 / 1000.0);
                let load_score = 1.0 - bid.load_factor;
                let confidence_score = bid.confidence;

                let total_weight = latency_weight + load_weight + confidence_weight;
                if total_weight == 0.0 {
                    return 0.0;
                }

                (latency_weight * latency_score
                    + load_weight * load_score
                    + confidence_weight * confidence_score)
                    / total_weight
            }
        }
    }

    /// Filter bids that meet thresholds.
    pub fn filter_eligible<'a>(&self, bids: &'a [ReceivedBid]) -> Vec<&'a ReceivedBid> {
        bids.iter()
            .filter(|b| b.confidence >= self.min_confidence && b.load_factor <= self.max_load)
            .collect()
    }

    /// Select the best bid from a collection. Returns `None` if no eligible bids.
    pub fn select_winner<'a>(&self, bids: &'a [ReceivedBid]) -> Option<&'a ReceivedBid> {
        let eligible = self.filter_eligible(bids);
        eligible.into_iter().max_by(|a, b| {
            self.score_bid(a)
                .partial_cmp(&self.score_bid(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Rank all eligible bids by score (highest first).
    pub fn rank_bids<'a>(&self, bids: &'a [ReceivedBid]) -> Vec<(&'a ReceivedBid, f64)> {
        let mut eligible: Vec<(&ReceivedBid, f64)> = self
            .filter_eligible(bids)
            .into_iter()
            .map(|b| (b, self.score_bid(b)))
            .collect();
        eligible.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        eligible
    }
}

impl Default for Negotiator {
    fn default() -> Self {
        Self::new(Duration::from_millis(500), BidScoring::default())
    }
}

// ---------------------------------------------------------------------------
// Negotiation state tracker
// ---------------------------------------------------------------------------

/// State of a single active negotiation.
#[derive(Debug)]
pub struct ActiveNegotiation {
    pub request_id: Uuid,
    pub capability: Capability,
    pub payload_hint: u64,
    pub bids: Vec<ReceivedBid>,
    pub started_at: Instant,
    pub deadline: Duration,
    /// Number of peers the offer was sent to.
    pub peers_solicited: usize,
    /// The original task request, stored so we can forward it to the winner.
    stored_request: Option<TaskRequest>,
}

impl ActiveNegotiation {
    pub fn new(
        request_id: Uuid,
        capability: Capability,
        payload_hint: u64,
        deadline: Duration,
        peers_solicited: usize,
    ) -> Self {
        Self {
            request_id,
            capability,
            payload_hint,
            bids: Vec::new(),
            started_at: Instant::now(),
            deadline,
            peers_solicited,
            stored_request: None,
        }
    }

    /// Create a new negotiation with the original task request stored.
    pub fn with_request(
        request_id: Uuid,
        capability: Capability,
        payload_hint: u64,
        deadline: Duration,
        peers_solicited: usize,
        request: TaskRequest,
    ) -> Self {
        Self {
            request_id,
            capability,
            payload_hint,
            bids: Vec::new(),
            started_at: Instant::now(),
            deadline,
            peers_solicited,
            stored_request: Some(request),
        }
    }

    /// Take the stored task request, leaving None in its place.
    pub fn take_request(&mut self) -> Option<TaskRequest> {
        self.stored_request.take()
    }

    /// Whether a task request is stored.
    pub fn has_request(&self) -> bool {
        self.stored_request.is_some()
    }

    /// Whether the bid deadline has elapsed.
    pub fn is_expired(&self) -> bool {
        self.started_at.elapsed() >= self.deadline
    }

    /// Whether all solicited peers have responded.
    pub fn all_responded(&self) -> bool {
        self.bids.len() >= self.peers_solicited
    }

    /// Whether the negotiation is ready for winner selection.
    pub fn is_ready(&self) -> bool {
        self.is_expired() || self.all_responded()
    }

    /// Time remaining before deadline.
    pub fn time_remaining(&self) -> Duration {
        self.deadline.saturating_sub(self.started_at.elapsed())
    }

    /// Add a bid. Returns false if duplicate peer or wrong request_id.
    pub fn add_bid(&mut self, bid: ReceivedBid) -> bool {
        if bid.request_id != self.request_id {
            return false;
        }
        // Reject duplicate bids from same peer
        if self.bids.iter().any(|b| b.peer_id == bid.peer_id) {
            return false;
        }
        self.bids.push(bid);
        true
    }
}

/// Tracks all active negotiations across the node.
pub struct NegotiationState {
    active: HashMap<Uuid, ActiveNegotiation>,
}

impl NegotiationState {
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
        }
    }

    /// Start a new negotiation.
    pub fn start(
        &mut self,
        request_id: Uuid,
        capability: Capability,
        payload_hint: u64,
        deadline: Duration,
        peers_solicited: usize,
    ) -> &mut ActiveNegotiation {
        self.active.insert(
            request_id,
            ActiveNegotiation::new(
                request_id,
                capability,
                payload_hint,
                deadline,
                peers_solicited,
            ),
        );
        self.active.get_mut(&request_id).unwrap()
    }

    /// Start a new negotiation with the original task request stored for later dispatch.
    pub fn start_with_request(
        &mut self,
        request_id: Uuid,
        capability: Capability,
        payload_hint: u64,
        deadline: Duration,
        peers_solicited: usize,
        request: TaskRequest,
    ) -> &mut ActiveNegotiation {
        self.active.insert(
            request_id,
            ActiveNegotiation::with_request(
                request_id,
                capability,
                payload_hint,
                deadline,
                peers_solicited,
                request,
            ),
        );
        self.active.get_mut(&request_id).unwrap()
    }

    /// Record a bid for an active negotiation. Returns false if negotiation
    /// not found, bid is duplicate, or request_id doesn't match.
    pub fn record_bid(&mut self, bid: ReceivedBid) -> bool {
        match self.active.get_mut(&bid.request_id) {
            Some(neg) => neg.add_bid(bid),
            None => false,
        }
    }

    /// Get an active negotiation by request ID.
    pub fn get(&self, request_id: &Uuid) -> Option<&ActiveNegotiation> {
        self.active.get(request_id)
    }

    /// Remove and return a completed negotiation.
    pub fn complete(&mut self, request_id: &Uuid) -> Option<ActiveNegotiation> {
        self.active.remove(request_id)
    }

    /// Number of active negotiations.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    /// Remove all expired negotiations, returning their request IDs.
    pub fn cleanup_expired(&mut self) -> Vec<Uuid> {
        let expired: Vec<Uuid> = self
            .active
            .iter()
            .filter(|(_, n)| n.is_expired())
            .map(|(id, _)| *id)
            .collect();
        for id in &expired {
            self.active.remove(id);
        }
        expired
    }

    /// Get all negotiations that are ready for winner selection.
    pub fn ready_negotiations(&self) -> Vec<Uuid> {
        self.active
            .iter()
            .filter(|(_, n)| n.is_ready())
            .map(|(id, _)| *id)
            .collect()
    }
}

impl Default for NegotiationState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Bidding strategy
// ---------------------------------------------------------------------------

/// Trait for agents to decide whether and how to bid on task offers.
pub trait BiddingStrategy: Send + Sync {
    /// Whether this agent should bid on the given offer.
    fn should_bid(&self, capability: &Capability, payload_hint: u64) -> bool;

    /// Compute a bid for the given offer.
    fn compute_bid(
        &self,
        request_id: Uuid,
        capability: &Capability,
        payload_hint: u64,
    ) -> ReceivedBid;
}

/// Always bids, honestly reports load.
pub struct EagerBidder {
    peer_id: Vec<u8>,
    queue_depth: Arc<AtomicU64>,
    max_queue_depth: u64,
    capabilities: Vec<Capability>,
    /// Base latency estimate in ms.
    base_latency_ms: u64,
}

impl EagerBidder {
    pub fn new(
        peer_id: Vec<u8>,
        queue_depth: Arc<AtomicU64>,
        max_queue_depth: u64,
        capabilities: Vec<Capability>,
        base_latency_ms: u64,
    ) -> Self {
        Self {
            peer_id,
            queue_depth,
            max_queue_depth,
            capabilities,
            base_latency_ms,
        }
    }

    fn current_load(&self) -> f64 {
        if self.max_queue_depth == 0 {
            return 0.0;
        }
        let depth = self.queue_depth.load(Ordering::Relaxed);
        (depth as f64 / self.max_queue_depth as f64).clamp(0.0, 1.0)
    }
}

impl BiddingStrategy for EagerBidder {
    fn should_bid(&self, capability: &Capability, _payload_hint: u64) -> bool {
        self.capabilities.iter().any(|c| c.matches(capability))
    }

    fn compute_bid(
        &self,
        request_id: Uuid,
        _capability: &Capability,
        _payload_hint: u64,
    ) -> ReceivedBid {
        let load = self.current_load();
        // Estimated latency scales with load: idle = base, full = 10× base
        let latency = self.base_latency_ms + (self.base_latency_ms as f64 * load * 9.0) as u64;
        // Confidence decreases as load approaches capacity
        let confidence = 1.0 - (load * 0.5);

        ReceivedBid::new(request_id, self.peer_id.clone(), latency, load, confidence)
    }
}

/// Only bids when load is below a threshold.
pub struct LoadAwareBidder {
    peer_id: Vec<u8>,
    queue_depth: Arc<AtomicU64>,
    max_queue_depth: u64,
    capabilities: Vec<Capability>,
    /// Won't bid if load exceeds this (0.0–1.0).
    max_bid_load: f64,
    base_latency_ms: u64,
}

impl LoadAwareBidder {
    pub fn new(
        peer_id: Vec<u8>,
        queue_depth: Arc<AtomicU64>,
        max_queue_depth: u64,
        capabilities: Vec<Capability>,
        max_bid_load: f64,
        base_latency_ms: u64,
    ) -> Self {
        Self {
            peer_id,
            queue_depth,
            max_queue_depth,
            capabilities,
            max_bid_load: max_bid_load.clamp(0.0, 1.0),
            base_latency_ms,
        }
    }

    fn current_load(&self) -> f64 {
        if self.max_queue_depth == 0 {
            return 0.0;
        }
        let depth = self.queue_depth.load(Ordering::Relaxed);
        (depth as f64 / self.max_queue_depth as f64).clamp(0.0, 1.0)
    }
}

impl BiddingStrategy for LoadAwareBidder {
    fn should_bid(&self, capability: &Capability, _payload_hint: u64) -> bool {
        let has_capability = self.capabilities.iter().any(|c| c.matches(capability));
        has_capability && self.current_load() <= self.max_bid_load
    }

    fn compute_bid(
        &self,
        request_id: Uuid,
        _capability: &Capability,
        _payload_hint: u64,
    ) -> ReceivedBid {
        let load = self.current_load();
        let latency = self.base_latency_ms + (self.base_latency_ms as f64 * load * 5.0) as u64;
        // Higher confidence when less loaded
        let confidence = (1.0 - load).max(0.1);

        ReceivedBid::new(request_id, self.peer_id.clone(), latency, load, confidence)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bid(peer: u8, latency: u64, load: f64, confidence: f64) -> ReceivedBid {
        ReceivedBid::new(Uuid::new_v4(), vec![peer], latency, load, confidence)
    }

    fn make_bid_for(
        request_id: Uuid,
        peer: u8,
        latency: u64,
        load: f64,
        confidence: f64,
    ) -> ReceivedBid {
        ReceivedBid::new(request_id, vec![peer], latency, load, confidence)
    }

    // -- BidScoring -------------------------------------------------------

    #[test]
    fn score_lowest_latency_prefers_fast() {
        let n = Negotiator::new(Duration::from_millis(100), BidScoring::LowestLatency);
        let fast = make_bid(1, 50, 0.9, 0.5);
        let slow = make_bid(2, 5000, 0.1, 1.0);
        assert!(n.score_bid(&fast) > n.score_bid(&slow));
    }

    #[test]
    fn score_least_loaded_prefers_idle() {
        let n = Negotiator::new(Duration::from_millis(100), BidScoring::LeastLoaded);
        let idle = make_bid(1, 5000, 0.1, 0.5);
        let busy = make_bid(2, 50, 0.9, 1.0);
        assert!(n.score_bid(&idle) > n.score_bid(&busy));
    }

    #[test]
    fn score_highest_confidence_prefers_confident() {
        let n = Negotiator::new(Duration::from_millis(100), BidScoring::HighestConfidence);
        let sure = make_bid(1, 5000, 0.9, 0.95);
        let unsure = make_bid(2, 50, 0.1, 0.3);
        assert!(n.score_bid(&sure) > n.score_bid(&unsure));
    }

    #[test]
    fn score_weighted_balances_factors() {
        let n = Negotiator::new(
            Duration::from_millis(100),
            BidScoring::Weighted {
                latency_weight: 0.5,
                load_weight: 0.3,
                confidence_weight: 0.2,
            },
        );
        // Peer 1: fast, heavily loaded, low confidence
        let p1 = make_bid(1, 50, 0.9, 0.3);
        // Peer 2: moderate everything
        let p2 = make_bid(2, 200, 0.4, 0.7);
        // Peer 2 should win due to better balance
        assert!(n.score_bid(&p2) > n.score_bid(&p1));
    }

    #[test]
    fn score_weighted_zero_weights_returns_zero() {
        let n = Negotiator::new(
            Duration::from_millis(100),
            BidScoring::Weighted {
                latency_weight: 0.0,
                load_weight: 0.0,
                confidence_weight: 0.0,
            },
        );
        let bid = make_bid(1, 100, 0.5, 0.8);
        assert_eq!(n.score_bid(&bid), 0.0);
    }

    #[test]
    fn score_latency_inversely_proportional() {
        let n = Negotiator::new(Duration::from_millis(100), BidScoring::LowestLatency);
        let b0 = make_bid(1, 0, 0.5, 0.5);
        let b100 = make_bid(2, 100, 0.5, 0.5);
        let b1000 = make_bid(3, 1000, 0.5, 0.5);
        let b10000 = make_bid(4, 10000, 0.5, 0.5);

        let s0 = n.score_bid(&b0);
        let s100 = n.score_bid(&b100);
        let s1000 = n.score_bid(&b1000);
        let s10000 = n.score_bid(&b10000);

        assert!(s0 > s100);
        assert!(s100 > s1000);
        assert!(s1000 > s10000);
        // Zero latency → score = 1.0
        assert!((s0 - 1.0).abs() < f64::EPSILON);
    }

    // -- Negotiator selection ---------------------------------------------

    #[test]
    fn select_winner_empty_bids() {
        let n = Negotiator::default();
        let bids: Vec<ReceivedBid> = vec![];
        assert!(n.select_winner(&bids).is_none());
    }

    #[test]
    fn select_winner_single_bid() {
        let n = Negotiator::default();
        let bids = vec![make_bid(1, 100, 0.3, 0.8)];
        let winner = n.select_winner(&bids).unwrap();
        assert_eq!(winner.peer_id, vec![1]);
    }

    #[test]
    fn select_winner_picks_best() {
        let n = Negotiator::new(Duration::from_millis(100), BidScoring::LowestLatency);
        let bids = vec![
            make_bid(1, 500, 0.5, 0.8),
            make_bid(2, 50, 0.5, 0.8),
            make_bid(3, 200, 0.5, 0.8),
        ];
        let winner = n.select_winner(&bids).unwrap();
        assert_eq!(winner.peer_id, vec![2]); // fastest
    }

    #[test]
    fn select_winner_respects_min_confidence() {
        let n = Negotiator::default().with_min_confidence(0.5);
        let bids = vec![
            make_bid(1, 50, 0.2, 0.3),  // fast but low confidence → filtered
            make_bid(2, 200, 0.3, 0.8), // slower but confident → wins
        ];
        let winner = n.select_winner(&bids).unwrap();
        assert_eq!(winner.peer_id, vec![2]);
    }

    #[test]
    fn select_winner_respects_max_load() {
        let n = Negotiator::default().with_max_load(0.7);
        let bids = vec![
            make_bid(1, 50, 0.9, 0.9),  // fast, confident but overloaded → filtered
            make_bid(2, 200, 0.5, 0.7), // slower but under load limit → wins
        ];
        let winner = n.select_winner(&bids).unwrap();
        assert_eq!(winner.peer_id, vec![2]);
    }

    #[test]
    fn select_winner_all_filtered_returns_none() {
        let n = Negotiator::default()
            .with_min_confidence(0.9)
            .with_max_load(0.2);
        let bids = vec![make_bid(1, 50, 0.5, 0.3), make_bid(2, 100, 0.8, 0.5)];
        assert!(n.select_winner(&bids).is_none());
    }

    #[test]
    fn rank_bids_returns_sorted() {
        let n = Negotiator::new(Duration::from_millis(100), BidScoring::LowestLatency);
        let bids = vec![
            make_bid(1, 500, 0.5, 0.8),
            make_bid(2, 50, 0.5, 0.8),
            make_bid(3, 200, 0.5, 0.8),
        ];
        let ranked = n.rank_bids(&bids);
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].0.peer_id, vec![2]); // 50ms
        assert_eq!(ranked[1].0.peer_id, vec![3]); // 200ms
        assert_eq!(ranked[2].0.peer_id, vec![1]); // 500ms
                                                  // Scores descending
        assert!(ranked[0].1 >= ranked[1].1);
        assert!(ranked[1].1 >= ranked[2].1);
    }

    #[test]
    fn filter_eligible_applies_thresholds() {
        let n = Negotiator::default()
            .with_min_confidence(0.5)
            .with_max_load(0.8);
        let bids = vec![
            make_bid(1, 100, 0.3, 0.9),  // eligible
            make_bid(2, 100, 0.3, 0.2),  // low confidence
            make_bid(3, 100, 0.95, 0.9), // too loaded
            make_bid(4, 100, 0.7, 0.6),  // eligible
        ];
        let eligible = n.filter_eligible(&bids);
        assert_eq!(eligible.len(), 2);
    }

    // -- ActiveNegotiation ------------------------------------------------

    #[test]
    fn active_negotiation_tracks_bids() {
        let id = Uuid::new_v4();
        let cap = Capability::new("llm", "chat", 1);
        let mut neg = ActiveNegotiation::new(id, cap, 0, Duration::from_secs(5), 3);

        assert!(!neg.is_expired());
        assert!(!neg.all_responded());
        assert!(!neg.is_ready());

        assert!(neg.add_bid(make_bid_for(id, 1, 100, 0.3, 0.8)));
        assert!(neg.add_bid(make_bid_for(id, 2, 200, 0.5, 0.7)));
        assert!(!neg.all_responded()); // 2/3

        assert!(neg.add_bid(make_bid_for(id, 3, 150, 0.4, 0.9)));
        assert!(neg.all_responded()); // 3/3
        assert!(neg.is_ready());
    }

    #[test]
    fn active_negotiation_rejects_duplicates() {
        let id = Uuid::new_v4();
        let cap = Capability::new("llm", "chat", 1);
        let mut neg = ActiveNegotiation::new(id, cap, 0, Duration::from_secs(5), 3);

        assert!(neg.add_bid(make_bid_for(id, 1, 100, 0.3, 0.8)));
        assert!(!neg.add_bid(make_bid_for(id, 1, 200, 0.5, 0.7))); // duplicate peer
        assert_eq!(neg.bids.len(), 1);
    }

    #[test]
    fn active_negotiation_rejects_wrong_request_id() {
        let id = Uuid::new_v4();
        let cap = Capability::new("llm", "chat", 1);
        let mut neg = ActiveNegotiation::new(id, cap, 0, Duration::from_secs(5), 3);

        let wrong_id = Uuid::new_v4();
        assert!(!neg.add_bid(make_bid_for(wrong_id, 1, 100, 0.3, 0.8)));
        assert_eq!(neg.bids.len(), 0);
    }

    #[test]
    fn active_negotiation_expires() {
        let id = Uuid::new_v4();
        let cap = Capability::new("llm", "chat", 1);
        let neg = ActiveNegotiation::new(id, cap, 0, Duration::from_millis(0), 3);
        // Zero deadline → immediately expired
        assert!(neg.is_expired());
        assert!(neg.is_ready());
    }

    #[test]
    fn active_negotiation_time_remaining() {
        let id = Uuid::new_v4();
        let cap = Capability::new("llm", "chat", 1);
        let neg = ActiveNegotiation::new(id, cap, 0, Duration::from_secs(60), 3);
        // Should have roughly 60 seconds remaining
        assert!(neg.time_remaining() > Duration::from_secs(59));
    }

    // -- NegotiationState -------------------------------------------------

    #[test]
    fn state_start_and_get() {
        let mut state = NegotiationState::new();
        let id = Uuid::new_v4();
        let cap = Capability::new("llm", "chat", 1);

        state.start(id, cap.clone(), 1024, Duration::from_secs(5), 3);
        assert_eq!(state.active_count(), 1);

        let neg = state.get(&id).unwrap();
        assert_eq!(neg.request_id, id);
        assert_eq!(neg.capability, cap);
        assert_eq!(neg.payload_hint, 1024);
        assert_eq!(neg.peers_solicited, 3);
    }

    #[test]
    fn state_record_bid() {
        let mut state = NegotiationState::new();
        let id = Uuid::new_v4();
        state.start(
            id,
            Capability::new("llm", "chat", 1),
            0,
            Duration::from_secs(5),
            2,
        );

        let bid = make_bid_for(id, 1, 100, 0.3, 0.8);
        assert!(state.record_bid(bid));

        let neg = state.get(&id).unwrap();
        assert_eq!(neg.bids.len(), 1);
    }

    #[test]
    fn state_record_bid_unknown_negotiation() {
        let mut state = NegotiationState::new();
        let bid = make_bid_for(Uuid::new_v4(), 1, 100, 0.3, 0.8);
        assert!(!state.record_bid(bid));
    }

    #[test]
    fn state_complete_removes_negotiation() {
        let mut state = NegotiationState::new();
        let id = Uuid::new_v4();
        state.start(
            id,
            Capability::new("llm", "chat", 1),
            0,
            Duration::from_secs(5),
            2,
        );

        let neg = state.complete(&id).unwrap();
        assert_eq!(neg.request_id, id);
        assert_eq!(state.active_count(), 0);
        assert!(state.get(&id).is_none());
    }

    #[test]
    fn state_complete_nonexistent_returns_none() {
        let mut state = NegotiationState::new();
        assert!(state.complete(&Uuid::new_v4()).is_none());
    }

    #[test]
    fn state_cleanup_expired() {
        let mut state = NegotiationState::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        // id1: zero deadline → immediately expired
        state.start(
            id1,
            Capability::new("a", "b", 1),
            0,
            Duration::from_millis(0),
            1,
        );
        // id2: long deadline → not expired
        state.start(
            id2,
            Capability::new("c", "d", 1),
            0,
            Duration::from_secs(60),
            1,
        );

        let expired = state.cleanup_expired();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], id1);
        assert_eq!(state.active_count(), 1);
        assert!(state.get(&id2).is_some());
    }

    #[test]
    fn state_ready_negotiations() {
        let mut state = NegotiationState::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        // id1: expired → ready
        state.start(
            id1,
            Capability::new("a", "b", 1),
            0,
            Duration::from_millis(0),
            3,
        );
        // id2: all responded → ready
        state.start(
            id2,
            Capability::new("c", "d", 1),
            0,
            Duration::from_secs(60),
            1,
        );
        state.record_bid(make_bid_for(id2, 1, 100, 0.3, 0.8));
        // id3: not expired, not all responded → not ready
        state.start(
            id3,
            Capability::new("e", "f", 1),
            0,
            Duration::from_secs(60),
            3,
        );

        let ready = state.ready_negotiations();
        assert_eq!(ready.len(), 2);
        assert!(ready.contains(&id1));
        assert!(ready.contains(&id2));
        assert!(!ready.contains(&id3));
    }

    // -- BiddingStrategy: EagerBidder ------------------------------------

    #[test]
    fn eager_bidder_always_bids_on_matching_capability() {
        let caps = vec![Capability::new("llm", "chat", 1)];
        let bidder = EagerBidder::new(vec![1], Arc::new(AtomicU64::new(0)), 100, caps, 100);

        assert!(bidder.should_bid(&Capability::new("llm", "chat", 1), 0));
        assert!(!bidder.should_bid(&Capability::new("code", "review", 1), 0));
    }

    #[test]
    fn eager_bidder_load_affects_latency() {
        let depth = Arc::new(AtomicU64::new(0));
        let caps = vec![Capability::new("test", "t", 1)];
        let bidder = EagerBidder::new(vec![1], depth.clone(), 100, caps, 100);

        let id = Uuid::new_v4();
        let cap = Capability::new("test", "t", 1);

        // Idle: latency ≈ base (100ms)
        let idle_bid = bidder.compute_bid(id, &cap, 0);
        assert_eq!(idle_bid.estimated_latency_ms, 100); // 100 + 100*0*9 = 100

        // Half loaded: latency ≈ 550ms
        depth.store(50, Ordering::Relaxed);
        let half_bid = bidder.compute_bid(id, &cap, 0);
        assert!(half_bid.estimated_latency_ms > idle_bid.estimated_latency_ms);

        // Fully loaded: latency ≈ 1000ms
        depth.store(100, Ordering::Relaxed);
        let full_bid = bidder.compute_bid(id, &cap, 0);
        assert!(full_bid.estimated_latency_ms > half_bid.estimated_latency_ms);
    }

    #[test]
    fn eager_bidder_load_affects_confidence() {
        let depth = Arc::new(AtomicU64::new(0));
        let caps = vec![Capability::new("test", "t", 1)];
        let bidder = EagerBidder::new(vec![1], depth.clone(), 100, caps, 100);

        let id = Uuid::new_v4();
        let cap = Capability::new("test", "t", 1);

        let idle_bid = bidder.compute_bid(id, &cap, 0);
        depth.store(100, Ordering::Relaxed);
        let full_bid = bidder.compute_bid(id, &cap, 0);

        assert!(idle_bid.confidence > full_bid.confidence);
    }

    #[test]
    fn eager_bidder_zero_max_depth_reports_zero_load() {
        let caps = vec![Capability::new("test", "t", 1)];
        let bidder = EagerBidder::new(vec![1], Arc::new(AtomicU64::new(50)), 0, caps, 100);

        let id = Uuid::new_v4();
        let bid = bidder.compute_bid(id, &Capability::new("test", "t", 1), 0);
        assert!((bid.load_factor - 0.0).abs() < f64::EPSILON);
    }

    // -- BiddingStrategy: LoadAwareBidder --------------------------------

    #[test]
    fn load_aware_bidder_refuses_when_overloaded() {
        let depth = Arc::new(AtomicU64::new(80));
        let caps = vec![Capability::new("llm", "chat", 1)];
        let bidder = LoadAwareBidder::new(vec![1], depth, 100, caps, 0.7, 100);

        // Load is 0.8 > max_bid_load 0.7 → should not bid
        assert!(!bidder.should_bid(&Capability::new("llm", "chat", 1), 0));
    }

    #[test]
    fn load_aware_bidder_bids_when_under_threshold() {
        let depth = Arc::new(AtomicU64::new(50));
        let caps = vec![Capability::new("llm", "chat", 1)];
        let bidder = LoadAwareBidder::new(vec![1], depth, 100, caps, 0.7, 100);

        // Load is 0.5 < max_bid_load 0.7 → should bid
        assert!(bidder.should_bid(&Capability::new("llm", "chat", 1), 0));
    }

    #[test]
    fn load_aware_bidder_refuses_wrong_capability() {
        let depth = Arc::new(AtomicU64::new(0));
        let caps = vec![Capability::new("llm", "chat", 1)];
        let bidder = LoadAwareBidder::new(vec![1], depth, 100, caps, 0.9, 100);

        assert!(!bidder.should_bid(&Capability::new("code", "review", 1), 0));
    }

    #[test]
    fn load_aware_bidder_confidence_decreases_with_load() {
        let depth = Arc::new(AtomicU64::new(0));
        let caps = vec![Capability::new("test", "t", 1)];
        let bidder = LoadAwareBidder::new(vec![1], depth.clone(), 100, caps, 0.9, 100);

        let id = Uuid::new_v4();
        let cap = Capability::new("test", "t", 1);

        let idle_bid = bidder.compute_bid(id, &cap, 0);
        depth.store(70, Ordering::Relaxed);
        let loaded_bid = bidder.compute_bid(id, &cap, 0);

        assert!(idle_bid.confidence > loaded_bid.confidence);
    }

    // -- ReceivedBid clamping --------------------------------------------

    #[test]
    fn bid_clamps_load_factor() {
        let bid = ReceivedBid::new(Uuid::new_v4(), vec![1], 100, 1.5, 0.5);
        assert!((bid.load_factor - 1.0).abs() < f64::EPSILON);

        let bid2 = ReceivedBid::new(Uuid::new_v4(), vec![1], 100, -0.5, 0.5);
        assert!((bid2.load_factor - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn bid_clamps_confidence() {
        let bid = ReceivedBid::new(Uuid::new_v4(), vec![1], 100, 0.5, 2.0);
        assert!((bid.confidence - 1.0).abs() < f64::EPSILON);

        let bid2 = ReceivedBid::new(Uuid::new_v4(), vec![1], 100, 0.5, -1.0);
        assert!((bid2.confidence - 0.0).abs() < f64::EPSILON);
    }

    // -- Default Negotiator -----------------------------------------------

    #[test]
    fn default_negotiator_uses_weighted_scoring() {
        let n = Negotiator::default();
        assert!(matches!(n.scoring, BidScoring::Weighted { .. }));
        assert_eq!(n.bid_timeout, Duration::from_millis(500));
    }

    #[test]
    fn negotiator_with_min_confidence_clamps() {
        let n = Negotiator::default().with_min_confidence(1.5);
        assert!((n.min_confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn negotiator_with_max_load_clamps() {
        let n = Negotiator::default().with_max_load(-0.5);
        assert!((n.max_load - 0.0).abs() < f64::EPSILON);
    }

    // -- Integration: full negotiation flow --------------------------------

    #[test]
    fn full_negotiation_flow() {
        let mut state = NegotiationState::new();
        let negotiator = Negotiator::new(
            Duration::from_secs(5),
            BidScoring::Weighted {
                latency_weight: 0.4,
                load_weight: 0.3,
                confidence_weight: 0.3,
            },
        );

        // 1. Start negotiation
        let request_id = Uuid::new_v4();
        let cap = Capability::new("llm", "chat", 1);
        state.start(request_id, cap, 4096, Duration::from_secs(5), 3);

        // 2. Receive bids from 3 peers
        // Peer 1: fast but heavily loaded
        state.record_bid(make_bid_for(request_id, 1, 50, 0.9, 0.6));
        // Peer 2: balanced — moderate latency, moderate load, good confidence
        state.record_bid(make_bid_for(request_id, 2, 200, 0.3, 0.85));
        // Peer 3: slow but idle and very confident
        state.record_bid(make_bid_for(request_id, 3, 500, 0.1, 0.95));

        // 3. All responded → ready
        let neg = state.get(&request_id).unwrap();
        assert!(neg.is_ready());

        // 4. Select winner
        let winner = negotiator.select_winner(&neg.bids).unwrap();
        // With balanced weights, peer 3 (low load, high confidence) or peer 2 should win
        // Peer 1 is penalized for high load
        assert_ne!(winner.peer_id, vec![1]);

        // 5. Complete negotiation
        let completed = state.complete(&request_id).unwrap();
        assert_eq!(completed.bids.len(), 3);
        assert_eq!(state.active_count(), 0);
    }

    #[test]
    fn full_negotiation_with_strategies() {
        let depth1 = Arc::new(AtomicU64::new(90));
        let depth2 = Arc::new(AtomicU64::new(20));
        let cap = Capability::new("llm", "chat", 1);

        // Eager bidder always bids
        let eager = EagerBidder::new(vec![1], depth1, 100, vec![cap.clone()], 100);

        // Load-aware bidder with 0.5 threshold — won't bid at 90% load
        let load_aware = LoadAwareBidder::new(vec![2], depth2, 100, vec![cap.clone()], 0.5, 100);

        let request_id = Uuid::new_v4();

        // Eager: heavily loaded but still bids
        assert!(eager.should_bid(&cap, 0));
        let bid1 = eager.compute_bid(request_id, &cap, 0);
        assert!((bid1.load_factor - 0.9).abs() < f64::EPSILON);

        // Load-aware: lightly loaded and bids
        assert!(load_aware.should_bid(&cap, 0));
        let bid2 = load_aware.compute_bid(request_id, &cap, 0);
        assert!((bid2.load_factor - 0.2).abs() < f64::EPSILON);

        // Negotiator should prefer the less-loaded peer
        let negotiator = Negotiator::default();
        let bids = [bid1, bid2];
        let winner = negotiator.select_winner(&bids).unwrap();
        assert_eq!(winner.peer_id, vec![2]);
    }

    // -- ActiveNegotiation with stored request --------------------------------

    fn make_task_request(cap: &Capability) -> TaskRequest {
        TaskRequest {
            id: Uuid::new_v4(),
            capability: cap.clone(),
            payload: b"test payload".to_vec(),
            timeout_ms: 5000,
        }
    }

    #[test]
    fn active_negotiation_new_has_no_request() {
        let cap = Capability::new("llm", "chat", 1);
        let neg = ActiveNegotiation::new(Uuid::new_v4(), cap, 0, Duration::from_secs(1), 3);
        assert!(!neg.has_request());
    }

    #[test]
    fn active_negotiation_with_request_stores_it() {
        let cap = Capability::new("llm", "chat", 1);
        let req = make_task_request(&cap);
        let req_id = req.id;
        let neg = ActiveNegotiation::with_request(
            Uuid::new_v4(),
            cap,
            12,
            Duration::from_secs(1),
            3,
            req,
        );
        assert!(neg.has_request());
        assert_eq!(neg.payload_hint, 12);
        // The stored request preserves identity
        let stored = neg.stored_request.as_ref().unwrap();
        assert_eq!(stored.id, req_id);
        assert_eq!(stored.payload, b"test payload");
    }

    #[test]
    fn active_negotiation_take_request_consumes() {
        let cap = Capability::new("llm", "chat", 1);
        let req = make_task_request(&cap);
        let req_id = req.id;
        let mut neg =
            ActiveNegotiation::with_request(Uuid::new_v4(), cap, 0, Duration::from_secs(1), 1, req);
        assert!(neg.has_request());
        let taken = neg.take_request().unwrap();
        assert_eq!(taken.id, req_id);
        assert!(!neg.has_request());
        assert!(neg.take_request().is_none());
    }

    // -- NegotiationState with stored request ---------------------------------

    #[test]
    fn state_start_with_request_stores_and_retrieves() {
        let mut state = NegotiationState::new();
        let cap = Capability::new("llm", "chat", 1);
        let req = make_task_request(&cap);
        let req_id = req.id;
        let neg_id = Uuid::new_v4();

        state.start_with_request(neg_id, cap, 0, Duration::from_secs(5), 2, req);

        assert_eq!(state.active_count(), 1);
        let neg = state.get(&neg_id).unwrap();
        assert!(neg.has_request());
        assert_eq!(neg.peers_solicited, 2);

        // Complete and take the request
        let mut completed = state.complete(&neg_id).unwrap();
        let taken = completed.take_request().unwrap();
        assert_eq!(taken.id, req_id);
        assert_eq!(state.active_count(), 0);
    }

    #[test]
    fn state_start_with_request_and_bids_then_dispatch() {
        let mut state = NegotiationState::new();
        let cap = Capability::new("llm", "chat", 1);
        let req = make_task_request(&cap);
        let neg_id = req.id; // Use task ID as negotiation ID (matches real flow)

        state.start_with_request(neg_id, cap, 12, Duration::from_secs(5), 2, req);

        // Simulate two bids
        let bid1 = make_bid_for(neg_id, 1, 100, 0.3, 0.9);
        let bid2 = make_bid_for(neg_id, 2, 200, 0.5, 0.7);
        assert!(state.record_bid(bid1));
        assert!(state.record_bid(bid2));

        // All peers responded — negotiation should be ready
        let ready = state.ready_negotiations();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0], neg_id);

        // Complete, select winner, and extract request
        let mut completed = state.complete(&neg_id).unwrap();
        assert_eq!(completed.bids.len(), 2);

        let negotiator = Negotiator::new(Duration::from_millis(500), BidScoring::LowestLatency);
        let winner = negotiator.select_winner(&completed.bids).unwrap();
        assert_eq!(winner.peer_id, vec![1]); // Peer 1 has lower latency

        let task_req = completed.take_request().unwrap();
        assert_eq!(task_req.payload, b"test payload");
    }

    #[test]
    fn state_start_without_request_has_no_request() {
        let mut state = NegotiationState::new();
        let cap = Capability::new("llm", "chat", 1);
        let neg_id = Uuid::new_v4();

        state.start(neg_id, cap, 0, Duration::from_secs(5), 2);

        let mut completed = state.complete(&neg_id).unwrap();
        assert!(completed.take_request().is_none());
    }

    #[test]
    fn negotiation_dispatch_full_flow_with_trust() {
        use crate::trust::{TrustScore, TrustWeightedScoring};

        let mut state = NegotiationState::new();
        let cap = Capability::new("llm", "chat", 1);
        let req = make_task_request(&cap);
        let neg_id = req.id;

        state.start_with_request(neg_id, cap, 0, Duration::from_secs(5), 3, req);

        // 3 bids: reliable peer, fast peer, flaky peer
        let bid_reliable = make_bid_for(neg_id, 1, 200, 0.3, 0.9);
        let bid_fast = make_bid_for(neg_id, 2, 50, 0.2, 0.8);
        let bid_flaky = make_bid_for(neg_id, 3, 150, 0.1, 0.6);
        assert!(state.record_bid(bid_reliable));
        assert!(state.record_bid(bid_fast));
        assert!(state.record_bid(bid_flaky));

        let mut completed = state.complete(&neg_id).unwrap();

        // Score with trust weighting
        let negotiator = Negotiator::default();
        let trust_scoring = TrustWeightedScoring::default();

        // Simulate trust: reliable peer has high trust, fast peer has low trust
        let reliable_trust = TrustScore {
            reliability: 0.95,
            accuracy: 0.9,
            availability: 0.9,
            quality: 0.8,
            overall: 0.9,
            confidence: 0.8,
            observation_count: 50,
        };
        let fast_trust = TrustScore {
            reliability: 0.3,
            accuracy: 0.2,
            availability: 0.5,
            quality: 0.3,
            overall: 0.3,
            confidence: 0.7,
            observation_count: 30,
        };

        let scored: Vec<_> = completed
            .bids
            .iter()
            .map(|b| {
                let raw = negotiator.score_bid(b);
                let trust = if b.peer_id == vec![1] {
                    &reliable_trust
                } else if b.peer_id == vec![2] {
                    &fast_trust
                } else {
                    // Unknown — neutral trust
                    &TrustScore::default()
                };
                let weighted = trust_scoring.weighted_score(raw, trust);
                (b.peer_id.clone(), weighted)
            })
            .collect();

        // Find winner
        let winner = scored
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap();

        // Reliable peer should win despite being slower, because trust penalizes the fast peer
        assert_eq!(
            winner.0,
            vec![1],
            "Reliable peer should win with trust weighting"
        );

        // Can still extract the task request for dispatch
        let task_req = completed.take_request().unwrap();
        assert_eq!(task_req.timeout_ms, 5000);
    }
}
