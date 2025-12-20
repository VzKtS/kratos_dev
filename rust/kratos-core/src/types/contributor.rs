// Network Roles & Contributor System
//
// This module implements on-chain role management:
//
// 1. NETWORK ROLES (NetworkRole):
//    - Validator: block production, consensus participation
//    - Juror: arbitration participation
//    - Contributor: treasury-funded programs
//
// 2. CONTRIBUTOR ROLES (ContributorRole):
//    - Bug Bounty hunters
//    - Security researchers
//    - Content creators
//    - Core developers
//    - Ambassadors
//
// Design Philosophy:
// - Roles are ON-CHAIN and verifiable
// - Roles require governance approval (except Validator via staking)
// - Roles enable specific actions (block production, treasury payments)
// - Pseudonymity preserved (AccountId only)

use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash};
use serde::{Deserialize, Serialize};

// =============================================================================
// NETWORK ROLES - Unified role system
// =============================================================================

/// All possible roles an account can hold on the network
/// This unifies consensus roles and contributor roles into a single view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NetworkRole {
    // =========================================================================
    // CONSENSUS ROLES (managed by staking/validator system)
    // =========================================================================

    /// Validator - participates in block production and consensus
    /// Granted via: RegisterValidator transaction + stake requirement
    /// Removed via: UnregisterValidator or slashing
    Validator,

    /// Juror - participates in cross-chain arbitration
    /// Granted via: automatic (validators with sufficient VC)
    /// Duration: per dispute (not permanent role)
    Juror,

    // =========================================================================
    // CONTRIBUTOR ROLES (managed by governance)
    // =========================================================================

    /// Contributor with specific role (treasury-funded)
    /// Granted via: governance proposal
    Contributor(ContributorRole),
}

impl NetworkRole {
    /// Check if this is a consensus role (validator/juror)
    pub fn is_consensus_role(&self) -> bool {
        matches!(self, NetworkRole::Validator | NetworkRole::Juror)
    }

    /// Check if this is a contributor role (treasury-funded)
    pub fn is_contributor_role(&self) -> bool {
        matches!(self, NetworkRole::Contributor(_))
    }

    /// Get the contributor role if applicable
    pub fn as_contributor(&self) -> Option<ContributorRole> {
        match self {
            NetworkRole::Contributor(role) => Some(*role),
            _ => None,
        }
    }

    /// Role display name
    pub fn name(&self) -> &'static str {
        match self {
            NetworkRole::Validator => "Validator",
            NetworkRole::Juror => "Juror",
            NetworkRole::Contributor(role) => role.name(),
        }
    }

    /// Get registration mechanism
    pub fn registration_method(&self) -> RoleRegistrationMethod {
        match self {
            NetworkRole::Validator => RoleRegistrationMethod::StakingTransaction,
            NetworkRole::Juror => RoleRegistrationMethod::AutomaticVCBased,
            NetworkRole::Contributor(_) => RoleRegistrationMethod::GovernanceProposal,
        }
    }
}

/// How a role is granted
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoleRegistrationMethod {
    /// Via RegisterValidator transaction (requires stake)
    StakingTransaction,

    /// Automatic based on Validator Credits threshold
    AutomaticVCBased,

    /// Via governance proposal vote
    GovernanceProposal,
}

// =============================================================================
// NETWORK ROLE ENTRY - Track individual role assignments
// =============================================================================

/// A single role entry in the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRoleEntry {
    /// The role assigned
    pub role: NetworkRole,

    /// Account holding this role
    pub account: AccountId,

    /// Chain scope (None = Root Chain / global)
    pub scope: Option<ChainId>,

    /// Block when role was granted
    pub granted_at: BlockNumber,

    /// Block when role expires (None = no expiry for consensus roles)
    pub expires_at: Option<BlockNumber>,

    /// Current status
    pub active: bool,

    /// Additional metadata (e.g., validator stake, proposal ID)
    pub metadata: RoleMetadata,
}

/// Role-specific metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoleMetadata {
    /// Validator-specific data
    Validator {
        stake: Balance,
        is_bootstrap: bool,
    },

    /// Juror-specific data (per-dispute)
    Juror {
        dispute_id: Hash,
        validator_credits: u32,
    },

    /// Contributor-specific data
    Contributor {
        registration_id: Hash,
        granted_by_proposal: Hash,
    },
}

impl NetworkRoleEntry {
    /// Create a new validator role entry
    pub fn new_validator(
        account: AccountId,
        stake: Balance,
        is_bootstrap: bool,
        current_block: BlockNumber,
    ) -> Self {
        Self {
            role: NetworkRole::Validator,
            account,
            scope: None, // Validators are global (Root Chain)
            granted_at: current_block,
            expires_at: None, // Validators don't expire automatically
            active: true,
            metadata: RoleMetadata::Validator { stake, is_bootstrap },
        }
    }

