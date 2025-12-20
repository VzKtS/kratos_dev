// Security Types - SPEC v9: Security Model, Adversary Assumptions & Failure Modes
// Principle: A protocol that cannot describe its enemies is not finished
//
// This module defines:
// - Adversary classes (economic, social, technical, institutional, temporal, emergent)
// - Threat surfaces per layer (consensus, governance, identity, sidechain, emergency, fork)
// - Slow capture detection
// - Terminal failure modes
// - Protocol invariants
// - Security audit framework

use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash as CryptoHash};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

// =============================================================================
// ADVERSARY MODEL (SPEC v9 Section 2)
// =============================================================================

/// Classes of adversaries the protocol must defend against
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdversaryClass {
    /// Stake-based cartel, rent-seeking behavior
    Economic,

    /// Ideological capture, mob governance
    Social,

    /// Protocol abuse, bug exploitation
    Technical,

    /// Regulatory or state capture
    Institutional,

    /// Long-term slow capture over decades
    Temporal,

    /// Unforeseen coordination effects
    Emergent,
}

impl AdversaryClass {
    /// Get description of this adversary class
    pub fn description(&self) -> &'static str {
        match self {
            AdversaryClass::Economic => "Stake-based cartel, rent-seeking behavior",
            AdversaryClass::Social => "Ideological capture, mob governance",
            AdversaryClass::Technical => "Protocol abuse, bug exploitation",
            AdversaryClass::Institutional => "Regulatory or state capture",
            AdversaryClass::Temporal => "Long-term slow capture over decades",
            AdversaryClass::Emergent => "Unforeseen coordination effects",
        }
    }

    /// Get all adversary classes
    pub fn all() -> Vec<AdversaryClass> {
        vec![
            AdversaryClass::Economic,
            AdversaryClass::Social,
            AdversaryClass::Technical,
            AdversaryClass::Institutional,
            AdversaryClass::Temporal,
            AdversaryClass::Emergent,
        ]
    }
}

/// Actor rationality assumption
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorRationality {
    /// Acts to maximize self-interest
    Rational,

    /// Acts without clear strategy
    Irrational,

    /// Acts to harm regardless of cost
    Malicious,
}

// =============================================================================
// NON-ASSUMPTIONS (SPEC v9 Section 3)
// =============================================================================

/// Explicit non-assumptions - things KratOs does NOT assume
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NonAssumption {
    /// Protocol does NOT assume honest majority
    HonestMajority,

    /// Protocol does NOT assume benevolent governance
    BenevolentGovernance,

    /// Protocol does NOT assume aligned incentives
    AlignedIncentives,

    /// Protocol does NOT assume stable ideology
    StableIdeology,

    /// Protocol does NOT assume permanent participation
    PermanentParticipation,
}

impl NonAssumption {
    /// All non-assumptions that defenses must work without
    pub fn all() -> Vec<NonAssumption> {
        vec![
            NonAssumption::HonestMajority,
            NonAssumption::BenevolentGovernance,
            NonAssumption::AlignedIncentives,
            NonAssumption::StableIdeology,
            NonAssumption::PermanentParticipation,
        ]
    }

    pub fn description(&self) -> &'static str {
        match self {
            NonAssumption::HonestMajority => "Defenses work even without honest majority",
            NonAssumption::BenevolentGovernance => "Defenses work even with hostile governance",
            NonAssumption::AlignedIncentives => "Defenses work even when incentives diverge",
            NonAssumption::StableIdeology => "Defenses work even when ideology shifts",
            NonAssumption::PermanentParticipation => "Defenses work even with mass exit",
        }
    }
}

// =============================================================================
// THREAT SURFACES (SPEC v9 Section 4)
// =============================================================================

/// Layer of the protocol being threatened
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThreatLayer {
    /// Consensus mechanism threats
    Consensus,

    /// Governance system threats
    Governance,

    /// Identity and reputation threats
    IdentityReputation,

    /// Sidechain ecosystem threats
    Sidechain,

    /// Emergency power threats
    Emergency,

    /// Forking and ossification threats
    ForkingOssification,
}

