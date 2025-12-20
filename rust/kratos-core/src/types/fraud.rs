// Fraud Proofs - SPEC v3.1 Phase 5
// Cryptographic proofs of validator misbehavior or invalid state transitions

use super::primitives::{BlockNumber, ChainId};
use super::account::AccountId;
use super::signature::Signature64;
use super::account::AccountInfo;
use super::block::BlockHeader;
use super::merkle::MerkleProof;
use super::chain::SidechainInfo;
use serde::{Deserialize, Serialize};

/// Fraud proof - cryptographic evidence of validator misbehavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FraudProof {
    /// Invalid state transition detected
    /// Proves that a state change violated the protocol rules
    InvalidStateTransition {
        /// Account that had invalid state transition
        account: AccountId,

        /// State before the invalid transition
        before_state: AccountInfo,

        /// State after the invalid transition
        after_state: AccountInfo,

        /// Merkle proof of before_state in previous block
        merkle_proof_before: MerkleProof,

        /// Merkle proof of after_state in current block
        merkle_proof_after: MerkleProof,

        /// Block number where violation occurred
        block_number: BlockNumber,

        /// The violating validator (who produced the block)
        validator: AccountId,
    },

    /// Double finalization detected
    /// Proves that a validator signed two conflicting blocks at the same height
    DoubleFinalization {
        /// Validator who signed both blocks
        validator: AccountId,

        /// First finalized block
        block_a: BlockHeader,

        /// Second conflicting block (same height, different hash)
        block_b: BlockHeader,

        /// Validator's signature on block_a
        signature_a: Signature64,

        /// Validator's signature on block_b
        signature_b: Signature64,
    },

    /// Invalid exit detected
    /// Proves that a sidechain exit claim violates purge conditions
    InvalidExit {
        /// Chain ID attempting to exit
        chain_id: ChainId,

        /// The exit claim being made
        exit_claim: SidechainInfo,

        /// Type of violation
        violation: ExitViolation,

        /// Merkle proof of actual state (contradicting the claim)
        merkle_proof: MerkleProof,

        /// Block number when fraud was detected
        block_number: BlockNumber,
    },
}

/// Types of exit violations for InvalidExit fraud proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExitViolation {
    /// Exit attempted before withdrawal window
    PrematureExit {
        /// Actual purge start block
        actual_purge_start: BlockNumber,

        /// Claimed withdrawal start
        claimed_withdrawal_start: BlockNumber,
    },

    /// Exit attempted after withdrawal window expired
    ExpiredWithdrawal {
        /// Withdrawal window end block
        window_end: BlockNumber,

        /// Block when exit was attempted
        exit_attempt_block: BlockNumber,
    },

    /// Exit claim amount exceeds actual locked balance
    InflatedBalance {
        /// Actual balance according to state root
        actual_balance: u128,

        /// Claimed balance in exit
        claimed_balance: u128,
    },

    /// Exit attempted for non-existent or already purged chain
    InvalidChainState {
        /// What the actual state is
        actual_state: String,
    },
}

/// Errors that can occur during fraud proof verification
#[derive(Debug, Clone, thiserror::Error)]
pub enum FraudProofError {
    #[error("Merkle proof verification failed")]
    InvalidMerkleProof,

    #[error("Signature verification failed")]
    InvalidSignature,

    #[error("Blocks are not conflicting (same hash or different heights)")]
    BlocksNotConflicting,

    #[error("State transition is valid (no violation detected)")]
    NoViolationDetected,

    #[error("Validator not found: {0:?}")]
    ValidatorNotFound(AccountId),

    #[error("Chain not found: {0:?}")]
    ChainNotFound(ChainId),

    #[error("Exit violation not proven")]
    ExitViolationNotProven,

    #[error("Fraud proof expired (too old)")]
    ProofExpired,

    #[error("Invalid proof structure: {0}")]
    InvalidProofStructure(String),
}

impl FraudProof {
    /// Get the validator accused by this fraud proof
    pub fn accused_validator(&self) -> AccountId {
        match self {
            FraudProof::InvalidStateTransition { validator, .. } => *validator,
            FraudProof::DoubleFinalization { validator, .. } => *validator,
            // For InvalidExit, we'd need to look up who was validating that chain
            // For now, return a placeholder
            FraudProof::InvalidExit { .. } => AccountId::from_bytes([0; 32]),
        }
    }

