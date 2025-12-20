// Identity Registry - SPEC v4 Phase 0
// Local identity management with attestation-based activation
//
// Lifecycle:
// 1. declare_identity() -> Declared
// 2. submit_attestation() x3+ -> Attested -> Active
// 3. renew_identity() (before expiry) -> extends expiry
// 4. revoke_identity() (voluntary or enforced) -> Revoked

use crate::types::identity::{
    AntiSybilConfig, IdentityAttestation, IdentityCommitment, IdentityEvent, IdentityId,
    IdentityStatus, ReputationScore, RevocationReason,
    calculate_attestation_weight, MIN_ATTESTATIONS_FOR_ACTIVE, MAX_ATTESTATIONS,
};
use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Identity declaration deposit (refundable on voluntary revocation)
pub const IDENTITY_DEPOSIT: Balance = 10;

/// Reputation points for various actions
pub const REPUTATION_POINTS_GOVERNANCE: u32 = 10;
pub const REPUTATION_POINTS_ARBITRATION: u32 = 50;
pub const REPUTATION_POINTS_ATTESTATION: u32 = 5;

/// Identity Registry - manages identities for a chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityRegistry {
    /// Chain this registry belongs to
    chain_id: ChainId,

    /// All identities by ID
    identities: HashMap<IdentityId, IdentityCommitment>,

    /// Owner to identity mapping (one identity per owner per chain)
    owner_to_identity: HashMap<AccountId, IdentityId>,

    /// Attestations per identity
    attestations: HashMap<IdentityId, Vec<IdentityAttestation>>,

    /// Who has attested whom (prevents double attestation)
    attestation_pairs: HashSet<(AccountId, IdentityId)>,

    /// Anti-Sybil configuration
    config: AntiSybilConfig,

    /// Events emitted (for indexing)
    events: Vec<IdentityEvent>,

    /// Total active identities
    active_count: u64,
}

impl IdentityRegistry {
    /// Create a new identity registry for a chain
    pub fn new(chain_id: ChainId) -> Self {
        Self {
            chain_id,
            identities: HashMap::new(),
            owner_to_identity: HashMap::new(),
            attestations: HashMap::new(),
            attestation_pairs: HashSet::new(),
            config: AntiSybilConfig::default(),
            events: Vec::new(),
            active_count: 0,
        }
    }

    /// Create registry with custom configuration
    pub fn with_config(chain_id: ChainId, config: AntiSybilConfig) -> Self {
        Self {
            chain_id,
            config,
            ..Self::new(chain_id)
        }
    }

    /// Get chain ID
    pub fn chain_id(&self) -> ChainId {
        self.chain_id
    }

    /// Get anti-Sybil configuration
    pub fn config(&self) -> &AntiSybilConfig {
        &self.config
    }

    /// Update configuration
    pub fn update_config(&mut self, config: AntiSybilConfig) {
        self.config = config;
    }

    /// Get total active identities
    pub fn active_count(&self) -> u64 {
        self.active_count
    }

    // =========================================================================
    // IDENTITY DECLARATION
    // =========================================================================

    /// Declare a new identity
    ///
    /// Creates an identity in Declared state. Requires attestations to become Active.
    pub fn declare_identity(
        &mut self,
        owner: AccountId,
        commitment_data: &[u8],
        display_name: Option<String>,
        current_block: BlockNumber,
    ) -> Result<IdentityId, IdentityError> {
        // Check owner doesn't already have an identity
        if self.owner_to_identity.contains_key(&owner) {
            return Err(IdentityError::IdentityAlreadyExists);
        }

        // Create identity
        let identity = IdentityCommitment::new(
            owner,
            self.chain_id,
            commitment_data,
            display_name,
            current_block,
        );

        let identity_id = identity.identity_id;

        // Store identity
        self.identities.insert(identity_id, identity);
        self.owner_to_identity.insert(owner, identity_id);
        self.attestations.insert(identity_id, Vec::new());

        // Emit event
        self.events.push(IdentityEvent::IdentityDeclared {
            identity_id,
            owner,
            scope: self.chain_id,
        });

        Ok(identity_id)
    }