/// Specific threat types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ThreatType {
    // Consensus Layer (4.1)
    ValidatorCartelization,
    LongRangeAttack,
    VrfGrinding,

    // Governance Layer (4.2)
    ProposalSpam,
    VoterApathy,
    MajorityTyranny,

    // Identity & Reputation (4.3)
    SybilAccumulation,
    ReputationFarming,
    CrossChainLaundering,

    // Sidechains (4.4)
    ZombieChains,
    GovernanceCapture,
    FederationAbuse,

    // Emergency Powers (4.5)
    EmergencyAbuse,
    PermanentExceptionalState,

    // Forking & Ossification (4.6)
    ForkSpam,
    OssificationAbuse,
    MinorityLockOut,
}

impl ThreatType {
    /// Get the layer this threat affects
    pub fn layer(&self) -> ThreatLayer {
        match self {
            ThreatType::ValidatorCartelization
            | ThreatType::LongRangeAttack
            | ThreatType::VrfGrinding => ThreatLayer::Consensus,

            ThreatType::ProposalSpam
            | ThreatType::VoterApathy
            | ThreatType::MajorityTyranny => ThreatLayer::Governance,

            ThreatType::SybilAccumulation
            | ThreatType::ReputationFarming
            | ThreatType::CrossChainLaundering => ThreatLayer::IdentityReputation,

            ThreatType::ZombieChains
            | ThreatType::GovernanceCapture
            | ThreatType::FederationAbuse => ThreatLayer::Sidechain,

            ThreatType::EmergencyAbuse
            | ThreatType::PermanentExceptionalState => ThreatLayer::Emergency,

            ThreatType::ForkSpam
            | ThreatType::OssificationAbuse
            | ThreatType::MinorityLockOut => ThreatLayer::ForkingOssification,
        }
    }

    /// Get mitigations for this threat
    pub fn mitigations(&self) -> Vec<Mitigation> {
        match self {
            ThreatType::ValidatorCartelization => vec![
                Mitigation::VcDecay,
                Mitigation::StakeCaps,
                Mitigation::ForkNeutrality,
            ],
            ThreatType::LongRangeAttack => vec![
                Mitigation::VcDecay,
                Mitigation::EmergencySlowdown,
            ],
            ThreatType::VrfGrinding => vec![
                Mitigation::VcDecay,
                Mitigation::StakeCaps,
            ],
            ThreatType::ProposalSpam => vec![
                Mitigation::QuorumBounds,
                Mitigation::Timelocks,
            ],
            ThreatType::VoterApathy => vec![
                Mitigation::QuorumBounds,
                Mitigation::ReputationDecay,
            ],
            ThreatType::MajorityTyranny => vec![
                Mitigation::ExitGuarantee,
                Mitigation::ForkGuarantee,
                Mitigation::Timelocks,
            ],
            ThreatType::SybilAccumulation => vec![
                Mitigation::ReputationDecay,
                Mitigation::ScopedIdentity,
                Mitigation::SlashingViaArbitration,
            ],
            ThreatType::ReputationFarming => vec![
                Mitigation::ReputationDecay,
                Mitigation::ImportDiscount,
            ],
            ThreatType::CrossChainLaundering => vec![
                Mitigation::ImportDiscount,
                Mitigation::SlashingViaArbitration,
            ],
            ThreatType::ZombieChains => vec![
                Mitigation::SidechainPurge,
                Mitigation::ScopedSecurity,
            ],
            ThreatType::GovernanceCapture => vec![
                Mitigation::IndependenceDefault,
                Mitigation::RevocableTreaties,
            ],
            ThreatType::FederationAbuse => vec![
                Mitigation::ScopedSecurity,
                Mitigation::RevocableTreaties,
            ],
            ThreatType::EmergencyAbuse => vec![
                Mitigation::HardDurationCaps,
                Mitigation::Cooldowns,
                Mitigation::ConstitutionalProhibitions,
            ],
            ThreatType::PermanentExceptionalState => vec![
                Mitigation::HardDurationCaps,
                Mitigation::ForkAvailability,
                Mitigation::ConstitutionalProhibitions,
            ],
            ThreatType::ForkSpam => vec![
                Mitigation::HighThresholds,
                Mitigation::PostForkDecay,
            ],
            ThreatType::OssificationAbuse => vec![
                Mitigation::NeutralForkRules,
                Mitigation::SurvivalForkException,
            ],
            ThreatType::MinorityLockOut => vec![
                Mitigation::NeutralForkRules,
                Mitigation::ExitGuarantee,
            ],
        }
    }
}

