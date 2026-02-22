# Agent Model

Agents are the units of computation in Axon. Each agent declares capabilities and handles incoming task requests.

## The Agent Trait

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> Vec<Capability>;
    async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError>;
}
```

### `name()`

A human-readable identifier for logging and the TUI dashboard.

### `capabilities()`

Returns the list of capabilities this agent can handle. Each capability is a `(namespace, name, version)` tuple. The runtime uses these to route incoming tasks.

### `handle()`

Processes a `TaskRequest` and returns a `TaskResponse`. The request contains:

```rust
struct TaskRequest {
    id: Uuid,
    capability: Capability,
    payload: Vec<u8>,
    timeout_ms: u64,
}
```

The response contains:

```rust
struct TaskResponse {
    request_id: Uuid,
    status: TaskStatus,
    payload: Vec<u8>,
    duration_ms: u64,
}
```

## Error Handling

Agents return `AgentError` on failure:

```rust
enum AgentError {
    Internal(String),
    Timeout,
}
```

The runtime catches errors and converts them to `TaskResponse` with `TaskStatus::Error`.

## Registration

Agents are registered with the runtime at startup:

```rust
let runtime = Runtime::new();
runtime.register(Arc::new(MyAgent)).await;
```

Multiple agents can be registered. The runtime dispatches to the first agent whose capabilities match the request.
