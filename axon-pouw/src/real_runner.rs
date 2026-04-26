//! Real `AxonRunner`: execute a PoUW workflow against a live `axon_core::Runtime`.
//!
//! Unlike [`crate::FakeRunner`] (which hashes the payload to fake a result),
//! this runner actually:
//!
//! 1. Decodes a [`WorkflowBlob`] out of the [`nous_pouw::JobEnvelope::workflow_payload`].
//! 2. Builds a `Vec<axon_core::orchestrate::types::WorkflowStep>` from the blob.
//! 3. Drives `axon_core::orchestrate::workflow::pipeline` against the embedded
//!    [`axon_core::runtime::Runtime`] to dispatch each stage to a real agent.
//! 4. Hashes the final response payload + emits a per-stage [`TraceEvent`] log.
//!
//! ## Async ↔ sync bridge
//!
//! The [`crate::AxonRunner`] trait is **synchronous** (the PoUW engine is a
//! synchronous state machine and we don't want to leak `async` into it), but
//! axon's pipeline is `async fn`. We bridge with a private `tokio::runtime::Runtime`
//! owned by [`RealAxonRunner`]: every call to `run()` does `tokio_rt.block_on(...)`.
//!
//! Care is taken to construct the runtime exactly once per `RealAxonRunner`
//! instance (via [`RealAxonRunner::new`]) so we don't pay for spinning up a
//! thread pool on every job. The runtime is dropped with the runner.
//!
//! ## Determinism
//!
//! This runner is deterministic **iff the registered axon agents are
//! deterministic**. For unit tests we register `StubAgent` instances (a closure
//! `Fn(&[u8]) -> Vec<u8>`), which obviously are. Real LLM agents are not
//! byte-deterministic across replicas in general — production deployments need
//! either:
//!
//! * Rubric-based scoring (see [`crate::rubric`]) so quorum forms over a
//!   normalized score rather than raw bytes, or
//! * A canonicalizer / normalizer agent appended as the last stage of the
//!   pipeline so semantically-equivalent outputs collapse to byte-equal bytes.
//!
//! `RealAxonRunner` is faithful to whatever the agents return — it does not
//! itself attempt to canonicalize.

use std::sync::Arc;

use async_trait::async_trait;
use axon_core::orchestrate::{
    types::{PayloadTransform, WorkflowStep},
    workflow::pipeline,
};
use axon_core::protocol::{Capability, TaskRequest, TaskResponse, TaskStatus};
use axon_core::runtime::{Agent, AgentError, Runtime};

use crate::runner::{AxonRunner, RunnerError, WorkflowOutput};
use crate::trace::TraceEvent;

/// Definition of one workflow stage in JSON-encodable form (so it can fit
/// inside [`nous_pouw::JobEnvelope::workflow_payload`]).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct StageSpec {
    pub capability_namespace: String,
    pub capability_name: String,
    pub capability_version: u32,
    pub timeout_ms: u64,
    /// Optional dotted path for `PayloadTransform::ExtractField`. None ⇒
    /// `PayloadTransform::PassThrough`.
    pub extract_field: Option<String>,
}

impl StageSpec {
    fn capability(&self) -> Capability {
        Capability::new(
            self.capability_namespace.clone(),
            self.capability_name.clone(),
            self.capability_version,
        )
    }

    fn to_step(&self) -> WorkflowStep {
        let cap = self.capability();
        let transform = match &self.extract_field {
            Some(path) => PayloadTransform::ExtractField(path.clone()),
            None => PayloadTransform::PassThrough,
        };
        WorkflowStep {
            capability: cap,
            transform,
            timeout_ms: self.timeout_ms,
        }
    }
}

/// JSON-encodable workflow blob carried in [`nous_pouw::JobEnvelope::workflow_payload`].
///
/// `RealAxonRunner` decodes one of these per `run()` call and builds an
/// in-memory pipeline from it.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct WorkflowBlob {
    pub initial_input: Vec<u8>,
    pub stages: Vec<StageSpec>,
}

impl WorkflowBlob {
    /// Encode to bytes suitable for `JobEnvelope::workflow_payload`.
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("WorkflowBlob is JSON-serializable")
    }
}

/// Stub agent that answers any task for one capability with `f(payload)`.
///
/// Registered via [`RealAxonRunner::register_stub`]. Used by the integration
/// tests so the pipeline can exercise the real `Runtime::dispatch` codepath
/// without pulling in a real model server.
struct StubAgent {
    name: String,
    cap: Capability,
    f: Arc<dyn Fn(&[u8]) -> Vec<u8> + Send + Sync + 'static>,
}

#[async_trait]
impl Agent for StubAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![self.cap.clone()]
    }

    async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
        let out = (self.f)(&request.payload);
        Ok(TaskResponse {
            request_id: request.id,
            status: TaskStatus::Success,
            payload: out,
            duration_ms: 0,
        })
    }
}

/// Real runner that executes the pipeline against an axon [`Runtime`].
///
/// Owns its own `tokio::runtime::Runtime` so the synchronous [`AxonRunner`]
/// trait can wrap async pipeline execution with `block_on`.
pub struct RealAxonRunner {
    runtime: Arc<Runtime>,
    tokio_rt: tokio::runtime::Runtime,
}

