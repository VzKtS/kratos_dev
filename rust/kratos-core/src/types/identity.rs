// Identity Types - SPEC v4 Phase 0
// 4-Layer Identity System: L0 (Wallet) -> L1 (Local) -> L2 (PoP) -> L3 (Reputation)
//
// Design Philosophy:
// - Identity is OPTIONAL, not mandatory
// - Identity is LOCAL by default, not global
// - Identity is NON-TRANSFERABLE and REVOCABLE
// - Privacy is DEFAULT, not retroactive

use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash};
use serde::{Deserialize, Serialize};

/// Unique identifier for an identity (Blake3 hash of commitment data)
pub type IdentityId = Hash;

/// Identity expiry period (default: 180 days = ~2,592,000 blocks at 6s/block)
pub const DEFAULT_IDENTITY_EXPIRY: BlockNumber = 2_592_000;

/// Minimum attestations required for Active status
pub const MIN_ATTESTATIONS_FOR_ACTIVE: usize = 3;

/// Attestation expiry (90 days = ~1,296,000 blocks)
pub const ATTESTATION_EXPIRY: BlockNumber = 1_296_000;

/// Maximum attestations per identity
pub const MAX_ATTESTATIONS: usize = 100;

/// Reputation decay rate: 1% per week of inactivity (~100,800 blocks)
pub const REPUTATION_DECAY_INTERVAL: BlockNumber = 100_800;
pub const REPUTATION_DECAY_PERCENT: u8 = 1;

/// Maximum reputation score
pub const MAX_REPUTATION: u32 = 10_000;

/// VC boost formula: vc_boost = min(1.5, 1.0 + (reputation / 10000))
pub const MAX_VC_BOOST_MULTIPLIER: u32 = 150; // 1.5x in basis points (100 = 1.0x)

// =============================================================================
// LAYER 1: Local Identity (Sidechain-Scoped)
// =============================================================================

/// Identity commitment - core identity object (SPEC v4 Section 4.1)
///
/// A commitment is a cryptographic proof of identity without revealing
/// the underlying data. Only the hash is stored on-chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityCommitment {
    /// Unique identity ID (derived from commitment hash)
    pub identity_id: IdentityId,

    /// Owner account (L0 wallet identity)
    pub owner: AccountId,

    /// Chain scope - identity is only valid in this chain
    pub scope: ChainId,

    /// Cryptographic commitment (hash of identity data)
    /// The actual data is never stored on-chain
    pub commitment: Hash,

    /// Optional display name (public, non-unique)
    pub display_name: Option<String>,

    /// Block when identity was declared
    pub declared_at: BlockNumber,

    /// Block when identity expires (requires renewal)
    pub expires_at: BlockNumber,

    /// Current status
    pub status: IdentityStatus,

    /// Accumulated reputation score
    pub reputation: ReputationScore,

    /// Last activity block (for decay calculation)
    pub last_activity: BlockNumber,
}

impl IdentityCommitment {
    /// Create a new identity commitment
    pub fn new(
        owner: AccountId,
        scope: ChainId,
        commitment_data: &[u8],
        display_name: Option<String>,
        created_at: BlockNumber,
    ) -> Self {
        // Generate identity ID from commitment data
        let commitment = Hash::hash(commitment_data);
        let identity_id = Self::compute_identity_id(&owner, &scope, &commitment);

        Self {
            identity_id,
            owner,
            scope,
            commitment,
            display_name,
            declared_at: created_at,
            expires_at: created_at + DEFAULT_IDENTITY_EXPIRY,
            status: IdentityStatus::Declared,
            reputation: ReputationScore::default(),
            last_activity: created_at,
        }
    }

    /// Compute identity ID from components
    pub fn compute_identity_id(owner: &AccountId, scope: &ChainId, commitment: &Hash) -> IdentityId {
        let mut data = Vec::new();
        data.extend_from_slice(owner.as_bytes());
        data.extend_from_slice(&scope.0.to_le_bytes());
        data.extend_from_slice(commitment.as_bytes());
        Hash::hash(&data)
    }