/// Protocol mitigations against threats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mitigation {
    // Consensus mitigations
    VcDecay,
    StakeCaps,
    ForkNeutrality,
    EmergencySlowdown,

    // Governance mitigations
    QuorumBounds,
    Timelocks,
    ReputationDecay,
    ExitGuarantee,
    ForkGuarantee,

    // Identity mitigations
    ScopedIdentity,
    ImportDiscount,
    SlashingViaArbitration,

    // Sidechain mitigations
    SidechainPurge,
    IndependenceDefault,
    ScopedSecurity,
    RevocableTreaties,

    // Emergency mitigations
    HardDurationCaps,
    Cooldowns,
    ConstitutionalProhibitions,
    ForkAvailability,

    // Fork mitigations
    HighThresholds,
    NeutralForkRules,
    PostForkDecay,
    SurvivalForkException,
}

impl Mitigation {
    pub fn description(&self) -> &'static str {
        match self {
            Mitigation::VcDecay => "Validator Credits decay over time without participation",
            Mitigation::StakeCaps => "Maximum stake limits prevent concentration",
            Mitigation::ForkNeutrality => "No slashing for fork choice",
            Mitigation::EmergencySlowdown => "Reduced block production during emergencies",
            Mitigation::QuorumBounds => "Minimum and maximum participation requirements",
            Mitigation::Timelocks => "Mandatory waiting periods for changes",
            Mitigation::ReputationDecay => "Reputation fades without active contribution",
            Mitigation::ExitGuarantee => "Users can always withdraw assets",
            Mitigation::ForkGuarantee => "Forking is always possible",
            Mitigation::ScopedIdentity => "Identity is chain-local by default",
            Mitigation::ImportDiscount => "Imported reputation starts at reduced value",
            Mitigation::SlashingViaArbitration => "Malicious behavior punished through arbitration",
            Mitigation::SidechainPurge => "Inactive sidechains can be removed",
            Mitigation::IndependenceDefault => "Sidechains default to independence on fork",
            Mitigation::ScopedSecurity => "Security guarantees are per-sidechain",
            Mitigation::RevocableTreaties => "Cross-chain agreements can be terminated",
            Mitigation::HardDurationCaps => "Emergency powers have fixed maximum duration",
            Mitigation::Cooldowns => "Mandatory waiting between emergencies",
            Mitigation::ConstitutionalProhibitions => "Some actions prohibited even in emergency",
            Mitigation::ForkAvailability => "Fork path always available as escape",
            Mitigation::HighThresholds => "High requirements for fork declaration",
            Mitigation::NeutralForkRules => "Protocol treats all forks equally",
            Mitigation::PostForkDecay => "Accelerated decay after fork",
            Mitigation::SurvivalForkException => "Emergency fork always possible",
        }
    }
}

// =============================================================================
// SLOW CAPTURE DETECTION (SPEC v9 Section 5)
// =============================================================================

/// Slow capture indicator - accumulation of influence below detection threshold
#[derive(Debug, Clone)]
pub struct SlowCaptureIndicator {
    /// Entity being monitored
    pub entity: AccountId,

    /// Stake accumulation rate (per epoch)
    pub stake_accumulation_rate: f64,

    /// Validator credit accumulation rate
    pub vc_accumulation_rate: f64,

    /// Reputation accumulation rate
    pub reputation_accumulation_rate: f64,

    /// Governance influence accumulation
    pub governance_influence_rate: f64,

    /// Monitoring period (epochs)
    pub monitoring_period: u64,

    /// Alert threshold (combined score)
    pub alert_threshold: f64,
}

impl SlowCaptureIndicator {
    /// Calculate combined capture risk score
    pub fn capture_risk_score(&self) -> f64 {
        // Weighted combination of accumulation rates
        let stake_weight = 0.3;
        let vc_weight = 0.25;
        let rep_weight = 0.25;
        let gov_weight = 0.2;

        (self.stake_accumulation_rate * stake_weight)
            + (self.vc_accumulation_rate * vc_weight)
            + (self.reputation_accumulation_rate * rep_weight)
            + (self.governance_influence_rate * gov_weight)
    }

