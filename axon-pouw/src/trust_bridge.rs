//! Trust bridge: fold quorum outcomes back into axon's [`TrustStore`].
//!
//! When a [`QuorumCertificate`] finalizes, every agreeing worker gets a
//! `TaskOutcome::Success` observation; every dissenting worker gets a
//! `TaskOutcome::Failure`. Axon's existing [`TrustScorer`] then turns those
//! observations into refreshed [`TrustScore`]s, which v1 will gossip back
//! to other nodes for their selection / quorum weighting.

use axon_core::trust::{TaskOutcome, TrustObservation, TrustStore};
use nous_pouw::quorum::QuorumCertificate;
use nous_pouw::state::WorkerId;

/// Map a single (worker, cert) pair to a `TrustObservation`.
pub fn observation_for(
    cert: &QuorumCertificate,
    worker: &WorkerId,
    estimated_latency_ms: u64,
    actual_latency_ms: u64,
) -> TrustObservation {
    let outcome = if cert.agreeing_workers.contains(worker) {
        TaskOutcome::Success
    } else if cert.dissenting_workers.contains(worker) {
        TaskOutcome::Failure
    } else {
        // Worker wasn't in this cert at all — treat as no-op (Success with
        // zero quality so it barely moves the score). Callers should usually
        // skip workers not in the cert.
        TaskOutcome::Success
    };
    TrustObservation::new(outcome, estimated_latency_ms, actual_latency_ms)
        .with_quality(cert.agreement_micro as f64 / 1_000_000.0)
}

/// One pass over one cert: write observations for every participating worker.
pub struct TrustBridge<'a> {
    pub store: &'a mut TrustStore,
}

impl<'a> TrustBridge<'a> {
    pub fn new(store: &'a mut TrustStore) -> Self {
        Self { store }
    }

    pub fn record_cert(&mut self, cert: &QuorumCertificate, latency_ms: u64) {
        for w in &cert.agreeing_workers {
            let obs = observation_for(cert, w, latency_ms, latency_ms);
            self.store.record_observation(&w.0, obs);
        }
        for w in &cert.dissenting_workers {
            let obs = observation_for(cert, w, latency_ms, latency_ms);
            self.store.record_observation(&w.0, obs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_core::trust::TrustScorer;
    use nous_pouw::envelope::JobId;

    fn worker(seed: u8) -> WorkerId {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        WorkerId(bytes)
    }

    fn cert(agreeing: Vec<WorkerId>, dissenting: Vec<WorkerId>) -> QuorumCertificate {
        QuorumCertificate {
            job_id: JobId([0; 32]),
            output_hash: [0xab; 32],
            bounty: 100,
            agreeing_workers: agreeing,
            dissenting_workers: dissenting,
            agreement_micro: 800_000,
        }
    }

    #[test]
    fn agreeing_worker_observed_success() {
        let w = worker(1);
        let c = cert(vec![w], vec![]);
        let obs = observation_for(&c, &w, 100, 100);
        assert_eq!(obs.outcome, TaskOutcome::Success);
    }

    #[test]
    fn dissenting_worker_observed_failure() {
        let w = worker(2);
        let c = cert(vec![worker(1)], vec![w]);
        let obs = observation_for(&c, &w, 100, 100);
        assert_eq!(obs.outcome, TaskOutcome::Failure);
    }

    #[test]
    fn quality_reflects_cert_agreement() {
        let w = worker(1);
        let c = cert(vec![w], vec![]);
        let obs = observation_for(&c, &w, 100, 100);
        let q = obs.quality.expect("quality set");
        assert!((q - 0.8).abs() < 1e-6);
    }

    #[test]
    fn record_cert_updates_store_for_all_participants() {
        let scorer = TrustScorer::default();
        let mut store = TrustStore::new(scorer);
        let agree = vec![worker(1), worker(2)];
        let dissent = vec![worker(3)];
        let c = cert(agree.clone(), dissent.clone());
        let mut bridge = TrustBridge::new(&mut store);
        bridge.record_cert(&c, 50);
        assert_eq!(store.peer_count(), 3);
    }

    #[test]
    fn agreeing_workers_score_higher_than_dissenters() {
        let scorer = TrustScorer::default();
        let mut store = TrustStore::new(scorer);
        let winner = worker(1);
        let loser = worker(2);
        let c = cert(vec![winner], vec![loser]);
        let mut bridge = TrustBridge::new(&mut store);
        // Multiple rounds to amplify the difference.
        for _ in 0..5 {
            bridge.record_cert(&c, 10);
        }
        let win_score = store.score(&winner.0);
        let lose_score = store.score(&loser.0);
        assert!(
            win_score.overall > lose_score.overall,
            "winner score {} should beat loser {}",
            win_score.overall,
            lose_score.overall
        );
    }
}
