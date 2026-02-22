# Build Matrix

Verified build and test environments for Axon. All entries below have been tested with a full `cargo build --release` and `cargo test` pass.

## Tested Platforms

| Platform | Version | Kernel | Rust | GCC | Status | Date |
|----------|---------|--------|------|-----|--------|------|
| NixOS | unstable | 6.18.10 | 1.93.0 | 15.2.0 | **121/121 tests pass** | 2026-02-22 |
| Ubuntu Server | 24.04 LTS | Docker (6.18.10 host) | 1.93.1 | 13.3.0 | **121/121 tests pass** | 2026-02-22 |

## NixOS (Primary Development)

Build command:
```bash
nix-shell -p gcc pkg-config openssl --run "cargo build --release"
```

```
Finished `release` profile [optimized] target(s) in 42s
Binary size: 9.9 MB
```

Test output:
```
axon-cli   ......  9 passed
axon-core  ...... 108 passed
integration ......  4 passed
axon-sdk  ......    0 (re-exports only)
─────────────────────────────
Total:            121 passed, 0 failed
```

## Ubuntu Server 24.04 LTS

Tested in a Docker container (`ubuntu:24.04`) from a clean state — no pre-installed Rust or dev tools.

System dependencies:
```bash
sudo apt install -y curl gcc pkg-config libssl-dev git
```

Rust install + build:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
git clone https://github.com/teddytennant/axon && cd axon
cargo build --release
```

```
Finished `release` profile [optimized] target(s) in 43.62s
```

Test output:
```
axon-cli   ......  9 passed
axon-core  ...... 108 passed
integration ......  4 passed
axon-sdk  ......    0 (re-exports only)
─────────────────────────────
Total:            121 passed, 0 failed
```

Binary verification:
```
$ ./target/release/axon-cli --version
axon 0.1.0

$ ./target/release/axon-cli identity
Identity file: /root/.axon/identity.key
Peer ID: e27698888b7d2e18fa4fb46105d3ca68f0f105bc7493d769d26cf426212b1603
Short ID: e2769888
```

## Test Methodology

All platform tests follow the same procedure:

1. Start from a clean environment (no cached build artifacts)
2. Install only documented system dependencies
3. Install Rust via rustup
4. Clone the repo
5. `cargo build --release` — must succeed with zero errors
6. `cargo test` — all 121 tests must pass
7. `./target/release/axon-cli --version` — binary must execute
8. `./target/release/axon-cli identity` — identity generation must work

## Known Requirements

| Dependency | Ubuntu Package | Nix Package | Why |
|-----------|---------------|-------------|-----|
| C compiler | `gcc` | `gcc` | Native code in ring, openssl |
| pkg-config | `pkg-config` | `pkg-config` | Locates libssl |
| OpenSSL dev | `libssl-dev` | `openssl` | TLS for reqwest/rustls |
| git | `git` | — | Clone the repo |

## Not Yet Tested

These platforms should work but haven't been verified:

- Fedora / RHEL
- Arch Linux
- macOS (Intel / Apple Silicon)
- Windows (WSL2)
- Alpine Linux (musl)
- ARM64 (Raspberry Pi)
