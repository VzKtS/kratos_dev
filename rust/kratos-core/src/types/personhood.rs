// Proof of Personhood Types - SPEC v4 Layer 2
//
// Implements advanced Sybil resistance through:
// - Social graph analysis (web-of-trust)
// - Periodic challenges for identity verification
// - Uniqueness proofs (commitment-based)
// - Liveness detection

use crate::types::identity::{IdentityId, ReputationScore, calculate_attestation_weight};
use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// =============================================================================
// CONSTANTS
// =============================================================================

/// Challenge period (7 days = ~100,800 blocks at 6s/block)
pub const CHALLENGE_PERIOD: BlockNumber = 100_800;

/// Grace period to respond to challenge (3 days = ~43,200 blocks)
pub const CHALLENGE_GRACE_PERIOD: BlockNumber = 43_200;

/// Minimum time between challenges (30 days = ~432,000 blocks)
pub const MIN_CHALLENGE_INTERVAL: BlockNumber = 432_000;

/// Challenge deposit (returned on successful response)
pub const CHALLENGE_DEPOSIT: Balance = 50;

/// Penalty for failed challenge (paid to challenger)
pub const CHALLENGE_FAILURE_PENALTY: Balance = 100;

/// Maximum simultaneous challenges per identity
pub const MAX_CHALLENGES_PER_IDENTITY: usize = 3;

/// Minimum distinct attesters for uniqueness verification
pub const UNIQUENESS_MIN_ATTESTERS: usize = 5;

/// Maximum attestation network depth for social graph
pub const MAX_TRUST_DEPTH: usize = 3;

/// Minimum social graph connectivity score
pub const MIN_CONNECTIVITY_SCORE: u32 = 100;

// =============================================================================
// CHALLENGE TYPES
// =============================================================================

/// Types of challenges that can be issued
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChallengeType {
    /// Standard liveness check - must sign a message
    Liveness {
        /// Random nonce to sign
        nonce: Hash,
    },

    /// Social verification - requires attestations from existing members
    SocialVerification {
        /// Required number of attestations
        required_attestations: usize,
    },

    /// Uniqueness challenge - prove not a duplicate identity
    UniquenessChallenge {
        /// Suspected duplicate identity
        suspected_duplicate: IdentityId,
    },

    /// Stake challenge - lock additional stake to prove commitment
    StakeChallenge {
        /// Required stake amount
        required_stake: Balance,
    },
}

impl ChallengeType {
    /// Get the difficulty level (affects response requirements)
    pub fn difficulty(&self) -> u8 {
        match self {
            ChallengeType::Liveness { .. } => 1,
            ChallengeType::SocialVerification { required_attestations } => {
                (*required_attestations as u8).min(10)
            }
            ChallengeType::UniquenessChallenge { .. } => 5,
            ChallengeType::StakeChallenge { .. } => 3,
        }
    }
}

/// A challenge issued to verify identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonhoodChallenge {
    /// Unique challenge ID
    pub challenge_id: Hash,

    /// Identity being challenged
    pub target_identity: IdentityId,

    /// Who issued the challenge
    pub challenger: AccountId,

    /// Type of challenge
    pub challenge_type: ChallengeType,

    /// Block when challenge was issued
    pub issued_at: BlockNumber,

    /// Block when challenge expires
    pub expires_at: BlockNumber,

    /// Deposit locked by challenger
    pub deposit: Balance,

    /// Current status
    pub status: ChallengeStatus,

    /// Response if any
    pub response: Option<ChallengeResponse>,
}

impl PersonhoodChallenge {
    /// Create a new challenge
    pub fn new(
        target_identity: IdentityId,
        challenger: AccountId,
        challenge_type: ChallengeType,
        deposit: Balance,
        current_block: BlockNumber,
    ) -> Self {
        let challenge_id = Self::compute_id(&target_identity, &challenger, current_block);

        Self {
            challenge_id,
            target_identity,
            challenger,
            challenge_type,
            issued_at: current_block,
            expires_at: current_block + CHALLENGE_PERIOD,
            deposit,
            status: ChallengeStatus::Pending,
            response: None,
        }
    }

