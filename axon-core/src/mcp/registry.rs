use crate::mcp::schema::{McpToolSchema, ToolFilter, ToolSearchResult};
use std::collections::HashMap;
use tracing::info;

/// Tracks which peer offers a given tool.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ToolEntry {
    schema: McpToolSchema,
    peer_id: Vec<u8>,
    registered_at: u64,
}

/// Decentralized MCP tool registry.
///
/// Stores tool schemas advertised by mesh peers. Supports text-based
/// relevance search so agents can discover tools without loading all
/// schemas into their context window.
///
/// The registry is populated via gossip: when a peer advertises its
/// tool catalog, the registry indexes all tools and makes them
/// searchable by any node in the mesh.
pub struct ToolRegistry {
    /// All tools indexed by a composite key: "peer_hex:server:tool_name"
    tools: HashMap<String, ToolEntry>,
    /// Index: server_name -> list of tool keys
    server_index: HashMap<String, Vec<String>>,
    /// Index: peer_id hex -> list of tool keys
    peer_index: HashMap<String, Vec<String>>,
    /// Maximum tools per peer (prevents a single peer from flooding the registry)
    max_tools_per_peer: usize,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            server_index: HashMap::new(),
            peer_index: HashMap::new(),
            max_tools_per_peer: 500,
        }
    }

    pub fn with_max_tools_per_peer(mut self, max: usize) -> Self {
        self.max_tools_per_peer = max;
        self
    }

    /// Register tools advertised by a peer. Replaces any previously
    /// registered tools from the same peer.
    pub fn register_peer_tools(&mut self, peer_id: &[u8], tools: Vec<McpToolSchema>) {
        let peer_hex = hex_id(peer_id);
        let now = now_secs();

        // Remove old entries for this peer
        self.remove_peer(peer_id);

        // Enforce per-peer limit
        let tools = if tools.len() > self.max_tools_per_peer {
            info!(
                "Peer {} advertised {} tools, capping at {}",
                &peer_hex[..8.min(peer_hex.len())],
                tools.len(),
                self.max_tools_per_peer
            );
            tools.into_iter().take(self.max_tools_per_peer).collect()
        } else {
            tools
        };

        let tool_count = tools.len();
        let mut peer_keys = Vec::with_capacity(tool_count);

        for schema in tools {
            let key = format!("{}:{}:{}", peer_hex, schema.server_name, schema.name);

            self.server_index
                .entry(schema.server_name.clone())
                .or_default()
                .push(key.clone());

            peer_keys.push(key.clone());

            self.tools.insert(
                key,
                ToolEntry {
                    schema,
                    peer_id: peer_id.to_vec(),
                    registered_at: now,
                },
            );
        }

        self.peer_index.insert(peer_hex.clone(), peer_keys);

        info!(
            "Registered {} tools from peer {}",
            tool_count,
            &peer_hex[..8.min(peer_hex.len())]
        );
    }

    /// Remove all tools from a peer.
    pub fn remove_peer(&mut self, peer_id: &[u8]) {
        let peer_hex = hex_id(peer_id);

        if let Some(keys) = self.peer_index.remove(&peer_hex) {
            for key in &keys {
                if let Some(entry) = self.tools.remove(key) {
                    // Clean up server index
                    if let Some(server_keys) = self.server_index.get_mut(&entry.schema.server_name)
                    {
                        server_keys.retain(|k| k != key);
                        if server_keys.is_empty() {
                            self.server_index.remove(&entry.schema.server_name);
                        }
                    }
                }
            }
        }
    }

    /// Search for tools matching a filter. Returns results sorted by relevance score.
    pub fn search(&self, filter: &ToolFilter) -> Vec<ToolSearchResult> {
        let mut results: Vec<ToolSearchResult> = self
            .tools
            .values()
            .filter(|entry| {
                // Server filter
                if let Some(ref server) = filter.server_filter {
                    if entry.schema.server_name != *server {
                        return false;
                    }
                }
                true
            })
            .map(|entry| {
                let score = match &filter.query {
                    Some(query) => relevance_score(&entry.schema, query),
                    None => 1.0, // No query = all tools equally relevant
                };
                ToolSearchResult {
                    tool: entry.schema.clone(),
                    score,
                    peer_id_hex: hex_id(&entry.peer_id),
                }
            })
            .filter(|result| result.score > 0.0)
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Apply limit
        if filter.limit > 0 {
            results.truncate(filter.limit);
        }

        results
    }

    /// Get all tools from a specific peer.
    pub fn tools_for_peer(&self, peer_id: &[u8]) -> Vec<&McpToolSchema> {
        let peer_hex = hex_id(peer_id);
        self.peer_index
            .get(&peer_hex)
            .map(|keys| {
                keys.iter()
                    .filter_map(|k| self.tools.get(k).map(|e| &e.schema))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all tools from a specific MCP server (across all peers).
    pub fn tools_for_server(&self, server_name: &str) -> Vec<&McpToolSchema> {
        self.server_index
            .get(server_name)
            .map(|keys| {
                keys.iter()
                    .filter_map(|k| self.tools.get(k).map(|e| &e.schema))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Total number of tools in the registry.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Number of unique peers that have registered tools.
    pub fn peer_count(&self) -> usize {
        self.peer_index.len()
    }

    /// Number of unique MCP servers represented.
    pub fn server_count(&self) -> usize {
        self.server_index.len()
    }

    /// All unique server names in the registry.
    pub fn server_names(&self) -> Vec<String> {
        self.server_index.keys().cloned().collect()
    }

    /// Estimated total token cost of all tool schemas in the registry.
    pub fn total_estimated_tokens(&self) -> usize {
        self.tools
            .values()
            .map(|e| e.schema.estimated_tokens())
            .sum()
    }

    /// Get all tools as schemas (no peer info).
    pub fn all_tools(&self) -> Vec<&McpToolSchema> {
        self.tools.values().map(|e| &e.schema).collect()
    }

    /// Deduplicated tools — when multiple peers offer the same tool
    /// (same name + server), return only one copy.
    pub fn unique_tools(&self) -> Vec<&McpToolSchema> {
        let mut seen = std::collections::HashSet::new();
        self.tools
            .values()
            .filter(|e| {
                let key = format!("{}:{}", e.schema.server_name, e.schema.name);
                seen.insert(key)
            })
            .map(|e| &e.schema)
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute relevance score (0.0 to 1.0) for a tool against a query string.
///
/// Scoring strategy:
/// - Tokenize query into lowercase words
/// - Exact name match: 0.5 bonus
/// - Name contains query term: 0.3 per term
/// - Description contains query term: 0.2 per term
/// - Server name match: 0.1 bonus
/// - Normalize to [0.0, 1.0]
fn relevance_score(schema: &McpToolSchema, query: &str) -> f64 {
    let query_lower = query.to_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();

    if terms.is_empty() {
        return 1.0;
    }

    let name_lower = schema.name.to_lowercase();
    let desc_lower = schema.description.to_lowercase();
    let server_lower = schema.server_name.to_lowercase();

    let mut score = 0.0;
    let mut matched_terms = 0usize;

    // Exact name match is the strongest signal
    if name_lower == query_lower {
        score += 0.5;
    }

    for term in &terms {
        let mut term_matched = false;

        // Name contains term — strong signal
        if name_lower.contains(term) {
            score += 0.3;
            term_matched = true;
        }

        // Description contains term — medium signal
        if desc_lower.contains(term) {
            score += 0.2;
            term_matched = true;
        }

        // Server name contains term — weak signal
        if server_lower.contains(term) {
            score += 0.1;
            term_matched = true;
        }

        if term_matched {
            matched_terms += 1;
        }
    }

    // Coverage bonus: reward matching more query terms
    let coverage = matched_terms as f64 / terms.len() as f64;
    score *= 0.5 + 0.5 * coverage;

    // Normalize to [0.0, 1.0]
    score.min(1.0)
}

fn hex_id(id: &[u8]) -> String {
    id.iter().map(|b| format!("{:02x}", b)).collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fs_tools() -> Vec<McpToolSchema> {
        vec![
            McpToolSchema::new(
                "read_file",
                "Read the contents of a file at the given path",
                json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
                "filesystem",
            ),
            McpToolSchema::new(
                "write_file",
                "Write content to a file at the given path",
                json!({"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}, "required": ["path", "content"]}),
                "filesystem",
            ),
            McpToolSchema::new(
                "list_directory",
                "List files and directories in a given path",
                json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
                "filesystem",
            ),
        ]
    }

    fn gh_tools() -> Vec<McpToolSchema> {
        vec![
            McpToolSchema::new(
                "create_issue",
                "Create a new issue in a GitHub repository",
                json!({"type": "object", "properties": {"repo": {"type": "string"}, "title": {"type": "string"}}}),
                "github",
            ),
            McpToolSchema::new(
                "search_code",
                "Search for code across GitHub repositories",
                json!({"type": "object", "properties": {"query": {"type": "string"}}}),
                "github",
            ),
        ]
    }

    fn peer_a() -> Vec<u8> {
        vec![0xAA, 0xBB, 0xCC, 0x01]
    }

    fn peer_b() -> Vec<u8> {
        vec![0xAA, 0xBB, 0xCC, 0x02]
    }

    #[test]
    fn registry_starts_empty() {
        let reg = ToolRegistry::new();
        assert_eq!(reg.tool_count(), 0);
        assert_eq!(reg.peer_count(), 0);
        assert_eq!(reg.server_count(), 0);
    }

    #[test]
    fn register_and_count_tools() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        assert_eq!(reg.tool_count(), 3);
        assert_eq!(reg.peer_count(), 1);
        assert_eq!(reg.server_count(), 1);
    }

    #[test]
    fn register_multiple_peers() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());
        assert_eq!(reg.tool_count(), 5);
        assert_eq!(reg.peer_count(), 2);
        assert_eq!(reg.server_count(), 2);
    }

    #[test]
    fn re_register_replaces_tools() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        assert_eq!(reg.tool_count(), 3);

        // Re-register with different tools
        reg.register_peer_tools(&peer_a(), gh_tools());
        assert_eq!(reg.tool_count(), 2);
        assert_eq!(reg.peer_count(), 1);
        assert_eq!(reg.server_count(), 1); // only github now
    }

    #[test]
    fn remove_peer_cleans_up() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());
        assert_eq!(reg.tool_count(), 5);

        reg.remove_peer(&peer_a());
        assert_eq!(reg.tool_count(), 2);
        assert_eq!(reg.peer_count(), 1);
        assert_eq!(reg.server_count(), 1); // only github
    }

    #[test]
    fn remove_nonexistent_peer_is_noop() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.remove_peer(&[0xFF]); // doesn't exist
        assert_eq!(reg.tool_count(), 3);
    }

    #[test]
    fn tools_for_peer() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());

        let peer_a_tools = reg.tools_for_peer(&peer_a());
        assert_eq!(peer_a_tools.len(), 3);
        assert!(peer_a_tools.iter().all(|t| t.server_name == "filesystem"));

        let peer_b_tools = reg.tools_for_peer(&peer_b());
        assert_eq!(peer_b_tools.len(), 2);
        assert!(peer_b_tools.iter().all(|t| t.server_name == "github"));
    }

    #[test]
    fn tools_for_server() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());

        let fs = reg.tools_for_server("filesystem");
        assert_eq!(fs.len(), 3);

        let gh = reg.tools_for_server("github");
        assert_eq!(gh.len(), 2);

        let none = reg.tools_for_server("nonexistent");
        assert_eq!(none.len(), 0);
    }

    #[test]
    fn search_no_filter_returns_all() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());

        let results = reg.search(&ToolFilter::new().with_limit(100));
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn search_by_server() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());

        let results = reg.search(&ToolFilter::new().with_server("github").with_limit(100));
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.tool.server_name == "github"));
    }

    #[test]
    fn search_by_query_relevance() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());

        let results = reg.search(&ToolFilter::new().with_query("read file").with_limit(10));
        assert!(!results.is_empty());
        // read_file should be the top result
        assert_eq!(results[0].tool.name, "read_file");
        // Scores should be descending
        for window in results.windows(2) {
            assert!(window[0].score >= window[1].score);
        }
    }

    #[test]
    fn search_query_no_match() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());

        let results = reg.search(
            &ToolFilter::new()
                .with_query("quantum entanglement teleportation")
                .with_limit(10),
        );
        assert!(results.is_empty());
    }

    #[test]
    fn search_respects_limit() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());

        let results = reg.search(&ToolFilter::new().with_limit(2));
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_exact_name_match_scores_highest() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());

        let results = reg.search(&ToolFilter::new().with_query("read_file").with_limit(10));
        assert!(!results.is_empty());
        assert_eq!(results[0].tool.name, "read_file");
        // Exact match should score higher than partial matches
        if results.len() > 1 {
            assert!(results[0].score > results[1].score);
        }
    }

    #[test]
    fn search_description_match() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());

        // "issue" appears only in github create_issue description
        let results = reg.search(&ToolFilter::new().with_query("issue").with_limit(10));
        assert!(!results.is_empty());
        assert_eq!(results[0].tool.name, "create_issue");
    }

    #[test]
    fn max_tools_per_peer_enforced() {
        let mut reg = ToolRegistry::new().with_max_tools_per_peer(2);

        reg.register_peer_tools(&peer_a(), fs_tools()); // 3 tools, cap at 2
        assert_eq!(reg.tool_count(), 2);
    }

    #[test]
    fn unique_tools_deduplicates() {
        let mut reg = ToolRegistry::new();
        // Two peers offering the same filesystem tools
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), fs_tools());

        assert_eq!(reg.tool_count(), 6); // 3 * 2 peers
        assert_eq!(reg.unique_tools().len(), 3); // deduplicated
    }

    #[test]
    fn total_estimated_tokens() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        let tokens = reg.total_estimated_tokens();
        assert!(tokens > 0);
    }

    #[test]
    fn server_names_returned() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());

        let mut names = reg.server_names();
        names.sort();
        assert_eq!(names, vec!["filesystem", "github"]);
    }

    #[test]
    fn search_combined_server_and_query() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());
        reg.register_peer_tools(&peer_b(), gh_tools());

        let results = reg.search(
            &ToolFilter::new()
                .with_query("search")
                .with_server("github")
                .with_limit(10),
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].tool.name, "search_code");
    }

    #[test]
    fn relevance_score_empty_query() {
        let tool = McpToolSchema::new("test", "test tool", json!({}), "test");
        assert_eq!(relevance_score(&tool, ""), 1.0);
    }

    #[test]
    fn relevance_score_whitespace_query() {
        let tool = McpToolSchema::new("test", "test tool", json!({}), "test");
        assert_eq!(relevance_score(&tool, "   "), 1.0);
    }

    #[test]
    fn search_result_includes_peer_hex() {
        let mut reg = ToolRegistry::new();
        reg.register_peer_tools(&peer_a(), fs_tools());

        let results = reg.search(&ToolFilter::new().with_limit(1));
        assert!(!results.is_empty());
        assert_eq!(results[0].peer_id_hex, "aabbcc01");
    }
}
