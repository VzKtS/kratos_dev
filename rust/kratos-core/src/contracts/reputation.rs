// Advanced Reputation Contract - SPEC v4 Layer 3
//
// Implements multi-dimensional reputation system:
// - Domain-specific reputation tracking
// - Reputation staking for commitments
// - Cross-chain reputation portability
// - Endorsement system

use crate::types::identity::IdentityId;
use crate::types::reputation::{
    CrossChainReputation, DomainReputation, Endorsement, MultiDimensionalReputation,
    ReputationDomain, ReputationEvent, ReputationStake,
    CROSS_CHAIN_DISCOUNT, MAX_ENDORSEMENTS_PER_DOMAIN, MIN_REPUTATION_STAKE,
    REPUTATION_STAKE_LOCK,
};
use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash};
use crate::contracts::identity::IdentityRegistry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// REPUTATION REGISTRY
// =============================================================================

/// Advanced Reputation Registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationRegistry {
    /// Chain this registry belongs to
    chain_id: ChainId,

    /// Multi-dimensional reputation per identity
    reputations: HashMap<IdentityId, MultiDimensionalReputation>,

    /// Reputation stakes by ID
    stakes: HashMap<Hash, ReputationStake>,

    /// Stakes by staker
    stakes_by_staker: HashMap<AccountId, Vec<Hash>>,

    /// Stakes by identity (being boosted)
    stakes_by_identity: HashMap<IdentityId, Vec<Hash>>,

    /// Cross-chain reputation imports
    cross_chain_imports: HashMap<(IdentityId, ChainId), CrossChainReputation>,

    /// Endorsements by ID
    endorsements: HashMap<Hash, Endorsement>,

    /// Endorsements given by identity
    endorsements_given: HashMap<IdentityId, Vec<Hash>>,

    /// Endorsements received by identity (per domain)
    endorsements_received: HashMap<IdentityId, HashMap<ReputationDomain, Vec<Hash>>>,

    /// Total staked amount
    total_staked: Balance,

    /// Events
    events: Vec<ReputationEvent>,

    /// Configuration
    config: ReputationConfig,
}

/// Reputation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationConfig {
    /// Enable reputation staking
    pub staking_enabled: bool,

    /// Enable cross-chain imports
    pub cross_chain_enabled: bool,

    /// Enable endorsements
    pub endorsements_enabled: bool,

    /// Minimum reputation to endorse others
    pub min_rep_to_endorse: u32,

    /// Maximum stakes per identity
    pub max_stakes_per_identity: usize,

    // Note: auto_decay is always enabled per SPEC v5/v6
    // Reputation decay is MANDATORY to prevent permanent reputation accumulation
}

impl ReputationConfig {
    /// SPEC v5/v6: Reputation decay is always enabled (mandatory)
    /// This cannot be disabled as it would violate the spec requirement
    /// that reputation must decay over time
    #[inline]
    pub fn auto_decay_enabled(&self) -> bool {
        true  // Always enabled per spec
    }
}

impl Default for ReputationConfig {
    fn default() -> Self {
        Self {
            staking_enabled: true,
            cross_chain_enabled: true,
            endorsements_enabled: true,
            min_rep_to_endorse: 100,
            max_stakes_per_identity: 10,
        }
    }
}