    /// Compute challenge ID
    fn compute_id(target: &IdentityId, challenger: &AccountId, block: BlockNumber) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(target.as_bytes());
        data.extend_from_slice(challenger.as_bytes());
        data.extend_from_slice(&block.to_le_bytes());
        Hash::hash(&data)
    }

    /// Check if challenge is expired
    pub fn is_expired(&self, current_block: BlockNumber) -> bool {
        current_block > self.expires_at
    }

    /// Check if in grace period
    pub fn in_grace_period(&self, current_block: BlockNumber) -> bool {
        let grace_start = self.expires_at.saturating_sub(CHALLENGE_GRACE_PERIOD);
        current_block >= grace_start && current_block <= self.expires_at
    }
}

/// Challenge status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChallengeStatus {
    /// Challenge issued, awaiting response
    Pending,

    /// Response submitted, being verified
    ResponseSubmitted,

    /// Challenge passed successfully
    Passed,

    /// Challenge failed (identity penalized)
    Failed,

    /// Challenge expired without response
    Expired,

    /// Challenge cancelled by challenger
    Cancelled,
}

/// Response to a challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResponse {
    /// Block when response was submitted
    pub submitted_at: BlockNumber,

    /// Response data (signature, attestations, etc.)
    pub response_data: ChallengeResponseData,
}

/// Challenge response data variants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChallengeResponseData {
    /// Signed nonce for liveness
    LivenessProof {
        signature: Hash,
    },

    /// Attestations received for social verification
    SocialProof {
        attestation_ids: Vec<Hash>,
    },

    /// Proof of non-duplication
    UniquenessProof {
        /// Hash of uniqueness proof data
        proof_hash: Hash,
        /// Witnesses who verified
        witnesses: Vec<AccountId>,
    },

    /// Stake locked for stake challenge
    StakeProof {
        stake_amount: Balance,
        lock_until: BlockNumber,
    },
}

// =============================================================================
// SOCIAL GRAPH ANALYSIS
// =============================================================================

/// Social graph node representing identity connections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialGraphNode {
    /// Identity ID
    pub identity_id: IdentityId,

    /// Direct connections (attesters/attestees)
    pub connections: HashSet<IdentityId>,

    /// Incoming attestations (others attesting this identity)
    pub incoming: HashSet<IdentityId>,

    /// Outgoing attestations (this identity attesting others)
    pub outgoing: HashSet<IdentityId>,

    /// Computed connectivity score
    pub connectivity_score: u32,

    /// Cluster membership (for Sybil detection)
    pub cluster_id: Option<u32>,

    /// Trust depth from root (genesis identities)
    pub trust_depth: u8,
}

impl SocialGraphNode {
    /// Create a new graph node
    pub fn new(identity_id: IdentityId) -> Self {
        Self {
            identity_id,
            connections: HashSet::new(),
            incoming: HashSet::new(),
            outgoing: HashSet::new(),
            connectivity_score: 0,
            cluster_id: None,
            trust_depth: u8::MAX,
        }
    }

    /// Add a connection
    pub fn add_connection(&mut self, other: IdentityId, is_incoming: bool) {
        self.connections.insert(other);
        if is_incoming {
            self.incoming.insert(other);
        } else {
            self.outgoing.insert(other);
        }
    }

    /// Calculate connectivity score
    pub fn calculate_connectivity(&mut self, total_identities: u64) {
        // Score based on:
        // 1. Number of unique connections
        // 2. Balance between incoming/outgoing
        // 3. Trust depth

        let connection_count = self.connections.len() as u32;
        let incoming_count = self.incoming.len() as u32;
        let outgoing_count = self.outgoing.len() as u32;

        // Base score from connections
        let base = connection_count * 10;

        // Balance bonus (prefer balanced in/out ratio)
        let balance_ratio = if incoming_count == 0 || outgoing_count == 0 {
            0
        } else {
            let min = incoming_count.min(outgoing_count);
            let max = incoming_count.max(outgoing_count);
            (min * 100 / max).min(100)
        };
        let balance_bonus = balance_ratio / 2;

        // Trust depth penalty (deeper = less trusted)
        let depth_penalty = (self.trust_depth as u32 * 5).min(50);

        // Network share bonus (more connections relative to network = higher score)
        let network_share = if total_identities > 0 {
            (connection_count as u64 * 100 / total_identities) as u32
        } else {
            0
        };

        self.connectivity_score = base + balance_bonus + network_share - depth_penalty;
    }

