use axon_sdk::{async_trait, Agent, AgentError, Capability, TaskRequest, TaskResponse, TaskStatus};
use crate::providers::{CompletionRequest, LlmProvider};
use std::sync::Arc;

/// Echo agent — returns the input payload as-is. Useful for testing and diagnostics.
pub struct EchoAgent;

#[async_trait]
impl Agent for EchoAgent {
    fn name(&self) -> &str {
        "echo"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("echo", "ping", 1)]
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

/// System info agent — returns basic system information.
pub struct SystemInfoAgent;

#[async_trait]
impl Agent for SystemInfoAgent {
    fn name(&self) -> &str {
        "sysinfo"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("system", "info", 1)]
    }

    async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
        let info = serde_json::json!({
            "hostname": hostname(),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        });

        Ok(TaskResponse {
            request_id: request.id,
            status: TaskStatus::Success,
            payload: info.to_string().into_bytes(),
            duration_ms: 0,
        })
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Multi-provider LLM agent — routes to any configured LLM backend.
pub struct LlmAgent {
    provider: Arc<dyn LlmProvider>,
}

impl LlmAgent {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl Agent for LlmAgent {
    fn name(&self) -> &str {
        "llm"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("llm", "chat", 1)]
    }

    async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
        let prompt = String::from_utf8(request.payload)
            .map_err(|e| AgentError::Internal(format!("invalid UTF-8 payload: {}", e)))?;

        let completion = self.provider
            .complete(CompletionRequest {
                prompt,
                max_tokens: None,
                temperature: None,
            })
            .await
            .map_err(|e| AgentError::Internal(format!("{} error: {}", self.provider.name(), e)))?;

        Ok(TaskResponse {
            request_id: request.id,
            status: TaskStatus::Success,
            payload: completion.text.into_bytes(),
            duration_ms: 0,
        })
    }
}
