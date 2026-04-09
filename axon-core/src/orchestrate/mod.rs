//! Agent orchestration scaffolding for the Axon mesh.
//!
//! Inspired by Fugue (capability-gated hooks, message pipeline) and OpenClaw
//! (file-based agent definitions, blackboard state, heartbeat lifecycle) —
//! distilled to ~2K LOC of composable Rust primitives.
//!
//! # Modules
//!
//! - [`definition`] — TOML-based agent definitions (`AgentDefinition`)
//! - [`blackboard`] — Shared CRDT-backed state (`Blackboard`)
//! - [`hooks`] — Capability-gated pre/post message hooks (`HookRegistry`)
//! - [`lifecycle`] — Agent lifecycle + heartbeat (`ManagedAgent`, `AgentState`)
//! - [`trace`] — Workflow correlation IDs (`WorkflowSpan`)
//! - [`workflow`] — Orchestration patterns: `pipeline`, `fan_out`, `delegate`, `supervisor`

pub mod blackboard;
pub mod definition;
pub mod hooks;
pub mod lifecycle;
pub mod trace;
pub mod workflow;

pub use blackboard::Blackboard;
pub use definition::{AgentDefinition, CapabilityDef, DefinitionError};
pub use hooks::{Hook, HookPhase, HookRegistry, HookResult};
pub use lifecycle::{check_health, spawn_heartbeat, AgentState, ManagedAgent};
pub use trace::{
    emit_step_complete, emit_step_start, emit_workflow_complete, emit_workflow_error, WorkflowId,
    WorkflowSpan,
};
pub use workflow::{
    delegate, fan_out, pipeline, supervisor, swarm_dispatch, PayloadTransform, WorkflowError,
    WorkflowResult, WorkflowStep,
};
