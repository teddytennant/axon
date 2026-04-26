//! Integration tests for [`axon_pouw::RealAxonRunner`].
//!
//! These exercise the full `WorkflowBlob` → axon pipeline → trace events
//! pipeline, and verify quorum formation when the runner is used inside
//! `nous_pouw::Engine::step` via [`axon_pouw::Adapter`].

use std::sync::Arc;

use axon_core::protocol::Capability;
use axon_core::runtime::Runtime;
use axon_pouw::{
    Adapter, AxonRunner, RealAxonRunner, RunnerError, StageSpec, TraceEvent, WorkflowBlob,
};
use ed25519_dalek::SigningKey;
use nous_pouw::engine::{Engine, EngineConfig};
use nous_pouw::envelope::{JobEnvelope, ModelPin};
use nous_pouw::state::{ChainState, WorkerId};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

fn worker_id() -> WorkerId {
    let sk = SigningKey::generate(&mut rand::rngs::OsRng);
    WorkerId::from_verifying_key(&sk.verifying_key())
}

fn job_for(blob: &WorkflowBlob, nonce: u64) -> JobEnvelope {
    JobEnvelope {
        nonce,
        workflow_cid: [0; 32],
        workflow_payload: blob.encode(),
        model: ModelPin::new("axon-real", nonce),
        n_replicas: 5,
        bounty: 1_000,
        deadline_ms: 60_000,
    }
}

fn stage(ns: &str, name: &str) -> StageSpec {
    StageSpec {
        capability_namespace: ns.to_string(),
        capability_name: name.to_string(),
        capability_version: 1,
        timeout_ms: 5_000,
        extract_field: None,
    }
}

#[test]
fn single_stage_pipeline() {
    let rt = Arc::new(Runtime::new());
    let mut runner = RealAxonRunner::new(rt).unwrap();

    runner.block_on(runner.register_stub(Capability::new("echo", "reverse", 1), |bytes| {
        let mut v = bytes.to_vec();
        v.reverse();
        v
    }));

    let input = b"abcdef".to_vec();
    let blob = WorkflowBlob {
        initial_input: input.clone(),
        stages: vec![stage("echo", "reverse")],
    };
    let out = runner.run(&worker_id(), &job_for(&blob, 1)).unwrap();

    let mut expected = input.clone();
    expected.reverse();
    assert_eq!(out.output, expected);

    // Hash of the final payload should match blake3(reverse(input)).
    let want_hash = *blake3::hash(&expected).as_bytes();
    // The trace's last StepComplete output_hash is the per-step hash.
    match out.trace_events[1] {
        TraceEvent::StepComplete { output_hash, .. } => assert_eq!(output_hash, want_hash),
        _ => panic!("expected StepComplete at index 1"),
    }
}

#[test]
fn multi_stage_pipeline() {
    let rt = Arc::new(Runtime::new());
    let mut runner = RealAxonRunner::new(rt).unwrap();

    // Stage 1: prepend "hello-".
    runner.block_on(runner.register_stub(Capability::new("concat", "hello", 1), |bytes| {
        let mut out = b"hello-".to_vec();
        out.extend_from_slice(bytes);
        out
    }));
    // Stage 2: uppercase.
    runner.block_on(runner.register_stub(Capability::new("concat", "upper", 1), |bytes| {
        bytes.to_ascii_uppercase()
    }));

    let blob = WorkflowBlob {
        initial_input: b"input".to_vec(),
        stages: vec![stage("concat", "hello"), stage("concat", "upper")],
    };
    let out = runner.run(&worker_id(), &job_for(&blob, 2)).unwrap();
    assert_eq!(out.output, b"HELLO-INPUT");
}

