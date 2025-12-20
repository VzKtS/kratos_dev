// Spécification du genesis block
use crate::consensus::validator::{ValidatorInfo, ValidatorSet};
use crate::contracts::krat::TokenomicsState;
use crate::storage::state::StateBackend;
use crate::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Spécification du genesis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisSpec {
    /// Timestamp du genesis
    pub timestamp: Timestamp,

    /// Comptes initiaux avec leurs balances
    pub balances: HashMap<AccountId, Balance>,

    /// Validateurs initiaux
    pub validators: Vec<GenesisValidator>,

    /// État tokenomics initial
    pub tokenomics: TokenomicsState,
}

/// Validateur dans le genesis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidator {
    pub account: AccountId,
    pub stake: Balance,
    /// Bootstrap validator flag (SPEC v2.1)
    /// Bootstrap validator has 0 stake and produces blocks during bootstrap era
    #[serde(default)]
    pub is_bootstrap_validator: bool,
}

/// Fixed genesis timestamp for deterministic genesis hash across nodes
/// 2025-01-01 00:00:00 UTC
const GENESIS_TIMESTAMP: u64 = 1735689600;

impl GenesisSpec {
    /// Unified KratOs genesis configuration
    /// Bootstrap validator per SPEC v2.1:
    /// - 0 stake initially
    /// - No KRAT balance (relies on block rewards)
    /// - Can produce blocks during bootstrap era by constitutional exception
    pub fn mainnet() -> Self {
        let balances = HashMap::new();

        // Bootstrap validator: 0x9a0c703c572e8d170c32ad0db9c953d2565efc3ed585ed57da9637655abf78b8
        // SPEC v2.1: Bootstrap validator has 0 stake and 0 initial balance
        let bootstrap_validator = AccountId::from_bytes([
            0x9a, 0x0c, 0x70, 0x3c, 0x57, 0x2e, 0x8d, 0x17,
            0x0c, 0x32, 0xad, 0x0d, 0xb9, 0xc9, 0x53, 0xd2,
            0x56, 0x5e, 0xfc, 0x3e, 0xd5, 0x85, 0xed, 0x57,
            0xda, 0x96, 0x37, 0x65, 0x5a, 0xbf, 0x78, 0xb8,
        ]);

        // No initial balance for bootstrap validator per SPEC v2.1
        // Bootstrap validator earns KRAT through block production rewards

        Self {
            timestamp: GENESIS_TIMESTAMP,
            balances,
            validators: vec![GenesisValidator {
                account: bootstrap_validator,
                stake: 0, // SPEC v2.1: Bootstrap validator has 0 stake
                is_bootstrap_validator: true, // Constitutional exception for block production
            }],
            tokenomics: TokenomicsState::genesis(),
        }
    }

    /// Create genesis with a custom validator
    /// SECURITY FIX: Updated default stake to meet MIN_VALIDATOR_STAKE (50,000 KRAT)
    pub fn with_validator(validator_account: AccountId) -> Self {
        let mut balances = HashMap::new();

        // Give funds to the custom validator
        balances.insert(validator_account, 1_000_000 * KRAT);

        Self {
            timestamp: GENESIS_TIMESTAMP,
            balances,
            validators: vec![GenesisValidator {
                account: validator_account,
                stake: 50_000 * KRAT,  // SECURITY FIX: Min 50k per SPEC 1 §8.1
                is_bootstrap_validator: false,
            }],
            tokenomics: TokenomicsState::genesis(),
        }
    }

    /// Charge depuis un fichier JSON
    pub fn from_file(path: &str) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Sauvegarde vers un fichier JSON
    pub fn to_file(&self, path: &str) -> Result<(), std::io::Error> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, content)
    }
}

impl Default for GenesisSpec {
    fn default() -> Self {
        Self::mainnet()
    }
}

/// Builder pour le bloc genesis
pub struct GenesisBuilder {
    spec: GenesisSpec,
}

impl GenesisBuilder {
    pub fn new(spec: GenesisSpec) -> Self {
        Self { spec }
    }