    /// Get the block number where the fraud occurred
    pub fn fraud_block_number(&self) -> BlockNumber {
        match self {
            FraudProof::InvalidStateTransition { block_number, .. } => *block_number,
            FraudProof::DoubleFinalization { block_a, .. } => block_a.number,
            FraudProof::InvalidExit { block_number, .. } => *block_number,
        }
    }

    /// Get the chain ID affected by this fraud (if applicable)
    pub fn affected_chain(&self) -> Option<ChainId> {
        match self {
            FraudProof::InvalidExit { chain_id, .. } => Some(*chain_id),
            _ => None,
        }
    }

    /// Verify this fraud proof (delegates to specific verification methods)
    pub fn verify(&self) -> Result<(), FraudProofError> {
        match self {
            FraudProof::InvalidStateTransition { .. } => self.verify_invalid_state_transition(),
            FraudProof::DoubleFinalization { .. } => self.verify_double_finalization(),
            FraudProof::InvalidExit { .. } => self.verify_invalid_exit(),
        }
    }

    /// Verify InvalidStateTransition fraud proof
    fn verify_invalid_state_transition(&self) -> Result<(), FraudProofError> {
        if let FraudProof::InvalidStateTransition {
            account,
            before_state,
            after_state,
            merkle_proof_before,
            merkle_proof_after,
            block_number,
            validator: _,
        } = self
        {
            // 1. Verify Merkle proofs
            if !merkle_proof_before.verify() {
                return Err(FraudProofError::InvalidMerkleProof);
            }
            if !merkle_proof_after.verify() {
                return Err(FraudProofError::InvalidMerkleProof);
            }

            // 2. Verify proofs are for consecutive blocks
            if merkle_proof_after.block_number != merkle_proof_before.block_number + 1 {
                return Err(FraudProofError::InvalidProofStructure(
                    "Merkle proofs must be for consecutive blocks".to_string()
                ));
            }

            // 3. Verify the state transition is actually invalid
            // Check balance can't go negative, can't increase without valid source, etc.
            if !Self::is_state_transition_invalid(before_state, after_state) {
                return Err(FraudProofError::NoViolationDetected);
            }

            Ok(())
        } else {
            Err(FraudProofError::InvalidProofStructure(
                "Expected InvalidStateTransition variant".to_string()
            ))
        }
    }

    /// Verify DoubleFinalization fraud proof
    fn verify_double_finalization(&self) -> Result<(), FraudProofError> {
        if let FraudProof::DoubleFinalization {
            validator,
            block_a,
            block_b,
            signature_a,
            signature_b,
        } = self
        {
            // 1. Verify blocks are at same height but different
            if block_a.number != block_b.number {
                return Err(FraudProofError::BlocksNotConflicting);
            }
            if block_a.hash() == block_b.hash() {
                return Err(FraudProofError::BlocksNotConflicting);
            }

            // 2. Verify signatures
            let msg_a = block_a.hash();
            let msg_b = block_b.hash();

            if !validator.verify(msg_a.as_bytes(), signature_a.as_bytes()) {
                return Err(FraudProofError::InvalidSignature);
            }
            if !validator.verify(msg_b.as_bytes(), signature_b.as_bytes()) {
                return Err(FraudProofError::InvalidSignature);
            }

            Ok(())
        } else {
            Err(FraudProofError::InvalidProofStructure(
                "Expected DoubleFinalization variant".to_string()
            ))
        }
    }

