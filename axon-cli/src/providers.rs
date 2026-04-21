use async_trait::async_trait;
use axon_web::api::provider_responses::{OllamaGenerateResponse, OpenAiChatResponse};
use axon_web::providers::{self as shared, FetchModelsError};
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

impl From<FetchModelsError> for ProviderError {
    fn from(e: FetchModelsError) -> Self {
        match e {
            FetchModelsError::Http(err) => ProviderError::Http(err),
            FetchModelsError::Api(msg) => ProviderError::Api(msg),
            FetchModelsError::Config(msg) => ProviderError::Config(msg),
        }
    }
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
pub struct CompletionResponse {
    pub text: String,
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

// Ollama Provider

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

        let json: OllamaGenerateResponse = resp.json().await?;

        Ok(CompletionResponse {
            text: json.response,
            usage: None,
        })
    }
}

// OpenAI-compatible provider used for xAI, OpenRouter, and custom endpoints.
// See https://openrouter.ai/models for the full OpenRouter model catalog.

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

        let json: OpenAiChatResponse = resp.json().await?;

        let text = json
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        let usage = json.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens as u32,
            completion_tokens: u.completion_tokens as u32,
        });

        Ok(CompletionResponse { text, usage })
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
    shared::resolve_api_key(explicit, &kind.to_string())
}

/// Return the default model for a provider.
pub fn default_model(kind: &ProviderKind) -> &'static str {
    shared::default_model(&kind.to_string())
}

/// Return the default endpoint for a provider.
pub fn default_endpoint(kind: &ProviderKind) -> &'static str {
    shared::default_endpoint(&kind.to_string())
}

// Model listing — re-export the shared type so callers still see `ModelInfo`.
pub use axon_web::providers::ModelInfo;

/// Fetch available models for a provider. Delegates to the shared
/// implementation in `axon_web::providers` so web + CLI share the exact
/// same list/parse logic.
pub async fn fetch_models(
    kind: &ProviderKind,
    endpoint: &str,
    api_key: &str,
) -> Result<Vec<ModelInfo>, ProviderError> {
    Ok(shared::fetch_models(&kind.to_string(), endpoint, api_key).await?)
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
        let p = build_provider(&ProviderKind::Xai, "", "", "grok-2");
        assert!(p.is_err());
    }

    #[test]
    fn build_openrouter_requires_key() {
        let p = build_provider(&ProviderKind::OpenRouter, "", "", "x-ai/grok-2");
        assert!(p.is_err());
    }

    #[test]
    fn build_with_valid_keys() {
        let p = build_provider(&ProviderKind::Xai, "", "xai-test", "grok-2");
        assert!(p.is_ok());
        let p = build_provider(
            &ProviderKind::OpenRouter,
            "",
            "or-test",
            "anthropic/claude-sonnet-4",
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
    fn xai_default_model_starts_with_grok() {
        assert!(default_model(&ProviderKind::Xai).starts_with("grok-"));
    }

    #[test]
    fn openrouter_default_model_is_namespaced() {
        // OpenRouter IDs are `<vendor>/<model>`; we don't pin a specific name
        // because the "current best" changes frequently.
        assert!(default_model(&ProviderKind::OpenRouter).contains('/'));
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