    /// Construit le bloc genesis et initialise l'état
    /// Returns (Block, ValidatorSet)
    pub fn build(self, state: &mut StateBackend) -> Result<(Block, ValidatorSet), String> {
        // Initialise les balances
        for (account, balance) in &self.spec.balances {
            let mut account_info = AccountInfo::new();
            account_info.free = *balance;
            state.set_account(*account, account_info)
                .map_err(|e| format!("Erreur set_account: {:?}", e))?;
        }

        // Initialise les validateurs
        let mut validator_set = ValidatorSet::new();

        for validator in &self.spec.validators {
            // Bootstrap validator (SPEC v2.1): 0 stake, no initial balance
            if validator.is_bootstrap_validator {
                // Create account if it doesn't exist (with 0 balance)
                let account_info = state
                    .get_account(&validator.account)
                    .map_err(|e| format!("Erreur get_account: {:?}", e))?
                    .unwrap_or_else(AccountInfo::new);

                state.set_account(validator.account, account_info)
                    .map_err(|e| format!("Erreur set_account: {:?}", e))?;

                // Add bootstrap validator to set
                let validator_info = ValidatorInfo::new_bootstrap(validator.account, 0);
                validator_set.add_validator(validator_info)
                    .map_err(|e| format!("Erreur add_validator: {:?}", e))?;
            } else {
                // Regular validator: reserve stake from balance
                let mut account_info = state
                    .get_account(&validator.account)
                    .map_err(|e| format!("Erreur get_account: {:?}", e))?
                    .ok_or_else(|| format!("Compte validateur {:?} non trouvé", validator.account))?;

                account_info
                    .reserve(validator.stake)
                    .map_err(|e| format!("Impossible de réserver le stake: {:?}", e))?;

                state.set_account(validator.account, account_info)
                    .map_err(|e| format!("Erreur set_account: {:?}", e))?;

                // Add to validator set
                let validator_info = ValidatorInfo::new(validator.account, validator.stake, 0);
                validator_set.add_validator(validator_info)
                    .map_err(|e| format!("Erreur add_validator: {:?}", e))?;
            }
        }

        // Crée le bloc genesis
        // TODO: ChainId should be configured, not hardcoded
        let chain_id = ChainId(0);
        let state_root_computed = state.compute_state_root(0, chain_id);

        // Store the genesis state root
        state.store_state_root(0, state_root_computed)
            .map_err(|e| format!("Erreur store_state_root: {:?}", e))?;

        // SECURITY FIX #35: Initialize drift tracker with genesis timestamp
        // This tracker is inherited by forks (uses genesis_timestamp as absolute reference)
        state.init_drift_tracker(self.spec.timestamp)
            .map_err(|e| format!("Erreur init_drift_tracker: {:?}", e))?;

        let header = BlockHeader {
            number: 0,
            parent_hash: Hash::ZERO,
            transactions_root: Hash::ZERO,
            state_root: state_root_computed.root,
            timestamp: self.spec.timestamp,
            epoch: 0,
            slot: 0,
            author: AccountId::from_bytes([0; 32]), // Pas d'auteur pour genesis
            signature: Signature64([0; 64]),        // Pas de signature pour genesis
        };

        let block = Block {
            header,
            body: BlockBody {
                transactions: vec![],
            },
        };

        Ok((block, validator_set))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::Database;
    use tempfile::tempdir;

    #[test]
    fn test_mainnet_genesis() {
        // Unified KratOs genesis with bootstrap validator
        let spec = GenesisSpec::mainnet();
        assert_eq!(spec.validators.len(), 1);
        assert_eq!(spec.balances.len(), 0); // Bootstrap validator has no initial balance

        // Verify bootstrap validator configuration
        let bootstrap = &spec.validators[0];
        assert!(bootstrap.is_bootstrap_validator);
        assert_eq!(bootstrap.stake, 0);
    }

    #[test]
    fn test_with_validator_genesis() {
        // Genesis with a custom validator
        // SECURITY FIX: Updated to use new MIN_VALIDATOR_STAKE (50,000 KRAT)
        let validator = AccountId::from_bytes([1u8; 32]);
        let spec = GenesisSpec::with_validator(validator);
        assert_eq!(spec.validators.len(), 1);
        assert_eq!(spec.balances.len(), 1);
        assert_eq!(spec.validators[0].stake, 50_000 * KRAT);
        assert!(!spec.validators[0].is_bootstrap_validator);
    }

    #[test]
    fn test_genesis_builder_with_custom_validator() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().to_str().unwrap()).unwrap();
        let mut state = StateBackend::new(db);

