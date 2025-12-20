// Protocol Parameters - SPEC v6 Constitutional Safeguards
// Runtime-governable parameters with constitutional bounds
//
// Principle: Parameters can change within bounds, but bounds are immutable.
// This ensures protocol evolution without violating core guarantees.

use crate::types::{Balance, BlockNumber, ChainId, Hash};
use serde::{Deserialize, Serialize};

// =============================================================================
// CONSTITUTIONAL BOUNDS (IMMUTABLE)
// =============================================================================

/// Minimum inflation rate (0%)
pub const MIN_INFLATION_RATE: u8 = 0;
/// Maximum inflation rate (5%)
pub const MAX_INFLATION_RATE: u8 = 5;

/// Minimum fee burn rate (0%)
pub const MIN_FEE_BURN_RATE: u8 = 0;
/// Maximum fee burn rate (100%)
pub const MAX_FEE_BURN_RATE: u8 = 100;

/// Minimum validator count (SPEC v2.1: 50 validators for decentralization)
pub const MIN_VALIDATORS: u32 = 50;
/// Maximum validator count
pub const MAX_VALIDATORS: u32 = 101;

/// Minimum standard timelock (1 day = 14,400 blocks)
pub const MIN_STANDARD_TIMELOCK: BlockNumber = 14_400;
/// Maximum standard timelock (30 days = 432,000 blocks)
pub const MAX_STANDARD_TIMELOCK: BlockNumber = 432_000;

/// Minimum exit timelock (7 days = 100,800 blocks)
pub const MIN_EXIT_TIMELOCK: BlockNumber = 100_800;
/// Maximum exit timelock (90 days = 1,296,000 blocks)
pub const MAX_EXIT_TIMELOCK: BlockNumber = 1_296_000;

/// Minimum quorum (10%)
pub const MIN_QUORUM_BOUND: u8 = 10;
/// Maximum quorum (80%)
pub const MAX_QUORUM_BOUND: u8 = 80;

/// Minimum supermajority threshold (66% ≈ 2/3 constitutional minimum)
/// CONSTITUTIONAL BOUND: Cannot be lowered below 2/3 per Article III
pub const MIN_SUPERMAJORITY: u8 = 66;
/// Maximum supermajority threshold (90%)
pub const MAX_SUPERMAJORITY: u8 = 90;

/// Minimum voting period (1 day)
pub const MIN_VOTING_PERIOD: BlockNumber = 14_400;
/// Maximum voting period (30 days)
pub const MAX_VOTING_PERIOD: BlockNumber = 432_000;

// =============================================================================
// BOUNDED VALUE TYPES
// =============================================================================

/// A value bounded within constitutional limits
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedValue<T> {
    value: T,
    min: T,
    max: T,
}

impl<T: Copy + Ord> BoundedValue<T> {
    /// Create a new bounded value, clamping to bounds
    pub fn new(value: T, min: T, max: T) -> Self {
        let clamped = if value < min {
            min
        } else if value > max {
            max
        } else {
            value
        };
        Self {
            value: clamped,
            min,
            max,
        }
    }

    /// Get the current value
    pub fn value(&self) -> T {
        self.value
    }

    /// Get the minimum bound
    pub fn min(&self) -> T {
        self.min
    }

    /// Get the maximum bound
    pub fn max(&self) -> T {
        self.max
    }

    /// Try to set a new value, returns false if out of bounds
    pub fn try_set(&mut self, new_value: T) -> bool {
        if new_value >= self.min && new_value <= self.max {
            self.value = new_value;
            true
        } else {
            false
        }
    }

    /// Check if a value would be valid
    pub fn is_valid(&self, value: T) -> bool {
        value >= self.min && value <= self.max
    }
}

// =============================================================================
// PROTOCOL PARAMETERS
// =============================================================================

