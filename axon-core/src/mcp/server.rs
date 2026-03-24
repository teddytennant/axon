//! MCP Server — exposes axon's aggregated tools via the MCP protocol on stdio.
//!
//! This is the bridge between axon and the AI agent ecosystem. Any MCP-capable
//! client (Claude Code, Cursor, etc.) can connect to `axon serve-mcp` and get
//! access to tools from all connected MCP servers, with budget-constrained
//! tool selection to minimize context window overhead.
//!
//! ## Modes
//!
//! - **Local mode** (`McpStdioServer::new`): serves tools from locally connected
//!   MCP servers only.
//! - **Mesh mode** (`McpStdioServer::new_with_mesh`): joins the axon mesh and
//!   serves tools from both local MCP servers AND remote mesh peers. Tool calls
//!   for remote tools are forwarded via QUIC to the owning peer.
//!
//! ## Protocol
//!
//! Standard MCP over stdio (newline-delimited JSON-RPC 2.0):
//! - `initialize` → server capabilities and info
//! - `notifications/initialized` → no-op acknowledgment
//! - `tools/list` → aggregated tools from local + remote sources
//! - `tools/call` → routed to the correct local server or remote peer

use crate::discovery::PeerTable;
use crate::mcp::bridge::McpBridge;
use crate::mcp::jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::mcp::registry::ToolRegistry;
use crate::mcp::schema::McpToolSchema;
use crate::protocol::{Capability, Message, TaskRequest, TaskStatus};
use crate::transport::Transport;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Error type for the MCP server.
#[derive(Debug, thiserror::Error)]
pub enum McpServerError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Bridge error: {0}")]
    Bridge(String),
}

/// Tool entry mapping an exposed tool name to its source and original schema.
#[derive(Debug, Clone)]
struct ToolEntry {
    /// The MCP server that owns this tool
    server_name: String,
    /// The original tool name (may differ from exposed name if prefixed)
    original_name: String,
    /// Full tool schema
    schema: McpToolSchema,
    /// For remote mesh tools: the peer ID that owns this tool.
    /// None for tools served by the local bridge.
    remote_peer_id: Option<Vec<u8>>,
}

/// Mesh context for the MCP gateway — enables remote tool discovery and routing.
pub struct MeshContext {
    pub transport: Arc<Transport>,
    pub tool_registry: Arc<RwLock<ToolRegistry>>,
    pub peer_table: Arc<RwLock<PeerTable>>,
    pub local_peer_id: Vec<u8>,
}

/// MCP server that aggregates tools from connected MCP servers and serves them
/// via the MCP protocol on stdio. Optionally joins the mesh to serve remote tools.
pub struct McpStdioServer {
    bridge: Arc<McpBridge>,
    /// Static tool map for local tools (always available)
    tools: HashMap<String, ToolEntry>,
    /// Ordered list of exposed tool names (for deterministic tools/list)
    tool_order: Vec<String>,
    /// Optional mesh context for aggregating and routing remote tools
    mesh: Option<MeshContext>,
}

impl McpStdioServer {
    /// Create a new MCP server from a connected bridge (local mode only).
    ///
    /// Builds the tool map from all tools discovered by the bridge. If two
    /// servers expose tools with the same name, both are prefixed with their
    /// server name (e.g., `filesystem__read_file`).
    pub async fn new(bridge: Arc<McpBridge>) -> Self {
        let all_tools = bridge.all_tools().await;
        let (tools, tool_order) = Self::build_tool_map(all_tools);

        info!(
            "MCP server initialized with {} local tools from {} servers",
            tools.len(),
            {
                let servers: std::collections::HashSet<_> =
                    tools.values().map(|e| &e.server_name).collect();
                servers.len()
            }
        );

        Self {
            bridge,
            tools,
            tool_order,
            mesh: None,
        }
    }

    /// Create a new MCP server with mesh connectivity.
    ///
    /// In addition to local tools from the bridge, this server discovers and
    /// serves tools from remote mesh peers via the ToolRegistry. Tool calls
    /// for remote tools are forwarded to the owning peer via QUIC.
    pub async fn new_with_mesh(bridge: Arc<McpBridge>, mesh: MeshContext) -> Self {
        let all_tools = bridge.all_tools().await;
        let (tools, tool_order) = Self::build_tool_map(all_tools);

        let local_count = tools.len();
        let remote_count = {
            let reg = mesh.tool_registry.read().await;
            reg.remote_unique_tools(&mesh.local_peer_id).len()
        };

        info!(
            "MCP mesh gateway initialized: {} local tools, {} remote tools from mesh",
            local_count, remote_count,
        );

        Self {
            bridge,
            tools,
            tool_order,
            mesh: Some(mesh),
        }
    }

