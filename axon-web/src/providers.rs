//! Shared LLM-provider primitives used by both the web API and the CLI.
//!
//! Holds provider metadata (default endpoints, default models, env var
//! mapping), API-key resolution, and a single `fetch_models` implementation so
//! the two crates can't silently drift.

use crate::api::provider_responses::{
    OllamaModelEntry, OllamaTagsResponse, OpenRouterModelsResponse, XaiModelsResponse,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ---------- Shared provider metadata ----------

/// Default model for a provider (string-form). Accepts `ollama`, `xai`,
/// `openrouter`; anything else falls back to `"default"`.
pub fn default_model(provider: &str) -> &'static str {
    match provider {
        "ollama" => "llama3",
        "xai" => "grok-2-latest",
        "openrouter" => "anthropic/claude-sonnet-4",
        _ => "default",
    }
}

/// Default endpoint for a provider. `custom` / unknown returns empty string.
pub fn default_endpoint(provider: &str) -> &'static str {
    match provider {
        "ollama" => "http://localhost:11434",
        "xai" => "https://api.x.ai/v1",
        "openrouter" => "https://openrouter.ai/api/v1",
        _ => "",
    }
}

/// Environment variable holding the API key for `provider`, if any.
pub fn env_var_for_provider(provider: &str) -> Option<&'static str> {
    match provider {
        "xai" => Some("XAI_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "custom" => Some("LLM_API_KEY"),
        _ => None,
    }
}

/// Resolve the API key, preferring an explicit value and falling back to the
/// provider-specific env var.
pub fn resolve_api_key(explicit: &str, provider: &str) -> String {
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    match env_var_for_provider(provider) {
        Some(var) => std::env::var(var).unwrap_or_default(),
        None => String::new(),
    }
}

// ---------- Uniform model descriptor ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub context_length: Option<u64>,
}

/// Errors raised while fetching the model list from a remote provider.
#[derive(Debug, thiserror::Error)]
pub enum FetchModelsError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Provider returned error: {0}")]
    Api(String),
    #[error("Invalid configuration: {0}")]
    Config(String),
}

/// Fetch the list of models the caller can choose from. Returns an empty list
/// for unknown providers.
pub async fn fetch_models(
    provider: &str,
    endpoint: &str,
    api_key: &str,
) -> Result<Vec<ModelInfo>, FetchModelsError> {
    match provider {
        "ollama" => fetch_ollama_models(endpoint).await,
        "openrouter" => fetch_openrouter_models(api_key).await,
        "xai" => fetch_xai_models(api_key).await,
        "custom" => Ok(vec![ModelInfo {
            id: "default".into(),
            name: "Custom Model".into(),
            description: "Specify model name manually".into(),
            context_length: None,
        }]),
        _ => Ok(vec![]),
    }
}

async fn fetch_ollama_models(endpoint: &str) -> Result<Vec<ModelInfo>, FetchModelsError> {
    let ep = if endpoint.is_empty() {
        default_endpoint("ollama").to_string()
    } else {
        endpoint.to_string()
    };
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/tags", ep))
        .timeout(Duration::from_secs(5))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(FetchModelsError::Api(format!(
            "Ollama API returned {}",
            resp.status()
        )));
    }
    let json: OllamaTagsResponse = resp.json().await?;
    Ok(json.models.into_iter().map(ollama_to_info).collect())
}

fn ollama_to_info(m: OllamaModelEntry) -> ModelInfo {
    let size = m
        .size
        .map(|s| format!("{:.1}GB", s as f64 / 1e9))
        .unwrap_or_default();
    let desc = [m.details.family, m.details.parameter_size, size]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    ModelInfo {
        id: m.name.clone(),
        name: m.name,
        description: desc,
        context_length: None,
    }
}

async fn fetch_openrouter_models(api_key: &str) -> Result<Vec<ModelInfo>, FetchModelsError> {
    let client = reqwest::Client::new();
    let mut req = client
        .get("https://openrouter.ai/api/v1/models")
        .timeout(Duration::from_secs(10));
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(FetchModelsError::Api(format!(
            "OpenRouter API returned {}",
            resp.status()
        )));
    }
    let json: OpenRouterModelsResponse = resp.json().await?;
    let mut models: Vec<ModelInfo> = json
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
            ModelInfo {
                id: m.id,
                name,
                description: desc,
                context_length: m.context_length,
            }
        })
        .collect();

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
    Ok(models)
}

async fn fetch_xai_models(api_key: &str) -> Result<Vec<ModelInfo>, FetchModelsError> {
    if api_key.is_empty() {
        return Err(FetchModelsError::Config(
            "xAI model discovery requires an API key".into(),
        ));
    }
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.x.ai/v1/models")
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(10))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(FetchModelsError::Api(format!(
            "xAI API returned {}",
            resp.status()
        )));
    }
    let json: XaiModelsResponse = resp.json().await?;
    Ok(json
        .data
        .into_iter()
        .map(|m| ModelInfo {
            name: m.id.clone(),
            id: m.id,
            description: String::new(),
            context_length: None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_round_trip() {
        assert_eq!(default_model("ollama"), "llama3");
        assert_eq!(default_model("xai"), "grok-2-latest");
        assert!(default_model("openrouter").contains('/'));
        assert_eq!(default_model("unknown"), "default");
    }

    #[test]
    fn default_endpoint_known() {
        assert!(default_endpoint("ollama").starts_with("http://"));
        assert!(default_endpoint("xai").starts_with("https://"));
        assert!(default_endpoint("openrouter").starts_with("https://"));
        assert_eq!(default_endpoint("custom"), "");
    }

    #[test]
    fn resolve_api_key_prefers_explicit() {
        assert_eq!(resolve_api_key("explicit", "xai"), "explicit");
    }

    #[test]
    fn env_var_mapping() {
        assert_eq!(env_var_for_provider("xai"), Some("XAI_API_KEY"));
        assert_eq!(
            env_var_for_provider("openrouter"),
            Some("OPENROUTER_API_KEY")
        );
        assert_eq!(env_var_for_provider("ollama"), None);
    }
}
