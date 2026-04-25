//! `AxonRunner`: actually execute a workflow against axon (or a stub).
//!
//! Real axon integration runs the workflow via `axon_core::orchestrate::*` —
//! that requires a live mesh + an async runtime, so it's wired up in the
//! application binary, not here. The trait is what `Adapter` consumes.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What a workflow produced for one worker.
///
/// `output` is the canonical bytes that get hashed into [`OutputHash`](nous_pouw::OutputHash);
/// `trace_events` is a per-step log used to build the trace Merkle root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowOutput {
    pub output: Vec<u8>,
    pub trace_events: Vec<crate::trace::TraceEvent>,
    pub latency_ms: u64,
    /// Optional rubric score from a judge step.
    pub rubric: Option<f32>,
}

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("workflow execution failed: {0}")]
    Failed(String),
    #[error("workflow timed out")]
    Timeout,
}

/// Anything that can run a single PoUW work unit.
pub trait AxonRunner {
    fn run(
        &mut self,
        worker: &nous_pouw::WorkerId,
        job: &nous_pouw::JobEnvelope,
    ) -> Result<WorkflowOutput, RunnerError>;
}

/// Deterministic stub: output = blake3(workflow_payload) XOR (model.seed bytes).
///
/// Used by the integration tests so that all honest workers agree byte-for-byte
/// without spinning up a real axon mesh.
#[derive(Debug, Default, Clone, Copy)]
pub struct FakeRunner;

impl AxonRunner for FakeRunner {
    fn run(
        &mut self,
        _worker: &nous_pouw::WorkerId,
        job: &nous_pouw::JobEnvelope,
    ) -> Result<WorkflowOutput, RunnerError> {
        let mut buf = Vec::with_capacity(job.workflow_payload.len() + 8);
        buf.extend_from_slice(&job.workflow_payload);
        buf.extend_from_slice(&job.model.seed.to_le_bytes());
        let h = blake3::hash(&buf);

        Ok(WorkflowOutput {
            output: h.as_bytes().to_vec(),
            trace_events: vec![
                crate::trace::TraceEvent::WorkflowStart,
                crate::trace::TraceEvent::StepComplete {
                    step: 0,
                    output_hash: *h.as_bytes(),
                },
                crate::trace::TraceEvent::WorkflowEnd { ok: true },
            ],
            latency_ms: 1,
            rubric: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use nous_pouw::envelope::{JobEnvelope, ModelPin};
    use rand::rngs::OsRng;

    fn job(payload: &[u8], seed: u64) -> JobEnvelope {
        JobEnvelope {
            nonce: 1,
            workflow_cid: [0; 32],
            workflow_payload: payload.to_vec(),
            model: ModelPin::new("m", seed),
            n_replicas: 5,
            bounty: 100,
            deadline_ms: 1000,
        }
    }

    fn worker_id() -> nous_pouw::WorkerId {
        let sk = SigningKey::generate(&mut OsRng);
        nous_pouw::WorkerId::from_verifying_key(&sk.verifying_key())
    }

    #[test]
    fn fake_runner_is_deterministic() {
        let mut r = FakeRunner;
        let w = worker_id();
        let a = r.run(&w, &job(b"hello", 7)).unwrap();
        let b = r.run(&w, &job(b"hello", 7)).unwrap();
        assert_eq!(a.output, b.output);
    }

    #[test]
    fn fake_runner_changes_with_payload() {
        let mut r = FakeRunner;
        let w = worker_id();
        let a = r.run(&w, &job(b"hello", 7)).unwrap();
        let b = r.run(&w, &job(b"world", 7)).unwrap();
        assert_ne!(a.output, b.output);
    }

    #[test]
    fn fake_runner_changes_with_seed() {
        let mut r = FakeRunner;
        let w = worker_id();
        let a = r.run(&w, &job(b"hello", 7)).unwrap();
        let b = r.run(&w, &job(b"hello", 8)).unwrap();
        assert_ne!(a.output, b.output);
    }

    #[test]
    fn fake_runner_independent_of_worker_identity() {
        let mut r = FakeRunner;
        let a = r.run(&worker_id(), &job(b"x", 1)).unwrap();
        let b = r.run(&worker_id(), &job(b"x", 1)).unwrap();
        assert_eq!(a.output, b.output);
    }

    #[test]
    fn fake_runner_emits_trace_events() {
        let mut r = FakeRunner;
        let out = r.run(&worker_id(), &job(b"x", 1)).unwrap();
        assert_eq!(out.trace_events.len(), 3);
    }
}