    /// Check if identity is expired
    pub fn is_expired(&self, current_block: BlockNumber) -> bool {
        current_block > self.expires_at
    }

    /// Check if identity is active and valid
    pub fn is_active(&self, current_block: BlockNumber) -> bool {
        self.status == IdentityStatus::Active && !self.is_expired(current_block)
    }

    /// Renew identity expiry
    pub fn renew(&mut self, current_block: BlockNumber) {
        self.expires_at = current_block + DEFAULT_IDENTITY_EXPIRY;
    }

    /// Record activity (prevents reputation decay)
    pub fn record_activity(&mut self, current_block: BlockNumber) {
        self.last_activity = current_block;
    }

    /// Apply reputation decay based on inactivity
    pub fn apply_decay(&mut self, current_block: BlockNumber) {
        let inactive_blocks = current_block.saturating_sub(self.last_activity);
        let decay_periods = inactive_blocks / REPUTATION_DECAY_INTERVAL;

        if decay_periods > 0 {
            let decay_amount = (self.reputation.score as u64 * decay_periods * REPUTATION_DECAY_PERCENT as u64) / 100;
            self.reputation.score = self.reputation.score.saturating_sub(decay_amount as u32);
            self.last_activity = current_block;
        }
    }
}

/// Identity status lifecycle (SPEC v4 Section 5)
///
/// ```text
/// Declared → Attested → Active → Expired
///                ↓         ↓
///            Revoked   Revoked
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityStatus {
    /// Identity declared but not yet attested
    /// Requires MIN_ATTESTATIONS_FOR_ACTIVE to become Active
    Declared,

    /// Has some attestations but not enough for Active
    Attested,

    /// Fully active identity with sufficient attestations
    Active,

    /// Identity expired (can be renewed)
    Expired,

    /// Identity revoked (voluntary or enforced)
    /// Cannot be reactivated - must create new identity
    Revoked,
}

impl IdentityStatus {
    /// Check if identity can receive attestations
    pub fn can_receive_attestations(&self) -> bool {
        matches!(self, IdentityStatus::Declared | IdentityStatus::Attested | IdentityStatus::Active)
    }

    /// Check if identity can participate in governance
    pub fn can_participate(&self) -> bool {
        matches!(self, IdentityStatus::Active)
    }
}

// =============================================================================
// LAYER 2: Proof of Personhood (Optional)
// =============================================================================

/// Identity attestation - social proof of identity (SPEC v4 Section 4.2)
///
/// Attestations form a web-of-trust where existing identity holders
/// vouch for new identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityAttestation {
    /// Who is attesting
    pub attester: AccountId,

    /// Attester's identity ID (must be Active)
    pub attester_identity: IdentityId,

    /// Identity being attested
    pub target_identity: IdentityId,

    /// Hash of the attestation claim
    pub claim_hash: Hash,

    /// Attestation weight (based on attester's reputation)
    pub weight: u8,

    /// Block when attestation was made
    pub attested_at: BlockNumber,

    /// Block when attestation expires
    pub expires_at: BlockNumber,

    /// Whether attestation is still valid
    pub is_valid: bool,
}

impl IdentityAttestation {
    /// Create a new attestation
    pub fn new(
        attester: AccountId,
        attester_identity: IdentityId,
        target_identity: IdentityId,
        claim_data: &[u8],
        weight: u8,
        current_block: BlockNumber,
    ) -> Self {
        Self {
            attester,
            attester_identity,
            target_identity,
            claim_hash: Hash::hash(claim_data),
            weight,
            attested_at: current_block,
            expires_at: current_block + ATTESTATION_EXPIRY,
            is_valid: true,
        }
    }

    /// Check if attestation is expired
    pub fn is_expired(&self, current_block: BlockNumber) -> bool {
        current_block > self.expires_at
    }

    /// Check if attestation is currently valid
    pub fn is_active(&self, current_block: BlockNumber) -> bool {
        self.is_valid && !self.is_expired(current_block)
    }

    /// Revoke this attestation
    pub fn revoke(&mut self) {
        self.is_valid = false;
    }
}

