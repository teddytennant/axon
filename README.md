# axon

A Rust node that lets AI agents on different machines find each other and hand tasks back and forth over QUIC. Each node runs a few local agents (echo, system info, an LLM proxy, plus any MCP servers you point it at), advertises them as capabilities, and forwards tasks it can't handle to a peer that can.

It's early. CLI flags, config and on-disk state change between commits without migrations. See [CHANGELOG.md](CHANGELOG.md).

## Build

You need Rust, Node 20 (for the web UI, which gets embedded into the binary) and OpenSSL headers.

```bash
git clone https://github.com/teddytennant/axon
cd axon
(cd axon-web/frontend && npm install && npm run build)
cargo build --release -p axon-cli
```

`axon-desktop` needs the Tauri system libraries (GTK, WebKitGTK) only if you build it.

On NixOS: `nix-shell -p gcc pkg-config openssl --run "cargo build --release -p axon-cli"`.

The binary is `target/release/axon-cli`.

## Run

Start two nodes and send a task through the first:

```bash
axon-cli start --headless --listen 127.0.0.1:4242 --health-port 4243
axon-cli start --headless --listen 127.0.0.1:4252 --peer 127.0.0.1:4242

axon-cli send --peer 127.0.0.1:4242 --namespace echo --name ping --data hello
axon-cli peers --node 127.0.0.1:4242
```

Without `--headless`, `start` opens a terminal dashboard (tabs for mesh, agents, tasks, state, logs, settings and workflows; `q` quits, `Tab` switches). `--web-port 3000` serves a browser dashboard and JSON API on that port, bound to `0.0.0.0`. `--health-port` opens a TCP port that writes one line of JSON (uptime, task counts, peer count, queue depth) to anyone who connects.

The first interactive `start` with no config runs a setup wizard. You can also run it directly with `axon-cli setup`, or write a starter config with `axon-cli init`. Config lives in `~/.config/axon/config.toml`; the node identity, task queue and trust database live in `~/.axon/`.

Other commands: `status`, `identity`, `ask <prompt>`, `chat`, `auth <provider>`, `models`, `tools`, `serve-mcp`, `trust`. `axon-cli <command> --help` lists the flags.

## LLM providers

The built-in `llm:chat:v1` agent talks to Ollama (default, `http://localhost:11434`), xAI, OpenRouter, or any OpenAI-compatible endpoint (`custom`). Keys come from `axon-cli auth <provider>`, the config file, or `XAI_API_KEY` / `OPENROUTER_API_KEY`.

```bash
axon-cli auth openrouter
axon-cli models --provider openrouter --filter claude
axon-cli ask "what is the capital of france"
axon-cli chat
```

`chat` is a terminal chat UI with slash commands (`/help` lists them).

## MCP

A node spawns every MCP server listed in its config and advertises each tool as a capability named `mcp.<server>:<tool>:v1`, so peers can call it like any other agent.

```toml
[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
```

`axon-cli serve-mcp` goes the other way: it aggregates those servers and serves the combined tool list as one MCP server on stdio. With `--mesh` it also joins the mesh and forwards calls for tools that live on other peers. To use it from Claude Code:

```json
{ "mcpServers": { "axon": { "command": "axon-cli", "args": ["serve-mcp", "--mesh"] } } }
```

`axon-cli tools --node <addr>` lists the tools a running node knows about, with `--query`, `--server`, and `--budget <tokens>` to cap how much schema comes back.

## How tasks move

Each node has an Ed25519 identity in `~/.axon/identity.key`. Connections are QUIC with mutual TLS on self-signed certs derived from that key, followed by a handshake that checks the cert key matches the claimed peer ID. Peers are found by `--peer`, by mDNS on the LAN, and by gossiping peer lists.

When a node gets a task, it runs it if a local agent has the capability. If not, it sends an offer to every known peer that advertises the capability, collects bids for 2 seconds, and forwards the task to the best score. Scores are weighted by trust, which each node builds from its own record of how peers did on past tasks (success, failure, timeouts, latency against their estimate), with older results decaying. That record persists in `~/.axon/trust` and can be inspected with `axon-cli trust show` and `axon-cli trust history <peer-id>`. Tasks go through a persistent queue that is replayed on restart.

## Crates

- `axon-core`: transport, identity, protocol, discovery, negotiation, trust, task queue, MCP client and server. Also has CRDT types and workflow helpers (`pipeline`, `fan_out`, `supervisor`) that the node doesn't use yet.
- `axon-cli`: the `axon-cli` binary and its built-in agents.
- `axon-web`: the HTTP/WebSocket server and React dashboard behind `--web-port`.
- `axon-sdk`: re-exports the `Agent` trait and types for writing agents in Rust. `axon-cli` only registers its built-in agents, so a Rust agent needs its own node binary built on `axon_core::Runtime`.
- `axon-python`: write agents in Python and plug them in as MCP servers. See [axon-python/README.md](axon-python/README.md).
- `axon-desktop`: a Tauri desktop app that talks to a node started with `--web-port`.
- `axon-test-mcp-server`: a small MCP server used by the tests.
- `axon-pouw`: runs an axon workflow as a work unit for an external consensus crate. It isn't a workspace member and doesn't build without that crate.

## License

MIT
