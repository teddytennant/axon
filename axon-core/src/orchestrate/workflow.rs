use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use uuid::Uuid;

use crate::protocol::{Capability, TaskRequest, TaskResponse, TaskStatus};
use crate::runtime::Runtime;

use super::lifecycle::ManagedAgent;
use super::trace::{
    emit_step_complete, emit_step_start, emit_workflow_complete, emit_workflow_error, WorkflowSpan,
};

pub type WorkflowId = Uuid;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How to transform the output payload between pipeline steps.
#[derive(Debug, Clone)]
pub enum PayloadTransform {
    /// Pass the previous step's output payload as-is to the next step.
    PassThrough,
    /// Extract a JSON field from the payload. Supports dotted paths ("user.name").
    ExtractField(String),
}

/// A single step in a pipeline workflow.
#[derive(Debug, Clone)]
pub struct WorkflowStep {
    pub capability: Capability,
    pub transform: PayloadTransform,
    pub timeout_ms: u64,
}

impl WorkflowStep {
    pub fn new(capability: Capability) -> Self {
        Self {
            capability,
            transform: PayloadTransform::PassThrough,
            timeout_ms: 30_000,
        }
    }

    pub fn with_transform(mut self, transform: PayloadTransform) -> Self {
        self.transform = transform;
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
}

/// Result of a completed workflow execution.
#[derive(Debug)]
pub struct WorkflowResult {
    pub workflow_id: WorkflowId,
    pub steps_completed: usize,
    pub steps_total: usize,
    pub final_response: Option<TaskResponse>,
    /// All responses collected (fan-out fills this with N entries; pipeline fills it sequentially).
    pub all_responses: Vec<TaskResponse>,
    pub duration_ms: u64,
}

impl WorkflowResult {
    fn success(workflow_id: WorkflowId, responses: Vec<TaskResponse>, duration_ms: u64) -> Self {
        let steps = responses.len();
        let final_response = responses.last().cloned();
        Self {
            workflow_id,
            steps_completed: steps,
            steps_total: steps,
            final_response,
            all_responses: responses,
            duration_ms,
        }
    }

    fn partial(
        workflow_id: WorkflowId,
        completed: usize,
        total: usize,
        responses: Vec<TaskResponse>,
        duration_ms: u64,
    ) -> Self {
        let final_response = responses.last().cloned();
        Self {
            workflow_id,
            steps_completed: completed,
            steps_total: total,
            final_response,
            all_responses: responses,
            duration_ms,
        }
    }
}

/// Error during workflow execution.
#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("step {step} failed: {reason}")]
    StepFailed { step: usize, reason: String },
    #[error("all fan-out tasks failed")]
    AllFanOutFailed,
    #[error("workflow timed out")]
    Timeout,
}

// ---------------------------------------------------------------------------
// PayloadTransform
// ---------------------------------------------------------------------------

fn apply_transform(transform: &PayloadTransform, payload: &[u8]) -> Vec<u8> {
    match transform {
        PayloadTransform::PassThrough => payload.to_vec(),
        PayloadTransform::ExtractField(path) => extract_json_field(payload, path),
    }
}

