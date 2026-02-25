use crate::protocol::{Capability, TaskRequest, TaskResponse, TaskStatus};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Agent error: {0}")]
    Internal(String),
    #[error("Timeout")]
    Timeout,
    #[error("No agent found for capability")]
    NoCapability,
}

/// Trait that all agents must implement.
#[async_trait]
pub trait Agent: Send + Sync + 'static {
    /// Unique name for this agent type.
    fn name(&self) -> &str;

    /// Capabilities this agent provides.
    fn capabilities(&self) -> Vec<Capability>;

    /// Handle an incoming task request.
    async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError>;
}

/// Runtime that manages registered agents and dispatches tasks.
pub struct Runtime {
    agents: Arc<RwLock<Vec<Arc<dyn Agent>>>>,
    capability_index: Arc<RwLock<HashMap<String, Vec<usize>>>>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(Vec::new())),
            capability_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an agent with the runtime.
    pub async fn register(&self, agent: Arc<dyn Agent>) {
        let mut agents = self.agents.write().await;
        let idx = agents.len();
        let caps = agent.capabilities();
        info!("Registering agent '{}' with {} capabilities", agent.name(), caps.len());

        let mut index = self.capability_index.write().await;
        for cap in &caps {
            let tag = cap.tag();
            index.entry(tag).or_insert_with(Vec::new).push(idx);
        }

        agents.push(agent);
    }

    /// Get all capabilities from all registered agents.
    pub async fn all_capabilities(&self) -> Vec<Capability> {
        let agents = self.agents.read().await;
        agents.iter().flat_map(|a| a.capabilities()).collect()
    }

    /// Get names of all registered agents.
    pub async fn agent_names(&self) -> Vec<String> {
        let agents = self.agents.read().await;
        agents.iter().map(|a| a.name().to_string()).collect()
    }

    /// Number of registered agents.
    pub async fn agent_count(&self) -> usize {
        self.agents.read().await.len()
    }

    /// Dispatch a task to the first matching agent.
    ///
    /// Enforces the timeout specified in `request.timeout_ms`. A value of 0
    /// is treated as "use the default" (30 seconds). If the handler does not
    /// complete within the deadline, a `TaskStatus::Timeout` response is
    /// returned.
    pub async fn dispatch(&self, request: TaskRequest) -> TaskResponse {
        let start = std::time::Instant::now();
        let agents = self.agents.read().await;

        // Use the request timeout, falling back to 30s when unset (0).
        let timeout_ms = if request.timeout_ms == 0 { 30_000 } else { request.timeout_ms };
        let deadline = Duration::from_millis(timeout_ms);

        // Find an agent that can handle the requested capability
        for agent in agents.iter() {
            if agent.capabilities().iter().any(|c| c.matches(&request.capability)) {
                debug!("Dispatching task {} to agent '{}' (timeout {}ms)", request.id, agent.name(), timeout_ms);
                match tokio::time::timeout(deadline, agent.handle(request.clone())).await {
                    Ok(Ok(mut response)) => {
                        response.duration_ms = start.elapsed().as_millis() as u64;
                        return response;
                    }
                    Ok(Err(e)) => {
                        error!("Agent '{}' failed: {}", agent.name(), e);
                        return TaskResponse {
                            request_id: request.id,
                            status: TaskStatus::Error(e.to_string()),
                            payload: vec![],
                            duration_ms: start.elapsed().as_millis() as u64,
                        };
                    }
                    Err(_) => {
                        warn!("Agent '{}' timed out after {}ms for task {}", agent.name(), timeout_ms, request.id);
                        return TaskResponse {
                            request_id: request.id,
                            status: TaskStatus::Timeout,
                            payload: vec![],
                            duration_ms: start.elapsed().as_millis() as u64,
                        };
                    }
                }
            }
        }

        TaskResponse {
            request_id: request.id,
            status: TaskStatus::NoCapability,
            payload: vec![],
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    struct MockAgent {
        name: String,
        caps: Vec<Capability>,
        response_payload: Vec<u8>,
    }

    #[async_trait]
    impl Agent for MockAgent {
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
                payload: self.response_payload.clone(),
                duration_ms: 0,
            })
        }
    }

    struct FailingAgent;

    #[async_trait]
    impl Agent for FailingAgent {
        fn name(&self) -> &str {
            "failing"
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::new("test", "fail", 1)]
        }

        async fn handle(&self, _request: TaskRequest) -> Result<TaskResponse, AgentError> {
            Err(AgentError::Internal("intentional failure".to_string()))
        }
    }

    fn make_request(namespace: &str, name: &str) -> TaskRequest {
        TaskRequest {
            id: Uuid::new_v4(),
            capability: Capability::new(namespace, name, 1),
            payload: vec![],
            timeout_ms: 5000,
        }
    }

    #[tokio::test]
    async fn runtime_starts_empty() {
        let rt = Runtime::new();
        assert_eq!(rt.agent_count().await, 0);
        assert!(rt.all_capabilities().await.is_empty());
    }

    #[tokio::test]
    async fn runtime_register_agent() {
        let rt = Runtime::new();
        let agent = Arc::new(MockAgent {
            name: "test".to_string(),
            caps: vec![Capability::new("echo", "ping", 1)],
            response_payload: b"pong".to_vec(),
        });

        rt.register(agent).await;
        assert_eq!(rt.agent_count().await, 1);
        assert_eq!(rt.agent_names().await, vec!["test"]);
    }

    #[tokio::test]
    async fn runtime_all_capabilities() {
        let rt = Runtime::new();
        rt.register(Arc::new(MockAgent {
            name: "a1".to_string(),
            caps: vec![
                Capability::new("llm", "chat", 1),
                Capability::new("llm", "embed", 1),
            ],
            response_payload: vec![],
        }))
        .await;
        rt.register(Arc::new(MockAgent {
            name: "a2".to_string(),
            caps: vec![Capability::new("code", "review", 1)],
            response_payload: vec![],
        }))
        .await;

        let caps = rt.all_capabilities().await;
        assert_eq!(caps.len(), 3);
    }

    #[tokio::test]
    async fn runtime_dispatch_matching() {
        let rt = Runtime::new();
        rt.register(Arc::new(MockAgent {
            name: "echo".to_string(),
            caps: vec![Capability::new("echo", "ping", 1)],
            response_payload: b"pong".to_vec(),
        }))
        .await;

        let req = make_request("echo", "ping");
        let resp = rt.dispatch(req).await;
        assert_eq!(resp.status, TaskStatus::Success);
        assert_eq!(resp.payload, b"pong");
    }

    #[tokio::test]
    async fn runtime_dispatch_no_match() {
        let rt = Runtime::new();
        rt.register(Arc::new(MockAgent {
            name: "echo".to_string(),
            caps: vec![Capability::new("echo", "ping", 1)],
            response_payload: vec![],
        }))
        .await;

        let req = make_request("code", "review");
        let resp = rt.dispatch(req).await;
        assert_eq!(resp.status, TaskStatus::NoCapability);
    }

    #[tokio::test]
    async fn runtime_dispatch_failing_agent() {
        let rt = Runtime::new();
        rt.register(Arc::new(FailingAgent)).await;

        let req = make_request("test", "fail");
        let resp = rt.dispatch(req).await;
        match resp.status {
            TaskStatus::Error(msg) => assert!(msg.contains("intentional failure")),
            _ => panic!("expected error status"),
        }
    }

    #[tokio::test]
    async fn runtime_dispatch_empty() {
        let rt = Runtime::new();
        let req = make_request("any", "thing");
        let resp = rt.dispatch(req).await;
        assert_eq!(resp.status, TaskStatus::NoCapability);
    }

    #[tokio::test]
    async fn runtime_dispatch_duration_set() {
        let rt = Runtime::new();
        rt.register(Arc::new(MockAgent {
            name: "slow".to_string(),
            caps: vec![Capability::new("test", "slow", 1)],
            response_payload: vec![],
        }))
        .await;

        let req = make_request("test", "slow");
        let resp = rt.dispatch(req).await;
        // Duration should be set (>= 0)
        assert!(resp.duration_ms < 1000); // shouldn't take more than 1s
    }

    struct SlowAgent;

    #[async_trait]
    impl Agent for SlowAgent {
        fn name(&self) -> &str {
            "slow"
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::new("test", "slow", 1)]
        }

        async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            Ok(TaskResponse {
                request_id: request.id,
                status: TaskStatus::Success,
                payload: vec![],
                duration_ms: 0,
            })
        }
    }

    #[tokio::test]
    async fn runtime_dispatch_timeout() {
        let rt = Runtime::new();
        rt.register(Arc::new(SlowAgent)).await;

        let req = TaskRequest {
            id: Uuid::new_v4(),
            capability: Capability::new("test", "slow", 1),
            payload: vec![],
            timeout_ms: 50, // 50ms timeout — the SlowAgent sleeps for 10s
        };
        let resp = rt.dispatch(req).await;
        assert_eq!(resp.status, TaskStatus::Timeout);
    }

    #[tokio::test]
    async fn runtime_multiple_agents_first_match() {
        let rt = Runtime::new();
        rt.register(Arc::new(MockAgent {
            name: "first".to_string(),
            caps: vec![Capability::new("echo", "ping", 1)],
            response_payload: b"first".to_vec(),
        }))
        .await;
        rt.register(Arc::new(MockAgent {
            name: "second".to_string(),
            caps: vec![Capability::new("echo", "ping", 1)],
            response_payload: b"second".to_vec(),
        }))
        .await;

        let req = make_request("echo", "ping");
        let resp = rt.dispatch(req).await;
        // First registered agent should handle it
        assert_eq!(resp.payload, b"first");
    }
}
