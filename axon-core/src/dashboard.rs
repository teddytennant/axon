//! Shared dashboard/telemetry types used by both the TUI (`axon-cli`) and the
//! HTTP/WS API (`axon-web`). Previously these structs were duplicated in both
//! crates and the `axon-web` copies were never populated — returning empty
//! arrays for every `/api/agents`, `/api/tasks/log`, `/api/workflows`, and
//! `/api/blackboard` request.
//!
//! Centralising them here (with `Serialize`/`Deserialize`) lets the sync loop
//! in the CLI publish a single snapshot to both consumers and keeps the
//! frontend's hand-written TS mirrors (`axon-desktop/src/lib/types.ts`) in
//! lock-step with the Rust source of truth.

use serde::{Deserialize, Serialize};

/// A single row in the Tasks panel / `/api/tasks/log`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLogEntry {
    pub id: String,
    pub capability: String,
    pub status: String,
    pub duration_ms: u64,
    pub peer: String,
}

/// Rich agent telemetry, populated once per sync tick. Shared by the TUI's
/// Agents tab and `/api/agents`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentInfo {
    pub name: String,
    pub capabilities: Vec<String>,
    pub provider_type: String,
    pub model_name: String,
    /// Stringified `AgentStatus` (idle / busy / err). Represented as a string
    /// so the same struct crosses the HTTP boundary and is easy to render in
    /// both the TUI and the desktop app.
    pub status: String,
    pub tasks_handled: u64,
    pub tasks_succeeded: u64,
    pub avg_latency_ms: u64,
    pub lifecycle_state: String,
    pub last_heartbeat_secs_ago: Option<u64>,
}

/// Short symbolic status for an agent — identical to the TUI-side enum but
/// exposed here so both consumers agree on the set of states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Idle,
    Processing,
    Error,
}

impl AgentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Processing => "busy",
            AgentStatus::Error => "err",
        }
    }
}

/// Snapshot of a workflow run, used by the Workflows tab and `/api/workflows`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowSnapshot {
    pub id: String,
    pub pattern: String,
    pub steps_completed: usize,
    pub steps_total: usize,
    pub status: String,
    pub duration_ms: u64,
    pub started_at: String,
    pub steps: Vec<StepSnapshot>,
}

/// One step inside a [`WorkflowSnapshot`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepSnapshot {
    pub capability: String,
    pub status: String,
    pub latency_ms: u64,
    pub payload_bytes: usize,
}

/// A single CRDT blackboard entry, returned from `/api/blackboard` and
/// rendered in the TUI's State tab.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlackboardEntry {
    pub key: String,
    pub value: String,
    pub timestamp_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_status_strings_are_stable() {
        assert_eq!(AgentStatus::Idle.as_str(), "idle");
        assert_eq!(AgentStatus::Processing.as_str(), "busy");
        assert_eq!(AgentStatus::Error.as_str(), "err");
    }

    #[test]
    fn agent_info_json_roundtrip() {
        let info = AgentInfo {
            name: "echo".into(),
            capabilities: vec!["echo.ping".into()],
            provider_type: "builtin".into(),
            model_name: String::new(),
            status: AgentStatus::Idle.as_str().into(),
            tasks_handled: 3,
            tasks_succeeded: 3,
            avg_latency_ms: 5,
            lifecycle_state: "running".into(),
            last_heartbeat_secs_ago: Some(2),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: AgentInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn task_log_entry_json_roundtrip() {
        let entry = TaskLogEntry {
            id: "abc".into(),
            capability: "llm.chat".into(),
            status: "Success".into(),
            duration_ms: 42,
            peer: "local".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: TaskLogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }

    #[test]
    fn workflow_snapshot_json_roundtrip() {
        let wf = WorkflowSnapshot {
            id: "wf-1".into(),
            pattern: "sequential".into(),
            steps_completed: 1,
            steps_total: 2,
            status: "running".into(),
            duration_ms: 100,
            started_at: "1970-01-01T00:00:00Z".into(),
            steps: vec![StepSnapshot {
                capability: "echo.ping".into(),
                status: "success".into(),
                latency_ms: 10,
                payload_bytes: 4,
            }],
        };
        let json = serde_json::to_string(&wf).unwrap();
        let back: WorkflowSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(wf, back);
    }

    #[test]
    fn blackboard_entry_json_roundtrip() {
        let entry = BlackboardEntry {
            key: "plan".into(),
            value: "go".into(),
            timestamp_ms: 100,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: BlackboardEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }
}