    /// Create a new juror role entry (per-dispute)
    pub fn new_juror(
        account: AccountId,
        dispute_id: Hash,
        validator_credits: u32,
        current_block: BlockNumber,
    ) -> Self {
        Self {
            role: NetworkRole::Juror,
            account,
            scope: None,
            granted_at: current_block,
            expires_at: None, // Juror role ends when dispute resolves
            active: true,
            metadata: RoleMetadata::Juror {
                dispute_id,
                validator_credits,
            },
        }
    }

    /// Create a contributor role entry from a RoleRegistration
    pub fn from_contributor_registration(reg: &RoleRegistration) -> Self {
        Self {
            role: NetworkRole::Contributor(reg.role),
            account: reg.account,
            scope: reg.scope,
            granted_at: reg.granted_at,
            expires_at: Some(reg.expires_at),
            active: reg.status == RoleStatus::Active,
            metadata: RoleMetadata::Contributor {
                registration_id: reg.registration_id,
                granted_by_proposal: reg.granted_by_proposal,
            },
        }
    }

    /// Update validator stake
    pub fn update_validator_stake(&mut self, new_stake: Balance) {
        if let RoleMetadata::Validator { stake, .. } = &mut self.metadata {
            *stake = new_stake;
        }
    }

    /// Deactivate the role
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Check if role is active at given block
    pub fn is_active_at(&self, current_block: BlockNumber) -> bool {
        if !self.active {
            return false;
        }
        if let Some(expires_at) = self.expires_at {
            return current_block <= expires_at;
        }
        true
    }
}

// =============================================================================
// NETWORK ROLE REGISTRY - Unified role storage
// =============================================================================

use std::collections::HashMap;

/// Registry of all network roles
/// This provides a unified view of all roles (validator, juror, contributor)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkRoleRegistry {
    /// All role entries indexed by account
    /// One account can have multiple roles
    entries: HashMap<AccountId, Vec<NetworkRoleEntry>>,

    /// Quick lookup: validator accounts
    validators: Vec<AccountId>,

    /// Quick lookup: active jurors (by dispute)
    jurors_by_dispute: HashMap<Hash, Vec<AccountId>>,
}

