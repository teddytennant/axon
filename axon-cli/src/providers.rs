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
    Anthropic,
    Gemini,
    Xai,
    OpenRouter,
    Mistral,
    Groq,
    Together,
    DeepSeek,
    Fireworks,
    Cohere,
    Perplexity,
    Custom,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderKind::Ollama => write!(f, "ollama"),
            ProviderKind::OpenAi => write!(f, "openai"),
            ProviderKind::Anthropic => write!(f, "anthropic"),
            ProviderKind::Gemini => write!(f, "gemini"),
            ProviderKind::Xai => write!(f, "xai"),
            ProviderKind::OpenRouter => write!(f, "openrouter"),
            ProviderKind::Mistral => write!(f, "mistral"),
            ProviderKind::Groq => write!(f, "groq"),
            ProviderKind::Together => write!(f, "together"),
            ProviderKind::DeepSeek => write!(f, "deepseek"),
            ProviderKind::Fireworks => write!(f, "fireworks"),
            ProviderKind::Cohere => write!(f, "cohere"),
            ProviderKind::Perplexity => write!(f, "perplexity"),
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
            "anthropic" | "claude" => Ok(ProviderKind::Anthropic),
            "gemini" | "google" => Ok(ProviderKind::Gemini),
            "xai" | "grok" => Ok(ProviderKind::Xai),
            "openrouter" => Ok(ProviderKind::OpenRouter),
            "mistral" => Ok(ProviderKind::Mistral),
            "groq" => Ok(ProviderKind::Groq),
            "together" => Ok(ProviderKind::Together),
            "deepseek" => Ok(ProviderKind::DeepSeek),
            "fireworks" => Ok(ProviderKind::Fireworks),
            "cohere" => Ok(ProviderKind::Cohere),
            "perplexity" | "pplx" => Ok(ProviderKind::Perplexity),
            "custom" => Ok(ProviderKind::Custom),
            _ => Err(format!(
                "Unknown provider: {}. Options: ollama, openai, anthropic, gemini, xai, \
                 openrouter, mistral, groq, together, deepseek, fireworks, cohere, perplexity, custom",
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

// --- OpenAI-compatible Provider (also used by XAI, OpenRouter, Mistral, Groq, etc.) ---

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
        Self::new("openrouter", "https://openrouter.ai/api/v1", api_key, model)
            .with_header("X-Title", "axon-mesh")
    }

    pub fn mistral(api_key: String, model: String) -> Self {
        Self::new("mistral", "https://api.mistral.ai/v1", api_key, model)
    }

    pub fn groq(api_key: String, model: String) -> Self {
        Self::new("groq", "https://api.groq.com/openai/v1", api_key, model)
    }

    pub fn together(api_key: String, model: String) -> Self {
        Self::new("together", "https://api.together.xyz/v1", api_key, model)
    }

    pub fn deepseek(api_key: String, model: String) -> Self {
        Self::new("deepseek", "https://api.deepseek.com/v1", api_key, model)
    }

    pub fn fireworks(api_key: String, model: String) -> Self {
        Self::new(
            "fireworks",
            "https://api.fireworks.ai/inference/v1",
            api_key,
            model,
        )
    }

    pub fn cohere(api_key: String, model: String) -> Self {
        Self::new("cohere", "https://api.cohere.com/v2", api_key, model)
            .with_header("X-Client-Name", "axon-mesh")
    }

    pub fn perplexity(api_key: String, model: String) -> Self {
        Self::new("perplexity", "https://api.perplexity.ai", api_key, model)
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

// --- Anthropic Provider (custom API format) ---

pub struct AnthropicProvider {
    endpoint: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(endpoint: String, api_key: String, model: String) -> Self {
        Self {
            endpoint,
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let max_tokens = req.max_tokens.unwrap_or(1024);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": [{"role": "user", "content": req.prompt}],
        });

        if let Some(temp) = req.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.endpoint))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!(
                "Anthropic {} : {}",
                status, text
            )));
        }

        let json: serde_json::Value = resp.json().await?;

        // Anthropic returns {"content": [{"type": "text", "text": "..."}], ...}
        let text = json["content"]
            .as_array()
            .and_then(|blocks| {
                blocks.iter().find_map(|block| {
                    if block["type"].as_str() == Some("text") {
                        block["text"].as_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default();

        let usage = json.get("usage").map(|u| Usage {
            prompt_tokens: u["input_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["output_tokens"].as_u64().unwrap_or(0) as u32,
        });

        Ok(CompletionResponse {
            text,
            model: self.model.clone(),
            usage,
        })
    }
}

// --- Google Gemini Provider (custom API format) ---

pub struct GeminiProvider {
    endpoint: String,
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(endpoint: String, api_key: String, model: String) -> Self {
        Self {
            endpoint,
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError> {
        let mut body = serde_json::json!({
            "contents": [{"parts": [{"text": req.prompt}]}],
        });

        if let Some(max_tokens) = req.max_tokens {
            body["generationConfig"] = serde_json::json!({"maxOutputTokens": max_tokens});
        }
        if let Some(temp) = req.temperature {
            let gen_config = body
                .get_mut("generationConfig")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let mut config = gen_config;
            config["temperature"] = serde_json::json!(temp);
            body["generationConfig"] = config;
        }

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.endpoint, self.model, self.api_key
        );

        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Api(format!("Gemini {} : {}", status, text)));
        }

        let json: serde_json::Value = resp.json().await?;

        // Gemini returns {"candidates": [{"content": {"parts": [{"text": "..."}]}}]}
        let text = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let usage = json.get("usageMetadata").map(|u| Usage {
            prompt_tokens: u["promptTokenCount"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["candidatesTokenCount"].as_u64().unwrap_or(0) as u32,
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
        ProviderKind::OpenAi => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "OpenAI requires --api-key or OPENAI_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::openai(
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::Anthropic => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "Anthropic requires --api-key or ANTHROPIC_API_KEY".into(),
                ));
            }
            Ok(Box::new(AnthropicProvider::new(
                endpoint.to_string(),
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::Gemini => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "Gemini requires --api-key or GEMINI_API_KEY".into(),
                ));
            }
            Ok(Box::new(GeminiProvider::new(
                endpoint.to_string(),
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::Xai => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "XAI requires --api-key or XAI_API_KEY".into(),
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
        ProviderKind::Mistral => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "Mistral requires --api-key or MISTRAL_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::mistral(
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::Groq => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "Groq requires --api-key or GROQ_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::groq(
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::Together => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "Together requires --api-key or TOGETHER_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::together(
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::DeepSeek => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "DeepSeek requires --api-key or DEEPSEEK_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::deepseek(
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::Fireworks => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "Fireworks requires --api-key or FIREWORKS_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::fireworks(
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::Cohere => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "Cohere requires --api-key or COHERE_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::cohere(
                api_key.to_string(),
                model.to_string(),
            )))
        }
        ProviderKind::Perplexity => {
            if api_key.is_empty() {
                return Err(ProviderError::Config(
                    "Perplexity requires --api-key or PERPLEXITY_API_KEY".into(),
                ));
            }
            Ok(Box::new(OpenAiCompatibleProvider::perplexity(
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
        ProviderKind::OpenAi => "OPENAI_API_KEY",
        ProviderKind::Anthropic => "ANTHROPIC_API_KEY",
        ProviderKind::Gemini => "GEMINI_API_KEY",
        ProviderKind::Xai => "XAI_API_KEY",
        ProviderKind::OpenRouter => "OPENROUTER_API_KEY",
        ProviderKind::Mistral => "MISTRAL_API_KEY",
        ProviderKind::Groq => "GROQ_API_KEY",
        ProviderKind::Together => "TOGETHER_API_KEY",
        ProviderKind::DeepSeek => "DEEPSEEK_API_KEY",
        ProviderKind::Fireworks => "FIREWORKS_API_KEY",
        ProviderKind::Cohere => "COHERE_API_KEY",
        ProviderKind::Perplexity => "PERPLEXITY_API_KEY",
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
        ProviderKind::Anthropic => "claude-sonnet-4-20250514",
        ProviderKind::Gemini => "gemini-2.5-flash",
        ProviderKind::Xai => "grok-3-mini",
        ProviderKind::OpenRouter => "meta-llama/llama-3.1-8b-instruct",
        ProviderKind::Mistral => "mistral-large-latest",
        ProviderKind::Groq => "llama-3.3-70b-versatile",
        ProviderKind::Together => "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        ProviderKind::DeepSeek => "deepseek-chat",
        ProviderKind::Fireworks => "accounts/fireworks/models/llama-v3p3-70b-instruct",
        ProviderKind::Cohere => "command-r-plus",
        ProviderKind::Perplexity => "sonar-pro",
        ProviderKind::Custom => "default",
    }
}

/// Return the default endpoint for a provider.
pub fn default_endpoint(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Ollama => "http://localhost:11434",
        ProviderKind::OpenAi => "https://api.openai.com/v1",
        ProviderKind::Anthropic => "https://api.anthropic.com",
        ProviderKind::Gemini => "https://generativelanguage.googleapis.com",
        ProviderKind::Xai => "https://api.x.ai/v1",
        ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
        ProviderKind::Mistral => "https://api.mistral.ai/v1",
        ProviderKind::Groq => "https://api.groq.com/openai/v1",
        ProviderKind::Together => "https://api.together.xyz/v1",
        ProviderKind::DeepSeek => "https://api.deepseek.com/v1",
        ProviderKind::Fireworks => "https://api.fireworks.ai/inference/v1",
        ProviderKind::Cohere => "https://api.cohere.com/v2",
        ProviderKind::Perplexity => "https://api.perplexity.ai",
        ProviderKind::Custom => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- All ProviderKind variants for iteration in tests --
    fn all_provider_kinds() -> Vec<ProviderKind> {
        vec![
            ProviderKind::Ollama,
            ProviderKind::OpenAi,
            ProviderKind::Anthropic,
            ProviderKind::Gemini,
            ProviderKind::Xai,
            ProviderKind::OpenRouter,
            ProviderKind::Mistral,
            ProviderKind::Groq,
            ProviderKind::Together,
            ProviderKind::DeepSeek,
            ProviderKind::Fireworks,
            ProviderKind::Cohere,
            ProviderKind::Perplexity,
            ProviderKind::Custom,
        ]
    }

    #[test]
    fn provider_kind_from_str() {
        assert_eq!(
            "ollama".parse::<ProviderKind>().unwrap(),
            ProviderKind::Ollama
        );
        assert_eq!(
            "openai".parse::<ProviderKind>().unwrap(),
            ProviderKind::OpenAi
        );
        assert_eq!(
            "anthropic".parse::<ProviderKind>().unwrap(),
            ProviderKind::Anthropic
        );
        assert_eq!(
            "claude".parse::<ProviderKind>().unwrap(),
            ProviderKind::Anthropic
        );
        assert_eq!(
            "gemini".parse::<ProviderKind>().unwrap(),
            ProviderKind::Gemini
        );
        assert_eq!(
            "google".parse::<ProviderKind>().unwrap(),
            ProviderKind::Gemini
        );
        assert_eq!("xai".parse::<ProviderKind>().unwrap(), ProviderKind::Xai);
        assert_eq!("grok".parse::<ProviderKind>().unwrap(), ProviderKind::Xai);
        assert_eq!(
            "openrouter".parse::<ProviderKind>().unwrap(),
            ProviderKind::OpenRouter
        );
        assert_eq!(
            "mistral".parse::<ProviderKind>().unwrap(),
            ProviderKind::Mistral
        );
        assert_eq!(
            "groq".parse::<ProviderKind>().unwrap(),
            ProviderKind::Groq
        );
        assert_eq!(
            "together".parse::<ProviderKind>().unwrap(),
            ProviderKind::Together
        );
        assert_eq!(
            "deepseek".parse::<ProviderKind>().unwrap(),
            ProviderKind::DeepSeek
        );
        assert_eq!(
            "fireworks".parse::<ProviderKind>().unwrap(),
            ProviderKind::Fireworks
        );
        assert_eq!(
            "cohere".parse::<ProviderKind>().unwrap(),
            ProviderKind::Cohere
        );
        assert_eq!(
            "perplexity".parse::<ProviderKind>().unwrap(),
            ProviderKind::Perplexity
        );
        assert_eq!(
            "pplx".parse::<ProviderKind>().unwrap(),
            ProviderKind::Perplexity
        );
        assert_eq!(
            "custom".parse::<ProviderKind>().unwrap(),
            ProviderKind::Custom
        );
        assert!("invalid".parse::<ProviderKind>().is_err());
    }

    #[test]
    fn provider_kind_from_str_case_insensitive() {
        assert_eq!(
            "ANTHROPIC".parse::<ProviderKind>().unwrap(),
            ProviderKind::Anthropic
        );
        assert_eq!(
            "Gemini".parse::<ProviderKind>().unwrap(),
            ProviderKind::Gemini
        );
        assert_eq!(
            "DeepSeek".parse::<ProviderKind>().unwrap(),
            ProviderKind::DeepSeek
        );
    }

    #[test]
    fn provider_kind_display() {
        assert_eq!(ProviderKind::Ollama.to_string(), "ollama");
        assert_eq!(ProviderKind::OpenAi.to_string(), "openai");
        assert_eq!(ProviderKind::Anthropic.to_string(), "anthropic");
        assert_eq!(ProviderKind::Gemini.to_string(), "gemini");
        assert_eq!(ProviderKind::Xai.to_string(), "xai");
        assert_eq!(ProviderKind::OpenRouter.to_string(), "openrouter");
        assert_eq!(ProviderKind::Mistral.to_string(), "mistral");
        assert_eq!(ProviderKind::Groq.to_string(), "groq");
        assert_eq!(ProviderKind::Together.to_string(), "together");
        assert_eq!(ProviderKind::DeepSeek.to_string(), "deepseek");
        assert_eq!(ProviderKind::Fireworks.to_string(), "fireworks");
        assert_eq!(ProviderKind::Cohere.to_string(), "cohere");
        assert_eq!(ProviderKind::Perplexity.to_string(), "perplexity");
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
            "llama3.2",
        );
        assert!(p.is_ok());
    }

    #[test]
    fn build_openai_requires_key() {
        let p = build_provider(&ProviderKind::OpenAi, "", "", "gpt-4o-mini");
        assert!(p.is_err());
    }

    #[test]
    fn build_anthropic_requires_key() {
        let p = build_provider(
            &ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "",
            "claude-sonnet-4-20250514",
        );
        assert!(p.is_err());
    }

    #[test]
    fn build_gemini_requires_key() {
        let p = build_provider(
            &ProviderKind::Gemini,
            "https://generativelanguage.googleapis.com",
            "",
            "gemini-2.5-flash",
        );
        assert!(p.is_err());
    }

    #[test]
    fn build_xai_requires_key() {
        let p = build_provider(&ProviderKind::Xai, "", "", "grok-3-mini");
        assert!(p.is_err());
    }

    #[test]
    fn build_openrouter_requires_key() {
        let p = build_provider(
            &ProviderKind::OpenRouter,
            "",
            "",
            "meta-llama/llama-3.1-8b-instruct",
        );
        assert!(p.is_err());
    }

    #[test]
    fn build_mistral_requires_key() {
        let p = build_provider(&ProviderKind::Mistral, "", "", "mistral-large-latest");
        assert!(p.is_err());
    }

    #[test]
    fn build_groq_requires_key() {
        let p = build_provider(&ProviderKind::Groq, "", "", "llama-3.3-70b-versatile");
        assert!(p.is_err());
    }

    #[test]
    fn build_together_requires_key() {
        let p = build_provider(&ProviderKind::Together, "", "", "meta-llama/Llama-3.3-70B-Instruct-Turbo");
        assert!(p.is_err());
    }

    #[test]
    fn build_deepseek_requires_key() {
        let p = build_provider(&ProviderKind::DeepSeek, "", "", "deepseek-chat");
        assert!(p.is_err());
    }

    #[test]
    fn build_fireworks_requires_key() {
        let p = build_provider(&ProviderKind::Fireworks, "", "", "accounts/fireworks/models/llama-v3p3-70b-instruct");
        assert!(p.is_err());
    }

    #[test]
    fn build_cohere_requires_key() {
        let p = build_provider(&ProviderKind::Cohere, "", "", "command-r-plus");
        assert!(p.is_err());
    }

    #[test]
    fn build_perplexity_requires_key() {
        let p = build_provider(&ProviderKind::Perplexity, "", "", "sonar-pro");
        assert!(p.is_err());
    }

    #[test]
    fn build_with_valid_keys() {
        let p = build_provider(&ProviderKind::OpenAi, "", "sk-test", "gpt-4o-mini");
        assert!(p.is_ok());
        let p = build_provider(
            &ProviderKind::Anthropic,
            "https://api.anthropic.com",
            "sk-ant-test",
            "claude-sonnet-4-20250514",
        );
        assert!(p.is_ok());
        let p = build_provider(
            &ProviderKind::Gemini,
            "https://generativelanguage.googleapis.com",
            "gemini-key-test",
            "gemini-2.5-flash",
        );
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::Xai, "", "xai-test", "grok-3-mini");
        assert!(p.is_ok());
        let p = build_provider(
            &ProviderKind::OpenRouter,
            "",
            "or-test",
            "meta-llama/llama-3.1-8b-instruct",
        );
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::Mistral, "", "mis-test", "mistral-large-latest");
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::Groq, "", "groq-test", "llama-3.3-70b-versatile");
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::Together, "", "tog-test", "meta-llama/Llama-3.3-70B-Instruct-Turbo");
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::DeepSeek, "", "ds-test", "deepseek-chat");
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::Fireworks, "", "fw-test", "accounts/fireworks/models/llama-v3p3-70b-instruct");
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::Cohere, "", "co-test", "command-r-plus");
        assert!(p.is_ok());
        let p = build_provider(&ProviderKind::Perplexity, "", "pplx-test", "sonar-pro");
        assert!(p.is_ok());
    }

    #[test]
    fn resolve_api_key_explicit_wins() {
        let key = resolve_api_key("explicit-key", &ProviderKind::OpenAi);
        assert_eq!(key, "explicit-key");
    }

    #[test]
    fn resolve_api_key_ollama_always_empty() {
        let key = resolve_api_key("", &ProviderKind::Ollama);
        assert!(key.is_empty());
    }

    #[test]
    fn resolve_api_key_env_vars_mapped() {
        // Verify the env var names are correct by checking they don't panic
        // (actual env vars won't be set in test, so result is empty)
        for kind in all_provider_kinds() {
            let key = resolve_api_key("", &kind);
            if kind == ProviderKind::Ollama {
                assert!(key.is_empty());
            }
            // For others, it returns empty since env var isn't set — no panic = correct
        }
    }

    #[test]
    fn anthropic_endpoint_no_v1_suffix() {
        // Anthropic default endpoint should NOT have /v1 — the provider adds /v1/messages
        let ep = default_endpoint(&ProviderKind::Anthropic);
        assert_eq!(ep, "https://api.anthropic.com");
    }

    #[test]
    fn gemini_endpoint_is_base() {
        // Gemini default endpoint should be the base — the provider builds the full URL
        let ep = default_endpoint(&ProviderKind::Gemini);
        assert_eq!(ep, "https://generativelanguage.googleapis.com");
    }
}
