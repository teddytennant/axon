# CLI Reference

## `axon-cli start`

Start a mesh node.

```
axon-cli start [OPTIONS]
```

| Flag | Default | Description |
|------|---------|-------------|
| `-l, --listen` | `0.0.0.0:4242` | Address to listen on |
| `-p, --peer` | — | Bootstrap peer addresses (repeatable) |
| `--headless` | `false` | Disable TUI, run in background mode |
| `--provider` | `ollama` | LLM provider: ollama, openai, xai, openrouter, custom |
| `--llm-endpoint` | Per provider | LLM API endpoint URL |
| `--api-key` | Per env var | API key for the LLM provider |
| `--model` | Per provider | Model name |

## `axon-cli send`

Send a task to a specific peer.

```
axon-cli send --peer <ADDR> --namespace <NS> --name <NAME> [-d <DATA>]
```

| Flag | Description |
|------|-------------|
| `-p, --peer` | Target peer address |
| `-n, --namespace` | Capability namespace |
| `-c, --name` | Capability name |
| `-d, --data` | Payload string (default: empty) |

## `axon-cli identity`

Generate or display the node's identity.

```
axon-cli identity
```

## `axon-cli status`

Show node status and peer ID.

```
axon-cli status
```

## `axon-cli peers`

List known peers (queries a running node).

```
axon-cli peers [--node <ADDR>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `-p, --node` | `127.0.0.1:4242` | Address of the node to query |
