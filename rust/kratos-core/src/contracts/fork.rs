// Fork Contract - SPEC v8: Long-Term Resilience, Forking & Protocol Survivability
// Principle: Forking is a first-class mechanism, not a failure
//
// This contract manages:
// - Fork declaration and voting
// - Fork preparation phase
// - State snapshots for continuity
// - Sidechain alignment
// - Fork execution
// - Ossification mode

use crate::types::{
    AccountId, Balance, BlockNumber, ChainId, Hash,
    ForkType, ForkDeclarant, ForkStatus, ForkDeclaration,
    ForkSnapshot, ValidatorSnapshot, IdentitySnapshot, ReputationSnapshot,
    ForkAlignment, SidechainAlignment, ForkContinuity, OssificationState,
    ReputationDomain, IdentityStatus,
    FORK_VALIDATOR_THRESHOLD_PERCENT, FORK_STAKE_THRESHOLD_PERCENT,
    FORK_SIDECHAIN_THRESHOLD, OSSIFICATION_APPROVAL_THRESHOLD,
    POST_FORK_DECAY_MULTIPLIER, MIN_FORK_PREPARATION, MAX_FORK_PREPARATION,
};
use std::collections::{HashMap, HashSet};

// =============================================================================
// CONSTANTS
// =============================================================================

/// Minimum deposit to propose a fork (prevents spam)
pub const FORK_PROPOSAL_DEPOSIT: Balance = 100_000;

/// Voting period for fork declaration (14 days)
pub const FORK_VOTING_PERIOD: BlockNumber = 14 * 14_400;

/// Grace period for sidechain alignment decisions (7 days before fork)
pub const SIDECHAIN_ALIGNMENT_DEADLINE: BlockNumber = 7 * 14_400;

/// Maximum concurrent fork proposals
pub const MAX_CONCURRENT_FORKS: usize = 3;

/// Minimum time between fork executions (30 days)
pub const FORK_COOLDOWN: BlockNumber = 30 * 14_400;

// =============================================================================
// FORK CONTRACT
// =============================================================================

/// Fork management contract implementing SPEC v8
pub struct ForkContract {
    /// Active fork proposals
    pub proposals: HashMap<Hash, ForkDeclaration>,

    /// Executed forks (for history)
    pub executed_forks: Vec<ForkDeclaration>,

    /// Fork snapshots
    pub snapshots: HashMap<Hash, ForkSnapshot>,

    /// Fork continuity data
    pub continuity: HashMap<Hash, ForkContinuity>,

    /// Sidechain alignments for active forks
    pub alignments: HashMap<(Hash, ChainId), SidechainAlignment>,

    /// Ossification state
    pub ossification: OssificationState,

    /// Validator voting power
    voting_power: HashMap<AccountId, u64>,

    /// Total voting power
    total_power: u64,

    /// Total stake in the system
    total_stake: Balance,

    /// Last fork execution block
    last_fork_executed: Option<BlockNumber>,

    /// Events emitted
    events: Vec<ForkEvent>,
}

/// Events emitted by the fork contract
#[derive(Debug, Clone)]
pub enum ForkEvent {
    /// Fork proposed
    ForkProposed {
        fork_id: Hash,
        name: String,
        fork_type: ForkType,
        proposed_at: BlockNumber,
    },

    /// Fork declared (threshold met)
    ForkDeclared {
        fork_id: Hash,
        declared_at: BlockNumber,
        preparation_ends: BlockNumber,
    },

    /// Validator support added
    ValidatorSupport {
        fork_id: Hash,
        validator: AccountId,
        power: u64,
    },

    /// Sidechain alignment declared
    SidechainAligned {
        fork_id: Hash,
        chain_id: ChainId,
        alignment: ForkAlignment,
    },

    /// Fork snapshot created
    SnapshotCreated {
        fork_id: Hash,
        block_number: BlockNumber,
        state_root: Hash,
    },

    /// Fork executed
    ForkExecuted {
        fork_id: Hash,
        executed_at: BlockNumber,
        new_chain_id: ChainId,
    },

    /// Fork cancelled
    ForkCancelled {
        fork_id: Hash,
        reason: String,
    },

    /// Ossification proposed
    OssificationProposed {
        proposed_at: BlockNumber,
    },

