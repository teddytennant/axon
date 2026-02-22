# OpenAI

Use OpenAI's API for GPT models.

## Setup

```bash
export OPENAI_API_KEY=sk-...
axon-cli start --provider openai
```

## Configuration

| Flag | Default | Description |
|------|---------|-------------|
| `--provider` | — | `openai` |
| `--api-key` | `$OPENAI_API_KEY` | API key |
| `--model` | `gpt-4o-mini` | Model name |

## Available Models

| Model | Context | Notes |
|-------|---------|-------|
| `gpt-4o` | 128K | Most capable |
| `gpt-4o-mini` | 128K | Fast and cheap |
| `gpt-4-turbo` | 128K | Previous generation |
| `o1` | 200K | Reasoning model |

## API

Uses the OpenAI `/v1/chat/completions` endpoint.
