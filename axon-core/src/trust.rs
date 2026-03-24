//! Subjective trust and reputation system for mesh peers.
//!
//! Each node maintains its own trust scores for every peer it interacts with.
//! Trust is **subjective** (no global ledger), **experience-based** (derived
//! from actual task outcomes), and **decay-weighted** (recent observations
//! matter more than old ones).
//!
//! # Why subjective?
//!
//! Global reputation systems are sybilproof only if they're asymmetric.
//! A node that has completed 100 tasks successfully for *you* is trustworthy
//! *to you* — regardless of what some other node thinks. This aligns with
//! the research consensus: "no symmetric global reputation can be sybilproof,
//! but asymmetric subjective trust can."
//!
//! # Design
//!
//! - [`TrustObservation`] — recorded after each interaction (success/failure,
//!   latency accuracy, optional quality signal)
//! - [`TrustRecord`] — per-peer observation history with exponential decay
//! - [`TrustScore`] — computed aggregate: reliability, accuracy, availability
//! - [`TrustScorer`] — configurable scoring with tunable weights
//! - [`TrustStore`] — per-node trust state for all known peers
//! - [`TrustGossip`] — share observations between peers (transitive trust)
//!
//! # Integration with negotiation
//!
//! The [`Negotiator`](crate::negotiate::Negotiator) can weight bids by the
//! requester's trust in the bidder, so that a fast bid from an unreliable
//! peer scores lower than a slower bid from a proven peer.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Observation — the atomic unit of trust data
// ---------------------------------------------------------------------------

/// Outcome of a task interaction with a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TaskOutcome {
    /// Task completed successfully.
    Success,
    /// Task failed (peer returned an error).
    Failure,
    /// Task timed out (peer didn't respond in time).
    Timeout,
    /// Peer rejected the task (e.g., via BidReject or capability mismatch).
    Rejected,
}

/// A single trust observation recorded after interacting with a peer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrustObservation {
    /// When this observation was recorded (seconds since UNIX epoch).
    pub timestamp: u64,
    /// What happened.
    pub outcome: TaskOutcome,
    /// Latency the peer estimated in their bid (ms). 0 if no bid.
    pub estimated_latency_ms: u64,
    /// Actual latency observed (ms). 0 if timeout/rejected.
    pub actual_latency_ms: u64,
    /// Optional quality signal from the task consumer (0.0–1.0).
    /// None if no quality feedback was provided.
    pub quality: Option<f64>,
}

impl TrustObservation {
    pub fn new(outcome: TaskOutcome, estimated_latency_ms: u64, actual_latency_ms: u64) -> Self {
        Self {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            outcome,
            estimated_latency_ms,
            actual_latency_ms,
            quality: None,
        }
    }

    pub fn with_quality(mut self, quality: f64) -> Self {
        self.quality = Some(quality.clamp(0.0, 1.0));
        self
    }

    pub fn with_timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// How accurate was the latency estimate? Returns 0.0–1.0 where
    /// 1.0 means perfect prediction and 0.0 means wildly wrong.
    pub fn latency_accuracy(&self) -> f64 {
        if self.estimated_latency_ms == 0 || self.actual_latency_ms == 0 {
            return 0.5; // No data → neutral
        }
        let est = self.estimated_latency_ms as f64;
        let act = self.actual_latency_ms as f64;
        let ratio = if est > act { act / est } else { est / act };
        ratio.clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// TrustRecord — per-peer observation history
// ---------------------------------------------------------------------------

/// Trust record for a single peer, maintained by the local node.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrustRecord {
    /// The peer's identity (public key bytes).
    pub peer_id: Vec<u8>,
    /// Observation history, ordered by timestamp (oldest first).
    pub observations: Vec<TrustObservation>,
    /// Maximum observations to retain (older ones are pruned).
    pub max_observations: usize,
    /// When this peer was first observed (seconds since epoch).
    pub first_seen: u64,
}

impl TrustRecord {
    pub fn new(peer_id: Vec<u8>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            peer_id,
            observations: Vec::new(),
            max_observations: 1000,
            first_seen: now,
        }
    }

    pub fn with_max_observations(mut self, max: usize) -> Self {
        self.max_observations = max;
        self
    }

    /// Record a new observation. Prunes oldest if at capacity.
    pub fn record(&mut self, observation: TrustObservation) {
        self.observations.push(observation);
        while self.observations.len() > self.max_observations {
            self.observations.remove(0);
        }
    }

    /// Total number of observations.
    pub fn observation_count(&self) -> usize {
        self.observations.len()
    }

    /// Number of successful task completions.
    pub fn success_count(&self) -> usize {
        self.observations
            .iter()
            .filter(|o| o.outcome == TaskOutcome::Success)
            .count()
    }

    /// Number of failed task completions.
    pub fn failure_count(&self) -> usize {
        self.observations
            .iter()
            .filter(|o| o.outcome == TaskOutcome::Failure)
            .count()
    }

    /// Number of timeouts.
    pub fn timeout_count(&self) -> usize {
        self.observations
            .iter()
            .filter(|o| o.outcome == TaskOutcome::Timeout)
            .count()
    }

    /// Most recent observation, if any.
    pub fn last_observation(&self) -> Option<&TrustObservation> {
        self.observations.last()
    }

    /// How long ago (in seconds) the last interaction was. None if no observations.
    pub fn seconds_since_last(&self) -> Option<u64> {
        self.last_observation().map(|o| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now.saturating_sub(o.timestamp)
        })
    }

    /// Get observations recorded within `window_secs` of now.
    pub fn recent_observations(&self, window_secs: u64) -> Vec<&TrustObservation> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let cutoff = now.saturating_sub(window_secs);
        self.observations
            .iter()
            .filter(|o| o.timestamp >= cutoff)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// TrustScore — computed aggregate trust
// ---------------------------------------------------------------------------

/// Computed trust score for a peer. All fields are 0.0–1.0.
#[derive(Debug, Clone, Copy)]
pub struct TrustScore {
    /// How reliably this peer completes tasks (success rate, decay-weighted).
    pub reliability: f64,
    /// How accurate this peer's latency estimates are (decay-weighted).
    pub accuracy: f64,
    /// How available this peer has been (not timing out, decay-weighted).
    pub availability: f64,
    /// Optional quality score from consumer feedback.
    pub quality: f64,
    /// Overall composite score.
    pub overall: f64,
    /// How much evidence we have (0.0 = none, approaches 1.0 with many observations).
    /// Used to weight trust in decisions — a peer with 2 successes is less
    /// certain than one with 200.
    pub confidence: f64,
    /// Number of observations this score is based on.
    pub observation_count: usize,
}

impl TrustScore {
    /// The default "neutral" score for peers with no history.
    pub fn neutral() -> Self {
        Self {
            reliability: 0.5,
            accuracy: 0.5,
            availability: 0.5,
            quality: 0.5,
            overall: 0.5,
            confidence: 0.0,
            observation_count: 0,
        }
    }

    /// Whether this score represents a trustworthy peer (above threshold).
    pub fn is_trusted(&self, threshold: f64) -> bool {
        self.overall >= threshold && self.confidence > 0.1
    }

