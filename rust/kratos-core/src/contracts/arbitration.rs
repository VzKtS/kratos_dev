// Arbitration - SPEC v3.1 Phase 6: Cross-Chain Arbitration
// Dispute resolution engine with VRF-selected juries and enforcement

use crate::consensus::vrf_selection::VRFSelector;
use crate::consensus::validator_credits::ValidatorCreditsManager;
use crate::storage::db::{Database, DatabaseError};
use crate::types::{
    AccountId, Balance, BlockNumber, ChainId, Dispute, DisputeId, DisputeStatus, DisputeType,
    Enforcement, Evidence, JuryDecision, JuryVote, Jurisdiction, Verdict,
    ArbitrationError, ARBITRATION_VC_REWARD, DEFAULT_JURY_SIZE, DELIBERATION_PERIOD,
    MAX_DISPUTE_DURATION, MIN_JURY_SIZE, MAX_EVIDENCE_COUNT,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Validator info for jury selection
#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    pub account: AccountId,
    pub stake: Balance,
    pub vc: u64,
}

/// Arbitration contract for cross-chain dispute resolution
pub struct ArbitrationContract {
    /// Persistent storage
    db: Database,

    /// In-memory dispute cache
    disputes: HashMap<DisputeId, Dispute>,

    /// Validator registry (for jury selection)
    validators: HashMap<AccountId, ValidatorInfo>,

    /// Validator credits tracker
    validator_credits: ValidatorCreditsManager,

    /// Next dispute ID
    next_dispute_id: DisputeId,

    /// Current block number (updated each block)
    current_block: BlockNumber,
}

impl ArbitrationContract {
    /// Create a new arbitration contract
    pub fn new(db: Database, validator_credits: ValidatorCreditsManager) -> Self {
        Self {
            db,
            disputes: HashMap::new(),
            validators: HashMap::new(),
            validator_credits,
            next_dispute_id: 1,
            current_block: 0,
        }
    }

    /// Register a validator for jury selection
    pub fn register_validator(&mut self, account: AccountId, stake: Balance, vc: u64) {
        self.validators.insert(account, ValidatorInfo { account, stake, vc });
    }

    /// Update current block number (called each block)
    pub fn set_current_block(&mut self, block_number: BlockNumber) {
        self.current_block = block_number;
    }

    /// Raise a new dispute
    pub fn raise_dispute(
        &mut self,
        chain_id: ChainId,
        dispute_type: DisputeType,
        raised_by: AccountId,
        jurisdiction: Jurisdiction,
        initial_evidence: Option<Evidence>,
    ) -> Result<DisputeId, ArbitrationError> {
        // Validate jurisdiction
        self.validate_jurisdiction(&jurisdiction, &chain_id)?;

        // SECURITY FIX #30: Check for dispute ID overflow before creating new dispute
        // If next_dispute_id is at u64::MAX, we cannot create more disputes safely
        if self.next_dispute_id == u64::MAX {
            return Err(ArbitrationError::MaxDisputesReached);
        }

        // Create new dispute
        let dispute_id = self.next_dispute_id;

        // FIX: Verify dispute_id is unique (defensive check against state corruption)
        if self.disputes.contains_key(&dispute_id) {
            // This should never happen in normal operation, but protects against
            // state corruption from crashes or improper recovery
            return Err(ArbitrationError::DuplicateDispute(dispute_id));
        }

        // SECURITY FIX #30: Use checked_add instead of saturating_add
        // This ensures we detect overflow before it causes issues
        self.next_dispute_id = self.next_dispute_id.checked_add(1)
            .ok_or(ArbitrationError::MaxDisputesReached)?;

        let mut dispute = Dispute::new(
            dispute_id,
            chain_id,
            dispute_type,
            raised_by,
            self.current_block,
            jurisdiction,
        );

        // Add initial evidence if provided
        if let Some(evidence) = initial_evidence {
            dispute.evidence.push(evidence);
        }

        // Store dispute
        self.disputes.insert(dispute_id, dispute.clone());
        self.persist_dispute(&dispute)?;

        Ok(dispute_id)
    }

