//! MCP Server — exposes axon's aggregated tools via the MCP protocol on stdio.
//!
//! This is the bridge between axon and the AI agent ecosystem. Any MCP-capable
//! client (Claude Code, Cursor, etc.) can connect to `axon serve-mcp` and get
//! access to tools from all connected MCP servers, with budget-constrained
//! tool selection to minimize context window overhead.
//!
//! ## Protocol
//!
//! Standard MCP over stdio (newline-delimited JSON-RPC 2.0):
//! - `initialize` → server capabilities and info
//! - `notifications/initialized` → no-op acknowledgment
//! - `tools/list` → aggregated tools from all MCP servers
//! - `tools/call` → routed to the correct MCP server

use crate::mcp::bridge::McpBridge;
use crate::mcp::jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::mcp::schema::McpToolSchema;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};

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

/// Tool entry mapping an exposed tool name to its MCP server and original schema.
#[derive(Debug, Clone)]
struct ToolEntry {
    /// The MCP server that owns this tool
    server_name: String,
    /// The original tool name (may differ from exposed name if prefixed)
    original_name: String,
    /// Full tool schema
    schema: McpToolSchema,
}

/// MCP server that aggregates tools from connected MCP servers and serves them
/// via the MCP protocol on stdio.
pub struct McpStdioServer {
    bridge: Arc<McpBridge>,
    /// Map from exposed tool name → tool entry
    tools: HashMap<String, ToolEntry>,
    /// Ordered list of exposed tool names (for deterministic tools/list)
    tool_order: Vec<String>,
}

impl McpStdioServer {
    /// Create a new MCP server from a connected bridge.
    ///
    /// Builds the tool map from all tools discovered by the bridge. If two
    /// servers expose tools with the same name, both are prefixed with their
    /// server name (e.g., `filesystem__read_file`).
    pub async fn new(bridge: Arc<McpBridge>) -> Self {
        let all_tools = bridge.all_tools().await;
        let (tools, tool_order) = Self::build_tool_map(all_tools);

        info!(
            "MCP server initialized with {} tools from {} servers",
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
        }
    }

    /// Build the tool map from a list of MCP tool schemas.
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
            "tools/list" => self.handle_tools_list(req.id),
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
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": "axon-mcp-gateway",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        }
    }

    fn handle_tools_list(&self, id: Option<u64>) -> JsonRpcResponse {
        let tools: Vec<serde_json::Value> = self
            .tool_order
            .iter()
            .filter_map(|name| {
                let entry = self.tools.get(name)?;
                let input_schema = entry
                    .schema
                    .parse_input_schema()
                    .unwrap_or_else(|_| json!({"type": "object"}));
                Some(json!({
                    "name": name,
                    "description": entry.schema.description,
                    "inputSchema": input_schema
                }))
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

        let entry = match self.tools.get(tool_name) {
            Some(e) => e,
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

        let server_name = entry.server_name.clone();
        let original_name = entry.original_name.clone();

        debug!(
            "Routing tools/call {} → {}:{}",
            tool_name, server_name, original_name
        );

        match self
            .bridge
            .call_tool(&server_name, &original_name, arguments)
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
                error!("Tool call failed: {}", e);
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

    /// Get the number of exposed tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Get the list of exposed tool names.
    pub fn tool_names(&self) -> &[String] {
        &self.tool_order
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

    #[test]
    fn handle_tools_list() {
        let server = build_server_from_tools(make_test_tools());
        let resp = server.handle_tools_list(Some(2));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"search_repositories"));
    }

    #[test]
    fn handle_tools_list_with_collisions() {
        let server = build_server_from_tools(make_colliding_tools());
        let resp = server.handle_tools_list(Some(3));
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

    #[test]
    fn tools_list_includes_input_schema() {
        let server = build_server_from_tools(make_test_tools());
        let resp = server.handle_tools_list(Some(7));
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();

        let read_file = tools.iter().find(|t| t["name"] == "read_file").unwrap();
        assert!(read_file["inputSchema"]["properties"]["path"].is_object());
    }

    #[test]
    fn empty_tool_list() {
        let server = build_server_from_tools(vec![]);
        assert_eq!(server.tool_count(), 0);
        let resp = server.handle_tools_list(Some(8));
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(tools.is_empty());
    }
}