    /// Check if capture risk exceeds threshold
    pub fn is_alert(&self) -> bool {
        self.capture_risk_score() > self.alert_threshold
    }
}

/// Protocol countermeasures against slow capture
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlowCaptureCountermeasure {
    /// Power requires time to accumulate
    TimeWeightedPower,

    /// Validator Credits decay without action
    VcDecay,

    /// Reputation fades over time
    ReputationDecay,

    /// Capture incentivizes exit via fork
    ForkNeutrality,

    /// Capture effects are chain-local
    SidechainAutonomy,
}

impl SlowCaptureCountermeasure {
    pub fn all() -> Vec<SlowCaptureCountermeasure> {
        vec![
            SlowCaptureCountermeasure::TimeWeightedPower,
            SlowCaptureCountermeasure::VcDecay,
            SlowCaptureCountermeasure::ReputationDecay,
            SlowCaptureCountermeasure::ForkNeutrality,
            SlowCaptureCountermeasure::SidechainAutonomy,
        ]
    }

    pub fn effect(&self) -> &'static str {
        match self {
            SlowCaptureCountermeasure::TimeWeightedPower => "Cannot buy influence instantly",
            SlowCaptureCountermeasure::VcDecay => "Influence leaks without action",
            SlowCaptureCountermeasure::ReputationDecay => "Social power fades",
            SlowCaptureCountermeasure::ForkNeutrality => "Capture incentivizes exit",
            SlowCaptureCountermeasure::SidechainAutonomy => "Capture is local, not global",
        }
    }
}

// =============================================================================
// TERMINAL FAILURE MODES (SPEC v9 Section 6)
// =============================================================================

/// Acceptable failure modes - not considered protocol failure
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcceptableFailure {
    /// Majority of users leave the protocol
    MassExit,

    /// Protocol splits into multiple independent forks
    PeacefulFragmentation,

    /// Protocol is no longer used
    ProtocolAbandonment,

    /// Multiple competing forks exist
    MultipleForks,
}

impl AcceptableFailure {
    pub fn all() -> Vec<AcceptableFailure> {
        vec![
            AcceptableFailure::MassExit,
            AcceptableFailure::PeacefulFragmentation,
            AcceptableFailure::ProtocolAbandonment,
            AcceptableFailure::MultipleForks,
        ]
    }

    pub fn rationale(&self) -> &'static str {
        match self {
            AcceptableFailure::MassExit => "Exit is a fundamental right - mass exit is valid choice",
            AcceptableFailure::PeacefulFragmentation => "Fragmentation is resolution, not failure",
            AcceptableFailure::ProtocolAbandonment => "Protocol serves users, not vice versa",
            AcceptableFailure::MultipleForks => "Multiple visions can coexist",
        }
    }
}

/// Unacceptable failure modes - MUST be impossible by design
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnacceptableFailure {
    /// User assets permanently frozen
    FrozenAssets,

    /// Emergency state becomes permanent
    PermanentEmergency,

    /// Users cannot leave or change identity
    IdentityLockIn,

    /// Protocol enforces specific ideology
    ForcedIdeology,
}

impl UnacceptableFailure {
    pub fn all() -> Vec<UnacceptableFailure> {
        vec![
            UnacceptableFailure::FrozenAssets,
            UnacceptableFailure::PermanentEmergency,
            UnacceptableFailure::IdentityLockIn,
            UnacceptableFailure::ForcedIdeology,
        ]
    }

    pub fn violated_principle(&self) -> &'static str {
        match self {
            UnacceptableFailure::FrozenAssets => "Violates exit principle",
            UnacceptableFailure::PermanentEmergency => "Violates constitution",
            UnacceptableFailure::IdentityLockIn => "Violates autonomy",
            UnacceptableFailure::ForcedIdeology => "Violates neutrality",
        }
    }

    /// Check if current state exhibits this failure
    pub fn check(&self, state: &ProtocolHealthState) -> bool {
        match self {
            UnacceptableFailure::FrozenAssets => state.has_frozen_assets,
            UnacceptableFailure::PermanentEmergency => state.emergency_duration_exceeded,
            UnacceptableFailure::IdentityLockIn => state.identity_exit_blocked,
            UnacceptableFailure::ForcedIdeology => state.ideological_enforcement,
        }
    }
}