impl NetworkRoleRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            validators: Vec::new(),
            jurors_by_dispute: HashMap::new(),
        }
    }

    // =========================================================================
    // VALIDATOR OPERATIONS
    // =========================================================================

    /// Register a new validator
    pub fn register_validator(
        &mut self,
        account: AccountId,
        stake: Balance,
        is_bootstrap: bool,
        current_block: BlockNumber,
    ) -> Result<(), RoleRegistryError> {
        // Check if already a validator
        if self.is_validator(&account) {
            return Err(RoleRegistryError::AlreadyRegistered);
        }

        let entry = NetworkRoleEntry::new_validator(account, stake, is_bootstrap, current_block);

        self.entries
            .entry(account)
            .or_insert_with(Vec::new)
            .push(entry);

        self.validators.push(account);

        Ok(())
    }

    /// Unregister a validator
    pub fn unregister_validator(&mut self, account: &AccountId) -> Result<(), RoleRegistryError> {
        // Find and deactivate the validator role
        if let Some(entries) = self.entries.get_mut(account) {
            for entry in entries.iter_mut() {
                if matches!(entry.role, NetworkRole::Validator) && entry.active {
                    entry.deactivate();
                    self.validators.retain(|a| a != account);
                    return Ok(());
                }
            }
        }
        Err(RoleRegistryError::NotFound)
    }

    /// Update validator stake
    pub fn update_validator_stake(
        &mut self,
        account: &AccountId,
        new_stake: Balance,
    ) -> Result<(), RoleRegistryError> {
        if let Some(entries) = self.entries.get_mut(account) {
            for entry in entries.iter_mut() {
                if matches!(entry.role, NetworkRole::Validator) && entry.active {
                    entry.update_validator_stake(new_stake);
                    return Ok(());
                }
            }
        }
        Err(RoleRegistryError::NotFound)
    }

    /// Check if account is a validator
    pub fn is_validator(&self, account: &AccountId) -> bool {
        self.validators.contains(account)
    }

    /// Get all active validators
    pub fn get_validators(&self) -> &[AccountId] {
        &self.validators
    }

    /// Get validator entry
    pub fn get_validator_entry(&self, account: &AccountId) -> Option<&NetworkRoleEntry> {
        self.entries.get(account)?.iter().find(|e| {
            matches!(e.role, NetworkRole::Validator) && e.active
        })
    }

    // =========================================================================
    // JUROR OPERATIONS
    // =========================================================================

    /// Register a juror for a dispute
    pub fn register_juror(
        &mut self,
        account: AccountId,
        dispute_id: Hash,
        validator_credits: u32,
        current_block: BlockNumber,
    ) -> Result<(), RoleRegistryError> {
        // Check if already a juror for this dispute
        if let Some(jurors) = self.jurors_by_dispute.get(&dispute_id) {
            if jurors.contains(&account) {
                return Err(RoleRegistryError::AlreadyRegistered);
            }
        }

        let entry = NetworkRoleEntry::new_juror(account, dispute_id, validator_credits, current_block);

        self.entries
            .entry(account)
            .or_insert_with(Vec::new)
            .push(entry);

        self.jurors_by_dispute
            .entry(dispute_id)
            .or_insert_with(Vec::new)
            .push(account);

        Ok(())
    }

    /// Remove jurors when dispute is resolved
    pub fn resolve_dispute_jurors(&mut self, dispute_id: &Hash) {
        if let Some(jurors) = self.jurors_by_dispute.remove(dispute_id) {
            for account in jurors {
                if let Some(entries) = self.entries.get_mut(&account) {
                    for entry in entries.iter_mut() {
                        if let RoleMetadata::Juror { dispute_id: did, .. } = &entry.metadata {
                            if did == dispute_id {
                                entry.deactivate();
                            }
                        }
                    }
                }
            }
        }
    }

    /// Get jurors for a dispute
    pub fn get_dispute_jurors(&self, dispute_id: &Hash) -> Option<&Vec<AccountId>> {
        self.jurors_by_dispute.get(dispute_id)
    }

    // =========================================================================
    // CONTRIBUTOR OPERATIONS
    // =========================================================================

    /// Register a contributor role
    pub fn register_contributor(
        &mut self,
        registration: &RoleRegistration,
    ) -> Result<(), RoleRegistryError> {
        let entry = NetworkRoleEntry::from_contributor_registration(registration);

        self.entries
            .entry(registration.account)
            .or_insert_with(Vec::new)
            .push(entry);

        Ok(())
    }

    /// Update contributor role status
    pub fn update_contributor_status(
        &mut self,
        account: &AccountId,
        registration_id: &Hash,
        active: bool,
    ) -> Result<(), RoleRegistryError> {
        if let Some(entries) = self.entries.get_mut(account) {
            for entry in entries.iter_mut() {
                if let RoleMetadata::Contributor { registration_id: rid, .. } = &entry.metadata {
                    if rid == registration_id {
                        entry.active = active;
                        return Ok(());
                    }
                }
            }
        }
        Err(RoleRegistryError::NotFound)
    }

    // =========================================================================
    // GENERAL QUERIES
    // =========================================================================

    /// Get all roles for an account
    pub fn get_roles(&self, account: &AccountId) -> Option<&Vec<NetworkRoleEntry>> {
        self.entries.get(account)
    }

    /// Get all active roles for an account
    pub fn get_active_roles(&self, account: &AccountId, current_block: BlockNumber) -> Vec<&NetworkRoleEntry> {
        self.entries
            .get(account)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|e| e.is_active_at(current_block))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if account has a specific role type
    pub fn has_role(&self, account: &AccountId, role: &NetworkRole, current_block: BlockNumber) -> bool {
        self.entries
            .get(account)
            .map(|entries| {
                entries.iter().any(|e| {
                    &e.role == role && e.is_active_at(current_block)
                })
            })
            .unwrap_or(false)
    }

    /// Get total number of active validators
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    /// Get all accounts with any active role
    pub fn all_role_holders(&self) -> Vec<&AccountId> {
        self.entries.keys().collect()
    }
}

/// Errors for role registry operations
#[derive(Debug, thiserror::Error)]
pub enum RoleRegistryError {
    #[error("Role already registered")]
    AlreadyRegistered,

    #[error("Role not found")]
    NotFound,

    #[error("Invalid role for operation")]
    InvalidRole,
}

// =============================================================================
// CONSTANTS
// =============================================================================

/// Default role duration (180 days = ~2,592,000 blocks at 6s/block)
pub const DEFAULT_ROLE_DURATION: BlockNumber = 2_592_000;

/// Maximum roles per account
pub const MAX_ROLES_PER_ACCOUNT: usize = 5;

/// Minimum stake for role application (anti-spam)
pub const ROLE_APPLICATION_STAKE: Balance = 10_000_000_000_000; // 10 KRAT

/// Role renewal grace period (14 days = ~201,600 blocks)
pub const ROLE_GRACE_PERIOD: BlockNumber = 201_600;

// =============================================================================
// TREASURY PROGRAMS
// =============================================================================

/// Treasury-funded programs that require contributor roles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TreasuryProgram {
    /// Bug Bounty Program - security vulnerability rewards
    BugBounty,

    /// Security Audit Program - formal security reviews
    SecurityAudit,

    /// Core Development - protocol development
    CoreDevelopment,

    /// Content Creation - documentation, tutorials, marketing
    ContentCreation,

    /// Ambassador Program - community outreach
    Ambassador,

    /// Research Grants - academic/industry research
    ResearchGrant,

    /// Infrastructure - node operators, tooling
    Infrastructure,

    /// Translation - internationalization
    Translation,

    /// Education - training, workshops
    Education,
}

