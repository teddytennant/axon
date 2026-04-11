use std::time::Instant;
use tracing::{info, warn};
use uuid::Uuid;

use crate::protocol::{Capability, TaskStatus};

use super::workflow::{WorkflowError, WorkflowResult};

pub type WorkflowId = Uuid;

/// Tracks state and timing for a single workflow execution.
/// Correlation IDs flow from workflow functions into tracing spans
/// so any tracing subscriber automatically captures workflow context.
#[derive(Debug, Clone)]
pub struct WorkflowSpan {
    pub workflow_id: WorkflowId,
    pub step_index: usize,
    pub parent_task_id: Option<Uuid>,
    started_at: Instant,
}

impl WorkflowSpan {
    pub fn new(workflow_id: WorkflowId) -> Self {
        Self {
            workflow_id,
            step_index: 0,
            parent_task_id: None,
            started_at: Instant::now(),
        }
    }

    pub fn with_parent(mut self, parent_task_id: Uuid) -> Self {
        self.parent_task_id = Some(parent_task_id);
        self
    }

    /// Advance to the next step, returning a new span with incremented index.
    pub fn next_step(&self) -> Self {
        Self {
            workflow_id: self.workflow_id,
            step_index: self.step_index + 1,
            parent_task_id: self.parent_task_id,
            started_at: self.started_at,
        }
    }

    /// Return a span for step N (without chaining N calls to next_step).
    pub fn next_step_n(&self, n: usize) -> Self {
        Self {
            workflow_id: self.workflow_id,
            step_index: n,
            parent_task_id: self.parent_task_id,
            started_at: self.started_at,
        }
    }

    /// Milliseconds elapsed since this workflow started.
    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    /// Create a tracing span with workflow correlation fields.
    pub fn span(&self) -> tracing::Span {
        tracing::info_span!(
            "workflow",
            workflow_id = %self.workflow_id,
            step = self.step_index,
        )
    }
}

pub fn emit_step_start(span: &WorkflowSpan, capability: &Capability) {
    info!(
        workflow_id = %span.workflow_id,
        step = span.step_index,
        capability = %capability.tag(),
        "workflow step start"
    );
}

pub fn emit_step_complete(span: &WorkflowSpan, status: &TaskStatus, duration_ms: u64) {
    let ok = matches!(status, TaskStatus::Success);
    if ok {
        info!(
            workflow_id = %span.workflow_id,
            step = span.step_index,
            duration_ms,
            "workflow step complete"
        );
    } else {
        warn!(
            workflow_id = %span.workflow_id,
            step = span.step_index,
            duration_ms,
            status = ?status,
            "workflow step failed"
        );
    }
}

pub fn emit_workflow_complete(span: &WorkflowSpan, result: &WorkflowResult) {
    info!(
        workflow_id = %span.workflow_id,
        steps = result.steps_completed,
        duration_ms = result.duration_ms,
        "workflow complete"
    );
}

pub fn emit_workflow_error(span: &WorkflowSpan, error: &WorkflowError) {
    warn!(
        workflow_id = %span.workflow_id,
        error = %error,
        elapsed_ms = span.elapsed_ms(),
        "workflow failed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn workflow_span_initial_state() {
        let id = Uuid::new_v4();
        let span = WorkflowSpan::new(id);
        assert_eq!(span.workflow_id, id);
        assert_eq!(span.step_index, 0);
        assert!(span.parent_task_id.is_none());
    }

    #[test]
    fn next_step_increments_index() {
        let span = WorkflowSpan::new(Uuid::new_v4());
        let s1 = span.next_step();
        let s2 = s1.next_step();
        let s3 = s2.next_step();
        assert_eq!(s1.step_index, 1);
        assert_eq!(s2.step_index, 2);
        assert_eq!(s3.step_index, 3);
    }

    #[test]
    fn next_step_preserves_workflow_id() {
        let id = Uuid::new_v4();
        let span = WorkflowSpan::new(id);
        let next = span.next_step();
        assert_eq!(next.workflow_id, id);
    }

    #[test]
    fn next_step_preserves_parent_task_id() {
        let parent = Uuid::new_v4();
        let span = WorkflowSpan::new(Uuid::new_v4()).with_parent(parent);
        let next = span.next_step();
        assert_eq!(next.parent_task_id, Some(parent));
    }

    #[test]
    fn elapsed_ms_is_monotonic() {
        let span = WorkflowSpan::new(Uuid::new_v4());
        let t1 = span.elapsed_ms();
        thread::sleep(Duration::from_millis(5));
        let t2 = span.elapsed_ms();
        assert!(
            t2 >= t1,
            "elapsed_ms should be monotonically non-decreasing"
        );
    }

    #[test]
    fn elapsed_ms_starts_near_zero() {
        let span = WorkflowSpan::new(Uuid::new_v4());
        // Should be well under 1 second immediately after creation
        assert!(span.elapsed_ms() < 1000);
    }

    #[test]
    fn span_contains_workflow_id() {
        let id = Uuid::new_v4();
        let ws = WorkflowSpan::new(id);
        // Creating the span should not panic
        let _span = ws.span();
    }

    #[test]
    fn with_parent_sets_parent_id() {
        let parent = Uuid::new_v4();
        let span = WorkflowSpan::new(Uuid::new_v4()).with_parent(parent);
        assert_eq!(span.parent_task_id, Some(parent));
    }

    #[test]
    fn multiple_spans_from_same_workflow_share_id() {
        let id = Uuid::new_v4();
        let s0 = WorkflowSpan::new(id);
        let s1 = s0.next_step();
        let s2 = s1.next_step();
        assert_eq!(s0.workflow_id, s1.workflow_id);
        assert_eq!(s1.workflow_id, s2.workflow_id);
    }
}
