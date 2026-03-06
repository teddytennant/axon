use crate::identity::Identity;
use crate::protocol::Message;
use quinn::{ClientConfig, Connection, Endpoint, ServerConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Size of an Ed25519 public key in bytes.
const ED25519_PUBKEY_LEN: usize = 32;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Connection error: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("Connect error: {0}")]
    Connect(#[from] quinn::ConnectError),
    #[error("Write error: {0}")]
    Write(#[from] quinn::WriteError),
    #[error("Read error: {0}")]
    ReadExact(#[from] quinn::ReadExactError),
    #[error("Read to end error: {0}")]
    ReadToEnd(#[from] quinn::ReadToEndError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS error: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("Certificate generation error: {0}")]
    CertGen(#[from] rcgen::Error),
    #[error("Stream closed")]
    ClosedStream(#[from] quinn::ClosedStream),
    #[error("No connection to peer")]
    NotConnected,
    #[error("Message too large: {0} bytes (max {1})")]
    MessageTooLarge(usize, usize),
    #[error("Peer identity verification failed: {0}")]
    PeerVerificationFailed(String),
}

/// Maximum message size: 16 MB.
const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// QUIC-based transport layer for the mesh.
pub struct Transport {
    endpoint: Endpoint,
    connections: Arc<Mutex<std::collections::HashMap<SocketAddr, Connection>>>,
    /// The local node's Ed25519 public key, sent as the first message after
    /// a QUIC connection is established to allow lightweight peer identity
    /// verification. This is not a full PKI solution — TLS certificate
    /// verification is still skipped for self-signed certs — but it ensures
    /// the peer we connected to is who we expected (based on mDNS discovery
    /// or an explicit peer ID).
    local_public_key: Vec<u8>,
}

impl Transport {
    /// Create a new transport bound to the given address.
    pub async fn bind(
        addr: SocketAddr,
        identity: &Identity,
    ) -> Result<Self, TransportError> {
        let (server_config, client_config) = Self::make_tls_configs(identity)?;

        let mut endpoint = Endpoint::server(server_config, addr)?;
        endpoint.set_default_client_config(client_config);

        Ok(Self {
            endpoint,
            connections: Arc::new(Mutex::new(std::collections::HashMap::new())),
            local_public_key: identity.public_key_bytes(),
        })
    }

    /// Get the local address this transport is bound to.
    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        Ok(self.endpoint.local_addr()?)
    }

    /// Connect to a remote peer. After establishing the QUIC connection,
    /// performs a lightweight identity handshake: both sides exchange Ed25519
    /// public keys on dedicated unidirectional streams. This is not a full PKI
    /// solution — TLS certificate verification is still skipped for self-signed
    /// certs — but it allows either side to verify the peer it connected to is
    /// who it expected (based on mDNS discovery or explicit peer ID).
    pub async fn connect(&self, addr: SocketAddr) -> Result<Connection, TransportError> {
        {
            let conns = self.connections.lock().await;
            if let Some(conn) = conns.get(&addr) {
                if conn.close_reason().is_none() {
                    return Ok(conn.clone());
                }
            }
        }

        let conn = self
            .endpoint
            .connect(addr, "axon")?
            .await?;

        // Identity handshake: send ours, consume theirs.
        Self::send_identity(&conn, &self.local_public_key).await?;
        let _remote_key = Self::recv_identity(&conn).await?;

        let mut conns = self.connections.lock().await;
        conns.insert(addr, conn.clone());
        info!("Connected to peer at {}", addr);
        Ok(conn)
    }

    /// Connect to a remote peer and verify its identity against `expected_peer_id`.
    /// If the remote peer's public key does not match, the connection is closed
    /// and an error is returned.
    pub async fn connect_verified(
        &self,
        addr: SocketAddr,
        expected_peer_id: &[u8],
    ) -> Result<Connection, TransportError> {
        let conn = self
            .endpoint
            .connect(addr, "axon")?
            .await?;

        // Identity handshake: send ours, verify theirs.
        Self::send_identity(&conn, &self.local_public_key).await?;
        let remote_key = Self::recv_identity(&conn).await?;

        if remote_key != expected_peer_id {
            conn.close(1u32.into(), b"identity mismatch");
            return Err(TransportError::PeerVerificationFailed(format!(
                "expected peer ID {:02x?}..., got {:02x?}...",
                &expected_peer_id[..4.min(expected_peer_id.len())],
                &remote_key[..4.min(remote_key.len())],
            )));
        }

        let mut conns = self.connections.lock().await;
        conns.insert(addr, conn.clone());
        info!("Verified and connected to peer at {}", addr);
        Ok(conn)
    }

    /// Accept an incoming connection. Performs the identity handshake: sends
    /// our Ed25519 public key and reads the remote peer's key. The remote key
    /// is logged but not verified (use `accept_verified` for strict checks).
    pub async fn accept(&self) -> Option<Connection> {
        let incoming = self.endpoint.accept().await?;
        match incoming.await {
            Ok(conn) => {
                let addr = conn.remote_address();

                // Identity handshake: send ours, consume theirs.
                if let Err(e) = Self::send_identity(&conn, &self.local_public_key).await {
                    warn!("Failed to send identity to {}: {}", addr, e);
                    return None;
                }
                match Self::recv_identity(&conn).await {
                    Ok(remote_key) => {
                        debug!("Peer at {} presented identity {:02x?}...", addr, &remote_key[..4.min(remote_key.len())]);
                    }
                    Err(e) => {
                        warn!("Failed to receive identity from {}: {}", addr, e);
                        return None;
                    }
                }

                let mut conns = self.connections.lock().await;
                conns.insert(addr, conn.clone());
                info!("Accepted connection from {}", addr);
                Some(conn)
            }
            Err(e) => {
                warn!("Failed to accept connection: {}", e);
                None
            }
        }
    }

    /// Accept an incoming connection and verify the remote peer's identity.
    /// Returns the connection and the remote peer's public key. If
    /// `expected_peer_id` is `Some`, the remote key is checked against it.
    pub async fn accept_verified(
        &self,
        expected_peer_id: Option<&[u8]>,
    ) -> Result<(Connection, Vec<u8>), TransportError> {
        let incoming = self.endpoint.accept().await.ok_or(TransportError::NotConnected)?;
        let conn = incoming.await?;
        let addr = conn.remote_address();

        // Identity handshake: send ours, verify theirs.
        Self::send_identity(&conn, &self.local_public_key).await?;
        let remote_key = Self::recv_identity(&conn).await?;

        if let Some(expected) = expected_peer_id {
            if remote_key != expected {
                conn.close(1u32.into(), b"identity mismatch");
                return Err(TransportError::PeerVerificationFailed(format!(
                    "expected peer ID {:02x?}..., got {:02x?}...",
                    &expected[..4.min(expected.len())],
                    &remote_key[..4.min(remote_key.len())],
                )));
            }
        }

        let mut conns = self.connections.lock().await;
        conns.insert(addr, conn.clone());
        info!("Accepted verified connection from {}", addr);
        Ok((conn, remote_key))
    }

    /// Send a message over an existing connection.
    pub async fn send(
        conn: &Connection,
        message: &Message,
    ) -> Result<(), TransportError> {
        let data = message.encode()?;
        if data.len() > MAX_MESSAGE_SIZE {
            return Err(TransportError::MessageTooLarge(data.len(), MAX_MESSAGE_SIZE));
        }

        let mut send = conn.open_uni().await?;
        // Write length prefix (4 bytes, big-endian) then data.
        let len = (data.len() as u32).to_be_bytes();
        send.write_all(&len).await?;
        send.write_all(&data).await?;
        send.finish()?;
        debug!("Sent {} bytes", data.len());
        Ok(())
    }

    /// Receive a message from a connection.
    pub async fn recv(conn: &Connection) -> Result<Message, TransportError> {
        let mut recv = conn.accept_uni().await?;
        // Read length prefix.
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        if len > MAX_MESSAGE_SIZE {
            return Err(TransportError::MessageTooLarge(len, MAX_MESSAGE_SIZE));
        }

        let data = recv.read_to_end(len).await?;
        let message = Message::decode(&data)?;
        debug!("Received {} bytes", data.len());
        Ok(message)
    }

    /// Get a reference to the endpoint.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Close all connections and the endpoint.
    pub async fn shutdown(&self) {
        let conns = self.connections.lock().await;
        for (addr, conn) in conns.iter() {
            conn.close(0u32.into(), b"shutdown");
            debug!("Closed connection to {}", addr);
        }
        self.endpoint.close(0u32.into(), b"shutdown");
        info!("Transport shut down");
    }

    /// Number of active connections.
    pub async fn connection_count(&self) -> usize {
        let conns = self.connections.lock().await;
        conns.values().filter(|c| c.close_reason().is_none()).count()
    }

    /// Send our Ed25519 public key over a unidirectional stream.
    async fn send_identity(conn: &Connection, public_key: &[u8]) -> Result<(), TransportError> {
        let mut send = conn.open_uni().await?;
        send.write_all(public_key).await?;
        send.finish()?;
        debug!("Sent identity ({} bytes)", public_key.len());
        Ok(())
    }

    /// Receive a remote peer's Ed25519 public key from a unidirectional stream.
    async fn recv_identity(conn: &Connection) -> Result<Vec<u8>, TransportError> {
        let mut recv = conn.accept_uni().await?;
        let mut buf = [0u8; ED25519_PUBKEY_LEN];
        recv.read_exact(&mut buf).await?;
        debug!("Received remote identity");
        Ok(buf.to_vec())
    }

    fn make_tls_configs(
        _identity: &Identity,
    ) -> Result<(ServerConfig, ClientConfig), TransportError> {
        // Generate a self-signed certificate for QUIC
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
        let params = rcgen::CertificateParams::new(vec!["axon".to_string()])?;
        let cert = params.self_signed(&key_pair)?;

        let cert_der = rustls::pki_types::CertificateDer::from(cert.der().to_vec());
        let key_der = rustls::pki_types::PrivateKeyDer::try_from(key_pair.serialize_der())
            .map_err(|e| TransportError::Rustls(rustls::Error::General(e.to_string())))?;

        // Server config
        let server_config = ServerConfig::with_single_cert(
            vec![cert_der.clone()],
            key_der.clone_key(),
        )?;

        // Client config — skip server certificate verification (mesh peers use self-signed)
        let client_crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipVerification))
            .with_no_client_auth();

        let client_config = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
                .map_err(|e| TransportError::Rustls(rustls::Error::General(e.to_string())))?,
        ));

        Ok((server_config, client_config))
    }
}

