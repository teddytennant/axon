use crate::state::SharedWebState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct ValidateRequest {
    pub provider: String,
    pub api_key: String,
}

#[derive(Serialize)]
pub struct ValidateResponse {
    pub valid: bool,
    pub error: Option<String>,
}

pub async fn validate_key(
    State(_state): State<Arc<SharedWebState>>,
    Json(req): Json<ValidateRequest>,
) -> Json<ValidateResponse> {
    let client = reqwest::Client::new();
    let result = match req.provider.as_str() {
        "ollama" => Ok(true),
        "openrouter" => client
            .get("https://openrouter.ai/api/v1/models")
            .bearer_auth(&req.api_key)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map(|r| r.status().is_success()),
        "xai" => client
            .get("https://api.x.ai/v1/models")
            .bearer_auth(&req.api_key)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map(|r| r.status().is_success()),
        "custom" => Ok(!req.api_key.is_empty()),
        _ => Ok(false),
    };

    match result {
        Ok(valid) => Json(ValidateResponse { valid, error: None }),
        Err(e) => Json(ValidateResponse {
            valid: false,
            error: Some(e.to_string()),
        }),
    }
}

#[derive(Deserialize)]
pub struct SetKeyRequest {
    pub api_key: String,
}

pub async fn put_key(
    State(_state): State<Arc<SharedWebState>>,
    Path(provider): Path<String>,
    Json(req): Json<SetKeyRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let path = dirs::config_dir()
        .map(|d| d.join("axon").join("config.toml"))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    let contents = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut doc: toml::Table = toml::from_str(&contents).unwrap_or_default();

    let llm_table = doc
        .entry("llm")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    llm_table.insert("provider".into(), toml::Value::String(provider));
    llm_table.insert("api_key".into(), toml::Value::String(req.api_key));

    let header = "# Axon node configuration\n\
                  # Managed by axon CLI — edit freely or re-run `axon setup`\n\
                  # API keys: prefer env vars (XAI_API_KEY, OPENROUTER_API_KEY)\n\n";
    let serialized = toml::to_string_pretty(&doc).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(&path, format!("{}{}", header, serialized))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({"ok": true})))
}
