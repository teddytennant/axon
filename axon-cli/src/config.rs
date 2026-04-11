use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Configuration file for an axon node.
///
/// Loaded from `~/.config/axon/config.toml` (or `$XDG_CONFIG_HOME/axon/config.toml`).
/// CLI flags override values from the config file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    #[serde(default)]
    pub node: NodeSection,
    #[serde(default)]
    pub llm: LlmSection,
    #[serde(default)]
    pub mcp: McpSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSection {
    /// Address to listen on (default: 0.0.0.0:4242)
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    /// Bootstrap peers to connect to on startup
    #[serde(default)]
    pub peers: Vec<SocketAddr>,
    /// Run without TUI dashboard
    #[serde(default)]
    pub headless: bool,
    /// TCP port for health check endpoint (disabled if unset)
    #[serde(default)]
    pub health_port: Option<u16>,
    /// TCP port for the web UI dashboard (disabled if unset)
    #[serde(default)]
    pub web_port: Option<u16>,
}

impl Default for NodeSection {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            peers: Vec::new(),
            headless: false,
            health_port: None,
            web_port: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSection {
    /// Provider: ollama, xai, openrouter, custom
    /// Use openrouter for Anthropic, OpenAI, Gemini, Mistral, DeepSeek, etc.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// LLM endpoint URL (defaults per provider)
    #[serde(default)]
    pub endpoint: String,
    /// API key (prefer env vars for secrets)
    #[serde(default)]
    pub api_key: String,
    /// Model name (defaults per provider)
    #[serde(default)]
    pub model: String,
}

impl Default for LlmSection {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            endpoint: String::new(),
            api_key: String::new(),
            model: String::new(),
        }
    }
}

/// MCP server configuration section.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpSection {
    /// List of MCP servers to connect to on startup.
    #[serde(default)]
    pub servers: Vec<McpServerEntry>,
}

/// Configuration for a single MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    /// Human-readable name (e.g., "filesystem", "github")
    pub name: String,
    /// Command to spawn (e.g., "npx", "uvx")
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Request timeout in seconds (default: 30)
    #[serde(default = "default_mcp_timeout")]
    pub timeout_secs: u64,
}

fn default_mcp_timeout() -> u64 {
    30
}

impl McpServerEntry {
    /// Convert to the core McpServerConfig type.
    pub fn to_server_config(&self) -> axon_core::McpServerConfig {
        let mut config = axon_core::McpServerConfig::new(&self.name, &self.command)
            .with_args(self.args.clone())
            .with_timeout(self.timeout_secs);
        for (k, v) in &self.env {
            config = config.with_env(k, v);
        }
        config
    }
}

fn default_listen() -> SocketAddr {
    "0.0.0.0:4242".parse().unwrap()
}

fn default_provider() -> String {
    "ollama".to_string()
}

/// Return the default config file path: `~/.config/axon/config.toml`
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("axon").join("config.toml"))
}

/// Load config from the default path. Returns `NodeConfig::default()` if
/// the file doesn't exist.
pub fn load_config() -> NodeConfig {
    let path = match config_path() {
        Some(p) => p,
        None => return NodeConfig::default(),
    };

    if !path.exists() {
        return NodeConfig::default();
    }

    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => {
                tracing::info!("Loaded config from {}", path.display());
                config
            }
            Err(e) => {
                tracing::warn!("Failed to parse {}: {}. Using defaults.", path.display(), e);
                NodeConfig::default()
            }
        },
        Err(e) => {
            tracing::warn!("Failed to read {}: {}. Using defaults.", path.display(), e);
            NodeConfig::default()
        }
    }
}

/// Save a config to the default path.
pub fn save_config(config: &NodeConfig) -> anyhow::Result<PathBuf> {
    let path =
        config_path().ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let contents = toml::to_string_pretty(config)?;
    let header = "# Axon node configuration\n\
                  # Managed by axon CLI — edit freely or re-run `axon setup`\n\
                  # API keys: prefer env vars (XAI_API_KEY, OPENROUTER_API_KEY)\n\n";

    std::fs::write(&path, format!("{}{}", header, contents))?;
    Ok(path)
}

/// Returns true if a config file exists at the default path.
pub fn config_exists() -> bool {
    config_path().map(|p| p.exists()).unwrap_or(false)
}