impl TreasuryProgram {
    /// Get all programs
    pub fn all() -> &'static [TreasuryProgram] {
        &[
            TreasuryProgram::BugBounty,
            TreasuryProgram::SecurityAudit,
            TreasuryProgram::CoreDevelopment,
            TreasuryProgram::ContentCreation,
            TreasuryProgram::Ambassador,
            TreasuryProgram::ResearchGrant,
            TreasuryProgram::Infrastructure,
            TreasuryProgram::Translation,
            TreasuryProgram::Education,
        ]
    }

    /// Get program budget allocation percentage from treasury emissions
    /// Total: 100% of the 20% treasury allocation
    pub fn budget_allocation_percent(&self) -> u8 {
        match self {
            TreasuryProgram::BugBounty => 20,
            TreasuryProgram::SecurityAudit => 15,
            TreasuryProgram::CoreDevelopment => 25,
            TreasuryProgram::ContentCreation => 10,
            TreasuryProgram::Ambassador => 8,
            TreasuryProgram::ResearchGrant => 10,
            TreasuryProgram::Infrastructure => 5,
            TreasuryProgram::Translation => 4,
            TreasuryProgram::Education => 3,
        }
    }

    /// Get required approval threshold for role grants
    /// Higher security programs require more votes
    pub fn approval_threshold_percent(&self) -> u8 {
        match self {
            TreasuryProgram::BugBounty => 51,
            TreasuryProgram::SecurityAudit => 67,      // High trust required
            TreasuryProgram::CoreDevelopment => 67,    // High trust required
            TreasuryProgram::ContentCreation => 51,
            TreasuryProgram::Ambassador => 51,
            TreasuryProgram::ResearchGrant => 51,
            TreasuryProgram::Infrastructure => 51,
            TreasuryProgram::Translation => 51,
            TreasuryProgram::Education => 51,
        }
    }

    /// Get maximum payment per contribution (in KRAT base units)
    pub fn max_payment_per_contribution(&self) -> Balance {
        const KRAT: Balance = 1_000_000_000_000;
        match self {
            TreasuryProgram::BugBounty => 100_000 * KRAT,        // Critical vulns
            TreasuryProgram::SecurityAudit => 50_000 * KRAT,
            TreasuryProgram::CoreDevelopment => 25_000 * KRAT,
            TreasuryProgram::ContentCreation => 5_000 * KRAT,
            TreasuryProgram::Ambassador => 2_000 * KRAT,
            TreasuryProgram::ResearchGrant => 50_000 * KRAT,
            TreasuryProgram::Infrastructure => 10_000 * KRAT,
            TreasuryProgram::Translation => 1_000 * KRAT,
            TreasuryProgram::Education => 5_000 * KRAT,
        }
    }
}

// =============================================================================
// CONTRIBUTOR ROLES
// =============================================================================

/// Contributor role - official on-chain status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContributorRole {
    /// Bug Hunter - can submit bug bounty claims
    BugHunter,

    /// Security Auditor - can submit audit reports
    SecurityAuditor,

    /// Core Developer - can receive dev payments
    CoreDeveloper,

    /// Content Creator - can receive content payments
    ContentCreator,

    /// Ambassador - can receive outreach payments
    Ambassador,

    /// Researcher - can receive research grants
    Researcher,

    /// Infrastructure Provider - node operators
    InfrastructureProvider,

    /// Translator - localization work
    Translator,

    /// Educator - training and education
    Educator,
}

impl ContributorRole {
    /// Map role to its associated program
    pub fn program(&self) -> TreasuryProgram {
        match self {
            ContributorRole::BugHunter => TreasuryProgram::BugBounty,
            ContributorRole::SecurityAuditor => TreasuryProgram::SecurityAudit,
            ContributorRole::CoreDeveloper => TreasuryProgram::CoreDevelopment,
            ContributorRole::ContentCreator => TreasuryProgram::ContentCreation,
            ContributorRole::Ambassador => TreasuryProgram::Ambassador,
            ContributorRole::Researcher => TreasuryProgram::ResearchGrant,
            ContributorRole::InfrastructureProvider => TreasuryProgram::Infrastructure,
            ContributorRole::Translator => TreasuryProgram::Translation,
            ContributorRole::Educator => TreasuryProgram::Education,
        }
    }

    /// Role display name
    pub fn name(&self) -> &'static str {
        match self {
            ContributorRole::BugHunter => "Bug Hunter",
            ContributorRole::SecurityAuditor => "Security Auditor",
            ContributorRole::CoreDeveloper => "Core Developer",
            ContributorRole::ContentCreator => "Content Creator",
            ContributorRole::Ambassador => "Ambassador",
            ContributorRole::Researcher => "Researcher",
            ContributorRole::InfrastructureProvider => "Infrastructure Provider",
            ContributorRole::Translator => "Translator",
            ContributorRole::Educator => "Educator",
        }
    }
}

