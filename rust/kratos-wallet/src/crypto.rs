// Cryptographic operations for wallet
// - Key generation and management
// - Transaction signing
// - Wallet encryption/decryption

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::types::SignedTransaction;

/// Domain separator for transaction signatures (must match kratos-core)
const DOMAIN_TRANSACTION: &[u8] = b"KRATOS_TRANSACTION_V1:";

/// Create a domain-separated message for signing
#[inline]
fn domain_separate(domain: &[u8], message: &[u8]) -> Vec<u8> {
    let mut separated = Vec::with_capacity(domain.len() + message.len());
    separated.extend_from_slice(domain);
    separated.extend_from_slice(message);
    separated
}

/// Wallet keys (secret + public)
pub struct WalletKeys {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl WalletKeys {
    /// Generate new random keys
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Create from existing secret key bytes
    pub fn from_secret(secret: [u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(&secret);
        let verifying_key = signing_key.verifying_key();

        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Get account ID (public key) as hex string
    pub fn account_id_hex(&self) -> String {
        hex::encode(self.verifying_key.to_bytes())
    }

    /// Get account ID as bytes
    pub fn account_id_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }

    /// Get secret key as hex string (USE WITH CAUTION)
    pub fn secret_key_hex(&self) -> String {
        hex::encode(self.signing_key.to_bytes())
    }

    /// Get secret key bytes for encryption
    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// Sign a message
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        let signature = self.signing_key.sign(message);
        signature.to_bytes()
    }

    /// Create and sign a transfer transaction
    pub fn create_transfer(&self, to: [u8; 32], amount: u128, nonce: u64) -> SignedTransaction {
        let transaction = crate::types::Transaction {
            sender: self.account_id_bytes().into(),
            nonce,
            call: crate::types::TransactionCall::Transfer { to: to.into(), amount },
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Serialize for signing with domain separation
        let tx_bytes = bincode::serialize(&transaction).unwrap();
        let message = domain_separate(DOMAIN_TRANSACTION, &tx_bytes);
        let signature = self.sign(&message);

        SignedTransaction {
            transaction,
            signature,
        }
    }

    /// Create and sign a propose early validator transaction
    pub fn create_propose_early_validator(&self, candidate: [u8; 32], nonce: u64) -> SignedTransaction {
        let transaction = crate::types::Transaction {
            sender: self.account_id_bytes().into(),
            nonce,
            call: crate::types::TransactionCall::ProposeEarlyValidator { candidate: candidate.into() },
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Serialize for signing with domain separation
        let tx_bytes = bincode::serialize(&transaction).unwrap();
        let message = domain_separate(DOMAIN_TRANSACTION, &tx_bytes);
        let signature = self.sign(&message);

        SignedTransaction {
            transaction,
            signature,
        }
    }

    /// Create and sign a vote early validator transaction
    pub fn create_vote_early_validator(&self, candidate: [u8; 32], nonce: u64) -> SignedTransaction {
        let transaction = crate::types::Transaction {
            sender: self.account_id_bytes().into(),
            nonce,
            call: crate::types::TransactionCall::VoteEarlyValidator { candidate: candidate.into() },
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Serialize for signing with domain separation
        let tx_bytes = bincode::serialize(&transaction).unwrap();
        let message = domain_separate(DOMAIN_TRANSACTION, &tx_bytes);
        let signature = self.sign(&message);

        SignedTransaction {
            transaction,
            signature,
        }
    }
}

/// Encrypted wallet data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedWallet {
    /// Encrypted secret key (32 bytes + AES-GCM tag)
    pub encrypted_secret: Vec<u8>,
    /// Salt for key derivation
    pub salt: String,
    /// Nonce for AES-GCM
    pub nonce: [u8; 12],
    /// Public key (for verification)
    pub public_key: [u8; 32],
    /// RPC endpoint
    pub rpc_url: String,
    /// Version for future compatibility
    pub version: u32,
}

/// Derive encryption key from password using Argon2
pub fn derive_key(password: &str, salt: &str) -> [u8; 32] {
    let argon2 = Argon2::default();

    // Parse salt
    let salt = SaltString::from_b64(salt).expect("Invalid salt");

    // Hash password
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .expect("Failed to hash password");

    // Get hash output
    let hash_bytes = hash.hash.expect("No hash output");

    let mut key = [0u8; 32];
    key.copy_from_slice(&hash_bytes.as_bytes()[..32]);
    key
}

/// Generate a new salt
pub fn generate_salt() -> String {
    SaltString::generate(&mut OsRng).as_str().to_string()
}

/// Encrypt secret key with password
pub fn encrypt_secret(secret: &[u8; 32], password: &str) -> Result<EncryptedWallet, String> {
    // Generate salt and derive key
    let salt = generate_salt();
    let key = derive_key(password, &salt);

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher error: {}", e))?;

    // Generate random nonce
    let mut nonce_bytes = [0u8; 12];
    rand::Rng::fill(&mut OsRng, &mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt
    let encrypted = cipher
        .encrypt(nonce, secret.as_ref())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    // Get public key from secret
    let signing_key = SigningKey::from_bytes(secret);
    let public_key = signing_key.verifying_key().to_bytes();

    Ok(EncryptedWallet {
        encrypted_secret: encrypted,
        salt,
        nonce: nonce_bytes,
        public_key,
        rpc_url: String::new(), // Will be set by caller
        version: 1,
    })
}

/// Decrypt secret key with password
pub fn decrypt_secret(wallet: &EncryptedWallet, password: &str) -> Result<[u8; 32], String> {
    // Derive key from password
    let key = derive_key(password, &wallet.salt);

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher error: {}", e))?;

    // Create nonce
    let nonce = Nonce::from_slice(&wallet.nonce);

    // Decrypt
    let decrypted = cipher
        .decrypt(nonce, wallet.encrypted_secret.as_ref())
        .map_err(|_| "Invalid password or corrupted wallet")?;

    if decrypted.len() != 32 {
        return Err("Invalid decrypted key length".to_string());
    }

    let mut secret = [0u8; 32];
    secret.copy_from_slice(&decrypted);

    // Verify public key matches
    let signing_key = SigningKey::from_bytes(&secret);
    let public_key = signing_key.verifying_key().to_bytes();

    if public_key != wallet.public_key {
        return Err("Key verification failed".to_string());
    }

    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let keys = WalletKeys::generate();
        assert_eq!(keys.account_id_hex().len(), 64);
        assert_eq!(keys.secret_key_hex().len(), 64);
    }

    #[test]
    fn test_key_from_secret() {
        let secret = [1u8; 32];
        let keys = WalletKeys::from_secret(secret);

        let keys2 = WalletKeys::from_secret(secret);
        assert_eq!(keys.account_id_hex(), keys2.account_id_hex());
    }

    #[test]
    fn test_signing() {
        let keys = WalletKeys::generate();
        let message = b"test message";
        let signature = keys.sign(message);
        assert_eq!(signature.len(), 64);
    }

    #[test]
    fn test_encrypt_decrypt() {
        let secret = [42u8; 32];
        let password = "test_password_123";

        let encrypted = encrypt_secret(&secret, password).unwrap();
        let decrypted = decrypt_secret(&encrypted, password).unwrap();

        assert_eq!(secret, decrypted);
    }

    #[test]
    fn test_wrong_password() {
        let secret = [42u8; 32];
        let password = "correct_password";
        let wrong_password = "wrong_password";

        let encrypted = encrypt_secret(&secret, password).unwrap();
        let result = decrypt_secret(&encrypted, wrong_password);

        assert!(result.is_err());
    }
}