    /// Submit evidence for a dispute
    /// SECURITY FIX #4: Added submitter parameter and access control
    /// Only the dispute raiser, accused party, or registered validators can submit evidence
    pub fn submit_evidence(
        &mut self,
        dispute_id: DisputeId,
        evidence: Evidence,
        submitter: AccountId,
    ) -> Result<(), ArbitrationError> {
        let current_block = self.current_block;

        // SECURITY FIX #4: Check validator status BEFORE getting mutable reference
        // to avoid borrow conflicts
        let is_validator = self.validators.contains_key(&submitter);

        let dispute = self.get_dispute_mut(dispute_id)?;

        // Check if evidence submission is still open
        if !dispute.can_submit_evidence(current_block) {
            return Err(ArbitrationError::EvidenceWindowClosed);
        }

        // SECURITY FIX #4: Access control - only authorized parties can submit evidence
        // Authorized submitters are:
        // 1. The dispute raiser
        // 2. The accused party (if identified)
        // 3. Registered validators (who may have relevant evidence)
        let is_raiser = submitter == dispute.raised_by;
        let is_accused = dispute.accused().map_or(false, |a| a == submitter);

        if !is_raiser && !is_accused && !is_validator {
            return Err(ArbitrationError::Unauthorized {
                action: "submit_evidence".to_string(),
                required: "dispute party or validator".to_string(),
            });
        }

        // Check maximum evidence limit (prevent DoS)
        if dispute.evidence.len() >= MAX_EVIDENCE_COUNT {
            return Err(ArbitrationError::EvidenceLimitReached);
        }

        // Add evidence
        dispute.evidence.push(evidence);
        let dispute_clone = dispute.clone();
        self.persist_dispute(&dispute_clone)?;

        Ok(())
    }

    /// Close evidence submission and select jury via VRF
    pub fn select_jury(
        &mut self,
        dispute_id: DisputeId,
        jury_size: Option<usize>,
    ) -> Result<Vec<AccountId>, ArbitrationError> {
        let current_block = self.current_block;

        // Validate and get excluded party
        let excluded = {
            let dispute = self.get_dispute(dispute_id)?;

            // Check dispute status
            if dispute.status != DisputeStatus::Open {
                return Err(ArbitrationError::InvalidState {
                    expected: "Open".to_string(),
                    actual: format!("{:?}", dispute.status),
                });
            }

            // Check minimum evidence
            if dispute.evidence.is_empty() {
                return Err(ArbitrationError::InvalidState {
                    expected: "At least one evidence".to_string(),
                    actual: "No evidence submitted".to_string(),
                });
            }

            // Get accused party and dispute raiser (both excluded from jury)
            let accused = dispute.accused();
            let raiser = dispute.raised_by;
            (accused, raiser)
        };

        // Select jury using VRF
        let jury_size = jury_size.unwrap_or(DEFAULT_JURY_SIZE);

        // Build candidate list excluding:
        // 1. The accused party (if identified)
        // 2. The dispute raiser (always excluded to prevent conflict of interest)
        let candidates: Vec<(AccountId, Balance, u64)> = self.validators
            .values()
            .filter(|v| {
                // Exclude the accused if present
                if let Some(accused_id) = excluded.0 {
                    if v.account == accused_id {
                        return false;
                    }
                }
                // Always exclude the dispute raiser
                if v.account == excluded.1 {
                    return false;
                }
                true
            })
            .map(|v| (v.account, v.stake, v.vc))
            .collect();

        if candidates.is_empty() {
            return Err(ArbitrationError::InsufficientJury(0));
        }

        // Select N unique jury members using VRF
        let mut jury_members = Vec::new();
        let mut used_validators = Vec::new();

        for round in 0..jury_size {
            // Filter out already selected
            let available: Vec<(AccountId, Balance, u64)> = candidates
                .iter()
                .filter(|(id, _, _)| !used_validators.contains(id))
                .cloned()
                .collect();

            if available.is_empty() {
                break; // No more validators available
            }

            // Use dispute_id + round as epoch for determinism
            let epoch = dispute_id + round as u64;
            let selected = VRFSelector::select_validator(round as u64, epoch, &available)
                .map_err(|e| ArbitrationError::VRFSelectionFailed(format!("{:?}", e)))?;

            jury_members.push(selected);
            used_validators.push(selected);
        }

        if jury_members.len() < MIN_JURY_SIZE {
            return Err(ArbitrationError::InsufficientJury(jury_members.len()));
        }

        // Update dispute status
        {
            let dispute = self.get_dispute_mut(dispute_id)?;
            dispute.status = DisputeStatus::Deliberating;
            dispute.jury_members = jury_members.clone();
            dispute.deliberation_deadline = Some(current_block + DELIBERATION_PERIOD);
            let dispute_clone = dispute.clone();
            self.persist_dispute(&dispute_clone)?;
        }

        Ok(jury_members)
    }

    /// Submit a jury vote
    pub fn submit_jury_vote(
        &mut self,
        dispute_id: DisputeId,
        juror: AccountId,
        verdict: Verdict,
        justification: Option<String>,
    ) -> Result<(), ArbitrationError> {
        let current_block = self.current_block;

        let dispute = self.get_dispute_mut(dispute_id)?;

        // Check if deliberation is active
        if !dispute.can_vote(current_block) {
            return Err(ArbitrationError::DeliberationClosed);
        }

        // Check if juror is in the jury
        if !dispute.jury_members.contains(&juror) {
            return Err(ArbitrationError::NotJuryMember);
        }

        // Check if juror already voted
        if dispute.jury_votes.iter().any(|v| v.juror == juror) {
            return Err(ArbitrationError::AlreadyVoted);
        }

        // Record vote
        let vote = JuryVote {
            juror,
            verdict,
            justification,
            timestamp: current_block,
        };

        dispute.jury_votes.push(vote);
        let dispute_clone = dispute.clone();
        self.persist_dispute(&dispute_clone)?;

        Ok(())
    }

