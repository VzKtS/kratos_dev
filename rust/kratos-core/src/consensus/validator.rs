// Validator - Gestion des validateurs pour le consensus
use crate::types::{AccountId, Balance, BlockNumber};
use crate::types::contributor::{NetworkRoleRegistry, RoleRegistryError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Montant minimum pour devenir validateur
/// SECURITY FIX #40: Aligned with SPEC 1 §8.1 Stake Floor
/// Bootstrap: 50,000 KRAT (strict minimum to ensure economic security)
/// Post-Bootstrap: 25,000 KRAT (with VC reduction available)
/// Using bootstrap value as the MIN constant for maximum security
pub const MIN_VALIDATOR_STAKE: Balance = 50_000 * crate::types::primitives::KRAT;

/// Période d'unbonding (en blocs)
/// Ex: 28 jours = 28 * 24 * 3600 / 6 = 403,200 blocs
pub const UNBONDING_PERIOD: BlockNumber = 403_200;

/// Bootstrap era duration (in blocks)
/// After this period, bootstrap validators must have staked MIN_VALIDATOR_STAKE
/// 60 days = 60 * 24 * 3600 / 6 = 864,000 blocks (= 1440 epochs × 600 blocks)
/// Aligned with BOOTSTRAP_EPOCHS_MIN = 1440 from SPEC v2.3
pub const BOOTSTRAP_ERA_BLOCKS: BlockNumber = 864_000;

/// Grace period for bootstrap validators to stake after bootstrap era ends (in blocks)
/// Ex: 7 days = 7 * 24 * 3600 / 6 = 100,800 blocks
pub const BOOTSTRAP_GRACE_PERIOD: BlockNumber = 100_800;

/// Maximum number of early validators that can be added during bootstrap era
/// Constitutional limit to ensure controlled decentralization
pub const MAX_EARLY_VALIDATORS: usize = 21;

/// Expiration time for early validator candidacy (in blocks)
/// 7 days = 7 * 24 * 3600 / 6 = 100,800 blocks
pub const CANDIDACY_EXPIRATION: BlockNumber = 100_800;

// =============================================================================
// EARLY VALIDATOR VOTING SYSTEM
// Constitutional Principle: Progressive decentralization with vote-based inclusion
// During bootstrap, validators are added through voting with progressive thresholds
// =============================================================================

/// Candidacy status for early validator voting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidacyStatus {
    /// Pending votes
    Pending,
    /// Approved and added to validator set
    Approved,
    /// Rejected (expired or insufficient votes)
    Rejected,
    /// Expired (candidacy timeout)
    Expired,
}

/// Early validator candidate awaiting votes
/// Constitutional: Candidates must be voted in by existing validators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarlyValidatorCandidate {
    /// Candidate account
    pub candidate: AccountId,

    /// Who proposed the candidate (must be existing validator)
    pub proposer: AccountId,

    /// Set of validators who voted for this candidate
    pub voters: BTreeSet<AccountId>,

    /// Number of votes required for approval
    /// Dynamically calculated based on current validator count
    pub votes_required: usize,

    /// Block number when candidacy was created
    pub created_at: BlockNumber,

    /// Current status
    pub status: CandidacyStatus,

    /// Block number when approved (if approved)
    pub approved_at: Option<BlockNumber>,
}

impl EarlyValidatorCandidate {
    /// Create a new candidate
    pub fn new(
        candidate: AccountId,
        proposer: AccountId,
        votes_required: usize,
        created_at: BlockNumber,
    ) -> Self {
        let mut voters = BTreeSet::new();
        // Proposer automatically votes for the candidate
        voters.insert(proposer);

        Self {
            candidate,
            proposer,
            voters,
            votes_required,
            created_at,
            status: CandidacyStatus::Pending,
            approved_at: None,
        }
    }

    /// Add a vote from a validator
    pub fn add_vote(&mut self, voter: AccountId) -> bool {
        self.voters.insert(voter)
    }

    /// Check if candidate has enough votes
    pub fn has_quorum(&self) -> bool {
        self.voters.len() >= self.votes_required
    }

    /// Current vote count
    pub fn vote_count(&self) -> usize {
        self.voters.len()
    }

    /// Check if candidacy has expired
    pub fn is_expired(&self, current_block: BlockNumber) -> bool {
        current_block > self.created_at + CANDIDACY_EXPIRATION
    }

    /// Check if a specific validator has voted
    pub fn has_voted(&self, voter: &AccountId) -> bool {
        self.voters.contains(voter)
    }
}

/// Statut d'un validateur
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidatorStatus {
    /// Actif et participe au consensus
    Active,
    /// Inactif volontairement
    Inactive,
    /// En cours de débonding
    Unbonding,
    /// Slashé (pénalisé)
    Slashed,
    /// Banni (jailed)
    Jailed,
}

/// Informations d'un validateur
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    /// Identifiant (clé publique)
    pub id: AccountId,

    /// Montant staké
    pub stake: Balance,

    /// Statut
    pub status: ValidatorStatus,

    /// Score de réputation (0-100)
    pub reputation: u8,

    /// Blocs produits
    pub blocks_produced: u64,

    /// Blocs manqués
    pub blocks_missed: u32,

    /// Nombre de fois slashé
    pub slash_count: u32,

    /// Montant total slashé
    pub total_slashed: Balance,

    /// Bloc d'enregistrement
    pub registered_at: BlockNumber,

    /// Bloc où unbonding sera complété (si applicable)
    pub unbonding_at: Option<BlockNumber>,

    /// Bootstrap validator flag (SPEC v2.1)
    /// Bootstrap validator can produce blocks without stake during bootstrap era
    #[serde(default)]
    pub is_bootstrap_validator: bool,
}

impl ValidatorInfo {
    pub fn new(id: AccountId, stake: Balance, block: BlockNumber) -> Self {
        Self {
            id,
            stake,
            status: ValidatorStatus::Active,
            reputation: 100, // Commence à 100
            blocks_produced: 0,
            blocks_missed: 0,
            slash_count: 0,
            total_slashed: 0,
            registered_at: block,
            unbonding_at: None,
            is_bootstrap_validator: false,
        }
    }

