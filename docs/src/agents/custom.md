# Writing Custom Agents

Use `axon-sdk` to build agents that plug into the mesh.

## Minimal Example

```rust
use axon_sdk::{async_trait, Agent, AgentError, Capability, TaskRequest, TaskResponse, TaskStatus};

struct ReverseAgent;

#[async_trait]
impl Agent for ReverseAgent {
    fn name(&self) -> &str {
        "reverse"
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("text", "reverse", 1)]
    }

    async fn handle(&self, req: TaskRequest) -> Result<TaskResponse, AgentError> {
        let text = String::from_utf8(req.payload)
            .map_err(|e| AgentError::Internal(e.to_string()))?;

        let reversed: String = text.chars().rev().collect();

        Ok(TaskResponse {
            request_id: req.id,
            status: TaskStatus::Success,
            payload: reversed.into_bytes(),
            duration_ms: 0,
        })
    }
}
```

## Register with Runtime

```rust
use std::sync::Arc;
use axon_sdk::Runtime;

let runtime = Runtime::new();
runtime.register(Arc::new(ReverseAgent)).await;
```

## Cargo.toml

```toml
[dependencies]
axon-sdk = { path = "../axon-sdk" }
# or from git:
# axon-sdk = { git = "https://github.com/teddytennant/axon" }
```

## Multi-Capability Agents

An agent can handle multiple capabilities:

```rust
fn capabilities(&self) -> Vec<Capability> {
    vec![
        Capability::new("text", "reverse", 1),
        Capability::new("text", "uppercase", 1),
        Capability::new("text", "lowercase", 1),
    ]
}

async fn handle(&self, req: TaskRequest) -> Result<TaskResponse, AgentError> {
    let text = String::from_utf8(req.payload)
        .map_err(|e| AgentError::Internal(e.to_string()))?;

    let result = match req.capability.name.as_str() {
        "reverse" => text.chars().rev().collect(),
        "uppercase" => text.to_uppercase(),
        "lowercase" => text.to_lowercase(),
        _ => return Err(AgentError::Internal("unknown capability".into())),
    };

    Ok(TaskResponse {
        request_id: req.id,
        status: TaskStatus::Success,
        payload: result.into_bytes(),
        duration_ms: 0,
    })
}
```
