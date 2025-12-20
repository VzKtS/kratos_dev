// Meta-Governance Contract - SPEC v6
// Protocol evolution with constitutional safeguards
//
// Principle: The protocol can change, but its guarantees cannot.

use crate::types::{
    AccountId, Balance, BlockNumber, ChainId, Hash,
    ProtocolParameters, ProtocolVersion, ParameterChange, ParameterError,
    ConstitutionalAxiom, ConstitutionalProhibition,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// CONSTANTS
// =============================================================================

/// Protocol upgrade proposal requires supermajority (75%)
pub const PROTOCOL_UPGRADE_THRESHOLD: u8 = 75;

/// Protocol upgrade timelock (60 days = 864,000 blocks)
pub const PROTOCOL_UPGRADE_TIMELOCK: BlockNumber = 864_000;

/// Minimum signaling period for multi-chain coordination (14 days)
pub const SIGNALING_PERIOD: BlockNumber = 201_600;

/// Required chain adoption percentage for protocol upgrade (66% ≈ 2/3 supermajority)
pub const CHAIN_ADOPTION_THRESHOLD: u8 = 66;

/// Emergency parameter change timelock (7 days)
pub const EMERGENCY_TIMELOCK: BlockNumber = 100_800;

/// Maximum batch size for parameter changes
pub const MAX_BATCH_SIZE: usize = 10;

// =============================================================================
// UPGRADE PROPOSAL TYPES
// =============================================================================

/// Types of protocol upgrade proposals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpgradeProposalType {
    /// Change protocol parameters within constitutional bounds
    ParameterChange {
        changes: Vec<ParameterChange>,
        rationale: String,
    },

    /// Protocol version upgrade (new features)
    VersionUpgrade {
        new_version: ProtocolVersion,
        changelog: String,
        breaking_changes: bool,
    },

    /// Emergency parameter adjustment (shorter timelock)
    EmergencyChange {
        changes: Vec<ParameterChange>,
        justification: String,
    },
}

impl UpgradeProposalType {
    /// Get required threshold for this proposal type
    pub fn required_threshold(&self) -> u8 {
        match self {
            UpgradeProposalType::ParameterChange { .. } => PROTOCOL_UPGRADE_THRESHOLD,
            UpgradeProposalType::VersionUpgrade { breaking_changes, .. } => {
                if *breaking_changes { 80 } else { PROTOCOL_UPGRADE_THRESHOLD }
            }
            UpgradeProposalType::EmergencyChange { .. } => 80, // Higher threshold for emergency
        }
    }

    /// Get timelock for this proposal type
    pub fn timelock(&self) -> BlockNumber {
        match self {
            UpgradeProposalType::ParameterChange { .. } => PROTOCOL_UPGRADE_TIMELOCK,
            UpgradeProposalType::VersionUpgrade { .. } => PROTOCOL_UPGRADE_TIMELOCK,
            UpgradeProposalType::EmergencyChange { .. } => EMERGENCY_TIMELOCK,
        }
    }

    /// Check if this proposal type requires multi-chain signaling
    pub fn requires_signaling(&self) -> bool {
        matches!(self, UpgradeProposalType::VersionUpgrade { .. })
    }
}

// =============================================================================
// UPGRADE PROPOSAL
// =============================================================================

/// Status of an upgrade proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeStatus {
    /// Proposal is in signaling phase
    Signaling,
    /// Proposal is open for voting
    Voting,
    /// Voting passed, in timelock
    Approved,
    /// Ready for execution
    ExecutionReady,
    /// Successfully executed
    Executed,
    /// Rejected by vote or timeout
    Rejected,
    /// Cancelled by proposer
    Cancelled,
    /// Rejected due to constitutional violation
    ConstitutionallyRejected,
}

/// Chain signal for multi-chain coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainSignal {
    pub chain_id: ChainId,
    pub supports: bool,
    pub signaled_at: BlockNumber,
    pub validator_count: u32,
}