    /// Creates a bootstrap validator (SPEC v2.1)
    /// Bootstrap validator can produce blocks without stake during bootstrap era
    pub fn new_bootstrap(id: AccountId, block: BlockNumber) -> Self {
        Self {
            id,
            stake: 0,
            status: ValidatorStatus::Active,
            reputation: 100,
            blocks_produced: 0,
            blocks_missed: 0,
            slash_count: 0,
            total_slashed: 0,
            registered_at: block,
            unbonding_at: None,
            is_bootstrap_validator: true,
        }
    }

    /// Vérifie si le validateur peut participer au consensus
    /// Bootstrap validators can participate without stake only during bootstrap era (SPEC v2.1)
    /// After bootstrap era + grace period, they must have staked MIN_VALIDATOR_STAKE
    ///
    /// Note: For proper block validation, use can_participate_at(block_number) which
    /// accounts for bootstrap era timing. This method allows bootstrap validators
    /// to participate during the bootstrap period.
    pub fn can_participate(&self) -> bool {
        matches!(self.status, ValidatorStatus::Active)
            && (self.stake >= MIN_VALIDATOR_STAKE || self.is_bootstrap_validator)
            && self.reputation > 0
    }

    /// Check if validator can participate at a specific block height
    /// This accounts for bootstrap era timing
    pub fn can_participate_at(&self, current_block: BlockNumber) -> bool {
        if !matches!(self.status, ValidatorStatus::Active) || self.reputation == 0 {
            return false;
        }

        // Regular validators need minimum stake
        if self.stake >= MIN_VALIDATOR_STAKE {
            return true;
        }

        // Bootstrap validators can participate during bootstrap era + grace period
        if self.is_bootstrap_validator {
            let bootstrap_end = BOOTSTRAP_ERA_BLOCKS + BOOTSTRAP_GRACE_PERIOD;
            return current_block < bootstrap_end;
        }

        false
    }

    /// Check if this bootstrap validator needs to transition (stake or be removed)
    /// Returns true if bootstrap era has ended and validator hasn't staked enough
    pub fn needs_bootstrap_transition(&self, current_block: BlockNumber) -> bool {
        self.is_bootstrap_validator
            && self.stake < MIN_VALIDATOR_STAKE
            && current_block >= BOOTSTRAP_ERA_BLOCKS
    }

    /// Check if bootstrap validator is in grace period
    pub fn is_in_grace_period(&self, current_block: BlockNumber) -> bool {
        self.is_bootstrap_validator
            && self.stake < MIN_VALIDATOR_STAKE
            && current_block >= BOOTSTRAP_ERA_BLOCKS
            && current_block < BOOTSTRAP_ERA_BLOCKS + BOOTSTRAP_GRACE_PERIOD
    }

    /// Check if bootstrap validator has exceeded grace period without staking
    /// These validators should be removed from the active set
    pub fn exceeded_grace_period(&self, current_block: BlockNumber) -> bool {
        self.is_bootstrap_validator
            && self.stake < MIN_VALIDATOR_STAKE
            && current_block >= BOOTSTRAP_ERA_BLOCKS + BOOTSTRAP_GRACE_PERIOD
    }

    /// Transition bootstrap validator to regular validator status
    /// Called when bootstrap validator has staked MIN_VALIDATOR_STAKE
    pub fn transition_from_bootstrap(&mut self) {
        if self.is_bootstrap_validator && self.stake >= MIN_VALIDATOR_STAKE {
            self.is_bootstrap_validator = false;
        }
    }

    /// Add stake to validator (for bootstrap transition)
    pub fn add_stake(&mut self, amount: Balance) {
        self.stake = self.stake.saturating_add(amount);
        // Auto-transition if bootstrap validator reaches minimum stake
        if self.is_bootstrap_validator && self.stake >= MIN_VALIDATOR_STAKE {
            self.transition_from_bootstrap();
        }
    }

    /// Enregistre la production d'un bloc
    pub fn record_block_produced(&mut self) {
        self.blocks_produced += 1;
        // Augmente légèrement la réputation (max 100)
        self.reputation = (self.reputation + 1).min(100);
    }

    /// Enregistre un bloc manqué
    pub fn record_block_missed(&mut self) {
        self.blocks_missed += 1;
        // Diminue légèrement la réputation
        self.reputation = self.reputation.saturating_sub(1);
    }

    /// Applique un slash
    pub fn slash(&mut self, amount: Balance) -> Balance {
        let slashed = amount.min(self.stake);
        self.stake = self.stake.saturating_sub(slashed);
        self.total_slashed = self.total_slashed.saturating_add(slashed);
        self.slash_count += 1;
        self.reputation = self.reputation.saturating_sub(20);

        // Si stake tombe en dessous du minimum, inactive
        if self.stake < MIN_VALIDATOR_STAKE {
            self.status = ValidatorStatus::Slashed;
        }

        slashed
    }

    /// Démarre le unbonding
    pub fn start_unbonding(&mut self, current_block: BlockNumber) {
        self.status = ValidatorStatus::Unbonding;
        self.unbonding_at = Some(current_block + UNBONDING_PERIOD);
    }

    /// Vérifie si le unbonding est complété
    pub fn is_unbonding_completed(&self, current_block: BlockNumber) -> bool {
        if let Some(unbonding_at) = self.unbonding_at {
            current_block >= unbonding_at
        } else {
            false
        }
    }
}

/// Set de validateurs pour une epoch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSet {
    /// Validateurs actifs, indexés par leur ID
    pub validators: BTreeMap<AccountId, ValidatorInfo>,

    /// Stake total
    pub total_stake: Balance,

    /// Unified network role registry (tracks all roles: Validator, Juror, Contributor)
    #[serde(default)]
    pub role_registry: NetworkRoleRegistry,

    /// Early validator candidates (bootstrap era only)
    /// Constitutional: Progressive voting for decentralization
    #[serde(default)]
    pub early_candidates: BTreeMap<AccountId, EarlyValidatorCandidate>,
}

