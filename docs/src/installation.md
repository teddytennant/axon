# Installation

## Prerequisites

| Requirement | Notes |
|-------------|-------|
| **Rust** (stable) | Install via [rustup](https://rustup.rs) |
| **C compiler** | `gcc` or `clang` |
| **pkg-config** | For locating system libraries |
| **OpenSSL** | TLS dependency |
| **Nix** (recommended) | Handles all native deps automatically |

Axon works best on **NixOS** — all system dependencies are declared and reproducible. On NixOS you don't need to install anything manually; `nix-shell` provides everything.

On other Linux distros you'll need to install `gcc`, `pkg-config`, and `openssl` dev headers through your package manager (e.g. `apt install gcc pkg-config libssl-dev` on Debian/Ubuntu).

## From Source

### NixOS / Nix (recommended)

```bash
git clone https://github.com/teddytennant/axon
cd axon
nix-shell -p gcc pkg-config openssl --run "cargo build --release"
```

### Standard Linux

```bash
git clone https://github.com/teddytennant/axon
cd axon
cargo build --release
```

### macOS

```bash
# OpenSSL via Homebrew
brew install openssl pkg-config
git clone https://github.com/teddytennant/axon
cd axon
cargo build --release
```

The binary will be at `target/release/axon-cli`.

## Rust Dependencies

These are pulled automatically by Cargo:

| Crate | Purpose |
|-------|---------|
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
