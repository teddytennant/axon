use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Errors from LLM provider calls.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Provider returned error: {0}")]
    Api(String),
    #[error("Invalid configuration: {0}")]
    Config(String),
}

/// A completion request sent to any provider.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub prompt: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

/// A completion response from any provider.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CompletionResponse {
    pub text: String,
    pub model: String,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// Trait that all LLM providers implement.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError>;
}

/// Supported provider backends.
///
/// Use `Ollama` for local models, `Xai` for direct xAI API access,
/// `OpenRouter` for access to every model (Anthropic, OpenAI, Gemini, Mistral,
/// DeepSeek, etc.) via a single API key, or `Custom` for any OpenAI-compatible
/// endpoint.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderKind {
    Ollama,
    Xai,
    OpenRouter,
    Custom,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderKind::Ollama => write!(f, "ollama"),
            ProviderKind::Xai => write!(f, "xai"),
            ProviderKind::OpenRouter => write!(f, "openrouter"),
            ProviderKind::Custom => write!(f, "custom"),
        }
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ollama" => Ok(ProviderKind::Ollama),
            "xai" | "grok" => Ok(ProviderKind::Xai),
            "openrouter" => Ok(ProviderKind::OpenRouter),
            "custom" => Ok(ProviderKind::Custom),
            _ => Err(format!(
                "Unknown provider: {}. Options: ollama, xai, openrouter, custom. \
                 Use openrouter for Anthropic, OpenAI, Gemini, Mistral, DeepSeek, and 200+ other models.",
                s
            )),
        }
    }
}

// --- Ollama Provider ---

pub struct OllamaProvider {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(endpoint: String, model: String) -> Self {
        Self {
            endpoint,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let body = serde_json::json!({
            "model": self.model,
            "prompt": req.prompt,
            "stream": false,
        });

        let resp = self
            .client
            .post(format!("{}/api/generate", self.endpoint))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("Ollama {} : {}", status, text)));
        }

        let json: serde_json::Value = resp.json().await?;
        let text = json["response"].as_str().unwrap_or("").to_string();

        Ok(CompletionResponse {
            text,
            model: self.model.clone(),
            usage: None,
        })
    }
}

// --- OpenAI-compatible Provider (used by xAI, OpenRouter, and custom endpoints) ---
//
// OpenRouter is the recommended way to access models from every provider:
//   Anthropic:  openrouter model "anthropic/claude-sonnet-4-6"
//   OpenAI:     openrouter model "openai/gpt-5.4"
//   Gemini:     openrouter model "google/gemini-3.1-pro"
//   Mistral:    openrouter model "mistralai/mistral-large-latest"
//   DeepSeek:   openrouter model "deepseek/deepseek-chat"
//   Groq:       openrouter model "groq/llama-4-scout"
//   Cohere:     openrouter model "cohere/command-a"
//   Perplexity: openrouter model "perplexity/sonar-pro"
//   Meta:       openrouter model "meta-llama/llama-4-maverick"
//   ... and 200+ more at https://openrouter.ai/models

pub struct OpenAiCompatibleProvider {
    label: String,
    endpoint: String,
    api_key: String,
    model: String,
    extra_headers: Vec<(String, String)>,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        label: impl Into<String>,
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            model: model.into(),
            extra_headers: Vec::new(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((key.into(), value.into()));
        self
    }

    pub fn xai(api_key: String, model: String) -> Self {
        Self::new("xai", "https://api.x.ai/v1", api_key, model)
    }

