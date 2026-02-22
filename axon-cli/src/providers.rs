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
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderKind {
    Ollama,
    OpenAi,
    Xai,
    OpenRouter,
    Custom,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderKind::Ollama => write!(f, "ollama"),
            ProviderKind::OpenAi => write!(f, "openai"),
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
            "openai" => Ok(ProviderKind::OpenAi),
            "xai" | "grok" => Ok(ProviderKind::Xai),
            "openrouter" => Ok(ProviderKind::OpenRouter),
            "custom" => Ok(ProviderKind::Custom),
            _ => Err(format!("Unknown provider: {}. Options: ollama, openai, xai, openrouter, custom", s)),
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

        let resp = self.client
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

// --- OpenAI-compatible Provider (also used by XAI and OpenRouter) ---

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

    pub fn openai(api_key: String, model: String) -> Self {
        Self::new("openai", "https://api.openai.com/v1", api_key, model)
    }

    pub fn xai(api_key: String, model: String) -> Self {
        Self::new("xai", "https://api.x.ai/v1", api_key, model)
    }

    pub fn openrouter(api_key: String, model: String) -> Self {
        Self::new(
            "openrouter",
            "https://openrouter.ai/api/v1",
            api_key,
            model,
        )
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

        let mut request = self.client
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
            return Err(ProviderError::Api(format!("{} {} : {}", self.label, status, text)));
        }

        let json: serde_json::Value = resp.json().await?;

        let text = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let usage = if let Some(u) = json.get("usage") {
            Some(Usage {
                prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
            })
        } else {
            None
        };

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
        ProviderKind::Ollama => {
            Ok(Box::new(OllamaProvider::new(endpoint.to_string(), model.to_string())))
        }
        ProviderKind::OpenAi => {
            if api_key.is_empty() {
                return Err(ProviderError::Config("OpenAI requires --api-key or OPENAI_API_KEY".into()));
            }
            Ok(Box::new(OpenAiCompatibleProvider::openai(api_key.to_string(), model.to_string())))
        }
        ProviderKind::Xai => {
            if api_key.is_empty() {
                return Err(ProviderError::Config("XAI requires --api-key or XAI_API_KEY".into()));
            }
            Ok(Box::new(OpenAiCompatibleProvider::xai(api_key.to_string(), model.to_string())))
        }
        ProviderKind::OpenRouter => {
            if api_key.is_empty() {
                return Err(ProviderError::Config("OpenRouter requires --api-key or OPENROUTER_API_KEY".into()));
            }
            Ok(Box::new(OpenAiCompatibleProvider::openrouter(api_key.to_string(), model.to_string())))
        }
        ProviderKind::Custom => {
            if api_key.is_empty() {
                return Err(ProviderError::Config("Custom provider requires --api-key".into()));
            }
            if endpoint.is_empty() {
                return Err(ProviderError::Config("Custom provider requires --llm-endpoint".into()));
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
        ProviderKind::OpenAi => "OPENAI_API_KEY",
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
        ProviderKind::Ollama => "llama3.2",
        ProviderKind::OpenAi => "gpt-4o-mini",
        ProviderKind::Xai => "grok-3-mini",
        ProviderKind::OpenRouter => "meta-llama/llama-3.1-8b-instruct",
        ProviderKind::Custom => "default",
    }
}

/// Return the default endpoint for a provider.
pub fn default_endpoint(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Ollama => "http://localhost:11434",
        ProviderKind::OpenAi => "https://api.openai.com/v1",
        ProviderKind::Xai => "https://api.x.ai/v1",
        ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
        ProviderKind::Custom => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_from_str() {
        assert_eq!("ollama".parse::<ProviderKind>().unwrap(), ProviderKind::Ollama);
        assert_eq!("openai".parse::<ProviderKind>().unwrap(), ProviderKind::OpenAi);
        assert_eq!("xai".parse::<ProviderKind>().unwrap(), ProviderKind::Xai);
        assert_eq!("grok".parse::<ProviderKind>().unwrap(), ProviderKind::Xai);
        assert_eq!("openrouter".parse::<ProviderKind>().unwrap(), ProviderKind::OpenRouter);
        assert_eq!("custom".parse::<ProviderKind>().unwrap(), ProviderKind::Custom);
        assert!("invalid".parse::<ProviderKind>().is_err());
    }

    #[test]
    fn provider_kind_display() {
        assert_eq!(ProviderKind::Ollama.to_string(), "ollama");
        assert_eq!(ProviderKind::Xai.to_string(), "xai");
        assert_eq!(ProviderKind::OpenRouter.to_string(), "openrouter");
    }

    #[test]
    fn default_models_not_empty() {
        for kind in &[
            ProviderKind::Ollama,
            ProviderKind::OpenAi,
            ProviderKind::Xai,
            ProviderKind::OpenRouter,
        ] {
            assert!(!default_model(kind).is_empty());
        }
    }

    #[test]
    fn default_endpoints_not_empty() {
        for kind in &[
            ProviderKind::Ollama,
            ProviderKind::OpenAi,
            ProviderKind::Xai,
            ProviderKind::OpenRouter,
        ] {
            assert!(!default_endpoint(kind).is_empty());
        }
    }

    #[test]
    fn build_ollama_no_key_needed() {
        let p = build_provider(&ProviderKind::Ollama, "http://localhost:11434", "", "llama3.2");
        assert!(p.is_ok());
    }

    #[test]
    fn build_openai_requires_key() {
        let p = build_provider(&ProviderKind::OpenAi, "", "", "gpt-4o-mini");
        assert!(p.is_err());
    }

    #[test]
    fn build_xai_requires_key() {
        let p = build_provider(&ProviderKind::Xai, "", "", "grok-3-mini");
        assert!(p.is_err());
    }

    #[test]
    fn build_openrouter_requires_key() {
        let p = build_provider(&ProviderKind::OpenRouter, "", "", "meta-llama/llama-3.1-8b-instruct");
        assert!(p.is_err());
    }

    #[test]
    fn build_with_valid_keys() {
        let p = build_provider(&ProviderKind::OpenAi, "", "sk-test", "gpt-4o-mini");
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::Xai, "", "xai-test", "grok-3-mini");
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::OpenRouter, "", "or-test", "meta-llama/llama-3.1-8b-instruct");
        assert!(p.is_ok());
    }
}