impl RealAxonRunner {
    /// Build a new real runner around a shared axon [`Runtime`].
    ///
    /// The shared runtime is what holds registered agents — multiple runners
    /// (one per simulated worker, say) can share the same `Arc<Runtime>` so
    /// every replica sees the same agent set and produces the same output.
    pub fn new(runtime: Arc<Runtime>) -> Result<Self, std::io::Error> {
        let tokio_rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        Ok(Self { runtime, tokio_rt })
    }

    /// Convenience for tests: register a built-in deterministic agent that
    /// answers any task for `capability` with `f(payload)`.
    ///
    /// In production, agents are registered externally (loaded from configs,
    /// connected to model servers, etc.) — this helper only exists to make
    /// the integration tests fit in one file without a stand-alone fixture.
    pub async fn register_stub<F>(&self, capability: Capability, f: F)
    where
        F: Fn(&[u8]) -> Vec<u8> + Send + Sync + 'static,
    {
        let name = format!(
            "stub-{}-{}-v{}",
            capability.namespace, capability.name, capability.version
        );
        let agent = Arc::new(StubAgent {
            name,
            cap: capability,
            f: Arc::new(f),
        });
        self.runtime.register(agent).await;
    }

    /// Access the underlying tokio runtime — useful when a caller wants to
    /// drive `register_stub` (which is async) from synchronous test code.
    pub fn block_on<F: std::future::Future>(&self, fut: F) -> F::Output {
        self.tokio_rt.block_on(fut)
    }

    /// Access the shared axon `Runtime`.
    pub fn axon_runtime(&self) -> &Arc<Runtime> {
        &self.runtime
    }
}

impl AxonRunner for RealAxonRunner {
    fn run(
        &mut self,
        _worker: &nous_pouw::WorkerId,
        job: &nous_pouw::JobEnvelope,
    ) -> Result<WorkflowOutput, RunnerError> {
        let started = std::time::Instant::now();

        // 1. Decode WorkflowBlob from job.workflow_payload.
        let blob: WorkflowBlob = serde_json::from_slice(&job.workflow_payload)
            .map_err(|e| RunnerError::Failed(format!("invalid WorkflowBlob: {e}")))?;

        // 2. Build the steps.
        let steps: Vec<WorkflowStep> = blob.stages.iter().map(StageSpec::to_step).collect();

        // 3. block_on the async pipeline.
        let workflow_id = uuid::Uuid::new_v4();
        let result = self.tokio_rt.block_on(pipeline(
            self.runtime.as_ref(),
            &steps,
            blob.initial_input,
            workflow_id,
        ));

        let result = result.map_err(|e| RunnerError::Failed(format!("pipeline failed: {e}")))?;

        // 4. Hash final_response.payload into the canonical output bytes.
        //    (We use the raw payload itself as the output; the adapter then
        //    blake3-hashes it into output_hash.)
        let final_payload = result
            .final_response
            .as_ref()
            .map(|r| r.payload.clone())
            .unwrap_or_default();

        // 5. Build TraceEvent log: WorkflowStart + StepComplete per stage + WorkflowEnd.
        let mut trace_events = Vec::with_capacity(result.all_responses.len() + 2);
        trace_events.push(TraceEvent::WorkflowStart);
        for (i, resp) in result.all_responses.iter().enumerate() {
            let h = blake3::hash(&resp.payload);
            trace_events.push(TraceEvent::StepComplete {
                step: i as u32,
                output_hash: *h.as_bytes(),
            });
        }
        trace_events.push(TraceEvent::WorkflowEnd { ok: true });

        let latency_ms = started.elapsed().as_millis() as u64;
        Ok(WorkflowOutput {
            output: final_payload,
            trace_events,
            latency_ms,
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

    fn worker_id() -> nous_pouw::WorkerId {
        let sk = SigningKey::generate(&mut OsRng);
        nous_pouw::WorkerId::from_verifying_key(&sk.verifying_key())
    }

    fn job_with_blob(blob: &WorkflowBlob, seed: u64) -> JobEnvelope {
        JobEnvelope {
            nonce: 1,
            workflow_cid: [0; 32],
            workflow_payload: blob.encode(),
            model: ModelPin::new("axon-real", seed),
            n_replicas: 5,
            bounty: 100,
            deadline_ms: 1_000,
        }
    }

    #[test]
    fn real_runner_constructs_and_drops() {
        let rt = Arc::new(Runtime::new());
        let r = RealAxonRunner::new(rt).expect("construct");
        // Drop should tear down the tokio runtime cleanly.
        drop(r);
    }

    #[test]
    fn real_runner_decodes_invalid_blob() {
        let rt = Arc::new(Runtime::new());
        let mut r = RealAxonRunner::new(rt).unwrap();
        let bad = JobEnvelope {
            nonce: 1,
            workflow_cid: [0; 32],
            workflow_payload: b"not-json".to_vec(),
            model: ModelPin::new("m", 0),
            n_replicas: 1,
            bounty: 1,
            deadline_ms: 1,
        };
        let err = r.run(&worker_id(), &bad).unwrap_err();
        assert!(matches!(err, RunnerError::Failed(_)));
    }

    #[test]
    fn empty_pipeline_returns_initial_input_unchanged() {
        let rt = Arc::new(Runtime::new());
        let mut r = RealAxonRunner::new(rt).unwrap();
        let blob = WorkflowBlob {
            initial_input: b"unchanged".to_vec(),
            stages: vec![],
        };
        let out = r.run(&worker_id(), &job_with_blob(&blob, 0)).unwrap();
        assert_eq!(out.output, b"unchanged");
    }
}
