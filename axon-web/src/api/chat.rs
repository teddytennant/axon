use crate::api::provider_responses::{OllamaGenerateResponse, OpenAiChatResponse};
use crate::state::SharedWebState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::stream::{self, BoxStream, StreamExt};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;

/// Single message in an OpenAI-compatible chat completion request body.
#[derive(Debug, Serialize)]
struct OpenAiRequestMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub async fn completions(
    State(_state): State<Arc<SharedWebState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<BoxStream<'static, Result<Event, Infallible>>>, StatusCode> {
    // Load provider config
    let path = dirs::config_dir()
        .map(|d| d.join("axon").join("config.toml"))
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let (cfg_provider, cfg_endpoint, cfg_api_key, cfg_model) = if path.exists() {
        let contents =
            std::fs::read_to_string(&path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let val: toml::Value =
            toml::from_str(&contents).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let llm = val.get("llm").and_then(|v| v.as_table());
        (
            llm.and_then(|t| t.get("provider"))
                .and_then(|v| v.as_str())
                .unwrap_or("ollama")
                .to_string(),
            llm.and_then(|t| t.get("endpoint"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            llm.and_then(|t| t.get("api_key"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            llm.and_then(|t| t.get("model"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        )
    } else {
        ("ollama".into(), String::new(), String::new(), String::new())
    };

    let provider = req.provider.unwrap_or(cfg_provider);
    let model = req.model.unwrap_or(if cfg_model.is_empty() {
        default_model(&provider)
    } else {
        cfg_model
    });

    let api_key = if cfg_api_key.is_empty() {
        match provider.as_str() {
            "xai" => std::env::var("XAI_API_KEY").unwrap_or_default(),
            "openrouter" => std::env::var("OPENROUTER_API_KEY").unwrap_or_default(),
            _ => String::new(),
        }
    } else {
        cfg_api_key
    };

    let endpoint = if cfg_endpoint.is_empty() {
        default_endpoint(&provider)
    } else {
        cfg_endpoint
    };

    // Build the prompt from messages
    let prompt = req
        .messages
        .iter()
        .map(|m| {
            if m.role == "user" {
                m.content.clone()
            } else {
                format!("[{}]: {}", m.role, m.content)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // Make the LLM call
    let client = reqwest::Client::new();
    let response_text = match provider.as_str() {
        "ollama" => {
            let ep = if endpoint.is_empty() {
                "http://localhost:11434".to_string()
            } else {
                endpoint
            };
            let body = serde_json::json!({
                "model": model,
                "prompt": prompt,
                "stream": false,
            });
            let resp = client
                .post(format!("{}/api/generate", ep))
                .json(&body)
                .send()
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?;

            let json: OllamaGenerateResponse =
                resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
            json.response
        }
        _ => {
            // OpenAI-compatible (xai, openrouter, custom)
            let messages: Vec<OpenAiRequestMessage<'_>> = req
                .messages
                .iter()
                .map(|m| OpenAiRequestMessage {
                    role: &m.role,
                    content: &m.content,
                })
                .collect();

            let mut body = serde_json::json!({
                "model": model,
                "messages": messages,
            });
            if let Some(max_tokens) = req.max_tokens {
                body["max_tokens"] = serde_json::json!(max_tokens);
            }
            if let Some(temp) = req.temperature {
                body["temperature"] = serde_json::json!(temp);
            }

            let chat_endpoint = if endpoint.is_empty() {
                default_endpoint(&provider)
            } else {
                endpoint
            };

            let mut http_req = client
                .post(format!("{}/chat/completions", chat_endpoint))
                .bearer_auth(&api_key)
                .json(&body);

            if provider == "openrouter" {
                http_req = http_req.header("X-Title", "axon-mesh");
            }

            let resp = http_req.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

            if !resp.status().is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                let s: BoxStream<'static, Result<Event, Infallible>> =
                    stream::once(async move { Ok(Event::default().event("error").data(err_text)) })
                        .boxed();
                return Ok(Sse::new(s));
            }

            let json: OpenAiChatResponse =
                resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
            json.choices
                .into_iter()
                .next()
                .map(|c| c.message.content)
                .unwrap_or_default()
        }
    };

    // Stream the response as SSE events (chunked by words for streaming feel)
    let words: Vec<String> = response_text
        .split_inclusive(' ')
        .map(String::from)
        .collect();

    let event_stream: BoxStream<'static, Result<Event, Infallible>> = stream::iter(
        words
            .into_iter()
            .map(|chunk| {
                Ok(Event::default()
                    .event("message")
                    .data(serde_json::json!({"content": chunk}).to_string()))
            })
            .chain(std::iter::once(Ok(Event::default()
                .event("done")
                .data("[DONE]")))),
    )
    .boxed();

    Ok(Sse::new(event_stream))
}

fn default_model(provider: &str) -> String {
    match provider {
        "ollama" => "llama3".into(),
        "xai" => "grok-2-latest".into(),
        "openrouter" => "anthropic/claude-sonnet-4".into(),
        _ => "default".into(),
    }
}

fn default_endpoint(provider: &str) -> String {
    match provider {
        "ollama" => "http://localhost:11434".into(),
        "xai" => "https://api.x.ai/v1".into(),
        "openrouter" => "https://openrouter.ai/api/v1".into(),
        _ => String::new(),
    }
}
