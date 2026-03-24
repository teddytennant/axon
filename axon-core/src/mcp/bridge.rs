use crate::mcp::client::{McpClient, McpClientError, McpServerConfig};
use crate::mcp::schema::McpToolSchema;
use crate::protocol::{Capability, TaskRequest, TaskResponse, TaskStatus};
use crate::runtime::{Agent, AgentError};

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// MCP Bridge — manages connections to multiple MCP servers and provides
/// their tools as axon capabilities.
///
/// The bridge:
/// 1. Spawns MCP server processes via stdio
/// 2. Initializes each server and discovers its tools
/// 3. Exposes all discovered tools as axon Capabilities
/// 4. Routes incoming TaskRequests to the right MCP server via tools/call
///
/// This is the component that turns every axon node into an MCP gateway.
pub struct McpBridge {
    /// Live MCP server clients, keyed by server name
    clients: RwLock<HashMap<String, Arc<McpClient>>>,
    /// All discovered tools across all servers
    all_tools: RwLock<Vec<McpToolSchema>>,
}

impl McpBridge {
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
            all_tools: RwLock::new(Vec::new()),
        }
    }

    /// Connect to an MCP server: spawn, initialize, discover tools.
    pub async fn connect_server(
        &self,
        config: McpServerConfig,
    ) -> Result<Vec<McpToolSchema>, McpClientError> {
        let server_name = config.name.clone();

        // Spawn and initialize
        let client = McpClient::spawn(config).await?;
        client.initialize().await?;
        let tools = client.discover_tools().await?;

        let tool_count = tools.len();
        let client = Arc::new(client);

        // Register the client
        {
            let mut clients = self.clients.write().await;
            clients.insert(server_name.clone(), client);
        }

        // Update the combined tool list
        {
            let mut all = self.all_tools.write().await;
            all.retain(|t| t.server_name != server_name);
            all.extend(tools.clone());
        }

        info!(
            "MCP bridge: connected to '{}' with {} tools",
            server_name, tool_count
        );

        Ok(tools)
    }

    /// Connect to multiple MCP servers. Failures are logged but don't prevent
    /// other servers from connecting.
    pub async fn connect_all(&self, configs: Vec<McpServerConfig>) -> Vec<McpToolSchema> {
        let mut all_tools = Vec::new();

        for config in configs {
            let name = config.name.clone();
            match self.connect_server(config).await {
                Ok(tools) => {
                    all_tools.extend(tools);
                }
                Err(e) => {
                    error!("Failed to connect to MCP server '{}': {}", name, e);
                }
            }
        }

        all_tools
    }

    /// Call a tool on a specific MCP server.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpClientError> {
        let clients = self.clients.read().await;
        let client = clients.get(server_name).ok_or_else(|| {
            McpClientError::McpError(
                server_name.to_string(),
                format!("no connected server named '{}'", server_name),
            )
        })?;

        client.call_tool(tool_name, arguments).await
    }

    /// Get all discovered tools across all connected servers.
    pub async fn all_tools(&self) -> Vec<McpToolSchema> {
        self.all_tools.read().await.clone()
    }

    /// Get all capabilities derived from discovered MCP tools.
    pub async fn capabilities(&self) -> Vec<Capability> {
        self.all_tools
            .read()
            .await
            .iter()
            .map(|t| t.to_capability())
            .collect()
    }

    /// Number of connected MCP servers.
    pub async fn server_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Names of connected MCP servers.
    pub async fn server_names(&self) -> Vec<String> {
        self.clients.read().await.keys().cloned().collect()
    }

    /// Disconnect a specific MCP server.
    pub async fn disconnect_server(&self, server_name: &str) {
        let client = {
            let mut clients = self.clients.write().await;
            clients.remove(server_name)
        };

        if let Some(client) = client {
            client.shutdown().await;
            let mut all = self.all_tools.write().await;
            all.retain(|t| t.server_name != server_name);
            info!("MCP bridge: disconnected from '{}'", server_name);
        }
    }

    /// Shut down all connected MCP servers.
    pub async fn shutdown(&self) {
        let clients: Vec<(String, Arc<McpClient>)> = {
            let mut map = self.clients.write().await;
            map.drain().collect()
        };

        for (name, client) in clients {
            client.shutdown().await;
            info!("MCP bridge: shut down '{}'", name);
        }

        self.all_tools.write().await.clear();
    }
}

impl Default for McpBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Agent implementation that bridges MCP tools into the axon mesh.
///
/// When a TaskRequest arrives with a capability matching `mcp.<server>:<tool>:v1`,
/// this agent:
/// 1. Extracts the server name and tool name from the capability
/// 2. Deserializes the payload as JSON tool arguments
/// 3. Calls the MCP server via `tools/call`
/// 4. Returns the result as the task response payload
pub struct McpBridgeAgent {
    bridge: Arc<McpBridge>,
}