    /// Tally votes and finalize decision (can be called once all votes are in or deadline passed)
    pub fn tally_votes(&mut self, dispute_id: DisputeId) -> Result<JuryDecision, ArbitrationError> {
        let current_block = self.current_block;

        // Clone data we need before mutable borrow
        let (jury_votes, jury_members, status) = {
            let dispute = self.get_dispute(dispute_id)?;

            // Check status
            if dispute.status != DisputeStatus::Deliberating {
                return Err(ArbitrationError::InvalidState {
                    expected: "Deliberating".to_string(),
                    actual: format!("{:?}", dispute.status),
                });
            }

            // Check if deliberation deadline has passed OR all jury members have voted
            let all_voted = dispute.jury_votes.len() >= dispute.jury_members.len();
            let deadline_passed = dispute.deliberation_deadline
                .map(|deadline| current_block >= deadline)
                .unwrap_or(false);

            if !all_voted && !deadline_passed {
                return Err(ArbitrationError::DeliberationNotComplete);
            }

            // Check if we have votes
            if dispute.jury_votes.is_empty() {
                return Err(ArbitrationError::InvalidState {
                    expected: "At least one vote".to_string(),
                    actual: "No votes submitted".to_string(),
                });
            }

            (dispute.jury_votes.clone(), dispute.jury_members.clone(), dispute.status)
        };

        // Tally votes
        let decision = JuryDecision::tally_votes(
            jury_votes,
            self.current_block,
            dispute_id,
        );

        // Update dispute
        {
            let dispute = self.get_dispute_mut(dispute_id)?;
            dispute.decision = Some(decision.clone());
            dispute.status = DisputeStatus::Resolved;
            let dispute_clone = dispute.clone();
            self.persist_dispute(&dispute_clone)?;
        }

        // Reward jury members with validator credits
        self.reward_jury_members(&jury_members)?;

        Ok(decision)
    }

    /// Enforce a jury decision (slashing, purging, etc.)
    /// SECURITY FIX #4: Added caller parameter and access control
    /// Only validators or system accounts can enforce verdicts
    pub fn enforce_verdict(
        &mut self,
        dispute_id: DisputeId,
        enforcement: Enforcement,
        caller: AccountId,
    ) -> Result<(), ArbitrationError> {
        // SECURITY FIX #4: Only validators can enforce verdicts
        // This prevents malicious parties from triggering enforcement
        let is_validator = self.validators.contains_key(&caller);
        if !is_validator {
            return Err(ArbitrationError::Unauthorized {
                action: "enforce_verdict".to_string(),
                required: "registered validator".to_string(),
            });
        }

        // Check state first
        {
            let dispute = self.get_dispute(dispute_id)?;

            // Check if resolved
            if dispute.status != DisputeStatus::Resolved {
                return Err(ArbitrationError::InvalidState {
                    expected: "Resolved".to_string(),
                    actual: format!("{:?}", dispute.status),
                });
            }

            // Check if already enforced
            if dispute.decision.as_ref().and_then(|d| d.enforcement.as_ref()).is_some() {
                return Err(ArbitrationError::AlreadyResolved);
            }
        }

        // Update decision with enforcement
        {
            let dispute = self.get_dispute_mut(dispute_id)?;
            if let Some(decision) = &mut dispute.decision {
                decision.enforcement = Some(enforcement.clone());
            }
            let dispute_clone = dispute.clone();
            self.persist_dispute(&dispute_clone)?;
        }

        // Note: Actual enforcement (slashing, purging) happens externally
        // via integration with ValidatorSet and SidechainRegistry

        Ok(())
    }

    /// Appeal a decision to higher jurisdiction
    pub fn appeal_decision(
        &mut self,
        dispute_id: DisputeId,
        appealed_by: AccountId,
        new_jurisdiction: Jurisdiction,
    ) -> Result<DisputeId, ArbitrationError> {
        // Clone data we need from original dispute
        let (chain_id, dispute_type, jurisdiction, evidence) = {
            let original_dispute = self.get_dispute(dispute_id)?;

            // Check if resolved
            if original_dispute.status != DisputeStatus::Resolved {
                return Err(ArbitrationError::InvalidState {
                    expected: "Resolved".to_string(),
                    actual: format!("{:?}", original_dispute.status),
                });
            }

            // Validate jurisdiction escalation
            if !self.is_higher_jurisdiction(&new_jurisdiction, &original_dispute.jurisdiction) {
                return Err(ArbitrationError::InvalidJurisdiction);
            }

            (
                original_dispute.chain_id,
                original_dispute.dispute_type.clone(),
                original_dispute.jurisdiction.clone(),
                original_dispute.evidence.clone(),
            )
        };

        // Create new dispute with escalated jurisdiction
        let new_dispute_id = self.raise_dispute(
            chain_id,
            dispute_type,
            appealed_by,
            new_jurisdiction,
            None,
        )?;

        // Copy evidence from original dispute
        {
            let new_dispute = self.get_dispute_mut(new_dispute_id)?;
            new_dispute.evidence = evidence;
            let new_dispute_clone = new_dispute.clone();
            self.persist_dispute(&new_dispute_clone)?;
        }

        // Mark original as appealed
        {
            let original = self.get_dispute_mut(dispute_id)?;
            original.status = DisputeStatus::Appealed;
            let original_clone = original.clone();
            self.persist_dispute(&original_clone)?;
        }

        Ok(new_dispute_id)
    }

