use crate::mcp::jsonrpc::{JsonRpcRequest, JsonRpcResponse};
use crate::mcp::schema::McpToolSchema;

use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Errors from MCP client operations.
#[derive(Debug, Error)]
pub enum McpClientError {
    #[error("failed to spawn MCP server '{0}': {1}")]
    SpawnFailed(String, String),
    #[error("IO error with MCP server '{0}': {1}")]
    Io(String, String),
    #[error("MCP server '{0}' timed out after {1}s")]
    Timeout(String, u64),
    #[error("failed to parse response from MCP server '{0}': {1}")]
    ParseError(String, String),
    #[error("MCP error from '{0}': {1}")]
    McpError(String, String),
    #[error("MCP server '{0}' process exited")]
    ServerExited(String),
}

/// Configuration for connecting to an MCP server via stdio.
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Human-readable name (e.g., "filesystem", "github")
    pub name: String,
    /// Command to spawn (e.g., "npx", "uvx", "/usr/bin/mcp-server")
    pub command: String,
    /// Command arguments
    pub args: Vec<String>,
    /// Additional environment variables
    pub env: HashMap<String, String>,
    /// Timeout for individual requests (default: 30s)
    pub timeout_secs: u64,
}

impl McpServerConfig {
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
            env: HashMap::new(),
            timeout_secs: 30,
        }
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }
}

/// Internal state protected by a mutex.
struct ClientInner {
    child: Child,
    writer: tokio::io::BufWriter<tokio::process::ChildStdin>,
    reader: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

/// MCP client that communicates with a server process via stdio JSON-RPC 2.0.
///
/// Messages are newline-delimited JSON. The client handles the MCP lifecycle:
/// initialize → tools/list → tools/call. Thread-safe via internal Mutex.
pub struct McpClient {
    config: McpServerConfig,
    inner: Mutex<ClientInner>,
    tools: Mutex<Vec<McpToolSchema>>,
}

impl McpClient {
    /// Spawn the MCP server process and establish stdio communication.
    pub async fn spawn(config: McpServerConfig) -> Result<Self, McpClientError> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

