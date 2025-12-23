// GRANDPA-style Finality Gadget for KratOs
//
// This module implements a Byzantine Fault Tolerant finality gadget inspired by GRANDPA
// (GHOST-based Recursive ANcestor Deriving Prefix Agreement).
//
// Key properties:
// - Safety: Finalized blocks cannot be reverted (with 2/3 honest validators)
// - Liveness: All blocks eventually get finalized
// - Determinism: Same justification produces same finality on all nodes
//
// The finality process works in rounds:
// 1. Prevote: Validators broadcast their preferred block to finalize
// 2. Precommit: After seeing 2/3 prevotes for a block, validators precommit
// 3. Finalization: When 2/3 precommits are collected, block is finalized

pub mod types;
pub mod votes;
pub mod rounds;
pub mod gadget;

pub use types::*;
pub use votes::VoteCollector;
pub use rounds::FinalityRound;
pub use gadget::FinalityGadget;

use crate::types::primitives::BlockNumber;

/// Finality configuration constants
pub mod config {
    /// Minimum number of validators required for finality voting
    pub const MIN_VALIDATORS_FOR_FINALITY: usize = 3;

    /// Supermajority threshold (2/3 = 66%)
    pub const SUPERMAJORITY_THRESHOLD: u8 = 66;

    /// Maximum rounds before forcing finality attempt
    pub const MAX_ROUNDS_BEFORE_FORCE: u32 = 10;

    /// Timeout for a single round (in milliseconds)
    pub const ROUND_TIMEOUT_MS: u64 = 6000; // 6 seconds (1 block time)

    /// Maximum pending votes to keep in memory
    pub const MAX_PENDING_VOTES: usize = 1000;
}

/// Calculate if count reaches supermajority of total
#[inline]
pub fn has_supermajority(count: usize, total: usize) -> bool {
    if total == 0 {
        return false;
    }
    count * 100 >= total * config::SUPERMAJORITY_THRESHOLD as usize
}

/// Calculate minimum votes needed for supermajority
#[inline]
pub fn supermajority_threshold(total: usize) -> usize {
    // ceil(total * 2/3) = (total * 2 + 2) / 3
    (total * 2 + 2) / 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supermajority_calculation() {
        // 3 validators: need 2
        assert!(has_supermajority(2, 3));
        assert!(!has_supermajority(1, 3));

        // 4 validators: need 3 (66% of 4 = 2.64, so need at least 3)
        assert!(has_supermajority(3, 4));
        assert!(!has_supermajority(2, 4));

        // 10 validators: need 7 (66% of 10 = 6.6, so need at least 7)
        assert!(has_supermajority(7, 10));
        assert!(!has_supermajority(6, 10));

        // 100 validators: need 66
        assert!(has_supermajority(66, 100));
        assert!(!has_supermajority(65, 100));
    }

    #[test]
    fn test_supermajority_threshold() {
        assert_eq!(supermajority_threshold(3), 2);
        assert_eq!(supermajority_threshold(4), 3);
        assert_eq!(supermajority_threshold(10), 7);
        assert_eq!(supermajority_threshold(100), 67);
    }

    #[test]
    fn test_zero_validators() {
        assert!(!has_supermajority(0, 0));
        assert!(!has_supermajority(1, 0));
    }
}
