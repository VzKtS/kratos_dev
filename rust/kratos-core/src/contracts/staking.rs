// Staking - System contract pour la gestion du staking
use crate::consensus::validator::{ValidatorInfo, ValidatorSet};
use crate::types::{AccountId, Balance, BlockNumber};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// SECURITY FIX #31: Consolidate constants - use single source of truth from validator module
// Re-export to maintain backward compatibility but prevent divergence
pub use crate::consensus::validator::{MIN_VALIDATOR_STAKE, UNBONDING_PERIOD};

/// Registre de staking
///
/// # Thread Safety
/// SECURITY NOTE: This struct is NOT internally thread-safe. All concurrent access
/// MUST be protected by an external synchronization primitive (e.g., Arc<RwLock<StakingRegistry>>).
/// The KratOsNode wraps this in Arc<RwLock<>> - see node/service.rs.
///
/// Operations like `start_unbonding` perform check-then-act patterns that require
/// the entire operation to be atomic. Do NOT call methods from multiple threads
/// without holding a write lock for the entire operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingRegistry {
    /// Validateurs actifs
    validators: ValidatorSet,

    /// Requests d'unbonding en attente
    unbonding_requests: HashMap<AccountId, UnbondingRequest>,

    /// Historique des slashes
    slash_history: Vec<SlashRecord>,
}

impl StakingRegistry {
    pub fn new() -> Self {
        Self {
            validators: ValidatorSet::new(),
            unbonding_requests: HashMap::new(),
            slash_history: Vec::new(),
        }
    }

    /// Enregistre un nouveau validateur
    pub fn register_validator(
        &mut self,
        id: AccountId,
        stake: Balance,
        block: BlockNumber,
    ) -> Result<(), StakingError> {
        if stake < MIN_VALIDATOR_STAKE {
            return Err(StakingError::InsufficientStake);
        }

        let validator = ValidatorInfo::new(id, stake, block);
        self.validators.add_validator(validator)?;

        Ok(())
    }

    /// Désenregistre un validateur
    pub fn unregister_validator(&mut self, id: &AccountId) -> Result<(), StakingError> {
        let _validator = self.validators.remove_validator(id)?;

        // Le stake reste en unbonding
        Ok(())
    }

    /// Ajoute du stake à un validateur existant
    pub fn add_stake(&mut self, id: &AccountId, amount: Balance) -> Result<(), StakingError> {
        let validator = self
            .validators
            .get_validator_mut(id)
            .ok_or(StakingError::ValidatorNotFound)?;

        validator.stake = validator.stake.saturating_add(amount);
        self.validators.total_stake = self.validators.total_stake.saturating_add(amount);

        Ok(())
    }

    /// Démarre le unbonding
    /// SECURITY FIX #10: Immediately reduce stake to prevent race conditions
    /// The stake is locked during unbonding and cannot be used for validation
    pub fn start_unbonding(
        &mut self,
        id: &AccountId,
        amount: Balance,
        current_block: BlockNumber,
    ) -> Result<(), StakingError> {
        // SECURITY FIX #10: Check if there's already an active unbonding request
        // Only one unbonding request per account at a time to prevent abuse
        if self.unbonding_requests.contains_key(id) {
            return Err(StakingError::UnbondingAlreadyActive);
        }

        let validator = self
            .validators
            .get_validator_mut(id)
            .ok_or(StakingError::ValidatorNotFound)?;

        if validator.stake < amount {
            return Err(StakingError::InsufficientStake);
        }

        // SECURITY FIX #10: Immediately reduce stake to prevent double-counting
        // This ensures the unbonding stake cannot be used for validation weight
        validator.stake = validator.stake.saturating_sub(amount);
        self.validators.total_stake = self.validators.total_stake.saturating_sub(amount);

        // Crée la requête d'unbonding
        let request = UnbondingRequest {
            account: *id,
            amount,
            requested_at: current_block,
            ready_at: current_block + UNBONDING_PERIOD,
        };

        self.unbonding_requests.insert(*id, request);

        Ok(())
    }

    /// Retire les tokens après unbonding
    /// SECURITY FIX #10: Stake was already reduced in start_unbonding
    /// This just releases the locked funds to the user
    pub fn withdraw_unbonded(
        &mut self,
        id: &AccountId,
        current_block: BlockNumber,
    ) -> Result<Balance, StakingError> {
        let request = self
            .unbonding_requests
            .get(id)
            .ok_or(StakingError::NoUnbondingRequest)?;

        if current_block < request.ready_at {
            return Err(StakingError::UnbondingNotReady);
        }

        let amount = request.amount;

        // SECURITY FIX #10: Stake was already reduced in start_unbonding
        // No need to reduce again - just remove the unbonding request
        // The stake reduction happened atomically when unbonding started

        // Supprime la requête
        self.unbonding_requests.remove(id);

        Ok(amount)
    }