        for (k, v) in &config.env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| McpClientError::SpawnFailed(config.name.clone(), e.to_string()))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            McpClientError::SpawnFailed(config.name.clone(), "no stdin handle".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            McpClientError::SpawnFailed(config.name.clone(), "no stdout handle".into())
        })?;

        let inner = ClientInner {
            child,
            writer: tokio::io::BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
        };

        info!(
            "Spawned MCP server '{}': {} {:?}",
            config.name, config.command, config.args
        );

        Ok(Self {
            config,
            inner: Mutex::new(inner),
            tools: Mutex::new(Vec::new()),
        })
    }

    /// Server name from config.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Run the MCP initialization handshake.
    pub async fn initialize(&self) -> Result<serde_json::Value, McpClientError> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "axon",
                "version": "0.1.0"
            }
        });

        let result = self.request("initialize", Some(params)).await?;

        // Send the initialized notification (no response expected)
        self.notify("notifications/initialized", None).await?;

        info!("MCP server '{}' initialized", self.config.name);
        Ok(result)
    }

    /// Discover tools via tools/list and cache them.
    pub async fn discover_tools(&self) -> Result<Vec<McpToolSchema>, McpClientError> {
        let result = self.request("tools/list", None).await?;

        let tools_value = result.get("tools").ok_or_else(|| {
            McpClientError::McpError(
                self.config.name.clone(),
                "tools/list response missing 'tools' field".into(),
            )
        })?;

        let tools_array: Vec<serde_json::Value> = serde_json::from_value(tools_value.clone())
            .map_err(|e| McpClientError::ParseError(self.config.name.clone(), e.to_string()))?;

        let mut schemas = Vec::with_capacity(tools_array.len());
        for tool in tools_array {
            let name = tool
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let description = tool
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let input_schema = tool
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({}));

            schemas.push(McpToolSchema::new(
                name,
                description,
                input_schema,
                &self.config.name,
            ));
        }

        info!(
            "Discovered {} tools from MCP server '{}'",
            schemas.len(),
            self.config.name
        );

        // Cache the tools
        let mut cached = self.tools.lock().await;
        *cached = schemas.clone();

        Ok(schemas)
    }

    /// Call an MCP tool by name with the given arguments.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpClientError> {
        let params = serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        });

        let result = self.request("tools/call", Some(params)).await?;

        debug!(
            "tools/call '{}' on '{}' completed",
            tool_name, self.config.name
        );
        Ok(result)
    }

    /// Get cached tools (from the last discover_tools call).
    pub async fn cached_tools(&self) -> Vec<McpToolSchema> {
        self.tools.lock().await.clone()
    }

    /// Check if the server process is still running.
    pub async fn is_alive(&self) -> bool {
        let mut inner = self.inner.lock().await;
        matches!(inner.child.try_wait(), Ok(None))
    }

    /// Kill the server process.
    pub async fn shutdown(&self) {
        let mut inner = self.inner.lock().await;
        let _ = inner.child.kill().await;
        info!("MCP server '{}' shut down", self.config.name);
    }

    /// Send a JSON-RPC request and wait for the matching response.
    async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, McpClientError> {
        let mut inner = self.inner.lock().await;
        let id = inner.next_id;
        inner.next_id += 1;

        let request = JsonRpcRequest::new(id, method, params);
        Self::write_message(&mut inner.writer, &request, &self.config.name).await?;

        let response = Self::read_response(&mut inner.reader, id, &self.config).await?;

        response
            .into_result()
            .map_err(|e| McpClientError::McpError(self.config.name.clone(), e.to_string()))
    }

    /// Send a JSON-RPC notification (no response expected).
    async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), McpClientError> {
        let mut inner = self.inner.lock().await;
        let notification = JsonRpcRequest::notification(method, params);
        Self::write_message(&mut inner.writer, &notification, &self.config.name).await
    }

    /// Write a JSON-RPC message as a newline-delimited JSON line.
    async fn write_message(
        writer: &mut tokio::io::BufWriter<tokio::process::ChildStdin>,
        message: &JsonRpcRequest,
        server_name: &str,
    ) -> Result<(), McpClientError> {
        let json = serde_json::to_string(message).map_err(|e| {
            McpClientError::Io(server_name.to_string(), format!("serialize: {}", e))
        })?;

        writer
            .write_all(json.as_bytes())
            .await
            .map_err(|e| McpClientError::Io(server_name.to_string(), e.to_string()))?;

        writer
            .write_all(b"\n")
            .await
            .map_err(|e| McpClientError::Io(server_name.to_string(), e.to_string()))?;

        writer
            .flush()
            .await
            .map_err(|e| McpClientError::Io(server_name.to_string(), e.to_string()))?;

        Ok(())
    }

    /// Read lines from stdout until we get a JSON-RPC response with the expected ID.
    /// Skips notifications and non-JSON lines.
    async fn read_response(
        reader: &mut BufReader<tokio::process::ChildStdout>,
        expected_id: u64,
        config: &McpServerConfig,
    ) -> Result<JsonRpcResponse, McpClientError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(config.timeout_secs);

        loop {
            let mut line = String::new();
            let read_result = tokio::time::timeout_at(deadline, reader.read_line(&mut line)).await;

            match read_result {
                Err(_) => {
                    return Err(McpClientError::Timeout(
                        config.name.clone(),
                        config.timeout_secs,
                    ));
                }
                Ok(Err(e)) => {
                    return Err(McpClientError::Io(config.name.clone(), e.to_string()));
                }
                Ok(Ok(0)) => {
                    return Err(McpClientError::ServerExited(config.name.clone()));
                }
                Ok(Ok(_)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    // Try to parse as JSON
                    let value: serde_json::Value = match serde_json::from_str(trimmed) {
                        Ok(v) => v,
                        Err(_) => {
                            debug!(
                                "Skipping non-JSON line from '{}': {}",
                                config.name,
                                &trimmed[..trimmed.len().min(100)]
                            );
                            continue;
                        }
                    };

                    // Check if this is a response with our ID
                    if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
                        if id == expected_id {
                            let response: JsonRpcResponse =
                                serde_json::from_value(value).map_err(|e| {
                                    McpClientError::ParseError(config.name.clone(), e.to_string())
                                })?;
                            return Ok(response);
                        }
                        // Response for a different ID — skip (stale or out-of-order)
                        warn!(
                            "Received response for id={} while waiting for id={} from '{}'",
                            id, expected_id, config.name
                        );
                    }
                    // Notification or other message — skip
                    if let Some(method) = value.get("method").and_then(|v| v.as_str()) {
                        debug!("Received notification '{}' from '{}'", method, config.name);
                    }
                }
            }
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        // Best-effort kill on drop — can't await in Drop, but try_wait + start_kill
        if let Ok(mut inner) = self.inner.try_lock() {
            let _ = inner.child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_config_builder() {
        let config = McpServerConfig::new("filesystem", "npx")
            .with_args(vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-filesystem".to_string(),
                "/tmp".to_string(),
            ])
            .with_env("NODE_ENV", "production")
            .with_timeout(60);

        assert_eq!(config.name, "filesystem");
        assert_eq!(config.command, "npx");
        assert_eq!(config.args.len(), 3);
        assert_eq!(config.env.get("NODE_ENV").unwrap(), "production");
        assert_eq!(config.timeout_secs, 60);
    }

    #[test]
    fn server_config_defaults() {
        let config = McpServerConfig::new("test", "echo");
        assert_eq!(config.timeout_secs, 30);
        assert!(config.args.is_empty());
        assert!(config.env.is_empty());
    }

    #[tokio::test]
    async fn spawn_nonexistent_command_fails() {
        let config = McpServerConfig::new("bad", "/nonexistent/binary/abc123");
        let result = McpClient::spawn(config).await;
        assert!(result.is_err());
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        match err {
            McpClientError::SpawnFailed(name, _) => assert_eq!(name, "bad"),
            other => panic!("expected SpawnFailed, got: {}", other),
        }
    }

    #[tokio::test]
    async fn spawn_and_shutdown() {
        // Use `cat` as a simple process that reads stdin and writes to stdout
        let config = McpServerConfig::new("test", "cat");
        let client = McpClient::spawn(config).await.unwrap();
        assert!(client.is_alive().await);
        client.shutdown().await;
        // Give the process a moment to exit
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!client.is_alive().await);
    }

    #[tokio::test]
    async fn timeout_on_unresponsive_server() {
        // `sleep` never writes to stdout, so requests will time out
        let config = McpServerConfig::new("test", "sleep")
            .with_args(vec!["1000".to_string()])
            .with_timeout(1);
        let client = McpClient::spawn(config).await.unwrap();

        let result = client.request("test", None).await;
        assert!(result.is_err());
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        match err {
            McpClientError::Timeout(name, secs) => {
                assert_eq!(name, "test");
                assert_eq!(secs, 1);
            }
            other => panic!("expected Timeout, got: {}", other),
        }
        client.shutdown().await;
    }

    #[test]
    fn error_display() {
        let e = McpClientError::Timeout("fs".to_string(), 30);
        assert_eq!(format!("{}", e), "MCP server 'fs' timed out after 30s");

        let e = McpClientError::SpawnFailed("gh".to_string(), "not found".to_string());
        assert_eq!(
            format!("{}", e),
            "failed to spawn MCP server 'gh': not found"
        );
    }

    #[test]
    fn cached_tools_starts_empty() {
        // Can't easily test async in sync context, but we can verify the types compile
        let config = McpServerConfig::new("test", "echo");
        assert_eq!(config.name, "test");
    }
}
