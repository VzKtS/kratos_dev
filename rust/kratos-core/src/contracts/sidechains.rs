// Sidechains - System contract pour la gestion des sidechains et hostchains
use crate::types::{
    AccountId, Balance, BlockNumber, ChainId, ChainStatus, HostChainInfo, SecurityMode,
    SidechainInfo,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Seuil d'inactivité (90 jours) - SPEC v3.1 Section 2.1
/// 90 days = 90 * 24 * 3600 / 6 = 1,296,000 blocks
pub const INACTIVITY_THRESHOLD: BlockNumber = 1_296_000;

/// Intervalle de vérification pour purge auto
pub const PURGE_CHECK_INTERVAL: BlockNumber = 3_600; // 6 heures

/// Registre des sidechains et hostchains
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainRegistry {
    /// Sidechains par ID
    sidechains: HashMap<ChainId, SidechainInfo>,

    /// Hostchains par ID
    hostchains: HashMap<ChainId, HostChainInfo>,

    /// Mapping sidechain → hostchain
    sidechain_to_host: HashMap<ChainId, ChainId>,

    /// Prochain ID disponible
    next_chain_id: u32,

    /// Dernier bloc de vérification de purge
    last_purge_check: BlockNumber,
}

impl ChainRegistry {
    pub fn new() -> Self {
        Self {
            sidechains: HashMap::new(),
            hostchains: HashMap::new(),
            sidechain_to_host: HashMap::new(),
            next_chain_id: 1, // 0 est réservé pour root chain
            last_purge_check: 0,
        }
    }

    /// Crée une nouvelle sidechain (SPEC v3)
    ///
    /// # Parameters
    /// - `owner`: Account creating the sidechain
    /// - `name`: Optional sidechain name
    /// - `description`: Optional description
    /// - `parent`: Parent chain (for child chains)
    /// - `security_mode`: Security mode (Inherited/Shared/Sovereign)
    /// - `host_id`: Hostchain ID if using Shared mode
    /// - `deposit`: Deposit amount (must meet security mode requirements)
    /// - `block`: Current block number
    pub fn create_sidechain(
        &mut self,
        owner: AccountId,
        name: Option<String>,
        description: Option<String>,
        parent: Option<ChainId>,
        security_mode: SecurityMode,
        host_id: Option<ChainId>,
        deposit: Balance,
        block: BlockNumber,
    ) -> Result<ChainId, ChainError> {
        // Calculate required deposit based on security mode
        let required_deposit = match security_mode {
            SecurityMode::Inherited => crate::types::BASE_DEPOSIT,
            SecurityMode::Shared => {
                // For shared mode, deposit depends on hostchain size
                let host = host_id
                    .and_then(|id| self.hostchains.get(&id))
                    .ok_or(ChainError::HostNotFound)?;
                crate::types::calculate_deposit(security_mode, host.member_chains.len())
            }
            SecurityMode::Sovereign => crate::types::SOVEREIGN_DEPOSIT,
        };

        // Verify sufficient deposit
        if deposit < required_deposit {
            return Err(ChainError::InsufficientDeposit);
        }

        // Verify parent exists if specified
        if let Some(parent_id) = parent {
            if !self.sidechains.contains_key(&parent_id) {
                return Err(ChainError::ParentNotFound);
            }
        }

        // Create sidechain
        let chain_id = ChainId(self.next_chain_id);
        self.next_chain_id += 1;

        let sidechain = SidechainInfo::new(
            chain_id,
            parent,
            owner,
            name,
            description,
            security_mode,
            deposit,
            block,
        );

        self.sidechains.insert(chain_id, sidechain);

        // Auto-affiliate to hostchain if Shared mode
        if security_mode == SecurityMode::Shared {
            if let Some(host) = host_id {
                self.affiliate_sidechain(chain_id, host)?;
            }
        }

        // Auto-assign validators based on security mode
        // For Inherited mode: copy parent's validators
        // For Shared mode: copy hostchain pool
        // For Sovereign mode: starts with empty set (manual management)
        if security_mode != SecurityMode::Sovereign {
            let _ = self.assign_validators(chain_id)?;
        }

        Ok(chain_id)
    }

    /// Enregistre une activité sur une sidechain
    pub fn record_activity(&mut self, chain_id: ChainId, block: BlockNumber) -> Result<(), ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        sidechain.record_activity(block);
        Ok(())
    }

    /// Purge une sidechain inactive
    pub fn purge_sidechain(&mut self, chain_id: ChainId, current_block: BlockNumber) -> Result<Balance, ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        if !sidechain.is_inactive(current_block, INACTIVITY_THRESHOLD) {
            return Err(ChainError::ChainNotInactive);
        }

        sidechain.status = ChainStatus::Purged;
        let deposit = sidechain.deposit;

        // Retire de la hostchain si affiliée
        if let Some(host_id) = self.sidechain_to_host.remove(&chain_id) {
            if let Some(host) = self.hostchains.get_mut(&host_id) {
                let _ = host.remove_member(&chain_id);
            }
        }

        Ok(deposit)
    }

    /// Vérification automatique et purge des chaînes inactives
    pub fn auto_purge_inactive(&mut self, current_block: BlockNumber) -> Vec<ChainId> {
        if current_block < self.last_purge_check + PURGE_CHECK_INTERVAL {
            return Vec::new();
        }

        self.last_purge_check = current_block;

        let mut purged = Vec::new();

        // Identifie les chaînes inactives
        let inactive_chains: Vec<ChainId> = self
            .sidechains
            .iter()
            .filter(|(_, sc)| {
                sc.status == ChainStatus::Active && sc.is_inactive(current_block, INACTIVITY_THRESHOLD)
            })
            .map(|(id, _)| *id)
            .collect();

        // Purge chacune
        for chain_id in inactive_chains {
            if self.purge_sidechain(chain_id, current_block).is_ok() {
                purged.push(chain_id);
            }
        }

        purged
    }

    /// Crée une hostchain
    pub fn create_hostchain(&mut self, creator: AccountId, block: BlockNumber) -> ChainId {
        let host_id = ChainId(self.next_chain_id);
        self.next_chain_id += 1;

        let hostchain = HostChainInfo::new(host_id, creator, block);
        self.hostchains.insert(host_id, hostchain);

        host_id
    }

    /// Affilie une sidechain à une hostchain
    pub fn affiliate_sidechain(
        &mut self,
        sidechain_id: ChainId,
        host_id: ChainId,
    ) -> Result<(), ChainError> {
        // Vérifie que les deux existent
        if !self.sidechains.contains_key(&sidechain_id) {
            return Err(ChainError::ChainNotFound);
        }

        let host = self
            .hostchains
            .get_mut(&host_id)
            .ok_or(ChainError::HostNotFound)?;

        host.add_member(sidechain_id)?;
        self.sidechain_to_host.insert(sidechain_id, host_id);

        Ok(())
    }

    /// Retire une sidechain d'une hostchain
    pub fn leave_host(&mut self, sidechain_id: ChainId) -> Result<(), ChainError> {
        let host_id = self
            .sidechain_to_host
            .remove(&sidechain_id)
            .ok_or(ChainError::NotAffiliated)?;

        let host = self
            .hostchains
            .get_mut(&host_id)
            .ok_or(ChainError::HostNotFound)?;

        host.remove_member(&sidechain_id)?;

        Ok(())
    }

    /// Récupère une sidechain
    pub fn get_sidechain(&self, chain_id: &ChainId) -> Option<&SidechainInfo> {
        self.sidechains.get(chain_id)
    }

    /// Récupère une hostchain
    pub fn get_hostchain(&self, chain_id: &ChainId) -> Option<&HostChainInfo> {
        self.hostchains.get(chain_id)
    }

    /// Liste toutes les sidechains actives
    pub fn active_sidechains(&self) -> Vec<&SidechainInfo> {
        self.sidechains
            .values()
            .filter(|sc| sc.status == ChainStatus::Active)
            .collect()
    }

    /// Compte des sidechains
    pub fn sidechain_count(&self) -> usize {
        self.sidechains.len()
    }

    /// Compte des hostchains
    pub fn hostchain_count(&self) -> usize {
        self.hostchains.len()
    }

    /// Assign validators to a sidechain based on its security mode (SPEC v3)
    ///
    /// # Security Mode Behaviors:
    /// - **Inherited**: Copies validators from parent chain
    /// - **Shared**: Assigns validators from hostchain pool (with rotation)
    /// - **Sovereign**: Uses chain's own validator set (no auto-assignment)
    ///
    /// # Returns
    /// Vec of assigned validator AccountIds
    pub fn assign_validators(
        &mut self,
        chain_id: ChainId,
    ) -> Result<Vec<AccountId>, ChainError> {
        // Get sidechain info (immutably first)
        let (security_mode, parent, host_id) = {
            let sidechain = self
                .sidechains
                .get(&chain_id)
                .ok_or(ChainError::ChainNotFound)?;

            (
                sidechain.security_mode,
                sidechain.parent,
                self.sidechain_to_host.get(&chain_id).copied(),
            )
        };

        // Assign based on security mode
        match security_mode {
            SecurityMode::Inherited => {
                // Get parent's validators
                let parent_id = parent.ok_or(ChainError::ParentNotFound)?;
                let parent_validators = self
                    .sidechains
                    .get(&parent_id)
                    .ok_or(ChainError::ParentNotFound)?
                    .validators
                    .clone();

                // Assign to child
                let sidechain = self.sidechains.get_mut(&chain_id).unwrap();
                sidechain.validators = parent_validators.clone();

                Ok(parent_validators.into_iter().collect())
            }

            SecurityMode::Shared => {
                // Get validators from hostchain pool
                let host = host_id
                    .and_then(|id| self.hostchains.get(&id))
                    .ok_or(ChainError::HostNotFound)?;

                let pool_validators = host.validator_pool.clone();

                // Assign to sidechain
                let sidechain = self.sidechains.get_mut(&chain_id).unwrap();
                sidechain.validators = pool_validators.clone();

                Ok(pool_validators.into_iter().collect())
            }

            SecurityMode::Sovereign => {
                // Sovereign chains manage their own validators
                // Return current validator set
                let sidechain = self.sidechains.get(&chain_id).unwrap();
                Ok(sidechain.validators.iter().copied().collect())
            }
        }
    }

    /// Add a validator to a sidechain's validator set
    /// Only valid for Sovereign mode chains
    pub fn add_validator_to_chain(
        &mut self,
        chain_id: ChainId,
        validator: AccountId,
    ) -> Result<(), ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        // Only Sovereign chains can manually add validators
        if sidechain.security_mode != SecurityMode::Sovereign {
            return Err(ChainError::InvalidSecurityMode);
        }

        sidechain.add_validator(validator)?;
        Ok(())
    }

    /// Remove a validator from a sidechain's validator set
    /// Only valid for Sovereign mode chains
    pub fn remove_validator_from_chain(
        &mut self,
        chain_id: ChainId,
        validator: &AccountId,
    ) -> Result<(), ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        // Only Sovereign chains can manually remove validators
        if sidechain.security_mode != SecurityMode::Sovereign {
            return Err(ChainError::InvalidSecurityMode);
        }

        sidechain.remove_validator(validator)?;
        Ok(())
    }

    /// Add a validator to a hostchain's shared pool
    pub fn add_validator_to_hostchain(
        &mut self,
        host_id: ChainId,
        validator: AccountId,
    ) -> Result<(), ChainError> {
        let host = self
            .hostchains
            .get_mut(&host_id)
            .ok_or(ChainError::HostNotFound)?;

        host.add_validator(validator)?;

        // Re-assign validators to all affiliated sidechains
        let affiliated_chains: Vec<ChainId> = self
            .sidechain_to_host
            .iter()
            .filter(|(_, h)| **h == host_id)
            .map(|(c, _)| *c)
            .collect();

        for chain_id in affiliated_chains {
            self.assign_validators(chain_id)?;
        }

        Ok(())
    }

    /// Remove a validator from a hostchain's shared pool
    pub fn remove_validator_from_hostchain(
        &mut self,
        host_id: ChainId,
        validator: &AccountId,
    ) -> Result<(), ChainError> {
        let host = self
            .hostchains
            .get_mut(&host_id)
            .ok_or(ChainError::HostNotFound)?;

        host.remove_validator(validator)?;

        // Re-assign validators to all affiliated sidechains
        let affiliated_chains: Vec<ChainId> = self
            .sidechain_to_host
            .iter()
            .filter(|(_, h)| **h == host_id)
            .map(|(c, _)| *c)
            .collect();

        for chain_id in affiliated_chains {
            self.assign_validators(chain_id)?;
        }

        Ok(())
    }

    // ==================== SPEC v3.1: Enhanced Purge System ====================

    /// Check if a sidechain meets any purge trigger conditions (SPEC v3.1 Section 2.1)
    ///
    /// Returns Some(PurgeTrigger) if conditions are met, None otherwise
    pub fn check_purge_triggers(
        &self,
        chain_id: ChainId,
        current_block: BlockNumber,
    ) -> Option<crate::types::PurgeTrigger> {
        let sidechain = self.sidechains.get(&chain_id)?;

        // 1. Inactivity: ≥ 90 days (1,296,000 blocks)
        if sidechain.is_inactive(current_block, crate::types::INACTIVITY_THRESHOLD_V3_1) {
            return Some(crate::types::PurgeTrigger::Inactivity);
        }

        // 2. Governance Failure: 3 consecutive failed votes
        if sidechain.governance_failures >= crate::types::GOVERNANCE_FAILURE_THRESHOLD {
            return Some(crate::types::PurgeTrigger::GovernanceFailure);
        }

        // 3. Validator Fraud: ≥ 33% of validators slashed
        // SPEC v3.1: Use cross-multiplication to avoid integer division truncation
        // Instead of: (slashed * 100) / total >= 33
        // Use: slashed * 100 >= total * 33
        if !sidechain.validators.is_empty() {
            let slashed = sidechain.slashed_validators_count as u64;
            let total = sidechain.validators.len() as u64;
            let threshold = crate::types::VALIDATOR_FRAUD_THRESHOLD_PERCENT as u64;
            // Cross-multiply to avoid truncation (use saturating_mul for safety)
            if slashed.saturating_mul(100) >= total.saturating_mul(threshold) {
                return Some(crate::types::PurgeTrigger::ValidatorFraud);
            }
        }

        // 4. Security Insolvency: Unable to pay parent fees
        // TODO: Implement once fee system exists (Phase 4+ dependency)

        // 5. State Divergence: Invalid state root detected (SPEC v3.1 Phase 4)
        if sidechain.state_divergence_detected_at.is_some() {
            return Some(crate::types::PurgeTrigger::StateDivergence);
        }

        None
    }

    /// Trigger purge for a sidechain (SPEC v3.1 Section 2.2)
    ///
    /// Transitions chain to PendingPurge status with 30-day warning period
    pub fn trigger_purge(
        &mut self,
        chain_id: ChainId,
        trigger: crate::types::PurgeTrigger,
        current_block: BlockNumber,
    ) -> Result<(), ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        // Only Active or Inactive chains can enter purge
        if !matches!(sidechain.status, crate::types::ChainStatus::Active | crate::types::ChainStatus::Inactive) {
            return Err(ChainError::InvalidState);
        }

        sidechain.status = crate::types::ChainStatus::PendingPurge;
        sidechain.purge_triggered_at = Some(current_block);
        sidechain.purge_trigger = Some(trigger);

        Ok(())
    }

    /// Advance purge state machine (SPEC v3.1 Section 2.2)
    ///
    /// State transitions:
    /// PendingPurge (30d) → Frozen → Snapshot → WithdrawalWindow (30d) → Purged
    pub fn advance_purge_state(
        &mut self,
        chain_id: ChainId,
        current_block: BlockNumber,
    ) -> Result<crate::types::ChainStatus, ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        match sidechain.status {
            crate::types::ChainStatus::PendingPurge => {
                // Check if warning period (30 days) has passed
                if let Some(triggered_at) = sidechain.purge_triggered_at {
                    if current_block >= triggered_at + crate::types::PURGE_WARNING_PERIOD {
                        // Transition to Frozen
                        sidechain.status = crate::types::ChainStatus::Frozen;
                        sidechain.frozen_at = Some(current_block);
                    }
                }
            }

            crate::types::ChainStatus::Frozen => {
                // SPEC v3.1: Capture final state root before transitioning to Snapshot
                // This ensures withdrawal verification has a valid state reference
                if sidechain.snapshot_state_root.is_none() {
                    // Copy last verified state root as snapshot state root
                    if let Some(state_root) = sidechain.last_verified_state_root {
                        sidechain.snapshot_state_root = Some(state_root);
                    }
                    // Note: If no state root exists, chain can still transition
                    // but withdrawals will require alternative verification
                }
                // Transition to Snapshot
                sidechain.status = crate::types::ChainStatus::Snapshot;
                sidechain.snapshot_at = Some(current_block);
            }

            crate::types::ChainStatus::Snapshot => {
                // Immediately transition to WithdrawalWindow
                sidechain.status = crate::types::ChainStatus::WithdrawalWindow;
                sidechain.withdrawal_window_start = Some(current_block);
            }

            crate::types::ChainStatus::WithdrawalWindow => {
                // Check if withdrawal window (30 days) has passed
                if let Some(window_start) = sidechain.withdrawal_window_start {
                    if current_block >= window_start + crate::types::WITHDRAWAL_WINDOW_DURATION {
                        // Final transition to Purged
                        sidechain.status = crate::types::ChainStatus::Purged;

                        // Remove from hostchain if affiliated
                        if let Some(host_id) = self.sidechain_to_host.remove(&chain_id) {
                            if let Some(host) = self.hostchains.get_mut(&host_id) {
                                let _ = host.remove_member(&chain_id);
                            }
                        }
                    }
                }
            }

            _ => {
                // No transition for Active, Inactive, or Purged states
            }
        }

        Ok(sidechain.status)
    }

    /// Automatic purge trigger detection and state advancement (SPEC v3.1)
    ///
    /// Runs periodically to:
    /// 1. Check all chains for purge triggers
    /// 2. Advance purge state machine for chains in purge process
    ///
    /// Returns list of (ChainId, PurgeTrigger) for newly triggered purges
    pub fn auto_purge_v3_1(
        &mut self,
        current_block: BlockNumber,
    ) -> Vec<(ChainId, crate::types::PurgeTrigger)> {
        if current_block < self.last_purge_check + PURGE_CHECK_INTERVAL {
            return Vec::new();
        }

        self.last_purge_check = current_block;
        let mut newly_triggered = Vec::new();

        // Collect chain IDs to avoid borrow conflicts
        let chain_ids: Vec<ChainId> = self.sidechains.keys().copied().collect();

        for chain_id in chain_ids {
            // 1. Check for trigger conditions on active chains
            if let Some(sidechain) = self.sidechains.get(&chain_id) {
                if matches!(sidechain.status, crate::types::ChainStatus::Active | crate::types::ChainStatus::Inactive) {
                    if let Some(trigger) = self.check_purge_triggers(chain_id, current_block) {
                        if self.trigger_purge(chain_id, trigger, current_block).is_ok() {
                            newly_triggered.push((chain_id, trigger));
                        }
                    }
                }
            }

            // 2. Advance state machine for chains already in purge
            // Keep advancing through immediate transitions (Frozen → Snapshot → WithdrawalWindow)
            let mut max_iterations = 5; // Prevent infinite loops
            while max_iterations > 0 {
                let prev_status = if let Some(sidechain) = self.sidechains.get(&chain_id) {
                    sidechain.status
                } else {
                    break;
                };

                if self.advance_purge_state(chain_id, current_block).is_err() {
                    break;
                }

                let new_status = if let Some(sidechain) = self.sidechains.get(&chain_id) {
                    sidechain.status
                } else {
                    break;
                };

                // If state didn't change, we're done
                if prev_status == new_status {
                    break;
                }

                max_iterations -= 1;
            }
        }

        newly_triggered
    }

    /// Increment governance failure counter (called by governance module)
    pub fn record_governance_failure(&mut self, chain_id: ChainId) -> Result<(), ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        sidechain.governance_failures += 1;
        Ok(())
    }

    /// Reset governance failure counter (called on successful vote)
    pub fn reset_governance_failures(&mut self, chain_id: ChainId) -> Result<(), ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        sidechain.governance_failures = 0;
        Ok(())
    }

    /// Record validator slashing (called by fraud detection)
    pub fn slash_validator(
        &mut self,
        chain_id: ChainId,
        _validator: &AccountId,
    ) -> Result<(), ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        sidechain.slashed_validators_count += 1;
        Ok(())
    }

    /// Update verified state root for a sidechain (SPEC v3.1 Phase 4)
    ///
    /// Should be called when a valid state root is confirmed
    pub fn update_verified_state_root(
        &mut self,
        chain_id: ChainId,
        state_root: crate::types::Hash,
    ) -> Result<(), ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        sidechain.last_verified_state_root = Some(state_root);
        Ok(())
    }

    /// Report state divergence for a sidechain (SPEC v3.1 Phase 4)
    ///
    /// Should be called when an invalid state root is detected
    /// This marks the chain for purge via StateDivergence trigger
    ///
    /// SECURITY FIX #7: Added reporter parameter and access control
    /// Only validators of the affected chain or parent chain can report divergence
    /// This prevents malicious parties from triggering false divergence reports
    pub fn report_state_divergence(
        &mut self,
        chain_id: ChainId,
        block_number: crate::types::BlockNumber,
        reporter: AccountId,
    ) -> Result<(), ChainError> {
        // SECURITY FIX #7: First validate authorization before modifying state
        // Check if reporter is authorized (validator on the chain or parent chain)
        let is_authorized = {
            let sidechain = self
                .sidechains
                .get(&chain_id)
                .ok_or(ChainError::ChainNotFound)?;

            // Check if reporter is a validator on the sidechain
            let is_sidechain_validator = sidechain.validators.contains(&reporter);

            // Check if reporter is a validator on the parent chain (if any)
            let is_parent_validator = sidechain.parent
                .and_then(|parent_id| self.sidechains.get(&parent_id))
                .map(|parent| parent.validators.contains(&reporter))
                .unwrap_or(false);

            // Check if reporter is the chain owner (has authority to report issues)
            let is_owner = sidechain.owner == reporter;

            is_sidechain_validator || is_parent_validator || is_owner
        };

        if !is_authorized {
            return Err(ChainError::Unauthorized {
                action: "report_state_divergence".to_string(),
                required: "chain validator, parent validator, or chain owner".to_string(),
            });
        }

        // Now safe to modify
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        // Only report once
        if sidechain.state_divergence_detected_at.is_none() {
            sidechain.state_divergence_detected_at = Some(block_number);
        }

        Ok(())
    }

    /// Set snapshot state root for a sidechain (SPEC v3.1 Phase 4)
    ///
    /// Should be called during Frozen→Snapshot transition to commit the final state root
    /// This state root can be used for withdrawal verification and dispute resolution
    pub fn set_snapshot_state_root(
        &mut self,
        chain_id: ChainId,
        state_root_hash: crate::types::Hash,
    ) -> Result<(), ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        // Only allow setting snapshot when in Snapshot status
        if sidechain.status != crate::types::ChainStatus::Snapshot {
            return Err(ChainError::InvalidState);
        }

        sidechain.snapshot_state_root = Some(state_root_hash);
        Ok(())
    }

    // ==================== SPEC v3.1 Phase 5: Fraud Proofs ====================

    /// Verify a fraud proof and apply slashing if valid (SPEC v3.1 Phase 5)
    ///
    /// # Parameters
    /// - `proof`: The fraud proof to verify
    /// - `current_block`: Current block number
    ///
    /// # Returns
    /// - `Ok(FraudProofVerification)`: Verification result if valid
    /// - `Err(ChainError)`: If proof is invalid or chain not found
    pub fn verify_fraud_proof(
        &mut self,
        proof: crate::types::FraudProof,
        current_block: BlockNumber,
    ) -> Result<crate::types::FraudProofVerification, ChainError> {
        // 1. Verify the fraud proof cryptographically
        proof.verify()
            .map_err(|e| ChainError::FraudProofInvalid(e.to_string()))?;

        // 2. Determine accused validator and severity
        let accused = proof.accused_validator();
        let severity = crate::types::FraudSeverity::from_fraud_proof(&proof);
        let fraud_block = proof.fraud_block_number();

        // 3. Check if proof is not expired (30 days window)
        const FRAUD_PROOF_VALIDITY_PERIOD: BlockNumber = 432_000; // 30 days
        if current_block > fraud_block + FRAUD_PROOF_VALIDITY_PERIOD {
            return Err(ChainError::FraudProofExpired);
        }

        // 4. Apply slashing to the validator
        if let Some(chain_id) = proof.affected_chain() {
            self.slash_validator(chain_id, &accused)?;
        }

        Ok(crate::types::FraudProofVerification {
            is_valid: true,
            accused,
            severity,
            fraud_block,
        })
    }

    /// Apply fraud proof slashing with automatic purge at 33% threshold (SPEC v3.1 Phase 5)
    ///
    /// # Parameters
    /// - `proof`: The verified fraud proof
    /// - `current_block`: Current block number
    ///
    /// # Returns
    /// - `Ok(bool)`: True if purge was triggered, false otherwise
    /// - `Err(ChainError)`: If chain not found or invalid state
    pub fn apply_fraud_proof(
        &mut self,
        proof: crate::types::FraudProof,
        current_block: BlockNumber,
    ) -> Result<bool, ChainError> {
        // Verify the fraud proof first
        let verification = self.verify_fraud_proof(proof.clone(), current_block)?;

        // Get the affected chain (if applicable)
        let chain_id = match proof.affected_chain() {
            Some(id) => id,
            None => {
                // For non-chain-specific fraud (e.g., validator misbehavior),
                // we'd need to look up which chain the validator was assigned to.
                // For now, just slash the validator without chain context.
                return Ok(false);
            }
        };

        let sidechain = self.sidechains.get(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        // Check if 33% threshold is reached
        let total_validators = sidechain.validators.len();
        if total_validators == 0 {
            return Ok(false);
        }

        let fraud_percent = (sidechain.slashed_validators_count * 100) / total_validators;

        // If we've reached the 33% fraud threshold, trigger purge
        if fraud_percent >= crate::types::VALIDATOR_FRAUD_THRESHOLD_PERCENT {
            self.trigger_purge(
                chain_id,
                crate::types::PurgeTrigger::ValidatorFraud,
                current_block,
            )?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Withdraw funds during withdrawal window (SPEC v3.1 Section 2.3)
    ///
    /// SECURITY FIX #33: Full implementation of withdrawal with asset tracking
    /// Constitution Article I §6: "Exit is a fundamental right"
    /// Constitution Article IX: "No capital SHALL be frozen without a withdrawal path"
    ///
    /// Only available when chain is in WithdrawalWindow status
    pub fn withdraw_from_purged_chain(
        &mut self,
        chain_id: ChainId,
        owner: AccountId,
    ) -> Result<WithdrawalResult, ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        // Only allow withdrawal during withdrawal window
        if sidechain.status != crate::types::ChainStatus::WithdrawalWindow {
            return Err(ChainError::InvalidState);
        }

        // SECURITY FIX #33: Track withdrawals to prevent double-withdrawal
        if sidechain.withdrawn_accounts.contains(&owner) {
            return Err(ChainError::AlreadyWithdrawn);
        }

        // Calculate withdrawal amount based on ownership
        // For chain owner: return full deposit
        // For other users: return proportional share based on snapshot state
        let withdrawal_amount = if owner == sidechain.owner {
            sidechain.deposit
        } else {
            // For non-owners, withdrawal requires Merkle proof of balance at snapshot
            // This is handled by withdraw_with_proof()
            return Err(ChainError::RequiresMerkleProof);
        };

        // Mark as withdrawn
        sidechain.withdrawn_accounts.insert(owner);

        Ok(WithdrawalResult {
            chain_id,
            recipient: owner,
            amount: withdrawal_amount,
            withdrawal_type: WithdrawalType::OwnerDeposit,
        })
    }

    /// SECURITY FIX #33: Withdraw with Merkle proof for non-owner accounts
    ///
    /// Constitution Article I §6: "Exit is a fundamental right"
    /// Users can prove their balance at snapshot time using Merkle proof
    pub fn withdraw_with_proof(
        &mut self,
        chain_id: ChainId,
        account: AccountId,
        balance: Balance,
        merkle_proof: crate::types::MerkleProof,
    ) -> Result<WithdrawalResult, ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        // Only allow withdrawal during withdrawal window
        if sidechain.status != crate::types::ChainStatus::WithdrawalWindow {
            return Err(ChainError::InvalidState);
        }

        // Check not already withdrawn
        if sidechain.withdrawn_accounts.contains(&account) {
            return Err(ChainError::AlreadyWithdrawn);
        }

        // Verify Merkle proof against snapshot state root
        let snapshot_root = sidechain.snapshot_state_root
            .ok_or(ChainError::NoSnapshotStateRoot)?;

        // Verify the proof
        if !merkle_proof.verify() || merkle_proof.root != snapshot_root {
            return Err(ChainError::InvalidMerkleProof);
        }

        // Mark as withdrawn
        sidechain.withdrawn_accounts.insert(account);

        Ok(WithdrawalResult {
            chain_id,
            recipient: account,
            amount: balance,
            withdrawal_type: WithdrawalType::UserBalance,
        })
    }

    /// SECURITY FIX #33: Emergency exit - ALWAYS possible per Constitution Article VIII
    ///
    /// "No emergency SHALL suspend exit" - This method bypasses ALL restrictions
    /// except basic validation. It is the constitutional guarantee of exit rights.
    pub fn emergency_exit(
        &mut self,
        chain_id: ChainId,
        account: AccountId,
        balance: Balance,
        merkle_proof: Option<crate::types::MerkleProof>,
    ) -> Result<WithdrawalResult, ChainError> {
        let sidechain = self
            .sidechains
            .get_mut(&chain_id)
            .ok_or(ChainError::ChainNotFound)?;

        // SECURITY FIX #33: Emergency exit is ALWAYS allowed regardless of chain status
        // This implements Constitution Article VIII: "No emergency SHALL suspend exit"

        // Check not already withdrawn (this is the ONLY check we keep)
        if sidechain.withdrawn_accounts.contains(&account) {
            return Err(ChainError::AlreadyWithdrawn);
        }

        // For owner, return deposit directly
        if account == sidechain.owner {
            sidechain.withdrawn_accounts.insert(account);
            return Ok(WithdrawalResult {
                chain_id,
                recipient: account,
                amount: sidechain.deposit,
                withdrawal_type: WithdrawalType::EmergencyOwner,
            });
        }

        // For non-owners, verify Merkle proof if available
        // If no proof available but snapshot exists, allow with reduced amount (slippage)
        let withdrawal_amount = if let Some(proof) = merkle_proof {
            if let Some(snapshot_root) = sidechain.snapshot_state_root {
                if proof.verify() && proof.root == snapshot_root {
                    balance
                } else {
                    // Invalid proof, but emergency exit still allowed with penalty
                    // User gets 50% as emergency slippage (better than nothing)
                    balance / 2
                }
            } else {
                // No snapshot, allow full withdrawal on good faith
                balance
            }
        } else {
            // No proof provided, allow with 50% slippage
            balance / 2
        };

        sidechain.withdrawn_accounts.insert(account);

        Ok(WithdrawalResult {
            chain_id,
            recipient: account,
            amount: withdrawal_amount,
            withdrawal_type: WithdrawalType::Emergency,
        })
    }
}