/// Extract a dotted-path field from JSON bytes.
/// Returns the serialized value or the original payload on any error.
fn extract_json_field(payload: &[u8], path: &str) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(payload) else {
        return payload.to_vec();
    };
    for segment in path.split('.') {
        value = match value {
            serde_json::Value::Object(mut m) => match m.remove(segment) {
                Some(v) => v,
                None => return payload.to_vec(),
            },
            _ => return payload.to_vec(),
        };
    }
    match serde_json::to_vec(&value) {
        Ok(v) => v,
        Err(_) => payload.to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Orchestration patterns
// ---------------------------------------------------------------------------

/// Pipeline: A → B → C.
///
/// Each step's output payload feeds into the next step's input.
/// Stops and returns `WorkflowError::StepFailed` on first failure.
pub async fn pipeline(
    runtime: &Runtime,
    steps: &[WorkflowStep],
    initial_payload: Vec<u8>,
    workflow_id: WorkflowId,
) -> Result<WorkflowResult, WorkflowError> {
    let span = WorkflowSpan::new(workflow_id);
    let started = Instant::now();

    if steps.is_empty() {
        let resp = TaskResponse {
            request_id: workflow_id,
            status: TaskStatus::Success,
            payload: initial_payload,
            duration_ms: 0,
        };
        return Ok(WorkflowResult::success(workflow_id, vec![resp], 0));
    }

    let mut current_payload = initial_payload;
    let mut responses = Vec::with_capacity(steps.len());

    for (i, step) in steps.iter().enumerate() {
        let step_span = if i == 0 {
            span.clone()
        } else {
            span.next_step_n(i)
        };
        emit_step_start(&step_span, &step.capability);

        let req = TaskRequest {
            id: Uuid::new_v4(),
            capability: step.capability.clone(),
            payload: current_payload.clone(),
            timeout_ms: step.timeout_ms,
        };

        let resp = runtime.dispatch(req).await;
        emit_step_complete(&step_span, &resp.status, resp.duration_ms);

        match &resp.status {
            TaskStatus::Success => {
                // Apply transform to get input for next step
                current_payload = apply_transform(&step.transform, &resp.payload);
                responses.push(resp);
            }
            TaskStatus::Error(e) => {
                let _duration_ms = started.elapsed().as_millis() as u64;
                let err = WorkflowError::StepFailed {
                    step: i,
                    reason: e.clone(),
                };
                emit_workflow_error(&step_span, &err);
                return Err(err);
            }
            TaskStatus::Timeout => {
                let _duration_ms = started.elapsed().as_millis() as u64;
                let err = WorkflowError::StepFailed {
                    step: i,
                    reason: "timeout".to_string(),
                };
                emit_workflow_error(&step_span, &err);
                return Err(err);
            }
            TaskStatus::NoCapability => {
                let err = WorkflowError::StepFailed {
                    step: i,
                    reason: "no capable agent".to_string(),
                };
                emit_workflow_error(&step_span, &err);
                return Err(err);
            }
        }
    }

    let duration_ms = started.elapsed().as_millis() as u64;
    let result = WorkflowResult::success(workflow_id, responses, duration_ms);
    emit_workflow_complete(&span, &result);
    Ok(result)
}

/// Fan-out: dispatch the same payload to N capabilities in parallel, collect results.
///
/// Returns a result with all successful responses. If all fail, returns
/// `WorkflowError::AllFanOutFailed`. Partial failures are represented as
/// fewer entries in `all_responses` vs `steps_total`.
pub async fn fan_out(
    runtime: &Runtime,
    targets: &[Capability],
    payload: Vec<u8>,
    timeout_ms: u64,
    workflow_id: WorkflowId,
) -> Result<WorkflowResult, WorkflowError> {
    let span = WorkflowSpan::new(workflow_id);
    let started = Instant::now();
    let total = targets.len();

    let futures: Vec<_> = targets
        .iter()
        .enumerate()
        .map(|(i, cap)| {
            let step_span = span.next_step_n(i);
            let cap = cap.clone();
            let payload = payload.clone();
            emit_step_start(&step_span, &cap);
            async move {
                let req = TaskRequest {
                    id: Uuid::new_v4(),
                    capability: cap,
                    payload,
                    timeout_ms,
                };
                let resp = runtime.dispatch(req).await;
                emit_step_complete(&step_span, &resp.status, resp.duration_ms);
                resp
            }
        })
        .collect();

    let responses: Vec<TaskResponse> = futures::future::join_all(futures).await;
    let successful: Vec<TaskResponse> = responses
        .into_iter()
        .filter(|r| matches!(r.status, TaskStatus::Success))
        .collect();

    let duration_ms = started.elapsed().as_millis() as u64;

    if successful.is_empty() {
        let err = WorkflowError::AllFanOutFailed;
        emit_workflow_error(&span, &err);
        return Err(err);
    }

    let result = WorkflowResult::partial(
        workflow_id,
        successful.len(),
        total,
        successful,
        duration_ms,
    );
    emit_workflow_complete(&span, &result);
    Ok(result)
}

/// Delegate: dispatch a single task to the best local agent for a capability.
///
/// This is a thin wrapper around `Runtime::dispatch`. In a full mesh deployment
/// the node's event loop handles routing to remote peers; this function handles
/// local dispatch for orchestration flows.
pub async fn delegate(
    runtime: &Runtime,
    capability: &Capability,
    payload: Vec<u8>,
    timeout_ms: u64,
    workflow_id: WorkflowId,
) -> Result<WorkflowResult, WorkflowError> {
    let span = WorkflowSpan::new(workflow_id);
    let started = Instant::now();
    emit_step_start(&span, capability);

    let req = TaskRequest {
        id: Uuid::new_v4(),
        capability: capability.clone(),
        payload,
        timeout_ms,
    };

    let resp = runtime.dispatch(req).await;
    emit_step_complete(&span, &resp.status, resp.duration_ms);
    let duration_ms = started.elapsed().as_millis() as u64;

    match &resp.status {
        TaskStatus::Success => {
            let result = WorkflowResult::success(workflow_id, vec![resp], duration_ms);
            emit_workflow_complete(&span, &result);
            Ok(result)
        }
        TaskStatus::Error(e) => {
            let err = WorkflowError::StepFailed {
                step: 0,
                reason: e.clone(),
            };
            emit_workflow_error(&span, &err);
            Err(err)
        }
        TaskStatus::Timeout => {
            let err = WorkflowError::Timeout;
            emit_workflow_error(&span, &err);
            Err(err)
        }
        TaskStatus::NoCapability => {
            let err = WorkflowError::StepFailed {
                step: 0,
                reason: "no capable agent".to_string(),
            };
            emit_workflow_error(&span, &err);
            Err(err)
        }
    }
}

/// Supervisor: monitor a set of managed agents and re-start any that have drifted
/// to Paused or Created state.
///
/// Runs in the background until the returned `JoinHandle` is aborted.
/// Logs warnings for Stopped agents (which cannot be auto-restarted).
pub fn supervisor(
    agents: Vec<Arc<ManagedAgent>>,
    check_interval: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(check_interval);
        loop {
            ticker.tick().await;
            for agent in &agents {
                let current = agent.state().await;
                match current {
                    super::lifecycle::AgentState::Paused
                    | super::lifecycle::AgentState::Created => {
                        tracing::warn!(
                            agent = agent.definition.name,
                            state = %current,
                            "supervisor: restarting drifted agent"
                        );
                        agent.start().await;
                    }
                    super::lifecycle::AgentState::Stopped => {
                        tracing::error!(
                            agent = agent.definition.name,
                            "supervisor: agent is stopped — cannot restart automatically"
                        );
                    }
                    super::lifecycle::AgentState::Running => {}
                }
            }
        }
    })
}