    /// Slash un validateur
    /// SECURITY FIX #32: Update total_stake when slashing to maintain consistency
    pub fn slash_validator(
        &mut self,
        id: &AccountId,
        amount: Balance,
        reason: SlashReason,
        block: BlockNumber,
    ) -> Result<Balance, StakingError> {
        let validator = self
            .validators
            .get_validator_mut(id)
            .ok_or(StakingError::ValidatorNotFound)?;

        let slashed = validator.slash(amount);

        // SECURITY FIX #32: Update total_stake to reflect the slashed amount
        // This ensures the validator set's total stake remains consistent
        self.validators.total_stake = self.validators.total_stake.saturating_sub(slashed);

        // Enregistre dans l'historique
        self.slash_history.push(SlashRecord {
            validator: *id,
            amount: slashed,
            reason,
            block,
        });

        // Garde seulement les 1000 derniers slashes
        if self.slash_history.len() > 1000 {
            self.slash_history.remove(0);
        }

        Ok(slashed)
    }

    /// Récupère les validateurs actifs
    pub fn active_validators(&self) -> Vec<&ValidatorInfo> {
        self.validators.active_validators()
    }

    /// Récupère un validateur
    pub fn get_validator(&self, id: &AccountId) -> Option<&ValidatorInfo> {
        self.validators.get_validator(id)
    }

    /// Stake total
    pub fn total_stake(&self) -> Balance {
        self.validators.total_stake
    }

    /// Nombre de validateurs
    pub fn validator_count(&self) -> usize {
        self.validators.validators.len()
    }
}

impl Default for StakingRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Requête d'unbonding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnbondingRequest {
    pub account: AccountId,
    pub amount: Balance,
    pub requested_at: BlockNumber,
    pub ready_at: BlockNumber,
}

/// Raisons de slash
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SlashReason {
    DoubleSign,
    ExtendedDowntime,
    InvalidAttestation,
    MaliciousBehavior,
    FailedEmergencyResponse,
}

/// Enregistrement de slash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashRecord {
    pub validator: AccountId,
    pub amount: Balance,
    pub reason: SlashReason,
    pub block: BlockNumber,
}

/// Erreurs de staking
#[derive(Debug, thiserror::Error)]
pub enum StakingError {
    #[error("Stake insuffisant")]
    InsufficientStake,

    #[error("Validateur non trouvé")]
    ValidatorNotFound,

    #[error("Pas de requête d'unbonding")]
    NoUnbondingRequest,

    #[error("Unbonding pas encore prêt")]
    UnbondingNotReady,

    /// SECURITY FIX #10: Prevent multiple concurrent unbonding requests
    #[error("Une requête d'unbonding est déjà active")]
    UnbondingAlreadyActive,

    #[error("Erreur ValidatorSet: {0}")]
    ValidatorSetError(#[from] crate::consensus::validator::ValidatorError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_validator() {
        let mut registry = StakingRegistry::new();
        let validator_id = AccountId::from_bytes([1; 32]);

        let result = registry.register_validator(validator_id, MIN_VALIDATOR_STAKE, 0);
        assert!(result.is_ok());

        assert_eq!(registry.validator_count(), 1);
        assert_eq!(registry.total_stake(), MIN_VALIDATOR_STAKE);
    }

    #[test]
    fn test_insufficient_stake() {
        let mut registry = StakingRegistry::new();
        let validator_id = AccountId::from_bytes([1; 32]);

        // Essaie avec moins que le minimum
        let result = registry.register_validator(validator_id, MIN_VALIDATOR_STAKE - 1, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_unbonding_flow() {
        let mut registry = StakingRegistry::new();
        let validator_id = AccountId::from_bytes([1; 32]);

        // Enregistre
        registry
            .register_validator(validator_id, MIN_VALIDATOR_STAKE, 0)
            .unwrap();

        // Démarre unbonding
        registry
            .start_unbonding(&validator_id, 1000 * crate::types::KRAT, 0)
            .unwrap();

        // Essaie de retirer trop tôt
        let result = registry.withdraw_unbonded(&validator_id, 100);
        assert!(result.is_err());

        // Attend la période d'unbonding
        let result = registry.withdraw_unbonded(&validator_id, UNBONDING_PERIOD);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1000 * crate::types::KRAT);
    }

    #[test]
    fn test_slash_validator() {
        let mut registry = StakingRegistry::new();
        let validator_id = AccountId::from_bytes([1; 32]);

        registry
            .register_validator(validator_id, MIN_VALIDATOR_STAKE, 0)
            .unwrap();

        let slash_amount = 1000 * crate::types::KRAT;
        let slashed = registry
            .slash_validator(&validator_id, slash_amount, SlashReason::DoubleSign, 100)
            .unwrap();

        assert_eq!(slashed, slash_amount);
        assert_eq!(registry.slash_history.len(), 1);

        let validator = registry.get_validator(&validator_id).unwrap();
        assert_eq!(validator.stake, MIN_VALIDATOR_STAKE - slash_amount);
    }
}