impl Default for ChainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// SECURITY FIX #33: Withdrawal result with full tracking
#[derive(Debug, Clone)]
pub struct WithdrawalResult {
    /// Chain the withdrawal is from
    pub chain_id: ChainId,
    /// Recipient of the withdrawal
    pub recipient: AccountId,
    /// Amount withdrawn
    pub amount: Balance,
    /// Type of withdrawal
    pub withdrawal_type: WithdrawalType,
}

/// SECURITY FIX #33: Type of withdrawal for audit trail
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WithdrawalType {
    /// Owner withdrawing their deposit
    OwnerDeposit,
    /// User withdrawing balance with Merkle proof
    UserBalance,
    /// Emergency withdrawal for owner
    EmergencyOwner,
    /// Emergency withdrawal (may have slippage)
    Emergency,
}

/// Erreurs de gestion de chaînes
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("Dépôt insuffisant")]
    InsufficientDeposit,

    #[error("Chaîne non trouvée")]
    ChainNotFound,

    #[error("Chaîne parente non trouvée")]
    ParentNotFound,

    #[error("Hostchain non trouvée")]
    HostNotFound,

    #[error("Chaîne pas inactive")]
    ChainNotInactive,

    #[error("Pas affiliée à une hostchain")]
    NotAffiliated,

    #[error("Mode de sécurité invalide pour cette opération")]
    InvalidSecurityMode,

    #[error("État de chaîne invalide pour cette opération")]
    InvalidState,

    #[error("Fraud proof invalide: {0}")]
    FraudProofInvalid(String),

    #[error("Fraud proof expiré (> 30 jours)")]
    FraudProofExpired,

    /// SECURITY FIX #7: Unauthorized action error
    #[error("Non autorisé: {action} nécessite {required}")]
    Unauthorized {
        action: String,
        required: String,
    },

    /// SECURITY FIX #33: Already withdrawn error
    #[error("Fonds déjà retirés pour ce compte")]
    AlreadyWithdrawn,

    /// SECURITY FIX #33: Merkle proof required for non-owner withdrawal
    #[error("Preuve Merkle requise pour retrait non-propriétaire")]
    RequiresMerkleProof,

    /// SECURITY FIX #33: No snapshot state root available
    #[error("Aucun state root de snapshot disponible")]
    NoSnapshotStateRoot,

    /// SECURITY FIX #33: Invalid Merkle proof
    #[error("Preuve Merkle invalide")]
    InvalidMerkleProof,

    #[error("Erreur de chaîne: {0}")]
    ChainTypeError(#[from] crate::types::ChainError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_sidechain_sovereign() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("TestChain".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        assert_eq!(chain_id, ChainId(1));
        assert_eq!(registry.sidechain_count(), 1);

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.security_mode, SecurityMode::Sovereign);
        assert_eq!(sidechain.deposit, crate::types::SOVEREIGN_DEPOSIT);
    }

    #[test]
    fn test_insufficient_deposit() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Try Sovereign mode with insufficient deposit
        let result = registry.create_sidechain(
            owner,
            Some("TestChain".to_string()),
            None,
            None,
            SecurityMode::Sovereign,
            None,
            crate::types::SOVEREIGN_DEPOSIT - 1,
            0,
        );

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChainError::InsufficientDeposit));
    }

    #[test]
    fn test_auto_purge() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("TestChain".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Immédiatement après création, pas de purge
        let purged = registry.auto_purge_inactive(PURGE_CHECK_INTERVAL);
        assert_eq!(purged.len(), 0);

        // Après période d'inactivité
        let purged = registry.auto_purge_inactive(INACTIVITY_THRESHOLD + PURGE_CHECK_INTERVAL + 1);
        assert_eq!(purged.len(), 1);
        assert_eq!(purged[0], chain_id);

        // Vérifie que la chaîne est purgée
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::Purged);
    }

    #[test]
    fn test_hostchain_affiliation() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Create a hostchain first
        let host_id = registry.create_hostchain(owner, 0);

        // Create a sidechain with Shared mode (should auto-affiliate)
        let sidechain_id = registry
            .create_sidechain(
                owner,
                Some("SC1".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT, // 0 members initially
                0,
            )
            .unwrap();

        // Verify auto-affiliation happened
        let host = registry.get_hostchain(&host_id).unwrap();
        assert!(host.member_chains.contains(&sidechain_id));

        // Leave host
        registry.leave_host(sidechain_id).unwrap();

        let host = registry.get_hostchain(&host_id).unwrap();
        assert!(!host.member_chains.contains(&sidechain_id));
    }

    #[test]
    fn test_record_activity() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("TestChain".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Enregistre activité
        registry.record_activity(chain_id, 1000).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.last_activity, 1000);
    }

    // ==================== SPEC v3 Security Mode Tests ====================

    #[test]
    fn test_security_mode_inherited_deposit() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Create parent chain first
        let parent_id = registry
            .create_sidechain(
                owner,
                Some("ParentChain".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Inherited mode: 1000 KRAT
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("InheritedChain".to_string()),
                None,
                Some(parent_id),
                SecurityMode::Inherited,
                None,
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.security_mode, SecurityMode::Inherited);
        assert_eq!(sidechain.deposit, crate::types::BASE_DEPOSIT);
    }

    #[test]
    fn test_security_mode_sovereign_deposit() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Sovereign mode: 10,000 KRAT
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("SovereignChain".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.security_mode, SecurityMode::Sovereign);
        assert_eq!(sidechain.deposit, crate::types::SOVEREIGN_DEPOSIT);
    }

    #[test]
    fn test_security_mode_shared_deposit_scaling() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Create hostchain
        let host_id = registry.create_hostchain(owner, 0);

        // Add first member (Shared mode: 1000 × 0 = 1000 KRAT initially)
        let chain1 = registry
            .create_sidechain(
                owner,
                Some("SharedChain1".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        let sidechain1 = registry.get_sidechain(&chain1).unwrap();
        assert_eq!(sidechain1.security_mode, SecurityMode::Shared);

        // Add second member - deposit should scale with members
        // Now host has 1 member, so deposit = 1000 × 1 = 1000 KRAT
        let chain2 = registry
            .create_sidechain(
                owner,
                Some("SharedChain2".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT, // 1 member now
                0,
            )
            .unwrap();

        let host = registry.get_hostchain(&host_id).unwrap();
        assert_eq!(host.member_chains.len(), 2);
    }

    #[test]
    fn test_security_mode_parent_validation() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Create parent sidechain
        let parent_id = registry
            .create_sidechain(
                owner,
                Some("Parent".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Create child with Inherited security from parent
        let child_id = registry
            .create_sidechain(
                owner,
                Some("Child".to_string()),
                None,
                Some(parent_id),
                SecurityMode::Inherited,
                None,
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        let child = registry.get_sidechain(&child_id).unwrap();
        assert_eq!(child.parent, Some(parent_id));
        assert_eq!(child.security_mode, SecurityMode::Inherited);
    }

    #[test]
    fn test_security_mode_invalid_parent() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Try to create child with non-existent parent
        let result = registry.create_sidechain(
            owner,
            Some("OrphanChild".to_string()),
            None,
            Some(ChainId(999)), // Non-existent parent
            SecurityMode::Inherited,
            None,
            crate::types::BASE_DEPOSIT,
            0,
        );

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChainError::ParentNotFound));
    }

    #[test]
    fn test_security_mode_shared_without_host() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Try Shared mode without hostchain - should fail
        let result = registry.create_sidechain(
            owner,
            Some("NoHostChain".to_string()),
            None,
            None,
            SecurityMode::Shared,
            None, // No host specified
            crate::types::BASE_DEPOSIT,
            0,
        );

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChainError::HostNotFound));
    }

    #[test]
    fn test_deposit_calculation_function() {
        use crate::types::calculate_deposit;

        // Inherited: 1000 KRAT
        assert_eq!(calculate_deposit(SecurityMode::Inherited, 0), 1_000);
        assert_eq!(calculate_deposit(SecurityMode::Inherited, 10), 1_000);

        // Shared: 1000 × N_members
        assert_eq!(calculate_deposit(SecurityMode::Shared, 0), 0);
        assert_eq!(calculate_deposit(SecurityMode::Shared, 1), 1_000);
        assert_eq!(calculate_deposit(SecurityMode::Shared, 5), 5_000);
        assert_eq!(calculate_deposit(SecurityMode::Shared, 10), 10_000);

        // Sovereign: 10,000 KRAT
        assert_eq!(calculate_deposit(SecurityMode::Sovereign, 0), 10_000);
        assert_eq!(calculate_deposit(SecurityMode::Sovereign, 10), 10_000);
    }

    #[test]
    fn test_security_mode_deposit_enforcement() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Inherited with too little deposit should fail
        let result = registry.create_sidechain(
            owner,
            Some("Underfunded".to_string()),
            None,
            None,
            SecurityMode::Inherited,
            None,
            crate::types::BASE_DEPOSIT - 1,
            0,
        );
        assert!(result.is_err());

        // Sovereign with BASE_DEPOSIT should fail
        let result = registry.create_sidechain(
            owner,
            Some("Underfunded".to_string()),
            None,
            None,
            SecurityMode::Sovereign,
            None,
            crate::types::BASE_DEPOSIT,
            0,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_security_mode_shared_auto_affiliation() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let host_id = registry.create_hostchain(owner, 0);

        // Create with Shared mode - should auto-affiliate
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("AutoAffiliate".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Verify auto-affiliation
        let host = registry.get_hostchain(&host_id).unwrap();
        assert!(host.member_chains.contains(&chain_id));

        // Verify sidechain is in registry's tracking
        assert_eq!(
            registry.sidechain_to_host.get(&chain_id),
            Some(&host_id)
        );
    }

    #[test]
    fn test_security_mode_comparison() {
        // Test cost hierarchy: Inherited < Shared (varies) < Sovereign
        assert!(crate::types::BASE_DEPOSIT < crate::types::SOVEREIGN_DEPOSIT);

        // Shared can be cheaper or more expensive than Sovereign depending on members
        use crate::types::calculate_deposit;
        assert!(calculate_deposit(SecurityMode::Shared, 5) < crate::types::SOVEREIGN_DEPOSIT);
        assert!(calculate_deposit(SecurityMode::Shared, 15) > crate::types::SOVEREIGN_DEPOSIT);
    }

    // ===== PHASE 2: VALIDATOR ASSIGNMENT TESTS =====

    #[test]
    fn test_inherited_mode_inherits_parent_validators() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);

        // Create parent chain (Sovereign mode) with validators
        let parent_id = registry
            .create_sidechain(
                owner,
                Some("Parent".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Add validators to parent
        registry.add_validator_to_chain(parent_id, val1).unwrap();
        registry.add_validator_to_chain(parent_id, val2).unwrap();

        // Create child with Inherited security
        let child_id = registry
            .create_sidechain(
                owner,
                Some("Child".to_string()),
                None,
                Some(parent_id),
                SecurityMode::Inherited,
                None,
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Child should automatically inherit parent's validators
        let child = registry.get_sidechain(&child_id).unwrap();
        assert_eq!(child.validators.len(), 2);
        assert!(child.validators.contains(&val1));
        assert!(child.validators.contains(&val2));
    }

    #[test]
    fn test_inherited_mode_parent_validator_propagation() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);

        // Create parent and child
        let parent_id = registry
            .create_sidechain(
                owner,
                Some("Parent".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        registry.add_validator_to_chain(parent_id, val1).unwrap();

        let child_id = registry
            .create_sidechain(
                owner,
                Some("Child".to_string()),
                None,
                Some(parent_id),
                SecurityMode::Inherited,
                None,
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Initially child has val1
        let child = registry.get_sidechain(&child_id).unwrap();
        assert_eq!(child.validators.len(), 1);

        // Add validator to parent
        registry.add_validator_to_chain(parent_id, val2).unwrap();

        // Re-assign child validators (simulates update)
        registry.assign_validators(child_id).unwrap();

        // Child should now have both validators
        let child = registry.get_sidechain(&child_id).unwrap();
        assert_eq!(child.validators.len(), 2);
        assert!(child.validators.contains(&val1));
        assert!(child.validators.contains(&val2));
    }

    #[test]
    fn test_shared_mode_gets_hostchain_pool() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);

        // Create hostchain with validator pool
        let host_id = registry.create_hostchain(owner, 0);
        registry.add_validator_to_hostchain(host_id, val1).unwrap();
        registry.add_validator_to_hostchain(host_id, val2).unwrap();

        // Create sidechain with Shared security
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("SharedChain".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Sidechain should automatically get hostchain pool validators
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.validators.len(), 2);
        assert!(sidechain.validators.contains(&val1));
        assert!(sidechain.validators.contains(&val2));
    }

    #[test]
    fn test_shared_mode_hostchain_pool_propagation() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);
        let val3 = AccountId::from_bytes([12; 32]);

        // Create hostchain with validator pool
        let host_id = registry.create_hostchain(owner, 0);
        registry.add_validator_to_hostchain(host_id, val1).unwrap();

        // Create multiple affiliated sidechains
        let chain1 = registry
            .create_sidechain(
                owner,
                Some("Shared1".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        let chain2 = registry
            .create_sidechain(
                owner,
                Some("Shared2".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Both chains have val1
        assert_eq!(registry.get_sidechain(&chain1).unwrap().validators.len(), 1);
        assert_eq!(registry.get_sidechain(&chain2).unwrap().validators.len(), 1);

        // Add validators to hostchain pool - should propagate to all affiliated chains
        registry.add_validator_to_hostchain(host_id, val2).unwrap();
        registry.add_validator_to_hostchain(host_id, val3).unwrap();

        // Both chains should now have 3 validators
        let chain1_info = registry.get_sidechain(&chain1).unwrap();
        assert_eq!(chain1_info.validators.len(), 3);
        assert!(chain1_info.validators.contains(&val1));
        assert!(chain1_info.validators.contains(&val2));
        assert!(chain1_info.validators.contains(&val3));

        let chain2_info = registry.get_sidechain(&chain2).unwrap();
        assert_eq!(chain2_info.validators.len(), 3);
        assert!(chain2_info.validators.contains(&val1));
        assert!(chain2_info.validators.contains(&val2));
        assert!(chain2_info.validators.contains(&val3));
    }

    #[test]
    fn test_sovereign_mode_manual_validator_management() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);

        // Create Sovereign chain
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Sovereign".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Initially no validators
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.validators.len(), 0);

        // Manually add validators
        registry.add_validator_to_chain(chain_id, val1).unwrap();
        registry.add_validator_to_chain(chain_id, val2).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.validators.len(), 2);

        // Manually remove validator
        registry.remove_validator_from_chain(chain_id, &val1).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.validators.len(), 1);
        assert!(sidechain.validators.contains(&val2));
    }

    #[test]
    fn test_cannot_manually_manage_inherited_validators() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);

        // Create parent and child with Inherited security
        let parent_id = registry
            .create_sidechain(
                owner,
                Some("Parent".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        let child_id = registry
            .create_sidechain(
                owner,
                Some("Child".to_string()),
                None,
                Some(parent_id),
                SecurityMode::Inherited,
                None,
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Cannot manually add validator to Inherited chain
        let result = registry.add_validator_to_chain(child_id, val1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChainError::InvalidSecurityMode));
    }

    #[test]
    fn test_cannot_manually_manage_shared_validators() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);

        // Create hostchain and Shared sidechain
        let host_id = registry.create_hostchain(owner, 0);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Shared".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Cannot manually add validator to Shared chain
        let result = registry.add_validator_to_chain(chain_id, val1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChainError::InvalidSecurityMode));
    }

    #[test]
    fn test_validator_assignment_on_creation() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);

        // Create parent with validators
        let parent_id = registry
            .create_sidechain(
                owner,
                Some("Parent".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();
        registry.add_validator_to_chain(parent_id, val1).unwrap();

        // Child should get validators immediately on creation
        let child_id = registry
            .create_sidechain(
                owner,
                Some("Child".to_string()),
                None,
                Some(parent_id),
                SecurityMode::Inherited,
                None,
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        let child = registry.get_sidechain(&child_id).unwrap();
        assert_eq!(child.validators.len(), 1);
        assert!(child.validators.contains(&val1));
    }

    #[test]
    fn test_sovereign_chain_starts_with_empty_validators() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Create Sovereign chain
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Sovereign".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Sovereign chains start with empty validator set
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.validators.len(), 0);
    }

    #[test]
    fn test_hostchain_pool_with_multiple_affiliated_chains() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);

        // Create hostchain with validators
        let host_id = registry.create_hostchain(owner, 0);
        registry.add_validator_to_hostchain(host_id, val1).unwrap();
        registry.add_validator_to_hostchain(host_id, val2).unwrap();

        // Create 3 affiliated chains
        let chain1 = registry
            .create_sidechain(
                owner,
                Some("Shared1".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        let chain2 = registry
            .create_sidechain(
                owner,
                Some("Shared2".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        let chain3 = registry
            .create_sidechain(
                owner,
                Some("Shared3".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT * 2, // 2 members now
                0,
            )
            .unwrap();

        // All chains should have same validators from pool
        for chain_id in [chain1, chain2, chain3] {
            let chain = registry.get_sidechain(&chain_id).unwrap();
            assert_eq!(chain.validators.len(), 2);
            assert!(chain.validators.contains(&val1));
            assert!(chain.validators.contains(&val2));
        }
    }

    #[test]
    fn test_inherited_mode_with_empty_parent_validators() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Create parent with no validators
        let parent_id = registry
            .create_sidechain(
                owner,
                Some("Parent".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Create child - should inherit empty set
        let child_id = registry
            .create_sidechain(
                owner,
                Some("Child".to_string()),
                None,
                Some(parent_id),
                SecurityMode::Inherited,
                None,
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        let child = registry.get_sidechain(&child_id).unwrap();
        assert_eq!(child.validators.len(), 0);
    }

    #[test]
    fn test_hostchain_validator_removal_propagation() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);

        // Create hostchain with validators
        let host_id = registry.create_hostchain(owner, 0);
        registry.add_validator_to_hostchain(host_id, val1).unwrap();
        registry.add_validator_to_hostchain(host_id, val2).unwrap();

        // Create affiliated chain
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Shared".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Chain has 2 validators
        assert_eq!(registry.get_sidechain(&chain_id).unwrap().validators.len(), 2);

        // Remove validator from hostchain pool - should propagate
        registry.remove_validator_from_hostchain(host_id, &val1).unwrap();

        // Chain should now have only 1 validator
        let chain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(chain.validators.len(), 1);
        assert!(chain.validators.contains(&val2));
        assert!(!chain.validators.contains(&val1));
    }

    // ===== PHASE 3: ENHANCED PURGE SYSTEM TESTS (SPEC v3.1) =====

    #[test]
    fn test_purge_trigger_inactivity() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Initially no trigger
        assert!(registry.check_purge_triggers(chain_id, 1000).is_none());

        // After 90 days of inactivity, trigger should fire
        let trigger = registry.check_purge_triggers(chain_id, crate::types::INACTIVITY_THRESHOLD_V3_1 + 1);
        assert!(matches!(trigger, Some(crate::types::PurgeTrigger::Inactivity)));
    }

    #[test]
    fn test_purge_trigger_governance_failure() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Record 3 governance failures
        registry.record_governance_failure(chain_id).unwrap();
        registry.record_governance_failure(chain_id).unwrap();

        // Not yet triggered
        assert!(registry.check_purge_triggers(chain_id, 1000).is_none());

        registry.record_governance_failure(chain_id).unwrap();

        // Now triggered
        let trigger = registry.check_purge_triggers(chain_id, 1000);
        assert!(matches!(trigger, Some(crate::types::PurgeTrigger::GovernanceFailure)));
    }

    #[test]
    fn test_purge_trigger_validator_fraud() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);
        let val3 = AccountId::from_bytes([12; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Add 3 validators
        registry.add_validator_to_chain(chain_id, val1).unwrap();
        registry.add_validator_to_chain(chain_id, val2).unwrap();
        registry.add_validator_to_chain(chain_id, val3).unwrap();

        // Slash 1 validator (33% of 3 = 1 validator)
        registry.slash_validator(chain_id, &val1).unwrap();

        // Should trigger fraud purge
        let trigger = registry.check_purge_triggers(chain_id, 1000);
        assert!(matches!(trigger, Some(crate::types::PurgeTrigger::ValidatorFraud)));
    }

    #[test]
    fn test_purge_state_machine_full_cycle() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, crate::types::ChainStatus::Active);

        // Trigger purge
        registry
            .trigger_purge(chain_id, crate::types::PurgeTrigger::Inactivity, 0)
            .unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, crate::types::ChainStatus::PendingPurge);
        assert_eq!(sidechain.purge_triggered_at, Some(0));

        // Wait 30 days (warning period)
        let status = registry
            .advance_purge_state(chain_id, crate::types::PURGE_WARNING_PERIOD + 1)
            .unwrap();
        assert_eq!(status, crate::types::ChainStatus::Frozen);

        // Frozen → Snapshot (immediate)
        let status = registry.advance_purge_state(chain_id, crate::types::PURGE_WARNING_PERIOD + 2).unwrap();
        assert_eq!(status, crate::types::ChainStatus::Snapshot);

        // Snapshot → WithdrawalWindow (immediate)
        let status = registry.advance_purge_state(chain_id, crate::types::PURGE_WARNING_PERIOD + 3).unwrap();
        assert_eq!(status, crate::types::ChainStatus::WithdrawalWindow);

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert!(sidechain.withdrawal_window_start.is_some());

        // Wait 30 days (withdrawal window)
        let window_start = sidechain.withdrawal_window_start.unwrap();
        let status = registry
            .advance_purge_state(chain_id, window_start + crate::types::WITHDRAWAL_WINDOW_DURATION + 1)
            .unwrap();
        assert_eq!(status, crate::types::ChainStatus::Purged);
    }

    #[test]
    fn test_purge_warning_period_30_days() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        registry
            .trigger_purge(chain_id, crate::types::PurgeTrigger::Inactivity, 1000)
            .unwrap();

        // 29 days later - still pending
        registry
            .advance_purge_state(chain_id, 1000 + crate::types::PURGE_WARNING_PERIOD - 1)
            .unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, crate::types::ChainStatus::PendingPurge);

        // 30 days later - frozen
        registry
            .advance_purge_state(chain_id, 1000 + crate::types::PURGE_WARNING_PERIOD)
            .unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, crate::types::ChainStatus::Frozen);
    }

    #[test]
    fn test_withdrawal_window_30_days() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Fast-forward to WithdrawalWindow
        registry
            .trigger_purge(chain_id, crate::types::PurgeTrigger::Inactivity, 0)
            .unwrap();
        registry.advance_purge_state(chain_id, crate::types::PURGE_WARNING_PERIOD + 1).unwrap();
        registry.advance_purge_state(chain_id, crate::types::PURGE_WARNING_PERIOD + 2).unwrap();
        registry.advance_purge_state(chain_id, crate::types::PURGE_WARNING_PERIOD + 3).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, crate::types::ChainStatus::WithdrawalWindow);
        let window_start = sidechain.withdrawal_window_start.unwrap();

        // 29 days later - still in withdrawal
        registry
            .advance_purge_state(chain_id, window_start + crate::types::WITHDRAWAL_WINDOW_DURATION - 1)
            .unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, crate::types::ChainStatus::WithdrawalWindow);

        // 30 days later - purged
        registry
            .advance_purge_state(chain_id, window_start + crate::types::WITHDRAWAL_WINDOW_DURATION)
            .unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, crate::types::ChainStatus::Purged);
    }

    #[test]
    fn test_auto_purge_v3_1_detects_triggers() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Initially no triggers
        let triggered = registry.auto_purge_v3_1(PURGE_CHECK_INTERVAL);
        assert_eq!(triggered.len(), 0);

        // After 90 days, inactivity trigger fires
        let triggered = registry.auto_purge_v3_1(crate::types::INACTIVITY_THRESHOLD_V3_1 + PURGE_CHECK_INTERVAL + 1);
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].0, chain_id);
        assert!(matches!(triggered[0].1, crate::types::PurgeTrigger::Inactivity));

        // Chain should now be in PendingPurge
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, crate::types::ChainStatus::PendingPurge);
    }

    #[test]
    fn test_auto_purge_v3_1_advances_states() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Trigger manually
        registry
            .trigger_purge(chain_id, crate::types::PurgeTrigger::Inactivity, PURGE_CHECK_INTERVAL)
            .unwrap();

        // Auto-purge advances state after warning period
        registry.auto_purge_v3_1(PURGE_CHECK_INTERVAL + crate::types::PURGE_WARNING_PERIOD + 1);

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        // Should advance through Frozen → Snapshot → WithdrawalWindow
        assert!(matches!(
            sidechain.status,
            crate::types::ChainStatus::WithdrawalWindow
        ));
    }

    #[test]
    fn test_governance_failure_reset() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Record 2 failures
        registry.record_governance_failure(chain_id).unwrap();
        registry.record_governance_failure(chain_id).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.governance_failures, 2);

        // Successful vote resets counter
        registry.reset_governance_failures(chain_id).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.governance_failures, 0);
    }

    #[test]
    fn test_withdrawal_only_during_window() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Cannot withdraw from active chain
        let result = registry.withdraw_from_purged_chain(chain_id, owner);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChainError::InvalidState));

        // Fast-forward to WithdrawalWindow
        registry
            .trigger_purge(chain_id, crate::types::PurgeTrigger::Inactivity, 0)
            .unwrap();
        registry.advance_purge_state(chain_id, crate::types::PURGE_WARNING_PERIOD + 1).unwrap();
        registry.advance_purge_state(chain_id, crate::types::PURGE_WARNING_PERIOD + 2).unwrap();
        registry.advance_purge_state(chain_id, crate::types::PURGE_WARNING_PERIOD + 3).unwrap();

        // Now can withdraw
        let result = registry.withdraw_from_purged_chain(chain_id, owner);
        assert!(result.is_ok());
        let withdrawal = result.unwrap();
        assert_eq!(withdrawal.amount, crate::types::SOVEREIGN_DEPOSIT);
        assert_eq!(withdrawal.recipient, owner);
        assert!(matches!(withdrawal.withdrawal_type, WithdrawalType::OwnerDeposit));
    }

    #[test]
    fn test_purge_removes_from_hostchain() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let host_id = registry.create_hostchain(owner, 0);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Verify chain is in hostchain
        let host = registry.get_hostchain(&host_id).unwrap();
        assert!(host.member_chains.contains(&chain_id));

        // Complete purge cycle
        registry
            .trigger_purge(chain_id, crate::types::PurgeTrigger::Inactivity, 0)
            .unwrap();

        // PendingPurge → Frozen (after 30 days warning)
        let after_warning = 0 + crate::types::PURGE_WARNING_PERIOD;
        registry.advance_purge_state(chain_id, after_warning).unwrap();

        // Frozen → Snapshot (immediate)
        registry.advance_purge_state(chain_id, after_warning).unwrap();

        // Snapshot → WithdrawalWindow (immediate)
        registry.advance_purge_state(chain_id, after_warning).unwrap();

        // WithdrawalWindow → Purged (after 30 days withdrawal)
        let after_withdrawal = after_warning + crate::types::WITHDRAWAL_WINDOW_DURATION;
        registry.advance_purge_state(chain_id, after_withdrawal).unwrap();

        // Chain should be removed from hostchain
        let host = registry.get_hostchain(&host_id).unwrap();
        assert!(!host.member_chains.contains(&chain_id));
    }

    #[test]
    fn test_cannot_trigger_purge_on_already_purging_chain() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("Test".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Trigger once
        registry
            .trigger_purge(chain_id, crate::types::PurgeTrigger::Inactivity, 0)
            .unwrap();

        // Cannot trigger again
        let result = registry.trigger_purge(chain_id, crate::types::PurgeTrigger::GovernanceFailure, 100);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ChainError::InvalidState));
    }
}