/// An upgrade proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeProposal {
    /// Unique proposal ID
    pub id: Hash,

    /// Proposer account
    pub proposer: AccountId,

    /// Type of upgrade
    pub proposal_type: UpgradeProposalType,

    /// Current status
    pub status: UpgradeStatus,

    /// Block when proposal was created
    pub created_at: BlockNumber,

    /// Block when voting started (after signaling if required)
    pub voting_started: Option<BlockNumber>,

    /// Block when voting ended
    pub voting_ended: Option<BlockNumber>,

    /// Yes votes (weighted)
    pub yes_votes: Balance,

    /// No votes (weighted)
    pub no_votes: Balance,

    /// Voters who have voted
    pub voters: HashMap<AccountId, bool>,

    /// Chain signals (for version upgrades)
    pub chain_signals: HashMap<ChainId, ChainSignal>,

    /// Impact analysis (required)
    pub impact_analysis: ImpactAnalysis,

    /// Deposit locked
    pub deposit: Balance,
}

impl UpgradeProposal {
    /// Create a new upgrade proposal
    pub fn new(
        proposer: AccountId,
        proposal_type: UpgradeProposalType,
        impact_analysis: ImpactAnalysis,
        deposit: Balance,
        current_block: BlockNumber,
    ) -> Self {
        // Create unique proposal ID
        let mut id_data = Vec::new();
        id_data.extend_from_slice(proposer.as_bytes());
        id_data.extend_from_slice(&current_block.to_le_bytes());
        id_data.extend_from_slice(&deposit.to_le_bytes());
        let id = Hash::hash(&id_data);

        let status = if proposal_type.requires_signaling() {
            UpgradeStatus::Signaling
        } else {
            UpgradeStatus::Voting
        };

        let voting_started = if !proposal_type.requires_signaling() {
            Some(current_block)
        } else {
            None
        };

        Self {
            id,
            proposer,
            proposal_type,
            status,
            created_at: current_block,
            voting_started,
            voting_ended: None,
            yes_votes: 0,
            no_votes: 0,
            voters: HashMap::new(),
            chain_signals: HashMap::new(),
            impact_analysis,
            deposit,
        }
    }

    /// Check if voting is open
    pub fn is_voting_open(&self, current_block: BlockNumber, voting_period: BlockNumber) -> bool {
        if self.status != UpgradeStatus::Voting {
            return false;
        }

        if let Some(started) = self.voting_started {
            current_block < started + voting_period
        } else {
            false
        }
    }

    /// Calculate approval percentage
    pub fn approval_percentage(&self) -> u8 {
        let total = self.yes_votes + self.no_votes;
        if total == 0 {
            return 0;
        }
        ((self.yes_votes * 100) / total) as u8
    }

    /// Check if proposal passed
    pub fn passed(&self) -> bool {
        self.approval_percentage() >= self.proposal_type.required_threshold()
    }

    /// Check if timelock has elapsed
    pub fn timelock_elapsed(&self, current_block: BlockNumber) -> bool {
        if let Some(ended) = self.voting_ended {
            current_block >= ended + self.proposal_type.timelock()
        } else {
            false
        }
    }
}

// =============================================================================
// IMPACT ANALYSIS
// =============================================================================

/// Required impact analysis for upgrade proposals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactAnalysis {
    /// Affected subsystems
    pub affected_subsystems: Vec<String>,

    /// Backward compatibility assessment
    pub backward_compatible: bool,

    /// Risk level (1-5)
    pub risk_level: u8,

    /// Estimated rollback difficulty (1-5)
    pub rollback_difficulty: u8,

    /// Required node version (if any)
    pub required_node_version: Option<String>,

    /// Migration steps (if any)
    pub migration_steps: Vec<String>,

    /// Test results summary
    pub test_summary: String,
}

impl ImpactAnalysis {
    /// Create a minimal impact analysis for parameter changes
    pub fn minimal(subsystem: &str) -> Self {
        Self {
            affected_subsystems: vec![subsystem.to_string()],
            backward_compatible: true,
            risk_level: 1,
            rollback_difficulty: 1,
            required_node_version: None,
            migration_steps: vec![],
            test_summary: "Standard parameter change".to_string(),
        }
    }