impl ValidatorSet {
    pub fn new() -> Self {
        Self {
            validators: BTreeMap::new(),
            total_stake: 0,
            role_registry: NetworkRoleRegistry::new(),
            early_candidates: BTreeMap::new(),
        }
    }

    /// Ajoute un validateur
    /// Bootstrap validators can be added with 0 stake (SPEC v2.1)
    /// Also registers the validator role in the unified NetworkRoleRegistry
    pub fn add_validator(&mut self, validator: ValidatorInfo) -> Result<(), ValidatorError> {
        if validator.stake < MIN_VALIDATOR_STAKE && !validator.is_bootstrap_validator {
            return Err(ValidatorError::InsufficientStake);
        }

        // Register in unified role registry
        if let Err(e) = self.role_registry.register_validator(
            validator.id,
            validator.stake,
            validator.is_bootstrap_validator,
            validator.registered_at,
        ) {
            // Map RoleRegistryError to ValidatorError
            return match e {
                RoleRegistryError::AlreadyRegistered => Err(ValidatorError::AlreadyRegistered),
                _ => Err(ValidatorError::CannotParticipate),
            };
        }

        self.total_stake = self.total_stake.saturating_add(validator.stake);
        self.validators.insert(validator.id, validator);
        Ok(())
    }

    /// Retire un validateur
    /// Also unregisters the validator role from the unified NetworkRoleRegistry
    pub fn remove_validator(&mut self, id: &AccountId) -> Result<ValidatorInfo, ValidatorError> {
        let validator = self
            .validators
            .remove(id)
            .ok_or(ValidatorError::NotFound)?;

        // Unregister from unified role registry
        let _ = self.role_registry.unregister_validator(id);

        self.total_stake = self.total_stake.saturating_sub(validator.stake);
        Ok(validator)
    }

    /// Récupère un validateur
    pub fn get_validator(&self, id: &AccountId) -> Option<&ValidatorInfo> {
        self.validators.get(id)
    }

    /// Récupère un validateur (mutable)
    pub fn get_validator_mut(&mut self, id: &AccountId) -> Option<&mut ValidatorInfo> {
        self.validators.get_mut(id)
    }

    /// Ensure a bootstrap validator is registered and active.
    /// If the validator already exists, update them to be a bootstrap validator.
    /// If not, add them as a new bootstrap validator.
    /// This is used during initial sync when we discover validators from blocks.
    pub fn ensure_bootstrap_validator(&mut self, id: AccountId, block: BlockNumber) -> Result<(), ValidatorError> {
        if let Some(existing) = self.validators.get_mut(&id) {
            // Validator exists - ensure they're set up as bootstrap and active
            if !existing.can_participate() {
                existing.is_bootstrap_validator = true;
                existing.status = ValidatorStatus::Active;
                existing.reputation = 100;
            }
            Ok(())
        } else {
            // Validator doesn't exist - add them
            let validator = ValidatorInfo::new_bootstrap(id, block);

            // Try to register in role registry, ignore AlreadyRegistered error
            if let Err(e) = self.role_registry.register_validator(
                validator.id,
                validator.stake,
                validator.is_bootstrap_validator,
                validator.registered_at,
            ) {
                match e {
                    RoleRegistryError::AlreadyRegistered => {
                        // Already in registry, just add to validators map
                    }
                    _ => return Err(ValidatorError::CannotParticipate),
                }
            }

            self.total_stake = self.total_stake.saturating_add(validator.stake);
            self.validators.insert(id, validator);
            Ok(())
        }
    }

    /// Nombre de validateurs actifs (without block context - for backwards compatibility)
    pub fn active_count(&self) -> usize {
        self.validators
            .values()
            .filter(|v| v.can_participate())
            .count()
    }

    /// Number of validators that can participate at a specific block height
    pub fn active_count_at(&self, current_block: BlockNumber) -> usize {
        self.validators
            .values()
            .filter(|v| v.can_participate_at(current_block))
            .count()
    }

    /// Liste des validateurs qui peuvent participer (without block context)
    pub fn active_validators(&self) -> Vec<&ValidatorInfo> {
        self.validators
            .values()
            .filter(|v| v.can_participate())
            .collect()
    }

    /// List of validators that can participate at a specific block height
    pub fn active_validators_at(&self, current_block: BlockNumber) -> Vec<&ValidatorInfo> {
        self.validators
            .values()
            .filter(|v| v.can_participate_at(current_block))
            .collect()
    }

    /// Vérifie si un validateur est actif (without block context)
    pub fn is_active(&self, id: &AccountId) -> bool {
        self.validators
            .get(id)
            .map(|v| v.can_participate())
            .unwrap_or(false)
    }

    /// Check if validator is active at a specific block height
    pub fn is_active_at(&self, id: &AccountId, current_block: BlockNumber) -> bool {
        self.validators
            .get(id)
            .map(|v| v.can_participate_at(current_block))
            .unwrap_or(false)
    }

    /// Process bootstrap era transitions
    /// Should be called at epoch boundaries to clean up bootstrap validators
    /// Returns list of validators that were removed due to exceeding grace period
    pub fn process_bootstrap_transitions(&mut self, current_block: BlockNumber) -> Vec<AccountId> {
        let mut removed = Vec::new();

        // Find validators that have exceeded grace period
        let to_remove: Vec<AccountId> = self
            .validators
            .iter()
            .filter(|(_, v)| v.exceeded_grace_period(current_block))
            .map(|(id, _)| *id)
            .collect();

        // Remove them
        for id in to_remove {
            if let Ok(validator) = self.remove_validator(&id) {
                tracing::warn!(
                    "Bootstrap validator {} removed: exceeded grace period without staking (stake: {}, required: {})",
                    id,
                    validator.stake,
                    MIN_VALIDATOR_STAKE
                );
                removed.push(id);
            }
        }

        removed
    }

