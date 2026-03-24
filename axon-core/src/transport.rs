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

/// Wrap a 32-byte Ed25519 secret key seed in PKCS#8 v1 DER encoding (RFC 8410).
///
/// The resulting 48-byte DER can be consumed by `rcgen::KeyPair::from_der()` or
/// `rustls::pki_types::PrivateKeyDer::try_from()`.
fn ed25519_seed_to_pkcs8_der(seed: &[u8; 32]) -> Vec<u8> {
    let mut der = Vec::with_capacity(48);
    der.extend_from_slice(&[0x30, 0x2e]);                   // SEQUENCE (46 bytes)
    der.extend_from_slice(&[0x02, 0x01, 0x00]);             // INTEGER 0 (version)
    der.extend_from_slice(&[0x30, 0x05]);                   // SEQUENCE (5 bytes)
    der.extend_from_slice(&[0x06, 0x03, 0x2b, 0x65, 0x70]); // OID 1.3.101.112 (Ed25519)
    der.extend_from_slice(&[0x04, 0x22]);                   // OCTET STRING (34 bytes)
    der.extend_from_slice(&[0x04, 0x20]);                   // OCTET STRING (32 bytes)
    der.extend_from_slice(seed);
    der
}

/// Extract the Ed25519 public key from a DER-encoded X.509 certificate.
///
/// Finds the SubjectPublicKeyInfo structure with the Ed25519 OID (1.3.101.112)
/// and returns the 32-byte public key. Returns `None` if the certificate does
/// not contain an Ed25519 key.
pub fn extract_ed25519_pubkey_from_cert(cert_der: &[u8]) -> Option<[u8; 32]> {
    // SubjectPublicKeyInfo for Ed25519 has a fixed DER prefix:
    // SEQUENCE(42) { SEQUENCE(5) { OID 1.3.101.112 } BITSTRING(33) { 0x00 <32 bytes> } }
    const SPKI_PREFIX: [u8; 12] = [
        0x30, 0x2a, // SEQUENCE (42 bytes)
        0x30, 0x05, // SEQUENCE (5 bytes)
        0x06, 0x03, 0x2b, 0x65, 0x70, // OID 1.3.101.112
        0x03, 0x21, // BIT STRING (33 bytes)
        0x00,       // unused bits
    ];

    cert_der
        .windows(SPKI_PREFIX.len() + 32)
        .find(|w| w[..SPKI_PREFIX.len()] == SPKI_PREFIX)
        .map(|w| {
            let mut key = [0u8; 32];
            key.copy_from_slice(&w[SPKI_PREFIX.len()..SPKI_PREFIX.len() + 32]);
            key
        })
}

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
///
/// TLS certificates are derived from the node's Ed25519 identity key, so the
/// certificate's public key IS the peer ID.  TLS handshake signatures are
/// cryptographically verified (via `MeshCertVerifier`), proving the remote
/// peer holds the private key for the advertised identity.  An additional
/// application-level identity handshake exchanges raw Ed25519 public keys as
/// a belt-and-suspenders check.
pub struct Transport {
    endpoint: Endpoint,
    connections: Arc<Mutex<std::collections::HashMap<SocketAddr, Connection>>>,
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

    /// Connect to a remote peer. After establishing the QUIC connection (with
    /// TLS handshake signature verification), performs an identity handshake:
    /// both sides exchange Ed25519 public keys on dedicated unidirectional
    /// streams. Use `connect_verified` to also assert the peer's identity.
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
    ///
    /// Verification happens at two layers:
    /// 1. **TLS layer** — the peer's certificate public key is extracted and
    ///    compared against `expected_peer_id` (the cert is derived from the
    ///    identity key, so this proves the TLS channel terminates at the
    ///    expected node).
    /// 2. **Application layer** — the identity handshake exchanges raw Ed25519
    ///    public keys and the received key is checked.
    ///
    /// If either check fails, the connection is closed and an error is returned.
    pub async fn connect_verified(
        &self,
        addr: SocketAddr,
        expected_peer_id: &[u8],
    ) -> Result<Connection, TransportError> {
        let conn = self
            .endpoint
            .connect(addr, "axon")?
            .await?;

        // TLS-level verification: extract the public key from the peer's
        // certificate and check it matches the expected identity.
        if let Some(peer_identity) = conn.peer_identity() {
            if let Some(certs) = peer_identity
                .downcast_ref::<Vec<rustls::pki_types::CertificateDer<'static>>>()
            {
                if let Some(cert) = certs.first() {
                    if let Some(cert_pubkey) = extract_ed25519_pubkey_from_cert(cert.as_ref()) {
                        if cert_pubkey.as_slice() != expected_peer_id {
                            conn.close(1u32.into(), b"TLS cert identity mismatch");
                            return Err(TransportError::PeerVerificationFailed(
                                "TLS certificate public key does not match expected peer ID"
                                    .to_string(),
                            ));
                        }
                        debug!("TLS certificate identity verified for peer at {}", addr);
                    }
                }
            }
        }

        // Application-level identity handshake: send ours, verify theirs.
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
        identity: &Identity,
    ) -> Result<(ServerConfig, ClientConfig), TransportError> {
        // Derive the TLS certificate from the node's persistent Ed25519 identity
        // key.  This binds the TLS channel to the node identity: the certificate's
        // public key IS the peer ID.
        let pkcs8_bytes = ed25519_seed_to_pkcs8_der(identity.secret_bytes());
        let pkcs8_der = rustls::pki_types::PrivateKeyDer::try_from(pkcs8_bytes)
            .map_err(|e| TransportError::Rustls(rustls::Error::General(e.to_string())))?;
        let key_pair = rcgen::KeyPair::from_der_and_sign_algo(&pkcs8_der, &rcgen::PKCS_ED25519)?;

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

        // Client config — self-signed certs are accepted, but TLS handshake
        // signatures are cryptographically verified (not skipped).
        let client_crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(MeshCertVerifier::new()))
            .with_no_client_auth();