    /// Validate impact analysis completeness
    pub fn is_complete(&self) -> bool {
        !self.affected_subsystems.is_empty()
            && self.risk_level >= 1 && self.risk_level <= 5
            && self.rollback_difficulty >= 1 && self.rollback_difficulty <= 5
            && !self.test_summary.is_empty()
    }
}

// =============================================================================
// META GOVERNANCE CONTRACT
// =============================================================================

/// Meta-governance contract for protocol evolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaGovernanceContract {
    /// Current protocol parameters
    parameters: ProtocolParameters,

    /// Historical parameters (for rollback)
    parameter_history: Vec<(BlockNumber, ProtocolParameters)>,

    /// Active proposals
    proposals: HashMap<Hash, UpgradeProposal>,

    /// Executed proposal IDs (for deduplication)
    executed_proposals: Vec<Hash>,

    /// Registered chains for signaling
    registered_chains: HashMap<ChainId, ChainInfo>,

    /// Root chain voting power
    voting_power: HashMap<AccountId, Balance>,

    /// Total voting power
    total_voting_power: Balance,

    /// Events emitted
    events: Vec<MetaGovernanceEvent>,
}

/// Basic chain info for signaling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainInfo {
    pub chain_id: ChainId,
    pub validator_count: u32,
    pub last_signal: BlockNumber,
}

impl MetaGovernanceContract {
    /// Create new meta-governance contract
    pub fn new() -> Self {
        Self {
            parameters: ProtocolParameters::genesis(),
            parameter_history: vec![],
            proposals: HashMap::new(),
            executed_proposals: Vec::new(),
            registered_chains: HashMap::new(),
            voting_power: HashMap::new(),
            total_voting_power: 0,
            events: Vec::new(),
        }
    }

    /// Get current protocol parameters
    pub fn parameters(&self) -> &ProtocolParameters {
        &self.parameters
    }

    /// Set voting power for an account
    pub fn set_voting_power(&mut self, account: AccountId, power: Balance) {
        let old_power = self.voting_power.get(&account).copied().unwrap_or(0);
        self.total_voting_power = self.total_voting_power - old_power + power;

        if power > 0 {
            self.voting_power.insert(account, power);
        } else {
            self.voting_power.remove(&account);
        }
    }

    /// Register a chain for signaling
    pub fn register_chain(&mut self, chain_id: ChainId, validator_count: u32, current_block: BlockNumber) {
        self.registered_chains.insert(chain_id, ChainInfo {
            chain_id,
            validator_count,
            last_signal: current_block,
        });
    }

    /// Create a parameter change proposal
    pub fn propose_parameter_change(
        &mut self,
        proposer: AccountId,
        changes: Vec<ParameterChange>,
        rationale: String,
        deposit: Balance,
        current_block: BlockNumber,
    ) -> Result<Hash, MetaGovernanceError> {
        // Validate proposer has voting power
        if self.voting_power.get(&proposer).copied().unwrap_or(0) == 0 {
            return Err(MetaGovernanceError::NoVotingPower);
        }

        // Validate batch size
        if changes.len() > MAX_BATCH_SIZE {
            return Err(MetaGovernanceError::BatchTooLarge);
        }

        // Validate all changes are constitutional
        for change in &changes {
            if !change.is_constitutional(&self.parameters) {
                return Err(MetaGovernanceError::ConstitutionalViolation);
            }
        }

        // Create impact analysis
        let impact = ImpactAnalysis::minimal("parameters");

        let proposal = UpgradeProposal::new(
            proposer,
            UpgradeProposalType::ParameterChange { changes, rationale },
            impact,
            deposit,
            current_block,
        );

        let id = proposal.id;
        self.proposals.insert(id, proposal);

        self.events.push(MetaGovernanceEvent::ProposalCreated {
            proposal_id: id,
            proposer,
            proposal_type: "ParameterChange".to_string(),
        });

        Ok(id)
    }