/// Economics parameters - SPEC v2.1 bounds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicsParameters {
    /// Annual inflation rate (0-5%)
    pub inflation_rate: BoundedValue<u8>,

    /// Fee burn percentage (0-100%)
    pub fee_burn_rate: BoundedValue<u8>,

    /// Fee to validators percentage (remainder after burn)
    pub fee_validator_rate: BoundedValue<u8>,

    /// Minimum stake to become validator
    pub min_validator_stake: Balance,

    /// Stake cap for VRF weighting
    pub stake_cap: Balance,
}

impl Default for EconomicsParameters {
    fn default() -> Self {
        Self {
            inflation_rate: BoundedValue::new(2, MIN_INFLATION_RATE, MAX_INFLATION_RATE),
            fee_burn_rate: BoundedValue::new(50, MIN_FEE_BURN_RATE, MAX_FEE_BURN_RATE),
            fee_validator_rate: BoundedValue::new(50, MIN_FEE_BURN_RATE, MAX_FEE_BURN_RATE),
            min_validator_stake: 10_000,
            stake_cap: 1_000_000,
        }
    }
}

/// Consensus parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusParameters {
    /// Target number of validators
    pub target_validators: BoundedValue<u32>,

    /// Blocks per epoch
    pub blocks_per_epoch: BlockNumber,

    /// Slots per epoch
    pub slots_per_epoch: u64,

    /// VC decay rate per quarter (percentage)
    pub vc_decay_rate: u8,
}

impl Default for ConsensusParameters {
    fn default() -> Self {
        Self {
            target_validators: BoundedValue::new(51, MIN_VALIDATORS, MAX_VALIDATORS),
            blocks_per_epoch: 14_400, // ~1 day
            slots_per_epoch: 14_400,
            vc_decay_rate: 10, // 10% per quarter
        }
    }
}

/// Governance parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceParameters {
    /// Standard proposal timelock
    pub standard_timelock: BoundedValue<BlockNumber>,

    /// Exit proposal timelock (must be >= standard)
    pub exit_timelock: BoundedValue<BlockNumber>,

    /// Voting period duration
    pub voting_period: BoundedValue<BlockNumber>,

    /// Minimum quorum percentage
    pub min_quorum: BoundedValue<u8>,

    /// Supermajority threshold for exit votes
    pub supermajority_threshold: BoundedValue<u8>,

    /// Standard threshold for regular votes
    pub standard_threshold: u8,

    /// Grace period after voting ends
    pub grace_period: BlockNumber,

    /// Proposal deposit amount
    pub proposal_deposit: Balance,
}

impl Default for GovernanceParameters {
    fn default() -> Self {
        Self {
            standard_timelock: BoundedValue::new(172_800, MIN_STANDARD_TIMELOCK, MAX_STANDARD_TIMELOCK),
            exit_timelock: BoundedValue::new(432_000, MIN_EXIT_TIMELOCK, MAX_EXIT_TIMELOCK),
            voting_period: BoundedValue::new(100_800, MIN_VOTING_PERIOD, MAX_VOTING_PERIOD),
            min_quorum: BoundedValue::new(30, MIN_QUORUM_BOUND, MAX_QUORUM_BOUND),
            supermajority_threshold: BoundedValue::new(66, MIN_SUPERMAJORITY, MAX_SUPERMAJORITY),
            standard_threshold: 50,
            grace_period: 28_800, // 2 days
            proposal_deposit: 100,
        }
    }
}

/// Identity parameters - SPEC v4 bounds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityParameters {
    /// Identity deposit amount
    pub identity_deposit: Balance,

    /// Identity expiry period
    pub identity_expiry: BlockNumber,

    /// Minimum attestations for activation
    pub min_attestations: u8,

    /// Attestation expiry period
    pub attestation_expiry: BlockNumber,

    /// Maximum reputation score
    pub max_reputation: u32,

    /// Reputation decay interval
    pub reputation_decay_interval: BlockNumber,

    /// Reputation decay percentage per interval
    pub reputation_decay_percent: u8,
}

