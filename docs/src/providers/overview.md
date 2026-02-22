# Provider System

The LLM provider system abstracts over different AI backends through a common trait.

## Supported Providers

| Provider | Endpoint | Auth | Default Model |
|----------|----------|------|---------------|
| [Ollama](ollama.md) | `http://localhost:11434` | None | `llama3.2` |
| [OpenAI](openai.md) | `https://api.openai.com/v1` | API key | `gpt-4o-mini` |
| [XAI (Grok)](xai.md) | `https://api.x.ai/v1` | API key | `grok-3-mini` |
| [OpenRouter](openrouter.md) | `https://openrouter.ai/api/v1` | API key | `meta-llama/llama-3.1-8b-instruct` |
| [Custom](custom.md) | User-specified | API key | `default` |

## The Provider Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, ProviderError>;
}
```

## CLI Usage

```bash
# Select provider with --provider
axon-cli start --provider openai --model gpt-4o

# API keys via flag or environment variable
axon-cli start --provider xai --api-key xai-xxx
# or
export XAI_API_KEY=xai-xxx
axon-cli start --provider xai

# Custom endpoint
axon-cli start --provider custom --llm-endpoint https://my-llm.example.com/v1 --api-key sk-xxx
```

## API Key Resolution

Keys are resolved in order:
1. `--api-key` flag
2. Environment variable (`OPENAI_API_KEY`, `XAI_API_KEY`, `OPENROUTER_API_KEY`, `LLM_API_KEY`)
