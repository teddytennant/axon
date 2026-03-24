use serde::{Deserialize, Serialize};

/// Level of detail when returning tool schemas.
///
/// Controls the token cost of tool discovery. An agent doing initial
/// discovery can request `Compact` (name + one-line description, ~15 tokens)
/// then fetch `Full` schemas only for the tools it actually needs.
///
/// Token savings are dramatic:
/// - 100 tools × Full  ≈ 20,000 tokens
/// - 100 tools × Compact ≈ 1,500 tokens  (13× reduction)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaDetail {
    /// Full schema: name, description, input_schema, server_name.
    #[default]
    Full = 0,
    /// Summary: name, description, parameter names (no types/descriptions).
    Summary = 1,
    /// Compact: name, first sentence of description only.
    Compact = 2,
}

impl SchemaDetail {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Summary,
            2 => Self::Compact,
            _ => Self::Full,
        }
    }
}

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
        crate::protocol::Capability::new(format!("mcp.{}", self.server_name), &self.name, 1)
    }

    /// Estimated token cost of including this tool's schema in a context window.
    /// Rough heuristic: ~4 chars per token for JSON.
    pub fn estimated_tokens(&self) -> usize {
        self.tokens_at_detail(SchemaDetail::Full)
    }

    /// Estimated token cost at a specific detail level.
    pub fn tokens_at_detail(&self, detail: SchemaDetail) -> usize {
        match detail {
            SchemaDetail::Full => {
                let total = self.input_schema.len() + self.description.len() + self.name.len() + 50;
                total / 4
            }
            SchemaDetail::Summary => {
                let param_names = self.extract_parameter_names();
                let params_len: usize = param_names.iter().map(|p| p.len() + 2).sum();
                let total = self.name.len() + self.description.len() + params_len + 30;
                total / 4
            }
            SchemaDetail::Compact => {
                let first_sentence = first_sentence_of(&self.description);
                let total = self.name.len() + first_sentence.len() + 10;
                total / 4
            }
        }
    }

    /// Extract parameter names from the JSON Schema input_schema.
    /// Returns an empty vec if the schema can't be parsed or has no properties.
    pub fn extract_parameter_names(&self) -> Vec<String> {
        let Ok(schema) = serde_json::from_str::<serde_json::Value>(&self.input_schema) else {
            return vec![];
        };
        let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
            return vec![];
        };
        props.keys().cloned().collect()
    }

    /// Produce a compact representation: name + first sentence of description.
    pub fn to_compact(&self) -> CompactToolSchema {
        CompactToolSchema {
            name: self.name.clone(),
            description: first_sentence_of(&self.description),
            server_name: self.server_name.clone(),
        }
    }

    /// Produce a summary representation: name + description + parameter names.
    pub fn to_summary(&self) -> SummaryToolSchema {
        SummaryToolSchema {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.extract_parameter_names(),
            server_name: self.server_name.clone(),
        }
    }
}

/// Compact tool representation for low-token discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactToolSchema {
    pub name: String,
    pub description: String,
    pub server_name: String,
}

/// Summary tool representation with parameter names but no full schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Vec<String>,
    pub server_name: String,
}

/// Extract the first sentence from a description string.
fn first_sentence_of(text: &str) -> String {
    // Find first sentence-ending punctuation followed by space or end
    for (i, c) in text.char_indices() {
        if c == '.' || c == '!' || c == '?' {
            let next_idx = i + c.len_utf8();
            if next_idx >= text.len() || text[next_idx..].starts_with(' ') {
                return text[..next_idx].to_string();
            }
        }
    }
    // No sentence boundary found — return whole text
    text.to_string()
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
    /// Maximum token budget. When set, results are capped so their
    /// combined token cost at the requested detail level doesn't exceed
    /// this budget. Tools are added greedily in relevance order.
    pub max_tokens: Option<usize>,
    /// Schema detail level for token estimation and response format.
    pub detail: SchemaDetail,
}