    /// Build the tool map from a list of local MCP tool schemas.
    fn build_tool_map(all_tools: Vec<McpToolSchema>) -> (HashMap<String, ToolEntry>, Vec<String>) {
        let mut name_count: HashMap<String, usize> = HashMap::new();
        for tool in &all_tools {
            *name_count.entry(tool.name.clone()).or_insert(0) += 1;
        }

        let mut tools = HashMap::new();
        let mut tool_order = Vec::new();

        for tool in all_tools {
            let exposed_name = if name_count.get(&tool.name).copied().unwrap_or(0) > 1 {
                format!("{}__{}", tool.server_name, tool.name)
            } else {
                tool.name.clone()
            };

            tool_order.push(exposed_name.clone());
            tools.insert(
                exposed_name,
                ToolEntry {
                    server_name: tool.server_name.clone(),
                    original_name: tool.name.clone(),
                    schema: tool,
                    remote_peer_id: None,
                },
            );
        }

        (tools, tool_order)
    }

    /// Build a combined tool map from local bridge tools + remote registry tools.
    /// Used by mesh mode to produce a full tool list on each tools/list request.
    async fn build_mesh_tool_map(&self) -> (HashMap<String, ToolEntry>, Vec<String>) {
        let mesh = match &self.mesh {
            Some(m) => m,
            None => return (self.tools.clone(), self.tool_order.clone()),
        };

        // Get local tools from bridge
        let local_tools = self.bridge.all_tools().await;

        // Get remote tools from registry (excluding local peer)
        let remote_tools = {
            let reg = mesh.tool_registry.read().await;
            reg.remote_unique_tools(&mesh.local_peer_id)
        };

        // Count all tool names for collision detection
        let mut name_count: HashMap<String, usize> = HashMap::new();
        for tool in &local_tools {
            *name_count.entry(tool.name.clone()).or_insert(0) += 1;
        }
        for (tool, _) in &remote_tools {
            *name_count.entry(tool.name.clone()).or_insert(0) += 1;
        }

        let mut tools = HashMap::new();
        let mut tool_order = Vec::new();

        // Add local tools first (they take priority)
        for tool in local_tools {
            let exposed_name = if name_count.get(&tool.name).copied().unwrap_or(0) > 1 {
                format!("{}__{}", tool.server_name, tool.name)
            } else {
                tool.name.clone()
            };

            tool_order.push(exposed_name.clone());
            tools.insert(
                exposed_name,
                ToolEntry {
                    server_name: tool.server_name.clone(),
                    original_name: tool.name.clone(),
                    schema: tool,
                    remote_peer_id: None,
                },
            );
        }

        // Add remote tools
        for (tool, peer_id) in remote_tools {
            let mut exposed_name = if name_count.get(&tool.name).copied().unwrap_or(0) > 1 {
                format!("{}__{}", tool.server_name, tool.name)
            } else {
                tool.name.clone()
            };

            // If this exposed name collides with an existing entry (local tool
            // from the same server, or another remote tool), disambiguate with
            // the peer's short ID.
            if tools.contains_key(&exposed_name) {
                let peer_short: String = peer_id
                    .iter()
                    .take(4)
                    .map(|b| format!("{:02x}", b))
                    .collect();
                exposed_name = format!("{}@{}", exposed_name, peer_short);
            }

            tool_order.push(exposed_name.clone());
            tools.insert(
                exposed_name,
                ToolEntry {
                    server_name: tool.server_name.clone(),
                    original_name: tool.name.clone(),
                    schema: tool,
                    remote_peer_id: Some(peer_id),
                },
            );
        }

        (tools, tool_order)
    }

