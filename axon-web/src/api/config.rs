use crate::state::SharedWebState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize)]
pub struct ConfigResponse {
    pub node: NodeConfigResponse,
    pub llm: LlmConfigResponse,
    pub mcp: McpConfigResponse,
}

#[derive(Serialize)]
pub struct NodeConfigResponse {
    pub listen: String,
    pub peers: Vec<String>,
    pub headless: bool,
    pub health_port: Option<u16>,
    pub web_port: Option<u16>,
}

#[derive(Serialize, Deserialize)]
pub struct LlmConfigResponse {
    pub provider: String,
    pub endpoint: String,
    /// Always masked in GET responses
    pub api_key: String,
    pub model: String,
}

#[derive(Serialize)]
pub struct McpConfigResponse {
    pub servers: Vec<McpServerResponse>,
}

#[derive(Serialize)]
pub struct McpServerResponse {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
}

fn config_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("axon").join("config.toml"))
}

pub async fn get_config(
    State(_state): State<Arc<SharedWebState>>,
) -> Result<Json<ConfigResponse>, StatusCode> {
    let path = config_path().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    if !path.exists() {
        // Return defaults
        return Ok(Json(ConfigResponse {
            node: NodeConfigResponse {
                listen: "0.0.0.0:4242".into(),
                peers: vec![],
                headless: false,
                health_port: None,
                web_port: None,
            },
            llm: LlmConfigResponse {
                provider: "ollama".into(),
                endpoint: "http://localhost:11434".into(),
                api_key: "".into(),
                model: "llama4-maverick".into(),
            },
            mcp: McpConfigResponse { servers: vec![] },
        }));
    }

    let contents =
        std::fs::read_to_string(&path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Parse as generic TOML to extract values
    let val: toml::Value =
        toml::from_str(&contents).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let node_table = val.get("node").and_then(|v| v.as_table());
    let llm_table = val.get("llm").and_then(|v| v.as_table());
    let mcp_table = val.get("mcp").and_then(|v| v.as_table());

    let listen = node_table
        .and_then(|t| t.get("listen"))
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0.0:4242")
        .to_string();

    let peers: Vec<String> = node_table
        .and_then(|t| t.get("peers"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let headless = node_table
        .and_then(|t| t.get("headless"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let health_port = node_table
        .and_then(|t| t.get("health_port"))
        .and_then(|v| v.as_integer())
        .map(|v| v as u16);

    let web_port = node_table
        .and_then(|t| t.get("web_port"))
        .and_then(|v| v.as_integer())
        .map(|v| v as u16);

    let provider = llm_table
        .and_then(|t| t.get("provider"))
        .and_then(|v| v.as_str())
        .unwrap_or("ollama")
        .to_string();

    let endpoint = llm_table
        .and_then(|t| t.get("endpoint"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let model = llm_table
        .and_then(|t| t.get("model"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Mask API key — never expose
    let has_key = llm_table
        .and_then(|t| t.get("api_key"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    let mcp_servers: Vec<McpServerResponse> = mcp_table
        .and_then(|t| t.get("servers"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let t = entry.as_table()?;
                    Some(McpServerResponse {
                        name: t.get("name")?.as_str()?.to_string(),
                        command: t.get("command")?.as_str()?.to_string(),
                        args: t
                            .get("args")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default(),
                        timeout_secs: t
                            .get("timeout_secs")
                            .and_then(|v| v.as_integer())
                            .unwrap_or(30) as u64,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Json(ConfigResponse {
        node: NodeConfigResponse {
            listen,
            peers,
            headless,
            health_port,
            web_port,
        },
        llm: LlmConfigResponse {
            provider,
            endpoint,
            api_key: if has_key {
                "***".to_string()
            } else {
                String::new()
            },
            model,
        },
        mcp: McpConfigResponse {
            servers: mcp_servers,
        },
    }))
}

#[derive(Deserialize)]
pub struct UpdateConfigRequest {
    pub node: Option<UpdateNodeConfig>,
    pub llm: Option<LlmConfigResponse>,
}

#[derive(Deserialize)]
pub struct UpdateNodeConfig {
    pub listen: Option<String>,
    pub peers: Option<Vec<String>>,
    pub headless: Option<bool>,
    pub health_port: Option<u16>,
    pub web_port: Option<u16>,
}

pub async fn put_config(
    State(_state): State<Arc<SharedWebState>>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let path = config_path().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    // Load existing
    let contents = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut doc: toml::Table =
        toml::from_str(&contents).unwrap_or_default();

    if let Some(node) = req.node {
        let node_table = doc
            .entry("node")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(listen) = node.listen {
            node_table.insert("listen".into(), toml::Value::String(listen));
        }
        if let Some(peers) = node.peers {
            node_table.insert(
                "peers".into(),
                toml::Value::Array(peers.into_iter().map(toml::Value::String).collect()),
            );
        }
        if let Some(headless) = node.headless {
            node_table.insert("headless".into(), toml::Value::Boolean(headless));
        }
        if let Some(hp) = node.health_port {
            node_table.insert("health_port".into(), toml::Value::Integer(hp as i64));
        }
        if let Some(wp) = node.web_port {
            node_table.insert("web_port".into(), toml::Value::Integer(wp as i64));
        }
    }

    if let Some(llm) = req.llm {
        let llm_table = doc
            .entry("llm")
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

        llm_table.insert("provider".into(), toml::Value::String(llm.provider));
        llm_table.insert("endpoint".into(), toml::Value::String(llm.endpoint));
        llm_table.insert("model".into(), toml::Value::String(llm.model));
        // Only update api_key if not masked
        if llm.api_key != "***" {
            llm_table.insert("api_key".into(), toml::Value::String(llm.api_key));
        }
    }

    let header = "# Axon node configuration\n\
                  # Managed by axon CLI — edit freely or re-run `axon setup`\n\
                  # API keys: prefer env vars (XAI_API_KEY, OPENROUTER_API_KEY)\n\n";
    let serialized = toml::to_string_pretty(&doc).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(&path, format!("{}{}", header, serialized))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn put_llm_config(
    State(state): State<Arc<SharedWebState>>,
    Json(llm): Json<LlmConfigResponse>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    put_config(
        State(state),
        Json(UpdateConfigRequest {
            node: None,
            llm: Some(llm),
        }),
    )
    .await
}