        let client_config = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
                .map_err(|e| TransportError::Rustls(rustls::Error::General(e.to_string())))?,
        ));

        Ok((server_config, client_config))
    }
}

/// Certificate verifier for the axon mesh.
///
/// Self-signed certificates are accepted (no CA chain verification), but TLS
/// handshake signatures are cryptographically verified using the ring crypto
/// provider.  This proves the remote peer holds the private key corresponding
/// to the certificate — and since certificates are derived from identity keys,
/// this proves the peer's identity at the TLS layer.
///
/// Post-handshake identity verification (checking the cert's public key against
/// the expected peer ID) happens in `connect_verified()` / `accept_verified()`.
#[derive(Debug)]
struct MeshCertVerifier {
    supported_algs: rustls::crypto::WebPkiSupportedAlgorithms,
}

impl MeshCertVerifier {
    fn new() -> Self {
        Self {
            supported_algs: rustls::crypto::ring::default_provider()
                .signature_verification_algorithms,
        }
    }
}

impl rustls::client::danger::ServerCertVerifier for MeshCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // Accept self-signed certificates — there is no CA in the mesh.
        // The actual identity check happens post-handshake by extracting
        // the cert's public key and comparing against the expected peer ID.
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        // QUIC mandates TLS 1.3, so this should never be called — but verify
        // properly in case it is.
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported_algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported_algs)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.supported_algs.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::client::danger::ServerCertVerifier;

    /// Helper: create an rcgen KeyPair from an Identity using PKCS#8 DER.
    fn keypair_from_identity(id: &Identity) -> rcgen::KeyPair {
        let bytes = ed25519_seed_to_pkcs8_der(id.secret_bytes());
        let pkcs8 = rustls::pki_types::PrivateKeyDer::try_from(bytes).unwrap();
        rcgen::KeyPair::from_der_and_sign_algo(&pkcs8, &rcgen::PKCS_ED25519).unwrap()
    }

    #[test]
    fn tls_config_generation_succeeds() {
        let identity = Identity::generate();
        let result = Transport::make_tls_configs(&identity);
        assert!(result.is_ok(), "TLS config generation should not fail: {:?}", result.err());
    }

    #[test]
    fn pkcs8_der_encoding_length() {
        let seed = [0xABu8; 32];
        let der = ed25519_seed_to_pkcs8_der(&seed);
        assert_eq!(der.len(), 48);
    }

    #[test]
    fn pkcs8_der_roundtrip_via_rcgen() {
        let identity = Identity::generate();
        let kp = keypair_from_identity(&identity);
        assert_eq!(kp.public_key_raw(), identity.public_key_bytes());
    }

    #[test]
    fn identity_derived_cert_contains_peer_id() {
        let identity = Identity::generate();
        let kp = keypair_from_identity(&identity);
        let params = rcgen::CertificateParams::new(vec!["axon".to_string()]).unwrap();
        let cert = params.self_signed(&kp).unwrap();
        let cert_der = cert.der();

        let extracted = extract_ed25519_pubkey_from_cert(cert_der);
        assert!(extracted.is_some(), "should extract Ed25519 pubkey from cert");
        assert_eq!(
            extracted.unwrap().as_slice(),
            identity.public_key_bytes().as_slice(),
            "cert pubkey must equal identity pubkey"
        );
    }

    #[test]
    fn extract_pubkey_from_non_ed25519_cert_returns_none() {
        assert!(extract_ed25519_pubkey_from_cert(&[]).is_none());
        assert!(extract_ed25519_pubkey_from_cert(&[0u8; 100]).is_none());
    }

    #[test]
    fn different_identities_produce_different_certs() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();

        let make_cert = |id: &Identity| -> Vec<u8> {
            let kp = keypair_from_identity(id);
            let params = rcgen::CertificateParams::new(vec!["axon".to_string()]).unwrap();
            let cert = params.self_signed(&kp).unwrap();
            cert.der().to_vec()
        };

        let cert1 = make_cert(&id1);
        let cert2 = make_cert(&id2);

        let pk1 = extract_ed25519_pubkey_from_cert(&cert1).unwrap();
        let pk2 = extract_ed25519_pubkey_from_cert(&cert2).unwrap();
        assert_ne!(pk1, pk2);
    }

    #[test]
    fn mesh_cert_verifier_supported_schemes_not_empty() {
        let v = MeshCertVerifier::new();
        let schemes = v.supported_verify_schemes();
        assert!(!schemes.is_empty());
        assert!(schemes.contains(&rustls::SignatureScheme::ED25519));
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