// =============================================================================
// PROTOCOL INVARIANTS (SPEC v9 Section 7)
// =============================================================================

/// Core protocol invariants that MUST always hold
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProtocolInvariant {
    /// Exit without permission
    ExitWithoutPermission,

    /// Fork without punishment
    ForkWithoutPunishment,

    /// Decay of accumulated power
    PowerDecay,

    /// Local failure without global collapse
    LocalFailureContainment,

    /// Emergency without permanence
    TemporaryEmergency,

    /// Ossification without imprisonment
    OssificationWithExit,
}

impl ProtocolInvariant {
    pub fn all() -> Vec<ProtocolInvariant> {
        vec![
            ProtocolInvariant::ExitWithoutPermission,
            ProtocolInvariant::ForkWithoutPunishment,
            ProtocolInvariant::PowerDecay,
            ProtocolInvariant::LocalFailureContainment,
            ProtocolInvariant::TemporaryEmergency,
            ProtocolInvariant::OssificationWithExit,
        ]
    }

    pub fn description(&self) -> &'static str {
        match self {
            ProtocolInvariant::ExitWithoutPermission =>
                "Users can always withdraw assets without approval",
            ProtocolInvariant::ForkWithoutPunishment =>
                "Participants can fork without slashing or penalty",
            ProtocolInvariant::PowerDecay =>
                "All forms of accumulated power decay over time",
            ProtocolInvariant::LocalFailureContainment =>
                "Sidechain failures don't cause root chain failures",
            ProtocolInvariant::TemporaryEmergency =>
                "Emergency powers have hard duration limits",
            ProtocolInvariant::OssificationWithExit =>
                "Even ossified protocol allows exit and fork",
        }
    }

    /// Check if this invariant holds
    pub fn check(&self, state: &ProtocolHealthState) -> InvariantCheckResult {
        let holds = match self {
            ProtocolInvariant::ExitWithoutPermission => state.exit_always_possible,
            ProtocolInvariant::ForkWithoutPunishment => state.fork_without_slashing,
            ProtocolInvariant::PowerDecay => state.decay_active,
            ProtocolInvariant::LocalFailureContainment => state.failures_contained,
            ProtocolInvariant::TemporaryEmergency => !state.emergency_duration_exceeded,
            ProtocolInvariant::OssificationWithExit => state.exit_possible_when_ossified,
        };

        InvariantCheckResult {
            invariant: *self,
            holds,
            checked_at: state.checked_at,
        }
    }
}

/// Result of checking an invariant
#[derive(Debug, Clone)]
pub struct InvariantCheckResult {
    pub invariant: ProtocolInvariant,
    pub holds: bool,
    pub checked_at: BlockNumber,
}

// =============================================================================
// PROTOCOL HEALTH STATE
// =============================================================================

/// Current health state of the protocol for invariant checking
#[derive(Debug, Clone, Default)]
pub struct ProtocolHealthState {
    /// Block number when state was captured
    pub checked_at: BlockNumber,

    // Exit invariant
    pub exit_always_possible: bool,

    // Fork invariant
    pub fork_without_slashing: bool,

    // Decay invariant
    pub decay_active: bool,

    // Containment invariant
    pub failures_contained: bool,

    // Emergency invariant
    pub emergency_duration_exceeded: bool,

    // Ossification invariant
    pub exit_possible_when_ossified: bool,

    // Unacceptable failure checks
    pub has_frozen_assets: bool,
    pub identity_exit_blocked: bool,
    pub ideological_enforcement: bool,
}

impl ProtocolHealthState {
    pub fn new(block: BlockNumber) -> Self {
        Self {
            checked_at: block,
            exit_always_possible: true,
            fork_without_slashing: true,
            decay_active: true,
            failures_contained: true,
            emergency_duration_exceeded: false,
            exit_possible_when_ossified: true,
            has_frozen_assets: false,
            identity_exit_blocked: false,
            ideological_enforcement: false,
        }
    }

