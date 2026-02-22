# XAI (Grok)

Use xAI's Grok models via the OpenAI-compatible API.

## Setup

```bash
export XAI_API_KEY=xai-...
axon-cli start --provider xai
```

Or use the alias `grok`:

```bash
axon-cli start --provider grok
```

## Configuration

| Flag | Default | Description |
|------|---------|-------------|
| `--provider` | — | `xai` or `grok` |
| `--api-key` | `$XAI_API_KEY` | API key |
| `--model` | `grok-3-mini` | Model name |
| `--llm-endpoint` | `https://api.x.ai/v1` | API URL |

## Available Models

| Model | Notes |
|-------|-------|
| `grok-3` | Most capable |
| `grok-3-mini` | Fast, efficient |

## API

Uses the xAI `/v1/chat/completions` endpoint (OpenAI-compatible).
