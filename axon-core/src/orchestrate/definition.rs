use std::path::Path;
use thiserror::Error;

use crate::protocol::Capability;

#[derive(Debug, Error)]
pub enum DefinitionError {
    #[error("TOML parse error: {0}")]
    ParseError(#[from] toml::de::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Missing required field: {0}")]
    MissingField(String),
}

/// A capability declared in an agent definition file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CapabilityDef {
    pub namespace: String,
    pub name: String,
    #[serde(default = "default_version")]
    pub version: u32,
}

fn default_version() -> u32 {
    1
}

/// File-based agent definition loaded from a TOML file.
///
/// Inspired by OpenClaw's SOUL.md/TOOLS.md approach but using structured
/// TOML so it's unambiguous and parses trivially in Rust.
///
/// Example:
/// ```toml
/// name = "code-reviewer"
/// description = "Reviews code changes and provides feedback"
///
/// [[capabilities]]
/// namespace = "code"
/// name = "review"
/// version = 1
///
/// tools = ["read_file", "search_code"]
/// permissions = ["mesh:send", "blackboard:read", "blackboard:write"]
/// heartbeat_interval_secs = 30
/// max_concurrent_tasks = 4
/// ```
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<CapabilityDef>,
    /// MCP tool names this agent can invoke.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Named hooks to attach (pre/post handle).
    #[serde(default)]
    pub hooks: Vec<String>,
    /// Permission strings granted to hooks (e.g. "blackboard:write", "mesh:send").
    #[serde(default)]
    pub permissions: Vec<String>,
    /// How often to send heartbeats (seconds). None = no automatic heartbeat.
    pub heartbeat_interval_secs: Option<u64>,
    /// Maximum concurrent in-flight tasks. None = unlimited.
    pub max_concurrent_tasks: Option<usize>,
}

impl AgentDefinition {
    /// Parse from a TOML string.
    pub fn from_toml(content: &str) -> Result<Self, DefinitionError> {
        let def: AgentDefinition = toml::from_str(content)?;
        if def.name.is_empty() {
            return Err(DefinitionError::MissingField("name".to_string()));
        }
        Ok(def)
    }

    /// Load from a TOML file on disk.
    pub fn from_file(path: &Path) -> Result<Self, DefinitionError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    /// Convert declared capabilities to axon-core `Capability` types.
    pub fn to_capabilities(&self) -> Vec<Capability> {
        self.capabilities
            .iter()
            .map(|c| Capability::new(c.namespace.clone(), c.name.clone(), c.version))
            .collect()
    }

    /// Check whether this agent has a given permission string.
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_minimal_definition() {
        let toml = r#"name = "minimal""#;
        let def = AgentDefinition::from_toml(toml).unwrap();
        assert_eq!(def.name, "minimal");
        assert!(def.description.is_empty());
        assert!(def.capabilities.is_empty());
        assert!(def.tools.is_empty());
        assert!(def.permissions.is_empty());
        assert!(def.heartbeat_interval_secs.is_none());
        assert!(def.max_concurrent_tasks.is_none());
    }

    #[test]
    fn parse_full_definition() {
        // Root-level fields must appear before [[capabilities]] array-of-tables
        let toml = r#"
name = "code-reviewer"
description = "Reviews code changes"
tools = ["read_file", "search_code"]
hooks = ["log_tasks"]
permissions = ["mesh:send", "blackboard:read", "blackboard:write"]
heartbeat_interval_secs = 30
max_concurrent_tasks = 4

[[capabilities]]
namespace = "code"
name = "review"
version = 1

[[capabilities]]
namespace = "code"
name = "explain"
version = 2
"#;
        let def = AgentDefinition::from_toml(toml).unwrap();
        assert_eq!(def.name, "code-reviewer");
        assert_eq!(def.description, "Reviews code changes");
        assert_eq!(def.capabilities.len(), 2);
        assert_eq!(def.capabilities[0].namespace, "code");
        assert_eq!(def.capabilities[0].name, "review");
        assert_eq!(def.capabilities[0].version, 1);
        assert_eq!(def.capabilities[1].version, 2);
        assert_eq!(def.tools, vec!["read_file", "search_code"]);
        assert_eq!(def.hooks, vec!["log_tasks"]);
        assert_eq!(def.permissions.len(), 3);
        assert_eq!(def.heartbeat_interval_secs, Some(30));
        assert_eq!(def.max_concurrent_tasks, Some(4));
    }

    #[test]
    fn parse_invalid_toml_returns_error() {
        let result = AgentDefinition::from_toml("not valid toml ][[]");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DefinitionError::ParseError(_)
        ));
    }

    #[test]
    fn missing_name_returns_error() {
        // When `name` is absent, toml/serde returns a ParseError (missing field).
        let toml = r#"description = "no name here""#;
        let result = AgentDefinition::from_toml(toml);
        assert!(result.is_err());
        // Either ParseError (field absent) or MissingField (field present but empty) is acceptable
        assert!(matches!(
            result.unwrap_err(),
            DefinitionError::ParseError(_) | DefinitionError::MissingField(_)
        ));
    }

    #[test]
    fn empty_name_returns_error() {
        let toml = r#"name = """#;
        let result = AgentDefinition::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn empty_capabilities_is_valid() {
        let toml = r#"name = "observer""#;
        let def = AgentDefinition::from_toml(toml).unwrap();
        assert!(def.capabilities.is_empty());
        assert!(def.to_capabilities().is_empty());
    }

    #[test]
    fn to_capabilities_converts_correctly() {
        let toml = r#"
name = "agent"
[[capabilities]]
namespace = "llm"
name = "chat"
version = 2
"#;
        let def = AgentDefinition::from_toml(toml).unwrap();
        let caps = def.to_capabilities();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].namespace, "llm");
        assert_eq!(caps[0].name, "chat");
        assert_eq!(caps[0].version, 2);
    }

    #[test]
    fn capability_default_version_is_one() {
        let toml = r#"
name = "agent"
[[capabilities]]
namespace = "test"
name = "ping"
"#;
        let def = AgentDefinition::from_toml(toml).unwrap();
        assert_eq!(def.capabilities[0].version, 1);
    }

    #[test]
    fn from_file_reads_toml() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, r#"name = "file-agent""#).unwrap();
        writeln!(f, r#"description = "loaded from file""#).unwrap();

        let def = AgentDefinition::from_file(f.path()).unwrap();
        assert_eq!(def.name, "file-agent");
        assert_eq!(def.description, "loaded from file");
    }

    #[test]
    fn from_file_missing_path_returns_error() {
        let result = AgentDefinition::from_file(Path::new("/nonexistent/path/agent.toml"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DefinitionError::Io(_)));
    }

    #[test]
    fn has_permission_returns_true_when_present() {
        let toml = r#"name = "a"
permissions = ["blackboard:write", "mesh:send"]"#;
        let def = AgentDefinition::from_toml(toml).unwrap();
        assert!(def.has_permission("blackboard:write"));
        assert!(def.has_permission("mesh:send"));
        assert!(!def.has_permission("blackboard:read"));
    }

    #[test]
    fn to_capabilities_preserves_all_entries() {
        let toml = r#"
name = "multi"
[[capabilities]]
namespace = "a"
name = "x"
version = 1
[[capabilities]]
namespace = "b"
name = "y"
version = 3
[[capabilities]]
namespace = "c"
name = "z"
version = 2
"#;
        let def = AgentDefinition::from_toml(toml).unwrap();
        let caps = def.to_capabilities();
        assert_eq!(caps.len(), 3);
        assert_eq!(caps[2].namespace, "c");
        assert_eq!(caps[2].version, 2);
    }
}