    // =========================================================================
    // ATTESTATIONS
    // =========================================================================

    /// Submit an attestation for an identity
    ///
    /// Attester must have an Active identity. Attestation weight is based on
    /// attester's reputation. Identity transitions to Active after MIN_ATTESTATIONS_FOR_ACTIVE.
    pub fn submit_attestation(
        &mut self,
        attester: AccountId,
        target_identity_id: IdentityId,
        claim_data: &[u8],
        current_block: BlockNumber,
    ) -> Result<(), IdentityError> {
        // Get attester's identity
        let attester_identity_id = self.owner_to_identity
            .get(&attester)
            .ok_or(IdentityError::AttesterHasNoIdentity)?;

        let attester_identity = self.identities
            .get(attester_identity_id)
            .ok_or(IdentityError::IdentityNotFound)?;

        // Attester must be Active
        if !attester_identity.is_active(current_block) {
            return Err(IdentityError::AttesterNotActive);
        }

        // Cannot self-attest
        if *attester_identity_id == target_identity_id {
            return Err(IdentityError::CannotSelfAttest);
        }

        // Check target identity exists and can receive attestations
        let target = self.identities
            .get(&target_identity_id)
            .ok_or(IdentityError::IdentityNotFound)?;

        if !target.status.can_receive_attestations() {
            return Err(IdentityError::CannotReceiveAttestations);
        }

        // Check not already attested
        let pair = (attester, target_identity_id);
        if self.attestation_pairs.contains(&pair) {
            return Err(IdentityError::AlreadyAttested);
        }

        // Check max attestations
        let attestations = self.attestations
            .get(&target_identity_id)
            .map(|a| a.len())
            .unwrap_or(0);

        if attestations >= MAX_ATTESTATIONS {
            return Err(IdentityError::MaxAttestationsReached);
        }

        // Calculate weight based on attester's reputation
        let weight = calculate_attestation_weight(attester_identity.reputation.score);

        // Create attestation
        let attestation = IdentityAttestation::new(
            attester,
            *attester_identity_id,
            target_identity_id,
            claim_data,
            weight,
            current_block,
        );

        // Store attestation
        self.attestations
            .entry(target_identity_id)
            .or_default()
            .push(attestation);
        self.attestation_pairs.insert(pair);

        // Reward attester with reputation
        if let Some(attester_id) = self.owner_to_identity.get(&attester).copied() {
            if let Some(identity) = self.identities.get_mut(&attester_id) {
                identity.reputation.record_attestation(REPUTATION_POINTS_ATTESTATION);
            }
        }

        // Emit event
        self.events.push(IdentityEvent::AttestationReceived {
            identity_id: target_identity_id,
            attester,
            weight,
        });

        // Check if target should transition to Active
        self.maybe_activate_identity(target_identity_id, current_block)?;

        Ok(())
    }

    /// Count valid attestations for an identity
    pub fn count_valid_attestations(
        &self,
        identity_id: IdentityId,
        current_block: BlockNumber,
    ) -> usize {
        self.attestations
            .get(&identity_id)
            .map(|atts| atts.iter().filter(|a| a.is_active(current_block)).count())
            .unwrap_or(0)
    }

