// Fork Types - SPEC v8: Long-Term Resilience, Forking & Protocol Survivability
// Principle: Forking is a first-class mechanism, not a failure
//
// This module defines:
// - Fork types (technical, constitutional, governance, social, survival)
// - Fork declarations and declarants
// - Fork snapshots for state continuity
// - Sidechain alignment mechanisms
// - Ossification mode

use crate::types::{
    AccountId, Balance, BlockNumber, ChainId, Hash,
    IdentityStatus, ReputationDomain,
};
use crate::types::protocol::{ConstitutionalAxiom, ProtocolVersion};
use std::collections::{HashMap, HashSet};

// =============================================================================
// FORK TYPES (SPEC v8 Section 3.1)
// =============================================================================

/// Types of forks as defined in SPEC v8
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForkType {
    /// Technical fork: bugfix, security patch
    Technical {
        version: ProtocolVersion,
        description: String,
    },

    /// Constitutional fork: axiom-level disagreement
    Constitutional {
        violated_axiom: ConstitutionalAxiom,
        rationale: String,
    },

    /// Governance fork: irreconcilable voting deadlock
    Governance {
        deadlock_reason: String,
        failed_proposals: u32,
    },

    /// Social fork: community value divergence
    Social {
        community_rationale: String,
    },

    /// Survival fork: external coercion or capture
    Survival {
        external_threat: String,
    },
}

impl ForkType {
    /// Get the minimum preparation period for this fork type (in blocks)
    pub fn min_preparation_period(&self) -> BlockNumber {
        match self {
            // Technical forks can be faster
            ForkType::Technical { .. } => 30 * 14_400, // 30 days

            // Constitutional forks need more time
            ForkType::Constitutional { .. } => 60 * 14_400, // 60 days

            // Governance forks standard period
            ForkType::Governance { .. } => 45 * 14_400, // 45 days

            // Social forks need time for community
            ForkType::Social { .. } => 60 * 14_400, // 60 days

            // Survival forks can be urgent
            ForkType::Survival { .. } => 30 * 14_400, // 30 days
        }
    }

    /// Get the maximum preparation period (in blocks)
    pub fn max_preparation_period(&self) -> BlockNumber {
        90 * 14_400 // 90 days for all types
    }
}

// =============================================================================
// FORK DECLARATION (SPEC v8 Section 3.2)
// =============================================================================

/// Who can declare a fork
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForkDeclarant {
    /// ≥33% of validators
    Validators {
        count: u32,
        total_validators: u32,
        voting_power: u64,
    },

    /// ≥40% of active stake
    Stake {
        amount: Balance,
        total_stake: Balance,
        percent: u8,
    },

    /// ≥3 major sidechains
    Sidechains {
        chains: Vec<ChainId>,
    },

    /// Emergency escalation failure
    EmergencyEscalation {
        failed_recoveries: u8,
    },
}

impl ForkDeclarant {
    /// Check if this declarant meets the threshold for declaration
    pub fn meets_threshold(&self) -> bool {
        match self {
            ForkDeclarant::Validators { count, total_validators, .. } => {
                // ≥33% of validators
                let threshold = (*total_validators * 33) / 100;
                *count >= threshold.max(1)
            }
            ForkDeclarant::Stake { percent, .. } => {
                // ≥40% of active stake
                *percent >= 40
            }
            ForkDeclarant::Sidechains { chains } => {
                // ≥3 major sidechains
                chains.len() >= 3
            }
            ForkDeclarant::EmergencyEscalation { failed_recoveries } => {
                // Emergency escalation if recovery failed
                *failed_recoveries >= 2
            }
        }
    }
}

/// Status of a fork declaration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkStatus {
    /// Fork has been proposed, gathering support
    Proposed,

    /// Fork declaration threshold met, preparation phase
    Declared,

    /// Preparation complete, ready to execute
    Ready,

    /// Fork executed, chains split
    Executed,

    /// Fork cancelled (not enough support)
    Cancelled,

    /// Fork expired without execution
    Expired,
}

/// A fork declaration
#[derive(Debug, Clone)]
pub struct ForkDeclaration {
    /// Unique fork ID
    pub id: Hash,

    /// Name of the fork
    pub name: String,