    /// Get bootstrap validators that need to stake (in grace period)
    pub fn bootstrap_validators_in_grace_period(&self, current_block: BlockNumber) -> Vec<&ValidatorInfo> {
        self.validators
            .values()
            .filter(|v| v.is_in_grace_period(current_block))
            .collect()
    }

    /// Check if network is still in bootstrap era
    pub fn is_bootstrap_era(current_block: BlockNumber) -> bool {
        current_block < BOOTSTRAP_ERA_BLOCKS
    }

    /// Get number of bootstrap validators still active
    pub fn bootstrap_validator_count(&self) -> usize {
        self.validators
            .values()
            .filter(|v| v.is_bootstrap_validator)
            .count()
    }

    // =========================================================================
    // NETWORK ROLE REGISTRY INTEGRATION
    // =========================================================================

    /// Update validator stake in both ValidatorInfo and NetworkRoleRegistry
    pub fn update_validator_stake(&mut self, id: &AccountId, amount: Balance) -> Result<(), ValidatorError> {
        let validator = self.validators.get_mut(id).ok_or(ValidatorError::NotFound)?;

        let old_stake = validator.stake;
        validator.add_stake(amount);

        // Update total stake
        self.total_stake = self.total_stake.saturating_sub(old_stake).saturating_add(validator.stake);

        // Sync to role registry
        let _ = self.role_registry.update_validator_stake(id, validator.stake);

        Ok(())
    }

    /// Get access to the unified role registry
    pub fn role_registry(&self) -> &NetworkRoleRegistry {
        &self.role_registry
    }

    /// Get mutable access to the unified role registry
    pub fn role_registry_mut(&mut self) -> &mut NetworkRoleRegistry {
        &mut self.role_registry
    }

    /// Check if account has validator role in the unified registry
    pub fn has_validator_role(&self, account: &AccountId) -> bool {
        self.role_registry.is_validator(account)
    }

    /// Get validator count from the unified registry
    pub fn role_registry_validator_count(&self) -> usize {
        self.role_registry.validator_count()
    }

    // =========================================================================
    // EARLY VALIDATOR VOTING SYSTEM
    // Constitutional: Progressive decentralization through voting
    // =========================================================================

    /// Calculate votes required based on current validator count
    /// Constitutional principle: Progressive thresholds for security
    ///
    /// - Validators 1-2: Bootstrap validator votes alone (1 vote)
    /// - Validators 3-5: Require 3 votes (majority of small set)
    /// - Validators 6-10: Require 4 votes
    /// - Validators 11+: Require 5 votes (sovereign threshold)
    pub fn votes_required_for_new_validator(&self) -> usize {
        let current_count = self.active_count();
        match current_count {
            0..=2 => 1,   // Bootstrap phase: bootstrap validator decides
            3..=5 => 3,   // Small set: 3 votes (60-100%)
            6..=10 => 4,  // Medium set: 4 votes (40-67%)
            _ => 5,       // Large set: 5 votes (sovereign minimum)
        }
    }

    /// Propose a new early validator candidate
    /// Only during bootstrap era, only by active validators
    pub fn propose_early_validator(
        &mut self,
        candidate: AccountId,
        proposer: AccountId,
        current_block: BlockNumber,
    ) -> Result<(), ValidatorError> {
        // Security: Only during bootstrap era
        if !Self::is_bootstrap_era(current_block) {
            return Err(ValidatorError::BootstrapEnded);
        }

        // Security: Check maximum validator limit
        if self.active_count() >= MAX_EARLY_VALIDATORS {
            return Err(ValidatorError::MaxValidatorsReached);
        }

        // Security: Proposer must be active validator
        if !self.is_active(&proposer) {
            return Err(ValidatorError::NotValidator);
        }

        // Security: Candidate must not already be a validator
        if self.validators.contains_key(&candidate) {
            return Err(ValidatorError::AlreadyRegistered);
        }

        // Security: Candidate must not already be pending
        if self.early_candidates.contains_key(&candidate) {
            return Err(ValidatorError::CandidacyExists);
        }

        // Create candidacy with calculated vote threshold
        let votes_required = self.votes_required_for_new_validator();
        let candidacy = EarlyValidatorCandidate::new(
            candidate,
            proposer,
            votes_required,
            current_block,
        );

        self.early_candidates.insert(candidate, candidacy);

        tracing::info!(
            "Early validator candidacy proposed: {} by {} (votes needed: {})",
            candidate, proposer, votes_required
        );

        Ok(())
    }

    /// Vote for an early validator candidate
    /// Only active validators can vote, each validator votes once
    pub fn vote_early_validator(
        &mut self,
        candidate: AccountId,
        voter: AccountId,
        current_block: BlockNumber,
    ) -> Result<bool, ValidatorError> {
        // Security: Only during bootstrap era
        if !Self::is_bootstrap_era(current_block) {
            return Err(ValidatorError::BootstrapEnded);
        }

        // Security: Voter must be active validator
        if !self.is_active(&voter) {
            return Err(ValidatorError::NotValidator);
        }

        // Get candidacy
        let candidacy = self.early_candidates
            .get_mut(&candidate)
            .ok_or(ValidatorError::CandidacyNotFound)?;

        // Security: Check expiration
        if candidacy.is_expired(current_block) {
            candidacy.status = CandidacyStatus::Expired;
            return Err(ValidatorError::CandidacyExpired);
        }

        // Security: Candidacy must still be pending
        if candidacy.status != CandidacyStatus::Pending {
            return Err(ValidatorError::CandidacyNotPending);
        }

        // Security: Voter must not have already voted
        if candidacy.has_voted(&voter) {
            return Err(ValidatorError::AlreadyVoted);
        }

        // Add vote
        candidacy.add_vote(voter);

        tracing::info!(
            "Vote added for early validator {}: {}/{} votes",
            candidate, candidacy.vote_count(), candidacy.votes_required
        );

        // Check if quorum reached
        Ok(candidacy.has_quorum())
    }