    /// Check if this node appears to be a Sybil (suspicious patterns)
    pub fn is_suspicious(&self) -> bool {
        // Suspicious if:
        // 1. Only outgoing attestations (trying to boost others without being verified)
        // 2. Very low connectivity despite being "active"
        // 3. All connections in same cluster

        if self.incoming.is_empty() && !self.outgoing.is_empty() {
            return true;
        }

        if self.connectivity_score < MIN_CONNECTIVITY_SCORE && !self.connections.is_empty() {
            return true;
        }

        false
    }
}

/// Sybil cluster - group of potentially coordinated identities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SybilCluster {
    /// Cluster ID
    pub cluster_id: u32,

    /// Members of this cluster
    pub members: HashSet<IdentityId>,

    /// Internal connection density (high = suspicious)
    pub internal_density: f32,

    /// External connection count (low = suspicious)
    pub external_connections: u32,

    /// Risk score (0-100)
    pub risk_score: u8,

    /// Flagged for review
    pub flagged: bool,
}

impl SybilCluster {
    /// Create a new cluster
    pub fn new(cluster_id: u32) -> Self {
        Self {
            cluster_id,
            members: HashSet::new(),
            internal_density: 0.0,
            external_connections: 0,
            risk_score: 0,
            flagged: false,
        }
    }

    /// Add a member to the cluster
    pub fn add_member(&mut self, identity: IdentityId) {
        self.members.insert(identity);
    }

    /// Calculate risk score
    pub fn calculate_risk(&mut self) {
        let member_count = self.members.len();

        if member_count < 3 {
            self.risk_score = 0;
            return;
        }

        // High internal density + low external connections = high risk
        let density_risk = (self.internal_density * 50.0) as u8;
        let isolation_risk = if self.external_connections < member_count as u32 {
            30
        } else {
            0
        };
        let size_risk = if member_count > 10 { 20 } else { 0 };

        self.risk_score = (density_risk + isolation_risk + size_risk).min(100);
        self.flagged = self.risk_score > 70;
    }
}

// =============================================================================
// UNIQUENESS VERIFICATION
// =============================================================================

/// Uniqueness commitment - proof that identity is unique
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniquenessCommitment {
    /// Identity ID
    pub identity_id: IdentityId,

    /// Commitment hash (hash of unique identifier + salt)
    pub commitment: Hash,

    /// Block when commitment was made
    pub committed_at: BlockNumber,

    /// Verification status
    pub verified: bool,

    /// Witnesses who verified (minimum 3)
    pub witnesses: Vec<AccountId>,
}

impl UniquenessCommitment {
    /// Create a new uniqueness commitment
    pub fn new(
        identity_id: IdentityId,
        commitment_data: &[u8],
        current_block: BlockNumber,
    ) -> Self {
        Self {
            identity_id,
            commitment: Hash::hash(commitment_data),
            committed_at: current_block,
            verified: false,
            witnesses: Vec::new(),
        }
    }

    /// Add a witness
    pub fn add_witness(&mut self, witness: AccountId) -> bool {
        if self.witnesses.contains(&witness) {
            return false;
        }
        self.witnesses.push(witness);

        // Auto-verify if enough witnesses
        if self.witnesses.len() >= 3 {
            self.verified = true;
        }

        true
    }
}

// =============================================================================
// PERSONHOOD SCORE
// =============================================================================