    /// Calculate total attestation weight for an identity
    pub fn total_attestation_weight(
        &self,
        identity_id: IdentityId,
        current_block: BlockNumber,
    ) -> u32 {
        self.attestations
            .get(&identity_id)
            .map(|atts| {
                atts.iter()
                    .filter(|a| a.is_active(current_block))
                    .map(|a| a.weight as u32)
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Maybe transition identity to Active if enough attestations
    fn maybe_activate_identity(
        &mut self,
        identity_id: IdentityId,
        current_block: BlockNumber,
    ) -> Result<(), IdentityError> {
        let count = self.count_valid_attestations(identity_id, current_block);
        let min_required = self.config.min_attestations;

        let identity = self.identities
            .get_mut(&identity_id)
            .ok_or(IdentityError::IdentityNotFound)?;

        match identity.status {
            IdentityStatus::Declared if count > 0 => {
                identity.status = IdentityStatus::Attested;
            }
            IdentityStatus::Declared | IdentityStatus::Attested if count >= min_required => {
                identity.status = IdentityStatus::Active;
                self.active_count += 1;
                self.events.push(IdentityEvent::IdentityActivated { identity_id });
            }
            _ => {}
        }

        Ok(())
    }

    // =========================================================================
    // IDENTITY QUERIES
    // =========================================================================

    /// Get identity by ID
    pub fn get_identity(&self, identity_id: &IdentityId) -> Option<&IdentityCommitment> {
        self.identities.get(identity_id)
    }

    /// Get identity by owner
    pub fn get_identity_by_owner(&self, owner: &AccountId) -> Option<&IdentityCommitment> {
        self.owner_to_identity
            .get(owner)
            .and_then(|id| self.identities.get(id))
    }

    /// Get identity ID by owner
    pub fn get_identity_id(&self, owner: &AccountId) -> Option<IdentityId> {
        self.owner_to_identity.get(owner).copied()
    }

    /// Check if owner has an identity
    pub fn has_identity(&self, owner: &AccountId) -> bool {
        self.owner_to_identity.contains_key(owner)
    }

    /// Check if identity is active
    pub fn is_active(&self, identity_id: &IdentityId, current_block: BlockNumber) -> bool {
        self.identities
            .get(identity_id)
            .map(|i| i.is_active(current_block))
            .unwrap_or(false)
    }

    /// Check if owner has active identity
    pub fn owner_is_active(&self, owner: &AccountId, current_block: BlockNumber) -> bool {
        self.owner_to_identity
            .get(owner)
            .and_then(|id| self.identities.get(id))
            .map(|i| i.is_active(current_block))
            .unwrap_or(false)
    }

    /// Get all attestations for an identity
    pub fn get_attestations(&self, identity_id: &IdentityId) -> Option<&Vec<IdentityAttestation>> {
        self.attestations.get(identity_id)
    }

    // =========================================================================
    // IDENTITY LIFECYCLE
    // =========================================================================

    /// Renew an identity before expiry
    pub fn renew_identity(
        &mut self,
        owner: AccountId,
        current_block: BlockNumber,
    ) -> Result<BlockNumber, IdentityError> {
        let identity_id = *self.owner_to_identity
            .get(&owner)
            .ok_or(IdentityError::IdentityNotFound)?;

        // Check if revoked first (immutable borrow)
        {
            let identity = self.identities
                .get(&identity_id)
                .ok_or(IdentityError::IdentityNotFound)?;
            if identity.status == IdentityStatus::Revoked {
                return Err(IdentityError::IdentityRevoked);
            }
        }

        // Count attestations if needed (immutable borrow)
        let was_expired = {
            let identity = self.identities
                .get(&identity_id)
                .ok_or(IdentityError::IdentityNotFound)?;
            identity.status == IdentityStatus::Expired
        };

        let new_status = if was_expired {
            let count = self.count_valid_attestations(identity_id, current_block);
            if count >= self.config.min_attestations {
                Some(IdentityStatus::Active)
            } else if count > 0 {
                Some(IdentityStatus::Attested)
            } else {
                Some(IdentityStatus::Declared)
            }
        } else {
            None
        };

        // Now do mutable operations
        let identity = self.identities
            .get_mut(&identity_id)
            .ok_or(IdentityError::IdentityNotFound)?;

        // Renew
        identity.renew(current_block);

        // If was expired, restore previous status
        if let Some(status) = new_status {
            identity.status = status;
        }

        let new_expiry = identity.expires_at;

        self.events.push(IdentityEvent::IdentityRenewed {
            identity_id,
            new_expiry,
        });

        Ok(new_expiry)
    }

    /// Voluntarily revoke own identity
    pub fn revoke_own_identity(
        &mut self,
        owner: AccountId,
    ) -> Result<Balance, IdentityError> {
        let identity_id = self.owner_to_identity
            .get(&owner)
            .ok_or(IdentityError::IdentityNotFound)?;

        let identity = self.identities
            .get_mut(identity_id)
            .ok_or(IdentityError::IdentityNotFound)?;

        // Cannot revoke already revoked
        if identity.status == IdentityStatus::Revoked {
            return Err(IdentityError::AlreadyRevoked);
        }

        // Track if was active
        let was_active = identity.status == IdentityStatus::Active;

        // Revoke
        identity.status = IdentityStatus::Revoked;

        if was_active {
            self.active_count = self.active_count.saturating_sub(1);
        }

        self.events.push(IdentityEvent::IdentityRevoked {
            identity_id: *identity_id,
            reason: RevocationReason::Voluntary,
        });

        // Return deposit
        Ok(IDENTITY_DEPOSIT)
    }

    /// Forcefully revoke identity (governance action or misconduct)
    pub fn force_revoke_identity(
        &mut self,
        identity_id: IdentityId,
        reason: RevocationReason,
    ) -> Result<(), IdentityError> {
        let identity = self.identities
            .get_mut(&identity_id)
            .ok_or(IdentityError::IdentityNotFound)?;

        if identity.status == IdentityStatus::Revoked {
            return Err(IdentityError::AlreadyRevoked);
        }

        let was_active = identity.status == IdentityStatus::Active;
        identity.status = IdentityStatus::Revoked;

        if was_active {
            self.active_count = self.active_count.saturating_sub(1);
        }

        // Invalidate all attestations given by this identity
        let owner = identity.owner;
        self.invalidate_attestations_from(owner);

        self.events.push(IdentityEvent::IdentityRevoked {
            identity_id,
            reason,
        });

        Ok(())
    }

    /// Invalidate all attestations from a specific attester
    fn invalidate_attestations_from(&mut self, attester: AccountId) {
        for attestations in self.attestations.values_mut() {
            for attestation in attestations.iter_mut() {
                if attestation.attester == attester {
                    attestation.revoke();
                }
            }
        }
    }

    // =========================================================================
    // EXPIRY MANAGEMENT
    // =========================================================================

    /// Check and update expired identities
    /// Also downgrades Active identities to Attested if all attestations have expired
    pub fn process_expirations(&mut self, current_block: BlockNumber) -> Vec<IdentityId> {
        let mut expired = Vec::new();
        let mut downgraded = Vec::new();

        // First collect identity IDs and their attestation counts
        // to avoid borrow checker issues
        let attestation_counts: Vec<(IdentityId, usize)> = self.identities
            .keys()
            .map(|id| {
                let count = self.count_valid_attestations(*id, current_block);
                (*id, count)
            })
            .collect();

        for (id, identity) in self.identities.iter_mut() {
            // Check for identity expiration
            if identity.is_expired(current_block) && identity.status != IdentityStatus::Expired && identity.status != IdentityStatus::Revoked {
                let was_active = identity.status == IdentityStatus::Active;
                identity.status = IdentityStatus::Expired;

                if was_active {
                    self.active_count = self.active_count.saturating_sub(1);
                }

                expired.push(*id);
            }
            // Check for attestation expiration (downgrade Active to Attested)
            else if identity.status == IdentityStatus::Active {
                // Find attestation count for this identity
                let valid_count = attestation_counts
                    .iter()
                    .find(|(i, _)| i == id)
                    .map(|(_, c)| *c)
                    .unwrap_or(0);

                // If no valid attestations remain, downgrade to Attested
                if valid_count == 0 {
                    identity.status = IdentityStatus::Attested;
                    self.active_count = self.active_count.saturating_sub(1);
                    downgraded.push(*id);
                }
            }
        }

        // Emit events
        for id in &expired {
            self.events.push(IdentityEvent::IdentityExpired {
                identity_id: *id,
            });
        }

        for id in &downgraded {
            self.events.push(IdentityEvent::StatusChanged {
                identity_id: *id,
                old_status: IdentityStatus::Active,
                new_status: IdentityStatus::Attested,
                reason: "All attestations expired".to_string(),
            });
        }

        expired
    }

    // =========================================================================
    // REPUTATION MANAGEMENT
    // =========================================================================

    /// Record governance participation
    pub fn record_governance_participation(
        &mut self,
        owner: &AccountId,
        current_block: BlockNumber,
    ) -> Result<(), IdentityError> {
        let identity_id = self.owner_to_identity
            .get(owner)
            .ok_or(IdentityError::IdentityNotFound)?;

        let identity = self.identities
            .get_mut(identity_id)
            .ok_or(IdentityError::IdentityNotFound)?;

        let old_score = identity.reputation.score;
        identity.reputation.record_governance(REPUTATION_POINTS_GOVERNANCE);
        identity.record_activity(current_block);

        self.events.push(IdentityEvent::ReputationChanged {
            identity_id: *identity_id,
            old_score,
            new_score: identity.reputation.score,
            reason: "governance participation".to_string(),
        });

        Ok(())
    }

    /// Record arbitration participation
    pub fn record_arbitration_participation(
        &mut self,
        owner: &AccountId,
        current_block: BlockNumber,
    ) -> Result<(), IdentityError> {
        let identity_id = self.owner_to_identity
            .get(owner)
            .ok_or(IdentityError::IdentityNotFound)?;

        let identity = self.identities
            .get_mut(identity_id)
            .ok_or(IdentityError::IdentityNotFound)?;

        let old_score = identity.reputation.score;
        identity.reputation.record_arbitration(REPUTATION_POINTS_ARBITRATION);
        identity.record_activity(current_block);

        self.events.push(IdentityEvent::ReputationChanged {
            identity_id: *identity_id,
            old_score,
            new_score: identity.reputation.score,
            reason: "arbitration service".to_string(),
        });

        Ok(())
    }

    /// Apply slashing penalty to identity
    pub fn slash_reputation(
        &mut self,
        owner: &AccountId,
        penalty_percent: u8,
    ) -> Result<(), IdentityError> {
        let identity_id = self.owner_to_identity
            .get(owner)
            .ok_or(IdentityError::IdentityNotFound)?;

        let identity = self.identities
            .get_mut(identity_id)
            .ok_or(IdentityError::IdentityNotFound)?;

        let old_score = identity.reputation.score;
        identity.reputation.apply_slash(penalty_percent);

        self.events.push(IdentityEvent::ReputationChanged {
            identity_id: *identity_id,
            old_score,
            new_score: identity.reputation.score,
            reason: format!("slashed {}%", penalty_percent),
        });

        Ok(())
    }

    /// Apply decay to all identities
    pub fn apply_reputation_decay(&mut self, current_block: BlockNumber) {
        if !self.config.enable_reputation_decay {
            return;
        }

        for identity in self.identities.values_mut() {
            if identity.status == IdentityStatus::Active {
                identity.apply_decay(current_block);
            }
        }
    }

    // =========================================================================
    // GOVERNANCE CHECKS
    // =========================================================================

    /// Check if account can vote
    /// CONSTITUTIONAL (Article VI): Identity is always optional
    /// This always returns true - identity cannot be required for voting
    pub fn can_vote(&self, _owner: &AccountId, _current_block: BlockNumber) -> bool {
        // Article VI: "Identity SHALL NEVER be required to [...] transact"
        // self.config.require_identity_for_voting() always returns false
        true
    }

    /// Check if account can create proposals
    /// CONSTITUTIONAL (Article VI): Identity is always optional
    /// This always returns true - identity cannot be required for proposals
    pub fn can_create_proposal(&self, _owner: &AccountId, _current_block: BlockNumber) -> bool {
        // Article VI: "Identity SHALL NEVER be required to [...] transact"
        // self.config.require_identity_for_proposals() always returns false
        true
    }

    /// Get vote weight for an account
    pub fn get_vote_weight(&self, owner: &AccountId, base_weight: Balance) -> Balance {
        self.get_identity_by_owner(owner)
            .map(|i| {
                let multiplier = i.reputation.vote_weight() as u128;
                // Use saturating_mul to prevent overflow on large weights
                base_weight.saturating_mul(multiplier) / 100
            })
            .unwrap_or(base_weight)
    }

    /// Get VC boost for an account
    pub fn get_vc_boost(&self, owner: &AccountId) -> u32 {
        self.get_identity_by_owner(owner)
            .map(|i| i.reputation.vc_boost())
            .unwrap_or(100) // 1.0x if no identity
    }

    // =========================================================================
    // EVENTS
    // =========================================================================

    /// Get all events
    pub fn events(&self) -> &[IdentityEvent] {
        &self.events
    }

    /// Clear events (after processing)
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Force activate an identity (for testing purposes)
    #[cfg(test)]
    pub fn force_activate(&mut self, identity_id: &IdentityId) {
        if let Some(identity) = self.identities.get_mut(identity_id) {
            if identity.status != IdentityStatus::Active {
                identity.status = IdentityStatus::Active;
                self.active_count += 1;
            }
        }
    }

    /// Get mutable access to identities (for testing)
    #[cfg(test)]
    pub fn identities_mut(&mut self) -> &mut std::collections::HashMap<IdentityId, crate::types::identity::IdentityCommitment> {
        &mut self.identities
    }
}

impl Default for IdentityRegistry {
    fn default() -> Self {
        Self::new(ChainId(0))
    }
}

/// Errors that can occur in identity operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum IdentityError {
    #[error("Identity already exists for this owner")]
    IdentityAlreadyExists,

    #[error("Identity not found")]
    IdentityNotFound,

    #[error("Attester has no identity")]
    AttesterHasNoIdentity,

    #[error("Attester identity is not active")]
    AttesterNotActive,

    #[error("Cannot attest your own identity")]
    CannotSelfAttest,

    #[error("Identity cannot receive attestations in current state")]
    CannotReceiveAttestations,

    #[error("Already attested this identity")]
    AlreadyAttested,

    #[error("Maximum attestations reached")]
    MaxAttestationsReached,

    #[error("Identity has been revoked")]
    IdentityRevoked,

    #[error("Identity already revoked")]
    AlreadyRevoked,
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

    fn setup_registry() -> IdentityRegistry {
        IdentityRegistry::new(ChainId(1))
    }

    fn setup_with_active_identity(registry: &mut IdentityRegistry, owner: AccountId) -> IdentityId {
        let id = registry.declare_identity(owner, b"data", None, 1000).unwrap();
        // Force activate for testing
        if let Some(identity) = registry.identities.get_mut(&id) {
            identity.status = IdentityStatus::Active;
        }
        registry.active_count += 1;
        id
    }

    #[test]
    fn test_declare_identity() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        let id = registry.declare_identity(owner, b"commitment", Some("Alice".to_string()), 1000);
        assert!(id.is_ok());

        let identity = registry.get_identity_by_owner(&owner).unwrap();
        assert_eq!(identity.owner, owner);
        assert_eq!(identity.status, IdentityStatus::Declared);
        assert_eq!(identity.display_name, Some("Alice".to_string()));
    }

    #[test]
    fn test_cannot_declare_twice() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        registry.declare_identity(owner, b"data1", None, 1000).unwrap();
        let result = registry.declare_identity(owner, b"data2", None, 1001);

        assert!(matches!(result, Err(IdentityError::IdentityAlreadyExists)));
    }

