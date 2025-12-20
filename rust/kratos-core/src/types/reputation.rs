// Advanced Reputation Types - SPEC v4 Layer 3
//
// Implements multi-dimensional reputation with:
// - Domain-specific scores (governance, technical, community)
// - Reputation staking for commitments
// - Cross-chain reputation portability
// - Advanced decay mechanics

use crate::types::identity::{IdentityId, REPUTATION_DECAY_INTERVAL, MAX_REPUTATION};
use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash as CryptoHash};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::Hash;

// =============================================================================
// CONSTANTS
// =============================================================================

/// Maximum domain score
pub const MAX_DOMAIN_SCORE: u32 = 10_000;

/// Reputation stake lock period (90 days = ~1,296,000 blocks)
pub const REPUTATION_STAKE_LOCK: BlockNumber = 1_296_000;

/// Minimum stake amount for reputation commitment
pub const MIN_REPUTATION_STAKE: Balance = 100;

/// Maximum reputation multiplier from staking (2x)
pub const MAX_STAKE_MULTIPLIER: u32 = 200;

/// Decay rate per domain per week of inactivity (%)
pub const DOMAIN_DECAY_RATE: u8 = 2;

/// Cross-chain reputation discount (foreign reputation worth 50%)
pub const CROSS_CHAIN_DISCOUNT: u32 = 50;

/// Maximum endorsements per identity per domain
pub const MAX_ENDORSEMENTS_PER_DOMAIN: usize = 50;

/// Points per endorsement (varies by endorser reputation)
pub const BASE_ENDORSEMENT_POINTS: u32 = 10;

// =============================================================================
// REPUTATION DOMAINS
// =============================================================================

/// Reputation domains - different areas of contribution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReputationDomain {
    /// Governance participation (voting, proposals)
    Governance,

    /// Technical contribution (code, audits)
    Technical,

    /// Community building (attestations, onboarding)
    Community,

    /// Arbitration and dispute resolution
    Arbitration,

    /// Economic activity (staking, transactions)
    Economic,

    /// Validator operations (uptime, performance)
    Validation,
}

impl ReputationDomain {
    /// Get all domains
    pub fn all() -> &'static [ReputationDomain] {
        &[
            ReputationDomain::Governance,
            ReputationDomain::Technical,
            ReputationDomain::Community,
            ReputationDomain::Arbitration,
            ReputationDomain::Economic,
            ReputationDomain::Validation,
        ]
    }

    /// Get weight for overall score calculation
    pub fn weight(&self) -> u32 {
        match self {
            ReputationDomain::Governance => 25,
            ReputationDomain::Technical => 20,
            ReputationDomain::Community => 20,
            ReputationDomain::Arbitration => 15,
            ReputationDomain::Economic => 10,
            ReputationDomain::Validation => 10,
        }
    }

    /// Get decay rate for this domain
    pub fn decay_rate(&self) -> u8 {
        match self {
            ReputationDomain::Validation => 1, // Slower decay for validators
            _ => DOMAIN_DECAY_RATE,
        }
    }
}

// =============================================================================
// DOMAIN-SPECIFIC REPUTATION
// =============================================================================

/// Domain-specific reputation score
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DomainReputation {
    /// Raw score (0 - MAX_DOMAIN_SCORE)
    pub score: u32,

    /// Actions performed in this domain
    pub action_count: u32,

    /// Last activity in this domain
    pub last_activity: BlockNumber,

    /// Endorsements received
    pub endorsement_count: u32,

    /// Total endorsement weight
    pub endorsement_weight: u32,

    /// Slash count in this domain
    pub slash_count: u32,
}

impl DomainReputation {
    /// Create a new domain reputation
    pub fn new() -> Self {
        Self::default()
    }

    /// Add points to this domain
    pub fn add(&mut self, points: u32, current_block: BlockNumber) {
        self.score = self.score.saturating_add(points).min(MAX_DOMAIN_SCORE);
        self.action_count += 1;
        self.last_activity = current_block;
    }

