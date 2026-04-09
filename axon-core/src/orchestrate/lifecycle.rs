use std::sync::Arc;
use std::time::{Duration, Instant};
use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::protocol::{TaskRequest, TaskResponse};
use crate::runtime::{Agent, AgentError};

use super::definition::AgentDefinition;
use super::hooks::{HookPhase, HookRegistry, HookResult};

/// Lifecycle state of a managed agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Created,
    Running,
    Paused,
    Stopped,
}

impl std::fmt::Display for AgentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentState::Created => write!(f, "Created"),
            AgentState::Running => write!(f, "Running"),
            AgentState::Paused => write!(f, "Paused"),
            AgentState::Stopped => write!(f, "Stopped"),
        }
    }
}

/// Wraps an `Agent` with lifecycle management and hook support.
///
/// `ManagedAgent` implements the `Agent` trait and can be registered
/// directly into `Runtime::register()` — no changes to Runtime needed.
///
/// State transitions:
/// - `Created` → `Running` (via `start()`)
/// - `Running` → `Paused` (via `pause()`)
/// - `Paused` → `Running` (via `resume()`)
/// - Any → `Stopped` (via `stop()`)
///
/// Paused and stopped agents reject all incoming task requests.
pub struct ManagedAgent {
    pub definition: AgentDefinition,
    inner: Arc<dyn Agent>,
    state: Arc<RwLock<AgentState>>,
    last_heartbeat: Arc<RwLock<Instant>>,
    hooks: HookRegistry,
}

impl ManagedAgent {
    pub fn new(definition: AgentDefinition, inner: Arc<dyn Agent>) -> Self {
        Self {
            definition,
            inner,
            state: Arc::new(RwLock::new(AgentState::Created)),
            last_heartbeat: Arc::new(RwLock::new(Instant::now())),
            hooks: HookRegistry::new(),
        }
    }

    pub fn with_hooks(mut self, hooks: HookRegistry) -> Self {
        self.hooks = hooks;
        self
    }

    pub async fn state(&self) -> AgentState {
        *self.state.read().await
    }

    /// Transition to Running state.
    pub async fn start(&self) {
        let mut s = self.state.write().await;
        if *s == AgentState::Created || *s == AgentState::Paused {
            info!(agent = self.definition.name, "agent starting");
            *s = AgentState::Running;
        }
    }

    /// Pause a running agent (rejects new tasks).
    pub async fn pause(&self) {
        let mut s = self.state.write().await;
        if *s == AgentState::Running {
            info!(agent = self.definition.name, "agent paused");
            *s = AgentState::Paused;
        }
    }

    /// Resume a paused agent.
    pub async fn resume(&self) {
        let mut s = self.state.write().await;
        if *s == AgentState::Paused {
            info!(agent = self.definition.name, "agent resumed");
            *s = AgentState::Running;
        }
    }

    /// Stop the agent permanently.
    pub async fn stop(&self) {
        let mut s = self.state.write().await;
        info!(agent = self.definition.name, state = %*s, "agent stopped");
        *s = AgentState::Stopped;
    }

    /// Update the last heartbeat timestamp to now.
    pub async fn heartbeat(&self) {
        *self.last_heartbeat.write().await = Instant::now();
        debug!(agent = self.definition.name, "heartbeat");
    }

    /// Returns true if the agent heartbeated within `timeout`.
    pub async fn is_alive(&self, timeout: Duration) -> bool {
        self.last_heartbeat.read().await.elapsed() < timeout
    }

    pub fn inner(&self) -> Arc<dyn Agent> {
        self.inner.clone()
    }
}

#[async_trait]
impl Agent for ManagedAgent {
    fn name(&self) -> &str {
        &self.definition.name
    }

    fn capabilities(&self) -> Vec<crate::protocol::Capability> {
        self.definition.to_capabilities()
    }

