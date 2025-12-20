// Emergency Types - SPEC v7: Threat Model & Emergency Powers
// Principle: Resilience is surviving failure without betrayal of principles

use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash};
use std::collections::HashSet;

// =============================================================================
// EMERGENCY STATE
// =============================================================================

/// Emergency state for the protocol
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmergencyState {
    /// Whether emergency is currently active
    pub active: bool,

    /// Block when emergency was declared
    pub declared_at: Option<BlockNumber>,

    /// What triggered the emergency
    pub trigger: Option<EmergencyTrigger>,

    /// When the emergency automatically expires
    pub expires_at: Option<BlockNumber>,

    /// Actions taken during this emergency
    pub actions_taken: Vec<EmergencyAction>,

    /// Validators who voted to declare emergency
    pub declaring_validators: HashSet<AccountId>,

    /// Total voting power that approved
    pub approval_power: u64,

    /// Total voting power in the system
    pub total_power: u64,
}

impl EmergencyState {
    /// Creates a new inactive emergency state
    pub fn new() -> Self {
        Self {
            active: false,
            declared_at: None,
            trigger: None,
            expires_at: None,
            actions_taken: Vec::new(),
            declaring_validators: HashSet::new(),
            approval_power: 0,
            total_power: 0,
        }
    }

    /// Check if emergency has expired
    pub fn is_expired(&self, current_block: BlockNumber) -> bool {
        match self.expires_at {
            Some(expires) => current_block >= expires,
            None => false,
        }
    }

    /// Get approval percentage (0-100)
    pub fn approval_percent(&self) -> u8 {
        if self.total_power == 0 {
            return 0;
        }
        ((self.approval_power * 100) / self.total_power) as u8
    }
}

impl Default for EmergencyState {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// EMERGENCY TRIGGERS
// =============================================================================

/// What can trigger an emergency state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmergencyTrigger {
    /// Validator vote (requires ≥75% approval)
    ValidatorVote {
        /// Approval percentage achieved
        approval_percent: u8,
        /// Number of validators who voted
        voter_count: u32,
    },

    /// Consensus safety violation detected
    ConsensusFailure {
        /// Description of the failure
        failure_type: ConsensusFailureType,
        /// Evidence hash
        evidence: Hash,
    },

    /// Critical bug verified
    CriticalBug {
        /// Bug report hash
        report_hash: Hash,
        /// Affected component
        component: String,
    },

    /// Multiple failure signals detected (≥2)
    MultipleSignals {
        /// List of detected signals
        signals: Vec<FailureSignal>,
    },

    /// Automatic trigger from circuit breaker
    CircuitBreakerTriggered {
        /// Which breaker triggered
        breaker_id: String,
    },
}

/// Types of consensus failures
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsensusFailureType {
    /// Finality stalled for too long
    FinalityStall { epochs_stalled: u64 },

    /// Conflicting finality claims detected
    ConflictingFinality { block_a: Hash, block_b: Hash },

    /// State root divergence across validators
    StateRootDivergence { expected: Hash, found: Hash },

    /// Insufficient validators online
    InsufficientValidators { online: u32, required: u32 },
}

// =============================================================================
// EMERGENCY ACTIONS
// =============================================================================

/// Actions that can be taken during an emergency
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmergencyAction {
    /// Pause block production (temporary, not permanent freeze)
    PauseBlockProduction {
        /// When the pause started
        started_at: BlockNumber,
        /// Maximum duration before auto-resume
        max_duration: BlockNumber,
    },

    /// Freeze governance (no new proposals)
    FreezeGovernance {
        /// Chains affected (empty = all)
        chains: Vec<ChainId>,
    },

    /// Tighten parameters within constitutional bounds
    TightenParameters {
        /// Parameter changes applied
        changes: Vec<ParameterTightening>,
    },

    /// Halt slashing temporarily
    HaltSlashing {
        /// Reason for halt
        reason: String,
    },

    /// Halt new sidechain creation
    HaltSidechainCreation,

    /// Extend all timelocks
    ExtendTimelocks {
        /// Multiplier for timelocks (e.g., 2 = double)
        multiplier: u8,
    },

    /// Increase quorum requirements
    IncreaseQuorum {
        /// New minimum quorum percent
        new_quorum: u8,
    },
}