/// Attestation weight calculation based on attester's reputation
pub fn calculate_attestation_weight(attester_reputation: u32) -> u8 {
    // Weight ranges from 1-10 based on reputation
    // 0-1000 rep = weight 1
    // 1001-2000 rep = weight 2
    // ...
    // 9001-10000 rep = weight 10
    let weight = (attester_reputation / 1000) as u8 + 1;
    weight.min(10)
}

// =============================================================================
// LAYER 3: Reputation (Derived)
// =============================================================================

/// Reputation score - derived from participation (SPEC v4 Section 3.4)
///
/// Reputation is earned through:
/// - Governance participation (voting)
/// - Arbitration service (jury duty)
/// - Validator uptime (integrated with VC)
/// - Account longevity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReputationScore {
    /// Overall reputation score (0 - MAX_REPUTATION)
    pub score: u32,

    /// Governance participation count
    pub governance_actions: u32,

    /// Arbitration participations
    pub arbitration_count: u32,

    /// Successful attestations given
    pub attestations_given: u32,

    /// Slashing events (reduces reputation)
    pub slash_count: u32,
}

impl ReputationScore {
    /// Create a new reputation score
    pub fn new() -> Self {
        Self::default()
    }

    /// Add reputation points (capped at MAX_REPUTATION)
    pub fn add(&mut self, points: u32) {
        self.score = self.score.saturating_add(points).min(MAX_REPUTATION);
    }

    /// Subtract reputation points (minimum 0)
    pub fn subtract(&mut self, points: u32) {
        self.score = self.score.saturating_sub(points);
    }

    /// Record governance participation
    pub fn record_governance(&mut self, points: u32) {
        self.governance_actions += 1;
        self.add(points);
    }

    /// Record arbitration participation
    pub fn record_arbitration(&mut self, points: u32) {
        self.arbitration_count += 1;
        self.add(points);
    }

    /// Record attestation given
    pub fn record_attestation(&mut self, points: u32) {
        self.attestations_given += 1;
        self.add(points);
    }

    /// Apply slashing penalty
    pub fn apply_slash(&mut self, penalty_percent: u8) {
        self.slash_count += 1;
        let penalty = (self.score as u64 * penalty_percent as u64) / 100;
        self.subtract(penalty as u32);
    }

    /// Calculate VC boost multiplier (in basis points, 100 = 1.0x)
    /// Formula: min(1.5, 1.0 + (reputation / 10000))
    pub fn vc_boost(&self) -> u32 {
        let boost = 100 + (self.score * 50 / MAX_REPUTATION);
        boost.min(MAX_VC_BOOST_MULTIPLIER)
    }

    /// Calculate vote weight multiplier (in basis points)
    /// Higher reputation = higher vote weight (capped)
    pub fn vote_weight(&self) -> u32 {
        // Base weight of 100 (1.0x), max of 200 (2.0x)
        let multiplier = 100 + (self.score * 100 / MAX_REPUTATION);
        multiplier.min(200)
    }
}

// =============================================================================
// IDENTITY EVENTS
// =============================================================================

/// Events emitted by the identity system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IdentityEvent {
    /// Identity declared
    IdentityDeclared {
        identity_id: IdentityId,
        owner: AccountId,
        scope: ChainId,
    },

    /// Attestation received
    AttestationReceived {
        identity_id: IdentityId,
        attester: AccountId,
        weight: u8,
    },

    /// Identity became active
    IdentityActivated {
        identity_id: IdentityId,
    },

    /// Identity expired
    IdentityExpired {
        identity_id: IdentityId,
    },

    /// Identity revoked
    IdentityRevoked {
        identity_id: IdentityId,
        reason: RevocationReason,
    },

    /// Identity renewed
    IdentityRenewed {
        identity_id: IdentityId,
        new_expiry: BlockNumber,
    },

    /// Reputation changed
    ReputationChanged {
        identity_id: IdentityId,
        old_score: u32,
        new_score: u32,
        reason: String,
    },

    /// Identity status changed (e.g., Active → Attested when attestations expire)
    StatusChanged {
        identity_id: IdentityId,
        old_status: IdentityStatus,
        new_status: IdentityStatus,
        reason: String,
    },
}