    pub fn openrouter(api_key: String, model: String) -> Self {
        Self::new("openrouter", "https://openrouter.ai/api/v1", api_key, model)
            .with_header("X-Title", "axon-mesh")
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        &self.label
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": req.prompt,
        })];

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
        });

        if let Some(max_tokens) = req.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(temp) = req.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        let mut request = self
            .client
            .post(format!("{}/chat/completions", self.endpoint))
            .bearer_auth(&self.api_key)
            .json(&body);

        for (key, value) in &self.extra_headers {
            request = request.header(key.as_str(), value.as_str());
        }

        let resp = request.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!(
                "{} {} : {}",
                self.label, status, text
            )));
        }

        let json: serde_json::Value = resp.json().await?;

        let text = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let usage = json.get("usage").map(|u| Usage {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
        });

        Ok(CompletionResponse {
            text,
            model: self.model.clone(),
            usage,
        })
    }
}

/// Build a provider from CLI arguments.
pub fn build_provider(
    kind: &ProviderKind,
    endpoint: &str,
    api_key: &str,
    model: &str,
) -> Result<Box<dyn LlmProvider>, ProviderError> {
    match kind {
        ProviderKind::Ollama => Ok(Box::new(OllamaProvider::new(
            endpoint.to_string(),
            model.to_string(),
        ))),
        ProviderKind::Xai => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "xAI requires --api-key or XAI_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::xai(
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::OpenRouter => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "OpenRouter requires --api-key or OPENROUTER_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::openrouter(
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::Custom => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "Custom provider requires --api-key".into(),
                ));
            }
            if endpoint.is_empty() {
                return Err(ProviderError::Config(
                    "Custom provider requires --llm-endpoint".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::new(
                "custom",
                endpoint.to_string(),
                api_key.to_string(),
                model.to_string(),
            )))
        }
    }
}

/// Resolve the API key: explicit flag > env var > empty.
pub fn resolve_api_key(explicit: &str, kind: &ProviderKind) -> String {
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    let env_var = match kind {
        ProviderKind::Xai => "XAI_API_KEY",
        ProviderKind::OpenRouter => "OPENROUTER_API_KEY",
        ProviderKind::Custom => "LLM_API_KEY",
        ProviderKind::Ollama => return String::new(),
    };
    std::env::var(env_var).unwrap_or_default()
}

/// Return the default model for a provider.
pub fn default_model(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Ollama => "llama4-maverick",
        ProviderKind::Xai => "grok-4.20",
        ProviderKind::OpenRouter => "x-ai/grok-4.20-beta",
        ProviderKind::Custom => "default",
    }
}

/// Return the default endpoint for a provider.
pub fn default_endpoint(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Ollama => "http://localhost:11434",
        ProviderKind::Xai => "https://api.x.ai/v1",
        ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
        ProviderKind::Custom => "",
    }
}

// ---------------------------------------------------------------------------
// Model listing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub context_length: Option<u64>,
}

/// Fetch available models for a provider.
pub async fn fetch_models(
    kind: &ProviderKind,
    endpoint: &str,
    api_key: &str,
) -> Result<Vec<ModelInfo>, ProviderError> {
    match kind {
        ProviderKind::Ollama => fetch_ollama_models(endpoint).await,
        ProviderKind::OpenRouter => fetch_openrouter_models(api_key).await,
        ProviderKind::Xai => Ok(xai_models()),
        ProviderKind::Custom => Ok(vec![ModelInfo {
            id: "default".into(),
            name: "Custom Model".into(),
            description: "Specify model name manually".into(),
            context_length: None,
        }]),
    }
}