    /// Approve an early validator candidate and add to validator set
    /// Called when quorum is reached
    pub fn approve_early_validator(
        &mut self,
        candidate: AccountId,
        current_block: BlockNumber,
    ) -> Result<(), ValidatorError> {
        // First, validate candidacy without keeping borrow
        let vote_count = {
            let candidacy = self.early_candidates
                .get(&candidate)
                .ok_or(ValidatorError::CandidacyNotFound)?;

            if !candidacy.has_quorum() {
                return Err(ValidatorError::InsufficientVotes);
            }

            if candidacy.status != CandidacyStatus::Pending {
                return Err(ValidatorError::CandidacyNotPending);
            }

            candidacy.vote_count()
        };

        // Create bootstrap validator (0 stake during bootstrap)
        let validator = ValidatorInfo::new_bootstrap(candidate, current_block);

        // Add to validator set (with is_bootstrap_validator = true)
        self.add_validator(validator)?;

        // Now update candidacy status
        if let Some(candidacy) = self.early_candidates.get_mut(&candidate) {
            candidacy.status = CandidacyStatus::Approved;
            candidacy.approved_at = Some(current_block);
        }

        tracing::info!(
            "Early validator approved and added: {} (votes: {})",
            candidate, vote_count
        );

        Ok(())
    }

    /// Get pending early validator candidates
    pub fn pending_candidates(&self) -> Vec<&EarlyValidatorCandidate> {
        self.early_candidates
            .values()
            .filter(|c| c.status == CandidacyStatus::Pending)
            .collect()
    }

    /// Get candidate by account
    pub fn get_candidate(&self, candidate: &AccountId) -> Option<&EarlyValidatorCandidate> {
        self.early_candidates.get(candidate)
    }

    /// Clean up expired candidacies
    pub fn cleanup_expired_candidacies(&mut self, current_block: BlockNumber) -> Vec<AccountId> {
        let expired: Vec<AccountId> = self.early_candidates
            .iter()
            .filter(|(_, c)| c.is_expired(current_block) && c.status == CandidacyStatus::Pending)
            .map(|(id, _)| *id)
            .collect();

        for id in &expired {
            if let Some(c) = self.early_candidates.get_mut(id) {
                c.status = CandidacyStatus::Expired;
            }
        }

        expired
    }

    /// Check if account is an early validator (bootstrap validator added via voting)
    pub fn is_early_validator(&self, account: &AccountId) -> bool {
        // Check if they're a validator AND their candidacy was approved
        self.validators.get(account)
            .map(|v| v.is_bootstrap_validator)
            .unwrap_or(false)
            && self.early_candidates.get(account)
                .map(|c| c.status == CandidacyStatus::Approved)
                .unwrap_or(false)
    }

    /// Check if account can vote for early validators
    /// Returns true if account is an active bootstrap validator during bootstrap era
    pub fn can_vote_early_validator(&self, account: &AccountId, current_block: BlockNumber) -> bool {
        Self::is_bootstrap_era(current_block) && self.is_active_at(account, current_block)
    }
}

impl Default for ValidatorSet {
    fn default() -> Self {
        Self::new()
    }
}

/// Erreurs de validateur
#[derive(Debug, thiserror::Error)]
pub enum ValidatorError {
    #[error("Stake insuffisant (minimum: {MIN_VALIDATOR_STAKE})")]
    InsufficientStake,

    #[error("Validateur non trouvé")]
    NotFound,

    #[error("Validateur déjà enregistré")]
    AlreadyRegistered,

    #[error("Validateur ne peut pas participer")]
    CannotParticipate,

    #[error("Unbonding non complété")]
    UnbondingNotCompleted,

    // Early validator voting errors
    #[error("Bootstrap era ended - voting no longer allowed")]
    BootstrapEnded,

    #[error("Maximum number of validators reached ({MAX_EARLY_VALIDATORS})")]
    MaxValidatorsReached,

    #[error("Account is not a validator")]
    NotValidator,

    #[error("Candidacy already exists for this account")]
    CandidacyExists,

    #[error("Candidacy not found")]
    CandidacyNotFound,

    #[error("Candidacy has expired")]
    CandidacyExpired,

    #[error("Candidacy is not pending")]
    CandidacyNotPending,

    #[error("Voter has already voted for this candidate")]
    AlreadyVoted,

    #[error("Insufficient votes for approval")]
    InsufficientVotes,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_creation() {
        let id = AccountId::from_bytes([1; 32]);
        let validator = ValidatorInfo::new(id, MIN_VALIDATOR_STAKE, 0);

        assert_eq!(validator.status, ValidatorStatus::Active);
        assert_eq!(validator.reputation, 100);
        assert!(validator.can_participate());
    }

    #[test]
    fn test_validator_slash() {
        let id = AccountId::from_bytes([1; 32]);
        let mut validator = ValidatorInfo::new(id, MIN_VALIDATOR_STAKE, 0);

        let slashed = validator.slash(1000 * crate::types::primitives::KRAT);
        assert_eq!(slashed, 1000 * crate::types::primitives::KRAT);
        assert_eq!(validator.slash_count, 1);
        assert_eq!(validator.reputation, 80); // 100 - 20
    }

    #[test]
    fn test_validator_set() {
        let mut set = ValidatorSet::new();

        let id1 = AccountId::from_bytes([1; 32]);
        let validator1 = ValidatorInfo::new(id1, MIN_VALIDATOR_STAKE, 0);

        assert!(set.add_validator(validator1).is_ok());
        assert_eq!(set.active_count(), 1);
        assert_eq!(set.total_stake, MIN_VALIDATOR_STAKE);
    }