        let alice = AccountId::from_bytes([1u8; 32]);
        let spec = GenesisSpec::with_validator(alice);
        let builder = GenesisBuilder::new(spec.clone());

        let (block, validator_set) = builder.build(&mut state).unwrap();

        assert_eq!(block.header.number, 0);
        assert_eq!(block.header.parent_hash, Hash::ZERO);

        // Verify Alice account balance (1M - 50k staked)
        // SECURITY FIX: Updated to use new MIN_VALIDATOR_STAKE (50,000 KRAT)
        let account = state.get_account(&alice).unwrap().unwrap();
        assert_eq!(account.free, 950_000 * KRAT);

        // Verify validator is in the set
        assert_eq!(validator_set.active_count(), 1);
        assert!(validator_set.is_active(&alice));
    }

    #[test]
    fn test_genesis_validators_stake() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().to_str().unwrap()).unwrap();
        let mut state = StateBackend::new(db);

        let alice = AccountId::from_bytes([1u8; 32]);
        let spec = GenesisSpec::with_validator(alice);
        // SECURITY FIX: Updated to use new MIN_VALIDATOR_STAKE (50,000 KRAT)
        let expected_stake = 50_000 * KRAT;

        let builder = GenesisBuilder::new(spec.clone());
        let (_, validator_set) = builder.build(&mut state).unwrap();

        // Verify stake is reserved
        let account = state.get_account(&alice).unwrap().unwrap();
        assert_eq!(account.reserved, expected_stake);
        assert_eq!(account.free, 1_000_000 * KRAT - expected_stake);

        // Verify validator is registered
        assert!(validator_set.is_active(&alice));
        let validator = validator_set.get_validator(&alice).unwrap();
        assert_eq!(validator.stake, expected_stake);
    }

    #[test]
    fn test_bootstrap_validator_genesis() {
        // Bootstrap validator has 0 stake and produces blocks
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().to_str().unwrap()).unwrap();
        let mut state = StateBackend::new(db);

        let spec = GenesisSpec::mainnet();
        let builder = GenesisBuilder::new(spec.clone());

        let (block, validator_set) = builder.build(&mut state).unwrap();

        assert_eq!(block.header.number, 0);

        // Bootstrap validator account exists with 0 balance
        let bootstrap_account = AccountId::from_bytes([
            0x9a, 0x0c, 0x70, 0x3c, 0x57, 0x2e, 0x8d, 0x17,
            0x0c, 0x32, 0xad, 0x0d, 0xb9, 0xc9, 0x53, 0xd2,
            0x56, 0x5e, 0xfc, 0x3e, 0xd5, 0x85, 0xed, 0x57,
            0xda, 0x96, 0x37, 0x65, 0x5a, 0xbf, 0x78, 0xb8,
        ]);
        let account = state.get_account(&bootstrap_account).unwrap().unwrap();
        assert_eq!(account.free, 0);
        assert_eq!(account.reserved, 0);

        // Bootstrap validator is active despite 0 stake (at block 0 during bootstrap era)
        assert_eq!(validator_set.active_count_at(0), 1);
        assert!(validator_set.is_active_at(&bootstrap_account, 0));

        // Verify bootstrap flag
        let validator = validator_set.get_validator(&bootstrap_account).unwrap();
        assert!(validator.is_bootstrap_validator);
        assert_eq!(validator.stake, 0);
    }
}
