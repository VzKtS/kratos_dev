// Chain - Métadonnées des chaînes (root, sidechains, hostchains)
use super::account::AccountId;
use super::primitives::{Balance, BlockNumber, ChainId, Hash};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

/// Statut d'une sidechain (SPEC v3.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChainStatus {
    /// Active et opérationnelle
    Active,
    /// Inactive (pas d'activité récente)
    Inactive,
    /// En attente de purge (30 days warning - SPEC v3.1)
    PendingPurge,
    /// Frozen - no governance, no new transactions (SPEC v3.1)
    Frozen,
    /// Snapshot - final state root committed (SPEC v3.1)
    Snapshot,
    /// WithdrawalWindow - assets withdrawable only (30 days - SPEC v3.1)
    WithdrawalWindow,
    /// Purgée (supprimée) - state deleted, chain ID retired (SPEC v3.1)
    Purged,
}

/// Purge trigger reasons (SPEC v3.1 Section 2.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PurgeTrigger {
    /// ≥ 90 consecutive days of block inactivity (777,600 blocks at 6s/block)
    Inactivity,
    /// 3 consecutive failed governance votes
    GovernanceFailure,
    /// ≥ 33% of validators slashed for fraud
    ValidatorFraud,
    /// Unable to pay parent chain security fees
    SecurityInsolvency,
    /// Invalid state root detected by parent chain
    StateDivergence,
}

/// Security mode for sidechains (SPEC v3)
/// Determines validator assignment and deposit requirements
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityMode {
    /// Inherited: Validated by parent chain's validators
    /// - Cheapest option (1000 KRAT deposit)
    /// - Security inherited from parent
    /// - Best for child chains that trust parent
    Inherited,

    /// Shared: Validated by hostchain's shared validator pool
    /// - Moderate cost (1000 × N_members KRAT deposit)
    /// - Security shared across federation
    /// - Best for collaborative sidechains
    Shared,

    /// Sovereign: Own dedicated validator set
    /// - Most expensive (10,000 KRAT deposit)
    /// - Independent security
    /// - Best for high-value or isolated chains
    Sovereign,
}

/// Métadonnées d'une sidechain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidechainInfo {
    /// ID unique
    pub id: ChainId,

    /// Chaîne parente (optionnel)
    pub parent: Option<ChainId>,

    /// Propriétaire/créateur
    pub owner: AccountId,

    /// Nom (optionnel)
    pub name: Option<String>,

    /// Description (optionnelle)
    pub description: Option<String>,

    /// Security mode (SPEC v3)
    pub security_mode: SecurityMode,

    /// Validateurs assignés
    pub validators: BTreeSet<AccountId>,

    /// Statut
    pub status: ChainStatus,

    /// Bloc de création
    pub created_at: BlockNumber,

    /// Dernier bloc d'activité
    pub last_activity: BlockNumber,

    /// Dépôt bloqué
    pub deposit: Balance,

    /// Purge tracking (SPEC v3.1)
    /// Block when purge was triggered (None if not triggered)
    pub purge_triggered_at: Option<BlockNumber>,

    /// Purge trigger reason (None if not triggered)
    pub purge_trigger: Option<PurgeTrigger>,

    /// Block when state freeze occurred (None if not frozen)
    pub frozen_at: Option<BlockNumber>,

    /// Block when snapshot was taken (None if not snapshotted)
    pub snapshot_at: Option<BlockNumber>,

    /// Block when withdrawal window started (None if not in withdrawal)
    pub withdrawal_window_start: Option<BlockNumber>,

    /// Governance failure counter (for GovernanceFailure trigger)
    pub governance_failures: u32,

    /// Slashed validators count (for ValidatorFraud trigger)
    pub slashed_validators_count: usize,

    /// Last verified state root hash (SPEC v3.1 Phase 4 - State Divergence detection)
    /// None if no state root has been verified yet
    pub last_verified_state_root: Option<Hash>,

    /// Block number when state divergence was detected (None if no divergence)
    pub state_divergence_detected_at: Option<BlockNumber>,

    /// Snapshot state root hash (SPEC v3.1 Phase 4 - Frozen→Snapshot transition)
    /// This is the final committed state root before purge
    pub snapshot_state_root: Option<Hash>,

    /// SECURITY FIX #33: Track accounts that have already withdrawn
    /// Constitution Article I §6: "Exit is a fundamental right"
    /// Prevents double-withdrawal attacks
    #[serde(default)]
    pub withdrawn_accounts: HashSet<AccountId>,
}

