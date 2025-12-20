// VC Decay - Inactivity control mechanism
// Principle: Reputation decays without activity, prevents zombie validators

use crate::consensus::validator_credits::ValidatorCreditsRecord;
use crate::types::{AccountId, BlockNumber, EpochNumber};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Decay configuration
pub struct DecayConfig {
    /// Decay rate per inactive quarter (0.0 - 1.0)
    /// Default: 0.10 (10% decay per quarter)
    pub decay_rate: f64,

    /// Epochs per quarter (default: 13 epochs ≈ 3 months)
    pub epochs_per_quarter: u64,

    /// Minimum VC threshold before decay stops
    /// (Prevents eternal decay to zero)
    pub min_vc_threshold: u64,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            decay_rate: 0.10,        // 10% per quarter
            epochs_per_quarter: 13,  // ~3 months
            min_vc_threshold: 1,     // Stop at 1 VC
        }
    }
}

/// Activity tracking for decay calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityTracker {
    /// Validator ID
    pub validator_id: AccountId,

    /// Last epoch with any activity
    pub last_activity_epoch: EpochNumber,

    /// Activity flags for current quarter
    pub has_governance_vote: bool,
    pub has_uptime_credit: bool,
    pub has_arbitration: bool,

    /// Last quarter when decay was applied
    pub last_decay_quarter: u64,

    /// Quarter start epoch (for tracking boundaries)
    pub quarter_start_epoch: EpochNumber,
}

impl ActivityTracker {
    /// Create new activity tracker
    pub fn new(validator_id: AccountId, current_epoch: EpochNumber) -> Self {
        Self {
            validator_id,
            last_activity_epoch: current_epoch,
            has_governance_vote: false,
            has_uptime_credit: false,
            has_arbitration: false,
            last_decay_quarter: 0,
            quarter_start_epoch: current_epoch,
        }
    }

    /// Check if validator is inactive for current quarter
    pub fn is_inactive(&self) -> bool {
        !self.has_governance_vote && !self.has_uptime_credit && !self.has_arbitration
    }

    /// Record governance vote activity
    pub fn record_governance_vote(&mut self, epoch: EpochNumber) {
        self.has_governance_vote = true;
        self.last_activity_epoch = epoch;
    }

    /// Record uptime credit activity
    pub fn record_uptime_credit(&mut self, epoch: EpochNumber) {
        self.has_uptime_credit = true;
        self.last_activity_epoch = epoch;
    }

    /// Record arbitration activity
    pub fn record_arbitration(&mut self, epoch: EpochNumber) {
        self.has_arbitration = true;
        self.last_activity_epoch = epoch;
    }

    /// Reset activity for new quarter
    pub fn reset_quarter(&mut self, new_quarter_start: EpochNumber) {
        self.has_governance_vote = false;
        self.has_uptime_credit = false;
        self.has_arbitration = false;
        self.quarter_start_epoch = new_quarter_start;
    }
}

/// VC Decay manager
pub struct VCDecayManager {
    /// Decay configuration
    config: DecayConfig,

    /// Activity tracking per validator
    activity_trackers: HashMap<AccountId, ActivityTracker>,
}

impl VCDecayManager {
    /// Create new decay manager with default config
    pub fn new() -> Self {
        Self {
            config: DecayConfig::default(),
            activity_trackers: HashMap::new(),
        }
    }

    /// Create with custom config
    pub fn with_config(config: DecayConfig) -> Self {
        Self {
            config,
            activity_trackers: HashMap::new(),
        }
    }

    /// Initialize activity tracker for validator
    pub fn initialize_validator(&mut self, validator_id: AccountId, current_epoch: EpochNumber) {
        self.activity_trackers.insert(
            validator_id,
            ActivityTracker::new(validator_id, current_epoch),
        );
    }

    /// Record governance vote activity
    pub fn record_governance_activity(
        &mut self,
        validator_id: &AccountId,
        epoch: EpochNumber,
    ) -> Result<(), DecayError> {
        let tracker = self
            .activity_trackers
            .get_mut(validator_id)
            .ok_or(DecayError::ValidatorNotFound)?;

        tracker.record_governance_vote(epoch);
        Ok(())
    }

    /// Record uptime activity
    pub fn record_uptime_activity(
        &mut self,
        validator_id: &AccountId,
        epoch: EpochNumber,
    ) -> Result<(), DecayError> {
        let tracker = self
            .activity_trackers
            .get_mut(validator_id)
            .ok_or(DecayError::ValidatorNotFound)?;

        tracker.record_uptime_credit(epoch);
        Ok(())
    }

