//! Cryptographic utilities for DNS Seed
//!
//! Uses Ed25519 for signing, compatible with kratos-core.
//! All heartbeats and IDpeers.json files are signed to prevent tampering.

use ed25519_dalek::{
    Signature, SigningKey, VerifyingKey, Signer, Verifier,
    SECRET_KEY_LENGTH, SIGNATURE_LENGTH,
};
use rand::rngs::OsRng;
use std::path::Path;
use tracing::info;

use crate::types::{Hash, PublicKey, SeedId, HeartbeatMessage, IdPeersFile};

/// Domain separation prefix for heartbeat signatures
const DOMAIN_HEARTBEAT: &[u8] = b"KRATOS_DNS_HEARTBEAT_V1:";

/// Domain separation prefix for IDpeers.json signatures
const DOMAIN_IDPEERS: &[u8] = b"KRATOS_IDPEERS_V1:";

// =============================================================================
// KEYPAIR MANAGEMENT
// =============================================================================

/// Generate a new Ed25519 keypair
pub fn generate_keypair() -> SigningKey {
    SigningKey::generate(&mut OsRng)
}

/// Load keypair from file or generate new one
pub async fn load_or_generate_keypair(
    data_dir: &Path,
    key_file: Option<&std::path::PathBuf>,
) -> anyhow::Result<SigningKey> {
    let key_path = key_file
        .map(|p| p.clone())
        .unwrap_or_else(|| data_dir.join("dns_seed.key"));

    if key_path.exists() {
        info!("🔑 Loading existing keypair from {:?}", key_path);
        load_keypair(&key_path).await
    } else {
        info!("🔑 Generating new keypair");
        let keypair = generate_keypair();
        save_keypair(&keypair, &key_path).await?;
        info!("🔑 Keypair saved to {:?}", key_path);
        Ok(keypair)
    }
}

/// Save keypair to file (secret key only, verifying key is derived)
pub async fn save_keypair(keypair: &SigningKey, path: &Path) -> anyhow::Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Write secret key bytes
    let secret_bytes = keypair.to_bytes();
    tokio::fs::write(path, secret_bytes).await?;

    // Set file permissions (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = tokio::fs::metadata(path).await?.permissions();
        perms.set_mode(0o600);
        tokio::fs::set_permissions(path, perms).await?;
    }

    Ok(())
}

/// Load keypair from file
pub async fn load_keypair(path: &Path) -> anyhow::Result<SigningKey> {
    let bytes = tokio::fs::read(path).await?;

    if bytes.len() != SECRET_KEY_LENGTH {
        anyhow::bail!(
            "Invalid key file size: expected {}, got {}",
            SECRET_KEY_LENGTH,
            bytes.len()
        );
    }

    let mut secret_bytes = [0u8; SECRET_KEY_LENGTH];
    secret_bytes.copy_from_slice(&bytes);

    Ok(SigningKey::from_bytes(&secret_bytes))
}

/// Derive Seed ID from keypair (public key hash)
pub fn keypair_to_seed_id(keypair: &SigningKey) -> SeedId {
    let verifying_key = keypair.verifying_key();
    let mut seed_id = [0u8; 32];
    seed_id.copy_from_slice(verifying_key.as_bytes());
    seed_id
}

/// Get public key bytes from keypair
pub fn keypair_to_public_key(keypair: &SigningKey) -> PublicKey {
    let verifying_key = keypair.verifying_key();
    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(verifying_key.as_bytes());
    pubkey
}

// =============================================================================
// HEARTBEAT SIGNING & VERIFICATION
// =============================================================================

/// Create signature for a heartbeat message
pub fn sign_heartbeat(keypair: &SigningKey, message: &HeartbeatMessage) -> [u8; SIGNATURE_LENGTH] {
    let data = message.signing_data();
    let domain_data = domain_separate(DOMAIN_HEARTBEAT, &data);
    let signature = keypair.sign(&domain_data);
    signature.to_bytes()
}

/// Verify a heartbeat signature
pub fn verify_heartbeat(
    message: &HeartbeatMessage,
) -> Result<(), SignatureError> {
    // Reconstruct verifying key from peer_id (which is the public key)
    let verifying_key = VerifyingKey::from_bytes(&message.peer_id)
        .map_err(|_| SignatureError::InvalidPublicKey)?;

    let data = message.signing_data();
    let domain_data = domain_separate(DOMAIN_HEARTBEAT, &data);

    let signature = Signature::from_bytes(&message.signature);

    verifying_key
        .verify(&domain_data, &signature)
        .map_err(|_| SignatureError::InvalidSignature)
}

/// Verify heartbeat with a specific public key
pub fn verify_heartbeat_with_key(
    message: &HeartbeatMessage,
    public_key: &PublicKey,
) -> Result<(), SignatureError> {
    let verifying_key = VerifyingKey::from_bytes(public_key)
        .map_err(|_| SignatureError::InvalidPublicKey)?;

    let data = message.signing_data();
    let domain_data = domain_separate(DOMAIN_HEARTBEAT, &data);

    let signature = Signature::from_bytes(&message.signature);

    verifying_key
        .verify(&domain_data, &signature)
        .map_err(|_| SignatureError::InvalidSignature)
}

// =============================================================================
// IDPEERS FILE SIGNING & VERIFICATION
// =============================================================================

