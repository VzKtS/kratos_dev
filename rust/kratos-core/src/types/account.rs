// Account - Système de comptes minimal
use super::primitives::{Balance, Hash, Nonce};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::fmt;

/// AccountId = clé publique Ed25519 (32 bytes)
/// Principe: Pas d'identité, juste des clés
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AccountId([u8; 32]);

impl AccountId {
    pub fn from_public_key(key: &VerifyingKey) -> Self {
        AccountId(key.to_bytes())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        AccountId(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Vérifie une signature
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> bool {
        let public_key = match VerifyingKey::from_bytes(&self.0) {
            Ok(pk) => pk,
            Err(_) => return false,
        };

        let sig = Signature::from_bytes(signature);

        public_key.verify(message, &sig).is_ok()
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0[..8]))
    }
}

impl From<[u8; 32]> for AccountId {
    fn from(bytes: [u8; 32]) -> Self {
        AccountId(bytes)
    }
}

/// État d'un compte dans le ledger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    /// Nonce pour prévenir replay
    pub nonce: Nonce,

    /// Balance libre
    pub free: Balance,

    /// Balance réservée (staking, dépôts)
    pub reserved: Balance,

    /// Hash du dernier bloc où ce compte a été modifié
    pub last_modified: Hash,
}

impl AccountInfo {
    pub fn new() -> Self {
        Self {
            nonce: 0,
            free: 0,
            reserved: 0,
            last_modified: Hash::ZERO,
        }
    }

    /// Balance totale
    pub fn total(&self) -> Balance {
        self.free.saturating_add(self.reserved)
    }

    /// Peut transférer ce montant?
    pub fn can_transfer(&self, amount: Balance) -> bool {
        self.free >= amount
    }

    /// Réserve une balance (pour staking, dépôts, etc.)
    pub fn reserve(&mut self, amount: Balance) -> Result<(), AccountError> {
        if self.free < amount {
            return Err(AccountError::InsufficientBalance);
        }
        self.free = self.free.saturating_sub(amount);
        self.reserved = self.reserved.saturating_add(amount);
        Ok(())
    }

    /// Libère une balance réservée
    pub fn unreserve(&mut self, amount: Balance) -> Result<(), AccountError> {
        if self.reserved < amount {
            return Err(AccountError::InsufficientReserved);
        }
        self.reserved = self.reserved.saturating_sub(amount);
        self.free = self.free.saturating_add(amount);
        Ok(())
    }

    /// Slash (confisque) une balance réservée
    pub fn slash_reserved(&mut self, amount: Balance) -> Balance {
        let slashed = amount.min(self.reserved);
        self.reserved = self.reserved.saturating_sub(slashed);
        slashed
    }
}

impl Default for AccountInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// Erreurs de compte
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("Balance insuffisante")]
    InsufficientBalance,

    #[error("Balance réservée insuffisante")]
    InsufficientReserved,

    #[error("Compte inexistant")]
    NotFound,

    #[error("Dépôt existentiel non atteint")]
    BelowExistentialDeposit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_reserve() {
        let mut account = AccountInfo::new();
        account.free = 1000;

        assert!(account.reserve(500).is_ok());
        assert_eq!(account.free, 500);
        assert_eq!(account.reserved, 500);
        assert_eq!(account.total(), 1000);
    }

    #[test]
    fn test_account_unreserve() {
        let mut account = AccountInfo::new();
        account.reserved = 1000;

        assert!(account.unreserve(300).is_ok());
        assert_eq!(account.free, 300);
        assert_eq!(account.reserved, 700);
    }

    #[test]
    fn test_account_slash() {
        let mut account = AccountInfo::new();
        account.reserved = 1000;

        let slashed = account.slash_reserved(400);
        assert_eq!(slashed, 400);
        assert_eq!(account.reserved, 600);
    }
}