/// Reasons for identity revocation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevocationReason {
    /// Voluntary revocation by owner
    Voluntary,

    /// Revoked due to misconduct/slashing
    Misconduct,

    /// Revoked due to fraudulent attestations
    FraudulentAttestations,

    /// Revoked by governance decision
    GovernanceAction,
}

// =============================================================================
// ANTI-SYBIL CONFIGURATION
// =============================================================================

/// Anti-Sybil configuration per chain (SPEC v4 Section 7)
///
/// CONSTITUTIONAL COMPLIANCE (Article VI):
/// Identity is ALWAYS optional at the protocol level.
/// "Identity SHALL NEVER be required to: hold assets, transact, exit, fork"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiSybilConfig {
    /// Maximum voting power per identity (in basis points of total)
    pub max_voting_power_percent: u8,

    /// Minimum attestations for active status
    pub min_attestations: usize,

    /// Enable diminishing returns on reputation
    pub enable_diminishing_returns: bool,

    /// Reputation decay enabled
    pub enable_reputation_decay: bool,
}

impl AntiSybilConfig {
    /// CONSTITUTIONAL MANDATE: Identity is always optional
    /// This method always returns false per Article VI
    #[inline]
    pub fn require_identity_for_voting(&self) -> bool {
        // Article VI: "Identity SHALL NEVER be required to [...] transact"
        // Voting is a form of on-chain transaction, therefore identity cannot be required
        false
    }

    /// CONSTITUTIONAL MANDATE: Identity is always optional
    /// This method always returns false per Article VI
    #[inline]
    pub fn require_identity_for_proposals(&self) -> bool {
        // Article VI: "Identity SHALL NEVER be required to [...] transact"
        // Creating proposals is a form of on-chain transaction, therefore identity cannot be required
        false
    }
}