    /// Record arbitration activity
    pub fn record_arbitration_activity(
        &mut self,
        validator_id: &AccountId,
        epoch: EpochNumber,
    ) -> Result<(), DecayError> {
        let tracker = self
            .activity_trackers
            .get_mut(validator_id)
            .ok_or(DecayError::ValidatorNotFound)?;

        tracker.record_arbitration(epoch);
        Ok(())
    }

    /// Apply decay at quarter boundary if inactive
    /// Returns true if decay was applied
    pub fn apply_decay_if_needed(
        &mut self,
        validator_id: &AccountId,
        current_epoch: EpochNumber,
        current_block: BlockNumber,
        vc_record: &mut ValidatorCreditsRecord,
    ) -> Result<bool, DecayError> {
        // Calculate current quarter
        let current_quarter = current_epoch / self.config.epochs_per_quarter;

        // Get tracker info (immutably first)
        let (should_decay, _last_quarter, _is_inactive) = {
            let tracker = self
                .activity_trackers
                .get(validator_id)
                .ok_or(DecayError::ValidatorNotFound)?;

            // Check if we've entered a new quarter
            if current_quarter <= tracker.last_decay_quarter {
                return Ok(false); // Not yet time
            }

            (
                tracker.is_inactive(),
                tracker.last_decay_quarter,
                tracker.is_inactive(),
            )
        };

        // Now mutate
        if should_decay {
            // Apply decay
            let total_vc = vc_record.total_vc();

            // Don't decay below minimum threshold
            if total_vc <= self.config.min_vc_threshold {
                let tracker = self.activity_trackers.get_mut(validator_id).unwrap();
                tracker.last_decay_quarter = current_quarter;
                tracker.reset_quarter(current_epoch);
                return Ok(false);
            }

            // Calculate decay amount
            let decay_amount = (total_vc as f64 * self.config.decay_rate) as u64;
            let decay_amount = decay_amount.max(1); // At least 1 VC

            // Apply proportional decay across categories
            Self::apply_proportional_decay_static(
                &self.config,
                vc_record,
                decay_amount,
                current_block,
            );

            let tracker = self.activity_trackers.get_mut(validator_id).unwrap();
            tracker.last_decay_quarter = current_quarter;
            tracker.reset_quarter(current_epoch);

            Ok(true)
        } else {
            // Validator was active, reset for next quarter
            let tracker = self.activity_trackers.get_mut(validator_id).unwrap();
            tracker.last_decay_quarter = current_quarter;
            tracker.reset_quarter(current_epoch);
            Ok(false)
        }
    }

    /// Apply proportional decay (static version to avoid borrow issues)
    fn apply_proportional_decay_static(
        _config: &DecayConfig,
        vc_record: &mut ValidatorCreditsRecord,
        decay_amount: u64,
        current_block: BlockNumber,
    ) {
        let total_vc = vc_record.total_vc();
        if total_vc == 0 {
            return;
        }

        // Calculate proportional decay for each category
        let vote_decay = ((vc_record.vote_credits as u64 * decay_amount) / total_vc) as u32;
        let uptime_decay = ((vc_record.uptime_credits as u64 * decay_amount) / total_vc) as u32;
        let arbitration_decay =
            ((vc_record.arbitration_credits as u64 * decay_amount) / total_vc) as u32;
        let seniority_decay =
            ((vc_record.seniority_credits as u64 * decay_amount) / total_vc) as u32;

        // Apply decay (saturating subtraction)
        vc_record.vote_credits = vc_record.vote_credits.saturating_sub(vote_decay);
        vc_record.uptime_credits = vc_record.uptime_credits.saturating_sub(uptime_decay);
        vc_record.arbitration_credits = vc_record
            .arbitration_credits
            .saturating_sub(arbitration_decay);
        vc_record.seniority_credits = vc_record.seniority_credits.saturating_sub(seniority_decay);

        vc_record.last_update = current_block;
    }


    /// Get activity tracker for validator
    pub fn get_tracker(&self, validator_id: &AccountId) -> Option<&ActivityTracker> {
        self.activity_trackers.get(validator_id)
    }

    /// Check if validator is currently inactive
    pub fn is_validator_inactive(&self, validator_id: &AccountId) -> bool {
        self.activity_trackers
            .get(validator_id)
            .map(|t| t.is_inactive())
            .unwrap_or(true)
    }

    /// Get decay config
    pub fn config(&self) -> &DecayConfig {
        &self.config
    }
}

impl Default for VCDecayManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Decay errors
#[derive(Debug, thiserror::Error)]
pub enum DecayError {
    #[error("Validator not found")]
    ValidatorNotFound,