impl Default for IdentityParameters {
    fn default() -> Self {
        Self {
            identity_deposit: 10,
            identity_expiry: 2_592_000, // 180 days
            min_attestations: 3,
            attestation_expiry: 1_296_000, // 90 days
            max_reputation: 10_000,
            reputation_decay_interval: 100_800, // 7 days
            reputation_decay_percent: 1,
        }
    }
}

/// Sidechain parameters - SPEC v3 bounds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidechainParameters {
    /// Purge withdrawal window
    pub purge_withdrawal_window: BlockNumber,

    /// Maximum governance failures before purge
    pub max_governance_failures: u8,

    /// Inactivity threshold for purge trigger
    pub inactivity_purge_threshold: BlockNumber,

    /// Fraud percentage threshold for purge
    pub fraud_purge_threshold: u8,
}

impl Default for SidechainParameters {
    fn default() -> Self {
        Self {
            purge_withdrawal_window: 432_000, // 30 days
            max_governance_failures: 3,
            // SPEC v3.1 Section 2.1: 90 days of inactivity before purge trigger
            // 90 days = 90 * 24 * 3600 / 6 = 1,296,000 blocks
            inactivity_purge_threshold: 1_296_000, // 90 days (SPEC v3.1 compliant)
            fraud_purge_threshold: 33, // 33% validators fraudulent
        }
    }
}

// =============================================================================
// COMPLETE PROTOCOL PARAMETERS
// =============================================================================

/// Complete protocol parameters with all subsystems
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolParameters {
    /// Protocol version
    pub version: ProtocolVersion,

    /// Economics parameters
    pub economics: EconomicsParameters,

    /// Consensus parameters
    pub consensus: ConsensusParameters,

    /// Governance parameters
    pub governance: GovernanceParameters,

    /// Identity parameters
    pub identity: IdentityParameters,

    /// Sidechain parameters
    pub sidechains: SidechainParameters,

    /// Block number when these parameters became active
    pub active_since: BlockNumber,

    /// Optional expiry (for transitional parameters)
    pub expires_at: Option<BlockNumber>,
}

impl Default for ProtocolParameters {
    fn default() -> Self {
        Self {
            version: ProtocolVersion::default(),
            economics: EconomicsParameters::default(),
            consensus: ConsensusParameters::default(),
            governance: GovernanceParameters::default(),
            identity: IdentityParameters::default(),
            sidechains: SidechainParameters::default(),
            active_since: 0,
            expires_at: None,
        }
    }
}

impl ProtocolParameters {
    /// Create genesis parameters
    pub fn genesis() -> Self {
        Self::default()
    }

    /// Check if parameters are currently active
    pub fn is_active(&self, current_block: BlockNumber) -> bool {
        current_block >= self.active_since
            && self.expires_at.map_or(true, |exp| current_block < exp)
    }

    /// Validate that exit timelock >= standard timelock
    pub fn validate_timelocks(&self) -> bool {
        self.governance.exit_timelock.value() >= self.governance.standard_timelock.value()
    }

    /// Validate all constitutional constraints
    pub fn validate(&self) -> Result<(), ParameterError> {
        // Exit timelock must be >= standard timelock
        if self.governance.exit_timelock.value() < self.governance.standard_timelock.value() {
            return Err(ParameterError::ExitTimelockTooShort);
        }

        // Fee rates must sum to <= 100%
        let total_fees = self.economics.fee_burn_rate.value() as u16
            + self.economics.fee_validator_rate.value() as u16;
        if total_fees > 100 {
            return Err(ParameterError::FeeRatesTooHigh);
        }

        Ok(())
    }
}

// =============================================================================
// PROTOCOL VERSION
// =============================================================================

/// Protocol version for upgrade tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Major version (breaking changes)
    pub major: u16,
    /// Minor version (new features)
    pub minor: u16,
    /// Patch version (bug fixes)
    pub patch: u16,
}

impl Default for ProtocolVersion {
    fn default() -> Self {
        Self {
            major: 1,
            minor: 0,
            patch: 0,
        }
    }
}