    /// Type of fork
    pub fork_type: ForkType,

    /// Who declared the fork
    pub declared_by: ForkDeclarant,

    /// Block when fork was declared
    pub declared_at: BlockNumber,

    /// Block when preparation ends
    pub preparation_ends: BlockNumber,

    /// Block height where fork will occur
    pub fork_height: BlockNumber,

    /// Validators supporting this fork
    pub supporting_validators: HashSet<AccountId>,

    /// Total stake supporting this fork
    pub supporting_stake: Balance,

    /// Sidechains supporting this fork
    pub supporting_sidechains: Vec<ChainId>,

    /// Current status
    pub status: ForkStatus,

    /// New chain ID for the fork (post-split)
    pub new_chain_id: Option<ChainId>,

    /// Description/manifesto
    pub description: String,
}

impl ForkDeclaration {
    /// Create a new fork declaration
    pub fn new(
        name: String,
        fork_type: ForkType,
        declared_by: ForkDeclarant,
        declared_at: BlockNumber,
        description: String,
    ) -> Self {
        // Generate unique ID
        let mut id_data = Vec::new();
        id_data.extend_from_slice(name.as_bytes());
        id_data.extend_from_slice(&declared_at.to_le_bytes());
        let id = Hash::hash(&id_data);

        // Calculate preparation end
        let preparation_period = fork_type.min_preparation_period();
        let preparation_ends = declared_at + preparation_period;

        // Fork height is slightly after preparation ends
        let fork_height = preparation_ends + 14_400; // +1 day buffer

        Self {
            id,
            name,
            fork_type,
            declared_by,
            declared_at,
            preparation_ends,
            fork_height,
            supporting_validators: HashSet::new(),
            supporting_stake: 0,
            supporting_sidechains: Vec::new(),
            status: ForkStatus::Proposed,
            new_chain_id: None,
            description,
        }
    }

    /// Check if fork is in preparation phase
    pub fn is_in_preparation(&self, current_block: BlockNumber) -> bool {
        self.status == ForkStatus::Declared && current_block < self.preparation_ends
    }

    /// Check if fork is ready to execute
    pub fn is_ready(&self, current_block: BlockNumber) -> bool {
        self.status == ForkStatus::Ready
            || (self.status == ForkStatus::Declared && current_block >= self.preparation_ends)
    }

    /// Get support percentage
    pub fn support_percent(&self, total_stake: Balance) -> u8 {
        if total_stake == 0 {
            return 0;
        }
        ((self.supporting_stake * 100) / total_stake) as u8
    }
}

// =============================================================================
// FORK SNAPSHOT (SPEC v8 Section 5.1)
// =============================================================================

/// Complete state snapshot at fork point
#[derive(Debug, Clone)]
pub struct ForkSnapshot {
    /// Block number of snapshot
    pub block_number: BlockNumber,

    /// State root at snapshot
    pub state_root: Hash,

    /// Validator set at snapshot
    pub validator_set: Vec<ValidatorSnapshot>,

    /// Merkle root of all identities
    pub identity_merkle_root: Hash,

    /// Merkle root of all reputations
    pub reputation_merkle_root: Hash,

    /// Merkle root of sidechain registry
    pub sidechain_registry_root: Hash,

    /// Total supply at snapshot
    pub total_supply: Balance,

    /// When snapshot was created
    pub created_at: BlockNumber,

    /// Fork this snapshot is for
    pub fork_id: Hash,
}

/// Snapshot of a validator's state
#[derive(Debug, Clone)]
pub struct ValidatorSnapshot {
    pub id: AccountId,
    pub stake: Balance,
    pub validator_credits: u64,
    pub is_active: bool,
}

/// Snapshot of an identity for fork continuity
#[derive(Debug, Clone)]
pub struct IdentitySnapshot {
    pub identity_hash: Hash,
    pub status: IdentityStatus,
    pub attestation_count: u32,
    pub deposit: Balance,
    pub registered_at: BlockNumber,
}

/// Snapshot of reputation for fork continuity
#[derive(Debug, Clone)]
pub struct ReputationSnapshot {
    pub identity_hash: Hash,
    pub chain_id: ChainId,
    pub domains: HashMap<ReputationDomain, u64>,
    /// Post-fork decay multiplier (e.g., 2 = 2x faster decay)
    pub post_fork_decay_multiplier: u8,
}

