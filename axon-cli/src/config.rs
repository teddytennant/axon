use serde::{Deserialize, Serialize};
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
}

impl Default for NodeSection {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            peers: Vec::new(),
            headless: false,
            health_port: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSection {
    /// Provider: ollama, openai, xai, openrouter, custom
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

/// Generate an example config file at the default path.
pub fn generate_example_config() -> anyhow::Result<PathBuf> {
    let path = config_path().ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let example = NodeConfig {
        node: NodeSection {
            listen: default_listen(),
            peers: Vec::new(),
            headless: false,
            health_port: None,
        },
        llm: LlmSection {
            provider: "ollama".to_string(),
            endpoint: "http://localhost:11434".to_string(),
            api_key: String::new(),
            model: "llama3.2".to_string(),
        },
    };

    let contents = toml::to_string_pretty(&example)?;
    let header = "# Axon node configuration\n\
                  # CLI flags override these values.\n\
                  # API keys: prefer env vars (OPENAI_API_KEY, XAI_API_KEY, etc.)\n\n";

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
provider = "openai"
endpoint = "https://api.openai.com/v1"
api_key = "sk-test"
model = "gpt-4o"
"#;
        let config: NodeConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.node.listen.to_string(), "127.0.0.1:5555");
        assert_eq!(config.node.peers.len(), 2);
        assert!(config.node.headless);
        assert_eq!(config.llm.provider, "openai");
        assert_eq!(config.llm.model, "gpt-4o");
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
}
