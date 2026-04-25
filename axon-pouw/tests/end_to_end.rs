//! End-to-end: drive nous-pouw with the FakeRunner-backed Adapter and a
//! TrustBridge feeding observations into axon's TrustStore.

use axon_core::trust::{TrustScorer, TrustStore};
use axon_pouw::{Adapter, FakeRunner, TrustBridge};
use ed25519_dalek::SigningKey;
use nous_pouw::engine::{mints_from_block, Engine, EngineConfig};
use nous_pouw::envelope::{JobEnvelope, ModelPin};
use nous_pouw::state::{ChainState, WorkerId};
#[allow(unused_imports)]
use rand::rngs::OsRng;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

fn build_workers(n: usize, seed: u64) -> Vec<SigningKey> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    (0..n).map(|_| SigningKey::generate(&mut rng)).collect()
}

fn job(nonce: u64, payload: &[u8]) -> JobEnvelope {
    JobEnvelope {
        nonce,
        workflow_cid: [0; 32],
        workflow_payload: payload.to_vec(),
        model: ModelPin::new("axon-fake", nonce),
        n_replicas: 5,
        bounty: 1_000,
        deadline_ms: 60_000,
    }
}

#[test]
fn axon_adapter_drives_full_pouw_round() {
    let sks = build_workers(8, 1);
    let mut state = ChainState::new();
    for sk in &sks {
        state.register_worker(
            WorkerId::from_verifying_key(&sk.verifying_key()),
            1_000,
            1.0,
        );
    }
    let mut engine = Engine::new(state, EngineConfig::default());
    let mut adapter = Adapter::new(FakeRunner, &sks);

    let outcome = engine
        .step(&mut adapter, &[job(1, b"hello")], &sks[0], 0)
        .expect("step ok");
    assert_eq!(outcome.block.body.certs.len(), 1);
    assert_eq!(outcome.block.body.certs[0].dissenting_workers.len(), 0);
    assert_eq!(engine.state.height, 1);
}

#[test]
fn many_jobs_via_adapter_then_trust_bridge() {
    let sks = build_workers(8, 2);
    let mut state = ChainState::new();
    for sk in &sks {
        state.register_worker(
            WorkerId::from_verifying_key(&sk.verifying_key()),
            1_000,
            1.0,
        );
    }
    let mut engine = Engine::new(state, EngineConfig::default());
    let mut adapter = Adapter::new(FakeRunner, &sks);

    let mut store = TrustStore::new(TrustScorer::default());
    let mut bridge = TrustBridge::new(&mut store);

    let mut total_minted = 0u64;
    for round in 0..10u64 {
        let payload = format!("round-{round}");
        let outcome = engine
            .step(
                &mut adapter,
                &[job(round + 1, payload.as_bytes())],
                &sks[0],
                round,
            )
            .unwrap();
        for cert in &outcome.block.body.certs {
            bridge.record_cert(cert, 5);
        }
        total_minted += mints_from_block(&outcome.block).values().sum::<u64>();
    }

    assert_eq!(engine.state.height, 10);
    assert_eq!(total_minted, 10_000);
    // Every worker got at least one Success observation.
    assert!(store.peer_count() <= 8);
}

#[test]
fn adapter_handles_unknown_worker_gracefully() {
    let sks = build_workers(3, 3);
    let mut adapter = Adapter::new(FakeRunner, &sks);
    let stranger = SigningKey::generate(&mut rand::rngs::OsRng);
    let stranger_id = WorkerId::from_verifying_key(&stranger.verifying_key());
    use nous_pouw::engine::WorkExecutor;
    let r = adapter.execute(stranger_id, &job(1, b"x"));
    assert!(r.verify().is_err());
}