impl ProtocolVersion {
    /// Create a new protocol version
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self { major, minor, patch }
    }

    /// Check if this version is compatible with another
    pub fn is_compatible(&self, other: &Self) -> bool {
        // Same major version = compatible
        self.major == other.major
    }

    /// Check if this version is newer than another
    pub fn is_newer(&self, other: &Self) -> bool {
        if self.major != other.major {
            return self.major > other.major;
        }
        if self.minor != other.minor {
            return self.minor > other.minor;
        }
        self.patch > other.patch
    }
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// =============================================================================
// PARAMETER CHANGE PROPOSAL
// =============================================================================

/// Types of parameter changes that can be proposed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterChange {
    /// Change inflation rate
    InflationRate(u8),

    /// Change fee burn rate
    FeeBurnRate(u8),

    /// Change target validator count
    TargetValidators(u32),

    /// Change standard timelock
    StandardTimelock(BlockNumber),

    /// Change exit timelock
    ExitTimelock(BlockNumber),

    /// Change voting period
    VotingPeriod(BlockNumber),

    /// Change minimum quorum
    MinQuorum(u8),

    /// Change supermajority threshold
    SupermajorityThreshold(u8),

    /// Change proposal deposit
    ProposalDeposit(Balance),

    /// Change VC decay rate
    VcDecayRate(u8),

    /// Batch of changes (applied atomically)
    Batch(Vec<ParameterChange>),
}

impl ParameterChange {
    /// Check if this change is within constitutional bounds
    pub fn is_constitutional(&self, current: &ProtocolParameters) -> bool {
        match self {
            ParameterChange::InflationRate(v) => {
                *v >= MIN_INFLATION_RATE && *v <= MAX_INFLATION_RATE
            }
            ParameterChange::FeeBurnRate(v) => {
                *v >= MIN_FEE_BURN_RATE && *v <= MAX_FEE_BURN_RATE
            }
            ParameterChange::TargetValidators(v) => {
                *v >= MIN_VALIDATORS && *v <= MAX_VALIDATORS
            }
            ParameterChange::StandardTimelock(v) => {
                *v >= MIN_STANDARD_TIMELOCK && *v <= MAX_STANDARD_TIMELOCK
            }
            ParameterChange::ExitTimelock(v) => {
                *v >= MIN_EXIT_TIMELOCK
                    && *v <= MAX_EXIT_TIMELOCK
                    && *v >= current.governance.standard_timelock.value()
            }
            ParameterChange::VotingPeriod(v) => {
                *v >= MIN_VOTING_PERIOD && *v <= MAX_VOTING_PERIOD
            }
            ParameterChange::MinQuorum(v) => {
                *v >= MIN_QUORUM_BOUND && *v <= MAX_QUORUM_BOUND
            }
            ParameterChange::SupermajorityThreshold(v) => {
                *v >= MIN_SUPERMAJORITY && *v <= MAX_SUPERMAJORITY
            }
            ParameterChange::ProposalDeposit(_) => true, // No constitutional bound
            ParameterChange::VcDecayRate(v) => *v <= 50, // Max 50% decay per quarter
            ParameterChange::Batch(changes) => {
                changes.iter().all(|c| c.is_constitutional(current))
            }
        }
    }