    #[test]
    fn test_bootstrap_validator() {
        // Bootstrap validator can be created with 0 stake (SPEC v2.1)
        let id = AccountId::from_bytes([42; 32]);
        let bootstrap = ValidatorInfo::new_bootstrap(id, 0);

        assert_eq!(bootstrap.stake, 0);
        assert!(bootstrap.is_bootstrap_validator);
        assert_eq!(bootstrap.status, ValidatorStatus::Active);

        // Bootstrap validators CAN participate (they have is_bootstrap_validator flag)
        assert!(bootstrap.can_participate());
        // And can also participate during bootstrap era with block context
        assert!(bootstrap.can_participate_at(0));
        assert!(bootstrap.can_participate_at(BOOTSTRAP_ERA_BLOCKS - 1));

        // Can add bootstrap validator to set
        let mut set = ValidatorSet::new();
        assert!(set.add_validator(bootstrap).is_ok());
        assert_eq!(set.active_count_at(0), 1);
        assert_eq!(set.total_stake, 0); // No stake added
        assert!(set.is_active_at(&id, 0));
    }

    #[test]
    fn test_bootstrap_validator_transition() {
        let id = AccountId::from_bytes([42; 32]);
        let mut bootstrap = ValidatorInfo::new_bootstrap(id, 0);

        // During bootstrap era
        assert!(bootstrap.can_participate_at(0));
        assert!(!bootstrap.needs_bootstrap_transition(0));

        // After bootstrap era starts (in grace period)
        assert!(bootstrap.can_participate_at(BOOTSTRAP_ERA_BLOCKS));
        assert!(bootstrap.needs_bootstrap_transition(BOOTSTRAP_ERA_BLOCKS));
        assert!(bootstrap.is_in_grace_period(BOOTSTRAP_ERA_BLOCKS));
        assert!(!bootstrap.exceeded_grace_period(BOOTSTRAP_ERA_BLOCKS));

        // After grace period ends
        let after_grace = BOOTSTRAP_ERA_BLOCKS + BOOTSTRAP_GRACE_PERIOD;
        assert!(!bootstrap.can_participate_at(after_grace));
        assert!(bootstrap.exceeded_grace_period(after_grace));

        // If bootstrap validator stakes, they transition to regular
        bootstrap.add_stake(MIN_VALIDATOR_STAKE);
        assert!(!bootstrap.is_bootstrap_validator);
        assert!(bootstrap.can_participate());
        assert!(bootstrap.can_participate_at(after_grace));
    }

