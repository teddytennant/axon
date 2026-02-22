# Custom Providers

Connect to any OpenAI-compatible API endpoint.

## Setup

```bash
axon-cli start \
  --provider custom \
  --llm-endpoint https://my-llm.example.com/v1 \
  --api-key sk-xxx \
  --model my-model
```

## Configuration

| Flag | Required | Description |
|------|----------|-------------|
| `--provider` | Yes | `custom` |
| `--llm-endpoint` | Yes | Base URL of the API |
| `--api-key` | Yes | API key (or `$LLM_API_KEY`) |
| `--model` | No | Model name (default: `default`) |

## Compatible Services

Any service implementing the OpenAI `/v1/chat/completions` endpoint works:

- [vLLM](https://github.com/vllm-project/vllm)
- [text-generation-inference](https://github.com/huggingface/text-generation-inference)
- [LocalAI](https://localai.io)
- [LiteLLM](https://github.com/BerriAI/litellm)
- [Anyscale Endpoints](https://www.anyscale.com)
- [Together AI](https://www.together.ai)
- [Groq](https://groq.com)
- [Fireworks AI](https://fireworks.ai)

## Request Format

The custom provider sends standard OpenAI chat completion requests:

```json
{
  "model": "your-model",
  "messages": [
    {"role": "user", "content": "your prompt"}
  ]
}
```