/// Sign an IDpeers file
pub fn sign_idpeers_file(keypair: &SigningKey, file: &IdPeersFile) -> [u8; SIGNATURE_LENGTH] {
    let data = file.signing_data();
    let domain_data = domain_separate(DOMAIN_IDPEERS, &data);
    let signature = keypair.sign(&domain_data);
    signature.to_bytes()
}

/// Verify an IDpeers file signature
pub fn verify_idpeers_file(file: &IdPeersFile) -> Result<(), SignatureError> {
    // The dns_seed_id is the public key
    let verifying_key = VerifyingKey::from_bytes(&file.dns_seed_id)
        .map_err(|_| SignatureError::InvalidPublicKey)?;

    let data = file.signing_data();
    let domain_data = domain_separate(DOMAIN_IDPEERS, &data);

    let signature = Signature::from_bytes(&file.signature);

    verifying_key
        .verify(&domain_data, &signature)
        .map_err(|_| SignatureError::InvalidSignature)
}

/// Verify IDpeers file with a specific seed ID (public key)
pub fn verify_idpeers_with_seed_id(
    file: &IdPeersFile,
    expected_seed_id: &SeedId,
) -> Result<(), SignatureError> {
    // Check seed ID matches
    if &file.dns_seed_id != expected_seed_id {
        return Err(SignatureError::SeedIdMismatch);
    }

    verify_idpeers_file(file)
}

// =============================================================================
// HELPERS
// =============================================================================

/// Apply domain separation to prevent cross-protocol replay attacks
fn domain_separate(domain: &[u8], data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(domain.len() + data.len());
    result.extend_from_slice(domain);
    result.extend_from_slice(data);
    result
}

/// Compute Blake3 hash
pub fn hash(data: &[u8]) -> Hash {
    let hash = blake3::hash(data);
    let mut result = [0u8; 32];
    result.copy_from_slice(hash.as_bytes());
    result
}

/// Hash to hex string
pub fn hash_to_hex(hash: &Hash) -> String {
    hex::encode(hash)
}

/// Parse hex string to hash
pub fn hex_to_hash(hex_str: &str) -> Result<Hash, hex::FromHexError> {
    let bytes = hex::decode(hex_str)?;
    if bytes.len() != 32 {
        return Err(hex::FromHexError::InvalidStringLength);
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(hash)
}

// =============================================================================
// ERRORS
// =============================================================================

#[derive(Debug, Clone, thiserror::Error)]
pub enum SignatureError {
    #[error("Invalid public key")]
    InvalidPublicKey,

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Seed ID mismatch")]
    SeedIdMismatch,
}

// =============================================================================
// OFFICIAL DNS SEED VERIFICATION
// =============================================================================

/// List of official DNS Seed public keys (will be populated via governance)
/// For now, this is empty - seeds are trusted by IP address
pub const OFFICIAL_SEED_PUBKEYS: &[&str] = &[
    // Add official seed public keys here as hex strings
    // Example: "a1b2c3d4e5f6..."
];

/// Check if a seed ID is in the official list
pub fn is_official_seed(seed_id: &SeedId) -> bool {
    let seed_hex = hex::encode(seed_id);
    OFFICIAL_SEED_PUBKEYS.contains(&seed_hex.as_str())
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = generate_keypair();
        let seed_id = keypair_to_seed_id(&keypair);
        assert_ne!(seed_id, [0u8; 32]);
    }

    #[test]
    fn test_heartbeat_signing() {
        let keypair = generate_keypair();
        let peer_id = keypair_to_public_key(&keypair);

        let mut message = HeartbeatMessage {
            version: 1,
            peer_id,
            libp2p_peer_id: "12D3KooWTestPeerId".to_string(),
            addresses: vec!["/ip4/1.2.3.4/tcp/30333".to_string()],
            current_height: 12345,
            best_hash: [0u8; 32],
            genesis_hash: [1u8; 32],
            is_validator: true,
            validator_count: Some(100),
            total_stake: Some(1_000_000_000_000_000),
            protocol_version: 1,
            timestamp: 1703318400,
            signature: [0u8; 64],
        };

        // Sign the message
        message.signature = sign_heartbeat(&keypair, &message);

        // Verify should succeed
        assert!(verify_heartbeat(&message).is_ok());

        // Modify message - verify should fail
        message.current_height = 99999;
        assert!(verify_heartbeat(&message).is_err());
    }

    #[test]
    fn test_domain_separation() {
        let data = b"test data";
        let domain1 = domain_separate(DOMAIN_HEARTBEAT, data);
        let domain2 = domain_separate(DOMAIN_IDPEERS, data);

        // Different domains should produce different results
        assert_ne!(domain1, domain2);
    }

    #[test]
    fn test_hash_functions() {
        let data = b"test data";
        let h = hash(data);

        let hex_str = hash_to_hex(&h);
        let parsed = hex_to_hash(&hex_str).unwrap();

        assert_eq!(h, parsed);
    }

    #[tokio::test]
    async fn test_keypair_save_load() {
        let temp_dir = tempfile::tempdir().unwrap();
        let key_path = temp_dir.path().join("test.key");

        let keypair = generate_keypair();
        let seed_id = keypair_to_seed_id(&keypair);

        // Save
        save_keypair(&keypair, &key_path).await.unwrap();

        // Load
        let loaded = load_keypair(&key_path).await.unwrap();
        let loaded_id = keypair_to_seed_id(&loaded);

        assert_eq!(seed_id, loaded_id);
    }
}
