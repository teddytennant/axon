# Installation

## From Source

Axon is built with Rust. You need `cargo` and a C compiler.

### Standard

```bash
git clone https://github.com/teddytennant/axon
cd axon
cargo build --release
```

The binary will be at `target/release/axon-cli`.

### NixOS

```bash
nix-shell -p gcc pkg-config openssl --run "cargo build --release"
```

## Dependencies

| Dependency | Purpose |
|-----------|---------|
| `quinn` | QUIC transport |
| `rustls` | TLS |
| `ed25519-dalek` | Ed25519 identity |
| `mdns-sd` | mDNS discovery |
| `ratatui` | TUI dashboard |
| `reqwest` | HTTP client for LLM providers |

## Verify Installation

```bash
./target/release/axon-cli --version
./target/release/axon-cli identity
```