/// Generate an example config file at the default path.
pub fn generate_example_config() -> anyhow::Result<PathBuf> {
    let path =
        config_path().ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let example = NodeConfig {
        node: NodeSection {
            listen: default_listen(),
            peers: Vec::new(),
            headless: false,
            health_port: None,
            web_port: None,
        },
        llm: LlmSection {
            provider: "ollama".to_string(),
            endpoint: "http://localhost:11434".to_string(),
            api_key: String::new(),
            model: "llama3.2".to_string(),
        },
        mcp: McpSection {
            servers: Vec::new(),
        },
    };

    let contents = toml::to_string_pretty(&example)?;
    let header = "# Axon node configuration\n\
                  # CLI flags override these values.\n\
                  # API keys: prefer env vars (XAI_API_KEY, OPENROUTER_API_KEY)\n\
                  # Use openrouter for access to Anthropic, OpenAI, Gemini, Mistral, DeepSeek, etc.\n\
                  #\n\
                  # MCP server example:\n\
                  # [[mcp.servers]]\n\
                  # name = \"filesystem\"\n\
                  # command = \"npx\"\n\
                  # args = [\"-y\", \"@modelcontextprotocol/server-filesystem\", \"/tmp\"]\n\
                  # timeout_secs = 30\n\n";

    std::fs::write(&path, format!("{}{}", header, contents))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let config = NodeConfig::default();
        assert_eq!(config.node.listen.to_string(), "0.0.0.0:4242");
        assert!(config.node.peers.is_empty());
        assert!(!config.node.headless);
        assert_eq!(config.llm.provider, "ollama");
    }

    #[test]
    fn parse_full_config() {
        let toml = r#"
[node]
listen = "127.0.0.1:5555"
peers = ["10.0.0.1:4242", "10.0.0.2:4242"]
headless = true

[llm]
provider = "xai"
endpoint = "https://api.x.ai/v1"
api_key = "xai-test"
model = "grok-4.20"
"#;
        let config: NodeConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.node.listen.to_string(), "127.0.0.1:5555");
        assert_eq!(config.node.peers.len(), 2);
        assert!(config.node.headless);
        assert_eq!(config.llm.provider, "xai");
        assert_eq!(config.llm.model, "grok-4.20");
    }

    #[test]
    fn parse_minimal_config() {
        let toml = "[node]\nlisten = \"0.0.0.0:9999\"\n";
        let config: NodeConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.node.listen.port(), 9999);
        assert_eq!(config.llm.provider, "ollama"); // default
    }

    #[test]
    fn parse_empty_config() {
        let toml = "";
        let config: NodeConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.node.listen.to_string(), "0.0.0.0:4242");
    }

    #[test]
    fn serialize_roundtrip() {
        let config = NodeConfig::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: NodeConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config.node.listen, deserialized.node.listen);
        assert_eq!(config.llm.provider, deserialized.llm.provider);
    }

    #[test]
    fn parse_mcp_servers() {
        let toml = r#"
[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

[[mcp.servers]]
name = "github"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
timeout_secs = 60

[mcp.servers.env]
GITHUB_TOKEN = "ghp_test123"
"#;
        let config: NodeConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.mcp.servers.len(), 2);
        assert_eq!(config.mcp.servers[0].name, "filesystem");
        assert_eq!(config.mcp.servers[0].command, "npx");
        assert_eq!(config.mcp.servers[0].args.len(), 3);
        assert_eq!(config.mcp.servers[0].timeout_secs, 30); // default
        assert_eq!(config.mcp.servers[1].name, "github");
        assert_eq!(config.mcp.servers[1].timeout_secs, 60);
        assert_eq!(
            config.mcp.servers[1].env.get("GITHUB_TOKEN").unwrap(),
            "ghp_test123"
        );
    }

    #[test]
    fn mcp_server_to_core_config() {
        let entry = McpServerEntry {
            name: "test".to_string(),
            command: "/usr/bin/test-server".to_string(),
            args: vec!["--flag".to_string()],
            env: HashMap::from([("KEY".to_string(), "val".to_string())]),
            timeout_secs: 45,
        };
        let config = entry.to_server_config();
        assert_eq!(config.name, "test");
        assert_eq!(config.command, "/usr/bin/test-server");
        assert_eq!(config.args, vec!["--flag"]);
        assert_eq!(config.timeout_secs, 45);
        assert_eq!(config.env.get("KEY").unwrap(), "val");
    }

    #[test]
    fn empty_mcp_section() {
        let toml = "[node]\nlisten = \"0.0.0.0:4242\"\n";
        let config: NodeConfig = toml::from_str(toml).unwrap();
        assert!(config.mcp.servers.is_empty());
    }
}
