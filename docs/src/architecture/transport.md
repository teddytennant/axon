# Transport

Axon uses QUIC (via `quinn`) for all node-to-node communication.

## Why QUIC

- **Multiplexed streams**: Multiple concurrent messages without head-of-line blocking
- **Built-in TLS**: Encryption is mandatory, not optional
- **Connection migration**: Handles network changes gracefully
- **Fast connection establishment**: 0-RTT or 1-RTT handshakes

## TLS Configuration

Each node generates a self-signed Ed25519 TLS certificate at startup using `rcgen`. Client connections skip certificate verification since identity is established through the Axon protocol layer (Ed25519 peer IDs).

## API

```rust
// Bind to an address
let transport = Transport::bind("0.0.0.0:4242".parse()?, &identity).await?;

// Connect to a peer
let conn = transport.connect(peer_addr).await?;

// Send a message
Transport::send(&conn, &message).await?;

// Receive a message
let msg = Transport::recv(&conn).await?;

// Accept incoming connections
if let Some(conn) = transport.accept().await { ... }
```

## Error Handling

Transport errors are typed:

```rust
enum TransportError {
    Io(std::io::Error),
    Connection(quinn::ConnectionError),
    WriteError(quinn::WriteError),
    ReadError(quinn::ReadExactError),
    ClosedStream(quinn::ClosedStream),
    Codec(String),
    MessageTooLarge(usize),
}
```
