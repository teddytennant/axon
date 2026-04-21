//! Typed response bodies for external LLM provider HTTP APIs.
//!
//! These replace ad-hoc `serde_json::Value` indexing with concrete structs so
//! callers get compile-checked field access. All fields are optional or
//! defaulted to tolerate providers omitting keys — matching the previous
//! `.as_str().unwrap_or("")` behaviour.

use serde::Deserialize;

// Ollama

#[derive(Debug, Default, Deserialize)]
pub struct OllamaGenerateResponse {
    #[serde(default)]
    pub response: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct OllamaModelDetails {
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub parameter_size: String,
}

fn unknown_model_name() -> String {
    "unknown".to_string()
}

#[derive(Debug, Deserialize)]
pub struct OllamaModelEntry {
    #[serde(default = "unknown_model_name")]
    pub name: String,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub details: OllamaModelDetails,
}

#[derive(Debug, Default, Deserialize)]
pub struct OllamaTagsResponse {
    #[serde(default)]
    pub models: Vec<OllamaModelEntry>,
}

// OpenAI-compatible chat completion

#[derive(Debug, Default, Deserialize)]
pub struct ChatCompletionMessage {
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct ChatCompletionChoice {
    #[serde(default)]
    pub message: ChatCompletionMessage,
}

#[derive(Debug, Default, Deserialize)]
pub struct OpenAiChatResponse {
    #[serde(default)]
    pub choices: Vec<ChatCompletionChoice>,
}

// OpenRouter models

#[derive(Debug, Deserialize)]
pub struct OpenRouterModel {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub context_length: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct OpenRouterModelsResponse {
    #[serde(default)]
    pub data: Vec<OpenRouterModel>,
}

// xAI models

#[derive(Debug, Deserialize)]
pub struct XaiModel {
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct XaiModelsResponse {
    #[serde(default)]
    pub data: Vec<XaiModel>,
}