impl Default for AntiSybilConfig {
    fn default() -> Self {
        Self {
            max_voting_power_percent: 5,  // Max 5% of total voting power
            min_attestations: MIN_ATTESTATIONS_FOR_ACTIVE,
            enable_diminishing_returns: true,
            enable_reputation_decay: true,
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_account(seed: u8) -> AccountId {
        AccountId::from_bytes([seed; 32])
    }

    #[test]
    fn test_identity_commitment_creation() {
        let owner = create_test_account(1);
        let scope = ChainId(1);
        let commitment_data = b"test identity data";

        let identity = IdentityCommitment::new(
            owner,
            scope,
            commitment_data,
            Some("Alice".to_string()),
            1000,
        );

        assert_eq!(identity.owner, owner);
        assert_eq!(identity.scope, scope);
        assert_eq!(identity.status, IdentityStatus::Declared);
        assert!(identity.display_name.is_some());
        assert_eq!(identity.declared_at, 1000);
        assert_eq!(identity.expires_at, 1000 + DEFAULT_IDENTITY_EXPIRY);
    }

    #[test]
    fn test_identity_id_uniqueness() {
        let owner1 = create_test_account(1);
        let owner2 = create_test_account(2);
        let scope = ChainId(1);
        let data = b"same data";

        let id1 = IdentityCommitment::new(owner1, scope, data, None, 1000);
        let id2 = IdentityCommitment::new(owner2, scope, data, None, 1000);
        let id3 = IdentityCommitment::new(owner1, ChainId(2), data, None, 1000);

        // Different owners = different IDs
        assert_ne!(id1.identity_id, id2.identity_id);
        // Different scopes = different IDs
        assert_ne!(id1.identity_id, id3.identity_id);
    }

    #[test]
    fn test_identity_expiry() {
        let owner = create_test_account(1);
        let identity = IdentityCommitment::new(owner, ChainId(1), b"data", None, 1000);

        // Not expired initially
        assert!(!identity.is_expired(1000));
        assert!(!identity.is_expired(1000 + DEFAULT_IDENTITY_EXPIRY - 1));

        // Expired after expiry block
        assert!(identity.is_expired(1000 + DEFAULT_IDENTITY_EXPIRY + 1));
    }

    #[test]
    fn test_identity_renewal() {
        let owner = create_test_account(1);
        let mut identity = IdentityCommitment::new(owner, ChainId(1), b"data", None, 1000);

        let old_expiry = identity.expires_at;
        identity.renew(2000);

        assert_eq!(identity.expires_at, 2000 + DEFAULT_IDENTITY_EXPIRY);
        assert!(identity.expires_at > old_expiry);
    }

    #[test]
    fn test_identity_status_transitions() {
        assert!(IdentityStatus::Declared.can_receive_attestations());
        assert!(IdentityStatus::Attested.can_receive_attestations());
        assert!(IdentityStatus::Active.can_receive_attestations());
        assert!(!IdentityStatus::Expired.can_receive_attestations());
        assert!(!IdentityStatus::Revoked.can_receive_attestations());

        assert!(!IdentityStatus::Declared.can_participate());
        assert!(!IdentityStatus::Attested.can_participate());
        assert!(IdentityStatus::Active.can_participate());
        assert!(!IdentityStatus::Expired.can_participate());
        assert!(!IdentityStatus::Revoked.can_participate());
    }

    #[test]
    fn test_attestation_creation() {
        let attester = create_test_account(1);
        let attester_id = Hash::hash(b"attester");
        let target_id = Hash::hash(b"target");

        let attestation = IdentityAttestation::new(
            attester,
            attester_id,
            target_id,
            b"I vouch for this identity",
            5,
            1000,
        );

        assert_eq!(attestation.attester, attester);
        assert_eq!(attestation.weight, 5);
        assert!(attestation.is_valid);
        assert_eq!(attestation.expires_at, 1000 + ATTESTATION_EXPIRY);
    }

    #[test]
    fn test_attestation_expiry() {
        let attester = create_test_account(1);
        let attestation = IdentityAttestation::new(
            attester,
            Hash::hash(b"a"),
            Hash::hash(b"b"),
            b"claim",
            5,
            1000,
        );

        assert!(attestation.is_active(1000));
        assert!(attestation.is_active(1000 + ATTESTATION_EXPIRY - 1));
        assert!(!attestation.is_active(1000 + ATTESTATION_EXPIRY + 1));
    }

    #[test]
    fn test_attestation_revocation() {
        let attester = create_test_account(1);
        let mut attestation = IdentityAttestation::new(
            attester,
            Hash::hash(b"a"),
            Hash::hash(b"b"),
            b"claim",
            5,
            1000,
        );

        assert!(attestation.is_active(1000));

        attestation.revoke();

        assert!(!attestation.is_active(1000));
        assert!(!attestation.is_valid);
    }

    #[test]
    fn test_attestation_weight_calculation() {
        assert_eq!(calculate_attestation_weight(0), 1);
        assert_eq!(calculate_attestation_weight(500), 1);
        assert_eq!(calculate_attestation_weight(1000), 2);
        assert_eq!(calculate_attestation_weight(1500), 2);
        assert_eq!(calculate_attestation_weight(5000), 6);
        assert_eq!(calculate_attestation_weight(9500), 10);
        assert_eq!(calculate_attestation_weight(10000), 10);
        assert_eq!(calculate_attestation_weight(15000), 10); // Capped
    }

    #[test]
    fn test_reputation_score_add() {
        let mut rep = ReputationScore::new();
        assert_eq!(rep.score, 0);

        rep.add(100);
        assert_eq!(rep.score, 100);

        rep.add(MAX_REPUTATION);
        assert_eq!(rep.score, MAX_REPUTATION); // Capped
    }

    #[test]
    fn test_reputation_score_subtract() {
        let mut rep = ReputationScore::new();
        rep.score = 100;

        rep.subtract(30);
        assert_eq!(rep.score, 70);

        rep.subtract(100);
        assert_eq!(rep.score, 0); // Minimum 0
    }

    #[test]
    fn test_reputation_governance() {
        let mut rep = ReputationScore::new();

        rep.record_governance(50);
        assert_eq!(rep.score, 50);
        assert_eq!(rep.governance_actions, 1);

        rep.record_governance(50);
        assert_eq!(rep.score, 100);
        assert_eq!(rep.governance_actions, 2);
    }

    #[test]
    fn test_reputation_arbitration() {
        let mut rep = ReputationScore::new();

        rep.record_arbitration(100);
        assert_eq!(rep.score, 100);
        assert_eq!(rep.arbitration_count, 1);
    }

    #[test]
    fn test_reputation_slash() {
        let mut rep = ReputationScore::new();
        rep.score = 1000;

        rep.apply_slash(10); // 10% penalty
        assert_eq!(rep.score, 900);
        assert_eq!(rep.slash_count, 1);

        rep.apply_slash(50); // 50% penalty
        assert_eq!(rep.score, 450);
        assert_eq!(rep.slash_count, 2);
    }

    #[test]
    fn test_reputation_vc_boost() {
        let mut rep = ReputationScore::new();

        // 0 reputation = 1.0x boost (100 basis points)
        assert_eq!(rep.vc_boost(), 100);

        // 5000 reputation = 1.25x boost
        rep.score = 5000;
        assert_eq!(rep.vc_boost(), 125);

        // MAX reputation = 1.5x boost (capped)
        rep.score = MAX_REPUTATION;
        assert_eq!(rep.vc_boost(), MAX_VC_BOOST_MULTIPLIER);
    }

    #[test]
    fn test_reputation_vote_weight() {
        let mut rep = ReputationScore::new();

        // 0 reputation = 1.0x weight
        assert_eq!(rep.vote_weight(), 100);

        // 5000 reputation = 1.5x weight
        rep.score = 5000;
        assert_eq!(rep.vote_weight(), 150);

        // MAX reputation = 2.0x weight (capped)
        rep.score = MAX_REPUTATION;
        assert_eq!(rep.vote_weight(), 200);
    }

    #[test]
    fn test_identity_decay() {
        let owner = create_test_account(1);
        let mut identity = IdentityCommitment::new(owner, ChainId(1), b"data", None, 1000);
        identity.reputation.score = 1000;

        // No decay if active recently
        identity.apply_decay(1000 + REPUTATION_DECAY_INTERVAL - 1);
        assert_eq!(identity.reputation.score, 1000);

        // 1% decay after one interval
        identity.last_activity = 1000;
        identity.apply_decay(1000 + REPUTATION_DECAY_INTERVAL);
        assert_eq!(identity.reputation.score, 990);

        // 2% decay after two intervals
        identity.last_activity = 1000;
        identity.reputation.score = 1000;
        identity.apply_decay(1000 + REPUTATION_DECAY_INTERVAL * 2);
        assert_eq!(identity.reputation.score, 980);
    }

    #[test]
    fn test_anti_sybil_config_default() {
        let config = AntiSybilConfig::default();

        assert_eq!(config.max_voting_power_percent, 5);
        assert_eq!(config.min_attestations, MIN_ATTESTATIONS_FOR_ACTIVE);
        // CONSTITUTIONAL: Identity is always optional (Article VI)
        assert!(!config.require_identity_for_voting());
        assert!(!config.require_identity_for_proposals());
        assert!(config.enable_reputation_decay);
    }

    #[test]
    fn test_identity_is_active() {
        let owner = create_test_account(1);
        let mut identity = IdentityCommitment::new(owner, ChainId(1), b"data", None, 1000);

        // Not active when Declared
        assert!(!identity.is_active(1000));

        // Active when status is Active and not expired
        identity.status = IdentityStatus::Active;
        assert!(identity.is_active(1000));
        assert!(identity.is_active(1000 + DEFAULT_IDENTITY_EXPIRY - 1));

        // Not active when expired
        assert!(!identity.is_active(1000 + DEFAULT_IDENTITY_EXPIRY + 1));
    }

    #[test]
    fn test_revocation_reasons() {
        let reason1 = RevocationReason::Voluntary;
        let reason2 = RevocationReason::Misconduct;
        let reason3 = RevocationReason::FraudulentAttestations;
        let reason4 = RevocationReason::GovernanceAction;

        assert_eq!(reason1, RevocationReason::Voluntary);
        assert_ne!(reason1, reason2);
        assert_ne!(reason2, reason3);
        assert_ne!(reason3, reason4);
    }
}