/// Parameter tightening during emergency
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterTightening {
    /// Parameter name
    pub parameter: String,
    /// Original value
    pub original: u64,
    /// Tightened value
    pub tightened: u64,
}

// =============================================================================
// FAILURE SIGNALS
// =============================================================================

/// Failure signals that can be detected
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureSignal {
    /// Consensus has stalled
    ConsensusStall {
        /// Epochs without finality
        epochs_stalled: u64,
    },

    /// Mass validator offline
    ValidatorMassOffline {
        /// Number of validators offline
        offline_count: u32,
        /// Total validators
        total_validators: u32,
    },

    /// Governance quorum failures
    GovernanceQuorumFailure {
        /// Consecutive failed votes
        consecutive_failures: u8,
        /// Chain affected
        chain_id: ChainId,
    },

    /// Network partition detected
    NetworkPartition {
        /// Estimated number of partitions
        partition_count: u8,
        /// Our partition size
        our_partition_size: u32,
    },

    /// State root divergence
    StateRootDivergence {
        /// Chains with divergence
        affected_chains: Vec<ChainId>,
    },

    /// Economic attack detected
    EconomicAnomaly {
        /// Type of anomaly
        anomaly_type: String,
        /// Severity (1-10)
        severity: u8,
    },

    /// Unusual slashing activity
    SlashingSpike {
        /// Slashing events in window
        events_count: u32,
        /// Normal expected count
        expected_count: u32,
    },
}

impl FailureSignal {
    /// Get the severity of this signal (1-10)
    pub fn severity(&self) -> u8 {
        match self {
            FailureSignal::ConsensusStall { epochs_stalled } => {
                if *epochs_stalled > 10 { 10 }
                else if *epochs_stalled > 5 { 8 }
                else if *epochs_stalled > 2 { 5 }
                else { 3 }
            }
            FailureSignal::ValidatorMassOffline { offline_count, total_validators } => {
                let offline_percent = (*offline_count * 100) / (*total_validators).max(1);
                if offline_percent > 50 { 10 }
                else if offline_percent > 33 { 8 }
                else if offline_percent > 20 { 5 }
                else { 3 }
            }
            FailureSignal::GovernanceQuorumFailure { consecutive_failures, .. } => {
                if *consecutive_failures >= 5 { 7 }
                else if *consecutive_failures >= 3 { 5 }
                else { 3 }
            }
            FailureSignal::NetworkPartition { partition_count, .. } => {
                if *partition_count > 2 { 9 } else { 7 }
            }
            FailureSignal::StateRootDivergence { affected_chains } => {
                if affected_chains.len() > 3 { 10 }
                else if affected_chains.len() > 1 { 8 }
                else { 6 }
            }
            FailureSignal::EconomicAnomaly { severity, .. } => *severity,
            FailureSignal::SlashingSpike { events_count, expected_count } => {
                let ratio = *events_count / (*expected_count).max(1);
                if ratio > 10 { 8 }
                else if ratio > 5 { 6 }
                else { 4 }
            }
        }
    }
}

// =============================================================================
// CIRCUIT BREAKERS
// =============================================================================