// =============================================================================
// ROLE STATUS
// =============================================================================

/// Status of a contributor role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoleStatus {
    /// Application submitted, awaiting governance vote
    Pending,

    /// Role is active and valid
    Active,

    /// Role expired (can be renewed)
    Expired,

    /// Role suspended (misconduct)
    Suspended,

    /// Role revoked (permanent)
    Revoked,
}

impl RoleStatus {
    /// Can this status receive payments?
    pub fn can_receive_payment(&self) -> bool {
        matches!(self, RoleStatus::Active)
    }

    /// Can this status be renewed?
    pub fn can_renew(&self) -> bool {
        matches!(self, RoleStatus::Active | RoleStatus::Expired)
    }
}

// =============================================================================
// ROLE REGISTRATION
// =============================================================================

/// Role registration - on-chain contributor status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleRegistration {
    /// Unique registration ID
    pub registration_id: Hash,

    /// Account holding this role (pseudonymous)
    pub account: AccountId,

    /// The role granted
    pub role: ContributorRole,

    /// Chain scope (None = Root Chain / global)
    pub scope: Option<ChainId>,

    /// Current status
    pub status: RoleStatus,

    /// Block when role was granted
    pub granted_at: BlockNumber,

    /// Block when role expires
    pub expires_at: BlockNumber,

    /// Proposal ID that granted this role
    pub granted_by_proposal: Hash,

    /// Total payments received
    pub total_payments: Balance,

    /// Number of contributions
    pub contribution_count: u32,

    /// Last activity block
    pub last_activity: BlockNumber,

    /// Optional public alias (not identity-linked)
    pub alias: Option<String>,
}

impl RoleRegistration {
    /// Create a new role registration
    pub fn new(
        account: AccountId,
        role: ContributorRole,
        scope: Option<ChainId>,
        granted_by_proposal: Hash,
        current_block: BlockNumber,
        alias: Option<String>,
    ) -> Self {
        let registration_id = Self::compute_id(&account, &role, current_block);

        Self {
            registration_id,
            account,
            role,
            scope,
            status: RoleStatus::Active,
            granted_at: current_block,
            expires_at: current_block + DEFAULT_ROLE_DURATION,
            granted_by_proposal,
            total_payments: 0,
            contribution_count: 0,
            last_activity: current_block,
            alias,
        }
    }

    /// Compute registration ID
    fn compute_id(account: &AccountId, role: &ContributorRole, block: BlockNumber) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(account.as_bytes());
        data.push(*role as u8);
        data.extend_from_slice(&block.to_le_bytes());
        Hash::hash(&data)
    }

    /// Check if role is active
    pub fn is_active(&self, current_block: BlockNumber) -> bool {
        self.status == RoleStatus::Active && !self.is_expired(current_block)
    }

    /// Check if role is expired
    pub fn is_expired(&self, current_block: BlockNumber) -> bool {
        current_block > self.expires_at
    }

    /// Check if within grace period for renewal
    pub fn in_grace_period(&self, current_block: BlockNumber) -> bool {
        let grace_end = self.expires_at + ROLE_GRACE_PERIOD;
        current_block > self.expires_at && current_block <= grace_end
    }

    /// Renew the role
    pub fn renew(&mut self, current_block: BlockNumber) -> bool {
        if !self.status.can_renew() {
            return false;
        }

        self.expires_at = current_block + DEFAULT_ROLE_DURATION;
        self.status = RoleStatus::Active;
        true
    }

    /// Record a contribution
    pub fn record_contribution(&mut self, payment: Balance, current_block: BlockNumber) {
        self.contribution_count += 1;
        self.total_payments = self.total_payments.saturating_add(payment);
        self.last_activity = current_block;
    }

    /// Suspend the role
    pub fn suspend(&mut self) {
        self.status = RoleStatus::Suspended;
    }

    /// Revoke the role (permanent)
    pub fn revoke(&mut self) {
        self.status = RoleStatus::Revoked;
    }

    /// Reactivate from suspension
    pub fn reactivate(&mut self, current_block: BlockNumber) -> bool {
        if self.status != RoleStatus::Suspended {
            return false;
        }
        self.status = RoleStatus::Active;
        self.last_activity = current_block;
        true
    }
}

// =============================================================================
// ROLE APPLICATION
// =============================================================================

/// Application for a contributor role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleApplication {
    /// Unique application ID
    pub application_id: Hash,

    /// Applicant account
    pub applicant: AccountId,

    /// Role being applied for
    pub role: ContributorRole,

    /// Chain scope
    pub scope: Option<ChainId>,

    /// Application reason/qualifications (hash of off-chain document)
    pub justification_hash: Hash,

    /// Stake deposited (refunded on approval, slashed on rejection for spam)
    pub stake: Balance,

    /// Block when application was submitted
    pub submitted_at: BlockNumber,

    /// Governance proposal ID (created for voting)
    pub proposal_id: Option<Hash>,

    /// Application status
    pub status: ApplicationStatus,

    /// Optional public alias
    pub alias: Option<String>,
}