impl SidechainInfo {
    pub fn new(
        id: ChainId,
        parent: Option<ChainId>,
        owner: AccountId,
        name: Option<String>,
        description: Option<String>,
        security_mode: SecurityMode,
        deposit: Balance,
        created_at: BlockNumber,
    ) -> Self {
        Self {
            id,
            parent,
            owner,
            name,
            description,
            security_mode,
            validators: BTreeSet::new(),
            status: ChainStatus::Active,
            created_at,
            last_activity: created_at,
            deposit,
            // SPEC v3.1 purge tracking (all None/0 for new chains)
            purge_triggered_at: None,
            purge_trigger: None,
            frozen_at: None,
            snapshot_at: None,
            withdrawal_window_start: None,
            governance_failures: 0,
            slashed_validators_count: 0,
            // SPEC v3.1 Phase 4: State divergence tracking
            last_verified_state_root: None,
            state_divergence_detected_at: None,
            snapshot_state_root: None,
            // SECURITY FIX #33: Withdrawal tracking
            withdrawn_accounts: HashSet::new(),
        }
    }

    /// Vérifie si la sidechain est inactive
    pub fn is_inactive(&self, current_block: BlockNumber, threshold: BlockNumber) -> bool {
        current_block.saturating_sub(self.last_activity) > threshold
    }

    /// Enregistre une activité
    pub fn record_activity(&mut self, block: BlockNumber) {
        self.last_activity = block;
        if self.status == ChainStatus::Inactive {
            self.status = ChainStatus::Active;
        }
    }

    /// Ajoute un validateur
    pub fn add_validator(&mut self, validator: AccountId) -> Result<(), ChainError> {
        if self.validators.len() >= MAX_VALIDATORS_PER_CHAIN {
            return Err(ChainError::TooManyValidators);
        }
        self.validators.insert(validator);
        Ok(())
    }

    /// Retire un validateur
    pub fn remove_validator(&mut self, validator: &AccountId) -> Result<(), ChainError> {
        if !self.validators.remove(validator) {
            return Err(ChainError::ValidatorNotFound);
        }
        Ok(())
    }
}

/// Métadonnées d'une hostchain (fédération de sidechains)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostChainInfo {
    /// ID unique
    pub id: ChainId,

    /// Créateur
    pub creator: AccountId,

    /// Sidechains membres
    pub member_chains: BTreeSet<ChainId>,

    /// Pool de validateurs partagés
    pub validator_pool: BTreeSet<AccountId>,

    /// Bloc de création
    pub created_at: BlockNumber,
}

impl HostChainInfo {
    pub fn new(id: ChainId, creator: AccountId, created_at: BlockNumber) -> Self {
        Self {
            id,
            creator,
            member_chains: BTreeSet::new(),
            validator_pool: BTreeSet::new(),
            created_at,
        }
    }

    /// Ajoute une sidechain membre
    pub fn add_member(&mut self, chain_id: ChainId) -> Result<(), ChainError> {
        if self.member_chains.len() >= MAX_MEMBERS_PER_HOST {
            return Err(ChainError::TooManyMembers);
        }
        self.member_chains.insert(chain_id);
        Ok(())
    }

    /// Retire une sidechain membre
    pub fn remove_member(&mut self, chain_id: &ChainId) -> Result<(), ChainError> {
        if !self.member_chains.remove(chain_id) {
            return Err(ChainError::MemberNotFound);
        }
        Ok(())
    }

    /// Ajoute un validateur au pool
    pub fn add_validator(&mut self, validator: AccountId) -> Result<(), ChainError> {
        if self.validator_pool.len() >= MAX_VALIDATORS_PER_HOST {
            return Err(ChainError::TooManyValidators);
        }
        self.validator_pool.insert(validator);
        Ok(())
    }

    /// Retire un validateur du pool
    pub fn remove_validator(&mut self, validator: &AccountId) -> Result<(), ChainError> {
        if !self.validator_pool.remove(validator) {
            return Err(ChainError::ValidatorNotFound);
        }
        Ok(())
    }
}