    /// Ossification vote cast
    OssificationVote {
        voter: AccountId,
        approve: bool,
    },

    /// Ossification activated
    OssificationActivated {
        activated_at: BlockNumber,
    },
}

// =============================================================================
// ERRORS
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForkError {
    /// Fork already exists
    ForkAlreadyExists(Hash),

    /// Fork not found
    ForkNotFound(Hash),

    /// Too many concurrent forks
    TooManyConcurrentForks,

    /// Fork already declared
    ForkAlreadyDeclared,

    /// Fork not in preparation
    NotInPreparation,

    /// Fork not ready
    ForkNotReady,

    /// Fork in cooldown
    ForkInCooldown { ends_at: BlockNumber },

    /// Insufficient support
    InsufficientSupport { have: u8, need: u8 },

    /// Already voted
    AlreadyVoted,

    /// No voting power
    NoVotingPower,

    /// Invalid fork type
    InvalidForkType(String),

    /// Preparation period not met
    PreparationNotMet,

    /// Sidechain not found
    SidechainNotFound(ChainId),

    /// Alignment deadline passed
    AlignmentDeadlinePassed,

    /// Snapshot already exists
    SnapshotAlreadyExists,

    /// Cannot ossify during emergency
    CannotOssifyDuringEmergency,

    /// Ossification not eligible
    OssificationNotEligible,

    /// Already ossified
    AlreadyOssified,

    /// Deposit too low
    InsufficientDeposit { have: Balance, need: Balance },
}

// =============================================================================
// IMPLEMENTATION
// =============================================================================

impl ForkContract {
    /// Create a new fork contract
    pub fn new() -> Self {
        Self {
            proposals: HashMap::new(),
            executed_forks: Vec::new(),
            snapshots: HashMap::new(),
            continuity: HashMap::new(),
            alignments: HashMap::new(),
            ossification: OssificationState::new(),
            voting_power: HashMap::new(),
            total_power: 0,
            total_stake: 0,
            last_fork_executed: None,
            events: Vec::new(),
        }
    }

    /// Set voting power for a validator
    pub fn set_voting_power(&mut self, validator: AccountId, power: u64) {
        if power > 0 {
            self.voting_power.insert(validator, power);
        } else {
            self.voting_power.remove(&validator);
        }
        self.total_power = self.voting_power.values().sum();
    }

    /// Set total stake
    pub fn set_total_stake(&mut self, stake: Balance) {
        self.total_stake = stake;
    }

    // =========================================================================
    // FORK PROPOSAL & DECLARATION
    // =========================================================================

    /// Propose a new fork
    pub fn propose_fork(
        &mut self,
        proposer: AccountId,
        name: String,
        fork_type: ForkType,
        description: String,
        deposit: Balance,
        current_block: BlockNumber,
    ) -> Result<Hash, ForkError> {
        // Check deposit
        if deposit < FORK_PROPOSAL_DEPOSIT {
            return Err(ForkError::InsufficientDeposit {
                have: deposit,
                need: FORK_PROPOSAL_DEPOSIT,
            });
        }

        // Check max concurrent forks
        let active_count = self.proposals.values()
            .filter(|f| matches!(f.status, ForkStatus::Proposed | ForkStatus::Declared | ForkStatus::Ready))
            .count();

        if active_count >= MAX_CONCURRENT_FORKS {
            return Err(ForkError::TooManyConcurrentForks);
        }

        // Check cooldown
        if let Some(last) = self.last_fork_executed {
            if current_block < last + FORK_COOLDOWN {
                return Err(ForkError::ForkInCooldown {
                    ends_at: last + FORK_COOLDOWN,
                });
            }
        }

        // Create fork declaration
        let declarant = ForkDeclarant::Validators {
            count: 1,
            total_validators: self.voting_power.len() as u32,
            voting_power: self.voting_power.get(&proposer).copied().unwrap_or(0),
        };

        let fork = ForkDeclaration::new(
            name.clone(),
            fork_type.clone(),
            declarant,
            current_block,
            description,
        );

        let fork_id = fork.id;

        // Check if already exists
        if self.proposals.contains_key(&fork_id) {
            return Err(ForkError::ForkAlreadyExists(fork_id));
        }

        // Insert fork and add proposer as first supporter
        let mut fork = fork;
        fork.supporting_validators.insert(proposer);

        self.proposals.insert(fork_id, fork);

        self.events.push(ForkEvent::ForkProposed {
            fork_id,
            name,
            fork_type,
            proposed_at: current_block,
        });

        Ok(fork_id)
    }