/// Application status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplicationStatus {
    /// Submitted, awaiting proposal creation
    Submitted,

    /// Proposal created, voting in progress
    Voting,

    /// Approved - role granted
    Approved,

    /// Rejected - stake returned
    Rejected,

    /// Rejected as spam - stake slashed
    RejectedSpam,

    /// Withdrawn by applicant
    Withdrawn,
}

impl RoleApplication {
    /// Create a new application
    pub fn new(
        applicant: AccountId,
        role: ContributorRole,
        scope: Option<ChainId>,
        justification_hash: Hash,
        stake: Balance,
        current_block: BlockNumber,
        alias: Option<String>,
    ) -> Option<Self> {
        if stake < ROLE_APPLICATION_STAKE {
            return None;
        }

        let application_id = Self::compute_id(&applicant, &role, current_block);

        Some(Self {
            application_id,
            applicant,
            role,
            scope,
            justification_hash,
            stake,
            submitted_at: current_block,
            proposal_id: None,
            status: ApplicationStatus::Submitted,
            alias,
        })
    }

    /// Compute application ID
    fn compute_id(applicant: &AccountId, role: &ContributorRole, block: BlockNumber) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(applicant.as_bytes());
        data.push(*role as u8);
        data.extend_from_slice(&block.to_le_bytes());
        data.extend_from_slice(b"APPLICATION");
        Hash::hash(&data)
    }

    /// Set governance proposal ID
    pub fn set_proposal(&mut self, proposal_id: Hash) {
        self.proposal_id = Some(proposal_id);
        self.status = ApplicationStatus::Voting;
    }

    /// Approve the application
    pub fn approve(&mut self) {
        self.status = ApplicationStatus::Approved;
    }

    /// Reject the application (stake returned)
    pub fn reject(&mut self) {
        self.status = ApplicationStatus::Rejected;
    }

    /// Reject as spam (stake slashed)
    pub fn reject_spam(&mut self) {
        self.status = ApplicationStatus::RejectedSpam;
    }

    /// Withdraw application (by applicant)
    pub fn withdraw(&mut self) -> bool {
        if self.status != ApplicationStatus::Submitted {
            return false;
        }
        self.status = ApplicationStatus::Withdrawn;
        true
    }
}

// =============================================================================
// CONTRIBUTION CLAIM
// =============================================================================

/// A claim for payment from a treasury program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionClaim {
    /// Unique claim ID
    pub claim_id: Hash,

    /// Contributor making the claim
    pub contributor: AccountId,

    /// Registration ID of the role
    pub registration_id: Hash,

    /// Program being claimed from
    pub program: TreasuryProgram,

    /// Amount requested
    pub amount: Balance,

    /// Description/evidence hash (off-chain document)
    pub evidence_hash: Hash,

    /// Block when claim was submitted
    pub submitted_at: BlockNumber,

    /// Claim status
    pub status: ClaimStatus,

    /// Governance proposal ID for approval
    pub proposal_id: Option<Hash>,

    /// Severity level (for bug bounty)
    pub severity: Option<BugSeverity>,
}

/// Claim status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClaimStatus {
    /// Submitted, awaiting review
    Submitted,

    /// Under governance review
    UnderReview,

    /// Approved, payment pending
    Approved,

    /// Paid out
    Paid,

    /// Rejected
    Rejected,

    /// Disputed (under arbitration)
    Disputed,
}

/// Bug severity levels (for bug bounty)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BugSeverity {
    /// Low - minor issues
    Low,

    /// Medium - moderate impact
    Medium,

    /// High - significant impact
    High,

    /// Critical - severe/exploitable
    Critical,
}

impl BugSeverity {
    /// Get reward multiplier (percentage of max payment)
    pub fn reward_percent(&self) -> u8 {
        match self {
            BugSeverity::Low => 5,
            BugSeverity::Medium => 20,
            BugSeverity::High => 50,
            BugSeverity::Critical => 100,
        }
    }
}

impl ContributionClaim {
    /// Create a new contribution claim
    pub fn new(
        contributor: AccountId,
        registration_id: Hash,
        program: TreasuryProgram,
        amount: Balance,
        evidence_hash: Hash,
        current_block: BlockNumber,
        severity: Option<BugSeverity>,
    ) -> Option<Self> {
        // Validate amount against program max
        let max_payment = program.max_payment_per_contribution();
        if amount > max_payment {
            return None;
        }

        let claim_id = Self::compute_id(&contributor, &evidence_hash, current_block);

        Some(Self {
            claim_id,
            contributor,
            registration_id,
            program,
            amount,
            evidence_hash,
            submitted_at: current_block,
            status: ClaimStatus::Submitted,
            proposal_id: None,
            severity,
        })
    }