    /// Check all invariants
    pub fn check_all_invariants(&self) -> Vec<InvariantCheckResult> {
        ProtocolInvariant::all()
            .iter()
            .map(|inv| inv.check(self))
            .collect()
    }

    /// Check for any unacceptable failures
    pub fn check_unacceptable_failures(&self) -> Vec<UnacceptableFailure> {
        UnacceptableFailure::all()
            .into_iter()
            .filter(|f| f.check(self))
            .collect()
    }

    /// Is the protocol in a healthy state?
    pub fn is_healthy(&self) -> bool {
        self.check_all_invariants().iter().all(|r| r.holds)
            && self.check_unacceptable_failures().is_empty()
    }
}

// =============================================================================
// SECURITY AUDIT FRAMEWORK (SPEC v9 Section 8)
// =============================================================================

/// Types of security audits required
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditType {
    /// Cryptographic primitives (VRF, signatures)
    Cryptography,

    /// Economic model simulations
    EconomicSimulation,

    /// Governance stress tests
    GovernanceStress,

    /// Fork scenario simulations
    ForkSimulation,

    /// Emergency abuse simulations
    EmergencyAbuse,
}

impl AuditType {
    pub fn all() -> Vec<AuditType> {
        vec![
            AuditType::Cryptography,
            AuditType::EconomicSimulation,
            AuditType::GovernanceStress,
            AuditType::ForkSimulation,
            AuditType::EmergencyAbuse,
        ]
    }

    pub fn description(&self) -> &'static str {
        match self {
            AuditType::Cryptography => "Review of VRF, signatures, and crypto primitives",
            AuditType::EconomicSimulation => "Agent-based economic model stress testing",
            AuditType::GovernanceStress => "Governance attack simulation",
            AuditType::ForkSimulation => "Fork scenario outcome analysis",
            AuditType::EmergencyAbuse => "Emergency power abuse simulation",
        }
    }
}

/// Audit record
#[derive(Debug, Clone)]
pub struct AuditRecord {
    /// Type of audit performed
    pub audit_type: AuditType,

    /// Auditor identifier
    pub auditor: String,

    /// When audit was performed
    pub performed_at: BlockNumber,

    /// Audit passed
    pub passed: bool,

    /// Findings summary
    pub findings: String,

    /// Hash of full audit report
    pub report_hash: CryptoHash,
}

/// Long-term review cycle tracking
#[derive(Debug, Clone)]
pub struct ReviewCycle {
    /// Cycle number
    pub cycle: u32,

    /// Start block
    pub started_at: BlockNumber,

    /// Review items completed
    pub axiom_review_complete: bool,
    pub fork_consideration_complete: bool,
    pub ossification_review_complete: bool,

    /// Next review due (blocks)
    pub next_review_due: BlockNumber,
}

impl ReviewCycle {
    /// 5-year review cycle in blocks (assuming 6 second blocks)
    pub const REVIEW_PERIOD_BLOCKS: BlockNumber = 5 * 365 * 24 * 60 * 10; // ~5 years

    pub fn new(cycle: u32, started_at: BlockNumber) -> Self {
        Self {
            cycle,
            started_at,
            axiom_review_complete: false,
            fork_consideration_complete: false,
            ossification_review_complete: false,
            next_review_due: started_at + Self::REVIEW_PERIOD_BLOCKS,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.axiom_review_complete
            && self.fork_consideration_complete
            && self.ossification_review_complete
    }

    pub fn is_due(&self, current_block: BlockNumber) -> bool {
        current_block >= self.next_review_due
    }
}

// =============================================================================
// THREAT ASSESSMENT
// =============================================================================

/// Complete threat assessment for a point in time
#[derive(Debug, Clone)]
pub struct ThreatAssessment {
    /// When assessment was made
    pub assessed_at: BlockNumber,

    /// Active threats detected
    pub active_threats: Vec<DetectedThreat>,

    /// Slow capture indicators
    pub capture_indicators: Vec<SlowCaptureIndicator>,

    /// Overall threat level
    pub threat_level: ThreatLevel,