    #[error("Invalid decay configuration")]
    InvalidConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_tracking() {
        let validator = AccountId::from_bytes([1; 32]);
        let mut tracker = ActivityTracker::new(validator, 0);

        // Initially inactive
        assert!(tracker.is_inactive());

        // Record vote activity
        tracker.record_governance_vote(1);
        assert!(!tracker.is_inactive());

        // Reset quarter
        tracker.reset_quarter(13);
        assert!(tracker.is_inactive());

        // Record uptime
        tracker.record_uptime_credit(14);
        assert!(!tracker.is_inactive());
    }

    #[test]
    fn test_decay_application() {
        let mut manager = VCDecayManager::new();
        let validator = AccountId::from_bytes([1; 32]);

        manager.initialize_validator(validator, 0);

        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 100;
        vc_record.uptime_credits = 100;
        // Total: 200 VC

        // No decay in same quarter
        let decayed = manager
            .apply_decay_if_needed(&validator, 12, 1000, &mut vc_record)
            .unwrap();
        assert!(!decayed);
        assert_eq!(vc_record.total_vc(), 200);

        // Move to next quarter (epoch 13), should decay
        let decayed = manager
            .apply_decay_if_needed(&validator, 13, 2000, &mut vc_record)
            .unwrap();
        assert!(decayed);

        // 10% decay = 20 VC removed
        let expected_vc = 180;
        assert_eq!(vc_record.total_vc(), expected_vc);
    }

    #[test]
    fn test_no_decay_with_activity() {
        let mut manager = VCDecayManager::new();
        let validator = AccountId::from_bytes([1; 32]);

        manager.initialize_validator(validator, 0);

        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 100;

        // Record activity in quarter
        manager.record_governance_activity(&validator, 5).unwrap();

        // Move to next quarter - no decay because of activity
        let decayed = manager
            .apply_decay_if_needed(&validator, 13, 1000, &mut vc_record)
            .unwrap();
        assert!(!decayed);
        assert_eq!(vc_record.total_vc(), 100); // No decay
    }

    #[test]
    fn test_decay_stops_at_minimum() {
        let config = DecayConfig {
            decay_rate: 0.50, // 50% decay
            epochs_per_quarter: 13,
            min_vc_threshold: 5,
        };
        let mut manager = VCDecayManager::with_config(config);
        let validator = AccountId::from_bytes([1; 32]);

        manager.initialize_validator(validator, 0);

        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 10;

        // First decay: 10 -> 5
        let decayed = manager
            .apply_decay_if_needed(&validator, 13, 1000, &mut vc_record)
            .unwrap();
        assert!(decayed);
        assert_eq!(vc_record.total_vc(), 5);

        // Second decay attempt: should stop at minimum
        let decayed = manager
            .apply_decay_if_needed(&validator, 26, 2000, &mut vc_record)
            .unwrap();
        assert!(!decayed);
        assert_eq!(vc_record.total_vc(), 5); // Stopped at minimum
    }

    #[test]
    fn test_cumulative_decay() {
        let mut manager = VCDecayManager::new();
        let validator = AccountId::from_bytes([1; 32]);

        manager.initialize_validator(validator, 0);

        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 1000;

        let mut epoch = 0;
        let mut block = 0;

        // Apply decay over 4 quarters (1 year)
        for _quarter in 0..4 {
            epoch += 13;
            block += 1000;

            let _ = manager.apply_decay_if_needed(&validator, epoch, block, &mut vc_record);
        }

        // After 4 quarters of 10% decay each:
        // Q1: 1000 * 0.9 = 900
        // Q2: 900 * 0.9 = 810
        // Q3: 810 * 0.9 = 729
        // Q4: 729 * 0.9 = 656
        let expected = 656;
        assert!(
            vc_record.total_vc() >= expected - 5 && vc_record.total_vc() <= expected + 5,
            "Expected ~{}, got {}",
            expected,
            vc_record.total_vc()
        );
    }

    #[test]
    fn test_proportional_decay() {
        let mut manager = VCDecayManager::new();
        let validator = AccountId::from_bytes([1; 32]);

        manager.initialize_validator(validator, 0);

        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 60; // 60%
        vc_record.uptime_credits = 30; // 30%
        vc_record.seniority_credits = 10; // 10%
        // Total: 100 VC

        let decayed = manager
            .apply_decay_if_needed(&validator, 13, 1000, &mut vc_record)
            .unwrap();
        assert!(decayed);

        // After 10% decay:
        // vote: 60 * 0.9 = 54
        // uptime: 30 * 0.9 = 27
        // seniority: 10 * 0.9 = 9
        // Total: 90

        assert_eq!(vc_record.total_vc(), 90);
        assert!(vc_record.vote_credits >= 50 && vc_record.vote_credits <= 60);
        assert!(vc_record.uptime_credits >= 25 && vc_record.uptime_credits <= 30);
    }
}