    /// Run the MCP server on stdio. Blocks until stdin is closed.
    pub async fn run(&self) -> Result<(), McpServerError> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                break; // EOF
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse as JSON
            let value: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Failed to parse JSON: {}", e);
                    continue;
                }
            };

            // Check if it's a notification (no id) or a request (has id)
            if value.get("id").is_none() {
                if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
                    debug!("Received notification: {}", method);
                }
                continue;
            }

            // Parse as request
            let req: JsonRpcRequest = match serde_json::from_value(value) {
                Ok(r) => r,
                Err(e) => {
                    warn!("Failed to parse JSON-RPC request: {}", e);
                    continue;
                }
            };

            let response = self.handle_request(&req).await;
            let mut response_json = serde_json::to_string(&response)?;
            response_json.push('\n');
            stdout.write_all(response_json.as_bytes()).await?;
            stdout.flush().await?;
        }

        info!("MCP server stdin closed, shutting down");
        Ok(())
    }

    async fn handle_request(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        debug!("Handling request: {} (id={:?})", req.method, req.id);

        match req.method.as_str() {
            "initialize" => self.handle_initialize(req.id),
            "tools/list" => self.handle_tools_list(req.id).await,
            "tools/call" => self.handle_tools_call(req.id, req.params.as_ref()).await,
            method => {
                warn!("Unknown method: {}", method);
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32601,
                        message: format!("Method not found: {}", method),
                        data: None,
                    }),
                }
            }
        }
    }

    fn handle_initialize(&self, id: Option<u64>) -> JsonRpcResponse {
        let is_mesh = self.mesh.is_some();
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": is_mesh
                    }
                },
                "serverInfo": {
                    "name": if is_mesh { "axon-mcp-mesh-gateway" } else { "axon-mcp-gateway" },
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        }
    }

    async fn handle_tools_list(&self, id: Option<u64>) -> JsonRpcResponse {
        // In mesh mode, dynamically build the tool list from bridge + registry
        let (tool_map, order) = if self.mesh.is_some() {
            self.build_mesh_tool_map().await
        } else {
            (self.tools.clone(), self.tool_order.clone())
        };

        let tools: Vec<serde_json::Value> = order
            .iter()
            .filter_map(|name| {
                let entry = tool_map.get(name)?;
                let input_schema = entry
                    .schema
                    .parse_input_schema()
                    .unwrap_or_else(|_| json!({"type": "object"}));
                let mut tool_json = json!({
                    "name": name,
                    "description": entry.schema.description,
                    "inputSchema": input_schema
                });
                // Annotate remote tools so the AI agent knows they come from the mesh
                if entry.remote_peer_id.is_some() {
                    if let Some(obj) = tool_json.as_object_mut() {
                        obj.insert(
                            "annotations".to_string(),
                            json!({"source": "mesh", "server": entry.server_name}),
                        );
                    }
                }
                Some(tool_json)
            })
            .collect();

        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(json!({ "tools": tools })),
            error: None,
        }
    }

    async fn handle_tools_call(
        &self,
        id: Option<u64>,
        params: Option<&serde_json::Value>,
    ) -> JsonRpcResponse {
        let params = match params {
            Some(p) => p,
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Missing params".to_string(),
                        data: None,
                    }),
                };
            }
        };

        let tool_name = match params.get("name").and_then(|n| n.as_str()) {
            Some(n) => n,
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Missing tool name in params".to_string(),
                        data: None,
                    }),
                };
            }
        };

        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        // In mesh mode, build the dynamic tool map to resolve both local and remote
        let (resolved_map, _) = if self.mesh.is_some() {
            self.build_mesh_tool_map().await
        } else {
            (self.tools.clone(), self.tool_order.clone())
        };

        let entry = match resolved_map.get(tool_name) {
            Some(e) => e.clone(),
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: format!("Unknown tool: {}", tool_name),
                        data: None,
                    }),
                };
            }
        };

        // Route based on whether this is a local or remote tool
        if let Some(ref peer_id) = entry.remote_peer_id {
            // Remote tool — forward via mesh
            debug!(
                "Routing tools/call {} → remote peer {:02x?}...:{}:{}",
                tool_name,
                &peer_id[..4.min(peer_id.len())],
                entry.server_name,
                entry.original_name,
            );
            self.call_remote_tool(
                peer_id,
                &entry.server_name,
                &entry.original_name,
                arguments,
                id,
            )
            .await
        } else {
            // Local tool — route via bridge
            debug!(
                "Routing tools/call {} → local {}:{}",
                tool_name, entry.server_name, entry.original_name
            );
            match self
                .bridge
                .call_tool(&entry.server_name, &entry.original_name, arguments)
                .await
            {
                Ok(result) => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: Some(json!({
                        "content": [{
                            "type": "text",
                            "text": result.to_string()
                        }]
                    })),
                    error: None,
                },
                Err(e) => {
                    error!("Local tool call failed: {}", e);
                    JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: Some(json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Error: {}", e)
                            }],
                            "isError": true
                        })),
                        error: None,
                    }
                }
            }
        }
    }

    /// Forward a tool call to a remote mesh peer via QUIC.
    ///
    /// Sends a TaskRequest with the MCP capability (`mcp.<server>:<tool>:v1`)
    /// to the peer, waits for the TaskResponse, and translates it back to an
    /// MCP JSON-RPC response.
    async fn call_remote_tool(
        &self,
        peer_id: &[u8],
        server_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
        request_id: Option<u64>,
    ) -> JsonRpcResponse {
        let mesh = match &self.mesh {
            Some(m) => m,
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: Some(json!({
                        "content": [{"type": "text", "text": "Error: mesh not connected"}],
                        "isError": true
                    })),
                    error: None,
                };
            }
        };

        // Look up peer address from PeerTable
        let peer_addr = {
            let pt = mesh.peer_table.read().await;
            pt.get(peer_id).map(|p| p.addr.clone())
        };

        let peer_addr = match peer_addr {
            Some(a) => a,
            None => {
                error!("Remote peer not found in peer table");
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: Some(json!({
                        "content": [{"type": "text", "text": "Error: remote peer not found in mesh"}],
                        "isError": true
                    })),
                    error: None,
                };
            }
        };

        let addr: std::net::SocketAddr = match peer_addr.parse() {
            Ok(a) => a,
            Err(e) => {
                error!("Invalid peer address '{}': {}", peer_addr, e);
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: Some(json!({
                        "content": [{"type": "text", "text": format!("Error: invalid peer address: {}", e)}],
                        "isError": true
                    })),
                    error: None,
                };
            }
        };

        // Connect to the remote peer
        let conn = match mesh.transport.connect(addr).await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to connect to remote peer at {}: {}", addr, e);
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: Some(json!({
                        "content": [{"type": "text", "text": format!("Error: connection failed: {}", e)}],
                        "isError": true
                    })),
                    error: None,
                };
            }
        };

        // Build the TaskRequest with MCP capability
        let capability = Capability::new(format!("mcp.{}", server_name), tool_name, 1);
        let payload = match serde_json::to_vec(&arguments) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: Some(json!({
                        "content": [{"type": "text", "text": format!("Error: failed to serialize arguments: {}", e)}],
                        "isError": true
                    })),
                    error: None,
                };
            }
        };

        let task_request = TaskRequest {
            id: Uuid::new_v4(),
            capability,
            payload,
            timeout_ms: 30000,
        };

        info!(
            "Forwarding MCP tool call {}:{} to peer at {} (task {})",
            server_name, tool_name, addr, task_request.id,
        );

        // Send the request
        if let Err(e) = Transport::send(&conn, &Message::TaskRequest(task_request.clone())).await {
            error!("Failed to send task to peer at {}: {}", addr, e);
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request_id,
                result: Some(json!({
                    "content": [{"type": "text", "text": format!("Error: send failed: {}", e)}],
                    "isError": true
                })),
                error: None,
            };
        }

        // Wait for the response
        let response = match Transport::recv(&conn).await {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to receive response from peer at {}: {}", addr, e);
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: Some(json!({
                        "content": [{"type": "text", "text": format!("Error: recv failed: {}", e)}],
                        "isError": true
                    })),
                    error: None,
                };
            }
        };

        // Parse the TaskResponse
        match response {
            Message::TaskResponse(resp) => match resp.status {
                TaskStatus::Success => {
                    // Try to parse as JSON first, fall back to text
                    let result_text = if let Ok(json_val) =
                        serde_json::from_slice::<serde_json::Value>(&resp.payload)
                    {
                        json_val.to_string()
                    } else {
                        String::from_utf8_lossy(&resp.payload).to_string()
                    };
                    info!(
                        "Remote tool call {}:{} succeeded ({}ms)",
                        server_name, tool_name, resp.duration_ms,
                    );
                    JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request_id,
                        result: Some(json!({
                            "content": [{"type": "text", "text": result_text}]
                        })),
                        error: None,
                    }
                }
                TaskStatus::Error(e) => {
                    error!("Remote tool call failed: {}", e);
                    JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: request_id,
                        result: Some(json!({
                            "content": [{"type": "text", "text": format!("Error: {}", e)}],
                            "isError": true
                        })),
                        error: None,
                    }
                }
                TaskStatus::Timeout => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: Some(json!({
                        "content": [{"type": "text", "text": "Error: remote tool call timed out"}],
                        "isError": true
                    })),
                    error: None,
                },
                TaskStatus::NoCapability => JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: Some(json!({
                        "content": [{"type": "text", "text": "Error: remote peer has no matching capability"}],
                        "isError": true
                    })),
                    error: None,
                },
            },
            other => {
                error!("Unexpected response from remote peer: {:?}", other);
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: Some(json!({
                        "content": [{"type": "text", "text": "Error: unexpected response from remote peer"}],
                        "isError": true
                    })),
                    error: None,
                }
            }
        }
    }

    /// Get the number of locally exposed tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Get the list of locally exposed tool names.
    pub fn tool_names(&self) -> &[String] {
        &self.tool_order
    }

    /// Whether this server is connected to the mesh.
    pub fn is_mesh_connected(&self) -> bool {
        self.mesh.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_tools() -> Vec<McpToolSchema> {
        vec![
            McpToolSchema::new(
                "read_file",
                "Read a file from the filesystem",
                json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                "filesystem",
            ),
            McpToolSchema::new(
                "write_file",
                "Write content to a file",
                json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}),
                "filesystem",
            ),
            McpToolSchema::new(
                "search_repositories",
                "Search GitHub repositories",
                json!({"type": "object", "properties": {"query": {"type": "string"}}}),
                "github",
            ),
        ]
    }

    fn make_colliding_tools() -> Vec<McpToolSchema> {
        vec![
            McpToolSchema::new(
                "search",
                "Search the filesystem",
                json!({"type": "object"}),
                "filesystem",
            ),
            McpToolSchema::new(
                "search",
                "Search GitHub",
                json!({"type": "object"}),
                "github",
            ),
        ]
    }

    /// Helper: build a server directly from a tool list (bypassing bridge).
    fn build_server_from_tools(tools: Vec<McpToolSchema>) -> McpStdioServer {
        let (tool_map, tool_order) = McpStdioServer::build_tool_map(tools);
        McpStdioServer {
            bridge: Arc::new(McpBridge::new()),
            tools: tool_map,
            tool_order,
            mesh: None,
        }
    }

    #[test]
    fn tool_map_no_collisions() {
        let server = build_server_from_tools(make_test_tools());
        assert_eq!(server.tool_count(), 3);
        assert!(server.tools.contains_key("read_file"));
        assert!(server.tools.contains_key("write_file"));
        assert!(server.tools.contains_key("search_repositories"));
    }

    #[test]
    fn tool_map_with_collisions() {
        let server = build_server_from_tools(make_colliding_tools());
        assert_eq!(server.tool_count(), 2);
        assert!(server.tools.contains_key("filesystem__search"));
        assert!(server.tools.contains_key("github__search"));
        assert!(!server.tools.contains_key("search"));
    }

    #[test]
    fn handle_initialize() {
        let server = build_server_from_tools(make_test_tools());
        let resp = server.handle_initialize(Some(1));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "axon-mcp-gateway");
    }

    #[tokio::test]
    async fn handle_tools_list() {
        let server = build_server_from_tools(make_test_tools());
        let resp = server.handle_tools_list(Some(2)).await;
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"search_repositories"));
    }

    #[tokio::test]
    async fn handle_tools_list_with_collisions() {
        let server = build_server_from_tools(make_colliding_tools());
        let resp = server.handle_tools_list(Some(3)).await;
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"filesystem__search"));
        assert!(names.contains(&"github__search"));
    }

    #[tokio::test]
    async fn handle_tools_call_missing_params() {
        let server = build_server_from_tools(make_test_tools());
        let resp = server.handle_tools_call(Some(4), None).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[tokio::test]
    async fn handle_tools_call_missing_name() {
        let server = build_server_from_tools(make_test_tools());
        let params = json!({"arguments": {}});
        let resp = server.handle_tools_call(Some(5), Some(&params)).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[tokio::test]
    async fn handle_tools_call_unknown_tool() {
        let server = build_server_from_tools(make_test_tools());
        let params = json!({"name": "nonexistent_tool"});
        let resp = server.handle_tools_call(Some(6), Some(&params)).await;
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("nonexistent_tool"));
    }

    #[tokio::test]
    async fn handle_unknown_method() {
        let server = build_server_from_tools(make_test_tools());
        let req = JsonRpcRequest::new(99, "unknown/method", None);
        let resp = server.handle_request(&req).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn tool_names_preserves_order() {
        let server = build_server_from_tools(make_test_tools());
        let names = server.tool_names();
        assert_eq!(names.len(), 3);
        assert_eq!(names[0], "read_file");
        assert_eq!(names[1], "write_file");
        assert_eq!(names[2], "search_repositories");
    }

    #[test]
    fn collision_routing_preserves_original_name() {
        let server = build_server_from_tools(make_colliding_tools());
        let entry = server.tools.get("filesystem__search").unwrap();
        assert_eq!(entry.original_name, "search");
        assert_eq!(entry.server_name, "filesystem");

        let entry = server.tools.get("github__search").unwrap();
        assert_eq!(entry.original_name, "search");
        assert_eq!(entry.server_name, "github");
    }

    #[tokio::test]
    async fn tools_list_includes_input_schema() {
        let server = build_server_from_tools(make_test_tools());
        let resp = server.handle_tools_list(Some(7)).await;
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();

        let read_file = tools.iter().find(|t| t["name"] == "read_file").unwrap();
        assert!(read_file["inputSchema"]["properties"]["path"].is_object());
    }

    #[tokio::test]
    async fn empty_tool_list() {
        let server = build_server_from_tools(vec![]);
        assert_eq!(server.tool_count(), 0);
        let resp = server.handle_tools_list(Some(8)).await;
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(tools.is_empty());
    }

    #[test]
    fn local_server_is_not_mesh_connected() {
        let server = build_server_from_tools(make_test_tools());
        assert!(!server.is_mesh_connected());
    }

    #[test]
    fn tool_entry_remote_peer_none_for_local() {
        let server = build_server_from_tools(make_test_tools());
        for entry in server.tools.values() {
            assert!(entry.remote_peer_id.is_none());
        }
    }

    #[tokio::test]
    async fn mesh_tool_map_without_mesh_returns_local() {
        let server = build_server_from_tools(make_test_tools());
        let (map, order) = server.build_mesh_tool_map().await;
        assert_eq!(map.len(), 3);
        assert_eq!(order.len(), 3);
        assert!(map.contains_key("read_file"));
    }

    #[tokio::test]
    async fn mesh_tool_map_with_registry() {
        let bridge = Arc::new(McpBridge::new());
        let registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let local_peer_id = vec![0x01, 0x02, 0x03, 0x04];
        let peer_table = Arc::new(RwLock::new(PeerTable::new(crate::protocol::PeerInfo {
            peer_id: local_peer_id.clone(),
            addr: "127.0.0.1:4242".to_string(),
            capabilities: vec![],
            last_seen: 0,
        })));

        // Register remote tools in the registry
        let remote_peer_id = vec![0xAA, 0xBB, 0xCC, 0xDD];
        {
            let mut reg = registry.write().await;
            reg.register_peer_tools(
                &remote_peer_id,
                vec![
                    McpToolSchema::new(
                        "list_repos",
                        "List GitHub repositories",
                        json!({"type": "object"}),
                        "github",
                    ),
                    McpToolSchema::new(
                        "create_issue",
                        "Create a GitHub issue",
                        json!({"type": "object"}),
                        "github",
                    ),
                ],
            );
        }

        let identity = crate::identity::Identity::generate();
        let transport = Transport::bind("127.0.0.1:0".parse().unwrap(), &identity)
            .await
            .unwrap();
        let mesh = MeshContext {
            transport: Arc::new(transport),
            tool_registry: registry,
            peer_table,
            local_peer_id,
        };

        // Build server with no local tools but mesh context
        let (tool_map, tool_order) = McpStdioServer::build_tool_map(vec![]);
        let server = McpStdioServer {
            bridge,
            tools: tool_map,
            tool_order,
            mesh: Some(mesh),
        };

        let (map, order) = server.build_mesh_tool_map().await;
        assert_eq!(map.len(), 2);
        assert_eq!(order.len(), 2);
        assert!(map.contains_key("list_repos"));
        assert!(map.contains_key("create_issue"));

        // Verify remote tools have peer ID
        let entry = map.get("list_repos").unwrap();
        assert_eq!(entry.remote_peer_id, Some(vec![0xAA, 0xBB, 0xCC, 0xDD]));
        assert_eq!(entry.server_name, "github");
        assert_eq!(entry.original_name, "list_repos");
    }

    #[tokio::test]
    async fn mesh_tool_map_collision_between_remote_peers() {
        let bridge = Arc::new(McpBridge::new());
        let registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let local_peer_id = vec![0x01, 0x02, 0x03, 0x04];
        let peer_table = Arc::new(RwLock::new(PeerTable::new(crate::protocol::PeerInfo {
            peer_id: local_peer_id.clone(),
            addr: "127.0.0.1:4242".to_string(),
            capabilities: vec![],
            last_seen: 0,
        })));

        // Two remote peers with same tool name but different servers
        let remote_peer_a = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let remote_peer_b = vec![0xEE, 0xFF, 0x11, 0x22];
        {
            let mut reg = registry.write().await;
            reg.register_peer_tools(
                &remote_peer_a,
                vec![McpToolSchema::new(
                    "search",
                    "Search filesystem",
                    json!({"type": "object"}),
                    "filesystem",
                )],
            );
            reg.register_peer_tools(
                &remote_peer_b,
                vec![McpToolSchema::new(
                    "search",
                    "Search GitHub",
                    json!({"type": "object"}),
                    "github",
                )],
            );
        }

        let identity = crate::identity::Identity::generate();
        let transport = Transport::bind("127.0.0.1:0".parse().unwrap(), &identity)
            .await
            .unwrap();
        let mesh = MeshContext {
            transport: Arc::new(transport),
            tool_registry: registry,
            peer_table,
            local_peer_id: local_peer_id.clone(),
        };

        let (tool_map, tool_order) = McpStdioServer::build_tool_map(vec![]);
        let server = McpStdioServer {
            bridge,
            tools: tool_map,
            tool_order,
            mesh: Some(mesh),
        };

        let (map, order) = server.build_mesh_tool_map().await;
        // Both tools should exist with server prefixes due to name collision
        assert_eq!(map.len(), 2);
        assert_eq!(order.len(), 2);

        // Both should be remote tools
        assert!(map.values().all(|e| e.remote_peer_id.is_some()));
        // Both should have original_name "search"
        assert!(map.values().all(|e| e.original_name == "search"));

        // Should have server-prefixed names due to collision
        assert!(map.contains_key("filesystem__search") || map.contains_key("github__search"));
    }

    #[test]
    fn mesh_initialize_reports_mesh_gateway() {
        let (tool_map, tool_order) = McpStdioServer::build_tool_map(vec![]);
        let server = McpStdioServer {
            bridge: Arc::new(McpBridge::new()),
            tools: tool_map,
            tool_order,
            mesh: None,
        };
        let resp = server.handle_initialize(Some(1));
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "axon-mcp-gateway");
        assert_eq!(result["capabilities"]["tools"]["listChanged"], false);
    }

    #[tokio::test]
    async fn handle_tools_list_includes_annotations_for_remote() {
        let bridge = Arc::new(McpBridge::new());
        let registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let local_peer_id = vec![0x01, 0x02, 0x03, 0x04];
        let peer_table = Arc::new(RwLock::new(PeerTable::new(crate::protocol::PeerInfo {
            peer_id: local_peer_id.clone(),
            addr: "127.0.0.1:4242".to_string(),
            capabilities: vec![],
            last_seen: 0,
        })));

        let remote_peer_id = vec![0xAA, 0xBB, 0xCC, 0xDD];
        {
            let mut reg = registry.write().await;
            reg.register_peer_tools(
                &remote_peer_id,
                vec![McpToolSchema::new(
                    "deploy",
                    "Deploy application",
                    json!({"type": "object"}),
                    "ci",
                )],
            );
        }

        let identity = crate::identity::Identity::generate();
        let transport = Transport::bind("127.0.0.1:0".parse().unwrap(), &identity)
            .await
            .unwrap();
        let mesh = MeshContext {
            transport: Arc::new(transport),
            tool_registry: registry,
            peer_table,
            local_peer_id,
        };

        let (tool_map, tool_order) = McpStdioServer::build_tool_map(vec![]);
        let server = McpStdioServer {
            bridge,
            tools: tool_map,
            tool_order,
            mesh: Some(mesh),
        };

        let resp = server.handle_tools_list(Some(10)).await;
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);

        let deploy = &tools[0];
        assert_eq!(deploy["name"], "deploy");
        assert_eq!(deploy["annotations"]["source"], "mesh");
        assert_eq!(deploy["annotations"]["server"], "ci");
    }
}