/// Circuit breaker configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitBreaker {
    /// Unique identifier
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Condition that triggers this breaker
    pub condition: BreakerCondition,

    /// Action to take when triggered
    pub action: BreakerAction,

    /// How long the breaker stays active (in blocks)
    pub duration: BlockNumber,

    /// Whether this breaker is currently active
    pub is_active: bool,

    /// When it was triggered (if active)
    pub triggered_at: Option<BlockNumber>,

    /// Whether this breaker is enabled
    pub enabled: bool,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(
        id: String,
        name: String,
        condition: BreakerCondition,
        action: BreakerAction,
        duration: BlockNumber,
    ) -> Self {
        Self {
            id,
            name,
            condition,
            action,
            duration,
            is_active: false,
            triggered_at: None,
            enabled: true,
        }
    }

    /// Check if the breaker has expired
    pub fn is_expired(&self, current_block: BlockNumber) -> bool {
        match self.triggered_at {
            Some(triggered) => current_block >= triggered + self.duration,
            None => false,
        }
    }

    /// Trigger this breaker
    pub fn trigger(&mut self, current_block: BlockNumber) {
        self.is_active = true;
        self.triggered_at = Some(current_block);
    }

    /// Reset this breaker
    pub fn reset(&mut self) {
        self.is_active = false;
        self.triggered_at = None;
    }
}

/// Conditions that can trigger a circuit breaker
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakerCondition {
    /// Finality delayed for too many epochs
    FinalityDelay {
        /// Epochs without finality
        epochs_threshold: u64,
    },

    /// Validator participation below threshold
    ValidatorParticipation {
        /// Minimum percent required
        min_percent: u8,
    },

    /// State root mismatch detected
    StateRootMismatch,

    /// Network partition detected
    NetworkPartition {
        /// Minimum peers required
        min_peers: u32,
    },

    /// Too many slashing events
    SlashingSpike {
        /// Max events per epoch
        max_events_per_epoch: u32,
    },

    /// Governance deadlock
    GovernanceDeadlock {
        /// Consecutive failed proposals
        failed_proposals: u8,
    },
}

/// Actions a circuit breaker can take
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakerAction {
    /// Slow down block production
    SlowBlockTime {
        /// Factor to slow by (2 = half speed)
        factor: u8,
    },

    /// Increase confirmation depth
    IncreaseConfirmations {
        /// New confirmation depth
        new_depth: u64,
    },

    /// Suspend risky operations
    SuspendRiskyOperations {
        /// Operations to suspend
        operations: Vec<String>,
    },

    /// Extend all timelocks
    ExtendTimelocks {
        /// Multiplier for timelocks
        multiplier: u8,
    },

    /// Increase quorum requirements
    RaiseQuorum {
        /// Additional percent to add
        additional_percent: u8,
    },

    /// Trigger emergency state
    TriggerEmergency,
}

// =============================================================================
// RECOVERY
// =============================================================================

/// Recovery process state
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryProcess {
    /// Current step in recovery
    pub current_step: RecoveryStep,

    /// When recovery started
    pub started_at: BlockNumber,

    /// Checkpoint being recovered to
    pub target_checkpoint: Option<Checkpoint>,

    /// Validators who have confirmed recovery
    pub confirmations: HashSet<AccountId>,

    /// Required confirmations (67% = 2/3 supermajority per Constitution Article III)
    pub required_confirmations: u32,
}

/// Steps in the recovery process
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryStep {
    /// System halted, awaiting audit
    Halt,

    /// Auditing state (roots, proofs)
    StateAudit,

    /// Reconfirming validator set
    ValidatorReconfirmation,

    /// Deciding on fork (if needed)
    ForkDecision,

    /// Gradually restarting
    GradualRestart { phase: u8, max_phases: u8 },

    /// Recovery complete
    Complete,
}

/// Checkpoint for recovery
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// Block number of checkpoint
    pub block_number: BlockNumber,

    /// State root at checkpoint
    pub state_root: Hash,

    /// Validator set hash at checkpoint
    pub validator_set_hash: Hash,

    /// When checkpoint was created
    pub created_at: BlockNumber,

    /// Chain this checkpoint is for
    pub chain_id: ChainId,
}

// =============================================================================
// CONSTANTS
// =============================================================================

/// Minimum approval for emergency declaration (75%)
pub const EMERGENCY_APPROVAL_THRESHOLD: u8 = 75;

/// Maximum duration of emergency state (7 days in blocks, ~100,800 blocks at 6s)
pub const EMERGENCY_MAX_DURATION: BlockNumber = 100_800;