impl ReputationRegistry {
    /// Create a new reputation registry
    pub fn new(chain_id: ChainId) -> Self {
        Self {
            chain_id,
            reputations: HashMap::new(),
            stakes: HashMap::new(),
            stakes_by_staker: HashMap::new(),
            stakes_by_identity: HashMap::new(),
            cross_chain_imports: HashMap::new(),
            endorsements: HashMap::new(),
            endorsements_given: HashMap::new(),
            endorsements_received: HashMap::new(),
            total_staked: 0,
            events: Vec::new(),
            config: ReputationConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(chain_id: ChainId, config: ReputationConfig) -> Self {
        Self {
            config,
            ..Self::new(chain_id)
        }
    }

    // =========================================================================
    // REPUTATION MANAGEMENT
    // =========================================================================

    /// Initialize reputation for identity
    pub fn initialize(&mut self, identity_id: IdentityId, current_block: BlockNumber) {
        if !self.reputations.contains_key(&identity_id) {
            let rep = MultiDimensionalReputation::new(identity_id, current_block);
            self.reputations.insert(identity_id, rep);
        }
    }

    /// Get reputation for identity
    pub fn get_reputation(&self, identity_id: &IdentityId) -> Option<&MultiDimensionalReputation> {
        self.reputations.get(identity_id)
    }

    /// Get domain score
    pub fn get_domain_score(&self, identity_id: &IdentityId, domain: ReputationDomain) -> u32 {
        self.reputations
            .get(identity_id)
            .map(|r| r.get_domain(domain))
            .unwrap_or(0)
    }

    /// Get overall score (including cross-chain and stake bonuses)
    pub fn get_effective_score(&self, identity_id: &IdentityId) -> u32 {
        let base_score = self.reputations
            .get(identity_id)
            .map(|r| r.overall_score)
            .unwrap_or(0);

        // Add cross-chain reputation
        let cross_chain_bonus: u32 = self.cross_chain_imports
            .iter()
            .filter(|((id, _), _)| id == identity_id)
            .map(|(_, import)| import.effective_score())
            .sum();

        // Apply stake multiplier
        let stake_multiplier = self.get_stake_multiplier(identity_id);

        let combined = base_score + (cross_chain_bonus / 2); // Cross-chain worth half
        (combined as u64 * stake_multiplier as u64 / 100) as u32
    }

    /// Add reputation in a domain
    pub fn add_reputation(
        &mut self,
        identity_id: IdentityId,
        domain: ReputationDomain,
        points: u32,
        reason: String,
        current_block: BlockNumber,
    ) -> Result<(), ReputationError> {
        let rep = self.reputations
            .get_mut(&identity_id)
            .ok_or(ReputationError::ReputationNotFound)?;

        let old_score = rep.get_domain(domain);
        rep.add_domain_rep(domain, points, current_block);
        let new_score = rep.get_domain(domain);

        self.events.push(ReputationEvent::DomainReputationChanged {
            identity_id,
            domain,
            old_score,
            new_score,
            reason,
        });

        Ok(())
    }

    /// Slash reputation in a domain
    pub fn slash_reputation(
        &mut self,
        identity_id: IdentityId,
        domain: ReputationDomain,
        penalty_percent: u8,
        reason: String,
    ) -> Result<(), ReputationError> {
        let rep = self.reputations
            .get_mut(&identity_id)
            .ok_or(ReputationError::ReputationNotFound)?;

        let old_score = rep.get_domain(domain);
        rep.slash_domain(domain, penalty_percent);
        let new_score = rep.get_domain(domain);

        self.events.push(ReputationEvent::DomainReputationChanged {
            identity_id,
            domain,
            old_score,
            new_score,
            reason,
        });

        Ok(())
    }

    /// Apply decay to all reputations
    /// SPEC v5/v6: Decay is always applied (mandatory per spec)
    pub fn apply_decay(&mut self, current_block: BlockNumber) {
        // Decay is always enabled per SPEC v5/v6
        // self.config.auto_decay_enabled() always returns true
        for rep in self.reputations.values_mut() {
            rep.apply_decay(current_block);
        }
    }

    /// Get reputation breakdown by domain
    pub fn get_breakdown(&self, identity_id: &IdentityId) -> Vec<(ReputationDomain, u32)> {
        self.reputations
            .get(identity_id)
            .map(|r| r.get_breakdown())
            .unwrap_or_default()
    }

    // =========================================================================
    // STAKING
    // =========================================================================

    /// Create a reputation stake
    pub fn create_stake(
        &mut self,
        staker: AccountId,
        identity_id: IdentityId,
        amount: Balance,
        domain: Option<ReputationDomain>,
        current_block: BlockNumber,
    ) -> Result<Hash, ReputationError> {
        if !self.config.staking_enabled {
            return Err(ReputationError::StakingDisabled);
        }

        // Check max stakes
        let current_stakes = self.stakes_by_identity
            .get(&identity_id)
            .map(|s| s.len())
            .unwrap_or(0);

        if current_stakes >= self.config.max_stakes_per_identity {
            return Err(ReputationError::TooManyStakes);
        }

        // Create stake
        let stake = ReputationStake::new(staker, identity_id, amount, domain, current_block)
            .ok_or(ReputationError::StakeTooSmall)?;

        let stake_id = stake.stake_id;
        let multiplier = stake.multiplier;

        // Store
        self.stakes.insert(stake_id, stake);
        self.stakes_by_staker
            .entry(staker)
            .or_default()
            .push(stake_id);
        self.stakes_by_identity
            .entry(identity_id)
            .or_default()
            .push(stake_id);

        self.total_staked += amount;

        self.events.push(ReputationEvent::StakeCreated {
            stake_id,
            staker,
            identity_id,
            amount,
            multiplier,
        });

        Ok(stake_id)
    }

    /// Withdraw stake (after lock period)
    pub fn withdraw_stake(
        &mut self,
        stake_id: Hash,
        withdrawer: AccountId,
        current_block: BlockNumber,
    ) -> Result<Balance, ReputationError> {
        let stake = self.stakes
            .get(&stake_id)
            .ok_or(ReputationError::StakeNotFound)?;

        if stake.staker != withdrawer {
            return Err(ReputationError::NotStaker);
        }

        if !stake.can_withdraw(current_block) {
            return Err(ReputationError::StakeLocked);
        }

        let amount = stake.amount;
        let identity_id = stake.identity_id;
        let staker = stake.staker;

        // Remove from mappings
        if let Some(stakes) = self.stakes_by_staker.get_mut(&staker) {
            stakes.retain(|s| *s != stake_id);
        }
        if let Some(stakes) = self.stakes_by_identity.get_mut(&identity_id) {
            stakes.retain(|s| *s != stake_id);
        }

        self.stakes.remove(&stake_id);
        self.total_staked = self.total_staked.saturating_sub(amount);

        self.events.push(ReputationEvent::StakeWithdrawn {
            stake_id,
            amount,
        });

        Ok(amount)
    }

    /// Slash a stake
    pub fn slash_stake(
        &mut self,
        stake_id: Hash,
        penalty_percent: u8,
    ) -> Result<Balance, ReputationError> {
        let stake = self.stakes
            .get_mut(&stake_id)
            .ok_or(ReputationError::StakeNotFound)?;

        let penalty = stake.slash(penalty_percent);

        self.total_staked = self.total_staked.saturating_sub(penalty);

        self.events.push(ReputationEvent::StakeSlashed {
            stake_id,
            penalty,
        });

        Ok(penalty)
    }

    /// Get stake multiplier for identity
    pub fn get_stake_multiplier(&self, identity_id: &IdentityId) -> u32 {
        self.stakes_by_identity
            .get(identity_id)
            .map(|stake_ids| {
                stake_ids.iter()
                    .filter_map(|id| self.stakes.get(id))
                    .filter(|s| !s.slashed)
                    .map(|s| s.multiplier - 100) // Extra multiplier above base
                    .max()
                    .map(|extra| 100 + extra) // Add back base
                    .unwrap_or(100)
            })
            .unwrap_or(100)
    }

    /// Get total staked for identity
    pub fn get_total_staked(&self, identity_id: &IdentityId) -> Balance {
        self.stakes_by_identity
            .get(identity_id)
            .map(|stake_ids| {
                stake_ids.iter()
                    .filter_map(|id| self.stakes.get(id))
                    .map(|s| s.amount)
                    .sum()
            })
            .unwrap_or(0)
    }

    // =========================================================================
    // CROSS-CHAIN REPUTATION
    // =========================================================================

    /// Import reputation from another chain
    pub fn import_reputation(
        &mut self,
        source_chain: ChainId,
        source_identity: IdentityId,
        target_identity: IdentityId,
        original_score: u32,
        domain_scores: HashMap<ReputationDomain, u32>,
        proof_hash: Hash,
        current_block: BlockNumber,
    ) -> Result<(), ReputationError> {
        if !self.config.cross_chain_enabled {
            return Err(ReputationError::CrossChainDisabled);
        }

        // Check no existing import from this chain
        let key = (target_identity, source_chain);
        if self.cross_chain_imports.contains_key(&key) {
            return Err(ReputationError::ImportAlreadyExists);
        }

        let import = CrossChainReputation::new(
            source_chain,
            source_identity,
            self.chain_id,
            original_score,
            domain_scores,
            proof_hash,
            current_block,
        );

        let imported_score = import.imported_score;
        self.cross_chain_imports.insert(key, import);

        self.events.push(ReputationEvent::ReputationImported {
            source_chain,
            target_chain: self.chain_id,
            identity_id: target_identity,
            imported_score,
        });

        Ok(())
    }

    /// Verify cross-chain import
    pub fn verify_import(
        &mut self,
        identity_id: IdentityId,
        source_chain: ChainId,
    ) -> Result<(), ReputationError> {
        let key = (identity_id, source_chain);
        let import = self.cross_chain_imports
            .get_mut(&key)
            .ok_or(ReputationError::ImportNotFound)?;

        import.verify();
        Ok(())
    }

    /// Get cross-chain imports for identity
    pub fn get_imports(&self, identity_id: &IdentityId) -> Vec<&CrossChainReputation> {
        self.cross_chain_imports
            .iter()
            .filter(|((id, _), _)| id == identity_id)
            .map(|(_, import)| import)
            .collect()
    }

    // =========================================================================
    // ENDORSEMENTS
    // =========================================================================

    /// Give an endorsement
    pub fn give_endorsement(
        &mut self,
        endorser: AccountId,
        endorser_identity: IdentityId,
        target_identity: IdentityId,
        domain: ReputationDomain,
        context: Option<String>,
        current_block: BlockNumber,
        identity_registry: &IdentityRegistry,
    ) -> Result<Hash, ReputationError> {
        if !self.config.endorsements_enabled {
            return Err(ReputationError::EndorsementsDisabled);
        }

        // Verify endorser has identity
        if !identity_registry.owner_is_active(&endorser, current_block) {
            return Err(ReputationError::EndorserNotActive);
        }

        // Cannot self-endorse
        if endorser_identity == target_identity {
            return Err(ReputationError::CannotSelfEndorse);
        }

        // Check endorser has enough reputation
        let endorser_rep = self.get_effective_score(&endorser_identity);
        if endorser_rep < self.config.min_rep_to_endorse {
            return Err(ReputationError::InsufficientReputation);
        }

        // Check max endorsements per domain
        let received = self.endorsements_received
            .get(&target_identity)
            .and_then(|d| d.get(&domain))
            .map(|e| e.len())
            .unwrap_or(0);

        if received >= MAX_ENDORSEMENTS_PER_DOMAIN {
            return Err(ReputationError::MaxEndorsementsReached);
        }

        // Create endorsement
        let endorsement = Endorsement::new(
            endorser,
            endorser_identity,
            target_identity,
            domain,
            endorser_rep,
            context,
            current_block,
        );

        let endorsement_id = endorsement.endorsement_id;
        let weight = endorsement.weight;

        // Store
        self.endorsements.insert(endorsement_id, endorsement);
        self.endorsements_given
            .entry(endorser_identity)
            .or_default()
            .push(endorsement_id);
        self.endorsements_received
            .entry(target_identity)
            .or_default()
            .entry(domain)
            .or_default()
            .push(endorsement_id);

        // Add to target's domain reputation
        if let Some(rep) = self.reputations.get_mut(&target_identity) {
            rep.add_endorsement(domain, endorser_rep);
        }

        self.events.push(ReputationEvent::EndorsementGiven {
            endorser,
            target: target_identity,
            domain,
            weight,
        });

        Ok(endorsement_id)
    }

    /// Revoke an endorsement
    pub fn revoke_endorsement(
        &mut self,
        endorsement_id: Hash,
        revoker: AccountId,
    ) -> Result<(), ReputationError> {
        let endorsement = self.endorsements
            .get_mut(&endorsement_id)
            .ok_or(ReputationError::EndorsementNotFound)?;

        if endorsement.endorser != revoker {
            return Err(ReputationError::NotEndorser);
        }

        endorsement.revoke();

        self.events.push(ReputationEvent::EndorsementRevoked {
            endorsement_id,
        });

        Ok(())
    }

    /// Get endorsements given by identity
    pub fn get_endorsements_given(&self, identity_id: &IdentityId) -> Vec<&Endorsement> {
        self.endorsements_given
            .get(identity_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.endorsements.get(id))
                    .filter(|e| e.active)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get endorsements received by identity in domain
    pub fn get_endorsements_received(
        &self,
        identity_id: &IdentityId,
        domain: ReputationDomain,
    ) -> Vec<&Endorsement> {
        self.endorsements_received
            .get(identity_id)
            .and_then(|d| d.get(&domain))
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.endorsements.get(id))
                    .filter(|e| e.active)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Count total endorsements received
    pub fn count_endorsements(&self, identity_id: &IdentityId) -> u32 {
        self.endorsements_received
            .get(identity_id)
            .map(|domains| {
                domains.values()
                    .flat_map(|ids| ids.iter())
                    .filter_map(|id| self.endorsements.get(id))
                    .filter(|e| e.active)
                    .count() as u32
            })
            .unwrap_or(0)
    }

    // =========================================================================
    // EVENTS
    // =========================================================================

    /// Get events
    pub fn events(&self) -> &[ReputationEvent] {
        &self.events
    }

    /// Clear events
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Get total staked amount
    pub fn total_staked(&self) -> Balance {
        self.total_staked
    }
}

/// Reputation errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReputationError {
    #[error("Reputation not found for identity")]
    ReputationNotFound,

    #[error("Staking is disabled")]
    StakingDisabled,

    #[error("Too many stakes for this identity")]
    TooManyStakes,

    #[error("Stake amount too small")]
    StakeTooSmall,

    #[error("Stake not found")]
    StakeNotFound,

    #[error("Not the staker")]
    NotStaker,

    #[error("Stake is still locked")]
    StakeLocked,

    #[error("Cross-chain imports are disabled")]
    CrossChainDisabled,

    #[error("Import already exists from this chain")]
    ImportAlreadyExists,

    #[error("Import not found")]
    ImportNotFound,

    #[error("Endorsements are disabled")]
    EndorsementsDisabled,

    #[error("Endorser identity is not active")]
    EndorserNotActive,

    #[error("Cannot endorse yourself")]
    CannotSelfEndorse,

    #[error("Insufficient reputation to endorse")]
    InsufficientReputation,

    #[error("Maximum endorsements reached for this domain")]
    MaxEndorsementsReached,

    #[error("Endorsement not found")]
    EndorsementNotFound,

    #[error("Not the endorser")]
    NotEndorser,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::identity::IdentityRegistry;
    use crate::types::identity::IdentityStatus;

    fn create_account(seed: u8) -> AccountId {
        AccountId::from_bytes([seed; 32])
    }

    fn create_hash(seed: u8) -> Hash {
        Hash::hash(&[seed; 32])
    }

    fn setup_with_identity() -> (ReputationRegistry, IdentityRegistry, IdentityId, AccountId) {
        let mut identity_registry = IdentityRegistry::new(ChainId(1));
        let owner = create_account(1);

        let id = identity_registry.declare_identity(owner, b"data", None, 1000).unwrap();
        identity_registry.force_activate(&id);

        let mut reputation_registry = ReputationRegistry::new(ChainId(1));
        reputation_registry.initialize(id, 1000);

        (reputation_registry, identity_registry, id, owner)
    }

    #[test]
    fn test_initialize_reputation() {
        let mut registry = ReputationRegistry::new(ChainId(1));
        let id = create_hash(1);

        registry.initialize(id, 1000);

        let rep = registry.get_reputation(&id);
        assert!(rep.is_some());
        assert_eq!(rep.unwrap().overall_score, 0);
    }

    #[test]
    fn test_add_domain_reputation() {
        let (mut registry, _, id, _) = setup_with_identity();

        registry.add_reputation(
            id,
            ReputationDomain::Governance,
            100,
            "voted".to_string(),
            2000,
        ).unwrap();

        let score = registry.get_domain_score(&id, ReputationDomain::Governance);
        assert_eq!(score, 100);
    }

    #[test]
    fn test_slash_reputation() {
        let (mut registry, _, id, _) = setup_with_identity();

        // Add some reputation first
        registry.add_reputation(id, ReputationDomain::Technical, 1000, "code".to_string(), 2000).unwrap();

        // Slash 50%
        registry.slash_reputation(id, ReputationDomain::Technical, 50, "bug".to_string()).unwrap();

        let score = registry.get_domain_score(&id, ReputationDomain::Technical);
        assert_eq!(score, 500);
    }

    #[test]
    fn test_create_stake() {
        let (mut registry, _, id, owner) = setup_with_identity();

        let stake_id = registry.create_stake(
            owner,
            id,
            MIN_REPUTATION_STAKE,
            None,
            2000,
        ).unwrap();

        assert!(registry.stakes.contains_key(&stake_id));
        assert_eq!(registry.total_staked(), MIN_REPUTATION_STAKE);
    }

    #[test]
    fn test_stake_too_small() {
        let (mut registry, _, id, owner) = setup_with_identity();

        let result = registry.create_stake(
            owner,
            id,
            MIN_REPUTATION_STAKE - 1,
            None,
            2000,
        );

        assert!(matches!(result, Err(ReputationError::StakeTooSmall)));
    }

    #[test]
    fn test_stake_multiplier() {
        let (mut registry, _, id, owner) = setup_with_identity();

        // No stake = 100 (1x)
        assert_eq!(registry.get_stake_multiplier(&id), 100);

        // Add stake
        registry.create_stake(owner, id, MIN_REPUTATION_STAKE * 4, None, 2000).unwrap();

        // Should be > 100
        let multiplier = registry.get_stake_multiplier(&id);
        assert!(multiplier > 100);
    }

    #[test]
    fn test_withdraw_stake() {
        let (mut registry, _, id, owner) = setup_with_identity();

        let stake_id = registry.create_stake(owner, id, MIN_REPUTATION_STAKE, None, 2000).unwrap();

        // Cannot withdraw while locked
        let result = registry.withdraw_stake(stake_id, owner, 2000);
        assert!(matches!(result, Err(ReputationError::StakeLocked)));

        // Can withdraw after lock period
        let amount = registry.withdraw_stake(stake_id, owner, 2000 + REPUTATION_STAKE_LOCK).unwrap();
        assert_eq!(amount, MIN_REPUTATION_STAKE);
        assert_eq!(registry.total_staked(), 0);
    }

    #[test]
    fn test_slash_stake() {
        let (mut registry, _, id, owner) = setup_with_identity();

        let stake_id = registry.create_stake(owner, id, 1000, None, 2000).unwrap();

        let penalty = registry.slash_stake(stake_id, 30).unwrap();

        assert_eq!(penalty, 300);
        assert_eq!(registry.total_staked(), 700);
    }

    #[test]
    fn test_import_cross_chain() {
        let (mut registry, _, target_id, _) = setup_with_identity();

        registry.import_reputation(
            ChainId(2), // Source chain
            create_hash(99), // Source identity
            target_id,
            1000, // Original score
            HashMap::new(),
            create_hash(100), // Proof hash
            3000,
        ).unwrap();

        let imports = registry.get_imports(&target_id);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].imported_score, 500); // 50% discount
    }