impl ReputationSnapshot {
    /// Apply post-fork decay to a score
    pub fn apply_fork_decay(&self, original_score: u64, epochs_since_fork: u64) -> u64 {
        // Faster decay post-fork to prevent infinite inflation
        let decay_per_epoch = 5 * self.post_fork_decay_multiplier as u64;
        let total_decay = decay_per_epoch * epochs_since_fork;

        original_score.saturating_sub(total_decay)
    }
}

// =============================================================================
// SIDECHAIN ALIGNMENT (SPEC v8 Section 7)
// =============================================================================

/// Sidechain's choice for a fork
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkAlignment {
    /// Follow fork A (original chain)
    FollowA,

    /// Follow fork B (new fork)
    FollowB,

    /// Become independent (sovereign)
    Independent,

    /// Not yet decided
    Undecided,
}

impl Default for ForkAlignment {
    fn default() -> Self {
        ForkAlignment::Undecided
    }
}

/// Sidechain alignment declaration
#[derive(Debug, Clone)]
pub struct SidechainAlignment {
    pub chain_id: ChainId,
    pub fork_id: Hash,
    pub alignment: ForkAlignment,
    pub decided_at: Option<BlockNumber>,
    pub decided_by: Option<Hash>, // Governance proposal ID
}

impl SidechainAlignment {
    /// Create undecided alignment
    pub fn undecided(chain_id: ChainId, fork_id: Hash) -> Self {
        Self {
            chain_id,
            fork_id,
            alignment: ForkAlignment::Undecided,
            decided_at: None,
            decided_by: None,
        }
    }

    /// Set alignment choice
    pub fn set_alignment(
        &mut self,
        alignment: ForkAlignment,
        current_block: BlockNumber,
        proposal_id: Hash,
    ) {
        self.alignment = alignment;
        self.decided_at = Some(current_block);
        self.decided_by = Some(proposal_id);
    }

    /// Check if alignment has been decided
    pub fn is_decided(&self) -> bool {
        self.alignment != ForkAlignment::Undecided
    }
}

// =============================================================================
// OSSIFICATION (SPEC v8 Section 9)
// =============================================================================

/// Ossification state for long-term protocol stability
#[derive(Debug, Clone)]
pub struct OssificationState {
    /// Last block where a parameter was changed
    pub last_parameter_change: BlockNumber,

    /// Whether ossification has been proposed
    pub ossification_proposed: bool,

    /// Block when ossification was proposed
    pub proposed_at: Option<BlockNumber>,

    /// Validators who voted for ossification
    pub ossification_votes: HashMap<AccountId, bool>,

    /// Total voting power for ossification
    pub approval_power: u64,

    /// Total voting power against ossification
    pub rejection_power: u64,

    /// Whether ossification is active
    pub ossification_active: bool,

    /// When ossification was activated
    pub activated_at: Option<BlockNumber>,
}

impl OssificationState {
    pub fn new() -> Self {
        Self {
            last_parameter_change: 0,
            ossification_proposed: false,
            proposed_at: None,
            ossification_votes: HashMap::new(),
            approval_power: 0,
            rejection_power: 0,
            ossification_active: false,
            activated_at: None,
        }
    }

    /// Check if ossification trigger conditions are met
    /// SPEC v8 Section 9.1: ≥10 years without parameter change
    pub fn can_trigger_ossification(&self, current_block: BlockNumber) -> bool {
        if self.ossification_active {
            return false;
        }

        let blocks_since_change = current_block.saturating_sub(self.last_parameter_change);
        let years = blocks_since_change / BLOCKS_PER_YEAR;

        years >= 10
    }

    /// Get approval percentage for ossification
    pub fn approval_percent(&self) -> u8 {
        let total = self.approval_power + self.rejection_power;
        if total == 0 {
            return 0;
        }
        ((self.approval_power * 100) / total) as u8
    }

    /// Check if ossification has ≥90% approval (SPEC v8 Section 9.1)
    pub fn has_sufficient_approval(&self) -> bool {
        self.approval_percent() >= 90
    }