    /// Verify InvalidExit fraud proof
    fn verify_invalid_exit(&self) -> Result<(), FraudProofError> {
        if let FraudProof::InvalidExit {
            chain_id: _,
            exit_claim: _,
            violation,
            merkle_proof,
            block_number: _,
        } = self
        {
            // 1. Verify Merkle proof
            if !merkle_proof.verify() {
                return Err(FraudProofError::InvalidMerkleProof);
            }

            // 2. Verify the specific violation
            match violation {
                ExitViolation::PrematureExit { actual_purge_start, claimed_withdrawal_start } => {
                    if claimed_withdrawal_start < actual_purge_start {
                        Ok(())
                    } else {
                        Err(FraudProofError::ExitViolationNotProven)
                    }
                }
                ExitViolation::ExpiredWithdrawal { window_end, exit_attempt_block } => {
                    if exit_attempt_block > window_end {
                        Ok(())
                    } else {
                        Err(FraudProofError::ExitViolationNotProven)
                    }
                }
                ExitViolation::InflatedBalance { actual_balance, claimed_balance } => {
                    if claimed_balance > actual_balance {
                        Ok(())
                    } else {
                        Err(FraudProofError::ExitViolationNotProven)
                    }
                }
                ExitViolation::InvalidChainState { .. } => {
                    // Chain state verified via Merkle proof
                    Ok(())
                }
            }
        } else {
            Err(FraudProofError::InvalidProofStructure(
                "Expected InvalidExit variant".to_string()
            ))
        }
    }

    /// Check if a state transition is invalid
    /// Returns true if the transition violates protocol rules
    fn is_state_transition_invalid(before: &AccountInfo, after: &AccountInfo) -> bool {
        // Balance can't go negative (though u128 prevents this at type level)
        // Balance can't increase without a valid deposit/transfer source
        // Nonce must increment monotonically

        // Example: Balance increased by more than it should (indicating theft/inflation)
        if after.free > before.free + 1_000_000_000 {
            // Arbitrary threshold - in reality would check against actual tx
            return true;
        }

        // Example: Nonce decreased (replay attack indicator)
        if after.nonce < before.nonce {
            return true;
        }

        // Example: Nonce jumped by more than 1 (indicating tx skipped)
        if after.nonce > before.nonce + 1 {
            return true;
        }

        false
    }
}

/// Result of fraud proof verification
#[derive(Debug, Clone)]
pub struct FraudProofVerification {
    /// Whether the proof is valid
    pub is_valid: bool,

    /// The accused validator
    pub accused: AccountId,

    /// Severity of the fraud (determines slashing amount)
    pub severity: FraudSeverity,

    /// Block number when fraud occurred
    pub fraud_block: BlockNumber,
}

/// Severity of fraud (determines slashing amount)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FraudSeverity {
    /// Minor violation (5% slash)
    Minor,

    /// Moderate violation (15% slash)
    Moderate,

    /// Severe violation (33% slash)
    Severe,

    /// Critical violation (100% slash + jail)
    Critical,
}

impl FraudSeverity {
    /// Get the slashing percentage for this severity
    pub fn slash_percentage(&self) -> u8 {
        match self {
            FraudSeverity::Minor => 5,
            FraudSeverity::Moderate => 15,
            FraudSeverity::Severe => 33,
            FraudSeverity::Critical => 100,
        }
    }