    /// Recommended actions
    pub recommendations: Vec<String>,
}

/// Detected active threat
#[derive(Debug, Clone)]
pub struct DetectedThreat {
    pub threat_type: ThreatType,
    pub severity: ThreatSeverity,
    pub evidence: String,
    pub detected_at: BlockNumber,
    pub affected_entities: Vec<AccountId>,
}

/// Threat severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreatSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Overall threat level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreatLevel {
    /// No significant threats
    Normal,

    /// Minor concerns requiring monitoring
    Elevated,

    /// Significant threats requiring action
    High,

    /// Critical threats requiring immediate response
    Critical,

    /// Protocol survival at risk
    Existential,
}

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
    fn test_adversary_classes() {
        let classes = AdversaryClass::all();
        assert_eq!(classes.len(), 6);

        for class in classes {
            assert!(!class.description().is_empty());
        }
    }

    #[test]
    fn test_non_assumptions() {
        let assumptions = NonAssumption::all();
        assert_eq!(assumptions.len(), 5);

        // Verify all are covered
        assert!(assumptions.contains(&NonAssumption::HonestMajority));
        assert!(assumptions.contains(&NonAssumption::BenevolentGovernance));
        assert!(assumptions.contains(&NonAssumption::AlignedIncentives));
        assert!(assumptions.contains(&NonAssumption::StableIdeology));
        assert!(assumptions.contains(&NonAssumption::PermanentParticipation));
    }

    #[test]
    fn test_threat_mitigations() {
        // Every threat should have at least one mitigation
        let threats = vec![
            ThreatType::ValidatorCartelization,
            ThreatType::LongRangeAttack,
            ThreatType::VrfGrinding,
            ThreatType::ProposalSpam,
            ThreatType::VoterApathy,
            ThreatType::MajorityTyranny,
            ThreatType::SybilAccumulation,
            ThreatType::ReputationFarming,
            ThreatType::CrossChainLaundering,
            ThreatType::ZombieChains,
            ThreatType::GovernanceCapture,
            ThreatType::FederationAbuse,
            ThreatType::EmergencyAbuse,
            ThreatType::PermanentExceptionalState,
            ThreatType::ForkSpam,
            ThreatType::OssificationAbuse,
            ThreatType::MinorityLockOut,
        ];

        for threat in threats {
            let mitigations = threat.mitigations();
            assert!(!mitigations.is_empty(),
                "Threat {:?} has no mitigations", threat);
        }
    }

    #[test]
    fn test_slow_capture_indicator() {
        let indicator = SlowCaptureIndicator {
            entity: create_account(1),
            stake_accumulation_rate: 0.1,
            vc_accumulation_rate: 0.2,
            reputation_accumulation_rate: 0.15,
            governance_influence_rate: 0.1,
            monitoring_period: 100,
            alert_threshold: 0.2,
        };

        let score = indicator.capture_risk_score();
        // 0.1*0.3 + 0.2*0.25 + 0.15*0.25 + 0.1*0.2 = 0.03 + 0.05 + 0.0375 + 0.02 = 0.1375
        assert!(score > 0.1 && score < 0.2);
        assert!(!indicator.is_alert()); // Below threshold
    }

    #[test]
    fn test_slow_capture_alert() {
        let indicator = SlowCaptureIndicator {
            entity: create_account(1),
            stake_accumulation_rate: 0.5,
            vc_accumulation_rate: 0.5,
            reputation_accumulation_rate: 0.5,
            governance_influence_rate: 0.5,
            monitoring_period: 100,
            alert_threshold: 0.2,
        };

        assert!(indicator.is_alert()); // High accumulation triggers alert
    }

    #[test]
    fn test_acceptable_failures() {
        let failures = AcceptableFailure::all();
        assert_eq!(failures.len(), 4);

        // These are acceptable - protocol philosophy
        for failure in failures {
            assert!(!failure.rationale().is_empty());
        }
    }

    #[test]
    fn test_unacceptable_failures() {
        let failures = UnacceptableFailure::all();
        assert_eq!(failures.len(), 4);

        // These MUST be impossible
        for failure in failures {
            assert!(!failure.violated_principle().is_empty());
        }
    }

    #[test]
    fn test_protocol_invariants() {
        let invariants = ProtocolInvariant::all();
        assert_eq!(invariants.len(), 6);

        for inv in invariants {
            assert!(!inv.description().is_empty());
        }
    }

    #[test]
    fn test_healthy_protocol_state() {
        let state = ProtocolHealthState::new(1000);

        // Fresh state should be healthy
        assert!(state.is_healthy());
        assert!(state.check_unacceptable_failures().is_empty());

        let results = state.check_all_invariants();
        assert!(results.iter().all(|r| r.holds));
    }

    #[test]
    fn test_unhealthy_protocol_state() {
        let mut state = ProtocolHealthState::new(1000);

        // Break an invariant
        state.exit_always_possible = false;

        assert!(!state.is_healthy());

        let results = state.check_all_invariants();
        let exit_check = results.iter()
            .find(|r| r.invariant == ProtocolInvariant::ExitWithoutPermission)
            .unwrap();
        assert!(!exit_check.holds);
    }

    #[test]
    fn test_unacceptable_failure_detection() {
        let mut state = ProtocolHealthState::new(1000);

        // Simulate frozen assets
        state.has_frozen_assets = true;

        let failures = state.check_unacceptable_failures();
        assert!(!failures.is_empty());
        assert!(failures.contains(&UnacceptableFailure::FrozenAssets));
    }

    #[test]
    fn test_audit_types() {
        let audits = AuditType::all();
        assert_eq!(audits.len(), 5);

        for audit in audits {
            assert!(!audit.description().is_empty());
        }
    }

    #[test]
    fn test_review_cycle() {
        let cycle = ReviewCycle::new(1, 0);

        assert!(!cycle.is_complete());
        assert!(!cycle.is_due(1000));

        // 5 years later
        assert!(cycle.is_due(ReviewCycle::REVIEW_PERIOD_BLOCKS + 1));
    }

    #[test]
    fn test_review_cycle_completion() {
        let mut cycle = ReviewCycle::new(1, 0);

        cycle.axiom_review_complete = true;
        assert!(!cycle.is_complete());

        cycle.fork_consideration_complete = true;
        assert!(!cycle.is_complete());

        cycle.ossification_review_complete = true;
        assert!(cycle.is_complete());
    }

    #[test]
    fn test_threat_layer_mapping() {
        assert_eq!(ThreatType::ValidatorCartelization.layer(), ThreatLayer::Consensus);
        assert_eq!(ThreatType::ProposalSpam.layer(), ThreatLayer::Governance);
        assert_eq!(ThreatType::SybilAccumulation.layer(), ThreatLayer::IdentityReputation);
        assert_eq!(ThreatType::ZombieChains.layer(), ThreatLayer::Sidechain);
        assert_eq!(ThreatType::EmergencyAbuse.layer(), ThreatLayer::Emergency);
        assert_eq!(ThreatType::ForkSpam.layer(), ThreatLayer::ForkingOssification);
    }

    #[test]
    fn test_slow_capture_countermeasures() {
        let countermeasures = SlowCaptureCountermeasure::all();
        assert_eq!(countermeasures.len(), 5);

        for cm in countermeasures {
            assert!(!cm.effect().is_empty());
        }
    }

    #[test]
    fn test_threat_severity_ordering() {
        assert!(ThreatSeverity::Low < ThreatSeverity::Medium);
        assert!(ThreatSeverity::Medium < ThreatSeverity::High);
        assert!(ThreatSeverity::High < ThreatSeverity::Critical);
    }

    #[test]
    fn test_threat_level_ordering() {
        assert!(ThreatLevel::Normal < ThreatLevel::Elevated);
        assert!(ThreatLevel::Elevated < ThreatLevel::High);
        assert!(ThreatLevel::High < ThreatLevel::Critical);
        assert!(ThreatLevel::Critical < ThreatLevel::Existential);
    }

    #[test]
    fn test_mitigation_descriptions() {
        let mitigations = vec![
            Mitigation::VcDecay,
            Mitigation::StakeCaps,
            Mitigation::ForkNeutrality,
            Mitigation::ExitGuarantee,
            Mitigation::ConstitutionalProhibitions,
        ];

        for m in mitigations {
            assert!(!m.description().is_empty());
        }
    }
}
