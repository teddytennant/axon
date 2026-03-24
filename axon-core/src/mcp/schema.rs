use serde::{Deserialize, Serialize};

/// An MCP tool schema — the standard definition of a tool exposed by an MCP server.
///
/// Maps directly to the MCP `tools/list` response format:
/// - `name`: unique tool identifier within its server
/// - `description`: human-readable explanation of what the tool does
/// - `input_schema`: JSON Schema string describing the tool's expected input
/// - `server_name`: which MCP server exposes this tool (e.g., "filesystem", "github")
///
/// The `input_schema` is stored as a JSON string rather than a parsed Value
/// for bincode wire compatibility (bincode doesn't support serde_json::Value).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpToolSchema {
    /// Tool name (e.g., "read_file", "search_code")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for the tool's input parameters (stored as JSON string)
    pub input_schema: String,
    /// The MCP server that exposes this tool
    pub server_name: String,
}

impl McpToolSchema {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        server_name: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: input_schema.to_string(),
            server_name: server_name.into(),
        }
    }

    /// Create from a raw JSON string (e.g., received from an MCP server).
    pub fn from_raw(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema_json: impl Into<String>,
        server_name: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: input_schema_json.into(),
            server_name: server_name.into(),
        }
    }

    /// Parse the input schema as a serde_json::Value.
    pub fn parse_input_schema(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_str(&self.input_schema)
    }

    /// Canonical capability tag for this tool: "mcp.<server>:<tool_name>:v1"
    pub fn capability_tag(&self) -> String {
        format!("mcp.{}:{}:v1", self.server_name, self.name)
    }

    /// Convert to an axon Capability for routing.
    pub fn to_capability(&self) -> crate::protocol::Capability {
        crate::protocol::Capability::new(
            format!("mcp.{}", self.server_name),
            &self.name,
            1,
        )
    }

    /// Estimated token cost of including this tool's schema in a context window.
    /// Rough heuristic: ~4 chars per token for JSON.
    pub fn estimated_tokens(&self) -> usize {
        let schema_len = self.input_schema.len();
        let desc_len = self.description.len();
        let name_len = self.name.len();
        (schema_len + desc_len + name_len + 50) / 4 // 50 for structural overhead
    }
}

/// Filter criteria for searching the tool registry.
#[derive(Debug, Clone, Default)]
pub struct ToolFilter {
    /// Free-text query matched against tool name and description
    pub query: Option<String>,
    /// Only return tools from this MCP server
    pub server_filter: Option<String>,
    /// Maximum number of results to return
    pub limit: usize,
}

impl ToolFilter {
    pub fn new() -> Self {
        Self {
            query: None,
            server_filter: None,
            limit: 20,
        }
    }

    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }

    pub fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server_filter = Some(server.into());
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// A search result from the tool registry, including relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSearchResult {
    /// The tool schema
    pub tool: McpToolSchema,
    /// Relevance score (0.0 to 1.0, higher = more relevant)
    pub score: f64,
    /// Which peer advertised this tool (peer_id as hex)
    pub peer_id_hex: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_tool() -> McpToolSchema {
        McpToolSchema::new(
            "read_file",
            "Read the contents of a file at the given path",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" }
                },
                "required": ["path"]
            }),
            "filesystem",
        )
    }

    #[test]
    fn schema_creation() {
        let tool = sample_tool();
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.server_name, "filesystem");
        assert!(tool.description.contains("file"));
    }

    #[test]
    fn capability_tag_format() {
        let tool = sample_tool();
        assert_eq!(tool.capability_tag(), "mcp.filesystem:read_file:v1");
    }

    #[test]
    fn to_capability_conversion() {
        let tool = sample_tool();
        let cap = tool.to_capability();
        assert_eq!(cap.namespace, "mcp.filesystem");
        assert_eq!(cap.name, "read_file");
        assert_eq!(cap.version, 1);
    }

    #[test]
    fn estimated_tokens_positive() {
        let tool = sample_tool();
        let tokens = tool.estimated_tokens();
        assert!(tokens > 0);
        assert!(tokens < 10000); // sanity check
    }

    #[test]
    fn schema_roundtrip_serde() {
        let tool = sample_tool();
        let json = serde_json::to_string(&tool).unwrap();
        let deserialized: McpToolSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(tool, deserialized);
    }

    #[test]
    fn schema_roundtrip_bincode() {
        let tool = sample_tool();
        let bytes = bincode::serialize(&tool).unwrap();
        let deserialized: McpToolSchema = bincode::deserialize(&bytes).unwrap();
        assert_eq!(tool, deserialized);
    }

    #[test]
    fn tool_filter_builder() {
        let filter = ToolFilter::new()
            .with_query("read file")
            .with_server("filesystem")
            .with_limit(5);
        assert_eq!(filter.query.as_deref(), Some("read file"));
        assert_eq!(filter.server_filter.as_deref(), Some("filesystem"));
        assert_eq!(filter.limit, 5);
    }

    #[test]
    fn tool_filter_defaults() {
        let filter = ToolFilter::new();
        assert!(filter.query.is_none());
        assert!(filter.server_filter.is_none());
        assert_eq!(filter.limit, 20);
    }

    #[test]
    fn search_result_serialization() {
        let result = ToolSearchResult {
            tool: sample_tool(),
            score: 0.85,
            peer_id_hex: "abcd1234".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ToolSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.score, 0.85);
        assert_eq!(deserialized.peer_id_hex, "abcd1234");
    }

    #[test]
    fn different_servers_different_tags() {
        let fs_tool = McpToolSchema::new("list", "List items", json!({}), "filesystem");
        let gh_tool = McpToolSchema::new("list", "List items", json!({}), "github");
        assert_ne!(fs_tool.capability_tag(), gh_tool.capability_tag());
    }

    #[test]
    fn empty_schema_valid() {
        let tool = McpToolSchema::new("noop", "Does nothing", json!({}), "test");
        assert!(tool.estimated_tokens() > 0);
    }

    #[test]
    fn parse_input_schema_roundtrip() {
        let original = json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        });
        let tool = McpToolSchema::new("test", "test", original.clone(), "test");
        let parsed = tool.parse_input_schema().unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn from_raw_preserves_json_string() {
        let schema_json = r#"{"type":"object"}"#;
        let tool = McpToolSchema::from_raw("test", "test", schema_json, "test");
        assert_eq!(tool.input_schema, schema_json);
    }
}
