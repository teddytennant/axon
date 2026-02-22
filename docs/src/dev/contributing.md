# Contributing

## Development Setup

### NixOS (recommended)

```bash
git clone https://github.com/teddytennant/axon
cd axon
nix-shell -p gcc pkg-config openssl
cargo build
cargo test
```

### Ubuntu

```bash
sudo apt install -y gcc pkg-config libssl-dev
git clone https://github.com/teddytennant/axon
cd axon
cargo build
cargo test
```

## Project Structure

```
axon/
├── axon-core/           # Core library
│   ├── src/
│   │   ├── protocol.rs  # Wire protocol, message types, capabilities
│   │   ├── identity.rs  # Ed25519 keypair management
│   │   ├── transport.rs # QUIC transport layer
│   │   ├── runtime.rs   # Agent trait and dispatch
│   │   ├── router.rs    # Capability-based routing
│   │   ├── discovery.rs # Peer table
│   │   ├── mdns.rs      # mDNS LAN discovery
│   │   ├── gossip.rs    # Gossip protocol
│   │   ├── crdt.rs      # GCounter, LWWRegister, ORSet
│   │   └── lib.rs       # Module exports
│   └── tests/
│       └── integration.rs
├── axon-cli/            # CLI binary
│   └── src/
│       ├── main.rs      # CLI entrypoint, node orchestration
│       ├── agents.rs    # Echo, SystemInfo, LLM agents
│       ├── providers.rs # LLM provider trait + implementations
│       └── tui.rs       # Ratatui dashboard
├── axon-sdk/            # Public SDK
│   └── src/
│       └── lib.rs       # Re-exports from axon-core
├── docs/                # mdBook documentation
│   ├── book.toml
│   ├── src/
│   └── theme/           # Catppuccin theme
├── Cargo.toml           # Workspace root
├── SPEC.md              # Full specification
└── CLAUDE.md            # Build instructions
```

## Workflow

1. Make changes
2. `cargo test` — all 121 tests must pass
3. `cargo build` — zero warnings
4. Commit with a descriptive message
5. Push

## Code Conventions

- **No unsafe code**
- **Errors**: `thiserror` for library errors, `anyhow` for CLI
- **Async**: `tokio` runtime, `async_trait` for trait objects
- **Serialization**: `serde` + `bincode` for wire protocol, `serde_json` for HTTP APIs
- **Logging**: `tracing` crate with `info!`, `debug!`, `error!` macros

## Adding a New Agent

1. Create your struct in `axon-cli/src/agents.rs` (or a new file)
2. Implement the `Agent` trait (name, capabilities, handle)
3. Register it in `run_node()` in `main.rs`
4. Add tests

## Adding a New LLM Provider

1. If it's OpenAI-compatible, use `OpenAiCompatibleProvider::new()` with a constructor in `providers.rs`
2. Add a variant to `ProviderKind`
3. Update `FromStr`, `Display`, `build_provider()`, `resolve_api_key()`, `default_model()`, `default_endpoint()`
4. Add tests
5. Add a doc page in `docs/src/providers/`

## Building Docs

```bash
mdbook serve docs --open
```