    /// Apply decay based on inactivity
    pub fn apply_decay(&mut self, current_block: BlockNumber, decay_rate: u8) {
        let inactive_blocks = current_block.saturating_sub(self.last_activity);
        let decay_periods = inactive_blocks / REPUTATION_DECAY_INTERVAL;

        if decay_periods > 0 {
            // Use saturating_mul to prevent overflow on large values
            let decay = (self.score as u64)
                .saturating_mul(decay_periods)
                .saturating_mul(decay_rate as u64) / 100;
            self.score = self.score.saturating_sub(decay as u32);
        }
    }

    /// Apply slash penalty
    pub fn slash(&mut self, penalty_percent: u8) {
        // Use saturating_mul to prevent overflow
        let penalty = (self.score as u64).saturating_mul(penalty_percent as u64) / 100;
        self.score = self.score.saturating_sub(penalty as u32);
        self.slash_count += 1;
    }

    /// Record endorsement
    pub fn add_endorsement(&mut self, weight: u32) {
        self.endorsement_count += 1;
        self.endorsement_weight += weight;
        // Endorsements add points based on weight
        self.score = self.score.saturating_add(weight * BASE_ENDORSEMENT_POINTS / 10).min(MAX_DOMAIN_SCORE);
    }
}

// =============================================================================
// MULTI-DIMENSIONAL REPUTATION
// =============================================================================

/// Multi-dimensional reputation across all domains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiDimensionalReputation {
    /// Identity this reputation belongs to
    pub identity_id: IdentityId,

    /// Per-domain scores
    pub domains: HashMap<ReputationDomain, DomainReputation>,

    /// Weighted overall score
    pub overall_score: u32,

    /// Creation block
    pub created_at: BlockNumber,

    /// Last update block
    pub updated_at: BlockNumber,
}

impl MultiDimensionalReputation {
    /// Create a new reputation profile
    pub fn new(identity_id: IdentityId, current_block: BlockNumber) -> Self {
        let mut domains = HashMap::new();
        for domain in ReputationDomain::all() {
            domains.insert(*domain, DomainReputation::new());
        }

        Self {
            identity_id,
            domains,
            overall_score: 0,
            created_at: current_block,
            updated_at: current_block,
        }
    }

    /// Get domain score
    pub fn get_domain(&self, domain: ReputationDomain) -> u32 {
        self.domains.get(&domain).map(|d| d.score).unwrap_or(0)
    }

    /// Add reputation in a specific domain
    pub fn add_domain_rep(
        &mut self,
        domain: ReputationDomain,
        points: u32,
        current_block: BlockNumber,
    ) {
        if let Some(domain_rep) = self.domains.get_mut(&domain) {
            domain_rep.add(points, current_block);
        }
        self.updated_at = current_block;
        self.recalculate_overall();
    }

    /// Recalculate overall weighted score
    pub fn recalculate_overall(&mut self) {
        let mut weighted_sum: u64 = 0;
        let mut total_weight: u64 = 0;

        for (domain, rep) in &self.domains {
            let weight = domain.weight() as u64;
            weighted_sum += rep.score as u64 * weight;
            total_weight += weight;
        }

        self.overall_score = if total_weight > 0 {
            (weighted_sum / total_weight) as u32
        } else {
            0
        };
    }

    /// Apply decay to all domains
    pub fn apply_decay(&mut self, current_block: BlockNumber) {
        for (domain, rep) in self.domains.iter_mut() {
            rep.apply_decay(current_block, domain.decay_rate());
        }
        self.recalculate_overall();
    }

    /// Slash reputation in a domain
    pub fn slash_domain(&mut self, domain: ReputationDomain, penalty_percent: u8) {
        if let Some(rep) = self.domains.get_mut(&domain) {
            rep.slash(penalty_percent);
        }
        self.recalculate_overall();
    }

    /// Record endorsement in a domain
    pub fn add_endorsement(
        &mut self,
        domain: ReputationDomain,
        endorser_rep: u32,
    ) -> bool {
        if let Some(rep) = self.domains.get_mut(&domain) {
            if rep.endorsement_count >= MAX_ENDORSEMENTS_PER_DOMAIN as u32 {
                return false;
            }
            // Weight based on endorser's reputation (1-10)
            let weight = (endorser_rep / 1000).max(1).min(10);
            rep.add_endorsement(weight);
            self.recalculate_overall();
            return true;
        }
        false
    }

