# Identity & Cryptography

Each Axon node has a unique Ed25519 identity.

## Key Generation

On first start, a 32-byte Ed25519 signing key is generated and persisted to `~/.config/axon/identity.key`. The public key (also 32 bytes) serves as the node's **Peer ID**.

## Peer ID

The peer ID is the hex-encoded public key (64 characters). The short form uses the first 8 characters.

```bash
axon-cli identity
# Peer ID: a1b2c3d4e5f6...
# Short ID: a1b2c3d4
```

## Signing

Messages can be signed with the node's private key and verified by any peer with the public key:

```rust
let signature = identity.sign(b"message");
let valid = identity.verify(b"message", &signature);
```

## Storage

The identity file is stored at the platform-appropriate config directory:
- Linux: `~/.config/axon/identity.key`
- macOS: `~/Library/Application Support/axon/identity.key`
