use crate::api::provider_responses::{
    OllamaTagsResponse, OpenRouterModelsResponse, XaiModelsResponse,
};
use crate::state::SharedWebState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct ModelResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub context_length: Option<u64>,
}

pub async fn get_models(
    State(_state): State<Arc<SharedWebState>>,
    Path(provider): Path<String>,
) -> Result<Json<Vec<ModelResponse>>, StatusCode> {
    // Load config to get endpoint and API key
    let path = dirs::config_dir()
        .map(|d| d.join("axon").join("config.toml"))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let (endpoint, api_key) = if path.exists() {
        let contents =
            std::fs::read_to_string(&path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let val: toml::Value =
            toml::from_str(&contents).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let llm = val.get("llm").and_then(|v| v.as_table());
        let ep = llm
            .and_then(|t| t.get("endpoint"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let key = llm
            .and_then(|t| t.get("api_key"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        (ep, key)
    } else {
        (String::new(), String::new())
    };

    // Also check env vars
    let api_key = if api_key.is_empty() {
        match provider.as_str() {
            "xai" => std::env::var("XAI_API_KEY").unwrap_or_default(),
            "openrouter" => std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            _ => String::new(),
        }
    } else {
        api_key
    };

    let client = reqwest::Client::new();

    let models: Vec<ModelResponse> = match provider.as_str() {
        "ollama" => {
            let ep = if endpoint.is_empty() {
                "http://localhost:11434".to_string()
            } else {
                endpoint
            };
            let resp = client
                .get(format!("{}/api/tags", ep))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?;

            if !resp.status().is_success() {
                return Err(StatusCode::BAD_GATEWAY);
            }

            let json: OllamaTagsResponse =
                resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

            json.models
                .into_iter()
                .map(|m| {
                    let size = m
                        .size
                        .map(|s| format!("{:.1}GB", s as f64 / 1e9))
                        .unwrap_or_default();
                    let desc = [m.details.family, m.details.parameter_size, size]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    ModelResponse {
                        id: m.name.clone(),
                        name: m.name,
                        description: desc,
                        context_length: None,
                    }
                })
                .collect()
        }
        "openrouter" => {
            let mut req = client
                .get("https://openrouter.ai/api/v1/models")
                .timeout(std::time::Duration::from_secs(10));
            if !api_key.is_empty() {
                req = req.bearer_auth(&api_key);
            }
            let resp = req.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
            if !resp.status().is_success() {
                return Err(StatusCode::BAD_GATEWAY);
            }
            let json: OpenRouterModelsResponse =
                resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

            let mut models: Vec<ModelResponse> = json
                .data
                .into_iter()
                .map(|m| {
                    let name = m.name.unwrap_or_else(|| m.id.clone());
                    let desc = m
                        .description
                        .unwrap_or_default()
                        .chars()
                        .take(80)
                        .collect::<String>();
                    ModelResponse {
                        id: m.id,
                        name,
                        description: desc,
                        context_length: m.context_length,
                    }
                })
                .collect();

            // Sort popular providers first
            let popular = [
                "anthropic/",
                "openai/",
                "google/",
                "x-ai/",
                "meta-llama/",
                "deepseek/",
                "mistralai/",
            ];
            models.sort_by(|a, b| {
                let a_pop = popular.iter().any(|p| a.id.starts_with(p));
                let b_pop = popular.iter().any(|p| b.id.starts_with(p));
                b_pop.cmp(&a_pop).then(a.id.cmp(&b.id))
            });
            models
        }
        "xai" => {
            // xAI requires an API key to list models.
            if api_key.is_empty() {
                return Err(StatusCode::UNAUTHORIZED);
            }
            let resp = client
                .get("https://api.x.ai/v1/models")
                .bearer_auth(&api_key)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?;
            if !resp.status().is_success() {
                return Err(StatusCode::BAD_GATEWAY);
            }
            let json: XaiModelsResponse = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
            json.data
                .into_iter()
                .map(|m| ModelResponse {
                    name: m.id.clone(),
                    id: m.id,
                    description: String::new(),
                    context_length: None,
                })
                .collect()
        }
        _ => vec![],
    };

    Ok(Json(models))
}