impl ToolFilter {
    pub fn new() -> Self {
        Self {
            query: None,
            server_filter: None,
            limit: 20,
            max_tokens: None,
            detail: SchemaDetail::Full,
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

    pub fn with_max_tokens(mut self, budget: usize) -> Self {
        self.max_tokens = Some(budget);
        self
    }

    pub fn with_detail(mut self, detail: SchemaDetail) -> Self {
        self.detail = detail;
        self
    }
}

/// Result of a budget-constrained tool search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSearchResult {
    /// Tools that fit within the budget, sorted by relevance.
    pub tools: Vec<ToolSearchResult>,
    /// Total tokens consumed by the returned tools.
    pub total_tokens: usize,
    /// Remaining budget (0 if no budget was set).
    pub budget_remaining: usize,
    /// Whether results were truncated due to token budget.
    pub truncated: bool,
    /// Total tools that matched the query (before budget truncation).
    pub total_matches: usize,
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

    // --- SchemaDetail tests ---

    #[test]
    fn schema_detail_default_is_full() {
        assert_eq!(SchemaDetail::default(), SchemaDetail::Full);
    }

    #[test]
    fn schema_detail_from_u8() {
        assert_eq!(SchemaDetail::from_u8(0), SchemaDetail::Full);
        assert_eq!(SchemaDetail::from_u8(1), SchemaDetail::Summary);
        assert_eq!(SchemaDetail::from_u8(2), SchemaDetail::Compact);
        assert_eq!(SchemaDetail::from_u8(255), SchemaDetail::Full); // unknown → Full
    }

    #[test]
    fn schema_detail_roundtrip_bincode() {
        for detail in [
            SchemaDetail::Full,
            SchemaDetail::Summary,
            SchemaDetail::Compact,
        ] {
            let bytes = bincode::serialize(&detail).unwrap();
            let decoded: SchemaDetail = bincode::deserialize(&bytes).unwrap();
            assert_eq!(decoded, detail);
        }
    }

    // --- Tiered schema methods ---

    #[test]
    fn to_compact_extracts_first_sentence() {
        let tool = sample_tool();
        let compact = tool.to_compact();
        assert_eq!(compact.name, "read_file");
        assert_eq!(compact.server_name, "filesystem");
        // Description has no period, so entire string is returned
        assert_eq!(
            compact.description,
            "Read the contents of a file at the given path"
        );
    }

    #[test]
    fn to_compact_multi_sentence() {
        let tool = McpToolSchema::new(
            "deploy",
            "Deploy the application to production. Requires admin access. Use with caution.",
            json!({"type": "object"}),
            "ops",
        );
        let compact = tool.to_compact();
        assert_eq!(compact.description, "Deploy the application to production.");
    }

    #[test]
    fn to_summary_extracts_parameter_names() {
        let tool = McpToolSchema::new(
            "write_file",
            "Write content to a file",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                    "encoding": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
            "filesystem",
        );
        let summary = tool.to_summary();
        assert_eq!(summary.name, "write_file");
        assert_eq!(summary.description, "Write content to a file");
        assert_eq!(summary.parameters.len(), 3);
        assert!(summary.parameters.contains(&"path".to_string()));
        assert!(summary.parameters.contains(&"content".to_string()));
        assert!(summary.parameters.contains(&"encoding".to_string()));
    }

    #[test]
    fn to_summary_empty_schema() {
        let tool = McpToolSchema::new("noop", "Does nothing", json!({}), "test");
        let summary = tool.to_summary();
        assert!(summary.parameters.is_empty());
    }

    #[test]
    fn extract_parameter_names_invalid_json() {
        let tool = McpToolSchema::from_raw("broken", "Broken schema", "not json", "test");
        assert!(tool.extract_parameter_names().is_empty());
    }

    // --- Token estimation at different detail levels ---

    #[test]
    fn tokens_at_detail_full_equals_estimated_tokens() {
        let tool = sample_tool();
        assert_eq!(
            tool.tokens_at_detail(SchemaDetail::Full),
            tool.estimated_tokens()
        );
    }

    #[test]
    fn tokens_at_detail_compact_less_than_full() {
        let tool = sample_tool();
        let full = tool.tokens_at_detail(SchemaDetail::Full);
        let compact = tool.tokens_at_detail(SchemaDetail::Compact);
        assert!(
            compact < full,
            "compact ({}) should be < full ({})",
            compact,
            full
        );
    }

    #[test]
    fn tokens_at_detail_summary_less_than_full() {
        let tool = sample_tool();
        let full = tool.tokens_at_detail(SchemaDetail::Full);
        let summary = tool.tokens_at_detail(SchemaDetail::Summary);
        assert!(
            summary < full,
            "summary ({}) should be < full ({})",
            summary,
            full
        );
    }

    #[test]
    fn tokens_at_detail_compact_less_than_summary() {
        let tool = McpToolSchema::new(
            "complex_tool",
            "A very complex tool that does many things. It has lots of parameters.",
            json!({
                "type": "object",
                "properties": {
                    "param1": {"type": "string", "description": "First param"},
                    "param2": {"type": "number", "description": "Second param"},
                    "param3": {"type": "boolean", "description": "Third param"},
                    "param4": {"type": "array", "description": "Fourth param"}
                }
            }),
            "complex_server",
        );
        let summary = tool.tokens_at_detail(SchemaDetail::Summary);
        let compact = tool.tokens_at_detail(SchemaDetail::Compact);
        assert!(
            compact < summary,
            "compact ({}) should be < summary ({})",
            compact,
            summary
        );
    }

    // --- ToolFilter budget builder ---

    #[test]
    fn tool_filter_with_max_tokens() {
        let filter = ToolFilter::new().with_max_tokens(5000);
        assert_eq!(filter.max_tokens, Some(5000));
    }

    #[test]
    fn tool_filter_with_detail() {
        let filter = ToolFilter::new().with_detail(SchemaDetail::Compact);
        assert_eq!(filter.detail, SchemaDetail::Compact);
    }

    #[test]
    fn tool_filter_default_has_no_budget() {
        let filter = ToolFilter::new();
        assert!(filter.max_tokens.is_none());
        assert_eq!(filter.detail, SchemaDetail::Full);
    }

    // --- first_sentence_of ---

    #[test]
    fn first_sentence_simple() {
        assert_eq!(first_sentence_of("Hello world."), "Hello world.");
    }

    #[test]
    fn first_sentence_multi() {
        assert_eq!(
            first_sentence_of("First sentence. Second sentence."),
            "First sentence."
        );
    }

    #[test]
    fn first_sentence_no_period() {
        assert_eq!(first_sentence_of("No period here"), "No period here");
    }

    #[test]
    fn first_sentence_exclamation() {
        assert_eq!(first_sentence_of("Wow! Amazing."), "Wow!");
    }

    #[test]
    fn first_sentence_abbreviation_with_space() {
        // Simple heuristic: "e.g." period IS followed by space, so it splits there.
        // This is a known limitation of the simple approach — acceptable for tool descriptions.
        assert_eq!(first_sentence_of("Use e.g. this tool"), "Use e.g.");
    }

    // --- Compact/Summary serialization ---

    #[test]
    fn compact_schema_serialization() {
        let compact = CompactToolSchema {
            name: "test".to_string(),
            description: "A test tool.".to_string(),
            server_name: "test_server".to_string(),
        };
        let json = serde_json::to_string(&compact).unwrap();
        let decoded: CompactToolSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.name, "test");
    }

    #[test]
    fn summary_schema_serialization() {
        let summary = SummaryToolSchema {
            name: "test".to_string(),
            description: "A test tool.".to_string(),
            parameters: vec!["param1".to_string(), "param2".to_string()],
            server_name: "test_server".to_string(),
        };
        let json = serde_json::to_string(&summary).unwrap();
        let decoded: SummaryToolSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.parameters.len(), 2);
    }
}