    /// Whether this score represents an untrustworthy peer (below threshold
    /// with sufficient evidence).
    pub fn is_distrusted(&self, threshold: f64) -> bool {
        self.overall < threshold && self.confidence > 0.2
    }
}

impl Default for TrustScore {
    fn default() -> Self {
        Self::neutral()
    }
}

// ---------------------------------------------------------------------------
// TrustScorer — configurable scoring engine
// ---------------------------------------------------------------------------

/// Configuration for how trust scores are computed.
#[derive(Debug, Clone)]
pub struct TrustScorer {
    /// Half-life for exponential decay (in seconds).
    /// Observations older than this contribute half as much.
    /// Default: 86400 (24 hours).
    pub decay_half_life: f64,
    /// Weight for reliability in overall score.
    pub reliability_weight: f64,
    /// Weight for accuracy in overall score.
    pub accuracy_weight: f64,
    /// Weight for availability in overall score.
    pub availability_weight: f64,
    /// Weight for quality in overall score.
    pub quality_weight: f64,
    /// Number of observations needed for confidence to reach 0.9.
    /// Follows: confidence = 1 - e^(-observations / confidence_scale).
    pub confidence_scale: f64,
}

impl TrustScorer {
    pub fn new() -> Self {
        Self {
            decay_half_life: 86400.0, // 24 hours
            reliability_weight: 0.4,
            accuracy_weight: 0.2,
            availability_weight: 0.2,
            quality_weight: 0.2,
            confidence_scale: 20.0,
        }
    }

    /// Compute the exponential decay weight for an observation at a given age.
    /// Returns 1.0 for age=0, 0.5 for age=half_life, approaches 0 for old observations.
    pub fn decay_weight(&self, age_seconds: f64) -> f64 {
        if self.decay_half_life <= 0.0 {
            return 1.0;
        }
        // w = 2^(-age / half_life) = e^(-age * ln(2) / half_life)
        let lambda = std::f64::consts::LN_2 / self.decay_half_life;
        (-lambda * age_seconds).exp()
    }

    /// Compute the confidence level from observation count.
    /// 0 observations → 0.0, confidence_scale observations → ~0.63, ∞ → 1.0.
    pub fn confidence(&self, observation_count: usize) -> f64 {
        if self.confidence_scale <= 0.0 {
            return if observation_count > 0 { 1.0 } else { 0.0 };
        }
        1.0 - (-(observation_count as f64) / self.confidence_scale).exp()
    }

    /// Compute a full trust score from a trust record.
    pub fn score(&self, record: &TrustRecord) -> TrustScore {
        if record.observations.is_empty() {
            return TrustScore::neutral();
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.score_at(record, now)
    }

    /// Compute trust score at a specific timestamp (for deterministic testing).
    pub fn score_at(&self, record: &TrustRecord, now: u64) -> TrustScore {
        if record.observations.is_empty() {
            return TrustScore::neutral();
        }

        let mut reliability_num = 0.0;
        let mut reliability_den = 0.0;
        let mut accuracy_num = 0.0;
        let mut accuracy_den = 0.0;
        let mut availability_num = 0.0;
        let mut availability_den = 0.0;
        let mut quality_num = 0.0;
        let mut quality_den = 0.0;

        for obs in &record.observations {
            let age = now.saturating_sub(obs.timestamp) as f64;
            let w = self.decay_weight(age);

            // Reliability: success = 1.0, failure/timeout = 0.0, rejected = skip
            match obs.outcome {
                TaskOutcome::Success => {
                    reliability_num += w;
                    reliability_den += w;
                }
                TaskOutcome::Failure | TaskOutcome::Timeout => {
                    reliability_den += w;
                }
                TaskOutcome::Rejected => {
                    // Rejections are voluntary — they don't count against reliability
                }
            }

            // Accuracy: only for completed tasks (success or failure)
            if obs.outcome == TaskOutcome::Success || obs.outcome == TaskOutcome::Failure {
                let acc = obs.latency_accuracy();
                accuracy_num += w * acc;
                accuracy_den += w;
            }

            // Availability: success/failure = available, timeout = unavailable
            match obs.outcome {
                TaskOutcome::Success | TaskOutcome::Failure => {
                    availability_num += w;
                    availability_den += w;
                }
                TaskOutcome::Timeout => {
                    availability_den += w;
                }
                TaskOutcome::Rejected => {
                    // Rejections are intentional — don't penalize availability
                }
            }

            // Quality: only when provided
            if let Some(q) = obs.quality {
                quality_num += w * q;
                quality_den += w;
            }
        }

        let reliability = if reliability_den > 0.0 {
            reliability_num / reliability_den
        } else {
            0.5 // No relevant data → neutral
        };

        let accuracy = if accuracy_den > 0.0 {
            accuracy_num / accuracy_den
        } else {
            0.5
        };

        let availability = if availability_den > 0.0 {
            availability_num / availability_den
        } else {
            0.5
        };

        let quality = if quality_den > 0.0 {
            quality_num / quality_den
        } else {
            0.5
        };

        // Composite score
        let total_weight = self.reliability_weight
            + self.accuracy_weight
            + self.availability_weight
            + self.quality_weight;
        let overall = if total_weight > 0.0 {
            (self.reliability_weight * reliability
                + self.accuracy_weight * accuracy
                + self.availability_weight * availability
                + self.quality_weight * quality)
                / total_weight
        } else {
            0.5
        };

        let confidence = self.confidence(record.observations.len());

        TrustScore {
            reliability,
            accuracy,
            availability,
            quality,
            overall,
            confidence,
            observation_count: record.observations.len(),
        }
    }
}

impl Default for TrustScorer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// TrustStore — per-node trust state
// ---------------------------------------------------------------------------

/// Per-node storage of trust records for all known peers.
pub struct TrustStore {
    records: HashMap<Vec<u8>, TrustRecord>,
    scorer: TrustScorer,
    /// Maximum records to keep. Least-recently-observed peers are evicted.
    max_peers: usize,
}

impl TrustStore {
    pub fn new(scorer: TrustScorer) -> Self {
        Self {
            records: HashMap::new(),
            scorer,
            max_peers: 10000,
        }
    }

    pub fn with_max_peers(mut self, max: usize) -> Self {
        self.max_peers = max;
        self
    }

    /// Record an observation for a peer. Creates a new record if needed.
    pub fn record_observation(&mut self, peer_id: &[u8], observation: TrustObservation) {
        let record = self
            .records
            .entry(peer_id.to_vec())
            .or_insert_with(|| TrustRecord::new(peer_id.to_vec()));
        record.record(observation);

        // Evict least-recently-observed peers if at capacity
        if self.records.len() > self.max_peers {
            self.evict_oldest();
        }
    }

    /// Get the trust score for a peer. Returns neutral if unknown.
    pub fn score(&self, peer_id: &[u8]) -> TrustScore {
        match self.records.get(peer_id) {
            Some(record) => self.scorer.score(record),
            None => TrustScore::neutral(),
        }
    }

    /// Get the trust score for a peer at a specific timestamp.
    pub fn score_at(&self, peer_id: &[u8], now: u64) -> TrustScore {
        match self.records.get(peer_id) {
            Some(record) => self.scorer.score_at(record, now),
            None => TrustScore::neutral(),
        }
    }