    /// Get dispute by ID (immutable)
    pub fn get_dispute(&self, dispute_id: DisputeId) -> Result<&Dispute, ArbitrationError> {
        self.disputes
            .get(&dispute_id)
            .ok_or(ArbitrationError::DisputeNotFound(dispute_id))
    }

    /// Get dispute by ID (mutable)
    fn get_dispute_mut(&mut self, dispute_id: DisputeId) -> Result<&mut Dispute, ArbitrationError> {
        self.disputes
            .get_mut(&dispute_id)
            .ok_or(ArbitrationError::DisputeNotFound(dispute_id))
    }

    /// Get all disputes for a chain
    pub fn get_disputes_for_chain(&self, chain_id: ChainId) -> Vec<&Dispute> {
        self.disputes
            .values()
            .filter(|d| d.chain_id == chain_id)
            .collect()
    }

    /// Get all active disputes (Open or Deliberating)
    pub fn get_active_disputes(&self) -> Vec<&Dispute> {
        self.disputes
            .values()
            .filter(|d| {
                matches!(d.status, DisputeStatus::Open | DisputeStatus::Deliberating)
            })
            .collect()
    }

    /// Check if a validator is currently serving on any jury
    pub fn is_jury_member(&self, validator: &AccountId) -> bool {
        self.disputes
            .values()
            .any(|d| d.status == DisputeStatus::Deliberating && d.jury_members.contains(validator))
    }

    /// Expire stale disputes that have exceeded their maximum duration
    /// This prevents disputes from indefinitely blocking chain exits
    /// Returns the IDs of disputes that were expired
    pub fn expire_stale_disputes(&mut self) -> Vec<DisputeId> {
        let current_block = self.current_block;
        let mut expired_ids = Vec::new();

        // Find all stale or expired disputes
        for (id, dispute) in self.disputes.iter() {
            if dispute.is_stale(current_block) || dispute.is_expired(current_block) {
                expired_ids.push(*id);
            }
        }

        // Update their status
        for id in &expired_ids {
            if let Some(dispute) = self.disputes.get_mut(id) {
                dispute.status = DisputeStatus::Expired;
                // Persist the update
                if let Ok(value) = serde_json::to_vec(dispute) {
                    let key = format!("dispute:{}", dispute.id);
                    let _ = self.db.put(key.as_bytes(), &value);
                }
            }
        }

        expired_ids
    }

    /// Dismiss a dispute (for lack of evidence, frivolous claims, etc.)
    /// SECURITY FIX #4: Added caller parameter and access control
    /// Only jurisdiction authorities (e.g., Host/Root chain validators) can dismiss
    pub fn dismiss_dispute(
        &mut self,
        dispute_id: DisputeId,
        reason: &str,
        caller: AccountId,
    ) -> Result<(), ArbitrationError> {
        // SECURITY FIX #4: Check validator status BEFORE getting mutable reference
        let is_validator = self.validators.contains_key(&caller);
        if !is_validator {
            return Err(ArbitrationError::Unauthorized {
                action: "dismiss_dispute".to_string(),
                required: "registered validator".to_string(),
            });
        }

        let dispute = self.get_dispute_mut(dispute_id)?;

        // Only Open or EvidenceComplete disputes can be dismissed
        if !matches!(dispute.status, DisputeStatus::Open | DisputeStatus::EvidenceComplete) {
            return Err(ArbitrationError::InvalidState {
                expected: "Open or EvidenceComplete".to_string(),
                actual: format!("{:?}", dispute.status),
            });
        }

        dispute.status = DisputeStatus::Dismissed;
        let dispute_clone = dispute.clone();
        self.persist_dispute(&dispute_clone)?;

        Ok(())
    }

    /// Check if there are any active disputes blocking an exit for a chain
    pub fn has_blocking_disputes(&self, chain_id: ChainId) -> bool {
        self.disputes
            .values()
            .any(|d| {
                d.chain_id == chain_id &&
                matches!(d.status, DisputeStatus::Open | DisputeStatus::EvidenceComplete | DisputeStatus::Deliberating)
            })
    }