#[test]
fn trace_events_match_stage_count() {
    let rt = Arc::new(Runtime::new());
    let mut runner = RealAxonRunner::new(rt).unwrap();

    runner.block_on(runner.register_stub(Capability::new("nop", "passthrough", 1), |b| {
        b.to_vec()
    }));

    for n in [1usize, 2, 3, 5] {
        let stages: Vec<StageSpec> = (0..n).map(|_| stage("nop", "passthrough")).collect();
        let blob = WorkflowBlob {
            initial_input: b"x".to_vec(),
            stages,
        };
        let out = runner.run(&worker_id(), &job_for(&blob, n as u64)).unwrap();
        assert_eq!(
            out.trace_events.len(),
            n + 2,
            "n={n} should yield n+2 trace events (Start + n×StepComplete + End)"
        );
        assert!(matches!(out.trace_events[0], TraceEvent::WorkflowStart));
        assert!(matches!(
            out.trace_events.last(),
            Some(TraceEvent::WorkflowEnd { ok: true })
        ));
        for i in 0..n {
            match &out.trace_events[1 + i] {
                TraceEvent::StepComplete { step, .. } => assert_eq!(*step as usize, i),
                other => panic!("unexpected event at {}: {:?}", 1 + i, other),
            }
        }
    }
}

#[test]
fn unknown_capability_returns_runner_error() {
    let rt = Arc::new(Runtime::new());
    let mut runner = RealAxonRunner::new(rt).unwrap();
    // No agents registered.
    let blob = WorkflowBlob {
        initial_input: b"x".to_vec(),
        stages: vec![stage("missing", "nope")],
    };
    let err = runner
        .run(&worker_id(), &job_for(&blob, 7))
        .expect_err("should fail without panic");
    assert!(matches!(err, RunnerError::Failed(_)));
}

#[test]
fn runner_is_deterministic_across_calls() {
    let rt = Arc::new(Runtime::new());
    let mut runner = RealAxonRunner::new(rt).unwrap();

    runner.block_on(runner.register_stub(Capability::new("det", "double", 1), |bytes| {
        let mut v = bytes.to_vec();
        v.extend_from_slice(bytes);
        v
    }));

    let blob = WorkflowBlob {
        initial_input: b"abc".to_vec(),
        stages: vec![stage("det", "double"), stage("det", "double")],
    };
    let job = job_for(&blob, 42);
    let a = runner.run(&worker_id(), &job).unwrap();
    let b = runner.run(&worker_id(), &job).unwrap();

    let ha = *blake3::hash(&a.output).as_bytes();
    let hb = *blake3::hash(&b.output).as_bytes();
    assert_eq!(ha, hb);
    assert_eq!(a.output, b.output);
}

/// Drive `nous_pouw::Engine::step` end-to-end with `RealAxonRunner` wrapped by
/// `Adapter`. All N selected workers share one `Arc<Runtime>` (one stub agent
/// instance), so they all produce the same output bytes and form a quorum.
#[test]
fn pouw_executor_via_real_runner() {
    let mut rng = ChaCha20Rng::seed_from_u64(99);
    let sks: Vec<SigningKey> = (0..8).map(|_| SigningKey::generate(&mut rng)).collect();

    let mut state = ChainState::new();
    for sk in &sks {
        state.register_worker(WorkerId::from_verifying_key(&sk.verifying_key()), 1_000, 1.0);
    }

    let axon_rt = Arc::new(Runtime::new());
    let mut runner = RealAxonRunner::new(axon_rt.clone()).unwrap();
    runner.block_on(runner.register_stub(
        Capability::new("e2e", "shout", 1),
        |bytes| {
            let mut v = bytes.to_ascii_uppercase();
            v.extend_from_slice(b"!");
            v
        },
    ));

    let blob = WorkflowBlob {
        initial_input: b"axon".to_vec(),
        stages: vec![stage("e2e", "shout")],
    };
    let job = job_for(&blob, 1);

    let mut adapter = Adapter::new(runner, &sks);
    let mut engine = Engine::new(state, EngineConfig::default());

    let outcome = engine.step(&mut adapter, &[job], &sks[0], 0).expect("step");
    assert_eq!(outcome.block.body.certs.len(), 1, "should produce one cert");
    let cert = &outcome.block.body.certs[0];

    // No dissenters: all selected workers ran the same stub against the same
    // input and got the same payload, so they all agree on output_hash.
    assert!(
        cert.dissenting_workers.is_empty(),
        "expected unanimous quorum, got {} dissenters",
        cert.dissenting_workers.len()
    );

    // The certified output_hash should be blake3("AXON!").
    let want_hash = *blake3::hash(b"AXON!").as_bytes();
    assert_eq!(cert.output_hash, want_hash);

    // And quorum should include at least the trust-weighted majority of selected workers.
    assert!(!cert.agreeing_workers.is_empty());
}