    /// Record a parameter change (resets ossification timer)
    pub fn record_parameter_change(&mut self, current_block: BlockNumber) {
        self.last_parameter_change = current_block;
        // Reset any pending ossification proposal
        if self.ossification_proposed && !self.ossification_active {
            self.ossification_proposed = false;
            self.proposed_at = None;
            self.ossification_votes.clear();
            self.approval_power = 0;
            self.rejection_power = 0;
        }
    }
}

impl Default for OssificationState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// FORK CONTINUITY (SPEC v8 Section 6)
// =============================================================================

/// Complete fork continuity data for asset and identity preservation
#[derive(Debug, Clone)]
pub struct ForkContinuity {
    /// Fork this continuity data is for
    pub fork_id: Hash,

    /// Block number of the fork
    pub fork_block: BlockNumber,

    /// Balance snapshots at fork
    pub balance_snapshot: HashMap<AccountId, Balance>,

    /// Identity snapshots at fork
    pub identity_snapshots: HashMap<Hash, IdentitySnapshot>,

    /// Reputation snapshots at fork
    pub reputation_snapshots: HashMap<(ChainId, Hash), ReputationSnapshot>,

    /// Sidechain alignments
    pub sidechain_alignments: HashMap<ChainId, ForkAlignment>,
}

impl ForkContinuity {
    pub fn new(fork_id: Hash, fork_block: BlockNumber) -> Self {
        Self {
            fork_id,
            fork_block,
            balance_snapshot: HashMap::new(),
            identity_snapshots: HashMap::new(),
            reputation_snapshots: HashMap::new(),
            sidechain_alignments: HashMap::new(),
        }
    }

    /// Get balance at fork for an account
    pub fn get_fork_balance(&self, account: &AccountId) -> Balance {
        self.balance_snapshot.get(account).copied().unwrap_or(0)
    }

    /// Check if identity exists at fork
    pub fn has_identity_at_fork(&self, identity_hash: &Hash) -> bool {
        self.identity_snapshots.contains_key(identity_hash)
    }

    /// Get reputation at fork for identity on chain
    pub fn get_fork_reputation(
        &self,
        chain_id: ChainId,
        identity_hash: &Hash,
    ) -> Option<&ReputationSnapshot> {
        self.reputation_snapshots.get(&(chain_id, *identity_hash))
    }
}

// =============================================================================
// CONSTANTS
// =============================================================================

/// Blocks per year (assuming 6 second blocks)
pub const BLOCKS_PER_YEAR: BlockNumber = 5_256_000;

/// Minimum validators for fork declaration (33%)
pub const FORK_VALIDATOR_THRESHOLD_PERCENT: u8 = 33;

/// Minimum stake for fork declaration (40%)
pub const FORK_STAKE_THRESHOLD_PERCENT: u8 = 40;

/// Minimum sidechains for fork declaration
pub const FORK_SIDECHAIN_THRESHOLD: usize = 3;

/// Ossification approval threshold (90%)
pub const OSSIFICATION_APPROVAL_THRESHOLD: u8 = 90;

/// Years without parameter change for ossification
pub const OSSIFICATION_YEARS_THRESHOLD: u64 = 10;

/// Default post-fork reputation decay multiplier
pub const POST_FORK_DECAY_MULTIPLIER: u8 = 2;

/// Minimum fork preparation period (30 days)
pub const MIN_FORK_PREPARATION: BlockNumber = 30 * 14_400;