    /// Get domain breakdown
    pub fn get_breakdown(&self) -> Vec<(ReputationDomain, u32)> {
        self.domains
            .iter()
            .map(|(d, r)| (*d, r.score))
            .collect()
    }
}

// =============================================================================
// REPUTATION STAKING
// =============================================================================

/// Reputation stake - lock stake to boost reputation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationStake {
    /// Unique stake ID
    pub stake_id: CryptoHash,

    /// Who is staking
    pub staker: AccountId,

    /// Identity whose reputation is boosted
    pub identity_id: IdentityId,

    /// Amount staked
    pub amount: Balance,

    /// Block when stake was created
    pub staked_at: BlockNumber,

    /// Block when stake can be withdrawn
    pub locked_until: BlockNumber,

    /// Domain being boosted (None = overall)
    pub domain: Option<ReputationDomain>,

    /// Current multiplier (in basis points, 100 = 1.0x)
    pub multiplier: u32,

    /// Whether stake has been slashed
    pub slashed: bool,
}

impl ReputationStake {
    /// Create a new reputation stake
    pub fn new(
        staker: AccountId,
        identity_id: IdentityId,
        amount: Balance,
        domain: Option<ReputationDomain>,
        current_block: BlockNumber,
    ) -> Option<Self> {
        if amount < MIN_REPUTATION_STAKE {
            return None;
        }

        let stake_id = Self::compute_id(&staker, &identity_id, current_block);

        // Calculate multiplier: sqrt(stake / 100) * 10, capped at 200 (2x)
        let multiplier = Self::calculate_multiplier(amount);

        Some(Self {
            stake_id,
            staker,
            identity_id,
            amount,
            staked_at: current_block,
            locked_until: current_block + REPUTATION_STAKE_LOCK,
            domain,
            multiplier,
            slashed: false,
        })
    }

    /// Compute stake ID
    fn compute_id(staker: &AccountId, identity: &IdentityId, block: BlockNumber) -> CryptoHash {
        let mut data = Vec::new();
        data.extend_from_slice(staker.as_bytes());
        data.extend_from_slice(identity.as_bytes());
        data.extend_from_slice(&block.to_le_bytes());
        CryptoHash::hash(&data)
    }

    /// Calculate multiplier from stake amount
    fn calculate_multiplier(amount: Balance) -> u32 {
        // sqrt(amount / MIN_STAKE) * 10 + 100, capped at MAX_STAKE_MULTIPLIER
        let ratio = amount as f64 / MIN_REPUTATION_STAKE as f64;
        let boost = (ratio.sqrt() * 10.0) as u32;
        (100 + boost).min(MAX_STAKE_MULTIPLIER)
    }

    /// Check if stake is locked
    pub fn is_locked(&self, current_block: BlockNumber) -> bool {
        current_block < self.locked_until
    }

    /// Check if stake can be withdrawn
    pub fn can_withdraw(&self, current_block: BlockNumber) -> bool {
        !self.is_locked(current_block) && !self.slashed
    }

    /// Apply slash to stake
    pub fn slash(&mut self, penalty_percent: u8) -> Balance {
        if self.slashed {
            return 0;
        }

        // Use saturating_mul to prevent overflow
        let penalty = (self.amount as u64)
            .saturating_mul(penalty_percent as u64) / 100;
        self.amount = self.amount.saturating_sub(penalty as Balance);
        self.slashed = true;
        self.multiplier = 100; // Reset multiplier

        penalty as Balance
    }
}

// =============================================================================
// CROSS-CHAIN REPUTATION
// =============================================================================

/// Cross-chain reputation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainReputation {
    /// Source chain
    pub source_chain: ChainId,

    /// Identity on source chain
    pub source_identity: IdentityId,

    /// Target chain where reputation is imported
    pub target_chain: ChainId,

    /// Imported overall score (discounted)
    pub imported_score: u32,

    /// Per-domain imported scores
    pub domain_scores: HashMap<ReputationDomain, u32>,

    /// Block when imported
    pub imported_at: BlockNumber,

    /// Merkle proof of source reputation (for verification)
    pub proof_hash: CryptoHash,

    /// Whether import is verified
    pub verified: bool,
}