/// Minimum signals required for automatic emergency (2)
pub const MIN_SIGNALS_FOR_EMERGENCY: usize = 2;

/// Minimum severity sum for automatic emergency
pub const MIN_SEVERITY_FOR_EMERGENCY: u8 = 15;

/// Default circuit breaker duration (1 day in blocks)
pub const DEFAULT_BREAKER_DURATION: BlockNumber = 14_400;

/// Finality delay threshold for automatic breaker (epochs)
pub const FINALITY_DELAY_THRESHOLD: u64 = 3;

/// Minimum validator participation before breaker (percent)
pub const MIN_VALIDATOR_PARTICIPATION: u8 = 50;

/// Maximum slashing events per epoch before breaker
pub const MAX_SLASHING_PER_EPOCH: u32 = 10;

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
    fn test_emergency_state_new() {
        let state = EmergencyState::new();

        assert!(!state.active);
        assert!(state.declared_at.is_none());
        assert!(state.trigger.is_none());
        assert!(state.actions_taken.is_empty());
    }

    #[test]
    fn test_emergency_state_expiration() {
        let mut state = EmergencyState::new();
        state.active = true;
        state.declared_at = Some(1000);
        state.expires_at = Some(2000);

        assert!(!state.is_expired(1500));
        assert!(state.is_expired(2000));
        assert!(state.is_expired(2500));
    }

    #[test]
    fn test_emergency_approval_percent() {
        let mut state = EmergencyState::new();
        state.approval_power = 75;
        state.total_power = 100;

        assert_eq!(state.approval_percent(), 75);

        state.approval_power = 80;
        assert_eq!(state.approval_percent(), 80);

        state.total_power = 0;
        assert_eq!(state.approval_percent(), 0);
    }

    #[test]
    fn test_failure_signal_severity() {
        let stall = FailureSignal::ConsensusStall { epochs_stalled: 6 };
        assert_eq!(stall.severity(), 8);

        let offline = FailureSignal::ValidatorMassOffline {
            offline_count: 40,
            total_validators: 100,
        };
        assert_eq!(offline.severity(), 8);

        let partition = FailureSignal::NetworkPartition {
            partition_count: 3,
            our_partition_size: 30,
        };
        assert_eq!(partition.severity(), 9);
    }

    #[test]
    fn test_circuit_breaker_trigger() {
        let mut breaker = CircuitBreaker::new(
            "finality_delay".to_string(),
            "Finality Delay Breaker".to_string(),
            BreakerCondition::FinalityDelay { epochs_threshold: 3 },
            BreakerAction::SlowBlockTime { factor: 2 },
            14_400,
        );

        assert!(!breaker.is_active);

        breaker.trigger(1000);

        assert!(breaker.is_active);
        assert_eq!(breaker.triggered_at, Some(1000));

        assert!(!breaker.is_expired(5000));
        assert!(breaker.is_expired(20000));
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let mut breaker = CircuitBreaker::new(
            "test".to_string(),
            "Test Breaker".to_string(),
            BreakerCondition::StateRootMismatch,
            BreakerAction::TriggerEmergency,
            1000,
        );

        breaker.trigger(100);
        assert!(breaker.is_active);

        breaker.reset();
        assert!(!breaker.is_active);
        assert!(breaker.triggered_at.is_none());
    }

    #[test]
    fn test_emergency_trigger_variants() {
        let vote_trigger = EmergencyTrigger::ValidatorVote {
            approval_percent: 80,
            voter_count: 50,
        };

        assert!(matches!(vote_trigger, EmergencyTrigger::ValidatorVote { .. }));

        let consensus_trigger = EmergencyTrigger::ConsensusFailure {
            failure_type: ConsensusFailureType::FinalityStall { epochs_stalled: 5 },
            evidence: Hash::ZERO,
        };

        assert!(matches!(consensus_trigger, EmergencyTrigger::ConsensusFailure { .. }));
    }

    #[test]
    fn test_recovery_step_progression() {
        let process = RecoveryProcess {
            current_step: RecoveryStep::Halt,
            started_at: 1000,
            target_checkpoint: None,
            confirmations: HashSet::new(),
            required_confirmations: 34,
        };

        assert!(matches!(process.current_step, RecoveryStep::Halt));
    }

    #[test]
    fn test_checkpoint_creation() {
        let checkpoint = Checkpoint {
            block_number: 10000,
            state_root: Hash::hash(b"state"),
            validator_set_hash: Hash::hash(b"validators"),
            created_at: 10000,
            chain_id: ChainId(0),
        };

        assert_eq!(checkpoint.block_number, 10000);
        assert_eq!(checkpoint.chain_id, ChainId(0));
    }

    #[test]
    fn test_emergency_action_variants() {
        let pause = EmergencyAction::PauseBlockProduction {
            started_at: 1000,
            max_duration: 10000,
        };

        assert!(matches!(pause, EmergencyAction::PauseBlockProduction { .. }));

        let tighten = EmergencyAction::TightenParameters {
            changes: vec![
                ParameterTightening {
                    parameter: "quorum".to_string(),
                    original: 30,
                    tightened: 50,
                }
            ],
        };

        if let EmergencyAction::TightenParameters { changes } = tighten {
            assert_eq!(changes.len(), 1);
            assert_eq!(changes[0].parameter, "quorum");
        }
    }

    #[test]
    fn test_consensus_failure_types() {
        let stall = ConsensusFailureType::FinalityStall { epochs_stalled: 10 };
        assert!(matches!(stall, ConsensusFailureType::FinalityStall { .. }));

        let conflict = ConsensusFailureType::ConflictingFinality {
            block_a: Hash::hash(b"a"),
            block_b: Hash::hash(b"b"),
        };
        assert!(matches!(conflict, ConsensusFailureType::ConflictingFinality { .. }));

        let insufficient = ConsensusFailureType::InsufficientValidators {
            online: 10,
            required: 30,
        };
        if let ConsensusFailureType::InsufficientValidators { online, required } = insufficient {
            assert!(online < required);
        }
    }

    #[test]
    fn test_breaker_condition_variants() {
        let conditions = vec![
            BreakerCondition::FinalityDelay { epochs_threshold: 3 },
            BreakerCondition::ValidatorParticipation { min_percent: 50 },
            BreakerCondition::StateRootMismatch,
            BreakerCondition::NetworkPartition { min_peers: 5 },
            BreakerCondition::SlashingSpike { max_events_per_epoch: 10 },
            BreakerCondition::GovernanceDeadlock { failed_proposals: 3 },
        ];

        assert_eq!(conditions.len(), 6);
    }

    #[test]
    fn test_breaker_action_variants() {
        let actions = vec![
            BreakerAction::SlowBlockTime { factor: 2 },
            BreakerAction::IncreaseConfirmations { new_depth: 100 },
            BreakerAction::SuspendRiskyOperations { operations: vec!["stake".to_string()] },
            BreakerAction::ExtendTimelocks { multiplier: 2 },
            BreakerAction::RaiseQuorum { additional_percent: 10 },
            BreakerAction::TriggerEmergency,
        ];

        assert_eq!(actions.len(), 6);
    }

    #[test]
    fn test_constants() {
        assert_eq!(EMERGENCY_APPROVAL_THRESHOLD, 75);
        assert_eq!(EMERGENCY_MAX_DURATION, 100_800);
        assert_eq!(MIN_SIGNALS_FOR_EMERGENCY, 2);
        assert_eq!(MIN_SEVERITY_FOR_EMERGENCY, 15);
        assert_eq!(DEFAULT_BREAKER_DURATION, 14_400);
        assert_eq!(FINALITY_DELAY_THRESHOLD, 3);
        assert_eq!(MIN_VALIDATOR_PARTICIPATION, 50);
        assert_eq!(MAX_SLASHING_PER_EPOCH, 10);
    }
}