    /// Create a version upgrade proposal
    pub fn propose_version_upgrade(
        &mut self,
        proposer: AccountId,
        new_version: ProtocolVersion,
        changelog: String,
        breaking_changes: bool,
        impact_analysis: ImpactAnalysis,
        deposit: Balance,
        current_block: BlockNumber,
    ) -> Result<Hash, MetaGovernanceError> {
        // Validate proposer
        if self.voting_power.get(&proposer).copied().unwrap_or(0) == 0 {
            return Err(MetaGovernanceError::NoVotingPower);
        }

        // Validate version is newer
        if !new_version.is_newer(&self.parameters.version) {
            return Err(MetaGovernanceError::InvalidVersion);
        }

        // Validate impact analysis
        if !impact_analysis.is_complete() {
            return Err(MetaGovernanceError::IncompleteImpactAnalysis);
        }

        let proposal = UpgradeProposal::new(
            proposer,
            UpgradeProposalType::VersionUpgrade {
                new_version,
                changelog,
                breaking_changes,
            },
            impact_analysis,
            deposit,
            current_block,
        );

        let id = proposal.id;
        self.proposals.insert(id, proposal);

        self.events.push(MetaGovernanceEvent::ProposalCreated {
            proposal_id: id,
            proposer,
            proposal_type: "VersionUpgrade".to_string(),
        });

        Ok(id)
    }

    /// Submit chain signal for a version upgrade
    pub fn submit_chain_signal(
        &mut self,
        proposal_id: Hash,
        chain_id: ChainId,
        supports: bool,
        current_block: BlockNumber,
    ) -> Result<(), MetaGovernanceError> {
        let proposal = self.proposals
            .get_mut(&proposal_id)
            .ok_or(MetaGovernanceError::ProposalNotFound)?;

        if proposal.status != UpgradeStatus::Signaling {
            return Err(MetaGovernanceError::NotInSignalingPhase);
        }

        let chain_info = self.registered_chains
            .get(&chain_id)
            .ok_or(MetaGovernanceError::ChainNotRegistered)?;

        proposal.chain_signals.insert(chain_id, ChainSignal {
            chain_id,
            supports,
            signaled_at: current_block,
            validator_count: chain_info.validator_count,
        });

        self.events.push(MetaGovernanceEvent::ChainSignaled {
            proposal_id,
            chain_id,
            supports,
        });

        Ok(())
    }

    /// Advance proposal from signaling to voting
    pub fn advance_to_voting(
        &mut self,
        proposal_id: Hash,
        current_block: BlockNumber,
    ) -> Result<(), MetaGovernanceError> {
        let proposal = self.proposals
            .get_mut(&proposal_id)
            .ok_or(MetaGovernanceError::ProposalNotFound)?;

        if proposal.status != UpgradeStatus::Signaling {
            return Err(MetaGovernanceError::NotInSignalingPhase);
        }

        // Check signaling period elapsed
        if current_block < proposal.created_at + SIGNALING_PERIOD {
            return Err(MetaGovernanceError::SignalingPeriodNotElapsed);
        }

        // Check chain adoption threshold
        let total_validators: u32 = self.registered_chains.values()
            .map(|c| c.validator_count)
            .sum();

        let supporting_validators: u32 = proposal.chain_signals.values()
            .filter(|s| s.supports)
            .map(|s| s.validator_count)
            .sum();

        let adoption = if total_validators > 0 {
            (supporting_validators * 100 / total_validators) as u8
        } else {
            0
        };

        if adoption < CHAIN_ADOPTION_THRESHOLD {
            proposal.status = UpgradeStatus::Rejected;
            self.events.push(MetaGovernanceEvent::ProposalRejected {
                proposal_id,
                reason: "Insufficient chain adoption".to_string(),
            });
            return Err(MetaGovernanceError::InsufficientChainAdoption);
        }

        proposal.status = UpgradeStatus::Voting;
        proposal.voting_started = Some(current_block);

        self.events.push(MetaGovernanceEvent::VotingStarted { proposal_id });

        Ok(())
    }