    #[test]
    fn test_submit_attestation() {
        let mut registry = setup_registry();
        let attester = create_account(1);
        let target = create_account(2);

        // Setup attester with active identity
        setup_with_active_identity(&mut registry, attester);

        // Declare target identity
        let target_id = registry.declare_identity(target, b"target", None, 1000).unwrap();

        // Submit attestation
        let result = registry.submit_attestation(attester, target_id, b"claim", 1001);
        assert!(result.is_ok());

        // Check attestation was recorded
        let count = registry.count_valid_attestations(target_id, 1001);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_cannot_self_attest() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        let id = setup_with_active_identity(&mut registry, owner);

        let result = registry.submit_attestation(owner, id, b"claim", 1001);
        assert!(matches!(result, Err(IdentityError::CannotSelfAttest)));
    }

    #[test]
    fn test_cannot_attest_without_identity() {
        let mut registry = setup_registry();
        let attester = create_account(1);
        let target = create_account(2);

        let target_id = registry.declare_identity(target, b"target", None, 1000).unwrap();

        let result = registry.submit_attestation(attester, target_id, b"claim", 1001);
        assert!(matches!(result, Err(IdentityError::AttesterHasNoIdentity)));
    }

    #[test]
    fn test_cannot_attest_twice() {
        let mut registry = setup_registry();
        let attester = create_account(1);
        let target = create_account(2);

        setup_with_active_identity(&mut registry, attester);
        let target_id = registry.declare_identity(target, b"target", None, 1000).unwrap();

        registry.submit_attestation(attester, target_id, b"claim1", 1001).unwrap();
        let result = registry.submit_attestation(attester, target_id, b"claim2", 1002);

        assert!(matches!(result, Err(IdentityError::AlreadyAttested)));
    }

