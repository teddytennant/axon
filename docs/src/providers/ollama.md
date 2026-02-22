# Ollama

Local LLM inference via [Ollama](https://ollama.com). No API key required.

## Setup

1. Install Ollama: `curl -fsSL https://ollama.com/install.sh | sh`
2. Pull a model: `ollama pull llama3.2`
3. Start Axon: `axon-cli start --provider ollama`

## Configuration

| Flag | Default | Description |
|------|---------|-------------|
| `--provider` | `ollama` | Provider selection |
| `--llm-endpoint` | `http://localhost:11434` | Ollama API URL |
| `--model` | `llama3.2` | Model name |

## API

Uses the Ollama `/api/generate` endpoint with `stream: false`.

## Recommended Models

| Model | Size | Use Case |
|-------|------|----------|
| `llama3.2` | 3B | General chat, fast |
| `llama3.1:8b` | 8B | Better quality |
| `codellama` | 7B | Code generation |
| `mistral` | 7B | General purpose |
