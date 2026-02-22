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

## From Source

### NixOS / Nix (recommended)

```bash
git clone https://github.com/teddytennant/axon
cd axon
nix-shell -p gcc pkg-config openssl --run "cargo build --release"
```

### Ubuntu Server / Debian

Tested on Ubuntu 24.04 LTS.

```bash
# Install system dependencies
sudo apt update
sudo apt install -y curl gcc pkg-config libssl-dev git

# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Clone and build
git clone https://github.com/teddytennant/axon
cd axon
cargo build --release
```

One-liner for a fresh Ubuntu Server:

```bash
sudo apt install -y curl gcc pkg-config libssl-dev git && \
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && \
source "$HOME/.cargo/env" && \
git clone https://github.com/teddytennant/axon && \
cd axon && cargo build --release
```

### Other Linux

Install your distro's equivalents of `gcc`, `pkg-config`, and OpenSSL development headers:

| Distro | Command |
|--------|---------|
| Ubuntu / Debian | `apt install gcc pkg-config libssl-dev` |
| Fedora / RHEL | `dnf install gcc pkg-config openssl-devel` |
| Arch | `pacman -S gcc pkg-config openssl` |

Then:

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

## Run Tests

```bash
cargo test
```

All 121 tests should pass (108 unit + 4 integration + 9 provider).

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
