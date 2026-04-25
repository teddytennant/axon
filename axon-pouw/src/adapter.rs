//! `Adapter`: bridge an [`AxonRunner`] into the `nous_pouw::WorkExecutor` shape.

use std::collections::HashMap;

use ed25519_dalek::SigningKey;
use nous_pouw::engine::WorkExecutor;
use nous_pouw::envelope::JobEnvelope;
use nous_pouw::receipt::{sign_receipt, RubricScore, WorkReceipt};
use nous_pouw::state::WorkerId;

use crate::runner::{AxonRunner, RunnerError};
use crate::trace::build_trace_root;

/// Wraps an `AxonRunner` + a per-worker keychain so receipts can be signed.
pub struct Adapter<R: AxonRunner> {
    runner: R,
    sks: HashMap<WorkerId, SigningKey>,
}

impl<R: AxonRunner> Adapter<R> {
    pub fn new(runner: R, sks: &[SigningKey]) -> Self {
        let mut map = HashMap::new();
        for sk in sks {
            let id = WorkerId::from_verifying_key(&sk.verifying_key());
            map.insert(id, SigningKey::from_bytes(&sk.to_bytes()));
        }
        Self { runner, sks: map }
    }

    pub fn runner(&self) -> &R {
        &self.runner
    }

    pub fn runner_mut(&mut self) -> &mut R {
        &mut self.runner
    }

    /// Produce a malformed receipt that the engine drops on signature verify.
    /// Used when the runner errors out — we can't sign on behalf of a worker
    /// that didn't actually finish.
    fn invalid_receipt(worker: WorkerId, job: &JobEnvelope) -> WorkReceipt {
        WorkReceipt {
            job_id: job.id(),
            worker,
            output_hash: [0; 32],
            trace_root: [0; 32],
            rubric: None,
            latency_ms: 0,
            signature: vec![0u8; 64],
        }
    }
}

impl<R: AxonRunner> WorkExecutor for Adapter<R> {
    fn execute(&mut self, worker: WorkerId, job: &JobEnvelope) -> WorkReceipt {
        let result = self.runner.run(&worker, job);
        let sk = match self.sks.get(&worker) {
            Some(sk) => sk,
            None => return Self::invalid_receipt(worker, job),
        };
        let output = match result {
            Ok(out) => out,
            Err(RunnerError::Failed(_)) | Err(RunnerError::Timeout) => {
                return Self::invalid_receipt(worker, job);
            }
        };
        let output_hash = *blake3::hash(&output.output).as_bytes();
        let trace_root = build_trace_root(&output.trace_events);
        let mut r = WorkReceipt {
            job_id: job.id(),
            worker,
            output_hash,
            trace_root,
            rubric: output.rubric.map(RubricScore),
            latency_ms: output.latency_ms,
            signature: vec![],
        };
        sign_receipt(&mut r, sk);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::FakeRunner;
    use ed25519_dalek::SigningKey;
    use nous_pouw::envelope::ModelPin;
    use rand::rngs::OsRng;

    fn job() -> JobEnvelope {
        JobEnvelope {
            nonce: 1,
            workflow_cid: [0; 32],
            workflow_payload: b"hello".to_vec(),
            model: ModelPin::new("m", 0),
            n_replicas: 3,
            bounty: 100,
            deadline_ms: 1_000,
        }
    }

    #[test]
    fn adapter_signs_valid_receipt() {
        let sks: Vec<_> = (0..3).map(|_| SigningKey::generate(&mut OsRng)).collect();
        let mut adapter = Adapter::new(FakeRunner, &sks);
        let w = WorkerId::from_verifying_key(&sks[0].verifying_key());
        let r = adapter.execute(w, &job());
        r.verify().expect("signed receipt verifies");
    }

    #[test]
    fn unknown_worker_yields_invalid_receipt() {
        let sks: Vec<_> = (0..1).map(|_| SigningKey::generate(&mut OsRng)).collect();
        let mut adapter = Adapter::new(FakeRunner, &sks);
        let other = SigningKey::generate(&mut OsRng);
        let w = WorkerId::from_verifying_key(&other.verifying_key());
        let r = adapter.execute(w, &job());
        assert!(r.verify().is_err());
    }

    #[test]
    fn determinism_across_workers_with_fake_runner() {
        let sks: Vec<_> = (0..5).map(|_| SigningKey::generate(&mut OsRng)).collect();
        let mut adapter = Adapter::new(FakeRunner, &sks);
        let outputs: Vec<_> = sks
            .iter()
            .map(|sk| {
                let w = WorkerId::from_verifying_key(&sk.verifying_key());
                adapter.execute(w, &job()).output_hash
            })
            .collect();
        // All workers produce the same output_hash.
        let first = outputs[0];
        assert!(outputs.iter().all(|o| o == &first));
    }

    #[test]
    fn adapter_propagates_trace_root() {
        let sks: Vec<_> = (0..1).map(|_| SigningKey::generate(&mut OsRng)).collect();
        let mut adapter = Adapter::new(FakeRunner, &sks);
        let w = WorkerId::from_verifying_key(&sks[0].verifying_key());
        let r = adapter.execute(w, &job());
        assert_ne!(r.trace_root, [0; 32]);
    }
}