    /// Support a fork proposal
    pub fn support_fork(
        &mut self,
        fork_id: Hash,
        supporter: AccountId,
        stake_amount: Balance,
        current_block: BlockNumber,
    ) -> Result<(), ForkError> {
        let power = self.voting_power.get(&supporter).copied()
            .ok_or(ForkError::NoVotingPower)?;

        let fork = self.proposals.get_mut(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        // Check if already voted
        if fork.supporting_validators.contains(&supporter) {
            return Err(ForkError::AlreadyVoted);
        }

        // Check status
        if fork.status != ForkStatus::Proposed {
            return Err(ForkError::ForkAlreadyDeclared);
        }

        // Add support
        fork.supporting_validators.insert(supporter);
        fork.supporting_stake += stake_amount;

        self.events.push(ForkEvent::ValidatorSupport {
            fork_id,
            validator: supporter,
            power,
        });

        // Check if threshold met
        self.check_declaration_threshold(fork_id, current_block)?;

        Ok(())
    }

    /// Check if fork declaration threshold is met
    fn check_declaration_threshold(
        &mut self,
        fork_id: Hash,
        current_block: BlockNumber,
    ) -> Result<(), ForkError> {
        let fork = self.proposals.get_mut(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        if fork.status != ForkStatus::Proposed {
            return Ok(());
        }

        let validator_count = fork.supporting_validators.len() as u32;
        let total_validators = self.voting_power.len() as u32;
        // Use saturating_mul to prevent overflow on large validator counts
        let validator_percent = if total_validators > 0 {
            validator_count.saturating_mul(100) / total_validators
        } else {
            0
        };

        // Use saturating_mul to prevent overflow on large stake values
        let stake_percent = if self.total_stake > 0 {
            (fork.supporting_stake.saturating_mul(100) / self.total_stake) as u8
        } else {
            0
        };

        // Check thresholds (SPEC v8 Section 3.2)
        let meets_validator_threshold = validator_percent >= FORK_VALIDATOR_THRESHOLD_PERCENT as u32;
        let meets_stake_threshold = stake_percent >= FORK_STAKE_THRESHOLD_PERCENT;

        if meets_validator_threshold || meets_stake_threshold {
            // Declare the fork
            fork.status = ForkStatus::Declared;

            // Update declarant info
            fork.declared_by = if meets_validator_threshold {
                ForkDeclarant::Validators {
                    count: validator_count,
                    total_validators,
                    voting_power: fork.supporting_validators.iter()
                        .filter_map(|v| self.voting_power.get(v))
                        .sum(),
                }
            } else {
                ForkDeclarant::Stake {
                    amount: fork.supporting_stake,
                    total_stake: self.total_stake,
                    percent: stake_percent,
                }
            };

            // Recalculate preparation period
            let prep_period = fork.fork_type.min_preparation_period();
            fork.preparation_ends = current_block + prep_period;
            fork.fork_height = fork.preparation_ends + 14_400;

            self.events.push(ForkEvent::ForkDeclared {
                fork_id,
                declared_at: current_block,
                preparation_ends: fork.preparation_ends,
            });
        }

        Ok(())
    }

    /// Declare fork from sidechains
    pub fn declare_fork_from_sidechains(
        &mut self,
        fork_id: Hash,
        sidechains: Vec<ChainId>,
        current_block: BlockNumber,
    ) -> Result<(), ForkError> {
        let fork = self.proposals.get_mut(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        if fork.status != ForkStatus::Proposed {
            return Err(ForkError::ForkAlreadyDeclared);
        }

        // Add sidechains
        for chain in &sidechains {
            if !fork.supporting_sidechains.contains(chain) {
                fork.supporting_sidechains.push(*chain);
            }
        }

        // Check threshold (SPEC v8: ≥3 major sidechains)
        if fork.supporting_sidechains.len() >= FORK_SIDECHAIN_THRESHOLD {
            fork.status = ForkStatus::Declared;
            fork.declared_by = ForkDeclarant::Sidechains {
                chains: fork.supporting_sidechains.clone(),
            };

            let prep_period = fork.fork_type.min_preparation_period();
            fork.preparation_ends = current_block + prep_period;
            fork.fork_height = fork.preparation_ends + 14_400;

            self.events.push(ForkEvent::ForkDeclared {
                fork_id,
                declared_at: current_block,
                preparation_ends: fork.preparation_ends,
            });
        }

        Ok(())
    }

    // =========================================================================
    // FORK PREPARATION
    // =========================================================================

    /// Create snapshot for a declared fork
    pub fn create_snapshot(
        &mut self,
        fork_id: Hash,
        state_root: Hash,
        validators: Vec<ValidatorSnapshot>,
        identity_root: Hash,
        reputation_root: Hash,
        sidechain_root: Hash,
        total_supply: Balance,
        current_block: BlockNumber,
    ) -> Result<(), ForkError> {
        let fork = self.proposals.get(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        if fork.status != ForkStatus::Declared {
            return Err(ForkError::NotInPreparation);
        }

        if self.snapshots.contains_key(&fork_id) {
            return Err(ForkError::SnapshotAlreadyExists);
        }

        let snapshot = ForkSnapshot {
            block_number: current_block,
            state_root,
            validator_set: validators,
            identity_merkle_root: identity_root,
            reputation_merkle_root: reputation_root,
            sidechain_registry_root: sidechain_root,
            total_supply,
            created_at: current_block,
            fork_id,
        };

        self.snapshots.insert(fork_id, snapshot);

        // Initialize continuity data
        self.continuity.insert(fork_id, ForkContinuity::new(fork_id, current_block));

        self.events.push(ForkEvent::SnapshotCreated {
            fork_id,
            block_number: current_block,
            state_root,
        });

        Ok(())
    }

    /// Add balance to snapshot
    pub fn add_balance_to_snapshot(
        &mut self,
        fork_id: Hash,
        account: AccountId,
        balance: Balance,
    ) -> Result<(), ForkError> {
        let continuity = self.continuity.get_mut(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        continuity.balance_snapshot.insert(account, balance);
        Ok(())
    }

    /// Add identity to snapshot
    pub fn add_identity_to_snapshot(
        &mut self,
        fork_id: Hash,
        identity: IdentitySnapshot,
    ) -> Result<(), ForkError> {
        let continuity = self.continuity.get_mut(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        continuity.identity_snapshots.insert(identity.identity_hash, identity);
        Ok(())
    }

    /// Add reputation to snapshot with post-fork decay
    pub fn add_reputation_to_snapshot(
        &mut self,
        fork_id: Hash,
        reputation: ReputationSnapshot,
    ) -> Result<(), ForkError> {
        let continuity = self.continuity.get_mut(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        let key = (reputation.chain_id, reputation.identity_hash);
        continuity.reputation_snapshots.insert(key, reputation);
        Ok(())
    }

    // =========================================================================
    // SIDECHAIN ALIGNMENT
    // =========================================================================

    /// Set sidechain alignment for a fork
    pub fn set_sidechain_alignment(
        &mut self,
        fork_id: Hash,
        chain_id: ChainId,
        alignment: ForkAlignment,
        proposal_id: Hash,
        current_block: BlockNumber,
    ) -> Result<(), ForkError> {
        let fork = self.proposals.get(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        // Check deadline
        if current_block > fork.fork_height.saturating_sub(SIDECHAIN_ALIGNMENT_DEADLINE) {
            return Err(ForkError::AlignmentDeadlinePassed);
        }

        let key = (fork_id, chain_id);
        let mut sidechain_alignment = self.alignments
            .remove(&key)
            .unwrap_or_else(|| SidechainAlignment::undecided(chain_id, fork_id));

        sidechain_alignment.set_alignment(alignment, current_block, proposal_id);

        self.alignments.insert(key, sidechain_alignment.clone());

        // Update continuity
        if let Some(continuity) = self.continuity.get_mut(&fork_id) {
            continuity.sidechain_alignments.insert(chain_id, alignment);
        }

        self.events.push(ForkEvent::SidechainAligned {
            fork_id,
            chain_id,
            alignment,
        });

        Ok(())
    }

    /// Get sidechain alignment
    pub fn get_alignment(&self, fork_id: Hash, chain_id: ChainId) -> ForkAlignment {
        self.alignments
            .get(&(fork_id, chain_id))
            .map(|a| a.alignment)
            .unwrap_or(ForkAlignment::Undecided)
    }

    /// Apply default alignment (independence) for undecided sidechains
    pub fn apply_default_alignments(
        &mut self,
        fork_id: Hash,
        sidechains: Vec<ChainId>,
        current_block: BlockNumber,
    ) -> Result<(), ForkError> {
        for chain_id in sidechains {
            let key = (fork_id, chain_id);
            if !self.alignments.contains_key(&key) {
                let mut alignment = SidechainAlignment::undecided(chain_id, fork_id);
                alignment.alignment = ForkAlignment::Independent; // Default to independence
                alignment.decided_at = Some(current_block);
                self.alignments.insert(key, alignment);

                if let Some(continuity) = self.continuity.get_mut(&fork_id) {
                    continuity.sidechain_alignments.insert(chain_id, ForkAlignment::Independent);
                }
            }
        }
        Ok(())
    }

    // =========================================================================
    // FORK EXECUTION
    // =========================================================================

    /// Mark fork as ready for execution
    pub fn mark_ready(
        &mut self,
        fork_id: Hash,
        current_block: BlockNumber,
    ) -> Result<(), ForkError> {
        let fork = self.proposals.get_mut(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        if fork.status != ForkStatus::Declared {
            return Err(ForkError::NotInPreparation);
        }

        if current_block < fork.preparation_ends {
            return Err(ForkError::PreparationNotMet);
        }

        fork.status = ForkStatus::Ready;
        Ok(())
    }

    /// Execute a fork
    pub fn execute_fork(
        &mut self,
        fork_id: Hash,
        new_chain_id: ChainId,
        current_block: BlockNumber,
    ) -> Result<(), ForkError> {
        let fork = self.proposals.get_mut(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        if fork.status != ForkStatus::Ready {
            return Err(ForkError::ForkNotReady);
        }

        if current_block < fork.fork_height {
            return Err(ForkError::PreparationNotMet);
        }

        fork.status = ForkStatus::Executed;
        fork.new_chain_id = Some(new_chain_id);

        self.last_fork_executed = Some(current_block);

        // Move to executed list
        let executed = self.proposals.remove(&fork_id).unwrap();
        self.executed_forks.push(executed);

        self.events.push(ForkEvent::ForkExecuted {
            fork_id,
            executed_at: current_block,
            new_chain_id,
        });

        Ok(())
    }

    /// Cancel a fork proposal
    pub fn cancel_fork(
        &mut self,
        fork_id: Hash,
        reason: String,
    ) -> Result<(), ForkError> {
        let fork = self.proposals.get_mut(&fork_id)
            .ok_or(ForkError::ForkNotFound(fork_id))?;

        if matches!(fork.status, ForkStatus::Executed) {
            return Err(ForkError::ForkNotFound(fork_id));
        }

        fork.status = ForkStatus::Cancelled;

        self.events.push(ForkEvent::ForkCancelled {
            fork_id,
            reason,
        });

        Ok(())
    }

    // =========================================================================
    // OSSIFICATION
    // =========================================================================

    /// Propose ossification
    pub fn propose_ossification(
        &mut self,
        proposer: AccountId,
        current_block: BlockNumber,
        emergency_active: bool,
    ) -> Result<(), ForkError> {
        if emergency_active {
            return Err(ForkError::CannotOssifyDuringEmergency);
        }

        if self.ossification.ossification_active {
            return Err(ForkError::AlreadyOssified);
        }

        if !self.ossification.can_trigger_ossification(current_block) {
            return Err(ForkError::OssificationNotEligible);
        }

        self.ossification.ossification_proposed = true;
        self.ossification.proposed_at = Some(current_block);

        self.events.push(ForkEvent::OssificationProposed {
            proposed_at: current_block,
        });

        Ok(())
    }

    /// Vote on ossification
    pub fn vote_ossification(
        &mut self,
        voter: AccountId,
        approve: bool,
        current_block: BlockNumber,
    ) -> Result<(), ForkError> {
        if !self.ossification.ossification_proposed {
            return Err(ForkError::OssificationNotEligible);
        }

        if self.ossification.ossification_active {
            return Err(ForkError::AlreadyOssified);
        }

        let power = self.voting_power.get(&voter).copied()
            .ok_or(ForkError::NoVotingPower)?;

        // Remove previous vote if any
        if let Some(previous) = self.ossification.ossification_votes.get(&voter) {
            if *previous {
                self.ossification.approval_power -= power;
            } else {
                self.ossification.rejection_power -= power;
            }
        }

        // Record new vote
        self.ossification.ossification_votes.insert(voter, approve);
        if approve {
            self.ossification.approval_power += power;
        } else {
            self.ossification.rejection_power += power;
        }

        self.events.push(ForkEvent::OssificationVote {
            voter,
            approve,
        });

        // Check if threshold met (90% of TOTAL validator power, not just voters)
        // SPEC v8 Section 9.1: ≥90% validator consensus
        // Use saturating_mul to prevent overflow on large power values
        let approval_of_total = if self.total_power > 0 {
            (self.ossification.approval_power.saturating_mul(100) / self.total_power) as u8
        } else {
            0
        };

        if approval_of_total >= OSSIFICATION_APPROVAL_THRESHOLD {
            self.ossification.ossification_active = true;
            self.ossification.activated_at = Some(current_block);

            self.events.push(ForkEvent::OssificationActivated {
                activated_at: current_block,
            });
        }

        Ok(())
    }

    /// Check if ossification is active
    pub fn is_ossified(&self) -> bool {
        self.ossification.ossification_active
    }

    // =========================================================================
    // QUERIES
    // =========================================================================

    /// Get fork proposal
    pub fn get_fork(&self, fork_id: Hash) -> Option<&ForkDeclaration> {
        self.proposals.get(&fork_id)
    }

    /// Get fork snapshot
    pub fn get_snapshot(&self, fork_id: Hash) -> Option<&ForkSnapshot> {
        self.snapshots.get(&fork_id)
    }

    /// Get fork continuity data
    pub fn get_continuity(&self, fork_id: Hash) -> Option<&ForkContinuity> {
        self.continuity.get(&fork_id)
    }

    /// Get active fork proposals
    pub fn active_forks(&self) -> Vec<&ForkDeclaration> {
        self.proposals.values()
            .filter(|f| matches!(f.status, ForkStatus::Proposed | ForkStatus::Declared | ForkStatus::Ready))
            .collect()
    }

    /// Get executed forks history
    pub fn fork_history(&self) -> &[ForkDeclaration] {
        &self.executed_forks
    }

    /// Drain events
    pub fn drain_events(&mut self) -> Vec<ForkEvent> {
        std::mem::take(&mut self.events)
    }
}

impl Default for ForkContract {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::protocol::ProtocolVersion;

    fn create_account(seed: u8) -> AccountId {
        AccountId::from_bytes([seed; 32])
    }

    fn setup_contract() -> ForkContract {
        let mut contract = ForkContract::new();

        // Set up validators with voting power
        for i in 1..=10 {
            contract.set_voting_power(create_account(i), 10);
        }
        contract.set_total_stake(100_000_000);

        contract
    }

    #[test]
    fn test_new_contract() {
        let contract = ForkContract::new();

        assert!(contract.proposals.is_empty());
        assert!(contract.executed_forks.is_empty());
        assert!(!contract.ossification.ossification_active);
    }

    #[test]
    fn test_propose_fork() {
        let mut contract = setup_contract();

        let result = contract.propose_fork(
            create_account(1),
            "Reform Fork".to_string(),
            ForkType::Governance {
                deadlock_reason: "Quorum failures".to_string(),
                failed_proposals: 5,
            },
            "A governance reform fork".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        );

        assert!(result.is_ok());
        let fork_id = result.unwrap();

        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Proposed);
        assert_eq!(fork.name, "Reform Fork");
    }

    #[test]
    fn test_insufficient_deposit() {
        let mut contract = setup_contract();

        let result = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT - 1,
            1000,
        );

        assert!(matches!(result, Err(ForkError::InsufficientDeposit { .. })));
    }

    #[test]
    fn test_support_fork() {
        let mut contract = setup_contract();

        let fork_id = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // Support from validators
        for i in 2..=4 {
            contract.support_fork(
                fork_id,
                create_account(i),
                10_000_000,
                1100,
            ).unwrap();
        }

        let fork = contract.get_fork(fork_id).unwrap();
        // 4 validators: proposer (1) + 3 supporters (2, 3, 4)
        assert_eq!(fork.supporting_validators.len(), 4);
        assert_eq!(fork.supporting_stake, 30_000_000);
    }

    #[test]
    fn test_fork_declaration_threshold() {
        let mut contract = setup_contract();

        let fork_id = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // Need 33% of validators (4/10) or 40% stake
        for i in 2..=4 {
            contract.support_fork(
                fork_id,
                create_account(i),
                10_000_000,
                1100,
            ).unwrap();
        }

        // 4/10 = 40% > 33% threshold
        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Declared);
    }

    #[test]
    fn test_create_snapshot() {
        let mut contract = setup_contract();

        let fork_id = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // Declare the fork
        for i in 2..=4 {
            contract.support_fork(fork_id, create_account(i), 10_000_000, 1100).unwrap();
        }

        // Create snapshot
        let result = contract.create_snapshot(
            fork_id,
            Hash::hash(b"state"),
            vec![],
            Hash::hash(b"identity"),
            Hash::hash(b"reputation"),
            Hash::hash(b"sidechains"),
            100_000_000_000,
            2000,
        );

        assert!(result.is_ok());
        assert!(contract.get_snapshot(fork_id).is_some());
    }

    #[test]
    fn test_sidechain_alignment() {
        let mut contract = setup_contract();

        let fork_id = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // Declare the fork
        for i in 2..=4 {
            contract.support_fork(fork_id, create_account(i), 10_000_000, 1100).unwrap();
        }

        // Set alignment before deadline
        let result = contract.set_sidechain_alignment(
            fork_id,
            ChainId(1),
            ForkAlignment::FollowB,
            Hash::hash(b"proposal"),
            2000,
        );

        assert!(result.is_ok());
        assert_eq!(contract.get_alignment(fork_id, ChainId(1)), ForkAlignment::FollowB);
    }

    #[test]
    fn test_sidechain_default_alignment() {
        let mut contract = setup_contract();

        let fork_id = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        for i in 2..=4 {
            contract.support_fork(fork_id, create_account(i), 10_000_000, 1100).unwrap();
        }

        // Apply default alignments
        contract.apply_default_alignments(
            fork_id,
            vec![ChainId(1), ChainId(2)],
            2000,
        ).unwrap();

        // Default is Independence
        assert_eq!(contract.get_alignment(fork_id, ChainId(1)), ForkAlignment::Independent);
        assert_eq!(contract.get_alignment(fork_id, ChainId(2)), ForkAlignment::Independent);
    }

    #[test]
    fn test_fork_execution() {
        let mut contract = setup_contract();

        let fork_id = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // Declare
        for i in 2..=4 {
            contract.support_fork(fork_id, create_account(i), 10_000_000, 1100).unwrap();
        }

        let fork = contract.get_fork(fork_id).unwrap();
        let prep_ends = fork.preparation_ends;
        let fork_height = fork.fork_height;

        // Mark ready
        contract.mark_ready(fork_id, prep_ends + 1).unwrap();

        // Execute
        let result = contract.execute_fork(fork_id, ChainId(100), fork_height + 1);
        assert!(result.is_ok());

        // Fork should be in history now
        assert!(contract.get_fork(fork_id).is_none());
        assert_eq!(contract.fork_history().len(), 1);
        assert_eq!(contract.fork_history()[0].status, ForkStatus::Executed);
    }

    #[test]
    fn test_ossification_proposal() {
        let mut contract = setup_contract();

        // Set last parameter change long ago (10+ years)
        contract.ossification.last_parameter_change = 0;

        let current_block = crate::types::fork::BLOCKS_PER_YEAR * 11;

        let result = contract.propose_ossification(
            create_account(1),
            current_block,
            false,
        );

        assert!(result.is_ok());
        assert!(contract.ossification.ossification_proposed);
    }

    #[test]
    fn test_ossification_during_emergency() {
        let mut contract = setup_contract();
        contract.ossification.last_parameter_change = 0;

        let current_block = crate::types::fork::BLOCKS_PER_YEAR * 11;

        let result = contract.propose_ossification(
            create_account(1),
            current_block,
            true, // Emergency active
        );

        assert!(matches!(result, Err(ForkError::CannotOssifyDuringEmergency)));
    }

    #[test]
    fn test_ossification_voting() {
        let mut contract = setup_contract();
        contract.ossification.last_parameter_change = 0;

        let current_block = crate::types::fork::BLOCKS_PER_YEAR * 11;

        contract.propose_ossification(create_account(1), current_block, false).unwrap();

        // Vote with 9/10 validators (90%)
        for i in 1..=9 {
            contract.vote_ossification(create_account(i), true, current_block + 100).unwrap();
        }

        assert!(contract.is_ossified());
    }

    #[test]
    fn test_ossification_needs_90_percent() {
        let mut contract = setup_contract();
        contract.ossification.last_parameter_change = 0;

        let current_block = crate::types::fork::BLOCKS_PER_YEAR * 11;

        contract.propose_ossification(create_account(1), current_block, false).unwrap();

        // Vote with 8/10 validators (80%) - not enough
        for i in 1..=8 {
            contract.vote_ossification(create_account(i), true, current_block + 100).unwrap();
        }

        // 2 reject
        for i in 9..=10 {
            contract.vote_ossification(create_account(i), false, current_block + 100).unwrap();
        }

        assert!(!contract.is_ossified());
        assert_eq!(contract.ossification.approval_percent(), 80);
    }

    #[test]
    fn test_fork_from_sidechains() {
        let mut contract = setup_contract();

        let fork_id = contract.propose_fork(
            create_account(1),
            "Sidechain Fork".to_string(),
            ForkType::Constitutional {
                violated_axiom: crate::types::protocol::ConstitutionalAxiom::ExitAlwaysPossible,
                rationale: "Exit blocked".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // 3 sidechains declare support
        contract.declare_fork_from_sidechains(
            fork_id,
            vec![ChainId(1), ChainId(2), ChainId(3)],
            1100,
        ).unwrap();

        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Declared);
        assert!(matches!(fork.declared_by, ForkDeclarant::Sidechains { .. }));
    }

    #[test]
    fn test_max_concurrent_forks() {
        let mut contract = setup_contract();

        // Create MAX_CONCURRENT_FORKS proposals
        for i in 0..MAX_CONCURRENT_FORKS {
            contract.propose_fork(
                create_account(1),
                format!("Fork {}", i),
                ForkType::Technical {
                    version: ProtocolVersion::new(2, i as u16, 0),
                    description: "Upgrade".to_string(),
                },
                "Description".to_string(),
                FORK_PROPOSAL_DEPOSIT,
                1000 + i as u64,
            ).unwrap();
        }

        // Next one should fail
        let result = contract.propose_fork(
            create_account(1),
            "Too Many".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(3, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            2000,
        );

        assert!(matches!(result, Err(ForkError::TooManyConcurrentForks)));
    }

    #[test]
    fn test_fork_cooldown() {
        let mut contract = setup_contract();

        // Set last fork executed
        contract.last_fork_executed = Some(1000);

        // Try to propose during cooldown
        let result = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000 + FORK_COOLDOWN - 1,
        );

        assert!(matches!(result, Err(ForkError::ForkInCooldown { .. })));

        // After cooldown is OK
        let result = contract.propose_fork(
            create_account(1),
            "Test Fork After".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000 + FORK_COOLDOWN + 1,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_cancel_fork() {
        let mut contract = setup_contract();

        let fork_id = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        contract.cancel_fork(fork_id, "Not needed".to_string()).unwrap();

        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Cancelled);
    }

    #[test]
    fn test_drain_events() {
        let mut contract = setup_contract();

        contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Upgrade".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        let events = contract.drain_events();
        assert!(!events.is_empty());

        // Events should be cleared
        let events2 = contract.drain_events();
        assert!(events2.is_empty());
    }
}
