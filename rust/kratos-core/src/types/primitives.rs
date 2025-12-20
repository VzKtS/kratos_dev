// Primitives KratOs - Types fondamentaux minimaux
use serde::{Deserialize, Serialize};
use std::fmt;

/// Hash universel (Blake3)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash([u8; 32]);

impl Hash {
    pub const ZERO: Hash = Hash([0u8; 32]);

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Hash(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Hash des données avec Blake3
    pub fn hash(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Hash(*hash.as_bytes())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.0[..8]))
    }
}

impl From<[u8; 32]> for Hash {
    fn from(bytes: [u8; 32]) -> Self {
        Hash(bytes)
    }
}

/// Numéro de bloc (u64 = ~584 milliards d'années à 1 bloc/sec)
pub type BlockNumber = u64;

/// Timestamp Unix en secondes
pub type Timestamp = u64;

/// Balance en KRAT (u128 = suffisant pour des siècles)
/// 1 KRAT = 10^12 units (1 trillion units)
pub type Balance = u128;

/// Constantes monétaires
pub const KRAT: Balance = 1_000_000_000_000; // 10^12
pub const MILLIKRAT: Balance = 1_000_000_000; // 10^9
pub const MICROKRAT: Balance = 1_000_000; // 10^6

/// Supply initiale: 1 milliard de KRAT
pub const INITIAL_SUPPLY: Balance = 1_000_000_000 * KRAT;

/// Dépôt existentiel minimum (évite spam)
pub const EXISTENTIAL_DEPOSIT: Balance = 1 * MILLIKRAT;

/// Nonce pour prévenir replay attacks
pub type Nonce = u64;

/// Epoch number (période de consensus, ex: 1 semaine)
pub type EpochNumber = u64;

/// Slot number dans une epoch
pub type SlotNumber = u64;

/// ChainId pour sidechains et hostchains
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChainId(pub u32);

impl ChainId {
    /// Root chain toujours ID 0
    pub const ROOT: ChainId = ChainId(0);

    pub fn is_root(&self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "chain:{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_deterministic() {
        let data = b"KratOs";
        let hash1 = Hash::hash(data);
        let hash2 = Hash::hash(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_krat_units() {
        assert_eq!(KRAT, 1_000_000_000_000);
        assert_eq!(1000 * MILLIKRAT, KRAT);
        assert_eq!(1_000_000 * MICROKRAT, KRAT);
    }

    #[test]
    fn test_chain_id_root() {
        assert!(ChainId::ROOT.is_root());
        assert!(!ChainId(1).is_root());
    }
}