    async fn handle(&self, mut request: TaskRequest) -> Result<TaskResponse, AgentError> {
        // Check lifecycle state before doing anything
        let current_state = *self.state.read().await;
        match current_state {
            AgentState::Paused => {
                return Err(AgentError::Internal(format!(
                    "agent '{}' is paused",
                    self.definition.name
                )));
            }
            AgentState::Stopped => {
                return Err(AgentError::Internal(format!(
                    "agent '{}' is stopped",
                    self.definition.name
                )));
            }
            AgentState::Created => {
                // Auto-start on first task
                *self.state.write().await = AgentState::Running;
            }
            AgentState::Running => {}
        }

        let granted = &self.definition.permissions;

        // Run BeforeHandle hooks
        match self.hooks.run(
            HookPhase::BeforeHandle,
            &request,
            &request.payload.clone(),
            granted,
        ) {
            HookResult::Continue(modified) => request.payload = modified,
            HookResult::ShortCircuit(resp) => return Ok(resp),
        }

        // Delegate to inner agent
        let mut response = self.inner.handle(request.clone()).await?;

        // Run AfterHandle hooks
        match self.hooks.run(
            HookPhase::AfterHandle,
            &request,
            &response.payload.clone(),
            granted,
        ) {
            HookResult::Continue(modified) => response.payload = modified,
            HookResult::ShortCircuit(resp) => return Ok(resp),
        }

        Ok(response)
    }
}

/// Spawn a background task that sends periodic heartbeats for a managed agent.
/// The returned `JoinHandle` can be aborted to stop heartbeating.
pub fn spawn_heartbeat(agent: Arc<ManagedAgent>, interval: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            let state = agent.state().await;
            if state == AgentState::Stopped {
                break;
            }
            agent.heartbeat().await;
        }
    })
}

