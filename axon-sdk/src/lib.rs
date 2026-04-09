pub use axon_core::{
    Agent, AgentError, Capability, Identity, McpBridge, McpBridgeAgent, McpClient, McpClientError,
    McpServerConfig, McpToolSchema, Message, PeerInfo, Runtime, TaskRequest, TaskResponse,
    TaskStatus, ToolFilter, ToolQueryResult, ToolRegistry, ToolSearchResult, Transport,
};

/// Orchestration scaffolding: workflows, lifecycle, blackboard, hooks.
pub use axon_core::orchestrate;
pub use axon_core::orchestrate::{
    AgentDefinition, AgentState, Blackboard, CapabilityDef, Hook, HookPhase, HookRegistry,
    HookResult, ManagedAgent, PayloadTransform, WorkflowError, WorkflowId, WorkflowResult,
    WorkflowSpan, WorkflowStep,
};

/// Re-export async_trait for agent implementations.
pub use async_trait::async_trait;