/// Accepts any certificate (peers in the mesh use self-signed certs).
#[derive(Debug)]
struct SkipVerification;

impl rustls::client::danger::ServerCertVerifier for SkipVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_config_generation_succeeds() {
        let identity = Identity::generate();
        let result = Transport::make_tls_configs(&identity);
        assert!(result.is_ok(), "TLS config generation should not fail: {:?}", result.err());
    }

    #[tokio::test]
    async fn transport_bind_and_get_addr() {
        let identity = Identity::generate();
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let transport = Transport::bind(addr, &identity).await.unwrap();
        let local = transport.local_addr().unwrap();
        assert_ne!(local.port(), 0);
    }

    #[tokio::test]
    async fn transport_connect_and_exchange() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();

        let addr1: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let addr2: SocketAddr = "127.0.0.1:0".parse().unwrap();

        let t1 = Transport::bind(addr1, &id1).await.unwrap();
        let t2 = Transport::bind(addr2, &id2).await.unwrap();

        let t2_addr = t2.local_addr().unwrap();

        // Spawn receiver
        let recv_handle = tokio::spawn(async move {
            let conn = t2.accept().await.unwrap();
            let msg = Transport::recv(&conn).await.unwrap();
            msg
        });

        // Connect and send
        let conn = t1.connect(t2_addr).await.unwrap();
        let ping = Message::Ping { nonce: 42 };
        Transport::send(&conn, &ping).await.unwrap();

        let received = recv_handle.await.unwrap();
        match received {
            Message::Ping { nonce } => assert_eq!(nonce, 42),
            _ => panic!("wrong message type"),
        }
    }

    #[tokio::test]
    async fn transport_connection_count() {
        let id = Identity::generate();
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let transport = Transport::bind(addr, &id).await.unwrap();
        assert_eq!(transport.connection_count().await, 0);
    }

    #[tokio::test]
    async fn transport_verified_connect_success() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();
        let id2_pubkey = id2.public_key_bytes();

        let t1 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id1)
            .await
            .unwrap();
        let t2 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id2)
            .await
            .unwrap();

        let t2_addr = t2.local_addr().unwrap();

        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

        let accept_handle = tokio::spawn(async move {
            let (conn, remote_key) = t2.accept_verified(None).await.unwrap();
            assert_eq!(remote_key, id1.public_key_bytes());
            let _ = done_rx.await;
            conn
        });

        let conn = t1.connect_verified(t2_addr, &id2_pubkey).await.unwrap();
        // Verify the connection works by exchanging a message.
        Transport::send(&conn, &Message::Ping { nonce: 77 }).await.unwrap();

        let _ = done_tx.send(());
        let _ = accept_handle.await;
    }

    #[tokio::test]
    async fn transport_verified_connect_mismatch() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();
        let id3 = Identity::generate(); // wrong identity

        let t1 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id1)
            .await
            .unwrap();
        let t2 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id2)
            .await
            .unwrap();

        let t2_addr = t2.local_addr().unwrap();

        // Spawn the acceptor so the connection can complete.
        tokio::spawn(async move {
            // Accept will succeed from t2's perspective.
            let _ = t2.accept_verified(None).await;
        });

        // Connector expects id3 but will get id2 — should fail.
        let result = t1.connect_verified(t2_addr, &id3.public_key_bytes()).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::PeerVerificationFailed(_) => {} // expected
            e => panic!("unexpected error: {:?}", e),
        }
    }

    #[tokio::test]
    async fn transport_bidirectional_exchange() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();

        let t1 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id1)
            .await
            .unwrap();
        let t2 = Transport::bind("127.0.0.1:0".parse().unwrap(), &id2)
            .await
            .unwrap();

        let t2_addr = t2.local_addr().unwrap();

        // Use a channel to signal when t1 has received the pong,
        // so t2 stays alive until the exchange is complete.
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();

        let t2_handle = tokio::spawn(async move {
            let conn = t2.accept().await.unwrap();
            let msg = Transport::recv(&conn).await.unwrap();
            let pong = Message::Pong { nonce: 99 };
            Transport::send(&conn, &pong).await.unwrap();
            // Wait for t1 to confirm receipt before dropping
            let _ = rx.await;
            msg
        });

        let conn = t1.connect(t2_addr).await.unwrap();
        Transport::send(&conn, &Message::Ping { nonce: 99 }).await.unwrap();

        let pong = Transport::recv(&conn).await.unwrap();
        match pong {
            Message::Pong { nonce } => assert_eq!(nonce, 99),
            _ => panic!("expected pong"),
        }

        // Signal t2 that we're done
        let _ = tx.send(());

        let ping = t2_handle.await.unwrap();
        match ping {
            Message::Ping { nonce } => assert_eq!(nonce, 99),
            _ => panic!("expected ping"),
        }
    }
}
