// Epoch - Périodes de consensus (1 heure)
use crate::types::{BlockNumber, EpochNumber, SlotNumber};
use serde::{Deserialize, Serialize};

/// Durée d'une epoch en blocs
/// 1 heure = 3600 / 6 = 600 blocs (à 6 sec/bloc)
pub const EPOCH_DURATION_BLOCKS: BlockNumber = 600;

/// Durée d'un slot en secondes
pub const SLOT_DURATION_SECS: u64 = 6;

/// Configuration d'une epoch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpochConfig {
    /// Numéro de l'epoch
    pub number: EpochNumber,

    /// Bloc de début
    pub start_block: BlockNumber,

    /// Bloc de fin
    pub end_block: BlockNumber,

    /// Nombre total de slots
    pub total_slots: SlotNumber,
}

impl EpochConfig {
    pub fn new(epoch_number: EpochNumber) -> Self {
        let start_block = epoch_number * EPOCH_DURATION_BLOCKS;
        let end_block = start_block + EPOCH_DURATION_BLOCKS;

        Self {
            number: epoch_number,
            start_block,
            end_block,
            total_slots: EPOCH_DURATION_BLOCKS,
        }
    }

    /// Calcule l'epoch à partir d'un numéro de bloc
    pub fn from_block_number(block: BlockNumber) -> EpochNumber {
        block / EPOCH_DURATION_BLOCKS
    }

    /// Calcule le slot dans l'epoch à partir d'un numéro de bloc
    pub fn slot_from_block(block: BlockNumber) -> SlotNumber {
        block % EPOCH_DURATION_BLOCKS
    }

    /// Vérifie si un bloc appartient à cette epoch
    pub fn contains_block(&self, block: BlockNumber) -> bool {
        block >= self.start_block && block < self.end_block
    }

    /// Prochaine epoch
    pub fn next(&self) -> Self {
        Self::new(self.number + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_calculation() {
        // 1 epoch = 600 blocs = 1 heure
        assert_eq!(EpochConfig::from_block_number(0), 0);
        assert_eq!(EpochConfig::from_block_number(599), 0);
        assert_eq!(EpochConfig::from_block_number(600), 1);
        assert_eq!(EpochConfig::from_block_number(1200), 2);
    }

    #[test]
    fn test_slot_calculation() {
        assert_eq!(EpochConfig::slot_from_block(0), 0);
        assert_eq!(EpochConfig::slot_from_block(100), 100);
        assert_eq!(EpochConfig::slot_from_block(600), 0); // Première slot de epoch 1
        assert_eq!(EpochConfig::slot_from_block(700), 100);
    }

    #[test]
    fn test_epoch_contains() {
        let epoch0 = EpochConfig::new(0);
        assert!(epoch0.contains_block(0));
        assert!(epoch0.contains_block(300));
        assert!(epoch0.contains_block(599));
        assert!(!epoch0.contains_block(600));
    }
}