    #[test]
    fn test_import_duplicate() {
        let (mut registry, _, target_id, _) = setup_with_identity();

        registry.import_reputation(
            ChainId(2),
            create_hash(99),
            target_id,
            1000,
            HashMap::new(),
            create_hash(100),
            3000,
        ).unwrap();

        // Second import from same chain should fail
        let result = registry.import_reputation(
            ChainId(2),
            create_hash(99),
            target_id,
            2000,
            HashMap::new(),
            create_hash(101),
            3001,
        );

        assert!(matches!(result, Err(ReputationError::ImportAlreadyExists)));
    }

    #[test]
    fn test_verify_import() {
        let (mut registry, _, target_id, _) = setup_with_identity();

        registry.import_reputation(
            ChainId(2),
            create_hash(99),
            target_id,
            1000,
            HashMap::new(),
            create_hash(100),
            3000,
        ).unwrap();

        // Not verified initially
        let imports = registry.get_imports(&target_id);
        assert!(!imports[0].verified);

        // Verify
        registry.verify_import(target_id, ChainId(2)).unwrap();

        let imports = registry.get_imports(&target_id);
        assert!(imports[0].verified);
    }

    #[test]
    fn test_give_endorsement() {
        let (mut registry, mut identity_registry, target_id, _) = setup_with_identity();

        // Create endorser
        let endorser_account = create_account(2);
        let endorser_id = identity_registry.declare_identity(endorser_account, b"endorser", None, 1000).unwrap();
        identity_registry.force_activate(&endorser_id);

        // Give endorser some reputation
        registry.initialize(endorser_id, 1000);
        registry.add_reputation(endorser_id, ReputationDomain::Community, 500, "init".to_string(), 2000).unwrap();

        // Give endorsement
        let endorsement_id = registry.give_endorsement(
            endorser_account,
            endorser_id,
            target_id,
            ReputationDomain::Technical,
            Some("Great coder".to_string()),
            3000,
            &identity_registry,
        ).unwrap();

        assert!(registry.endorsements.contains_key(&endorsement_id));
        assert_eq!(registry.count_endorsements(&target_id), 1);
    }