/// Comprehensive personhood score combining all verification factors
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct PersonhoodScore {
    /// Overall score (0-1000)
    pub total: u32,

    /// Social verification score (0-200)
    pub social_score: u32,

    /// Challenge history score (0-200)
    pub challenge_score: u32,

    /// Uniqueness score (0-200)
    pub uniqueness_score: u32,

    /// Longevity score (0-200) - time since identity creation
    pub longevity_score: u32,

    /// Activity score (0-200) - recent participation
    pub activity_score: u32,

    /// Number of challenges passed
    pub challenges_passed: u32,

    /// Number of challenges failed
    pub challenges_failed: u32,

    /// Last verification block
    pub last_verified: BlockNumber,
}

impl PersonhoodScore {
    /// Create a new score
    pub fn new() -> Self {
        Self::default()
    }

    /// Recalculate total score
    pub fn recalculate(&mut self) {
        self.total = self.social_score
            + self.challenge_score
            + self.uniqueness_score
            + self.longevity_score
            + self.activity_score;
    }

    /// Update social score based on graph connectivity
    pub fn update_social(&mut self, connectivity: u32) {
        // Max 200 points from connectivity
        self.social_score = (connectivity / 5).min(200);
        self.recalculate();
    }

    /// Record passed challenge
    pub fn record_challenge_pass(&mut self, current_block: BlockNumber) {
        self.challenges_passed += 1;
        // +20 per challenge, max 200
        self.challenge_score = (self.challenges_passed * 20).min(200);
        self.last_verified = current_block;
        self.recalculate();
    }

    /// Record failed challenge
    pub fn record_challenge_fail(&mut self) {
        self.challenges_failed += 1;
        // -50 per failure
        self.challenge_score = self.challenge_score.saturating_sub(50);
        self.recalculate();
    }

    /// Update uniqueness based on verification
    pub fn update_uniqueness(&mut self, verified: bool, witness_count: usize) {
        if verified {
            // Base 100 + 20 per witness (max 200)
            self.uniqueness_score = 100 + ((witness_count as u32) * 20).min(100);
        } else {
            self.uniqueness_score = 0;
        }
        self.recalculate();
    }

    /// Update longevity based on identity age
    pub fn update_longevity(&mut self, identity_age_blocks: BlockNumber) {
        // ~200 points after 1 year (~5,256,000 blocks)
        let year_in_blocks: u64 = 5_256_000;
        let score = ((identity_age_blocks as u64 * 200) / year_in_blocks) as u32;
        self.longevity_score = score.min(200);
        self.recalculate();
    }

    /// Update activity based on recent participation
    pub fn update_activity(&mut self, actions_last_month: u32) {
        // 10 points per action, max 200
        self.activity_score = (actions_last_month * 10).min(200);
        self.recalculate();
    }

    /// Check if score meets threshold for enhanced privileges
    pub fn meets_threshold(&self, threshold: u32) -> bool {
        self.total >= threshold
    }

    /// Get verification status
    pub fn is_verified(&self, current_block: BlockNumber) -> bool {
        // Verified if score > 500 and verified in last 6 months
        let six_months: BlockNumber = 2_628_000;
        self.total >= 500 && current_block.saturating_sub(self.last_verified) < six_months
    }
}

// =============================================================================
// EVENTS
// =============================================================================