    /// Get remaining time before a dispute expires
    pub fn time_until_expiry(&self, dispute_id: DisputeId) -> Option<BlockNumber> {
        if let Ok(dispute) = self.get_dispute(dispute_id) {
            let deadline = dispute.absolute_deadline();
            if self.current_block < deadline {
                Some(deadline - self.current_block)
            } else {
                Some(0)
            }
        } else {
            None
        }
    }

    // --- Private helper methods ---

    /// Validate jurisdiction for a chain
    fn validate_jurisdiction(
        &self,
        jurisdiction: &Jurisdiction,
        chain_id: &ChainId,
    ) -> Result<(), ArbitrationError> {
        match jurisdiction {
            Jurisdiction::Local(jur_chain_id) => {
                if jur_chain_id != chain_id {
                    return Err(ArbitrationError::InvalidJurisdiction);
                }
            }
            Jurisdiction::Host(_) | Jurisdiction::Root => {
                // Host and Root can handle any chain's disputes
            }
        }
        Ok(())
    }

    /// Check if new jurisdiction is higher than current
    fn is_higher_jurisdiction(
        &self,
        new: &Jurisdiction,
        current: &Jurisdiction,
    ) -> bool {
        match (current, new) {
            (Jurisdiction::Local(_), Jurisdiction::Host(_)) => true,
            (Jurisdiction::Local(_), Jurisdiction::Root) => true,
            (Jurisdiction::Host(_), Jurisdiction::Root) => true,
            _ => false,
        }
    }

    /// Reward jury members with validator credits
    fn reward_jury_members(&mut self, jury_members: &[AccountId]) -> Result<(), ArbitrationError> {
        let block_number = self.current_block;
        let epoch = block_number / 600; // 600 blocks per epoch (EPOCH_DURATION_BLOCKS)

        for juror in jury_members {
            self.validator_credits
                .record_arbitration(juror, block_number, epoch)
                .map_err(|e| ArbitrationError::VRFSelectionFailed(e.to_string()))?;
        }
        Ok(())
    }

    /// Persist dispute to database
    fn persist_dispute(&self, dispute: &Dispute) -> Result<(), ArbitrationError> {
        let key = format!("dispute:{}", dispute.id);
        let value = serde_json::to_vec(dispute)
            .map_err(|e| ArbitrationError::SerializationError(e.to_string()))?;

        self.db
            .put(key.as_bytes(), &value)
            .map_err(|e| ArbitrationError::DatabaseError(format!("Database write failed: {:?}", e)))?;

        Ok(())
    }

    /// Load dispute from database
    pub fn load_dispute(&mut self, dispute_id: DisputeId) -> Result<(), ArbitrationError> {
        let key = format!("dispute:{}", dispute_id);

        let value = self.db
            .get(key.as_bytes())
            .map_err(|e| ArbitrationError::DatabaseError(format!("Database read failed: {:?}", e)))?
            .ok_or(ArbitrationError::DisputeNotFound(dispute_id))?;

        let dispute: Dispute = serde_json::from_slice(&value)
            .map_err(|e| ArbitrationError::SerializationError(e.to_string()))?;

        self.disputes.insert(dispute_id, dispute);
        Ok(())
    }

    /// Load all disputes from database
    pub fn load_all_disputes(&mut self) -> Result<usize, ArbitrationError> {
        let prefix = b"dispute:";
        let mut count = 0;

        for (key, value) in self.db.prefix_iterator(prefix) {
            let dispute: Dispute = serde_json::from_slice(&value)
                .map_err(|e| ArbitrationError::SerializationError(e.to_string()))?;

            self.disputes.insert(dispute.id, dispute);
            count += 1;
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DisputeType, FraudProof, Hash};
    use tempfile::TempDir;

    fn setup_arbitration() -> (ArbitrationContract, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();
        let validator_credits = ValidatorCreditsManager::new();

        let contract = ArbitrationContract::new(db, validator_credits);
        (contract, temp_dir)
    }

    fn create_test_fraud_proof(validator: AccountId) -> FraudProof {
        use crate::types::{BlockHeader, Hash, Signature64};

        let block_a = BlockHeader {
            number: 500,
            parent_hash: Hash::from_bytes([0; 32]),
            state_root: Hash::from_bytes([1; 32]),
            transactions_root: Hash::from_bytes([2; 32]),
            timestamp: 1000,
            epoch: 0,
            slot: 0,
            author: AccountId::from_bytes([0; 32]),
            signature: Signature64::zero(),
        };

        let block_b = BlockHeader {
            number: 500, // Same height
            parent_hash: Hash::from_bytes([0; 32]),
            state_root: Hash::from_bytes([3; 32]), // Different state
            transactions_root: Hash::from_bytes([4; 32]),
            timestamp: 1000,
            epoch: 0,
            slot: 0,
            author: AccountId::from_bytes([0; 32]),
            signature: Signature64::zero(),
        };

        FraudProof::DoubleFinalization {
            validator,
            block_a,
            block_b,
            signature_a: Signature64::zero(),
            signature_b: Signature64::zero(),
        }
    }

    #[test]
    fn test_raise_dispute() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            Jurisdiction::Root,
            None,
        ).unwrap();

