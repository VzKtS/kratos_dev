// Signature wrapper pour sérialisation
use serde::{Deserialize, Deserializer, Serialize, Serializer};

// =============================================================================
// SECURITY FIX #24: Domain separation constants for signatures
// =============================================================================
//
// Domain separation prevents signature replay attacks between different contexts.
// Each signature type uses a unique prefix that is prepended to the message
// before signing/verification.
//
// Per IETF recommendations and crypto best practices, all signatures should
// include a domain separator to prevent cross-protocol attacks.
// =============================================================================

/// Domain separator for block header signatures
/// Used when validators sign block headers
pub const DOMAIN_BLOCK_HEADER: &[u8] = b"KRATOS_BLOCK_HEADER_V1:";

/// Domain separator for transaction signatures
/// Used when users sign transactions
pub const DOMAIN_TRANSACTION: &[u8] = b"KRATOS_TRANSACTION_V1:";

/// Domain separator for VRF proofs
/// Used in VRF-based validator selection
pub const DOMAIN_VRF_PROOF: &[u8] = b"KRATOS_VRF_PROOF_V1:";

/// Domain separator for governance votes
/// Used when validators vote on proposals
pub const DOMAIN_GOVERNANCE_VOTE: &[u8] = b"KRATOS_GOVERNANCE_V1:";

/// Domain separator for arbitration decisions
/// Used when jury members sign arbitration verdicts
pub const DOMAIN_ARBITRATION: &[u8] = b"KRATOS_ARBITRATION_V1:";

// =============================================================================
// SECURITY FIX #36: Additional domain separators for complete coverage
// =============================================================================

/// Domain separator for sidechain state roots
/// Used when committing state roots for cross-chain verification
pub const DOMAIN_STATE_ROOT: &[u8] = b"KRATOS_STATE_ROOT_V1:";

/// Domain separator for Merkle proofs
/// Used when creating proofs for cross-chain verification
pub const DOMAIN_MERKLE_PROOF: &[u8] = b"KRATOS_MERKLE_PROOF_V1:";

/// Domain separator for emergency exit requests
/// Used when users request emergency withdrawal
pub const DOMAIN_EMERGENCY_EXIT: &[u8] = b"KRATOS_EMERGENCY_EXIT_V1:";

/// Domain separator for slashing evidence
/// Used when submitting slashing evidence
pub const DOMAIN_SLASHING_EVIDENCE: &[u8] = b"KRATOS_SLASHING_V1:";

/// Domain separator for dispute submissions
/// Used when filing disputes for arbitration
pub const DOMAIN_DISPUTE: &[u8] = b"KRATOS_DISPUTE_V1:";

/// Domain separator for staking operations
/// Used when delegating or undelegating stake
pub const DOMAIN_STAKING: &[u8] = b"KRATOS_STAKING_V1:";

/// Domain separator for cross-chain messages
/// Used in cross-chain message authentication
pub const DOMAIN_CROSS_CHAIN_MSG: &[u8] = b"KRATOS_XCHAIN_MSG_V1:";

/// Domain separator for finality justifications
/// Used when validators sign finality votes (GRANDPA-like)
/// SECURITY FIX #33: Prevents finality signature replay
pub const DOMAIN_FINALITY: &[u8] = b"KRATOS_FINALITY_V1:";

/// Create a domain-separated message for signing
///
/// # Arguments
/// * `domain` - The domain separator (e.g., DOMAIN_BLOCK_HEADER)
/// * `message` - The message to sign
///
/// # Returns
/// A new Vec<u8> with domain prefix prepended to message
#[inline]
pub fn domain_separate(domain: &[u8], message: &[u8]) -> Vec<u8> {
    let mut separated = Vec::with_capacity(domain.len() + message.len());
    separated.extend_from_slice(domain);
    separated.extend_from_slice(message);
    separated
}

/// Wrapper pour signatures Ed25519 (64 bytes) avec sérialisation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature64(pub [u8; 64]);

impl Signature64 {
    pub fn new(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }

    pub fn zero() -> Self {
        Self([0; 64])
    }
}

impl From<[u8; 64]> for Signature64 {
    fn from(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for Signature64 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

// Sérialisation manuelle
impl Serialize for Signature64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Signature64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes = <Vec<u8>>::deserialize(deserializer)?;
        if bytes.len() != 64 {
            return Err(serde::de::Error::custom("Signature must be 64 bytes"));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&bytes);
        Ok(Signature64(arr))
    }
}