impl McpBridgeAgent {
    pub fn new(bridge: Arc<McpBridge>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl Agent for McpBridgeAgent {
    fn name(&self) -> &str {
        "mcp-bridge"
    }

    fn capabilities(&self) -> Vec<Capability> {
        // This is synchronous but we need the async tool list.
        // We use try_read to avoid blocking — if we can't read, return empty.
        // The capabilities are populated after connect_all completes.
        match self.bridge.all_tools.try_read() {
            Ok(tools) => tools.iter().map(|t| t.to_capability()).collect(),
            Err(_) => {
                warn!("MCP bridge: couldn't read tools for capabilities (lock contention)");
                Vec::new()
            }
        }
    }

    async fn handle(&self, request: TaskRequest) -> Result<TaskResponse, AgentError> {
        // Extract server name from capability namespace: "mcp.filesystem" → "filesystem"
        let server_name = request
            .capability
            .namespace
            .strip_prefix("mcp.")
            .ok_or_else(|| {
                AgentError::Internal(format!(
                    "invalid MCP capability namespace: {}",
                    request.capability.namespace
                ))
            })?;

        let tool_name = &request.capability.name;

        // Parse the payload as JSON tool arguments
        let arguments: serde_json::Value = if request.payload.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_slice(&request.payload)
                .map_err(|e| AgentError::Internal(format!("invalid JSON payload: {}", e)))?
        };

        // Call the MCP server
        let result = self
            .bridge
            .call_tool(server_name, tool_name, arguments)
            .await
            .map_err(|e| AgentError::Internal(e.to_string()))?;

        // Serialize result as response payload
        let payload = serde_json::to_vec(&result).unwrap_or_default();

        Ok(TaskResponse {
            request_id: request.id,
            status: TaskStatus::Success,
            payload,
            duration_ms: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn bridge_starts_empty() {
        let bridge = McpBridge::new();
        assert_eq!(bridge.server_count().await, 0);
        assert!(bridge.all_tools().await.is_empty());
        assert!(bridge.capabilities().await.is_empty());
    }

    #[tokio::test]
    async fn bridge_connect_nonexistent_fails_gracefully() {
        let bridge = McpBridge::new();
        let config = McpServerConfig::new("bad", "/nonexistent/mcp/server");
        let result = bridge.connect_server(config).await;
        assert!(result.is_err());
        // Bridge should still be usable
        assert_eq!(bridge.server_count().await, 0);
    }

    #[tokio::test]
    async fn bridge_connect_all_skips_failures() {
        let bridge = McpBridge::new();
        let configs = vec![
            McpServerConfig::new("bad1", "/nonexistent/1"),
            McpServerConfig::new("bad2", "/nonexistent/2"),
        ];
        let tools = bridge.connect_all(configs).await;
        assert!(tools.is_empty());
        assert_eq!(bridge.server_count().await, 0);
    }

    #[tokio::test]
    async fn bridge_call_tool_no_server() {
        let bridge = McpBridge::new();
        let result = bridge.call_tool("missing", "read_file", json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn bridge_disconnect_nonexistent_is_noop() {
        let bridge = McpBridge::new();
        bridge.disconnect_server("nonexistent").await;
        assert_eq!(bridge.server_count().await, 0);
    }

    #[tokio::test]
    async fn bridge_shutdown_empty_is_noop() {
        let bridge = McpBridge::new();
        bridge.shutdown().await;
    }

    #[test]
    fn bridge_agent_name() {
        let bridge = Arc::new(McpBridge::new());
        let agent = McpBridgeAgent::new(bridge);
        assert_eq!(agent.name(), "mcp-bridge");
    }

    #[test]
    fn bridge_agent_empty_capabilities() {
        let bridge = Arc::new(McpBridge::new());
        let agent = McpBridgeAgent::new(bridge);
        assert!(agent.capabilities().is_empty());
    }

    #[tokio::test]
    async fn bridge_agent_handle_invalid_namespace() {
        let bridge = Arc::new(McpBridge::new());
        let agent = McpBridgeAgent::new(bridge);

        let request = TaskRequest {
            id: uuid::Uuid::new_v4(),
            capability: Capability::new("not_mcp", "read_file", 1),
            payload: vec![],
            timeout_ms: 5000,
        };

        let result = agent.handle(request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn bridge_agent_handle_no_server() {
        let bridge = Arc::new(McpBridge::new());
        let agent = McpBridgeAgent::new(bridge);

        let request = TaskRequest {
            id: uuid::Uuid::new_v4(),
            capability: Capability::new("mcp.filesystem", "read_file", 1),
            payload: json!({"path": "/tmp/test"}).to_string().into_bytes(),
            timeout_ms: 5000,
        };

        let result = agent.handle(request).await;
        assert!(result.is_err());
    }

    #[test]
    fn capability_from_tool_schema() {
        let tool = McpToolSchema::new(
            "read_file",
            "Read a file",
            json!({"type": "object"}),
            "filesystem",
        );
        let cap = tool.to_capability();
        assert_eq!(cap.namespace, "mcp.filesystem");
        assert_eq!(cap.name, "read_file");
        assert_eq!(cap.version, 1);
    }

    #[test]
    fn server_name_extraction() {
        let namespace = "mcp.filesystem";
        let server = namespace.strip_prefix("mcp.").unwrap();
        assert_eq!(server, "filesystem");

        let namespace = "mcp.github";
        let server = namespace.strip_prefix("mcp.").unwrap();
        assert_eq!(server, "github");
    }

    #[test]
    fn payload_deserialization() {
        let args = json!({"path": "/tmp/test", "encoding": "utf-8"});
        let payload = serde_json::to_vec(&args).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed, args);
    }

    #[test]
    fn empty_payload_becomes_empty_object() {
        let payload: &[u8] = &[];
        let args: serde_json::Value = if payload.is_empty() {
            json!({})
        } else {
            serde_json::from_slice(payload).unwrap()
        };
        assert_eq!(args, json!({}));
    }
}
