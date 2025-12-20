// PoS - Proof of Stake lent et résilient
// Principe: Sélection pseudo-aléatoire pondérée par le stake

use super::epoch::EpochConfig;
use super::validator::{ValidatorInfo, ValidatorSet};
use crate::types::{AccountId, BlockNumber, EpochNumber, Hash, SlotNumber};
use std::collections::HashMap;

/// Sélecteur de validateur pour un slot donné
pub struct ValidatorSelector {
    /// Set de validateurs actifs
    validator_set: ValidatorSet,

    /// Configuration de l'epoch courante
    epoch_config: EpochConfig,
}

impl ValidatorSelector {
    pub fn new(validator_set: ValidatorSet, epoch_config: EpochConfig) -> Self {
        Self {
            validator_set,
            epoch_config,
        }
    }

    /// Sélectionne le validateur pour un slot donné
    /// Utilise une sélection pseudo-aléatoire pondérée par le stake
    ///
    /// FIX: Returns Result instead of Option to distinguish error cases
    pub fn select_validator_for_slot(
        &self,
        slot: SlotNumber,
        randomness: &Hash,
    ) -> Result<AccountId, ValidatorSelectionError> {
        let active_validators = self.validator_set.active_validators();

        if active_validators.is_empty() {
            return Err(ValidatorSelectionError::NoValidatorsRegistered);
        }

        // Mélange le randomness avec le numéro de slot
        let seed = self.mix_randomness(randomness, slot);

        // Sélection pondérée par le stake
        self.weighted_selection(&active_validators, &seed)
            .ok_or(ValidatorSelectionError::ZeroTotalStake)
    }

    /// Legacy method for backward compatibility - returns Option
    #[deprecated(since = "0.2.0", note = "Use select_validator_for_slot which returns Result for better error handling")]
    pub fn select_validator_for_slot_opt(
        &self,
        slot: SlotNumber,
        randomness: &Hash,
    ) -> Option<AccountId> {
        self.select_validator_for_slot(slot, randomness).ok()
    }

    /// Mélange le randomness avec le slot number
    fn mix_randomness(&self, randomness: &Hash, slot: SlotNumber) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(randomness.as_bytes());
        data.extend_from_slice(&slot.to_le_bytes());
        Hash::hash(&data)
    }

    /// Sélection pondérée par le stake
    fn weighted_selection(&self, validators: &[&ValidatorInfo], seed: &Hash) -> Option<AccountId> {
        if validators.is_empty() {
            return None;
        }

        // Calcule le stake total
        let total_stake: u128 = validators.iter().map(|v| v.stake).sum();

        if total_stake == 0 {
            return None;
        }

        // Convertit le seed en un nombre entre 0 et total_stake
        let mut seed_bytes = [0u8; 16];
        seed_bytes.copy_from_slice(&seed.as_bytes()[0..16]);
        let random_point = u128::from_le_bytes(seed_bytes) % total_stake;

        // Trouve le validateur correspondant au point aléatoire
        let mut cumulative = 0u128;
        for validator in validators {
            cumulative += validator.stake;
            if cumulative > random_point {
                return Some(validator.id);
            }
        }

        // Fallback (ne devrait jamais arriver)
        validators.last().map(|v| v.id)
    }

    /// Vérifie si un validateur est autorisé à produire un bloc pour un slot donné
    pub fn can_produce_block(
        &self,
        validator: &AccountId,
        slot: SlotNumber,
        randomness: &Hash,
    ) -> bool {
        match self.select_validator_for_slot(slot, randomness) {
            Ok(selected) => &selected == validator,
            Err(_) => false,
        }
    }
}

/// FIX: Error type for validator selection to distinguish failure modes
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidatorSelectionError {
    #[error("No validators registered in the network")]
    NoValidatorsRegistered,

    #[error("All validators have zero stake")]
    ZeroTotalStake,
}

/// Générateur de randomness pour la sélection de validateurs
/// Utilise le hash du bloc précédent comme source de randomness
pub struct RandomnessProvider {
    /// Historique des hashes de blocs
    block_hashes: HashMap<BlockNumber, Hash>,
}

impl RandomnessProvider {
    pub fn new() -> Self {
        Self {
            block_hashes: HashMap::new(),
        }
    }

    /// Enregistre le hash d'un nouveau bloc
    pub fn record_block_hash(&mut self, block_number: BlockNumber, block_hash: Hash) {
        self.block_hashes.insert(block_number, block_hash);

        // Garde seulement les 1000 derniers blocs (économie mémoire)
        if self.block_hashes.len() > 1000 {
            if let Some(oldest) = block_number.checked_sub(1000) {
                self.block_hashes.remove(&oldest);
            }
        }
    }

    /// Récupère le randomness pour une epoch
    /// Utilise le hash du premier bloc de l'epoch précédente
    pub fn get_epoch_randomness(&self, epoch: EpochNumber) -> Hash {
        if epoch == 0 {
            // Genesis epoch utilise un seed fixe
            return Hash::ZERO;
        }

        let prev_epoch_start = (epoch - 1) * super::epoch::EPOCH_DURATION_BLOCKS;

        self.block_hashes
            .get(&prev_epoch_start)
            .copied()
            .unwrap_or(Hash::ZERO)
    }
}

impl Default for RandomnessProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Balance;

    #[test]
    fn test_validator_selection() {
        let mut set = ValidatorSet::new();

        // Crée des validateurs avec différents stakes
        // SECURITY FIX: Updated to meet new MIN_VALIDATOR_STAKE (50,000 KRAT)
        let id1 = AccountId::from_bytes([1; 32]);
        let id2 = AccountId::from_bytes([2; 32]);
        let id3 = AccountId::from_bytes([3; 32]);

        let v1 = ValidatorInfo::new(id1, 50_000 * crate::types::primitives::KRAT, 0);
        let v2 = ValidatorInfo::new(id2, 100_000 * crate::types::primitives::KRAT, 0);
        let v3 = ValidatorInfo::new(id3, 150_000 * crate::types::primitives::KRAT, 0);

        set.add_validator(v1).unwrap();
        set.add_validator(v2).unwrap();
        set.add_validator(v3).unwrap();

        let epoch_config = EpochConfig::new(0);
        let selector = ValidatorSelector::new(set, epoch_config);

        let randomness = Hash::hash(b"test_randomness");

        // Sélectionne un validateur pour le slot 0
        // FIX: Updated test to use Result instead of Option
        let selected = selector.select_validator_for_slot(0, &randomness);
        assert!(selected.is_ok());

        // Vérifie que le validateur sélectionné peut produire un bloc
        let selected_id = selected.unwrap();
        assert!(selector.can_produce_block(&selected_id, 0, &randomness));

        // Vérifie qu'un autre validateur ne peut pas
        let other_id = if selected_id == id1 { id2 } else { id1 };
        assert!(!selector.can_produce_block(&other_id, 0, &randomness));
    }

    #[test]
    fn test_randomness_provider() {
        let mut provider = RandomnessProvider::new();

        let hash1 = Hash::hash(b"block1");
        let hash2 = Hash::hash(b"block2");

        provider.record_block_hash(0, hash1);
        provider.record_block_hash(1, hash2);

        // Epoch 0 doit retourner ZERO
        assert_eq!(provider.get_epoch_randomness(0), Hash::ZERO);

        // Epoch 1 doit retourner le hash du bloc 0
        assert_eq!(provider.get_epoch_randomness(1), hash1);
    }
}