    #[test]
    fn test_bootstrap_validator_set_cleanup() {
        let mut set = ValidatorSet::new();

        // Add bootstrap validator
        let id = AccountId::from_bytes([42; 32]);
        let bootstrap = ValidatorInfo::new_bootstrap(id, 0);
        set.add_validator(bootstrap).unwrap();

        // Add regular validator
        let regular_id = AccountId::from_bytes([1; 32]);
        let regular = ValidatorInfo::new(regular_id, MIN_VALIDATOR_STAKE, 0);
        set.add_validator(regular).unwrap();

        // During bootstrap era, both can participate
        assert_eq!(set.active_count_at(0), 2);

        // After grace period, only regular can participate
        let after_grace = BOOTSTRAP_ERA_BLOCKS + BOOTSTRAP_GRACE_PERIOD;
        assert_eq!(set.active_count_at(after_grace), 1);

        // Process transitions removes the bootstrap validator
        let removed = set.process_bootstrap_transitions(after_grace);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0], id);
        assert_eq!(set.validators.len(), 1);
        assert!(set.get_validator(&regular_id).is_some());
    }

    #[test]
    fn test_non_bootstrap_insufficient_stake_rejected() {
        let mut set = ValidatorSet::new();
        let id = AccountId::from_bytes([1; 32]);

        // Non-bootstrap validator with 0 stake should be rejected
        let mut validator = ValidatorInfo::new(id, 0, 0);
        validator.stake = 0;

        assert!(set.add_validator(validator).is_err());
    }

    #[test]
    fn test_bootstrap_era_check() {
        assert!(ValidatorSet::is_bootstrap_era(0));
        assert!(ValidatorSet::is_bootstrap_era(BOOTSTRAP_ERA_BLOCKS - 1));
        assert!(!ValidatorSet::is_bootstrap_era(BOOTSTRAP_ERA_BLOCKS));
        assert!(!ValidatorSet::is_bootstrap_era(BOOTSTRAP_ERA_BLOCKS + 1));
    }

    // =========================================================================
    // NETWORK ROLE REGISTRY INTEGRATION TESTS
    // =========================================================================

    #[test]
    fn test_validator_registered_in_role_registry() {
        let mut set = ValidatorSet::new();

        let id = AccountId::from_bytes([1; 32]);
        let validator = ValidatorInfo::new(id, MIN_VALIDATOR_STAKE, 100);

        // Before adding: not in registry
        assert!(!set.has_validator_role(&id));
        assert_eq!(set.role_registry_validator_count(), 0);

        // Add validator
        assert!(set.add_validator(validator).is_ok());

        // After adding: should be in registry
        assert!(set.has_validator_role(&id));
        assert_eq!(set.role_registry_validator_count(), 1);

        // Should also be able to get the entry
        let entry = set.role_registry().get_validator_entry(&id);
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(entry.active);
    }

    #[test]
    fn test_validator_unregistered_from_role_registry() {
        let mut set = ValidatorSet::new();

        let id = AccountId::from_bytes([1; 32]);
        let validator = ValidatorInfo::new(id, MIN_VALIDATOR_STAKE, 100);

        set.add_validator(validator).unwrap();
        assert!(set.has_validator_role(&id));

        // Remove validator
        let removed = set.remove_validator(&id);
        assert!(removed.is_ok());

        // After removing: should NOT be active in registry
        assert!(!set.has_validator_role(&id));
    }

    #[test]
    fn test_bootstrap_validator_in_role_registry() {
        let mut set = ValidatorSet::new();

        let id = AccountId::from_bytes([42; 32]);
        let bootstrap = ValidatorInfo::new_bootstrap(id, 0);

        set.add_validator(bootstrap).unwrap();

        // Bootstrap validator should be in registry
        assert!(set.has_validator_role(&id));

        // Entry should show is_bootstrap = true
        let entry = set.role_registry().get_validator_entry(&id);
        assert!(entry.is_some());
    }

    #[test]
    fn test_stake_update_synced_to_registry() {
        let mut set = ValidatorSet::new();

        let id = AccountId::from_bytes([1; 32]);
        let validator = ValidatorInfo::new(id, MIN_VALIDATOR_STAKE, 100);

        set.add_validator(validator).unwrap();

        // Update stake
        let additional_stake = 5000 * crate::types::primitives::KRAT;
        assert!(set.update_validator_stake(&id, additional_stake).is_ok());

        // Verify ValidatorInfo has updated stake
        let validator = set.get_validator(&id).unwrap();
        assert_eq!(validator.stake, MIN_VALIDATOR_STAKE + additional_stake);
    }

    #[test]
    fn test_multiple_validators_in_registry() {
        let mut set = ValidatorSet::new();

        let id1 = AccountId::from_bytes([1; 32]);
        let id2 = AccountId::from_bytes([2; 32]);
        let id3 = AccountId::from_bytes([3; 32]);

        set.add_validator(ValidatorInfo::new(id1, MIN_VALIDATOR_STAKE, 100)).unwrap();
        set.add_validator(ValidatorInfo::new(id2, MIN_VALIDATOR_STAKE, 100)).unwrap();
        set.add_validator(ValidatorInfo::new(id3, MIN_VALIDATOR_STAKE, 100)).unwrap();

        assert_eq!(set.role_registry_validator_count(), 3);
        assert!(set.has_validator_role(&id1));
        assert!(set.has_validator_role(&id2));
        assert!(set.has_validator_role(&id3));

        // Remove one
        set.remove_validator(&id2).unwrap();
        assert_eq!(set.role_registry_validator_count(), 2);
        assert!(set.has_validator_role(&id1));
        assert!(!set.has_validator_role(&id2));
        assert!(set.has_validator_role(&id3));
    }

    #[test]
    fn test_duplicate_validator_rejected() {
        let mut set = ValidatorSet::new();

        let id = AccountId::from_bytes([1; 32]);
        let validator1 = ValidatorInfo::new(id, MIN_VALIDATOR_STAKE, 100);
        let validator2 = ValidatorInfo::new(id, MIN_VALIDATOR_STAKE, 200);

        // First add should succeed
        assert!(set.add_validator(validator1).is_ok());

        // Second add should fail (already registered)
        let result = set.add_validator(validator2);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ValidatorError::AlreadyRegistered));
    }

    // =========================================================================
    // EARLY VALIDATOR VOTING TESTS
    // =========================================================================

    #[test]
    fn test_early_validator_candidacy() {
        let mut set = ValidatorSet::new();

        // Add bootstrap validator
        let bootstrap = AccountId::from_bytes([1; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(bootstrap, 0)).unwrap();

        // Propose candidate
        let candidate = AccountId::from_bytes([2; 32]);
        let result = set.propose_early_validator(candidate, bootstrap, 100);
        assert!(result.is_ok());

        // Check candidacy exists
        let candidacy = set.get_candidate(&candidate);
        assert!(candidacy.is_some());
        let candidacy = candidacy.unwrap();
        assert_eq!(candidacy.candidate, candidate);
        assert_eq!(candidacy.proposer, bootstrap);
        assert_eq!(candidacy.status, CandidacyStatus::Pending);
        assert_eq!(candidacy.vote_count(), 1); // Proposer auto-votes
    }

    #[test]
    fn test_early_validator_voting_thresholds() {
        let mut set = ValidatorSet::new();

        // With 1 validator: need 1 vote
        let v1 = AccountId::from_bytes([1; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(v1, 0)).unwrap();
        assert_eq!(set.votes_required_for_new_validator(), 1);

        // With 2 validators: still need 1 vote
        let v2 = AccountId::from_bytes([2; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(v2, 1)).unwrap();
        assert_eq!(set.votes_required_for_new_validator(), 1);

        // With 3 validators: need 3 votes
        let v3 = AccountId::from_bytes([3; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(v3, 2)).unwrap();
        assert_eq!(set.votes_required_for_new_validator(), 3);

        // Add more validators
        for i in 4..=6 {
            let vi = AccountId::from_bytes([i as u8; 32]);
            set.add_validator(ValidatorInfo::new_bootstrap(vi, i as u64)).unwrap();
        }
        assert_eq!(set.votes_required_for_new_validator(), 4); // 6 validators: need 4

        // Add more to reach 11+
        for i in 7..=11 {
            let vi = AccountId::from_bytes([i as u8; 32]);
            set.add_validator(ValidatorInfo::new_bootstrap(vi, i as u64)).unwrap();
        }
        assert_eq!(set.votes_required_for_new_validator(), 5); // 11 validators: need 5
    }

    #[test]
    fn test_early_validator_vote_and_approve() {
        let mut set = ValidatorSet::new();

        // Add bootstrap validator
        let bootstrap = AccountId::from_bytes([1; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(bootstrap, 0)).unwrap();

        // With 1 validator, only 1 vote needed
        let candidate = AccountId::from_bytes([10; 32]);
        set.propose_early_validator(candidate, bootstrap, 100).unwrap();

        // Proposer auto-voted, so quorum should be reached
        let candidacy = set.get_candidate(&candidate).unwrap();
        assert!(candidacy.has_quorum());

        // Approve
        let result = set.approve_early_validator(candidate, 101);
        assert!(result.is_ok());

        // Candidate should now be a validator
        assert!(set.validators.contains_key(&candidate));
        assert!(set.is_active(&candidate));

        // Candidacy should be approved
        let candidacy = set.get_candidate(&candidate).unwrap();
        assert_eq!(candidacy.status, CandidacyStatus::Approved);
    }

    #[test]
    fn test_early_validator_multiple_votes_required() {
        let mut set = ValidatorSet::new();

        // Add 3 bootstrap validators (so we need 3 votes)
        let v1 = AccountId::from_bytes([1; 32]);
        let v2 = AccountId::from_bytes([2; 32]);
        let v3 = AccountId::from_bytes([3; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(v1, 0)).unwrap();
        set.add_validator(ValidatorInfo::new_bootstrap(v2, 1)).unwrap();
        set.add_validator(ValidatorInfo::new_bootstrap(v3, 2)).unwrap();

        // Propose candidate
        let candidate = AccountId::from_bytes([10; 32]);
        set.propose_early_validator(candidate, v1, 100).unwrap();

        // V1 auto-voted, so 1/3 votes
        let candidacy = set.get_candidate(&candidate).unwrap();
        assert_eq!(candidacy.vote_count(), 1);
        assert!(!candidacy.has_quorum());

        // V2 votes
        let has_quorum = set.vote_early_validator(candidate, v2, 101).unwrap();
        assert!(!has_quorum);
        assert_eq!(set.get_candidate(&candidate).unwrap().vote_count(), 2);

        // V3 votes - now has quorum
        let has_quorum = set.vote_early_validator(candidate, v3, 102).unwrap();
        assert!(has_quorum);

        // Approve
        set.approve_early_validator(candidate, 103).unwrap();
        assert!(set.is_active(&candidate));
    }

    #[test]
    fn test_early_validator_cannot_vote_twice() {
        let mut set = ValidatorSet::new();

        let v1 = AccountId::from_bytes([1; 32]);
        let v2 = AccountId::from_bytes([2; 32]);
        let v3 = AccountId::from_bytes([3; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(v1, 0)).unwrap();
        set.add_validator(ValidatorInfo::new_bootstrap(v2, 1)).unwrap();
        set.add_validator(ValidatorInfo::new_bootstrap(v3, 2)).unwrap();

        let candidate = AccountId::from_bytes([10; 32]);
        set.propose_early_validator(candidate, v1, 100).unwrap();

        // V1 already voted (as proposer), so can't vote again
        let result = set.vote_early_validator(candidate, v1, 101);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ValidatorError::AlreadyVoted));
    }

    #[test]
    fn test_early_validator_bootstrap_era_required() {
        let mut set = ValidatorSet::new();

        let bootstrap = AccountId::from_bytes([1; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(bootstrap, 0)).unwrap();

        let candidate = AccountId::from_bytes([10; 32]);

        // Try to propose after bootstrap era
        let result = set.propose_early_validator(candidate, bootstrap, BOOTSTRAP_ERA_BLOCKS + 1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ValidatorError::BootstrapEnded));
    }

    #[test]
    fn test_early_validator_candidacy_expiration() {
        let mut set = ValidatorSet::new();

        let v1 = AccountId::from_bytes([1; 32]);
        let v2 = AccountId::from_bytes([2; 32]);
        let v3 = AccountId::from_bytes([3; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(v1, 0)).unwrap();
        set.add_validator(ValidatorInfo::new_bootstrap(v2, 1)).unwrap();
        set.add_validator(ValidatorInfo::new_bootstrap(v3, 2)).unwrap();

        let candidate = AccountId::from_bytes([10; 32]);
        set.propose_early_validator(candidate, v1, 100).unwrap();

        // Try to vote after expiration
        let result = set.vote_early_validator(candidate, v2, 100 + CANDIDACY_EXPIRATION + 1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ValidatorError::CandidacyExpired));
    }

    #[test]
    fn test_early_validator_non_validator_cannot_propose() {
        let mut set = ValidatorSet::new();

        let bootstrap = AccountId::from_bytes([1; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(bootstrap, 0)).unwrap();

        // Non-validator tries to propose
        let non_validator = AccountId::from_bytes([99; 32]);
        let candidate = AccountId::from_bytes([10; 32]);

        let result = set.propose_early_validator(candidate, non_validator, 100);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ValidatorError::NotValidator));
    }

    #[test]
    fn test_early_validator_pending_candidates() {
        let mut set = ValidatorSet::new();

        let bootstrap = AccountId::from_bytes([1; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(bootstrap, 0)).unwrap();

        // Create multiple candidates
        let c1 = AccountId::from_bytes([10; 32]);
        let c2 = AccountId::from_bytes([11; 32]);

        set.propose_early_validator(c1, bootstrap, 100).unwrap();
        set.propose_early_validator(c2, bootstrap, 101).unwrap();

        // Both should be pending
        let pending = set.pending_candidates();
        assert_eq!(pending.len(), 2);

        // Approve one
        set.approve_early_validator(c1, 102).unwrap();

        // Now only one pending
        let pending = set.pending_candidates();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].candidate, c2);
    }

    #[test]
    fn test_cleanup_expired_candidacies() {
        let mut set = ValidatorSet::new();

        let v1 = AccountId::from_bytes([1; 32]);
        let v2 = AccountId::from_bytes([2; 32]);
        let v3 = AccountId::from_bytes([3; 32]);
        set.add_validator(ValidatorInfo::new_bootstrap(v1, 0)).unwrap();
        set.add_validator(ValidatorInfo::new_bootstrap(v2, 1)).unwrap();
        set.add_validator(ValidatorInfo::new_bootstrap(v3, 2)).unwrap();

        let candidate = AccountId::from_bytes([10; 32]);
        set.propose_early_validator(candidate, v1, 100).unwrap();

        // Before expiration: should be pending
        assert_eq!(set.pending_candidates().len(), 1);

        // After expiration: cleanup should mark it expired
        let expired = set.cleanup_expired_candidacies(100 + CANDIDACY_EXPIRATION + 1);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], candidate);

        // Now no pending candidates
        assert_eq!(set.pending_candidates().len(), 0);

        // Candidacy should be marked expired
        let candidacy = set.get_candidate(&candidate).unwrap();
        assert_eq!(candidacy.status, CandidacyStatus::Expired);
    }
}