    /// Apply this change to protocol parameters
    pub fn apply(&self, params: &mut ProtocolParameters) -> Result<(), ParameterError> {
        match self {
            ParameterChange::InflationRate(v) => {
                if !params.economics.inflation_rate.try_set(*v) {
                    return Err(ParameterError::OutOfBounds);
                }
            }
            ParameterChange::FeeBurnRate(v) => {
                if !params.economics.fee_burn_rate.try_set(*v) {
                    return Err(ParameterError::OutOfBounds);
                }
            }
            ParameterChange::TargetValidators(v) => {
                if !params.consensus.target_validators.try_set(*v) {
                    return Err(ParameterError::OutOfBounds);
                }
            }
            ParameterChange::StandardTimelock(v) => {
                if !params.governance.standard_timelock.try_set(*v) {
                    return Err(ParameterError::OutOfBounds);
                }
            }
            ParameterChange::ExitTimelock(v) => {
                if !params.governance.exit_timelock.try_set(*v) {
                    return Err(ParameterError::OutOfBounds);
                }
            }
            ParameterChange::VotingPeriod(v) => {
                if !params.governance.voting_period.try_set(*v) {
                    return Err(ParameterError::OutOfBounds);
                }
            }
            ParameterChange::MinQuorum(v) => {
                if !params.governance.min_quorum.try_set(*v) {
                    return Err(ParameterError::OutOfBounds);
                }
            }
            ParameterChange::SupermajorityThreshold(v) => {
                if !params.governance.supermajority_threshold.try_set(*v) {
                    return Err(ParameterError::OutOfBounds);
                }
            }
            ParameterChange::ProposalDeposit(v) => {
                params.governance.proposal_deposit = *v;
            }
            ParameterChange::VcDecayRate(v) => {
                if *v > 50 {
                    return Err(ParameterError::OutOfBounds);
                }
                params.consensus.vc_decay_rate = *v;
            }
            ParameterChange::Batch(changes) => {
                for change in changes {
                    change.apply(params)?;
                }
            }
        }

        // Validate after applying
        params.validate()
    }
}

// =============================================================================
// CONSTITUTIONAL AXIOMS
// =============================================================================

/// Constitutional axioms that can never be violated
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstitutionalAxiom {
    /// Exit is always possible
    ExitAlwaysPossible,
    /// No global authority exists
    NoGlobalAuthority,
    /// Failure is local, not systemic
    FailureIsLocal,
    /// Identity is optional
    IdentityOptional,
    /// Power accumulates slowly
    PowerAccumulatesSlowly,
    /// No layer is mandatory
    NoMandatoryLayer,
    /// Forking is legitimate
    ForkingLegitimate,
}

impl ConstitutionalAxiom {
    /// Get all axioms
    pub fn all() -> &'static [ConstitutionalAxiom] {
        &[
            ConstitutionalAxiom::ExitAlwaysPossible,
            ConstitutionalAxiom::NoGlobalAuthority,
            ConstitutionalAxiom::FailureIsLocal,
            ConstitutionalAxiom::IdentityOptional,
            ConstitutionalAxiom::PowerAccumulatesSlowly,
            ConstitutionalAxiom::NoMandatoryLayer,
            ConstitutionalAxiom::ForkingLegitimate,
        ]
    }

    /// Description of the axiom
    pub fn description(&self) -> &'static str {
        match self {
            ConstitutionalAxiom::ExitAlwaysPossible => "Exit is always possible",
            ConstitutionalAxiom::NoGlobalAuthority => "No global authority exists",
            ConstitutionalAxiom::FailureIsLocal => "Failure is local, not systemic",
            ConstitutionalAxiom::IdentityOptional => "Identity is optional",
            ConstitutionalAxiom::PowerAccumulatesSlowly => "Power accumulates slowly",
            ConstitutionalAxiom::NoMandatoryLayer => "No layer is mandatory",
            ConstitutionalAxiom::ForkingLegitimate => "Forking is legitimate",
        }
    }
}

/// Constitutional prohibitions - changes that are ALWAYS rejected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstitutionalProhibition {
    /// Mandatory identity requirements
    MandatoryIdentity,
    /// Removal of exit/purge mechanisms
    RemoveExitMechanism,
    /// Global governance authority
    GlobalAuthority,
    /// Permanent validator privileges
    PermanentPrivileges,
    /// Forced federation
    ForcedFederation,
    /// Confiscation without exit window
    ConfiscationWithoutExit,
}

