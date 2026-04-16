//! Shared data types for workflow orchestration.
//!
//! Extracted into its own module so that higher-level modules like
//! [`super::workflow`] (execution) and [`super::trace`] (observability)
//! can both depend on these types without depending on each other.

use thiserror::Error;
use uuid::Uuid;

use crate::protocol::{Capability, TaskResponse};

/// Stable identifier for a workflow execution.
pub type WorkflowId = Uuid;

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
    /// Build a result for a fully successful run.
    pub(super) fn success(
        workflow_id: WorkflowId,
        responses: Vec<TaskResponse>,
        duration_ms: u64,
    ) -> Self {
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

    /// Build a result for a partially successful run (e.g. fan-out with some failures).
    pub(super) fn partial(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::TaskStatus;

    fn make_response(id: WorkflowId) -> TaskResponse {
        TaskResponse {
            request_id: id,
            status: TaskStatus::Success,
            payload: Vec::new(),
            duration_ms: 1,
        }
    }

    #[test]
    fn workflow_step_defaults_to_pass_through() {
        let cap = Capability::new("n", "x", 1);
        let step = WorkflowStep::new(cap);
        assert!(matches!(step.transform, PayloadTransform::PassThrough));
        assert_eq!(step.timeout_ms, 30_000);
    }

    #[test]
    fn workflow_step_with_transform_and_timeout() {
        let cap = Capability::new("n", "x", 1);
        let step = WorkflowStep::new(cap)
            .with_transform(PayloadTransform::ExtractField("a.b".into()))
            .with_timeout(5_000);
        assert_eq!(step.timeout_ms, 5_000);
        assert!(matches!(step.transform, PayloadTransform::ExtractField(_)));
    }

    #[test]
    fn workflow_result_success_computes_counts() {
        let id = Uuid::new_v4();
        let resp = make_response(id);
        let result = WorkflowResult::success(id, vec![resp.clone(), resp.clone()], 42);
        assert_eq!(result.steps_completed, 2);
        assert_eq!(result.steps_total, 2);
        assert!(result.final_response.is_some());
        assert_eq!(result.all_responses.len(), 2);
        assert_eq!(result.duration_ms, 42);
    }

    #[test]
    fn workflow_result_partial_tracks_total_vs_completed() {
        let id = Uuid::new_v4();
        let resp = make_response(id);
        let result = WorkflowResult::partial(id, 1, 3, vec![resp], 7);
        assert_eq!(result.steps_completed, 1);
        assert_eq!(result.steps_total, 3);
        assert_eq!(result.duration_ms, 7);
    }

    #[test]
    fn workflow_error_display() {
        let e = WorkflowError::StepFailed {
            step: 2,
            reason: "boom".into(),
        };
        assert!(e.to_string().contains("step 2 failed"));
        assert_eq!(
            WorkflowError::AllFanOutFailed.to_string(),
            "all fan-out tasks failed"
        );
        assert_eq!(WorkflowError::Timeout.to_string(), "workflow timed out");
    }
}