/// Raison de purge d'une sidechain
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PurgeReason {
    /// Inactivité prolongée
    Inactivity,
    /// Validateurs frauduleux
    FraudulentValidators,
    /// Vote de gouvernance
    GovernanceVote,
}

/// Constantes
pub const MAX_VALIDATORS_PER_CHAIN: usize = 100;
pub const MAX_MEMBERS_PER_HOST: usize = 100;
pub const MAX_VALIDATORS_PER_HOST: usize = 200;

/// Seuil d'inactivité (en blocs) - SPEC v3.1
/// 90 jours = 90 * 24 * 3600 / 6 = 1,296,000 blocs (à 6 sec/bloc)
pub const INACTIVITY_THRESHOLD_V3_1: BlockNumber = 1_296_000;

/// Legacy threshold (7 days) - kept for backward compatibility
pub const INACTIVITY_THRESHOLD: BlockNumber = 100_800;

/// Warning period before freeze (SPEC v3.1 Section 2.2)
/// 30 jours = 30 * 24 * 3600 / 6 = 432,000 blocs
pub const PURGE_WARNING_PERIOD: BlockNumber = 432_000;

/// Withdrawal window duration (SPEC v3.1 Section 2.3)
/// 30 jours = 30 * 24 * 3600 / 6 = 432,000 blocs
pub const WITHDRAWAL_WINDOW_DURATION: BlockNumber = 432_000;

/// Governance failure threshold (SPEC v3.1 Section 2.1)
/// 3 consecutive failed votes trigger purge
pub const GOVERNANCE_FAILURE_THRESHOLD: u32 = 3;

/// Validator fraud threshold (SPEC v3.1 Section 2.1)
/// ≥ 33% of validators slashed triggers purge
pub const VALIDATOR_FRAUD_THRESHOLD_PERCENT: usize = 33;

/// Base deposit for Inherited and Shared security modes (SPEC v3)
pub const BASE_DEPOSIT: Balance = 1_000;

/// Deposit for Sovereign security mode (SPEC v3)
pub const SOVEREIGN_DEPOSIT: Balance = 10_000;

/// Calculate required deposit based on security mode (SPEC v3)
///
/// Formula:
/// - Inherited: 1,000 KRAT (base)
/// - Shared: 1,000 × N_members KRAT (scales with federation size)
/// - Sovereign: 10,000 KRAT (highest cost for independence)
pub fn calculate_deposit(security_mode: SecurityMode, hostchain_members: usize) -> Balance {
    match security_mode {
        SecurityMode::Inherited => BASE_DEPOSIT,
        SecurityMode::Shared => BASE_DEPOSIT * (hostchain_members as Balance),
        SecurityMode::Sovereign => SOVEREIGN_DEPOSIT,
    }
}

/// Erreurs de chaîne
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("Trop de validateurs")]
    TooManyValidators,

    #[error("Validateur non trouvé")]
    ValidatorNotFound,

    #[error("Trop de membres")]
    TooManyMembers,

    #[error("Membre non trouvé")]
    MemberNotFound,

    #[error("Chaîne non trouvée")]
    NotFound,

    #[error("Chaîne déjà purgée")]
    AlreadyPurged,

    #[error("Non autorisé")]
    Unauthorized,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidechain_activity() {
        let owner = AccountId::from_bytes([1; 32]);
        let mut chain = SidechainInfo::new(
            ChainId(1),
            None,
            owner,
            Some("TestChain".to_string()),
            None,
            SecurityMode::Sovereign,
            1000,
            0,
        );

        assert!(!chain.is_inactive(50_000, INACTIVITY_THRESHOLD));
        assert!(chain.is_inactive(200_000, INACTIVITY_THRESHOLD));

        chain.record_activity(150_000);
        assert_eq!(chain.last_activity, 150_000);
    }

    #[test]
    fn test_hostchain_members() {
        let creator = AccountId::from_bytes([1; 32]);
        let mut host = HostChainInfo::new(ChainId(100), creator, 0);

        assert!(host.add_member(ChainId(1)).is_ok());
        assert!(host.add_member(ChainId(2)).is_ok());
        assert_eq!(host.member_chains.len(), 2);

        assert!(host.remove_member(&ChainId(1)).is_ok());
        assert_eq!(host.member_chains.len(), 1);
    }
}
