// Validator - Gestion des validateurs pour le consensus
use crate::types::{AccountId, Balance, BlockNumber};
use crate::types::contributor::{NetworkRoleRegistry, RoleRegistryError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
}

impl ValidatorSet {
    pub fn new() -> Self {
        Self {
            validators: BTreeMap::new(),
            total_stake: 0,
            role_registry: NetworkRoleRegistry::new(),
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
}