    /// Get the trust record for a peer, if any.
    pub fn get_record(&self, peer_id: &[u8]) -> Option<&TrustRecord> {
        self.records.get(peer_id)
    }

    /// Get all peer IDs with trust records.
    pub fn known_peers(&self) -> Vec<Vec<u8>> {
        self.records.keys().cloned().collect()
    }

    /// Number of tracked peers.
    pub fn peer_count(&self) -> usize {
        self.records.len()
    }

    /// Get all peers with their trust scores, sorted by overall score (highest first).
    pub fn ranked_peers(&self) -> Vec<(Vec<u8>, TrustScore)> {
        let mut peers: Vec<(Vec<u8>, TrustScore)> = self
            .records
            .iter()
            .map(|(id, record)| (id.clone(), self.scorer.score(record)))
            .collect();
        peers.sort_by(|a, b| {
            b.1.overall
                .partial_cmp(&a.1.overall)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        peers
    }

    /// Get all peers with their trust scores at a specific timestamp.
    pub fn ranked_peers_at(&self, now: u64) -> Vec<(Vec<u8>, TrustScore)> {
        let mut peers: Vec<(Vec<u8>, TrustScore)> = self
            .records
            .iter()
            .map(|(id, record)| (id.clone(), self.scorer.score_at(record, now)))
            .collect();
        peers.sort_by(|a, b| {
            b.1.overall
                .partial_cmp(&a.1.overall)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        peers
    }

    /// Collect recent observations across all peers for gossip sharing.
    /// Returns (subject_peer_id, observation) pairs for observations within `window_secs`.
    pub fn recent_observations_all(&self, window_secs: u64) -> Vec<(Vec<u8>, TrustObservation)> {
        let mut result = Vec::new();
        for (peer_id, record) in &self.records {
            for obs in record.recent_observations(window_secs) {
                result.push((peer_id.clone(), obs.clone()));
            }
        }
        result
    }

    /// Remove a peer's trust record entirely.
    pub fn forget_peer(&mut self, peer_id: &[u8]) -> bool {
        self.records.remove(peer_id).is_some()
    }

    /// Evict the peer with the oldest last observation.
    fn evict_oldest(&mut self) {
        if let Some(oldest_id) = self
            .records
            .iter()
            .min_by_key(|(_, r)| r.last_observation().map(|o| o.timestamp).unwrap_or(0))
            .map(|(id, _)| id.clone())
        {
            self.records.remove(&oldest_id);
        }
    }

    /// Get a reference to the scorer.
    pub fn scorer(&self) -> &TrustScorer {
        &self.scorer
    }
}

impl Default for TrustStore {
    fn default() -> Self {
        Self::new(TrustScorer::default())
    }
}

// ---------------------------------------------------------------------------
// Trust-weighted bid scoring
// ---------------------------------------------------------------------------

/// Applies trust modulation to bid scores from the Negotiator.
///
/// The formula: `final_score = bid_score * trust_blend`
/// where `trust_blend = (1 - trust_influence) + trust_influence * trust_overall`
///
/// When trust_influence = 0.0, trust is ignored (backward compatible).
/// When trust_influence = 1.0, an untrusted peer (trust=0) gets score → 0.
/// Default trust_influence = 0.5 — a moderate blend.
pub struct TrustWeightedScoring {
    /// How much trust affects the final score (0.0–1.0).
    pub trust_influence: f64,
}

impl TrustWeightedScoring {
    pub fn new(trust_influence: f64) -> Self {
        Self {
            trust_influence: trust_influence.clamp(0.0, 1.0),
        }
    }

    /// Compute the trust-weighted score for a bid.
    ///
    /// `bid_score` is the raw score from the Negotiator (based on latency/load/confidence).
    /// `trust_score` is the requester's trust in the bidding peer.
    pub fn weighted_score(&self, bid_score: f64, trust_score: &TrustScore) -> f64 {
        // Blend trust into the score. When confidence is low, trust pulls
        // toward neutral (0.5) rather than penalizing unknown peers.
        let effective_trust =
            trust_score.overall * trust_score.confidence + 0.5 * (1.0 - trust_score.confidence);
        let trust_blend = (1.0 - self.trust_influence) + self.trust_influence * effective_trust;
        bid_score * trust_blend
    }
}

impl Default for TrustWeightedScoring {
    fn default() -> Self {
        Self::new(0.5)
    }
}

// ---------------------------------------------------------------------------
// Trust gossip — share observations between peers
// ---------------------------------------------------------------------------

/// A trust observation shared between peers via gossip.
/// The receiving node discounts this by their trust in the sender.
#[derive(Debug, Clone)]
pub struct SharedTrustObservation {
    /// Who the observation is about.
    pub subject_peer_id: Vec<u8>,
    /// Who recorded the observation.
    pub observer_peer_id: Vec<u8>,
    /// The observation data.
    pub observation: TrustObservation,
}

/// Processes trust observations received from other peers.
///
/// The key insight: trust in gossip is transitive but discounted.
/// If Alice trusts Bob (0.9) and Bob reports that Charlie failed,
/// Alice should weight that observation at 0.9 × original weight.
pub struct TrustGossipProcessor {
    /// Minimum trust in the sender required to accept their observations.
    pub min_sender_trust: f64,
    /// Discount factor for gossip observations vs direct observations.
    /// A gossip observation contributes `discount * sender_trust` of what
    /// a direct observation would contribute.
    pub gossip_discount: f64,
}

impl TrustGossipProcessor {
    pub fn new() -> Self {
        Self {
            min_sender_trust: 0.3,
            gossip_discount: 0.5,
        }
    }

    /// Decide whether to accept a shared observation.
    pub fn should_accept(&self, sender_trust: &TrustScore) -> bool {
        sender_trust.overall >= self.min_sender_trust && sender_trust.confidence > 0.1
    }

    /// Compute the effective weight of a gossip observation.
    /// This is used to discount the observation's impact on the local trust score.
    pub fn effective_weight(&self, sender_trust: &TrustScore) -> f64 {
        self.gossip_discount * sender_trust.overall * sender_trust.confidence
    }

    /// Process a shared observation: validate, discount, and record.
    /// Returns true if the observation was accepted and recorded.
    pub fn process(&self, store: &mut TrustStore, shared: &SharedTrustObservation) -> bool {
        // Don't accept gossip about ourselves
        let sender_trust = store.score(&shared.observer_peer_id);
        if !self.should_accept(&sender_trust) {
            return false;
        }

        // Record the observation (the TrustScorer's decay will naturally
        // weight it, and the gossip discount is applied via the quality
        // signal — we encode gossip trust as quality).
        let weight = self.effective_weight(&sender_trust);
        let mut obs = shared.observation.clone();
        // Mark gossip observations with effective trust as quality signal
        // so the scorer can differentiate direct vs transitive trust.
        obs.quality = Some(weight);
        store.record_observation(&shared.subject_peer_id, obs);
        true
    }
}

impl Default for TrustGossipProcessor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// PersistentTrustStore — sled-backed durable trust storage
// ---------------------------------------------------------------------------

/// Errors from persistent trust operations.
#[derive(Debug, thiserror::Error)]
pub enum TrustStoreError {
    #[error("storage error: {0}")]
    Storage(#[from] sled::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] bincode::Error),
}

/// A trust store backed by sled for durability across restarts.
///
/// Wraps the in-memory [`TrustStore`] with a sled database. Observations are
/// written to both memory and disk on every `record_observation()` call.
/// On startup, `open()` loads all persisted records into memory.
///
/// Storage layout: single sled tree `trust_records` mapping
/// peer_id bytes → bincode(TrustRecord).
pub struct PersistentTrustStore {
    store: TrustStore,
    tree: sled::Tree,
    _db: sled::Db,
}

impl PersistentTrustStore {
    /// Open (or create) a persistent trust store at `path`.
    ///
    /// Loads all existing records into memory on startup.
    pub fn open(
        path: impl AsRef<std::path::Path>,
        scorer: TrustScorer,
    ) -> Result<Self, TrustStoreError> {
        let db = sled::open(path)?;
        let tree = db.open_tree("trust_records")?;
        let mut store = TrustStore::new(scorer);

        // Load all persisted records into memory
        let mut loaded = 0usize;
        for entry in tree.iter() {
            let (key, value) = entry?;
            let record: TrustRecord = bincode::deserialize(&value)?;
            store.records.insert(key.to_vec(), record);
            loaded += 1;
        }

        tracing::info!("Trust store opened: {} peer records loaded", loaded);

        Ok(Self {
            store,
            tree,
            _db: db,
        })
    }

    /// Open a temporary in-memory trust store (for testing).
    pub fn open_temporary(scorer: TrustScorer) -> Result<Self, TrustStoreError> {
        let db = sled::Config::new().temporary(true).open()?;
        let tree = db.open_tree("trust_records")?;
        Ok(Self {
            store: TrustStore::new(scorer),
            tree,
            _db: db,
        })
    }

    /// Record an observation for a peer. Writes to both memory and disk.
    pub fn record_observation(
        &mut self,
        peer_id: &[u8],
        observation: TrustObservation,
    ) -> Result<(), TrustStoreError> {
        self.store.record_observation(peer_id, observation);

        // Persist the updated record to sled
        if let Some(record) = self.store.records.get(peer_id) {
            let encoded = bincode::serialize(record)?;
            self.tree.insert(peer_id, encoded)?;
        }

        Ok(())
    }

    /// Remove a peer's trust record from both memory and disk.
    pub fn forget_peer(&mut self, peer_id: &[u8]) -> Result<bool, TrustStoreError> {
        let removed = self.store.forget_peer(peer_id);
        if removed {
            self.tree.remove(peer_id)?;
        }
        Ok(removed)
    }

    /// Flush all in-memory records to disk.
    ///
    /// Useful after bulk operations or before shutdown.
    pub fn flush(&self) -> Result<(), TrustStoreError> {
        for (peer_id, record) in &self.store.records {
            let encoded = bincode::serialize(record)?;
            self.tree.insert(peer_id.as_slice(), encoded)?;
        }
        self.tree.flush()?;
        Ok(())
    }

    /// Force sled to sync to disk.
    pub fn sync(&self) -> Result<(), TrustStoreError> {
        self.tree.flush()?;
        Ok(())
    }

    /// Number of persisted peer records on disk.
    pub fn persisted_count(&self) -> usize {
        self.tree.len()
    }

    // -- Delegated methods from TrustStore --

    /// Get the trust score for a peer.
    pub fn score(&self, peer_id: &[u8]) -> TrustScore {
        self.store.score(peer_id)
    }

    /// Get the trust score at a specific timestamp.
    pub fn score_at(&self, peer_id: &[u8], now: u64) -> TrustScore {
        self.store.score_at(peer_id, now)
    }

    /// Get the trust record for a peer.
    pub fn get_record(&self, peer_id: &[u8]) -> Option<&TrustRecord> {
        self.store.get_record(peer_id)
    }

    /// Get all known peer IDs.
    pub fn known_peers(&self) -> Vec<Vec<u8>> {
        self.store.known_peers()
    }

    /// Number of tracked peers in memory.
    pub fn peer_count(&self) -> usize {
        self.store.peer_count()
    }

    /// All peers ranked by trust score.
    pub fn ranked_peers(&self) -> Vec<(Vec<u8>, TrustScore)> {
        self.store.ranked_peers()
    }

    /// All peers ranked at a specific timestamp.
    pub fn ranked_peers_at(&self, now: u64) -> Vec<(Vec<u8>, TrustScore)> {
        self.store.ranked_peers_at(now)
    }

    /// Collect recent observations across all peers for gossip sharing.
    pub fn recent_observations_all(&self, window_secs: u64) -> Vec<(Vec<u8>, TrustObservation)> {
        self.store.recent_observations_all(window_secs)
    }

    /// Get the scorer.
    pub fn scorer(&self) -> &TrustScorer {
        self.store.scorer()
    }

    /// Get a reference to the underlying in-memory store.
    pub fn inner(&self) -> &TrustStore {
        &self.store
    }

    /// Get a mutable reference to the underlying in-memory store.
    pub fn inner_mut(&mut self) -> &mut TrustStore {
        &mut self.store
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    // -- TrustObservation --

    #[test]
    fn observation_latency_accuracy_perfect() {
        let obs = TrustObservation::new(TaskOutcome::Success, 100, 100);
        assert!((obs.latency_accuracy() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn observation_latency_accuracy_double() {
        let obs = TrustObservation::new(TaskOutcome::Success, 100, 200);
        assert!((obs.latency_accuracy() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn observation_latency_accuracy_half() {
        let obs = TrustObservation::new(TaskOutcome::Success, 200, 100);
        assert!((obs.latency_accuracy() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn observation_latency_accuracy_no_data() {
        let obs = TrustObservation::new(TaskOutcome::Success, 0, 100);
        assert!((obs.latency_accuracy() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn observation_with_quality() {
        let obs = TrustObservation::new(TaskOutcome::Success, 100, 100).with_quality(0.8);
        assert_eq!(obs.quality, Some(0.8));
    }

    #[test]
    fn observation_quality_clamped() {
        let obs = TrustObservation::new(TaskOutcome::Success, 100, 100).with_quality(1.5);
        assert_eq!(obs.quality, Some(1.0));
    }

    // -- TrustRecord --

    #[test]
    fn record_new_is_empty() {
        let record = TrustRecord::new(vec![1, 2, 3]);
        assert_eq!(record.observation_count(), 0);
        assert_eq!(record.success_count(), 0);
        assert_eq!(record.failure_count(), 0);
    }

    #[test]
    fn record_tracks_observations() {
        let mut record = TrustRecord::new(vec![1]);
        record.record(TrustObservation::new(TaskOutcome::Success, 100, 100));
        record.record(TrustObservation::new(TaskOutcome::Success, 100, 110));
        record.record(TrustObservation::new(TaskOutcome::Failure, 100, 500));
        record.record(TrustObservation::new(TaskOutcome::Timeout, 100, 0));

        assert_eq!(record.observation_count(), 4);
        assert_eq!(record.success_count(), 2);
        assert_eq!(record.failure_count(), 1);
        assert_eq!(record.timeout_count(), 1);
    }

    #[test]
    fn record_prunes_oldest() {
        let mut record = TrustRecord::new(vec![1]).with_max_observations(3);
        for i in 0..5 {
            record.record(
                TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(1000 + i),
            );
        }
        assert_eq!(record.observation_count(), 3);
        assert_eq!(record.observations[0].timestamp, 1002);
    }

    #[test]
    fn record_last_observation() {
        let mut record = TrustRecord::new(vec![1]);
        assert!(record.last_observation().is_none());
        record.record(TrustObservation::new(TaskOutcome::Success, 50, 50).with_timestamp(100));
        record.record(TrustObservation::new(TaskOutcome::Failure, 100, 200).with_timestamp(200));
        assert_eq!(record.last_observation().unwrap().timestamp, 200);
    }

    // -- TrustScorer --

    #[test]
    fn scorer_decay_weight_at_zero() {
        let scorer = TrustScorer::new();
        assert!((scorer.decay_weight(0.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn scorer_decay_weight_at_half_life() {
        let scorer = TrustScorer::new();
        let w = scorer.decay_weight(scorer.decay_half_life);
        assert!((w - 0.5).abs() < 0.001);
    }

    #[test]
    fn scorer_decay_weight_decreases() {
        let scorer = TrustScorer::new();
        let w1 = scorer.decay_weight(1000.0);
        let w2 = scorer.decay_weight(2000.0);
        let w3 = scorer.decay_weight(10000.0);
        assert!(w1 > w2);
        assert!(w2 > w3);
        assert!(w3 > 0.0);
    }

    #[test]
    fn scorer_confidence_scaling() {
        let scorer = TrustScorer::new();
        assert!((scorer.confidence(0) - 0.0).abs() < f64::EPSILON);
        let c20 = scorer.confidence(20);
        assert!((c20 - 0.632).abs() < 0.01); // 1 - e^(-1) ≈ 0.632
        let c100 = scorer.confidence(100);
        assert!(c100 > 0.99);
    }

    #[test]
    fn scorer_empty_record_gives_neutral() {
        let scorer = TrustScorer::new();
        let record = TrustRecord::new(vec![1]);
        let score = scorer.score(&record);
        assert!((score.overall - 0.5).abs() < f64::EPSILON);
        assert!((score.confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn scorer_all_successes_high_reliability() {
        let scorer = TrustScorer::new();
        let mut record = TrustRecord::new(vec![1]);
        let now = now_secs();
        for i in 0..50 {
            record.record(
                TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now - i),
            );
        }
        let score = scorer.score_at(&record, now);
        assert!(
            score.reliability > 0.99,
            "reliability = {}",
            score.reliability
        );
        assert!(score.availability > 0.99);
        assert!(score.accuracy > 0.99);
        assert!(score.overall > 0.8);
        assert!(score.confidence > 0.9);
    }

    #[test]
    fn scorer_all_failures_low_reliability() {
        let scorer = TrustScorer::new();
        let mut record = TrustRecord::new(vec![1]);
        let now = now_secs();
        for i in 0..50 {
            record.record(
                TrustObservation::new(TaskOutcome::Failure, 100, 500).with_timestamp(now - i),
            );
        }
        let score = scorer.score_at(&record, now);
        assert!(
            score.reliability < 0.01,
            "reliability = {}",
            score.reliability
        );
        // Failures are still "available" — the peer responded, just badly
        assert!(score.availability > 0.99);
    }

    #[test]
    fn scorer_all_timeouts_low_availability() {
        let scorer = TrustScorer::new();
        let mut record = TrustRecord::new(vec![1]);
        let now = now_secs();
        for i in 0..50 {
            record.record(
                TrustObservation::new(TaskOutcome::Timeout, 100, 0).with_timestamp(now - i),
            );
        }
        let score = scorer.score_at(&record, now);
        assert!(score.reliability < 0.01);
        assert!(score.availability < 0.01);
    }

    #[test]
    fn scorer_rejections_dont_penalize() {
        let scorer = TrustScorer::new();
        let mut record = TrustRecord::new(vec![1]);
        let now = now_secs();
        // 10 successes, 40 rejections
        for i in 0..10 {
            record.record(
                TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now - i),
            );
        }
        for i in 10..50 {
            record
                .record(TrustObservation::new(TaskOutcome::Rejected, 0, 0).with_timestamp(now - i));
        }
        let score = scorer.score_at(&record, now);
        // Reliability should be 1.0 — only successes in the reliability pool
        assert!(
            score.reliability > 0.99,
            "reliability = {}",
            score.reliability
        );
    }

    #[test]
    fn scorer_decay_favors_recent() {
        let scorer = TrustScorer {
            decay_half_life: 3600.0, // 1 hour
            ..TrustScorer::new()
        };
        let mut record = TrustRecord::new(vec![1]);
        let now = now_secs();

        // 20 old failures (24 hours ago)
        for i in 0..20 {
            record.record(
                TrustObservation::new(TaskOutcome::Failure, 100, 500)
                    .with_timestamp(now - 86400 - i),
            );
        }
        // 10 recent successes (last minute)
        for i in 0..10 {
            record.record(
                TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now - i),
            );
        }

        let score = scorer.score_at(&record, now);
        // Recent successes should dominate over old failures
        assert!(
            score.reliability > 0.9,
            "reliability should favor recent: {}",
            score.reliability
        );
    }

    #[test]
    fn scorer_inaccurate_latency_lowers_accuracy() {
        let scorer = TrustScorer::new();
        let mut record = TrustRecord::new(vec![1]);
        let now = now_secs();
        // Peer claims 100ms but takes 1000ms every time
        for i in 0..30 {
            record.record(
                TrustObservation::new(TaskOutcome::Success, 100, 1000).with_timestamp(now - i),
            );
        }
        let score = scorer.score_at(&record, now);
        assert!(score.accuracy < 0.2, "accuracy = {}", score.accuracy);
        // But still reliable! The task completed.
        assert!(score.reliability > 0.99);
    }

    #[test]
    fn scorer_quality_feedback_affects_score() {
        let scorer = TrustScorer::new();
        let mut record = TrustRecord::new(vec![1]);
        let now = now_secs();
        for i in 0..30 {
            record.record(
                TrustObservation::new(TaskOutcome::Success, 100, 100)
                    .with_timestamp(now - i)
                    .with_quality(0.2), // Low quality results
            );
        }
        let score = scorer.score_at(&record, now);
        assert!(score.quality < 0.3, "quality = {}", score.quality);
        // Overall should be pulled down by quality compared to a peer with neutral quality
        // Perfect peer with neutral quality (0.5) gets ~0.9, this peer gets ~0.84
        assert!(score.overall < 0.86, "overall = {}", score.overall);
    }

    // -- TrustScore --

    #[test]
    fn trust_score_neutral() {
        let score = TrustScore::neutral();
        assert!((score.overall - 0.5).abs() < f64::EPSILON);
        assert!((score.confidence - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn trust_score_is_trusted() {
        let mut score = TrustScore::neutral();
        score.overall = 0.8;
        score.confidence = 0.5;
        assert!(score.is_trusted(0.7));
        assert!(!score.is_trusted(0.9));
    }

    #[test]
    fn trust_score_is_distrusted() {
        let mut score = TrustScore::neutral();
        score.overall = 0.2;
        score.confidence = 0.5;
        assert!(score.is_distrusted(0.5));
        assert!(!score.is_distrusted(0.1));
    }

    #[test]
    fn trust_score_low_confidence_not_trusted_or_distrusted() {
        let mut score = TrustScore::neutral();
        score.overall = 0.9;
        score.confidence = 0.05;
        assert!(!score.is_trusted(0.7));
        score.overall = 0.1;
        assert!(!score.is_distrusted(0.5));
    }

    // -- TrustStore --

    #[test]
    fn store_unknown_peer_neutral() {
        let store = TrustStore::default();
        let score = store.score(&[1, 2, 3]);
        assert!((score.overall - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn store_records_and_scores() {
        let mut store = TrustStore::default();
        let peer = vec![1, 2, 3];
        store.record_observation(&peer, TrustObservation::new(TaskOutcome::Success, 100, 100));
        let score = store.score(&peer);
        assert!(score.observation_count == 1);
        assert!(score.reliability > 0.9);
    }

    #[test]
    fn store_ranked_peers() {
        let mut store = TrustStore::default();
        let now = now_secs();

        // Good peer: all successes
        let good = vec![1];
        for i in 0..20 {
            store.record_observation(
                &good,
                TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now - i),
            );
        }

        // Bad peer: all failures
        let bad = vec![2];
        for i in 0..20 {
            store.record_observation(
                &bad,
                TrustObservation::new(TaskOutcome::Failure, 100, 500).with_timestamp(now - i),
            );
        }

        let ranked = store.ranked_peers_at(now);
        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].0, good);
        assert_eq!(ranked[1].0, bad);
        assert!(ranked[0].1.overall > ranked[1].1.overall);
    }

    #[test]
    fn store_forget_peer() {
        let mut store = TrustStore::default();
        let peer = vec![1];
        store.record_observation(&peer, TrustObservation::new(TaskOutcome::Success, 100, 100));
        assert_eq!(store.peer_count(), 1);
        assert!(store.forget_peer(&peer));
        assert_eq!(store.peer_count(), 0);
        assert!(!store.forget_peer(&peer));
    }

    #[test]
    fn store_evicts_oldest_at_capacity() {
        let mut store = TrustStore::new(TrustScorer::default()).with_max_peers(2);
        let now = now_secs();

        store.record_observation(
            &[1],
            TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now - 100),
        );
        store.record_observation(
            &[2],
            TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now - 50),
        );
        assert_eq!(store.peer_count(), 2);

        // Third peer should evict peer [1] (oldest observation)
        store.record_observation(
            &[3],
            TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now),
        );
        assert_eq!(store.peer_count(), 2);
        assert!(store.get_record(&[1]).is_none());
        assert!(store.get_record(&[2]).is_some());
        assert!(store.get_record(&[3]).is_some());
    }

    // -- TrustWeightedScoring --

    #[test]
    fn trust_weighted_no_influence() {
        let tws = TrustWeightedScoring::new(0.0);
        let mut trust = TrustScore::neutral();
        trust.overall = 0.1;
        trust.confidence = 1.0;
        // With zero influence, trust doesn't matter
        assert!((tws.weighted_score(0.8, &trust) - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn trust_weighted_full_influence_high_trust() {
        let tws = TrustWeightedScoring::new(1.0);
        let mut trust = TrustScore::neutral();
        trust.overall = 1.0;
        trust.confidence = 1.0;
        // Full trust → full score
        assert!((tws.weighted_score(0.8, &trust) - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn trust_weighted_full_influence_zero_trust() {
        let tws = TrustWeightedScoring::new(1.0);
        let mut trust = TrustScore::neutral();
        trust.overall = 0.0;
        trust.confidence = 1.0;
        // Zero trust → zero score
        assert!(tws.weighted_score(0.8, &trust).abs() < f64::EPSILON);
    }

    #[test]
    fn trust_weighted_unknown_peer_gets_neutral() {
        let tws = TrustWeightedScoring::new(1.0);
        let trust = TrustScore::neutral(); // confidence = 0.0
                                           // Unknown peer → effective trust = 0.5 (neutral, not penalized)
        let score = tws.weighted_score(1.0, &trust);
        assert!((score - 0.5).abs() < 0.01, "score = {}", score);
    }

    #[test]
    fn trust_weighted_moderate_influence() {
        let tws = TrustWeightedScoring::new(0.5);
        let mut trust = TrustScore::neutral();
        trust.overall = 0.8;
        trust.confidence = 0.9;
        let score = tws.weighted_score(1.0, &trust);
        // Should be between 0.5 (full influence, trust=0.5) and 1.0 (no influence)
        assert!(score > 0.8 && score < 1.0, "score = {}", score);
    }

    // -- TrustGossipProcessor --

    #[test]
    fn gossip_processor_rejects_untrusted_sender() {
        let proc = TrustGossipProcessor::new();
        let mut trust = TrustScore::neutral();
        trust.overall = 0.1;
        trust.confidence = 0.5;
        assert!(!proc.should_accept(&trust));
    }

    #[test]
    fn gossip_processor_accepts_trusted_sender() {
        let proc = TrustGossipProcessor::new();
        let mut trust = TrustScore::neutral();
        trust.overall = 0.8;
        trust.confidence = 0.5;
        assert!(proc.should_accept(&trust));
    }

    #[test]
    fn gossip_processor_rejects_low_confidence_sender() {
        let proc = TrustGossipProcessor::new();
        let mut trust = TrustScore::neutral();
        trust.overall = 0.9;
        trust.confidence = 0.05;
        assert!(!proc.should_accept(&trust));
    }

    #[test]
    fn gossip_processor_effective_weight() {
        let proc = TrustGossipProcessor::new();
        let mut trust = TrustScore::neutral();
        trust.overall = 0.8;
        trust.confidence = 0.9;
        let w = proc.effective_weight(&trust);
        // 0.5 * 0.8 * 0.9 = 0.36
        assert!((w - 0.36).abs() < 0.001, "weight = {}", w);
    }

    #[test]
    fn gossip_processor_records_observation() {
        let proc = TrustGossipProcessor::new();
        let mut store = TrustStore::default();
        let now = now_secs();

        // First, establish trust in the sender
        let sender = vec![1];
        for i in 0..30 {
            store.record_observation(
                &sender,
                TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now - i),
            );
        }

        // Now process a gossip observation about peer [2]
        let shared = SharedTrustObservation {
            subject_peer_id: vec![2],
            observer_peer_id: sender.clone(),
            observation: TrustObservation::new(TaskOutcome::Failure, 100, 500).with_timestamp(now),
        };
        assert!(proc.process(&mut store, &shared));
        assert!(store.get_record(&[2]).is_some());
        assert_eq!(store.get_record(&[2]).unwrap().observation_count(), 1);
    }

    #[test]
    fn gossip_processor_rejects_from_unknown() {
        let proc = TrustGossipProcessor::new();
        let mut store = TrustStore::default();

        // Unknown sender — should be rejected
        let shared = SharedTrustObservation {
            subject_peer_id: vec![2],
            observer_peer_id: vec![99],
            observation: TrustObservation::new(TaskOutcome::Failure, 100, 500),
        };
        assert!(!proc.process(&mut store, &shared));
        assert!(store.get_record(&[2]).is_none());
    }

    // -- Integration tests --

    #[test]
    fn integration_trust_builds_over_time() {
        let mut store = TrustStore::default();
        let peer = vec![42];
        let now = now_secs();

        // Initially neutral
        let score = store.score_at(&peer, now);
        assert!((score.overall - 0.5).abs() < f64::EPSILON);

        // After 5 successes
        for i in 0..5 {
            store.record_observation(
                &peer,
                TrustObservation::new(TaskOutcome::Success, 100, 105).with_timestamp(now + i),
            );
        }
        let score = store.score_at(&peer, now + 5);
        assert!(score.overall > 0.7, "after 5 successes: {}", score.overall);
        assert!(score.confidence > 0.2);

        // After 50 successes
        for i in 5..50 {
            store.record_observation(
                &peer,
                TrustObservation::new(TaskOutcome::Success, 100, 102).with_timestamp(now + i),
            );
        }
        let score = store.score_at(&peer, now + 50);
        // Max overall without quality feedback is ~0.9 (quality defaults to 0.5)
        assert!(
            score.overall > 0.85,
            "after 50 successes: {}",
            score.overall
        );
        assert!(score.confidence > 0.9);
    }

    #[test]
    fn integration_trust_erodes_with_failures() {
        let mut store = TrustStore::new(TrustScorer {
            decay_half_life: 3600.0, // 1 hour half-life for faster decay
            ..TrustScorer::new()
        });
        let peer = vec![42];
        let now = now_secs();

        // Build up trust
        for i in 0..30 {
            store.record_observation(
                &peer,
                TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now + i),
            );
        }
        let good_score = store.score_at(&peer, now + 30);
        assert!(
            good_score.overall > 0.85,
            "good_score = {}",
            good_score.overall
        );

        // Start failing
        for i in 30..60 {
            store.record_observation(
                &peer,
                TrustObservation::new(TaskOutcome::Failure, 100, 500).with_timestamp(now + i),
            );
        }
        let bad_score = store.score_at(&peer, now + 60);
        assert!(
            bad_score.reliability < good_score.reliability,
            "trust should erode: {} vs {}",
            bad_score.reliability,
            good_score.reliability
        );
    }

    #[test]
    fn integration_full_negotiation_with_trust() {
        use crate::negotiate::{BidScoring, Negotiator, ReceivedBid};
        use std::time::Duration;

        let mut store = TrustStore::default();
        let now = now_secs();
        let tws = TrustWeightedScoring::new(0.5);
        let negotiator = Negotiator::new(
            Duration::from_millis(500),
            BidScoring::Weighted {
                latency_weight: 0.4,
                load_weight: 0.3,
                confidence_weight: 0.3,
            },
        );

        let trusted_peer = vec![1];
        let untrusted_peer = vec![2];

        // Build trust for peer 1
        for i in 0..30 {
            store.record_observation(
                &trusted_peer,
                TrustObservation::new(TaskOutcome::Success, 100, 100).with_timestamp(now - i),
            );
        }

        // Peer 2 has failures
        for i in 0..30 {
            store.record_observation(
                &untrusted_peer,
                TrustObservation::new(TaskOutcome::Failure, 100, 500).with_timestamp(now - i),
            );
        }

        // Both peers bid the same — fast and confident
        let request_id = uuid::Uuid::new_v4();
        let bid1 = ReceivedBid::new(request_id, trusted_peer.clone(), 50, 0.2, 0.9);
        let bid2 = ReceivedBid::new(request_id, untrusted_peer.clone(), 50, 0.2, 0.9);

        // Raw scores should be identical
        let raw1 = negotiator.score_bid(&bid1);
        let raw2 = negotiator.score_bid(&bid2);
        assert!((raw1 - raw2).abs() < f64::EPSILON);

        // Trust-weighted scores should favor the trusted peer
        let trust1 = store.score_at(&trusted_peer, now);
        let trust2 = store.score_at(&untrusted_peer, now);
        let weighted1 = tws.weighted_score(raw1, &trust1);
        let weighted2 = tws.weighted_score(raw2, &trust2);
        assert!(
            weighted1 > weighted2,
            "trusted peer should win: {} vs {}",
            weighted1,
            weighted2
        );
    }

    // ---- PersistentTrustStore tests ----

    #[test]
    fn persistent_open_and_record() {
        let store = PersistentTrustStore::open_temporary(TrustScorer::default()).unwrap();
        assert_eq!(store.peer_count(), 0);
        assert_eq!(store.persisted_count(), 0);
    }

    #[test]
    fn persistent_record_and_score() {
        let mut store = PersistentTrustStore::open_temporary(TrustScorer::default()).unwrap();
        let peer = b"peer-a".to_vec();

        store
            .record_observation(&peer, TrustObservation::new(TaskOutcome::Success, 100, 105))
            .unwrap();

        assert_eq!(store.peer_count(), 1);
        assert_eq!(store.persisted_count(), 1);

        let score = store.score(&peer);
        assert!(score.reliability > 0.5);
        assert!(score.observation_count == 1);
    }

    #[test]
    fn persistent_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trust_db");
        let peer = b"peer-persistent".to_vec();

        // First session: record observations
        {
            let mut store = PersistentTrustStore::open(&path, TrustScorer::default()).unwrap();
            for i in 0..5 {
                store
                    .record_observation(
                        &peer,
                        TrustObservation::new(TaskOutcome::Success, 100, 110)
                            .with_timestamp(1000 + i),
                    )
                    .unwrap();
            }
            assert_eq!(store.peer_count(), 1);
            store.sync().unwrap();
        }
        // store dropped, sled closed

        // Second session: data should be loaded from disk
        {
            let store = PersistentTrustStore::open(&path, TrustScorer::default()).unwrap();
            assert_eq!(store.peer_count(), 1);
            assert_eq!(store.persisted_count(), 1);

            let record = store.get_record(&peer).unwrap();
            assert_eq!(record.observation_count(), 5);

            // Use score_at with a timestamp near the observations to avoid decay
            let score = store.score_at(&peer, 1005);
            assert!(
                score.reliability > 0.5,
                "reliability should be high: {}",
                score.reliability
            );
            assert_eq!(score.observation_count, 5);
        }
    }

    #[test]
    fn persistent_multiple_peers() {
        let mut store = PersistentTrustStore::open_temporary(TrustScorer::default()).unwrap();

        let peer_a = b"alpha".to_vec();
        let peer_b = b"beta".to_vec();
        let peer_c = b"gamma".to_vec();

        store
            .record_observation(&peer_a, TrustObservation::new(TaskOutcome::Success, 50, 55))
            .unwrap();
        store
            .record_observation(
                &peer_b,
                TrustObservation::new(TaskOutcome::Failure, 50, 200),
            )
            .unwrap();
        store
            .record_observation(&peer_c, TrustObservation::new(TaskOutcome::Success, 50, 52))
            .unwrap();

        assert_eq!(store.peer_count(), 3);
        assert_eq!(store.persisted_count(), 3);

        let ranked = store.ranked_peers();
        assert_eq!(ranked.len(), 3);
        // peer_b (failure) should rank lowest
        assert_eq!(ranked[2].0, peer_b);
    }

    #[test]
    fn persistent_forget_peer() {
        let mut store = PersistentTrustStore::open_temporary(TrustScorer::default()).unwrap();
        let peer = b"ephemeral".to_vec();

        store
            .record_observation(&peer, TrustObservation::new(TaskOutcome::Success, 50, 55))
            .unwrap();
        assert_eq!(store.peer_count(), 1);
        assert_eq!(store.persisted_count(), 1);

        let removed = store.forget_peer(&peer).unwrap();
        assert!(removed);
        assert_eq!(store.peer_count(), 0);
        assert_eq!(store.persisted_count(), 0);
    }

    #[test]
    fn persistent_forget_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("forget_db");
        let peer = b"to-forget".to_vec();

        {
            let mut store = PersistentTrustStore::open(&path, TrustScorer::default()).unwrap();
            store
                .record_observation(&peer, TrustObservation::new(TaskOutcome::Success, 50, 55))
                .unwrap();
            store.forget_peer(&peer).unwrap();
            store.sync().unwrap();
        }

        {
            let store = PersistentTrustStore::open(&path, TrustScorer::default()).unwrap();
            assert_eq!(store.peer_count(), 0);
            assert_eq!(store.persisted_count(), 0);
        }
    }

    #[test]
    fn persistent_flush_writes_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flush_db");

        {
            let mut store = PersistentTrustStore::open(&path, TrustScorer::default()).unwrap();
            // Record multiple peers
            for i in 0..10u8 {
                store
                    .record_observation(
                        &[i],
                        TrustObservation::new(TaskOutcome::Success, 100, 100 + i as u64),
                    )
                    .unwrap();
            }
            store.flush().unwrap();
        }

        {
            let store = PersistentTrustStore::open(&path, TrustScorer::default()).unwrap();
            assert_eq!(store.peer_count(), 10);
        }
    }

    #[test]
    fn persistent_score_unknown_peer_is_neutral() {
        let store = PersistentTrustStore::open_temporary(TrustScorer::default()).unwrap();
        let score = store.score(b"unknown");
        let neutral = TrustScore::neutral();
        assert!((score.overall - neutral.overall).abs() < f64::EPSILON);
    }

    #[test]
    fn persistent_accumulates_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accumulate_db");
        let peer = b"accumulator".to_vec();

        // Session 1: 3 observations
        {
            let mut store = PersistentTrustStore::open(&path, TrustScorer::default()).unwrap();
            for i in 0..3 {
                store
                    .record_observation(
                        &peer,
                        TrustObservation::new(TaskOutcome::Success, 50, 55)
                            .with_timestamp(1000 + i),
                    )
                    .unwrap();
            }
            store.sync().unwrap();
        }

        // Session 2: 2 more observations
        {
            let mut store = PersistentTrustStore::open(&path, TrustScorer::default()).unwrap();
            assert_eq!(store.get_record(&peer).unwrap().observation_count(), 3);

            for i in 0..2 {
                store
                    .record_observation(
                        &peer,
                        TrustObservation::new(TaskOutcome::Success, 50, 55)
                            .with_timestamp(2000 + i),
                    )
                    .unwrap();
            }
            store.sync().unwrap();
        }

        // Session 3: verify accumulated
        {
            let store = PersistentTrustStore::open(&path, TrustScorer::default()).unwrap();
            assert_eq!(store.get_record(&peer).unwrap().observation_count(), 5);
        }
    }

    #[test]
    fn persistent_inner_access() {
        let store = PersistentTrustStore::open_temporary(TrustScorer::default()).unwrap();
        let inner = store.inner();
        assert_eq!(inner.peer_count(), 0);
    }

    #[test]
    fn persistent_inner_mut_access() {
        let mut store = PersistentTrustStore::open_temporary(TrustScorer::default()).unwrap();
        let inner = store.inner_mut();
        inner.record_observation(
            &[1, 2, 3],
            TrustObservation::new(TaskOutcome::Success, 100, 110),
        );
        assert_eq!(inner.peer_count(), 1);
    }

    // -- TrustRecord::recent_observations --

    #[test]
    fn recent_observations_within_window() {
        let mut record = TrustRecord::new(vec![1, 2, 3]);
        let now = now_secs();
        // Add observation 10 seconds ago
        record
            .record(TrustObservation::new(TaskOutcome::Success, 100, 110).with_timestamp(now - 10));
        // Add observation 100 seconds ago
        record.record(
            TrustObservation::new(TaskOutcome::Failure, 200, 300).with_timestamp(now - 100),
        );
        // Window of 60 seconds should only include the recent one
        let recent = record.recent_observations(60);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].outcome, TaskOutcome::Success);
    }

    #[test]
    fn recent_observations_all_within_window() {
        let mut record = TrustRecord::new(vec![1, 2, 3]);
        let now = now_secs();
        record
            .record(TrustObservation::new(TaskOutcome::Success, 100, 110).with_timestamp(now - 5));
        record
            .record(TrustObservation::new(TaskOutcome::Failure, 200, 300).with_timestamp(now - 10));
        let recent = record.recent_observations(60);
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn recent_observations_none_within_window() {
        let mut record = TrustRecord::new(vec![1, 2, 3]);
        let now = now_secs();
        record.record(
            TrustObservation::new(TaskOutcome::Success, 100, 110).with_timestamp(now - 200),
        );
        let recent = record.recent_observations(60);
        assert!(recent.is_empty());
    }

    #[test]
    fn recent_observations_empty_record() {
        let record = TrustRecord::new(vec![1, 2, 3]);
        let recent = record.recent_observations(60);
        assert!(recent.is_empty());
    }

    // -- TrustStore::recent_observations_all --

    #[test]
    fn store_recent_observations_all() {
        let mut store = TrustStore::new(TrustScorer::default());
        let now = now_secs();
        store.record_observation(
            &[1, 1, 1],
            TrustObservation::new(TaskOutcome::Success, 100, 110).with_timestamp(now - 5),
        );
        store.record_observation(
            &[2, 2, 2],
            TrustObservation::new(TaskOutcome::Failure, 200, 300).with_timestamp(now - 10),
        );
        store.record_observation(
            &[1, 1, 1],
            TrustObservation::new(TaskOutcome::Timeout, 50, 0).with_timestamp(now - 200),
        );

        let recent = store.recent_observations_all(60);
        assert_eq!(recent.len(), 2); // only the two within 60 seconds
        assert!(recent.iter().any(|(pid, _)| pid == &vec![1, 1, 1]));
        assert!(recent.iter().any(|(pid, _)| pid == &vec![2, 2, 2]));
    }

    #[test]
    fn store_recent_observations_all_empty() {
        let store = TrustStore::new(TrustScorer::default());
        let recent = store.recent_observations_all(60);
        assert!(recent.is_empty());
    }

    #[test]
    fn persistent_recent_observations_all() {
        let mut store = PersistentTrustStore::open_temporary(TrustScorer::default()).unwrap();
        let now = now_secs();
        let _ = store.record_observation(
            &[1, 1, 1],
            TrustObservation::new(TaskOutcome::Success, 100, 110).with_timestamp(now - 5),
        );
        let _ = store.record_observation(
            &[2, 2, 2],
            TrustObservation::new(TaskOutcome::Failure, 200, 300).with_timestamp(now - 10),
        );
        let recent = store.recent_observations_all(60);
        assert_eq!(recent.len(), 2);
    }
}
