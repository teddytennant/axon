# OpenRouter

Access hundreds of models through a single API via [OpenRouter](https://openrouter.ai).

## Setup

```bash
export OPENROUTER_API_KEY=sk-or-...
axon-cli start --provider openrouter
```

## Configuration

| Flag | Default | Description |
|------|---------|-------------|
| `--provider` | — | `openrouter` |
| `--api-key` | `$OPENROUTER_API_KEY` | API key |
| `--model` | `meta-llama/llama-3.1-8b-instruct` | Model ID |

## Popular Models

| Model ID | Provider | Notes |
|----------|----------|-------|
| `meta-llama/llama-3.1-8b-instruct` | Meta | Free tier available |
| `anthropic/claude-sonnet-4` | Anthropic | High quality |
| `google/gemini-2.0-flash-001` | Google | Fast |
| `deepseek/deepseek-chat` | DeepSeek | Code-focused |
| `mistralai/mistral-large` | Mistral | General purpose |

## Extras

Axon sends `X-Title: axon-mesh` as an extra header for OpenRouter analytics.

## API

Uses the OpenRouter `/api/v1/chat/completions` endpoint (OpenAI-compatible).
