use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IdentityError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid key data: expected 32 bytes, got {0}")]
    InvalidKeyLength(usize),
    #[error("Signature verification failed")]
    VerificationFailed,
}

/// A node's cryptographic identity based on Ed25519.
#[derive(Debug)]
pub struct Identity {
    signing_key: SigningKey,
}

impl Identity {
    /// Generate a new random identity.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    /// Create identity from raw secret key bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, IdentityError> {
        if bytes.len() != 32 {
            return Err(IdentityError::InvalidKeyLength(bytes.len()));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(bytes);
        let signing_key = SigningKey::from_bytes(&key_bytes);
        Ok(Self { signing_key })
    }

    /// Load identity from file, or generate and save a new one.
    pub fn load_or_generate(path: &Path) -> Result<Self, IdentityError> {
        if path.exists() {
            let bytes = std::fs::read(path)?;
            Self::from_bytes(&bytes)
        } else {
            let identity = Self::generate();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, identity.secret_bytes())?;
            Ok(identity)
        }
    }

    /// Default path for identity key.
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".axon")
            .join("identity.key")
    }

    /// Raw secret key bytes (32 bytes).
    pub fn secret_bytes(&self) -> &[u8; 32] {
        self.signing_key.as_bytes()
    }

    /// Public key bytes (32 bytes) — used as PeerId.
    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.signing_key.verifying_key().to_bytes().to_vec()
    }

    /// Get the verifying (public) key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Get a reference to the signing key.
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Sign a message.
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let sig = self.signing_key.sign(message);
        sig.to_bytes().to_vec()
    }

    /// Verify a signature against a public key.
    pub fn verify(
        public_key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), IdentityError> {
        if public_key.len() != 32 {
            return Err(IdentityError::InvalidKeyLength(public_key.len()));
        }
        if signature.len() != 64 {
            return Err(IdentityError::VerificationFailed);
        }

        let mut pk_bytes = [0u8; 32];
        pk_bytes.copy_from_slice(public_key);
        let verifying_key = VerifyingKey::from_bytes(&pk_bytes)
            .map_err(|_| IdentityError::VerificationFailed)?;

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(signature);
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);

        verifying_key
            .verify(message, &signature)
            .map_err(|_| IdentityError::VerificationFailed)
    }

    /// Peer ID as hex string.
    pub fn peer_id_hex(&self) -> String {
        hex::encode(&self.public_key_bytes())
    }

    /// Short peer ID (first 8 hex chars).
    pub fn peer_id_short(&self) -> String {
        self.peer_id_hex()[..8].to_string()
    }
}

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn generate_produces_valid_identity() {
        let id = Identity::generate();
        assert_eq!(id.public_key_bytes().len(), 32);
        assert_eq!(id.secret_bytes().len(), 32);
    }

    #[test]
    fn from_bytes_roundtrip() {
        let id1 = Identity::generate();
        let secret = id1.secret_bytes().to_vec();
        let id2 = Identity::from_bytes(&secret).unwrap();
        assert_eq!(id1.public_key_bytes(), id2.public_key_bytes());
    }

    #[test]
    fn from_bytes_wrong_length() {
        let result = Identity::from_bytes(&[0u8; 16]);
        assert!(result.is_err());
        match result.unwrap_err() {
            IdentityError::InvalidKeyLength(16) => {}
            e => panic!("unexpected error: {:?}", e),
        }
    }

    #[test]
    fn sign_and_verify() {
        let id = Identity::generate();
        let message = b"hello axon mesh";
        let signature = id.sign(message);
        assert_eq!(signature.len(), 64);

        let result = Identity::verify(&id.public_key_bytes(), message, &signature);
        assert!(result.is_ok());
    }

    #[test]
    fn verify_wrong_message_fails() {
        let id = Identity::generate();
        let signature = id.sign(b"correct message");
        let result = Identity::verify(&id.public_key_bytes(), b"wrong message", &signature);
        assert!(result.is_err());
    }

    #[test]
    fn verify_wrong_key_fails() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();
        let signature = id1.sign(b"test");
        let result = Identity::verify(&id2.public_key_bytes(), b"test", &signature);
        assert!(result.is_err());
    }

    #[test]
    fn verify_invalid_signature_length() {
        let id = Identity::generate();
        let result = Identity::verify(&id.public_key_bytes(), b"test", &[0u8; 32]);
        assert!(result.is_err());
    }

    #[test]
    fn peer_id_hex_is_64_chars() {
        let id = Identity::generate();
        assert_eq!(id.peer_id_hex().len(), 64);
    }

    #[test]
    fn peer_id_short_is_8_chars() {
        let id = Identity::generate();
        assert_eq!(id.peer_id_short().len(), 8);
    }

    #[test]
    fn load_or_generate_creates_new_file() {
        let dir = std::env::temp_dir().join(format!("axon-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("identity.key");

        let id = Identity::load_or_generate(&path).unwrap();
        assert!(path.exists());

        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes, id.secret_bytes().as_slice());

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_or_generate_loads_existing() {
        let dir = std::env::temp_dir().join(format!("axon-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("identity.key");

        let id1 = Identity::load_or_generate(&path).unwrap();
        let id2 = Identity::load_or_generate(&path).unwrap();

        assert_eq!(id1.public_key_bytes(), id2.public_key_bytes());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn different_identities_have_different_keys() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();
        assert_ne!(id1.public_key_bytes(), id2.public_key_bytes());
    }
}