    /// Vote on a proposal
    pub fn vote(
        &mut self,
        proposal_id: Hash,
        voter: AccountId,
        support: bool,
        current_block: BlockNumber,
    ) -> Result<(), MetaGovernanceError> {
        let voting_period = self.parameters.governance.voting_period.value();
        let power = self.voting_power.get(&voter).copied().unwrap_or(0);

        if power == 0 {
            return Err(MetaGovernanceError::NoVotingPower);
        }

        let proposal = self.proposals
            .get_mut(&proposal_id)
            .ok_or(MetaGovernanceError::ProposalNotFound)?;

        if !proposal.is_voting_open(current_block, voting_period) {
            return Err(MetaGovernanceError::VotingNotOpen);
        }

        if proposal.voters.contains_key(&voter) {
            return Err(MetaGovernanceError::AlreadyVoted);
        }

        proposal.voters.insert(voter, support);
        if support {
            proposal.yes_votes += power;
        } else {
            proposal.no_votes += power;
        }

        self.events.push(MetaGovernanceEvent::VoteCast {
            proposal_id,
            voter,
            support,
            weight: power,
        });

        Ok(())
    }

    /// Finalize voting on a proposal
    pub fn finalize_voting(
        &mut self,
        proposal_id: Hash,
        current_block: BlockNumber,
    ) -> Result<(), MetaGovernanceError> {
        let voting_period = self.parameters.governance.voting_period.value();

        let proposal = self.proposals
            .get_mut(&proposal_id)
            .ok_or(MetaGovernanceError::ProposalNotFound)?;

        if proposal.status != UpgradeStatus::Voting {
            return Err(MetaGovernanceError::NotInVotingPhase);
        }

        let voting_started = proposal.voting_started
            .ok_or(MetaGovernanceError::VotingNotStarted)?;

        if current_block < voting_started + voting_period {
            return Err(MetaGovernanceError::VotingPeriodNotElapsed);
        }

        proposal.voting_ended = Some(current_block);

        if proposal.passed() {
            proposal.status = UpgradeStatus::Approved;
            self.events.push(MetaGovernanceEvent::ProposalApproved { proposal_id });
        } else {
            proposal.status = UpgradeStatus::Rejected;
            self.events.push(MetaGovernanceEvent::ProposalRejected {
                proposal_id,
                reason: "Did not reach threshold".to_string(),
            });
        }

        Ok(())
    }

    /// Execute an approved proposal
    pub fn execute_proposal(
        &mut self,
        proposal_id: Hash,
        current_block: BlockNumber,
    ) -> Result<(), MetaGovernanceError> {
        let proposal = self.proposals
            .get_mut(&proposal_id)
            .ok_or(MetaGovernanceError::ProposalNotFound)?;

        if proposal.status != UpgradeStatus::Approved {
            return Err(MetaGovernanceError::NotApproved);
        }

        if !proposal.timelock_elapsed(current_block) {
            return Err(MetaGovernanceError::TimelockNotElapsed);
        }

        // Store current parameters in history
        self.parameter_history.push((current_block, self.parameters.clone()));

        // Apply changes based on proposal type
        match &proposal.proposal_type {
            UpgradeProposalType::ParameterChange { changes, .. } => {
                for change in changes {
                    change.apply(&mut self.parameters)?;
                }
            }
            UpgradeProposalType::VersionUpgrade { new_version, .. } => {
                self.parameters.version = *new_version;
            }
            UpgradeProposalType::EmergencyChange { changes, .. } => {
                for change in changes {
                    change.apply(&mut self.parameters)?;
                }
            }
        }

        self.parameters.active_since = current_block;

        proposal.status = UpgradeStatus::Executed;
        self.executed_proposals.push(proposal_id);

        self.events.push(MetaGovernanceEvent::ProposalExecuted {
            proposal_id,
            effective_block: current_block,
        });

        Ok(())
    }

    /// Cancel a proposal (only by proposer, before execution)
    pub fn cancel_proposal(
        &mut self,
        proposal_id: Hash,
        caller: &AccountId,
    ) -> Result<Balance, MetaGovernanceError> {
        let proposal = self.proposals
            .get_mut(&proposal_id)
            .ok_or(MetaGovernanceError::ProposalNotFound)?;

        if &proposal.proposer != caller {
            return Err(MetaGovernanceError::NotProposer);
        }

        if proposal.status == UpgradeStatus::Executed {
            return Err(MetaGovernanceError::AlreadyExecuted);
        }

        let deposit = proposal.deposit;
        proposal.status = UpgradeStatus::Cancelled;

        self.events.push(MetaGovernanceEvent::ProposalCancelled { proposal_id });

        Ok(deposit)
    }