    /// Compute claim ID
    fn compute_id(contributor: &AccountId, evidence: &Hash, block: BlockNumber) -> Hash {
        let mut data = Vec::new();
        data.extend_from_slice(contributor.as_bytes());
        data.extend_from_slice(evidence.as_bytes());
        data.extend_from_slice(&block.to_le_bytes());
        Hash::hash(&data)
    }

    /// Set governance proposal
    pub fn set_proposal(&mut self, proposal_id: Hash) {
        self.proposal_id = Some(proposal_id);
        self.status = ClaimStatus::UnderReview;
    }

    /// Approve claim
    pub fn approve(&mut self) {
        self.status = ClaimStatus::Approved;
    }

    /// Mark as paid
    pub fn mark_paid(&mut self) {
        self.status = ClaimStatus::Paid;
    }

    /// Reject claim
    pub fn reject(&mut self) {
        self.status = ClaimStatus::Rejected;
    }

    /// Mark as disputed
    pub fn dispute(&mut self) {
        self.status = ClaimStatus::Disputed;
    }
}

// =============================================================================
// EVENTS
// =============================================================================

/// Events emitted by the contributor system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContributorEvent {
    /// Role application submitted
    ApplicationSubmitted {
        application_id: Hash,
        applicant: AccountId,
        role: ContributorRole,
    },

    /// Role granted
    RoleGranted {
        registration_id: Hash,
        account: AccountId,
        role: ContributorRole,
        expires_at: BlockNumber,
    },

    /// Role renewed
    RoleRenewed {
        registration_id: Hash,
        new_expiry: BlockNumber,
    },

    /// Role suspended
    RoleSuspended {
        registration_id: Hash,
        reason: String,
    },

    /// Role revoked
    RoleRevoked {
        registration_id: Hash,
        reason: String,
    },

    /// Contribution claim submitted
    ClaimSubmitted {
        claim_id: Hash,
        contributor: AccountId,
        program: TreasuryProgram,
        amount: Balance,
    },

    /// Claim approved
    ClaimApproved {
        claim_id: Hash,
        amount: Balance,
    },

    /// Claim paid
    ClaimPaid {
        claim_id: Hash,
        contributor: AccountId,
        amount: Balance,
    },

    /// Claim rejected
    ClaimRejected {
        claim_id: Hash,
        reason: String,
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

    fn create_hash(seed: u8) -> Hash {
        Hash::hash(&[seed; 32])
    }

    #[test]
    fn test_program_budget_allocation() {
        let total: u8 = TreasuryProgram::all()
            .iter()
            .map(|p| p.budget_allocation_percent())
            .sum();
        assert_eq!(total, 100);
    }

    #[test]
    fn test_role_registration() {
        let account = create_account(1);
        let proposal = create_hash(1);

        let reg = RoleRegistration::new(
            account,
            ContributorRole::BugHunter,
            None,
            proposal,
            1000,
            Some("AliceHunter".to_string()),
        );

        assert!(reg.is_active(1000));
        assert!(!reg.is_expired(1000));
        assert_eq!(reg.role, ContributorRole::BugHunter);
        assert_eq!(reg.alias, Some("AliceHunter".to_string()));
    }

    #[test]
    fn test_role_expiry() {
        let account = create_account(1);
        let proposal = create_hash(1);

        let reg = RoleRegistration::new(
            account,
            ContributorRole::CoreDeveloper,
            None,
            proposal,
            1000,
            None,
        );

        assert!(reg.is_active(1000));
        assert!(reg.is_active(1000 + DEFAULT_ROLE_DURATION - 1));
        assert!(!reg.is_active(1000 + DEFAULT_ROLE_DURATION + 1));
        assert!(reg.is_expired(1000 + DEFAULT_ROLE_DURATION + 1));
    }

    #[test]
    fn test_role_renewal() {
        let account = create_account(1);
        let proposal = create_hash(1);

        let mut reg = RoleRegistration::new(
            account,
            ContributorRole::Ambassador,
            None,
            proposal,
            1000,
            None,
        );

        let old_expiry = reg.expires_at;
        assert!(reg.renew(2000));
        assert!(reg.expires_at > old_expiry);
    }

    #[test]
    fn test_role_suspension_and_reactivation() {
        let account = create_account(1);
        let proposal = create_hash(1);

        let mut reg = RoleRegistration::new(
            account,
            ContributorRole::ContentCreator,
            None,
            proposal,
            1000,
            None,
        );

        assert!(reg.is_active(1000));

        reg.suspend();
        assert!(!reg.is_active(1000));
        assert_eq!(reg.status, RoleStatus::Suspended);

        assert!(reg.reactivate(2000));
        assert!(reg.is_active(2000));
    }

    #[test]
    fn test_role_revocation() {
        let account = create_account(1);
        let proposal = create_hash(1);

        let mut reg = RoleRegistration::new(
            account,
            ContributorRole::SecurityAuditor,
            None,
            proposal,
            1000,
            None,
        );

        reg.revoke();
        assert!(!reg.is_active(1000));
        assert!(!reg.status.can_renew());
    }

    #[test]
    fn test_role_application_min_stake() {
        let app = RoleApplication::new(
            create_account(1),
            ContributorRole::Researcher,
            None,
            create_hash(1),
            ROLE_APPLICATION_STAKE - 1, // Below minimum
            1000,
            None,
        );

        assert!(app.is_none());
    }

    #[test]
    fn test_role_application_valid() {
        let app = RoleApplication::new(
            create_account(1),
            ContributorRole::Educator,
            None,
            create_hash(1),
            ROLE_APPLICATION_STAKE,
            1000,
            Some("TeacherBob".to_string()),
        );

        assert!(app.is_some());
        let app = app.unwrap();
        assert_eq!(app.status, ApplicationStatus::Submitted);
    }

    #[test]
    fn test_contribution_claim_max_amount() {
        let program = TreasuryProgram::BugBounty;
        let max = program.max_payment_per_contribution();

        // Over max should fail
        let claim = ContributionClaim::new(
            create_account(1),
            create_hash(1),
            program,
            max + 1,
            create_hash(2),
            1000,
            Some(BugSeverity::Critical),
        );
        assert!(claim.is_none());

        // At max should succeed
        let claim = ContributionClaim::new(
            create_account(1),
            create_hash(1),
            program,
            max,
            create_hash(2),
            1000,
            Some(BugSeverity::Critical),
        );
        assert!(claim.is_some());
    }

    #[test]
    fn test_bug_severity_rewards() {
        assert_eq!(BugSeverity::Low.reward_percent(), 5);
        assert_eq!(BugSeverity::Medium.reward_percent(), 20);
        assert_eq!(BugSeverity::High.reward_percent(), 50);
        assert_eq!(BugSeverity::Critical.reward_percent(), 100);
    }

    #[test]
    fn test_role_program_mapping() {
        assert_eq!(
            ContributorRole::BugHunter.program(),
            TreasuryProgram::BugBounty
        );
        assert_eq!(
            ContributorRole::CoreDeveloper.program(),
            TreasuryProgram::CoreDevelopment
        );
    }

    #[test]
    fn test_record_contribution() {
        let account = create_account(1);
        let proposal = create_hash(1);

        let mut reg = RoleRegistration::new(
            account,
            ContributorRole::BugHunter,
            None,
            proposal,
            1000,
            None,
        );

        assert_eq!(reg.contribution_count, 0);
        assert_eq!(reg.total_payments, 0);

        reg.record_contribution(1000, 2000);
        assert_eq!(reg.contribution_count, 1);
        assert_eq!(reg.total_payments, 1000);

        reg.record_contribution(500, 3000);
        assert_eq!(reg.contribution_count, 2);
        assert_eq!(reg.total_payments, 1500);
    }

    // =========================================================================
    // NetworkRole Tests
    // =========================================================================

    #[test]
    fn test_network_role_validator() {
        let role = NetworkRole::Validator;

        assert!(role.is_consensus_role());
        assert!(!role.is_contributor_role());
        assert_eq!(role.name(), "Validator");
        assert_eq!(role.registration_method(), RoleRegistrationMethod::StakingTransaction);
        assert!(role.as_contributor().is_none());
    }

    #[test]
    fn test_network_role_juror() {
        let role = NetworkRole::Juror;

        assert!(role.is_consensus_role());
        assert!(!role.is_contributor_role());
        assert_eq!(role.name(), "Juror");
        assert_eq!(role.registration_method(), RoleRegistrationMethod::AutomaticVCBased);
    }

    #[test]
    fn test_network_role_contributor() {
        let role = NetworkRole::Contributor(ContributorRole::CoreDeveloper);

        assert!(!role.is_consensus_role());
        assert!(role.is_contributor_role());
        assert_eq!(role.name(), "Core Developer");
        assert_eq!(role.registration_method(), RoleRegistrationMethod::GovernanceProposal);
        assert_eq!(role.as_contributor(), Some(ContributorRole::CoreDeveloper));
    }

    #[test]
    fn test_all_network_roles() {
        // Test all contributor roles can be wrapped in NetworkRole
        for contributor_role in [
            ContributorRole::BugHunter,
            ContributorRole::SecurityAuditor,
            ContributorRole::CoreDeveloper,
            ContributorRole::ContentCreator,
            ContributorRole::Ambassador,
            ContributorRole::Researcher,
            ContributorRole::InfrastructureProvider,
            ContributorRole::Translator,
            ContributorRole::Educator,
        ] {
            let network_role = NetworkRole::Contributor(contributor_role);
            assert!(network_role.is_contributor_role());
            assert_eq!(network_role.as_contributor(), Some(contributor_role));
        }
    }
}