impl CrossChainReputation {
    /// Create a new cross-chain reputation import
    pub fn new(
        source_chain: ChainId,
        source_identity: IdentityId,
        target_chain: ChainId,
        original_score: u32,
        domain_scores: HashMap<ReputationDomain, u32>,
        proof_hash: CryptoHash,
        current_block: BlockNumber,
    ) -> Self {
        // Apply cross-chain discount (use saturating_mul to prevent overflow)
        let imported_score = original_score.saturating_mul(CROSS_CHAIN_DISCOUNT) / 100;

        let discounted_domains: HashMap<_, _> = domain_scores
            .iter()
            .map(|(d, s)| (*d, s.saturating_mul(CROSS_CHAIN_DISCOUNT) / 100))
            .collect();

        Self {
            source_chain,
            source_identity,
            target_chain,
            imported_score,
            domain_scores: discounted_domains,
            imported_at: current_block,
            proof_hash,
            verified: false,
        }
    }

    /// Verify the import
    pub fn verify(&mut self) {
        self.verified = true;
    }

    /// Get effective score (only if verified)
    pub fn effective_score(&self) -> u32 {
        if self.verified {
            self.imported_score
        } else {
            0
        }
    }
}

// =============================================================================
// ENDORSEMENTS
// =============================================================================

/// Endorsement from one identity to another
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endorsement {
    /// Unique endorsement ID
    pub endorsement_id: CryptoHash,

    /// Who is endorsing
    pub endorser: AccountId,

    /// Endorser's identity
    pub endorser_identity: IdentityId,

    /// Who is being endorsed
    pub target_identity: IdentityId,

    /// Domain of endorsement
    pub domain: ReputationDomain,

    /// Endorsement weight (based on endorser's reputation)
    pub weight: u32,

    /// Block when endorsement was made
    pub endorsed_at: BlockNumber,

    /// Optional message/context
    pub context: Option<String>,

    /// Whether endorsement is still active
    pub active: bool,
}

impl Endorsement {
    /// Create a new endorsement
    pub fn new(
        endorser: AccountId,
        endorser_identity: IdentityId,
        target_identity: IdentityId,
        domain: ReputationDomain,
        endorser_reputation: u32,
        context: Option<String>,
        current_block: BlockNumber,
    ) -> Self {
        let endorsement_id = Self::compute_id(&endorser_identity, &target_identity, &domain, current_block);

        // Weight 1-10 based on endorser's reputation
        let weight = (endorser_reputation / 1000).max(1).min(10);

        Self {
            endorsement_id,
            endorser,
            endorser_identity,
            target_identity,
            domain,
            weight,
            endorsed_at: current_block,
            context,
            active: true,
        }
    }

    /// Compute endorsement ID
    fn compute_id(
        endorser: &IdentityId,
        target: &IdentityId,
        domain: &ReputationDomain,
        block: BlockNumber,
    ) -> CryptoHash {
        let mut data = Vec::new();
        data.extend_from_slice(endorser.as_bytes());
        data.extend_from_slice(target.as_bytes());
        data.push(*domain as u8);
        data.extend_from_slice(&block.to_le_bytes());
        CryptoHash::hash(&data)
    }

    /// Revoke endorsement
    pub fn revoke(&mut self) {
        self.active = false;
    }
}

// =============================================================================
// REPUTATION EVENTS
// =============================================================================