    #[test]
    fn test_identity_activation() {
        let mut registry = setup_registry();

        // Create attesters with active identities
        let attester1 = create_account(1);
        let attester2 = create_account(2);
        let attester3 = create_account(3);
        let target = create_account(10);

        setup_with_active_identity(&mut registry, attester1);
        setup_with_active_identity(&mut registry, attester2);
        setup_with_active_identity(&mut registry, attester3);

        let target_id = registry.declare_identity(target, b"target", None, 1000).unwrap();

        // After 1 attestation: Attested
        registry.submit_attestation(attester1, target_id, b"c1", 1001).unwrap();
        assert_eq!(registry.get_identity(&target_id).unwrap().status, IdentityStatus::Attested);

        // After 2 attestations: still Attested
        registry.submit_attestation(attester2, target_id, b"c2", 1002).unwrap();
        assert_eq!(registry.get_identity(&target_id).unwrap().status, IdentityStatus::Attested);

        // After 3 attestations: Active!
        registry.submit_attestation(attester3, target_id, b"c3", 1003).unwrap();
        assert_eq!(registry.get_identity(&target_id).unwrap().status, IdentityStatus::Active);
    }

    #[test]
    fn test_renew_identity() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        setup_with_active_identity(&mut registry, owner);

        let old_expiry = registry.get_identity_by_owner(&owner).unwrap().expires_at;
        let new_expiry = registry.renew_identity(owner, 2000).unwrap();