        assert_eq!(dispute_id, 1);

        let dispute = contract.get_dispute(dispute_id).unwrap();
        assert_eq!(dispute.chain_id, ChainId(1));
        assert_eq!(dispute.status, DisputeStatus::Open);
    }

    #[test]
    fn test_submit_evidence() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        let raiser = AccountId::from_bytes([1; 32]);
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::FraudulentExit,
            raiser,
            Jurisdiction::Host(ChainId(0)),
            None,
        ).unwrap();

        // Submit evidence (by the dispute raiser)
        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([2; 32]));

        contract.submit_evidence(dispute_id, Evidence::FraudProof(fraud_proof), raiser).unwrap();

        let dispute = contract.get_dispute(dispute_id).unwrap();
        assert_eq!(dispute.evidence.len(), 1);
    }

    #[test]
    fn test_evidence_window_closed() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        let raiser = AccountId::from_bytes([1; 32]);
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::StateRootDispute,
            raiser,
            Jurisdiction::Root,
            None,
        ).unwrap();

        // Move past evidence deadline
        contract.set_current_block(200_000);

        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([2; 32]));

        let result = contract.submit_evidence(dispute_id, Evidence::FraudProof(fraud_proof), raiser);
        assert!(matches!(result, Err(ArbitrationError::EvidenceWindowClosed)));
    }

    #[test]
    fn test_jury_selection_requires_evidence() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            Jurisdiction::Root,
            None,
        ).unwrap();

        // Try to select jury without evidence
        let result = contract.select_jury(dispute_id, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_submit_jury_vote() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        // Setup validators for jury selection
        for i in 0..20 {
            let validator = AccountId::from_bytes([i; 32]);
            contract.register_validator(validator, 1_000_000, 0);
            contract.validator_credits.initialize_validator(validator, 1000, 0);
        }

        let raiser = AccountId::from_bytes([99; 32]);
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            raiser,
            Jurisdiction::Root,
            None,
        ).unwrap();

        // Submit evidence (validator 0 is authorized as a registered validator)
        let validator0 = AccountId::from_bytes([0; 32]);
        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([50; 32]));
        contract.submit_evidence(dispute_id, Evidence::FraudProof(fraud_proof), validator0).unwrap();

        // Select jury
        let jury = contract.select_jury(dispute_id, Some(13)).unwrap();
        assert!(jury.len() >= 7);

        // Submit vote from jury member
        contract.submit_jury_vote(
            dispute_id,
            jury[0],
            Verdict::Guilty,
            Some("Clear evidence".to_string()),
        ).unwrap();

        let dispute = contract.get_dispute(dispute_id).unwrap();
        assert_eq!(dispute.jury_votes.len(), 1);
    }

    #[test]
    fn test_not_jury_member() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        // Setup validators
        for i in 0..20 {
            let validator = AccountId::from_bytes([i; 32]);
            contract.register_validator(validator, 1_000_000, 0);
            contract.validator_credits.initialize_validator(validator, 1000, 0);
        }

        let raiser = AccountId::from_bytes([99; 32]);
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            raiser,
            Jurisdiction::Root,
            None,
        ).unwrap();

        let validator0 = AccountId::from_bytes([0; 32]);
        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([50; 32]));
        contract.submit_evidence(dispute_id, Evidence::FraudProof(fraud_proof), validator0).unwrap();

        contract.select_jury(dispute_id, Some(13)).unwrap();

        // Try to vote as non-jury member
        let non_juror = AccountId::from_bytes([88; 32]);
        let result = contract.submit_jury_vote(dispute_id, non_juror, Verdict::Guilty, None);
        assert!(matches!(result, Err(ArbitrationError::NotJuryMember)));
    }

    #[test]
    fn test_tally_votes_and_reward() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        // Setup validators
        for i in 0..20 {
            let validator = AccountId::from_bytes([i; 32]);
            contract.register_validator(validator, 1_000_000, 0);
            contract.validator_credits.initialize_validator(validator, 1000, 0);
        }

        let raiser = AccountId::from_bytes([99; 32]);
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            raiser,
            Jurisdiction::Root,
            None,
        ).unwrap();

        let validator0 = AccountId::from_bytes([0; 32]);
        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([50; 32]));
        contract.submit_evidence(dispute_id, Evidence::FraudProof(fraud_proof), validator0).unwrap();

        let jury = contract.select_jury(dispute_id, Some(7)).unwrap();

        // All jury members vote guilty
        for juror in &jury {
            contract.submit_jury_vote(dispute_id, *juror, Verdict::Guilty, None).unwrap();
        }

        // Tally votes
        let decision = contract.tally_votes(dispute_id).unwrap();
        assert_eq!(decision.verdict, Verdict::Guilty);

        // Check jury members were rewarded
        for juror in &jury {
            let vc = contract.validator_credits.get_total_vc(juror);
            assert!(vc >= ARBITRATION_VC_REWARD as u64);
        }
    }

    #[test]
    fn test_enforce_verdict() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        for i in 0..20 {
            let validator = AccountId::from_bytes([i; 32]);
            contract.register_validator(validator, 1_000_000, 0);
            contract.validator_credits.initialize_validator(validator, 1000, 0);
        }

        let raiser = AccountId::from_bytes([99; 32]);
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            raiser,
            Jurisdiction::Root,
            None,
        ).unwrap();

        let validator0 = AccountId::from_bytes([0; 32]);
        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([50; 32]));
        contract.submit_evidence(dispute_id, Evidence::FraudProof(fraud_proof), validator0).unwrap();

        let jury = contract.select_jury(dispute_id, Some(7)).unwrap();
        for juror in &jury {
            contract.submit_jury_vote(dispute_id, *juror, Verdict::Guilty, None).unwrap();
        }

        contract.tally_votes(dispute_id).unwrap();

        // Enforce verdict (validator0 is authorized as a registered validator)
        let enforcement = Enforcement::SlashValidator {
            validator: AccountId::from_bytes([50; 32]),
            amount: 100_000,
            severity: crate::types::FraudSeverity::Critical,
        };

        contract.enforce_verdict(dispute_id, enforcement.clone(), validator0).unwrap();

        let dispute = contract.get_dispute(dispute_id).unwrap();
        assert!(dispute.decision.as_ref().unwrap().enforcement.is_some());
    }

    #[test]
    fn test_appeal_escalation() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        for i in 0..20 {
            let validator = AccountId::from_bytes([i; 32]);
            contract.register_validator(validator, 1_000_000, 0);
            contract.validator_credits.initialize_validator(validator, 1000, 0);
        }

        let raiser = AccountId::from_bytes([99; 32]);
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            raiser,
            Jurisdiction::Local(ChainId(1)),
            None,
        ).unwrap();

        let validator0 = AccountId::from_bytes([0; 32]);
        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([50; 32]));
        contract.submit_evidence(dispute_id, Evidence::FraudProof(fraud_proof), validator0).unwrap();

        let jury = contract.select_jury(dispute_id, Some(7)).unwrap();
        for juror in &jury {
            contract.submit_jury_vote(dispute_id, *juror, Verdict::NotGuilty, None).unwrap();
        }

        contract.tally_votes(dispute_id).unwrap();

        // Appeal to higher jurisdiction
        let appealed_id = contract.appeal_decision(
            dispute_id,
            AccountId::from_bytes([99; 32]),
            Jurisdiction::Host(ChainId(0)),
        ).unwrap();

        // Check original is marked appealed
        let original = contract.get_dispute(dispute_id).unwrap();
        assert_eq!(original.status, DisputeStatus::Appealed);

        // Check new dispute has same evidence
        let appealed = contract.get_dispute(appealed_id).unwrap();
        assert_eq!(appealed.evidence.len(), original.evidence.len());
    }

    #[test]
    fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();

        {
            let db = Database::open(temp_dir.path()).unwrap();
            let validator_credits = ValidatorCreditsManager::new();
            let mut contract = ArbitrationContract::new(db, validator_credits);

            contract.set_current_block(1000);

            contract.raise_dispute(
                ChainId(1),
                DisputeType::ValidatorMisconduct,
                AccountId::from_bytes([1; 32]),
                Jurisdiction::Root,
                None,
            ).unwrap();
        }

        // Reload from disk
        {
            let db = Database::open(temp_dir.path()).unwrap();
            let validator_credits = ValidatorCreditsManager::new();
            let mut contract = ArbitrationContract::new(db, validator_credits);

            contract.load_dispute(1).unwrap();
            let dispute = contract.get_dispute(1).unwrap();
            assert_eq!(dispute.chain_id, ChainId(1));
        }
    }

    #[test]
    fn test_expire_stale_disputes_no_evidence() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        // Raise a dispute but don't submit evidence
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            Jurisdiction::Root,
            None,
        ).unwrap();

        // Move past evidence deadline
        contract.set_current_block(200_000);

        // Expire stale disputes
        let expired = contract.expire_stale_disputes();
        assert!(expired.contains(&dispute_id));

        let dispute = contract.get_dispute(dispute_id).unwrap();
        assert_eq!(dispute.status, DisputeStatus::Expired);
    }

    #[test]
    fn test_expire_stale_disputes_max_duration() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        // Raise a dispute with evidence
        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([50; 32]));
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            Jurisdiction::Root,
            Some(Evidence::FraudProof(fraud_proof)),
        ).unwrap();

        // Move past max duration (58 days)
        contract.set_current_block(1000 + MAX_DISPUTE_DURATION + 1);

        // Expire stale disputes
        let expired = contract.expire_stale_disputes();
        assert!(expired.contains(&dispute_id));

        let dispute = contract.get_dispute(dispute_id).unwrap();
        assert_eq!(dispute.status, DisputeStatus::Expired);
    }

    #[test]
    fn test_has_blocking_disputes() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        // Initially no blocking disputes
        assert!(!contract.has_blocking_disputes(ChainId(1)));

        // Raise a dispute
        contract.raise_dispute(
            ChainId(1),
            DisputeType::FraudulentExit,
            AccountId::from_bytes([1; 32]),
            Jurisdiction::Root,
            None,
        ).unwrap();

        // Now there is a blocking dispute
        assert!(contract.has_blocking_disputes(ChainId(1)));

        // Different chain should not be blocked
        assert!(!contract.has_blocking_disputes(ChainId(2)));
    }

    #[test]
    fn test_time_until_expiry() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            Jurisdiction::Root,
            None,
        ).unwrap();

        // Check time until expiry
        let time = contract.time_until_expiry(dispute_id).unwrap();
        assert_eq!(time, MAX_DISPUTE_DURATION);

        // Move forward
        contract.set_current_block(1000 + 100_000);
        let time = contract.time_until_expiry(dispute_id).unwrap();
        assert_eq!(time, MAX_DISPUTE_DURATION - 100_000);
    }

    #[test]
    fn test_dismiss_dispute() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        // Register a validator who can dismiss
        let validator = AccountId::from_bytes([10; 32]);
        contract.register_validator(validator, 1_000_000, 0);

        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            Jurisdiction::Root,
            None,
        ).unwrap();

        // Dismiss the dispute (by registered validator)
        contract.dismiss_dispute(dispute_id, "Frivolous claim", validator).unwrap();

        let dispute = contract.get_dispute(dispute_id).unwrap();
        assert_eq!(dispute.status, DisputeStatus::Dismissed);
    }

    #[test]
    fn test_unauthorized_evidence_submission() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        let raiser = AccountId::from_bytes([1; 32]);
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            raiser,
            Jurisdiction::Root,
            None,
        ).unwrap();

        // Try to submit evidence as unauthorized party (not raiser, accused, or validator)
        let unauthorized = AccountId::from_bytes([99; 32]);
        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([50; 32]));
        let result = contract.submit_evidence(dispute_id, Evidence::FraudProof(fraud_proof), unauthorized);

        // Should fail with Unauthorized error
        assert!(matches!(result, Err(ArbitrationError::Unauthorized { .. })));
    }

    #[test]
    fn test_unauthorized_dismiss() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            Jurisdiction::Root,
            None,
        ).unwrap();

        // Try to dismiss as non-validator
        let non_validator = AccountId::from_bytes([99; 32]);
        let result = contract.dismiss_dispute(dispute_id, "Frivolous", non_validator);

        // Should fail with Unauthorized error
        assert!(matches!(result, Err(ArbitrationError::Unauthorized { .. })));
    }

    #[test]
    fn test_unauthorized_enforce_verdict() {
        let (mut contract, _temp) = setup_arbitration();
        contract.set_current_block(1000);

        // Setup validators
        for i in 0..20 {
            let validator = AccountId::from_bytes([i; 32]);
            contract.register_validator(validator, 1_000_000, 0);
            contract.validator_credits.initialize_validator(validator, 1000, 0);
        }

        let raiser = AccountId::from_bytes([99; 32]);
        let dispute_id = contract.raise_dispute(
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            raiser,
            Jurisdiction::Root,
            None,
        ).unwrap();

        let validator0 = AccountId::from_bytes([0; 32]);
        let fraud_proof = create_test_fraud_proof(AccountId::from_bytes([50; 32]));
        contract.submit_evidence(dispute_id, Evidence::FraudProof(fraud_proof), validator0).unwrap();

        let jury = contract.select_jury(dispute_id, Some(7)).unwrap();
        for juror in &jury {
            contract.submit_jury_vote(dispute_id, *juror, Verdict::Guilty, None).unwrap();
        }

        contract.tally_votes(dispute_id).unwrap();

        // Try to enforce as non-validator
        let non_validator = AccountId::from_bytes([88; 32]);
        let enforcement = Enforcement::SlashValidator {
            validator: AccountId::from_bytes([50; 32]),
            amount: 100_000,
            severity: crate::types::FraudSeverity::Critical,
        };

        let result = contract.enforce_verdict(dispute_id, enforcement, non_validator);

        // Should fail with Unauthorized error
        assert!(matches!(result, Err(ArbitrationError::Unauthorized { .. })));
    }
}