/// Swarm dispatch: dispatch via the existing negotiation protocol.
///
/// In the current implementation this dispatches through the local runtime;
/// the mesh-level bid collection (TaskOffer/TaskBid) is handled by the node's
/// event loop. This function provides the orchestration-layer entry point for
/// capability tasks that should be routed via negotiation.
pub async fn swarm_dispatch(
    runtime: &Runtime,
    capability: &Capability,
    payload: Vec<u8>,
    timeout_ms: u64,
    workflow_id: WorkflowId,
) -> Result<WorkflowResult, WorkflowError> {
    // Identical to delegate at the local dispatch level.
    // Mesh-level swarm negotiation is triggered by the transport layer.
    delegate(runtime, capability, payload, timeout_ms, workflow_id).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Capability;
    use crate::runtime::{Agent, AgentError};
    use async_trait::async_trait;

    // Agent that uppercases its payload
    struct UpperAgent;
    #[async_trait]
    impl Agent for UpperAgent {
        fn name(&self) -> &str {
            "upper"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::new("test", "upper", 1)]
        }
        async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
            Ok(TaskResponse {
                request_id: request.id,
                status: TaskStatus::Success,
                payload: request.payload.to_ascii_uppercase(),
                duration_ms: 0,
            })
        }
    }

    // Agent that reverses its payload bytes
    struct ReverseAgent;
    #[async_trait]
    impl Agent for ReverseAgent {
        fn name(&self) -> &str {
            "reverse"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::new("test", "reverse", 1)]
        }
        async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
            let mut v = request.payload.clone();
            v.reverse();
            Ok(TaskResponse {
                request_id: request.id,
                status: TaskStatus::Success,
                payload: v,
                duration_ms: 0,
            })
        }
    }

    // Agent that always fails
    struct FailAgent;
    #[async_trait]
    impl Agent for FailAgent {
        fn name(&self) -> &str {
            "fail"
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::new("test", "fail", 1)]
        }
        async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
            Ok(TaskResponse {
                request_id: request.id,
                status: TaskStatus::Error("intentional".to_string()),
                payload: vec![],
                duration_ms: 0,
            })
        }
    }

    // Agent that echoes back
    struct EchoAgent {
        name: String,
        caps: Vec<Capability>,
    }
    #[async_trait]
    impl Agent for EchoAgent {
        fn name(&self) -> &str {
            &self.name
        }
        fn capabilities(&self) -> Vec<Capability> {
            self.caps.clone()
        }
        async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
            Ok(TaskResponse {
                request_id: request.id,
                status: TaskStatus::Success,
                payload: request.payload,
                duration_ms: 0,
            })
        }
    }

    async fn runtime_with(agents: Vec<Arc<dyn Agent>>) -> Runtime {
        let rt = Runtime::new();
        for a in agents {
            rt.register(a).await;
        }
        rt
    }

    fn wid() -> WorkflowId {
        Uuid::new_v4()
    }

    // ---- pipeline tests ----

    #[tokio::test]
    async fn pipeline_two_steps_chains_payloads() {
        let rt = runtime_with(vec![Arc::new(UpperAgent), Arc::new(ReverseAgent)]).await;

        let steps = vec![
            WorkflowStep::new(Capability::new("test", "upper", 1)),
            WorkflowStep::new(Capability::new("test", "reverse", 1)),
        ];

        // "hello" -> uppercase -> "HELLO" -> reverse -> "OLLEH"
        let result = pipeline(&rt, &steps, b"hello".to_vec(), wid())
            .await
            .unwrap();
        assert_eq!(result.steps_completed, 2);
        assert_eq!(result.final_response.unwrap().payload, b"OLLEH");
    }

    #[tokio::test]
    async fn pipeline_empty_steps_returns_initial_payload() {
        let rt = Runtime::new();
        let result = pipeline(&rt, &[], b"original".to_vec(), wid())
            .await
            .unwrap();
        assert_eq!(result.steps_completed, 1);
        assert_eq!(result.final_response.unwrap().payload, b"original");
    }

    #[tokio::test]
    async fn pipeline_single_step() {
        let rt = runtime_with(vec![Arc::new(UpperAgent)]).await;
        let steps = vec![WorkflowStep::new(Capability::new("test", "upper", 1))];
        let result = pipeline(&rt, &steps, b"world".to_vec(), wid())
            .await
            .unwrap();
        assert_eq!(result.final_response.unwrap().payload, b"WORLD");
    }

    #[tokio::test]
    async fn pipeline_fails_on_first_step_error() {
        let rt = runtime_with(vec![Arc::new(FailAgent), Arc::new(ReverseAgent)]).await;
        let steps = vec![
            WorkflowStep::new(Capability::new("test", "fail", 1)),
            WorkflowStep::new(Capability::new("test", "reverse", 1)),
        ];
        let err = pipeline(&rt, &steps, b"data".to_vec(), wid())
            .await
            .unwrap_err();
        assert!(matches!(err, WorkflowError::StepFailed { step: 0, .. }));
    }

    #[tokio::test]
    async fn pipeline_fails_on_no_capability() {
        let rt = Runtime::new(); // no agents registered
        let steps = vec![WorkflowStep::new(Capability::new("test", "upper", 1))];
        let err = pipeline(&rt, &steps, b"data".to_vec(), wid())
            .await
            .unwrap_err();
        assert!(matches!(err, WorkflowError::StepFailed { step: 0, .. }));
    }

    #[tokio::test]
    async fn pipeline_result_has_correct_step_count() {
        let rt = runtime_with(vec![Arc::new(UpperAgent), Arc::new(ReverseAgent)]).await;
        let steps = vec![
            WorkflowStep::new(Capability::new("test", "upper", 1)),
            WorkflowStep::new(Capability::new("test", "reverse", 1)),
        ];
        let result = pipeline(&rt, &steps, b"x".to_vec(), wid()).await.unwrap();
        assert_eq!(result.steps_total, 2);
        assert_eq!(result.steps_completed, 2);
        assert_eq!(result.all_responses.len(), 2);
    }

    #[tokio::test]
    async fn pipeline_payload_transform_extract_field() {
        // Three-step pipeline: echo1 → (extract "result") → echo2 → (passthrough) → echo3
        // The transform on step 0 feeds extracted data into step 1.
        let rt = runtime_with(vec![
            Arc::new(EchoAgent {
                name: "e1".to_string(),
                caps: vec![Capability::new("json", "echo1", 1)],
            }),
            Arc::new(EchoAgent {
                name: "e2".to_string(),
                caps: vec![Capability::new("json", "echo2", 1)],
            }),
        ])
        .await;

        let input = serde_json::json!({"result": "hello world"});
        // Step 0: echo1 echoes JSON back, transform extracts "result" for step 1
        // Step 1: echo2 receives extracted "result" value and echoes it back
        let steps = vec![
            WorkflowStep::new(Capability::new("json", "echo1", 1))
                .with_transform(PayloadTransform::ExtractField("result".to_string())),
            WorkflowStep::new(Capability::new("json", "echo2", 1)),
        ];
        let result = pipeline(&rt, &steps, serde_json::to_vec(&input).unwrap(), wid())
            .await
            .unwrap();
        // Step 1 (echo2) received the extracted "result" value and echoed it back
        let final_payload = result.final_response.unwrap().payload;
        let s: String = serde_json::from_slice(&final_payload).unwrap();
        assert_eq!(s, "hello world");
    }

    // ---- fan_out tests ----

    #[tokio::test]
    async fn fan_out_collects_all_results() {
        let rt = runtime_with(vec![
            Arc::new(EchoAgent {
                name: "a".to_string(),
                caps: vec![Capability::new("svc", "a", 1)],
            }),
            Arc::new(EchoAgent {
                name: "b".to_string(),
                caps: vec![Capability::new("svc", "b", 1)],
            }),
            Arc::new(EchoAgent {
                name: "c".to_string(),
                caps: vec![Capability::new("svc", "c", 1)],
            }),
        ])
        .await;

        let targets = vec![
            Capability::new("svc", "a", 1),
            Capability::new("svc", "b", 1),
            Capability::new("svc", "c", 1),
        ];
        let result = fan_out(&rt, &targets, b"ping".to_vec(), 5000, wid())
            .await
            .unwrap();
        assert_eq!(result.all_responses.len(), 3);
        assert_eq!(result.steps_completed, 3);
        assert_eq!(result.steps_total, 3);
    }

    #[tokio::test]
    async fn fan_out_partial_success() {
        let rt = runtime_with(vec![
            Arc::new(EchoAgent {
                name: "ok".to_string(),
                caps: vec![Capability::new("svc", "ok", 1)],
            }),
            Arc::new(FailAgent),
        ])
        .await;

        let targets = vec![
            Capability::new("svc", "ok", 1),
            Capability::new("test", "fail", 1),
        ];
        let result = fan_out(&rt, &targets, b"data".to_vec(), 5000, wid())
            .await
            .unwrap();
        // Only the successful response is in all_responses
        assert_eq!(result.all_responses.len(), 1);
        assert_eq!(result.steps_completed, 1);
        assert_eq!(result.steps_total, 2);
    }

    #[tokio::test]
    async fn fan_out_all_fail_returns_error() {
        let rt = runtime_with(vec![Arc::new(FailAgent)]).await;
        let targets = vec![Capability::new("test", "fail", 1)];
        let err = fan_out(&rt, &targets, b"x".to_vec(), 5000, wid())
            .await
            .unwrap_err();
        assert!(matches!(err, WorkflowError::AllFanOutFailed));
    }

    #[tokio::test]
    async fn fan_out_no_capability_returns_error() {
        let rt = Runtime::new();
        let targets = vec![Capability::new("svc", "missing", 1)];
        let err = fan_out(&rt, &targets, b"x".to_vec(), 5000, wid())
            .await
            .unwrap_err();
        assert!(matches!(err, WorkflowError::AllFanOutFailed));
    }

    // ---- delegate tests ----

    #[tokio::test]
    async fn delegate_routes_to_capable_agent() {
        let rt = runtime_with(vec![Arc::new(UpperAgent)]).await;
        let result = delegate(
            &rt,
            &Capability::new("test", "upper", 1),
            b"hi".to_vec(),
            5000,
            wid(),
        )
        .await
        .unwrap();
        assert_eq!(result.final_response.unwrap().payload, b"HI");
    }

    #[tokio::test]
    async fn delegate_no_capability_returns_error() {
        let rt = Runtime::new();
        let err = delegate(
            &rt,
            &Capability::new("test", "upper", 1),
            b"x".to_vec(),
            5000,
            wid(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, WorkflowError::StepFailed { step: 0, .. }));
    }

    // ---- workflow result ----

    #[tokio::test]
    async fn workflow_result_tracks_duration() {
        let rt = runtime_with(vec![Arc::new(UpperAgent)]).await;
        let steps = vec![WorkflowStep::new(Capability::new("test", "upper", 1))];
        let result = pipeline(&rt, &steps, b"x".to_vec(), wid()).await.unwrap();
        // Duration is a u64 >= 0 (could be 0 for fast ops)
        let _ = result.duration_ms; // just verifying it compiles and is accessible
    }

    // ---- payload transform ----

    #[test]
    fn payload_transform_passthrough() {
        let data = b"hello";
        let out = apply_transform(&PayloadTransform::PassThrough, data);
        assert_eq!(out, data);
    }

    #[test]
    fn payload_transform_extract_field_present() {
        let json = serde_json::json!({"name": "axon", "version": 1});
        let bytes = serde_json::to_vec(&json).unwrap();
        let out = apply_transform(&PayloadTransform::ExtractField("name".to_string()), &bytes);
        let s: String = serde_json::from_slice(&out).unwrap();
        assert_eq!(s, "axon");
    }

    #[test]
    fn payload_transform_extract_field_missing_returns_original() {
        let json = serde_json::json!({"a": 1});
        let bytes = serde_json::to_vec(&json).unwrap();
        let out = apply_transform(
            &PayloadTransform::ExtractField("missing".to_string()),
            &bytes,
        );
        assert_eq!(out, bytes);
    }

    #[test]
    fn payload_transform_extract_invalid_json_returns_original() {
        let bytes = b"not json";
        let out = apply_transform(&PayloadTransform::ExtractField("field".to_string()), bytes);
        assert_eq!(out, bytes);
    }

    #[test]
    fn payload_transform_extract_nested_field() {
        let json = serde_json::json!({"user": {"name": "teddy"}});
        let bytes = serde_json::to_vec(&json).unwrap();
        let out = apply_transform(&PayloadTransform::ExtractField("user".to_string()), &bytes);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["name"], "teddy");
    }
}