        assert!(new_expiry > old_expiry);
    }

    #[test]
    fn test_revoke_own_identity() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        setup_with_active_identity(&mut registry, owner);

        let deposit = registry.revoke_own_identity(owner).unwrap();
        assert_eq!(deposit, IDENTITY_DEPOSIT);

        let identity = registry.get_identity_by_owner(&owner).unwrap();
        assert_eq!(identity.status, IdentityStatus::Revoked);
    }

    #[test]
    fn test_force_revoke() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        let id = setup_with_active_identity(&mut registry, owner);

        registry.force_revoke_identity(id, RevocationReason::Misconduct).unwrap();

        let identity = registry.get_identity(&id).unwrap();
        assert_eq!(identity.status, IdentityStatus::Revoked);
    }

    #[test]
    fn test_process_expirations() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        let id = setup_with_active_identity(&mut registry, owner);

        // Set expiry to past
        if let Some(identity) = registry.identities.get_mut(&id) {
            identity.expires_at = 1000;
        }

        let expired = registry.process_expirations(1001);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], id);

        let identity = registry.get_identity(&id).unwrap();
        assert_eq!(identity.status, IdentityStatus::Expired);
    }

    #[test]
    fn test_reputation_governance() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        setup_with_active_identity(&mut registry, owner);

        registry.record_governance_participation(&owner, 1000).unwrap();

        let identity = registry.get_identity_by_owner(&owner).unwrap();
        assert_eq!(identity.reputation.score, REPUTATION_POINTS_GOVERNANCE);
        assert_eq!(identity.reputation.governance_actions, 1);
    }

    #[test]
    fn test_reputation_arbitration() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        setup_with_active_identity(&mut registry, owner);

        registry.record_arbitration_participation(&owner, 1000).unwrap();

        let identity = registry.get_identity_by_owner(&owner).unwrap();
        assert_eq!(identity.reputation.score, REPUTATION_POINTS_ARBITRATION);
        assert_eq!(identity.reputation.arbitration_count, 1);
    }

    #[test]
    fn test_slash_reputation() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        let id = setup_with_active_identity(&mut registry, owner);

        // Give some reputation first
        if let Some(identity) = registry.identities.get_mut(&id) {
            identity.reputation.score = 1000;
        }

        registry.slash_reputation(&owner, 50).unwrap(); // 50% slash

        let identity = registry.get_identity(&id).unwrap();
        assert_eq!(identity.reputation.score, 500);
        assert_eq!(identity.reputation.slash_count, 1);
    }

    #[test]
    fn test_vote_weight() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        let id = setup_with_active_identity(&mut registry, owner);

        // Set high reputation
        if let Some(identity) = registry.identities.get_mut(&id) {
            identity.reputation.score = 5000;
        }

        // Base weight 100, with 5000 rep should get 1.5x
        let weight = registry.get_vote_weight(&owner, 100);
        assert_eq!(weight, 150);
    }

    #[test]
    fn test_vc_boost() {
        let mut registry = setup_registry();
        let owner = create_account(1);
        let no_identity = create_account(2);

        let id = setup_with_active_identity(&mut registry, owner);

        if let Some(identity) = registry.identities.get_mut(&id) {
            identity.reputation.score = 5000;
        }

        // With reputation
        assert_eq!(registry.get_vc_boost(&owner), 125); // 1.25x

        // Without identity
        assert_eq!(registry.get_vc_boost(&no_identity), 100); // 1.0x
    }

    #[test]
    fn test_can_vote_constitutional() {
        let mut registry = setup_registry();
        let active_owner = create_account(1);
        let no_identity = create_account(2);

        setup_with_active_identity(&mut registry, active_owner);

        // CONSTITUTIONAL (Article VI): Identity is NEVER required for voting
        // Both accounts with and without identity can always vote
        assert!(registry.can_vote(&active_owner, 1000));
        assert!(registry.can_vote(&no_identity, 1000));

        // The config method always returns false per constitutional mandate
        assert!(!registry.config.require_identity_for_voting());
        assert!(!registry.config.require_identity_for_proposals());

        // Users without identity can still vote
        assert!(registry.can_vote(&no_identity, 1000));
        assert!(registry.can_create_proposal(&no_identity, 1000));
    }

    #[test]
    fn test_active_count_tracking() {
        let mut registry = setup_registry();

        assert_eq!(registry.active_count(), 0);

        let owner1 = create_account(1);
        setup_with_active_identity(&mut registry, owner1);
        assert_eq!(registry.active_count(), 1);

        let owner2 = create_account(2);
        setup_with_active_identity(&mut registry, owner2);
        assert_eq!(registry.active_count(), 2);

        registry.revoke_own_identity(owner1).unwrap();
        assert_eq!(registry.active_count(), 1);
    }

    #[test]
    fn test_attestation_weight_affects_total() {
        let mut registry = setup_registry();

        // Create attester with high reputation
        let high_rep_attester = create_account(1);
        let id1 = setup_with_active_identity(&mut registry, high_rep_attester);
        if let Some(identity) = registry.identities.get_mut(&id1) {
            identity.reputation.score = 9000; // High rep = weight 10
        }

        // Create attester with low reputation
        let low_rep_attester = create_account(2);
        setup_with_active_identity(&mut registry, low_rep_attester);
        // Default 0 rep = weight 1

        let target = create_account(10);
        let target_id = registry.declare_identity(target, b"t", None, 1000).unwrap();

        registry.submit_attestation(high_rep_attester, target_id, b"c1", 1001).unwrap();
        registry.submit_attestation(low_rep_attester, target_id, b"c2", 1002).unwrap();

        let total_weight = registry.total_attestation_weight(target_id, 1003);
        assert_eq!(total_weight, 10 + 1); // 10 from high rep, 1 from low rep
    }

    #[test]
    fn test_events_emitted() {
        let mut registry = setup_registry();
        let owner = create_account(1);

        registry.declare_identity(owner, b"data", None, 1000).unwrap();

        assert_eq!(registry.events().len(), 1);
        assert!(matches!(
            registry.events()[0],
            IdentityEvent::IdentityDeclared { .. }
        ));

        registry.clear_events();
        assert!(registry.events().is_empty());
    }
}
