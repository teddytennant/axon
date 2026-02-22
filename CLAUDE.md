# Axon — Decentralized AI Agent Mesh

## Build

```
nix-shell -p gcc pkg-config openssl --run "cargo build"
nix-shell -p gcc pkg-config openssl --run "cargo test"
```

## Architecture

Three-crate workspace:
- `axon-core`: Protocol, transport, identity, routing, CRDTs, discovery, runtime
- `axon-cli`: CLI binary with TUI dashboard and built-in agents
- `axon-sdk`: Public SDK for building custom agents

## Git

- Author: Teddy Tennant <teddytennant@icloud.com>
- No Co-Authored-By lines