impl ConstitutionalProhibition {
    /// Get all prohibitions
    pub fn all() -> &'static [ConstitutionalProhibition] {
        &[
            ConstitutionalProhibition::MandatoryIdentity,
            ConstitutionalProhibition::RemoveExitMechanism,
            ConstitutionalProhibition::GlobalAuthority,
            ConstitutionalProhibition::PermanentPrivileges,
            ConstitutionalProhibition::ForcedFederation,
            ConstitutionalProhibition::ConfiscationWithoutExit,
        ]
    }

    /// Description of the prohibition
    pub fn description(&self) -> &'static str {
        match self {
            ConstitutionalProhibition::MandatoryIdentity => "Mandatory identity requirements",
            ConstitutionalProhibition::RemoveExitMechanism => "Removal of exit/purge mechanisms",
            ConstitutionalProhibition::GlobalAuthority => "Global governance authority",
            ConstitutionalProhibition::PermanentPrivileges => "Permanent validator privileges",
            ConstitutionalProhibition::ForcedFederation => "Forced federation",
            ConstitutionalProhibition::ConfiscationWithoutExit => "Confiscation without exit window",
        }
    }
}

// =============================================================================
// ERRORS
// =============================================================================

/// Parameter-related errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParameterError {
    /// Value out of constitutional bounds
    OutOfBounds,
    /// Exit timelock must be >= standard timelock
    ExitTimelockTooShort,
    /// Fee rates sum to more than 100%
    FeeRatesTooHigh,
    /// Change violates constitutional axiom
    ConstitutionalViolation(ConstitutionalAxiom),
    /// Change matches constitutional prohibition
    ProhibitedChange(ConstitutionalProhibition),
    /// Invalid parameter combination
    InvalidCombination,
    /// Version incompatible
    IncompatibleVersion,
}

// =============================================================================
// PARAMETER EVENTS
// =============================================================================

