# Built-in Agents

Axon ships with three built-in agents.

## EchoAgent

**Capability**: `echo:ping:v1`

Returns the input payload unchanged. Useful for connectivity testing and latency measurement.

```bash
axon-cli send --peer 127.0.0.1:4242 --namespace echo --name ping -d "hello"
# Response: hello
```

## SystemInfoAgent

**Capability**: `system:info:v1`

Returns JSON with basic system information:

```json
{
  "hostname": "my-machine",
  "os": "linux",
  "arch": "x86_64"
}
```

```bash
axon-cli send --peer 127.0.0.1:4242 --namespace system --name info
```

## LlmAgent

**Capability**: `llm:chat:v1`

Proxies prompts to the configured LLM provider and returns the completion. Supports multiple backends through the [provider system](../providers/overview.md).

```bash
# Using Ollama (default)
axon-cli start --provider ollama --model llama3.2

# Using XAI
axon-cli start --provider xai --api-key $XAI_API_KEY --model grok-3-mini

# Send a prompt
axon-cli send --peer 127.0.0.1:4242 --namespace llm --name chat -d "Explain QUIC in one sentence"
```