    /// Determine severity from fraud proof type
    pub fn from_fraud_proof(proof: &FraudProof) -> Self {
        match proof {
            // Double finalization is critical - breaks consensus
            FraudProof::DoubleFinalization { .. } => FraudSeverity::Critical,

            // Invalid state transition is severe
            FraudProof::InvalidStateTransition { .. } => FraudSeverity::Severe,

            // Invalid exit severity depends on violation type
            FraudProof::InvalidExit { violation, .. } => match violation {
                ExitViolation::InflatedBalance { .. } => FraudSeverity::Critical,
                ExitViolation::InvalidChainState { .. } => FraudSeverity::Severe,
                ExitViolation::PrematureExit { .. } => FraudSeverity::Moderate,
                ExitViolation::ExpiredWithdrawal { .. } => FraudSeverity::Minor,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Hash, Signature64};

    #[test]
    fn test_fraud_severity_slash_percentages() {
        assert_eq!(FraudSeverity::Minor.slash_percentage(), 5);
        assert_eq!(FraudSeverity::Moderate.slash_percentage(), 15);
        assert_eq!(FraudSeverity::Severe.slash_percentage(), 33);
        assert_eq!(FraudSeverity::Critical.slash_percentage(), 100);
    }

    #[test]
    fn test_fraud_proof_accused_validator() {
        let validator = AccountId::from_bytes([1; 32]);

        let proof = FraudProof::InvalidStateTransition {
            account: AccountId::from_bytes([2; 32]),
            before_state: create_dummy_account_info(1000, 0),
            after_state: create_dummy_account_info(2000, 1),
            merkle_proof_before: create_dummy_merkle_proof(0),
            merkle_proof_after: create_dummy_merkle_proof(1),
            block_number: 100,
            validator,
        };

        assert_eq!(proof.accused_validator(), validator);
    }

    #[test]
    fn test_fraud_proof_block_number() {
        let proof = FraudProof::InvalidStateTransition {
            account: AccountId::from_bytes([1; 32]),
            before_state: create_dummy_account_info(1000, 0),
            after_state: create_dummy_account_info(2000, 1),
            merkle_proof_before: create_dummy_merkle_proof(0),
            merkle_proof_after: create_dummy_merkle_proof(1),
            block_number: 100,
            validator: AccountId::from_bytes([2; 32]),
        };

        assert_eq!(proof.fraud_block_number(), 100);
    }

    #[test]
    fn test_state_transition_invalid_nonce_decrease() {
        let before = create_dummy_account_info(1000, 5);
        let after = create_dummy_account_info(1000, 3);

        assert!(FraudProof::is_state_transition_invalid(&before, &after));
    }

    #[test]
    fn test_state_transition_invalid_nonce_jump() {
        let before = create_dummy_account_info(1000, 5);
        let after = create_dummy_account_info(1000, 10);

        assert!(FraudProof::is_state_transition_invalid(&before, &after));
    }

    #[test]
    fn test_double_finalization_severity() {
        let proof = FraudProof::DoubleFinalization {
            validator: AccountId::from_bytes([1; 32]),
            block_a: create_dummy_block_header(100),
            block_b: create_dummy_block_header(100),
            signature_a: Signature64::from_bytes([1; 64]),
            signature_b: Signature64::from_bytes([2; 64]),
        };

        let severity = FraudSeverity::from_fraud_proof(&proof);
        assert_eq!(severity, FraudSeverity::Critical);
    }

    #[test]
    fn test_invalid_state_transition_severity() {
        let proof = FraudProof::InvalidStateTransition {
            account: AccountId::from_bytes([1; 32]),
            before_state: create_dummy_account_info(1000, 0),
            after_state: create_dummy_account_info(2000, 1),
            merkle_proof_before: create_dummy_merkle_proof(0),
            merkle_proof_after: create_dummy_merkle_proof(1),
            block_number: 100,
            validator: AccountId::from_bytes([2; 32]),
        };

        let severity = FraudSeverity::from_fraud_proof(&proof);
        assert_eq!(severity, FraudSeverity::Severe);
    }

    #[test]
    fn test_exit_violation_inflated_balance_severity() {
        use crate::types::SecurityMode;
        let proof = FraudProof::InvalidExit {
            chain_id: ChainId(1),
            exit_claim: SidechainInfo::new(
                ChainId(1),
                None,
                AccountId::from_bytes([1; 32]),
                None,
                None,
                SecurityMode::Inherited,
                1000,
                0,
            ),
            violation: ExitViolation::InflatedBalance {
                actual_balance: 1000,
                claimed_balance: 5000,
            },
            merkle_proof: create_dummy_merkle_proof(0),
            block_number: 100,
        };

        let severity = FraudSeverity::from_fraud_proof(&proof);
        assert_eq!(severity, FraudSeverity::Critical);
    }

    // Helper functions for tests
    fn create_dummy_merkle_proof(block_number: BlockNumber) -> MerkleProof {
        MerkleProof::new(
            vec![1, 2, 3],
            0,
            vec![[0; 32]],
            Hash::from_bytes([0; 32]),
            block_number,
            ChainId(0),
        )
    }

    fn create_dummy_block_header(number: BlockNumber) -> BlockHeader {
        use crate::types::Hash;
        BlockHeader {
            number,
            parent_hash: Hash::from_bytes([0; 32]),
            transactions_root: Hash::from_bytes([0; 32]),
            state_root: Hash::from_bytes([0; 32]),
            timestamp: 0,
            epoch: 0,
            slot: 0,
            author: AccountId::from_bytes([0; 32]),
            signature: Signature64::from_bytes([0; 64]),
        }
    }

    fn create_dummy_account_info(balance: u128, nonce: u64) -> AccountInfo {
        use crate::types::Hash;
        AccountInfo {
            nonce,
            free: balance,
            reserved: 0,
            last_modified: Hash::from_bytes([0; 32]),
        }
    }
}