async fn fetch_ollama_models(endpoint: &str) -> Result<Vec<ModelInfo>, ProviderError> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/tags", endpoint))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(ProviderError::Api(format!(
            "Ollama API returned {}",
            resp.status()
        )));
    }

    let json: serde_json::Value = resp.json().await?;
    let models = json["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| {
                    let name = m["name"].as_str().unwrap_or("unknown").to_string();
                    let size = m["size"]
                        .as_u64()
                        .map(|s| format!("{:.1}GB", s as f64 / 1e9))
                        .unwrap_or_default();
                    let family = m["details"]["family"].as_str().unwrap_or("").to_string();
                    let params = m["details"]["parameter_size"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    let desc = [family, params, size]
                        .iter()
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    ModelInfo {
                        id: name.clone(),
                        name,
                        description: desc,
                        context_length: None,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}

async fn fetch_openrouter_models(api_key: &str) -> Result<Vec<ModelInfo>, ProviderError> {
    let client = reqwest::Client::new();
    let mut req = client
        .get("https://openrouter.ai/api/v1/models")
        .timeout(std::time::Duration::from_secs(10));
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().await?;

    if !resp.status().is_success() {
        return Err(ProviderError::Api(format!(
            "OpenRouter API returned {}",
            resp.status()
        )));
    }

    let json: serde_json::Value = resp.json().await?;
    let mut models: Vec<ModelInfo> = json["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| {
                    let id = m["id"].as_str().unwrap_or("").to_string();
                    let name = m["name"].as_str().unwrap_or(&id).to_string();
                    let ctx = m["context_length"].as_u64();
                    let desc = m["description"]
                        .as_str()
                        .unwrap_or("")
                        .chars()
                        .take(80)
                        .collect::<String>();
                    ModelInfo {
                        id,
                        name,
                        description: desc,
                        context_length: ctx,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // Sort: popular models first
    let popular_prefixes = [
        "anthropic/",
        "openai/",
        "google/",
        "x-ai/",
        "meta-llama/",
        "deepseek/",
        "mistralai/",
    ];
    models.sort_by(|a, b| {
        let a_pop = popular_prefixes.iter().any(|p| a.id.starts_with(p));
        let b_pop = popular_prefixes.iter().any(|p| b.id.starts_with(p));
        b_pop.cmp(&a_pop).then(a.id.cmp(&b.id))
    });

    Ok(models)
}

fn xai_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "grok-4.20".into(),
            name: "Grok 4.20".into(),
            description: "Latest flagship model".into(),
            context_length: Some(131072),
        },
        ModelInfo {
            id: "grok-4.20-mini".into(),
            name: "Grok 4.20 Mini".into(),
            description: "Smaller, faster model".into(),
            context_length: Some(131072),
        },
        ModelInfo {
            id: "grok-3-beta".into(),
            name: "Grok 3 Beta".into(),
            description: "Previous generation".into(),
            context_length: Some(131072),
        },
    ]
}

/// Validate an API key by making a lightweight request.
#[allow(dead_code)]
pub async fn validate_api_key(kind: &ProviderKind, api_key: &str) -> Result<bool, ProviderError> {
    match kind {
        ProviderKind::Ollama => Ok(true), // no key needed
        ProviderKind::OpenRouter => {
            let client = reqwest::Client::new();
            let resp = client
                .get("https://openrouter.ai/api/v1/models")
                .bearer_auth(api_key)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await?;
            Ok(resp.status().is_success())
        }
        ProviderKind::Xai => {
            let client = reqwest::Client::new();
            let resp = client
                .get("https://api.x.ai/v1/models")
                .bearer_auth(api_key)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await?;
            Ok(resp.status().is_success())
        }
        ProviderKind::Custom => Ok(!api_key.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_provider_kinds() -> Vec<ProviderKind> {
        vec![
            ProviderKind::Ollama,
            ProviderKind::Xai,
            ProviderKind::OpenRouter,
            ProviderKind::Custom,
        ]
    }

    #[test]
    fn provider_kind_from_str() {
        assert_eq!(
            "ollama".parse::<ProviderKind>().unwrap(),
            ProviderKind::Ollama
        );
        assert_eq!("xai".parse::<ProviderKind>().unwrap(), ProviderKind::Xai);
        assert_eq!("grok".parse::<ProviderKind>().unwrap(), ProviderKind::Xai);
        assert_eq!(
            "openrouter".parse::<ProviderKind>().unwrap(),
            ProviderKind::OpenRouter
        );
        assert_eq!(
            "custom".parse::<ProviderKind>().unwrap(),
            ProviderKind::Custom
        );
        assert!("invalid".parse::<ProviderKind>().is_err());
    }

    #[test]
    fn provider_kind_from_str_case_insensitive() {
        assert_eq!("XAI".parse::<ProviderKind>().unwrap(), ProviderKind::Xai);
        assert_eq!("Grok".parse::<ProviderKind>().unwrap(), ProviderKind::Xai);
        assert_eq!(
            "OPENROUTER".parse::<ProviderKind>().unwrap(),
            ProviderKind::OpenRouter
        );
    }

    #[test]
    fn provider_kind_display() {
        assert_eq!(ProviderKind::Ollama.to_string(), "ollama");
        assert_eq!(ProviderKind::Xai.to_string(), "xai");
        assert_eq!(ProviderKind::OpenRouter.to_string(), "openrouter");
        assert_eq!(ProviderKind::Custom.to_string(), "custom");
    }

    #[test]
    fn provider_kind_display_roundtrip() {
        for kind in all_provider_kinds() {
            let s = kind.to_string();
            let parsed: ProviderKind = s.parse().unwrap();
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn default_models_not_empty() {
        for kind in all_provider_kinds() {
            assert!(
                !default_model(&kind).is_empty(),
                "default_model is empty for {:?}",
                kind
            );
        }
    }

    #[test]
    fn default_endpoints_not_empty_except_custom() {
        for kind in all_provider_kinds() {
            if kind == ProviderKind::Custom {
                assert_eq!(default_endpoint(&kind), "");
            } else {
                assert!(
                    !default_endpoint(&kind).is_empty(),
                    "default_endpoint is empty for {:?}",
                    kind
                );
            }
        }
    }

    #[test]
    fn build_ollama_no_key_needed() {
        let p = build_provider(
            &ProviderKind::Ollama,
            "http://localhost:11434",
            "",
            "llama4-maverick",
        );
        assert!(p.is_ok());
    }

    #[test]
    fn build_xai_requires_key() {
        let p = build_provider(&ProviderKind::Xai, "", "", "grok-4.20");
        assert!(p.is_err());
    }

    #[test]
    fn build_openrouter_requires_key() {
        let p = build_provider(&ProviderKind::OpenRouter, "", "", "x-ai/grok-4.20-beta");
        assert!(p.is_err());
    }

    #[test]
    fn build_with_valid_keys() {
        let p = build_provider(&ProviderKind::Xai, "", "xai-test", "grok-4.20");
        assert!(p.is_ok());
        let p = build_provider(
            &ProviderKind::OpenRouter,
            "",
            "or-test",
            "anthropic/claude-sonnet-4-6",
        );
        assert!(p.is_ok());
    }

    #[test]
    fn build_custom_requires_key_and_endpoint() {
        let p = build_provider(&ProviderKind::Custom, "", "", "model");
        assert!(p.is_err());
        let p = build_provider(&ProviderKind::Custom, "http://localhost:8080", "", "model");
        assert!(p.is_err());
        let p = build_provider(&ProviderKind::Custom, "", "key", "model");
        assert!(p.is_err());
        let p = build_provider(
            &ProviderKind::Custom,
            "http://localhost:8080",
            "key",
            "model",
        );
        assert!(p.is_ok());
    }

    #[test]
    fn xai_default_model_is_latest() {
        assert_eq!(default_model(&ProviderKind::Xai), "grok-4.20");
    }

    #[test]
    fn openrouter_default_model_is_latest() {
        assert_eq!(
            default_model(&ProviderKind::OpenRouter),
            "x-ai/grok-4.20-beta"
        );
    }

    #[test]
    fn error_message_suggests_openrouter() {
        let err = "anthropic".parse::<ProviderKind>().unwrap_err();
        assert!(
            err.contains("openrouter"),
            "Error should suggest OpenRouter for unknown providers"
        );
    }
}