/// Check a slice of managed agents and return names of those that haven't
/// heartbeated within `timeout`.
pub async fn check_health(agents: &[Arc<ManagedAgent>], timeout: Duration) -> Vec<String> {
    let mut stale = Vec::new();
    for agent in agents {
        if !agent.is_alive(timeout).await {
            warn!(
                agent = agent.definition.name,
                "agent heartbeat timeout — considered unhealthy"
            );
            stale.push(agent.definition.name.clone());
        }
    }
    stale
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use crate::protocol::{Capability, TaskStatus};
    use uuid::Uuid;

    struct EchoAgent {
        name: String,
        caps: Vec<Capability>,
        call_count: Arc<AtomicUsize>,
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
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(TaskResponse {
                request_id: request.id,
                status: TaskStatus::Success,
                payload: request.payload,
                duration_ms: 0,
            })
        }
    }

    fn make_echo(name: &str) -> (Arc<AtomicUsize>, Arc<ManagedAgent>) {
        let count = Arc::new(AtomicUsize::new(0));
        let inner = Arc::new(EchoAgent {
            name: name.to_string(),
            caps: vec![Capability::new("test", "echo", 1)],
            call_count: count.clone(),
        });
        let def_toml = format!(r#"name = "{name}""#);
        let def = super::super::definition::AgentDefinition::from_toml(&def_toml).unwrap();
        (count, Arc::new(ManagedAgent::new(def, inner)))
    }

    fn make_request() -> TaskRequest {
        TaskRequest {
            id: Uuid::new_v4(),
            capability: Capability::new("test", "echo", 1),
            payload: b"ping".to_vec(),
            timeout_ms: 5000,
        }
    }

    #[tokio::test]
    async fn managed_agent_starts_created() {
        let (_, agent) = make_echo("a");
        assert_eq!(agent.state().await, AgentState::Created);
    }

    #[tokio::test]
    async fn start_transitions_to_running() {
        let (_, agent) = make_echo("a");
        agent.start().await;
        assert_eq!(agent.state().await, AgentState::Running);
    }

    #[tokio::test]
    async fn pause_transitions_from_running() {
        let (_, agent) = make_echo("a");
        agent.start().await;
        agent.pause().await;
        assert_eq!(agent.state().await, AgentState::Paused);
    }

    #[tokio::test]
    async fn resume_from_paused() {
        let (_, agent) = make_echo("a");
        agent.start().await;
        agent.pause().await;
        agent.resume().await;
        assert_eq!(agent.state().await, AgentState::Running);
    }

    #[tokio::test]
    async fn stop_from_running() {
        let (_, agent) = make_echo("a");
        agent.start().await;
        agent.stop().await;
        assert_eq!(agent.state().await, AgentState::Stopped);
    }

    #[tokio::test]
    async fn stop_from_created() {
        let (_, agent) = make_echo("a");
        agent.stop().await;
        assert_eq!(agent.state().await, AgentState::Stopped);
    }

    #[tokio::test]
    async fn running_agent_delegates_to_inner() {
        let (count, agent) = make_echo("a");
        agent.start().await;
        let resp = agent.handle(make_request()).await.unwrap();
        assert_eq!(resp.status, TaskStatus::Success);
        assert_eq!(resp.payload, b"ping");
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn created_agent_auto_starts_on_handle() {
        let (count, agent) = make_echo("a");
        // Call handle without explicit start()
        let resp = agent.handle(make_request()).await.unwrap();
        assert_eq!(resp.status, TaskStatus::Success);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(agent.state().await, AgentState::Running);
    }

    #[tokio::test]
    async fn paused_agent_rejects_tasks() {
        let (count, agent) = make_echo("a");
        agent.start().await;
        agent.pause().await;
        let result = agent.handle(make_request()).await;
        assert!(result.is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stopped_agent_rejects_tasks() {
        let (count, agent) = make_echo("a");
        agent.start().await;
        agent.stop().await;
        let result = agent.handle(make_request()).await;
        assert!(result.is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn heartbeat_updates_timestamp() {
        let (_, agent) = make_echo("a");
        // Sleep a bit then heartbeat — the heartbeat should be fresh
        tokio::time::sleep(Duration::from_millis(50)).await;
        agent.heartbeat().await;
        assert!(agent.is_alive(Duration::from_millis(100)).await);
    }

    #[tokio::test]
    async fn is_alive_true_when_recent_heartbeat() {
        let (_, agent) = make_echo("a");
        agent.heartbeat().await;
        assert!(agent.is_alive(Duration::from_secs(10)).await);
    }

    #[tokio::test]
    async fn is_alive_false_when_stale() {
        let (_, agent) = make_echo("a");
        // The agent was created slightly in the past; use 0ms timeout
        assert!(!agent.is_alive(Duration::from_nanos(0)).await);
    }

    #[tokio::test]
    async fn check_health_returns_stale_agents() {
        let (_, a1) = make_echo("fresh");
        let (_, a2) = make_echo("stale");
        // Sleep 60ms so both agents' initial heartbeat (set at creation) is 60ms old
        tokio::time::sleep(Duration::from_millis(60)).await;
        // Re-heartbeat a1 so it becomes fresh again
        a1.heartbeat().await;
        // 30ms timeout: a1 heartbeated ~0ms ago (fresh), a2 heartbeated ~60ms ago (stale)
        let stale = check_health(&[a1, a2], Duration::from_millis(30)).await;
        assert!(stale.contains(&"stale".to_string()));
        assert!(!stale.contains(&"fresh".to_string()));
    }

    #[tokio::test]
    async fn spawn_heartbeat_keeps_agent_alive() {
        let (_, agent) = make_echo("hb");
        let agent_arc = agent.clone();
        agent.start().await;
        let handle = spawn_heartbeat(agent_arc, Duration::from_millis(10));
        tokio::time::sleep(Duration::from_millis(50)).await;
        // After heartbeating every 10ms for 50ms, agent should be alive with 100ms timeout
        assert!(agent.is_alive(Duration::from_millis(100)).await);
        handle.abort();
    }

    #[tokio::test]
    async fn managed_agent_capabilities_from_definition() {
        let def = super::super::definition::AgentDefinition::from_toml(r#"
name = "capped"
[[capabilities]]
namespace = "llm"
name = "chat"
version = 1
"#).unwrap();
        let inner = Arc::new(EchoAgent {
            name: "capped".to_string(),
            caps: vec![],
            call_count: Arc::new(AtomicUsize::new(0)),
        });
        let agent = ManagedAgent::new(def, inner);
        let caps = agent.capabilities();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].namespace, "llm");
    }
}