/// Events emitted by the personhood system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PersonhoodEvent {
    /// Challenge issued
    ChallengeIssued {
        challenge_id: Hash,
        target: IdentityId,
        challenger: AccountId,
        challenge_type: String,
    },

    /// Challenge response submitted
    ChallengeResponseSubmitted {
        challenge_id: Hash,
        target: IdentityId,
    },

    /// Challenge resolved
    ChallengeResolved {
        challenge_id: Hash,
        target: IdentityId,
        passed: bool,
    },

    /// Sybil cluster detected
    SybilClusterDetected {
        cluster_id: u32,
        member_count: usize,
        risk_score: u8,
    },

    /// Uniqueness verified
    UniquenessVerified {
        identity_id: IdentityId,
        witness_count: usize,
    },

    /// Personhood score updated
    PersonhoodScoreUpdated {
        identity_id: IdentityId,
        old_score: u32,
        new_score: u32,
    },
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_hash(seed: u8) -> Hash {
        Hash::hash(&[seed; 32])
    }

    fn create_account(seed: u8) -> AccountId {
        AccountId::from_bytes([seed; 32])
    }

    #[test]
    fn test_challenge_creation() {
        let target = create_hash(1);
        let challenger = create_account(2);

        let challenge = PersonhoodChallenge::new(
            target,
            challenger,
            ChallengeType::Liveness { nonce: create_hash(99) },
            CHALLENGE_DEPOSIT,
            1000,
        );

        assert_eq!(challenge.status, ChallengeStatus::Pending);
        assert_eq!(challenge.deposit, CHALLENGE_DEPOSIT);
        assert_eq!(challenge.expires_at, 1000 + CHALLENGE_PERIOD);
    }

    #[test]
    fn test_challenge_expiry() {
        let challenge = PersonhoodChallenge::new(
            create_hash(1),
            create_account(2),
            ChallengeType::Liveness { nonce: create_hash(99) },
            CHALLENGE_DEPOSIT,
            1000,
        );

        assert!(!challenge.is_expired(1000));
        assert!(!challenge.is_expired(1000 + CHALLENGE_PERIOD));
        assert!(challenge.is_expired(1000 + CHALLENGE_PERIOD + 1));
    }

    #[test]
    fn test_challenge_grace_period() {
        let challenge = PersonhoodChallenge::new(
            create_hash(1),
            create_account(2),
            ChallengeType::Liveness { nonce: create_hash(99) },
            CHALLENGE_DEPOSIT,
            1000,
        );

        let grace_start = 1000 + CHALLENGE_PERIOD - CHALLENGE_GRACE_PERIOD;

        assert!(!challenge.in_grace_period(grace_start - 1));
        assert!(challenge.in_grace_period(grace_start));
        assert!(challenge.in_grace_period(1000 + CHALLENGE_PERIOD));
    }

    #[test]
    fn test_challenge_difficulty() {
        assert_eq!(ChallengeType::Liveness { nonce: create_hash(1) }.difficulty(), 1);
        assert_eq!(ChallengeType::SocialVerification { required_attestations: 5 }.difficulty(), 5);
        assert_eq!(ChallengeType::UniquenessChallenge { suspected_duplicate: create_hash(1) }.difficulty(), 5);
        assert_eq!(ChallengeType::StakeChallenge { required_stake: 100 }.difficulty(), 3);
    }

    #[test]
    fn test_social_graph_node() {
        let mut node = SocialGraphNode::new(create_hash(1));

        assert!(node.connections.is_empty());
        assert_eq!(node.connectivity_score, 0);

        // Add connections
        node.add_connection(create_hash(2), true);
        node.add_connection(create_hash(3), false);
        node.add_connection(create_hash(4), true);

        assert_eq!(node.connections.len(), 3);
        assert_eq!(node.incoming.len(), 2);
        assert_eq!(node.outgoing.len(), 1);
    }

    #[test]
    fn test_social_graph_connectivity() {
        let mut node = SocialGraphNode::new(create_hash(1));
        node.trust_depth = 1;

        // Add balanced connections
        for i in 2..=6 {
            node.add_connection(create_hash(i), i % 2 == 0);
        }

        node.calculate_connectivity(100);

        assert!(node.connectivity_score > 0);
    }

    #[test]
    fn test_suspicious_node_detection() {
        // Only outgoing = suspicious
        let mut suspicious_node = SocialGraphNode::new(create_hash(1));
        suspicious_node.add_connection(create_hash(2), false);
        suspicious_node.add_connection(create_hash(3), false);
        assert!(suspicious_node.is_suspicious());

        // Balanced = not suspicious
        let mut normal_node = SocialGraphNode::new(create_hash(10));
        normal_node.add_connection(create_hash(11), true);
        normal_node.add_connection(create_hash(12), false);
        normal_node.connectivity_score = 150;
        assert!(!normal_node.is_suspicious());
    }

    #[test]
    fn test_sybil_cluster() {
        let mut cluster = SybilCluster::new(1);

        cluster.add_member(create_hash(1));
        cluster.add_member(create_hash(2));
        cluster.add_member(create_hash(3));
        cluster.add_member(create_hash(4));

        cluster.internal_density = 0.9;
        cluster.external_connections = 1;

        cluster.calculate_risk();

        assert!(cluster.risk_score > 50);
        assert!(cluster.flagged);
    }

    #[test]
    fn test_uniqueness_commitment() {
        let identity_id = create_hash(1);
        let mut commitment = UniquenessCommitment::new(
            identity_id,
            b"unique_id_123",
            1000,
        );

        assert!(!commitment.verified);

        // Add witnesses
        commitment.add_witness(create_account(1));
        assert!(!commitment.verified);

        commitment.add_witness(create_account(2));
        assert!(!commitment.verified);

        commitment.add_witness(create_account(3));
        assert!(commitment.verified); // Auto-verified at 3 witnesses
    }

    #[test]
    fn test_uniqueness_duplicate_witness() {
        let mut commitment = UniquenessCommitment::new(
            create_hash(1),
            b"data",
            1000,
        );

        let witness = create_account(1);
        assert!(commitment.add_witness(witness));
        assert!(!commitment.add_witness(witness)); // Duplicate rejected
    }

    #[test]
    fn test_personhood_score_calculation() {
        let mut score = PersonhoodScore::new();

        score.update_social(500); // 100 points
        assert_eq!(score.social_score, 100);

        score.record_challenge_pass(1000); // 20 points
        assert_eq!(score.challenge_score, 20);

        score.update_uniqueness(true, 5); // 100 + 100 = 200 points
        assert_eq!(score.uniqueness_score, 200);

        score.recalculate();
        assert_eq!(score.total, 100 + 20 + 200);
    }

    #[test]
    fn test_personhood_challenge_history() {
        let mut score = PersonhoodScore::new();

        // Pass 10 challenges
        for i in 0..10 {
            score.record_challenge_pass(1000 + i * 100);
        }

        assert_eq!(score.challenges_passed, 10);
        assert_eq!(score.challenge_score, 200); // Capped at 200

        // Fail one
        score.record_challenge_fail();
        assert_eq!(score.challenges_failed, 1);
        assert_eq!(score.challenge_score, 150); // -50
    }

    #[test]
    fn test_personhood_longevity() {
        let mut score = PersonhoodScore::new();

        // 3 months old (~1,314,000 blocks)
        score.update_longevity(1_314_000);
        assert!(score.longevity_score > 0 && score.longevity_score < 100);

        // 1 year old
        score.update_longevity(5_256_000);
        assert_eq!(score.longevity_score, 200);

        // Very old - still capped at 200
        score.update_longevity(10_000_000);
        assert_eq!(score.longevity_score, 200);
    }

    #[test]
    fn test_personhood_activity() {
        let mut score = PersonhoodScore::new();

        score.update_activity(5); // 50 points
        assert_eq!(score.activity_score, 50);

        score.update_activity(20); // 200 points (max)
        assert_eq!(score.activity_score, 200);

        score.update_activity(50); // Still capped at 200
        assert_eq!(score.activity_score, 200);
    }

    #[test]
    fn test_personhood_verification_status() {
        let mut score = PersonhoodScore::new();
        score.total = 600;
        score.last_verified = 1000;

        // Recent verification
        assert!(score.is_verified(1000 + 1_000_000)); // ~2 months later

        // Stale verification (> 6 months)
        assert!(!score.is_verified(1000 + 3_000_000)); // ~7 months later

        // Low score
        score.total = 400;
        assert!(!score.is_verified(1000));
    }

    #[test]
    fn test_personhood_threshold() {
        let mut score = PersonhoodScore::new();
        score.total = 500;

        assert!(score.meets_threshold(500));
        assert!(score.meets_threshold(400));
        assert!(!score.meets_threshold(600));
    }
}