    #[test]
    fn test_cannot_self_endorse() {
        let (mut registry, identity_registry, id, owner) = setup_with_identity();

        // Give some reputation
        registry.add_reputation(id, ReputationDomain::Community, 500, "init".to_string(), 2000).unwrap();

        let result = registry.give_endorsement(
            owner,
            id,
            id, // Same identity
            ReputationDomain::Technical,
            None,
            3000,
            &identity_registry,
        );

        assert!(matches!(result, Err(ReputationError::CannotSelfEndorse)));
    }

    #[test]
    fn test_revoke_endorsement() {
        let (mut registry, mut identity_registry, target_id, _) = setup_with_identity();

        // Create endorser
        let endorser_account = create_account(2);
        let endorser_id = identity_registry.declare_identity(endorser_account, b"endorser", None, 1000).unwrap();
        identity_registry.force_activate(&endorser_id);
        registry.initialize(endorser_id, 1000);
        registry.add_reputation(endorser_id, ReputationDomain::Community, 500, "init".to_string(), 2000).unwrap();

        let endorsement_id = registry.give_endorsement(
            endorser_account,
            endorser_id,
            target_id,
            ReputationDomain::Technical,
            None,
            3000,
            &identity_registry,
        ).unwrap();

        // Revoke
        registry.revoke_endorsement(endorsement_id, endorser_account).unwrap();

        let endorsement = registry.endorsements.get(&endorsement_id).unwrap();
        assert!(!endorsement.active);
    }

    #[test]
    fn test_effective_score_with_stake() {
        let (mut registry, _, id, owner) = setup_with_identity();

        // Add base reputation
        registry.add_reputation(id, ReputationDomain::Governance, 1000, "vote".to_string(), 2000).unwrap();

        let score_without_stake = registry.get_effective_score(&id);

        // Add stake
        registry.create_stake(owner, id, MIN_REPUTATION_STAKE * 9, None, 3000).unwrap();

        let score_with_stake = registry.get_effective_score(&id);

        assert!(score_with_stake > score_without_stake);
    }

    #[test]
    fn test_effective_score_with_cross_chain() {
        let (mut registry, _, id, _) = setup_with_identity();

        // Add base reputation
        registry.add_reputation(id, ReputationDomain::Governance, 1000, "vote".to_string(), 2000).unwrap();

        let score_without_import = registry.get_effective_score(&id);

        // Import cross-chain reputation
        registry.import_reputation(
            ChainId(2),
            create_hash(99),
            id,
            2000,
            HashMap::new(),
            create_hash(100),
            3000,
        ).unwrap();
        registry.verify_import(id, ChainId(2)).unwrap();

        let score_with_import = registry.get_effective_score(&id);

        assert!(score_with_import > score_without_import);
    }
}