    /// Check if a parameter change would violate constitutional axioms
    pub fn check_constitutional_compliance(
        &self,
        changes: &[ParameterChange],
    ) -> Result<(), MetaGovernanceError> {
        for change in changes {
            if !change.is_constitutional(&self.parameters) {
                return Err(MetaGovernanceError::ConstitutionalViolation);
            }
        }
        Ok(())
    }

    /// Get a proposal by ID
    pub fn get_proposal(&self, proposal_id: &Hash) -> Option<&UpgradeProposal> {
        self.proposals.get(proposal_id)
    }

    /// Get all active proposals
    pub fn get_active_proposals(&self) -> Vec<&UpgradeProposal> {
        self.proposals.values()
            .filter(|p| !matches!(
                p.status,
                UpgradeStatus::Executed | UpgradeStatus::Rejected |
                UpgradeStatus::Cancelled | UpgradeStatus::ConstitutionallyRejected
            ))
            .collect()
    }

    /// Get parameter history
    pub fn get_parameter_history(&self) -> &[(BlockNumber, ProtocolParameters)] {
        &self.parameter_history
    }

    /// Get events
    pub fn events(&self) -> &[MetaGovernanceEvent] {
        &self.events
    }

    /// Clear events
    pub fn clear_events(&mut self) {
        self.events.clear();
    }
}

impl Default for MetaGovernanceContract {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// ERRORS
// =============================================================================

/// Meta-governance errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaGovernanceError {
    /// Proposer has no voting power
    NoVotingPower,
    /// Proposal not found
    ProposalNotFound,
    /// Batch too large
    BatchTooLarge,
    /// Constitutional violation
    ConstitutionalViolation,
    /// Invalid version
    InvalidVersion,
    /// Incomplete impact analysis
    IncompleteImpactAnalysis,
    /// Chain not registered
    ChainNotRegistered,
    /// Not in signaling phase
    NotInSignalingPhase,
    /// Signaling period not elapsed
    SignalingPeriodNotElapsed,
    /// Insufficient chain adoption
    InsufficientChainAdoption,
    /// Voting not open
    VotingNotOpen,
    /// Already voted
    AlreadyVoted,
    /// Not in voting phase
    NotInVotingPhase,
    /// Voting not started
    VotingNotStarted,
    /// Voting period not elapsed
    VotingPeriodNotElapsed,
    /// Not approved
    NotApproved,
    /// Timelock not elapsed
    TimelockNotElapsed,
    /// Not proposer
    NotProposer,
    /// Already executed
    AlreadyExecuted,
    /// Parameter error
    ParameterError(ParameterError),
}

impl From<ParameterError> for MetaGovernanceError {
    fn from(err: ParameterError) -> Self {
        MetaGovernanceError::ParameterError(err)
    }
}

// =============================================================================
// EVENTS
// =============================================================================