/// Events emitted by the reputation system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReputationEvent {
    /// Domain reputation changed
    DomainReputationChanged {
        identity_id: IdentityId,
        domain: ReputationDomain,
        old_score: u32,
        new_score: u32,
        reason: String,
    },

    /// Reputation stake created
    StakeCreated {
        stake_id: CryptoHash,
        staker: AccountId,
        identity_id: IdentityId,
        amount: Balance,
        multiplier: u32,
    },

    /// Reputation stake slashed
    StakeSlashed {
        stake_id: CryptoHash,
        penalty: Balance,
    },

    /// Stake withdrawn
    StakeWithdrawn {
        stake_id: CryptoHash,
        amount: Balance,
    },

    /// Cross-chain reputation imported
    ReputationImported {
        source_chain: ChainId,
        target_chain: ChainId,
        identity_id: IdentityId,
        imported_score: u32,
    },

    /// Endorsement given
    EndorsementGiven {
        endorser: AccountId,
        target: IdentityId,
        domain: ReputationDomain,
        weight: u32,
    },

    /// Endorsement revoked
    EndorsementRevoked {
        endorsement_id: CryptoHash,
    },
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_hash(seed: u8) -> CryptoHash {
        CryptoHash::hash(&[seed; 32])
    }

    fn create_account(seed: u8) -> AccountId {
        AccountId::from_bytes([seed; 32])
    }

    #[test]
    fn test_domain_weights() {
        let total_weight: u32 = ReputationDomain::all().iter().map(|d| d.weight()).sum();
        assert_eq!(total_weight, 100); // Should sum to 100%
    }

    #[test]
    fn test_domain_reputation_add() {
        let mut rep = DomainReputation::new();

        rep.add(100, 1000);
        assert_eq!(rep.score, 100);
        assert_eq!(rep.action_count, 1);
        assert_eq!(rep.last_activity, 1000);

        rep.add(MAX_DOMAIN_SCORE, 2000);
        assert_eq!(rep.score, MAX_DOMAIN_SCORE); // Capped
    }

    #[test]
    fn test_domain_reputation_decay() {
        let mut rep = DomainReputation::new();
        rep.score = 1000;
        rep.last_activity = 1000;

        // One decay period
        rep.apply_decay(1000 + REPUTATION_DECAY_INTERVAL, DOMAIN_DECAY_RATE);
        assert_eq!(rep.score, 980); // 2% decay
    }

    #[test]
    fn test_domain_reputation_slash() {
        let mut rep = DomainReputation::new();
        rep.score = 1000;

        rep.slash(25);
        assert_eq!(rep.score, 750);
        assert_eq!(rep.slash_count, 1);
    }

    #[test]
    fn test_domain_endorsement() {
        let mut rep = DomainReputation::new();

        rep.add_endorsement(10); // Max weight
        assert_eq!(rep.endorsement_count, 1);
        assert_eq!(rep.endorsement_weight, 10);
        assert!(rep.score > 0);
    }

    #[test]
    fn test_multi_dimensional_creation() {
        let identity = create_hash(1);
        let rep = MultiDimensionalReputation::new(identity, 1000);

        assert_eq!(rep.domains.len(), 6);
        assert_eq!(rep.overall_score, 0);
    }

    #[test]
    fn test_multi_dimensional_add() {
        let identity = create_hash(1);
        let mut rep = MultiDimensionalReputation::new(identity, 1000);

        rep.add_domain_rep(ReputationDomain::Governance, 100, 2000);

        assert_eq!(rep.get_domain(ReputationDomain::Governance), 100);
        assert!(rep.overall_score > 0);
    }

    #[test]
    fn test_multi_dimensional_weighted_score() {
        let identity = create_hash(1);
        let mut rep = MultiDimensionalReputation::new(identity, 1000);

        // Add 1000 to governance (weight 25)
        rep.add_domain_rep(ReputationDomain::Governance, 1000, 2000);

        // Overall should be 1000 * 25 / 100 = 250
        assert_eq!(rep.overall_score, 250);
    }

    #[test]
    fn test_reputation_stake_creation() {
        let staker = create_account(1);
        let identity = create_hash(2);

        let stake = ReputationStake::new(
            staker,
            identity,
            MIN_REPUTATION_STAKE,
            None,
            1000,
        );

        assert!(stake.is_some());
        let stake = stake.unwrap();
        assert_eq!(stake.amount, MIN_REPUTATION_STAKE);
        assert!(stake.multiplier >= 100);
    }

    #[test]
    fn test_reputation_stake_min_amount() {
        let stake = ReputationStake::new(
            create_account(1),
            create_hash(2),
            MIN_REPUTATION_STAKE - 1, // Below minimum
            None,
            1000,
        );

        assert!(stake.is_none());
    }

    #[test]
    fn test_reputation_stake_multiplier() {
        // Higher stakes = higher multiplier
        let stake1 = ReputationStake::new(
            create_account(1),
            create_hash(1),
            MIN_REPUTATION_STAKE,
            None,
            1000,
        ).unwrap();

        let stake2 = ReputationStake::new(
            create_account(2),
            create_hash(2),
            MIN_REPUTATION_STAKE * 4, // 4x stake
            None,
            1000,
        ).unwrap();

        assert!(stake2.multiplier > stake1.multiplier);
        assert!(stake2.multiplier <= MAX_STAKE_MULTIPLIER);
    }

    #[test]
    fn test_reputation_stake_lock() {
        let stake = ReputationStake::new(
            create_account(1),
            create_hash(1),
            MIN_REPUTATION_STAKE,
            None,
            1000,
        ).unwrap();

        assert!(stake.is_locked(1000));
        assert!(stake.is_locked(1000 + REPUTATION_STAKE_LOCK - 1));
        assert!(!stake.is_locked(1000 + REPUTATION_STAKE_LOCK));
    }

    #[test]
    fn test_reputation_stake_slash() {
        let mut stake = ReputationStake::new(
            create_account(1),
            create_hash(1),
            1000,
            None,
            1000,
        ).unwrap();

        let penalty = stake.slash(50);

        assert_eq!(penalty, 500);
        assert_eq!(stake.amount, 500);
        assert!(stake.slashed);
        assert_eq!(stake.multiplier, 100); // Reset
    }

    #[test]
    fn test_reputation_stake_withdraw() {
        let mut stake = ReputationStake::new(
            create_account(1),
            create_hash(1),
            MIN_REPUTATION_STAKE,
            None,
            1000,
        ).unwrap();

        // Cannot withdraw while locked
        assert!(!stake.can_withdraw(1000));
        assert!(!stake.can_withdraw(1000 + REPUTATION_STAKE_LOCK - 1));

        // Can withdraw after lock
        assert!(stake.can_withdraw(1000 + REPUTATION_STAKE_LOCK));

        // Cannot withdraw if slashed
        stake.slash(10);
        assert!(!stake.can_withdraw(1000 + REPUTATION_STAKE_LOCK));
    }

    #[test]
    fn test_cross_chain_reputation() {
        let cross_rep = CrossChainReputation::new(
            ChainId(1),
            create_hash(1),
            ChainId(2),
            1000, // Original score
            HashMap::new(),
            create_hash(99),
            1000,
        );

        // Score is discounted by CROSS_CHAIN_DISCOUNT (50%)
        assert_eq!(cross_rep.imported_score, 500);
        assert!(!cross_rep.verified);
        assert_eq!(cross_rep.effective_score(), 0); // Not verified
    }

    #[test]
    fn test_cross_chain_verification() {
        let mut cross_rep = CrossChainReputation::new(
            ChainId(1),
            create_hash(1),
            ChainId(2),
            1000,
            HashMap::new(),
            create_hash(99),
            1000,
        );

        cross_rep.verify();

        assert!(cross_rep.verified);
        assert_eq!(cross_rep.effective_score(), 500);
    }

    #[test]
    fn test_endorsement_creation() {
        let endorsement = Endorsement::new(
            create_account(1),
            create_hash(1),
            create_hash(2),
            ReputationDomain::Technical,
            5000, // High reputation
            Some("Great code contributions".to_string()),
            1000,
        );

        assert_eq!(endorsement.weight, 5); // 5000 / 1000 = 5
        assert!(endorsement.active);
    }

    #[test]
    fn test_endorsement_weight_bounds() {
        // Low reputation = minimum weight
        let low_rep = Endorsement::new(
            create_account(1),
            create_hash(1),
            create_hash(2),
            ReputationDomain::Technical,
            100,
            None,
            1000,
        );
        assert_eq!(low_rep.weight, 1);

        // High reputation = maximum weight
        let high_rep = Endorsement::new(
            create_account(2),
            create_hash(3),
            create_hash(4),
            ReputationDomain::Technical,
            50000,
            None,
            1000,
        );
        assert_eq!(high_rep.weight, 10); // Capped at 10
    }

    #[test]
    fn test_endorsement_revoke() {
        let mut endorsement = Endorsement::new(
            create_account(1),
            create_hash(1),
            create_hash(2),
            ReputationDomain::Community,
            1000,
            None,
            1000,
        );

        assert!(endorsement.active);
        endorsement.revoke();
        assert!(!endorsement.active);
    }
}