/// Maximum fork preparation period (90 days)
pub const MAX_FORK_PREPARATION: BlockNumber = 90 * 14_400;

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_account(seed: u8) -> AccountId {
        AccountId::from_bytes([seed; 32])
    }

    #[test]
    fn test_fork_type_preparation_periods() {
        let technical = ForkType::Technical {
            version: ProtocolVersion::new(2, 0, 0),
            description: "Security fix".to_string(),
        };
        assert_eq!(technical.min_preparation_period(), 30 * 14_400);

        let constitutional = ForkType::Constitutional {
            violated_axiom: ConstitutionalAxiom::ExitAlwaysPossible,
            rationale: "Axiom violation".to_string(),
        };
        assert_eq!(constitutional.min_preparation_period(), 60 * 14_400);

        let survival = ForkType::Survival {
            external_threat: "State capture".to_string(),
        };
        assert_eq!(survival.min_preparation_period(), 30 * 14_400);
    }

    #[test]
    fn test_fork_declarant_thresholds() {
        // Validator threshold (33%)
        let validators = ForkDeclarant::Validators {
            count: 34,
            total_validators: 100,
            voting_power: 34,
        };
        assert!(validators.meets_threshold());

        let validators_below = ForkDeclarant::Validators {
            count: 32,
            total_validators: 100,
            voting_power: 32,
        };
        assert!(!validators_below.meets_threshold());

        // Stake threshold (40%)
        let stake = ForkDeclarant::Stake {
            amount: 40_000_000,
            total_stake: 100_000_000,
            percent: 40,
        };
        assert!(stake.meets_threshold());

        let stake_below = ForkDeclarant::Stake {
            amount: 39_000_000,
            total_stake: 100_000_000,
            percent: 39,
        };
        assert!(!stake_below.meets_threshold());

        // Sidechain threshold (3)
        let sidechains = ForkDeclarant::Sidechains {
            chains: vec![ChainId(1), ChainId(2), ChainId(3)],
        };
        assert!(sidechains.meets_threshold());

        let sidechains_below = ForkDeclarant::Sidechains {
            chains: vec![ChainId(1), ChainId(2)],
        };
        assert!(!sidechains_below.meets_threshold());
    }

    #[test]
    fn test_fork_declaration_creation() {
        let declarant = ForkDeclarant::Validators {
            count: 40,
            total_validators: 100,
            voting_power: 40,
        };

        let fork = ForkDeclaration::new(
            "Governance Reform Fork".to_string(),
            ForkType::Governance {
                deadlock_reason: "Quorum failures".to_string(),
                failed_proposals: 5,
            },
            declarant,
            1000,
            "A fork to resolve governance deadlock".to_string(),
        );

        assert_eq!(fork.status, ForkStatus::Proposed);
        assert_eq!(fork.declared_at, 1000);
        assert!(fork.preparation_ends > fork.declared_at);
        assert!(fork.fork_height > fork.preparation_ends);
    }

    #[test]
    fn test_fork_snapshot() {
        let snapshot = ForkSnapshot {
            block_number: 1_000_000,
            state_root: Hash::hash(b"state"),
            validator_set: vec![
                ValidatorSnapshot {
                    id: create_account(1),
                    stake: 100_000,
                    validator_credits: 50,
                    is_active: true,
                },
            ],
            identity_merkle_root: Hash::hash(b"identities"),
            reputation_merkle_root: Hash::hash(b"reputation"),
            sidechain_registry_root: Hash::hash(b"sidechains"),
            total_supply: 100_000_000_000,
            created_at: 1_000_000,
            fork_id: Hash::hash(b"fork1"),
        };

        assert_eq!(snapshot.block_number, 1_000_000);
        assert_eq!(snapshot.validator_set.len(), 1);
    }

    #[test]
    fn test_reputation_fork_decay() {
        let mut domains = HashMap::new();
        domains.insert(ReputationDomain::Governance, 100);

        let reputation = ReputationSnapshot {
            identity_hash: Hash::hash(b"identity"),
            chain_id: ChainId(1),
            domains,
            post_fork_decay_multiplier: 2,
        };

        // 2x decay: 5 * 2 = 10 per epoch
        // After 5 epochs: 100 - 50 = 50
        let decayed = reputation.apply_fork_decay(100, 5);
        assert_eq!(decayed, 50);

        // After 10 epochs: 100 - 100 = 0
        let fully_decayed = reputation.apply_fork_decay(100, 10);
        assert_eq!(fully_decayed, 0);
    }

    #[test]
    fn test_sidechain_alignment() {
        let fork_id = Hash::hash(b"fork");
        let mut alignment = SidechainAlignment::undecided(ChainId(1), fork_id);

        assert!(!alignment.is_decided());
        assert_eq!(alignment.alignment, ForkAlignment::Undecided);

        alignment.set_alignment(
            ForkAlignment::FollowB,
            1000,
            Hash::hash(b"proposal"),
        );

        assert!(alignment.is_decided());
        assert_eq!(alignment.alignment, ForkAlignment::FollowB);
        assert_eq!(alignment.decided_at, Some(1000));
    }

    #[test]
    fn test_ossification_trigger() {
        let mut state = OssificationState::new();

        // Not enough time passed
        state.last_parameter_change = 0;
        assert!(!state.can_trigger_ossification(BLOCKS_PER_YEAR * 5));

        // 10 years passed
        assert!(state.can_trigger_ossification(BLOCKS_PER_YEAR * 10));

        // 15 years passed
        assert!(state.can_trigger_ossification(BLOCKS_PER_YEAR * 15));

        // Already active
        state.ossification_active = true;
        assert!(!state.can_trigger_ossification(BLOCKS_PER_YEAR * 20));
    }

    #[test]
    fn test_ossification_approval() {
        let mut state = OssificationState::new();

        state.approval_power = 90;
        state.rejection_power = 10;

        assert_eq!(state.approval_percent(), 90);
        assert!(state.has_sufficient_approval());

        state.approval_power = 85;
        state.rejection_power = 15;

        assert_eq!(state.approval_percent(), 85);
        assert!(!state.has_sufficient_approval());
    }

    #[test]
    fn test_ossification_reset_on_param_change() {
        let mut state = OssificationState::new();

        state.ossification_proposed = true;
        state.proposed_at = Some(1000);
        state.ossification_votes.insert(create_account(1), true);
        state.approval_power = 50;

        // Parameter change resets pending ossification
        state.record_parameter_change(2000);

        assert!(!state.ossification_proposed);
        assert!(state.proposed_at.is_none());
        assert!(state.ossification_votes.is_empty());
        assert_eq!(state.approval_power, 0);
        assert_eq!(state.last_parameter_change, 2000);
    }

    #[test]
    fn test_fork_continuity() {
        let mut continuity = ForkContinuity::new(
            Hash::hash(b"fork"),
            1_000_000,
        );

        let account = create_account(1);
        continuity.balance_snapshot.insert(account, 100_000);

        assert_eq!(continuity.get_fork_balance(&account), 100_000);
        assert_eq!(continuity.get_fork_balance(&create_account(2)), 0);
    }

    #[test]
    fn test_fork_alignment_default() {
        let alignment = ForkAlignment::default();
        assert_eq!(alignment, ForkAlignment::Undecided);
    }

    #[test]
    fn test_fork_status_progression() {
        let statuses = vec![
            ForkStatus::Proposed,
            ForkStatus::Declared,
            ForkStatus::Ready,
            ForkStatus::Executed,
        ];

        // Verify all statuses exist
        assert_eq!(statuses.len(), 4);
    }

    #[test]
    fn test_fork_declaration_support_percent() {
        let declarant = ForkDeclarant::Validators {
            count: 50,
            total_validators: 100,
            voting_power: 50,
        };

        let mut fork = ForkDeclaration::new(
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Test".to_string(),
            },
            declarant,
            1000,
            "Description".to_string(),
        );

        fork.supporting_stake = 40_000_000;
        let percent = fork.support_percent(100_000_000);

        assert_eq!(percent, 40);
    }

    #[test]
    fn test_constants() {
        assert_eq!(FORK_VALIDATOR_THRESHOLD_PERCENT, 33);
        assert_eq!(FORK_STAKE_THRESHOLD_PERCENT, 40);
        assert_eq!(FORK_SIDECHAIN_THRESHOLD, 3);
        assert_eq!(OSSIFICATION_APPROVAL_THRESHOLD, 90);
        assert_eq!(OSSIFICATION_YEARS_THRESHOLD, 10);
        assert_eq!(POST_FORK_DECAY_MULTIPLIER, 2);
    }

    #[test]
    fn test_fork_is_ready() {
        let declarant = ForkDeclarant::Validators {
            count: 40,
            total_validators: 100,
            voting_power: 40,
        };

        let mut fork = ForkDeclaration::new(
            "Test".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Test".to_string(),
            },
            declarant,
            1000,
            "Desc".to_string(),
        );

        fork.status = ForkStatus::Declared;

        // Not ready during preparation
        assert!(!fork.is_ready(1000));

        // Ready after preparation
        assert!(fork.is_ready(fork.preparation_ends + 1));
    }
}