/// Events emitted by meta-governance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetaGovernanceEvent {
    /// Proposal created
    ProposalCreated {
        proposal_id: Hash,
        proposer: AccountId,
        proposal_type: String,
    },

    /// Chain signaled support/opposition
    ChainSignaled {
        proposal_id: Hash,
        chain_id: ChainId,
        supports: bool,
    },

    /// Voting started
    VotingStarted {
        proposal_id: Hash,
    },

    /// Vote cast
    VoteCast {
        proposal_id: Hash,
        voter: AccountId,
        support: bool,
        weight: Balance,
    },

    /// Proposal approved
    ProposalApproved {
        proposal_id: Hash,
    },

    /// Proposal rejected
    ProposalRejected {
        proposal_id: Hash,
        reason: String,
    },

    /// Proposal executed
    ProposalExecuted {
        proposal_id: Hash,
        effective_block: BlockNumber,
    },

    /// Proposal cancelled
    ProposalCancelled {
        proposal_id: Hash,
    },
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
    fn test_create_parameter_change_proposal() {
        let mut contract = MetaGovernanceContract::new();
        let proposer = create_account(1);

        contract.set_voting_power(proposer, 100);

        let changes = vec![ParameterChange::InflationRate(3)];
        let result = contract.propose_parameter_change(
            proposer,
            changes,
            "Test rationale".to_string(),
            100,
            1000,
        );

        assert!(result.is_ok());
        let proposal_id = result.unwrap();

        let proposal = contract.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.status, UpgradeStatus::Voting);
    }

    #[test]
    fn test_constitutional_violation_rejected() {
        let mut contract = MetaGovernanceContract::new();
        let proposer = create_account(1);

        contract.set_voting_power(proposer, 100);

        // Try to set inflation to 10% (max is 5%)
        let changes = vec![ParameterChange::InflationRate(10)];
        let result = contract.propose_parameter_change(
            proposer,
            changes,
            "Invalid change".to_string(),
            100,
            1000,
        );

        assert_eq!(result, Err(MetaGovernanceError::ConstitutionalViolation));
    }

    #[test]
    fn test_vote_on_proposal() {
        let mut contract = MetaGovernanceContract::new();
        let proposer = create_account(1);
        let voter1 = create_account(2);
        let voter2 = create_account(3);

        contract.set_voting_power(proposer, 100);
        contract.set_voting_power(voter1, 100);
        contract.set_voting_power(voter2, 100);

        let proposal_id = contract.propose_parameter_change(
            proposer,
            vec![ParameterChange::InflationRate(3)],
            "Test".to_string(),
            100,
            1000,
        ).unwrap();

        // Vote
        assert!(contract.vote(proposal_id, proposer, true, 1100).is_ok());
        assert!(contract.vote(proposal_id, voter1, true, 1100).is_ok());
        assert!(contract.vote(proposal_id, voter2, false, 1100).is_ok());

        let proposal = contract.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.yes_votes, 200);
        assert_eq!(proposal.no_votes, 100);
        assert_eq!(proposal.approval_percentage(), 66);
    }

    #[test]
    fn test_finalize_and_execute() {
        let mut contract = MetaGovernanceContract::new();
        let proposer = create_account(1);

        contract.set_voting_power(proposer, 100);

        let proposal_id = contract.propose_parameter_change(
            proposer,
            vec![ParameterChange::InflationRate(3)],
            "Test".to_string(),
            100,
            1000,
        ).unwrap();

        // Vote with enough support
        contract.vote(proposal_id, proposer, true, 1100).unwrap();

        // Finalize after voting period
        let voting_period = contract.parameters.governance.voting_period.value();
        assert!(contract.finalize_voting(proposal_id, 1100 + voting_period).is_ok());

        let proposal = contract.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.status, UpgradeStatus::Approved);

        // Execute after timelock
        let timelock = proposal.proposal_type.timelock();
        let result = contract.execute_proposal(
            proposal_id,
            1100 + voting_period + timelock,
        );
        assert!(result.is_ok());

        // Check parameter was updated
        assert_eq!(contract.parameters.economics.inflation_rate.value(), 3);
    }

    #[test]
    fn test_version_upgrade_requires_signaling() {
        let mut contract = MetaGovernanceContract::new();
        let proposer = create_account(1);

        contract.set_voting_power(proposer, 100);
        contract.register_chain(ChainId(1), 50, 1000);
        contract.register_chain(ChainId(2), 50, 1000);

        let impact = ImpactAnalysis {
            affected_subsystems: vec!["consensus".to_string()],
            backward_compatible: true,
            risk_level: 2,
            rollback_difficulty: 2,
            required_node_version: Some("2.0.0".to_string()),
            migration_steps: vec![],
            test_summary: "All tests pass".to_string(),
        };

        let proposal_id = contract.propose_version_upgrade(
            proposer,
            ProtocolVersion::new(2, 0, 0),
            "Major upgrade".to_string(),
            false,
            impact,
            100,
            1000,
        ).unwrap();

        let proposal = contract.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.status, UpgradeStatus::Signaling);
    }

    #[test]
    fn test_chain_signaling() {
        let mut contract = MetaGovernanceContract::new();
        let proposer = create_account(1);

        contract.set_voting_power(proposer, 100);
        contract.register_chain(ChainId(1), 50, 1000);
        contract.register_chain(ChainId(2), 50, 1000);

        let impact = ImpactAnalysis {
            affected_subsystems: vec!["consensus".to_string()],
            backward_compatible: true,
            risk_level: 2,
            rollback_difficulty: 2,
            required_node_version: None,
            migration_steps: vec![],
            test_summary: "Tests pass".to_string(),
        };

        let proposal_id = contract.propose_version_upgrade(
            proposer,
            ProtocolVersion::new(2, 0, 0),
            "Upgrade".to_string(),
            false,
            impact,
            100,
            1000,
        ).unwrap();

        // Signal from both chains
        assert!(contract.submit_chain_signal(proposal_id, ChainId(1), true, 1100).is_ok());
        assert!(contract.submit_chain_signal(proposal_id, ChainId(2), true, 1100).is_ok());

        // Advance to voting after signaling period
        let result = contract.advance_to_voting(proposal_id, 1000 + SIGNALING_PERIOD + 1);
        assert!(result.is_ok());

        let proposal = contract.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.status, UpgradeStatus::Voting);
    }

    #[test]
    fn test_batch_parameter_change() {
        let mut contract = MetaGovernanceContract::new();
        let proposer = create_account(1);

        contract.set_voting_power(proposer, 100);

        // Note: fee_burn + fee_validator must sum to <= 100%
        // Default is 50/50, so we change burn to 40 (40+50=90, valid)
        let changes = vec![
            ParameterChange::InflationRate(3),
            ParameterChange::FeeBurnRate(40),
            ParameterChange::TargetValidators(61),
        ];

        let proposal_id = contract.propose_parameter_change(
            proposer,
            changes,
            "Batch update".to_string(),
            100,
            1000,
        ).unwrap();

        // Vote and execute
        contract.vote(proposal_id, proposer, true, 1100).unwrap();
        let voting_period = contract.parameters.governance.voting_period.value();
        contract.finalize_voting(proposal_id, 1100 + voting_period).unwrap();

        let timelock = PROTOCOL_UPGRADE_TIMELOCK;
        contract.execute_proposal(proposal_id, 1100 + voting_period + timelock).unwrap();

        // Verify all changes applied
        assert_eq!(contract.parameters.economics.inflation_rate.value(), 3);
        assert_eq!(contract.parameters.economics.fee_burn_rate.value(), 40);
        assert_eq!(contract.parameters.consensus.target_validators.value(), 61);
    }

    #[test]
    fn test_cancel_proposal() {
        let mut contract = MetaGovernanceContract::new();
        let proposer = create_account(1);
        let other = create_account(2);

        contract.set_voting_power(proposer, 100);

        let proposal_id = contract.propose_parameter_change(
            proposer,
            vec![ParameterChange::InflationRate(3)],
            "Test".to_string(),
            100,
            1000,
        ).unwrap();

        // Non-proposer cannot cancel
        assert_eq!(
            contract.cancel_proposal(proposal_id, &other),
            Err(MetaGovernanceError::NotProposer)
        );

        // Proposer can cancel
        let result = contract.cancel_proposal(proposal_id, &proposer);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 100); // Deposit returned

        let proposal = contract.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.status, UpgradeStatus::Cancelled);
    }

    #[test]
    fn test_parameter_history() {
        let mut contract = MetaGovernanceContract::new();
        let proposer = create_account(1);

        contract.set_voting_power(proposer, 100);

        // Initial inflation
        let initial = contract.parameters.economics.inflation_rate.value();

        // Create and execute change
        let proposal_id = contract.propose_parameter_change(
            proposer,
            vec![ParameterChange::InflationRate(3)],
            "Test".to_string(),
            100,
            1000,
        ).unwrap();

        contract.vote(proposal_id, proposer, true, 1100).unwrap();
        let voting_period = contract.parameters.governance.voting_period.value();
        contract.finalize_voting(proposal_id, 1100 + voting_period).unwrap();
        contract.execute_proposal(proposal_id, 1100 + voting_period + PROTOCOL_UPGRADE_TIMELOCK).unwrap();

        // Check history preserved
        let history = contract.get_parameter_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].1.economics.inflation_rate.value(), initial);
    }
}