/// Events emitted for parameter changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterEvent {
    /// Parameters were updated
    ParametersUpdated {
        version: ProtocolVersion,
        changes: Vec<ParameterChange>,
        effective_block: BlockNumber,
    },

    /// Parameter change was rejected
    ChangeRejected {
        change: ParameterChange,
        reason: String,
    },

    /// Constitutional violation attempted
    ConstitutionalViolationAttempted {
        axiom: String,
        proposal_id: Hash,
    },
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounded_value_creation() {
        let bv = BoundedValue::new(50u8, 0, 100);
        assert_eq!(bv.value(), 50);
        assert_eq!(bv.min(), 0);
        assert_eq!(bv.max(), 100);
    }

    #[test]
    fn test_bounded_value_clamping() {
        // Below min
        let bv = BoundedValue::new(0u8, 10, 100);
        assert_eq!(bv.value(), 10);

        // Above max
        let bv = BoundedValue::new(200u8, 10, 100);
        assert_eq!(bv.value(), 100);
    }

    #[test]
    fn test_bounded_value_try_set() {
        let mut bv = BoundedValue::new(50u8, 0, 100);

        // Valid set
        assert!(bv.try_set(75));
        assert_eq!(bv.value(), 75);

        // Invalid set (above max)
        assert!(!bv.try_set(150));
        assert_eq!(bv.value(), 75); // Unchanged
    }

    #[test]
    fn test_default_parameters() {
        let params = ProtocolParameters::default();

        // Check economics defaults
        assert_eq!(params.economics.inflation_rate.value(), 2);
        assert_eq!(params.economics.fee_burn_rate.value(), 50);

        // Check governance defaults
        assert_eq!(params.governance.standard_timelock.value(), 172_800);
        assert_eq!(params.governance.exit_timelock.value(), 432_000);
        assert_eq!(params.governance.supermajority_threshold.value(), 66);
    }

    #[test]
    fn test_parameter_validation() {
        let params = ProtocolParameters::default();
        assert!(params.validate().is_ok());
    }

    #[test]
    fn test_exit_timelock_validation() {
        let mut params = ProtocolParameters::default();

        // Exit timelock less than standard should fail
        params.governance.exit_timelock = BoundedValue::new(
            100_000,
            MIN_EXIT_TIMELOCK,
            MAX_EXIT_TIMELOCK,
        );
        params.governance.standard_timelock = BoundedValue::new(
            200_000,
            MIN_STANDARD_TIMELOCK,
            MAX_STANDARD_TIMELOCK,
        );

        assert_eq!(
            params.validate(),
            Err(ParameterError::ExitTimelockTooShort)
        );
    }

    #[test]
    fn test_parameter_change_constitutional() {
        let params = ProtocolParameters::default();

        // Valid changes
        assert!(ParameterChange::InflationRate(3).is_constitutional(&params));
        assert!(ParameterChange::TargetValidators(51).is_constitutional(&params));

        // Invalid changes (out of bounds)
        assert!(!ParameterChange::InflationRate(10).is_constitutional(&params));
        assert!(!ParameterChange::TargetValidators(10).is_constitutional(&params));
    }

    #[test]
    fn test_parameter_change_apply() {
        let mut params = ProtocolParameters::default();

        // Apply valid change
        let change = ParameterChange::InflationRate(3);
        assert!(change.apply(&mut params).is_ok());
        assert_eq!(params.economics.inflation_rate.value(), 3);
    }

    #[test]
    fn test_batch_parameter_change() {
        let mut params = ProtocolParameters::default();

        // Note: fee_burn + fee_validator must sum to <= 100%
        // Default is 50/50, so we change burn to 40 (40+50=90, valid)
        let batch = ParameterChange::Batch(vec![
            ParameterChange::InflationRate(3),
            ParameterChange::FeeBurnRate(40),
            ParameterChange::TargetValidators(61),
        ]);

        assert!(batch.is_constitutional(&params));
        assert!(batch.apply(&mut params).is_ok());

        assert_eq!(params.economics.inflation_rate.value(), 3);
        assert_eq!(params.economics.fee_burn_rate.value(), 40);
        assert_eq!(params.consensus.target_validators.value(), 61);
    }

    #[test]
    fn test_protocol_version() {
        let v1 = ProtocolVersion::new(1, 0, 0);
        let v2 = ProtocolVersion::new(1, 1, 0);
        let v3 = ProtocolVersion::new(2, 0, 0);

        assert!(v1.is_compatible(&v2));
        assert!(!v1.is_compatible(&v3));
        assert!(v2.is_newer(&v1));
        assert!(v3.is_newer(&v2));
    }

    #[test]
    fn test_constitutional_axioms() {
        let axioms = ConstitutionalAxiom::all();
        assert_eq!(axioms.len(), 7);

        // Verify all have descriptions
        for axiom in axioms {
            assert!(!axiom.description().is_empty());
        }
    }

    #[test]
    fn test_constitutional_prohibitions() {
        let prohibitions = ConstitutionalProhibition::all();
        assert_eq!(prohibitions.len(), 6);

        // Verify all have descriptions
        for prohibition in prohibitions {
            assert!(!prohibition.description().is_empty());
        }
    }

    #[test]
    fn test_parameters_active() {
        let mut params = ProtocolParameters::default();
        params.active_since = 100;
        params.expires_at = Some(1000);

        assert!(!params.is_active(50));   // Before active
        assert!(params.is_active(100));   // At active
        assert!(params.is_active(500));   // During active
        assert!(!params.is_active(1000)); // At expiry
        assert!(!params.is_active(1500)); // After expiry
    }

    #[test]
    fn test_inflation_bounds() {
        assert!(MIN_INFLATION_RATE == 0);
        assert!(MAX_INFLATION_RATE == 5);

        // Cannot create inflation > 5%
        let change = ParameterChange::InflationRate(6);
        let params = ProtocolParameters::default();
        assert!(!change.is_constitutional(&params));
    }

    #[test]
    fn test_validator_count_bounds() {
        assert!(MIN_VALIDATORS == 50);
        assert!(MAX_VALIDATORS == 101);

        // Cannot go below 50 (SPEC v2.1 requirement for decentralization)
        let change = ParameterChange::TargetValidators(49);
        let params = ProtocolParameters::default();
        assert!(!change.is_constitutional(&params));
    }
}
