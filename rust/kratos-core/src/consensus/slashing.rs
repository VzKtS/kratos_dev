// Slashing - Economic and reputation penalties for misbehavior
// Principle: Reputation slashed before capital, proportional and explainable

use crate::consensus::validator_credits::{ValidatorCreditsRecord, VCError};
use crate::types::{AccountId, Balance, BlockNumber, EpochNumber};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Slashing severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashingSeverity {
    /// Critical: Double signing, proven equivocation
    Critical,
    /// High: Arbitration misconduct, invalid governance execution
    High,
    /// Medium: Extended downtime
    Medium,
    /// Low: Repeated low participation
    Low,
}

impl SlashingSeverity {
    /// Get VC slash percentage for this severity
    pub fn vc_slash_percent(&self) -> f64 {
        match self {
            SlashingSeverity::Critical => 0.50, // -50%
            SlashingSeverity::High => 0.25,     // -25%
            SlashingSeverity::Medium => 0.10,   // -10%
            SlashingSeverity::Low => 0.05,      // -5%
        }
    }

    /// Get economic stake slash percentage for this severity
    pub fn stake_slash_percent(&self) -> f64 {
        match self {
            SlashingSeverity::Critical => 0.20, // 5-20% (configurable)
            SlashingSeverity::High => 0.05,     // 1-5%
            SlashingSeverity::Medium => 0.01,   // 0-1%
            SlashingSeverity::Low => 0.0,       // 0%
        }
    }

    /// Whether this severity requires cooldown period
    pub fn requires_cooldown(&self) -> bool {
        matches!(self, SlashingSeverity::Critical | SlashingSeverity::High)
    }

    /// Cooldown period in epochs
    pub fn cooldown_epochs(&self) -> u64 {
        match self {
            SlashingSeverity::Critical => 52, // ~1 year
            SlashingSeverity::High => 12,     // ~3 months
            _ => 0,
        }
    }
}

/// Slashable event types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SlashableEvent {
    /// Double signing (producing two blocks for same slot)
    DoubleSigning {
        slot: u64,
        epoch: EpochNumber,
        block_hash_1: [u8; 32],
        block_hash_2: [u8; 32],
    },
    /// Proven equivocation (conflicting votes)
    Equivocation {
        epoch: EpochNumber,
        evidence: Vec<u8>,
    },
    /// Arbitration misconduct
    ArbitrationMisconduct {
        arbitration_id: u64,
        reason: String,
    },
    /// Invalid governance execution
    InvalidGovernanceExecution {
        proposal_id: u64,
        reason: String,
    },
    /// Extended downtime (multiple epochs offline)
    ExtendedDowntime {
        epochs_offline: u64,
    },
    /// Repeated low participation
    RepeatedLowParticipation {
        epochs: Vec<EpochNumber>,
        avg_participation: f64,
    },
}

impl SlashableEvent {
    /// Get severity of this event
    pub fn severity(&self) -> SlashingSeverity {
        match self {
            SlashableEvent::DoubleSigning { .. } => SlashingSeverity::Critical,
            SlashableEvent::Equivocation { .. } => SlashingSeverity::Critical,
            SlashableEvent::ArbitrationMisconduct { .. } => SlashingSeverity::High,
            SlashableEvent::InvalidGovernanceExecution { .. } => SlashingSeverity::High,
            SlashableEvent::ExtendedDowntime { epochs_offline } => {
                if *epochs_offline >= 12 {
                    SlashingSeverity::Medium
                } else {
                    SlashingSeverity::Low
                }
            }
            SlashableEvent::RepeatedLowParticipation {
                avg_participation, ..
            } => {
                if *avg_participation < 0.50 {
                    SlashingSeverity::Medium
                } else {
                    SlashingSeverity::Low
                }
            }
        }
    }
}

/// Slashing record for a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingRecord {
    /// Validator ID
    pub validator_id: AccountId,

    /// Event that caused the slash
    pub event: SlashableEvent,

    /// When it occurred
    pub slash_epoch: EpochNumber,
    pub slash_block: BlockNumber,

    /// VC slashed
    pub vc_slashed: u64,

    /// Stake slashed (in KRAT)
    pub stake_slashed: Balance,

    /// Cooldown until epoch (if applicable)
    pub cooldown_until_epoch: Option<EpochNumber>,

    /// Whether validator was ejected
    pub ejected: bool,
}

/// Validator cooldown state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownState {
    /// Why validator is in cooldown
    pub reason: SlashingSeverity,

    /// Cooldown started at epoch
    pub start_epoch: EpochNumber,

    /// Cooldown ends at epoch
    pub end_epoch: EpochNumber,

    /// Number of critical events (for ejection tracking)
    pub critical_event_count: u32,
}

/// Maximum number of slashing records to keep
/// After this limit, oldest records are pruned
const MAX_SLASHING_RECORDS: usize = 10_000;

/// Age in epochs after which slashing records can be pruned
/// (roughly 2 years at 1 epoch/week)
const SLASHING_RECORD_RETENTION_EPOCHS: u64 = 104;

/// Critical count decay period in epochs
/// After this many epochs without a critical event, the count decays by 1
/// (roughly 6 months at 1 epoch/week)
const CRITICAL_COUNT_DECAY_EPOCHS: u64 = 26;

/// Slashing manager
pub struct SlashingManager {
    /// Slashing history
    slashing_records: Vec<SlashingRecord>,

    /// Validators in cooldown
    cooldown_states: HashMap<AccountId, CooldownState>,

    /// Critical event counts per validator with last critical epoch
    critical_counts: HashMap<AccountId, CriticalCountState>,
}

/// State for tracking critical events with decay
#[derive(Debug, Clone)]
struct CriticalCountState {
    count: u32,
    last_critical_epoch: EpochNumber,
}

impl SlashingManager {
    /// Create new slashing manager
    pub fn new() -> Self {
        Self {
            slashing_records: Vec::new(),
            cooldown_states: HashMap::new(),
            critical_counts: HashMap::new(),
        }
    }

    /// Process a slashing event
    /// Returns (vc_slashed, stake_slashed, ejected)
    pub fn slash_validator(
        &mut self,
        validator_id: AccountId,
        event: SlashableEvent,
        current_epoch: EpochNumber,
        current_block: BlockNumber,
        vc_record: &mut ValidatorCreditsRecord,
        current_stake: Balance,
    ) -> Result<SlashingRecord, SlashingError> {
        let severity = event.severity();

        // STEP 1: Slash VC (MANDATORY FIRST)
        let vc_before = vc_record.total_vc();
        let vc_slash_amount = self.calculate_vc_slash(vc_record, severity);
        self.apply_vc_slash(vc_record, vc_slash_amount, current_block);
        let vc_after = vc_record.total_vc();

        // STEP 2: Slash stake (only if VC exhausted or critical)
        let stake_slash_amount = if vc_after == 0 || matches!(severity, SlashingSeverity::Critical)
        {
            self.calculate_stake_slash(current_stake, severity)
        } else {
            0
        };

        // STEP 3: Determine cooldown/ejection
        let (cooldown_until, ejected) = self.determine_cooldown_ejection(
            validator_id,
            severity,
            current_epoch,
        );

        // Record the slash
        let record = SlashingRecord {
            validator_id,
            event,
            slash_epoch: current_epoch,
            slash_block: current_block,
            vc_slashed: vc_before - vc_after,
            stake_slashed: stake_slash_amount,
            cooldown_until_epoch: cooldown_until,
            ejected,
        };

        self.slashing_records.push(record.clone());

        Ok(record)
    }

    /// Calculate how much VC to slash
    /// SECURITY FIX #8: Use checked arithmetic to prevent overflow
    /// SECURITY FIX #25: Safe f64→u64 conversion with bounds checking
    fn calculate_vc_slash(
        &self,
        vc_record: &ValidatorCreditsRecord,
        severity: SlashingSeverity,
    ) -> u64 {
        let total_vc = vc_record.total_vc();
        let slash_percent = severity.vc_slash_percent();

        // SECURITY FIX #25: Safe floating point conversion
        // 1. Validate percent is in valid range [0, 1]
        let slash_percent = slash_percent.clamp(0.0, 1.0);

        // 2. Calculate with bounds checking
        let slash_f64 = (total_vc as f64) * slash_percent;

        // 3. Handle NaN, Infinity, and negative values safely
        let slash_amount = if slash_f64.is_nan() || slash_f64.is_infinite() || slash_f64 < 0.0 {
            0u64
        } else if slash_f64 > u64::MAX as f64 {
            u64::MAX
        } else {
            slash_f64.round() as u64
        };

        // Ensure we don't slash more than total VC
        slash_amount.min(total_vc)
    }

    /// Apply VC slash proportionally across categories
    /// SPEC v2: Preserves category separation with proper rounding
    fn apply_vc_slash(
        &self,
        vc_record: &mut ValidatorCreditsRecord,
        slash_amount: u64,
        current_block: BlockNumber,
    ) {
        if slash_amount == 0 {
            return;
        }

        let total_vc = vc_record.total_vc();
        if total_vc == 0 {
            return;
        }

        // Calculate proportional slash for each category
        let vote_slash = ((vc_record.vote_credits as u64 * slash_amount) / total_vc) as u32;
        let uptime_slash = ((vc_record.uptime_credits as u64 * slash_amount) / total_vc) as u32;
        let arbitration_slash =
            ((vc_record.arbitration_credits as u64 * slash_amount) / total_vc) as u32;
        let seniority_slash =
            ((vc_record.seniority_credits as u64 * slash_amount) / total_vc) as u32;

        // Calculate sum of proportional slashes
        let proportional_sum = vote_slash as u64 + uptime_slash as u64
            + arbitration_slash as u64 + seniority_slash as u64;

        // Calculate rounding residual (amount lost to integer truncation)
        let residual = if slash_amount > proportional_sum {
            (slash_amount - proportional_sum) as u32
        } else {
            0
        };

        // Assign residual to the largest category to ensure exact slash amount
        let (vote_slash, uptime_slash, arbitration_slash, seniority_slash) = {
            let max_category = [
                (vc_record.vote_credits, 0),
                (vc_record.uptime_credits, 1),
                (vc_record.arbitration_credits, 2),
                (vc_record.seniority_credits, 3),
            ]
            .into_iter()
            .max_by_key(|(credits, _)| *credits)
            .map(|(_, idx)| idx)
            .unwrap_or(0);

            match max_category {
                0 => (vote_slash + residual, uptime_slash, arbitration_slash, seniority_slash),
                1 => (vote_slash, uptime_slash + residual, arbitration_slash, seniority_slash),
                2 => (vote_slash, uptime_slash, arbitration_slash + residual, seniority_slash),
                _ => (vote_slash, uptime_slash, arbitration_slash, seniority_slash + residual),
            }
        };

        // Apply slashes (saturating subtraction)
        vc_record.vote_credits = vc_record.vote_credits.saturating_sub(vote_slash);
        vc_record.uptime_credits = vc_record.uptime_credits.saturating_sub(uptime_slash);
        vc_record.arbitration_credits = vc_record
            .arbitration_credits
            .saturating_sub(arbitration_slash);
        vc_record.seniority_credits = vc_record.seniority_credits.saturating_sub(seniority_slash);

        vc_record.last_update = current_block;
    }

    /// Calculate stake slash amount
    /// SECURITY FIX #8: Use checked arithmetic to prevent overflow
    /// SECURITY FIX #25: Safe f64→Balance conversion with bounds checking
    fn calculate_stake_slash(&self, current_stake: Balance, severity: SlashingSeverity) -> Balance {
        let slash_percent = severity.stake_slash_percent();

        // SECURITY FIX #25: Safe floating point conversion
        // 1. Validate percent is in valid range [0, 1]
        let slash_percent = slash_percent.clamp(0.0, 1.0);

        // 2. Calculate with bounds checking
        let slash_f64 = (current_stake as f64) * slash_percent;

        // 3. Handle NaN, Infinity, and negative values safely
        let slash_amount: Balance = if slash_f64.is_nan() || slash_f64.is_infinite() || slash_f64 < 0.0 {
            0
        } else if slash_f64 > Balance::MAX as f64 {
            Balance::MAX
        } else {
            slash_f64.round() as Balance
        };

        // Ensure we don't slash more than current stake
        slash_amount.min(current_stake)
    }

    /// Determine if validator enters cooldown or gets ejected
    fn determine_cooldown_ejection(
        &mut self,
        validator_id: AccountId,
        severity: SlashingSeverity,
        current_epoch: EpochNumber,
    ) -> (Option<EpochNumber>, bool) {
        // Track critical events with decay
        if matches!(severity, SlashingSeverity::Critical) {
            let state = self.critical_counts
                .entry(validator_id)
                .or_insert(CriticalCountState {
                    count: 0,
                    last_critical_epoch: current_epoch,
                });

            // Apply decay before incrementing
            let epochs_since_last = current_epoch.saturating_sub(state.last_critical_epoch);
            // FIX: Guard against division by zero (CRITICAL_COUNT_DECAY_EPOCHS should never be 0)
            let decay_amount = if CRITICAL_COUNT_DECAY_EPOCHS > 0 {
                (epochs_since_last / CRITICAL_COUNT_DECAY_EPOCHS) as u32
            } else {
                0
            };
            state.count = state.count.saturating_sub(decay_amount);

            // Now increment
            state.count += 1;
            state.last_critical_epoch = current_epoch;

            // Eject after 3 critical events
            if state.count >= 3 {
                return (None, true); // Ejected
            }
        }

        // Apply cooldown if required
        if severity.requires_cooldown() {
            let cooldown_epochs = severity.cooldown_epochs();
            let end_epoch = current_epoch + cooldown_epochs;

            let critical_count = self.critical_counts
                .get(&validator_id)
                .map(|s| s.count)
                .unwrap_or(0);

            self.cooldown_states.insert(
                validator_id,
                CooldownState {
                    reason: severity,
                    start_epoch: current_epoch,
                    end_epoch,
                    critical_event_count: critical_count,
                },
            );

            (Some(end_epoch), false)
        } else {
            (None, false)
        }
    }

    /// Check if validator is in cooldown
    /// SECURITY FIX #12: Cooldown is active until end_epoch (inclusive)
    /// Note: Use is_in_cooldown_with_cleanup for automatic cleanup
    pub fn is_in_cooldown(&self, validator_id: &AccountId, current_epoch: EpochNumber) -> bool {
        if let Some(cooldown) = self.cooldown_states.get(validator_id) {
            // SECURITY FIX #12: Use <= to include end_epoch in cooldown period
            // This prevents validators from returning one epoch early
            current_epoch <= cooldown.end_epoch
        } else {
            false
        }
    }

    /// Check if validator is in cooldown with automatic cleanup of expired entries
    /// This is the preferred method for production use
    pub fn is_in_cooldown_with_cleanup(&mut self, validator_id: &AccountId, current_epoch: EpochNumber) -> bool {
        // Automatically cleanup expired cooldowns
        self.cleanup_expired_cooldowns(current_epoch);

        if let Some(cooldown) = self.cooldown_states.get(validator_id) {
            // SECURITY FIX #12: Use <= to include end_epoch in cooldown period
            current_epoch <= cooldown.end_epoch
        } else {
            false
        }
    }

    /// Get cooldown state for validator
    pub fn get_cooldown_state(&self, validator_id: &AccountId) -> Option<&CooldownState> {
        self.cooldown_states.get(validator_id)
    }

    /// Remove expired cooldowns
    /// SECURITY FIX #12: Consistent with is_in_cooldown - cooldown until end_epoch inclusive
    /// Called automatically by is_in_cooldown_with_cleanup
    /// Can also be called at epoch boundaries for explicit cleanup
    pub fn cleanup_expired_cooldowns(&mut self, current_epoch: EpochNumber) {
        // SECURITY FIX #12: Keep entries where current_epoch <= end_epoch (cooldown active)
        // Remove entries where current_epoch > end_epoch (cooldown expired)
        self.cooldown_states
            .retain(|_, state| current_epoch <= state.end_epoch);
    }

    /// Process epoch boundary - cleanup expired cooldowns, decay critical counts, prune old records
    /// Should be called at each epoch boundary for proper state maintenance
    pub fn on_epoch_boundary(&mut self, current_epoch: EpochNumber) {
        // Cleanup expired cooldowns
        self.cleanup_expired_cooldowns(current_epoch);

        // Decay critical counts and remove entries that have fully decayed
        self.decay_critical_counts(current_epoch);

        // Prune old slashing records to prevent unbounded memory growth
        self.prune_old_records(current_epoch);
    }

    /// Decay critical counts based on time elapsed since last critical event
    fn decay_critical_counts(&mut self, current_epoch: EpochNumber) {
        self.critical_counts.retain(|_, state| {
            let epochs_since_last = current_epoch.saturating_sub(state.last_critical_epoch);
            // FIX: Guard against division by zero (CRITICAL_COUNT_DECAY_EPOCHS should never be 0)
            let decay_amount = if CRITICAL_COUNT_DECAY_EPOCHS > 0 {
                (epochs_since_last / CRITICAL_COUNT_DECAY_EPOCHS) as u32
            } else {
                0
            };
            state.count = state.count.saturating_sub(decay_amount);

            // Keep entry only if count > 0
            state.count > 0
        });
    }

    /// Prune old slashing records to prevent unbounded memory growth
    /// Keeps only recent records within retention period, up to MAX_SLASHING_RECORDS
    fn prune_old_records(&mut self, current_epoch: EpochNumber) {
        // First, remove records older than retention period
        let min_epoch = current_epoch.saturating_sub(SLASHING_RECORD_RETENTION_EPOCHS);
        self.slashing_records.retain(|r| r.slash_epoch >= min_epoch);

        // If still over limit, keep only most recent MAX_SLASHING_RECORDS
        if self.slashing_records.len() > MAX_SLASHING_RECORDS {
            // Sort by epoch (most recent first) and truncate
            self.slashing_records.sort_by(|a, b| b.slash_epoch.cmp(&a.slash_epoch));
            self.slashing_records.truncate(MAX_SLASHING_RECORDS);
        }
    }

    /// Get slashing history for validator
    pub fn get_validator_slashes(&self, validator_id: &AccountId) -> Vec<&SlashingRecord> {
        self.slashing_records
            .iter()
            .filter(|r| r.validator_id == *validator_id)
            .collect()
    }

    /// Get all slashing records
    pub fn get_all_slashes(&self) -> &[SlashingRecord] {
        &self.slashing_records
    }
}

impl Default for SlashingManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Slashing errors
#[derive(Debug, thiserror::Error)]
pub enum SlashingError {
    #[error("Validator not found")]
    ValidatorNotFound,

    #[error("Invalid slash amount")]
    InvalidSlashAmount,

    #[error("VC error: {0}")]
    VCError(#[from] VCError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_levels() {
        assert_eq!(SlashingSeverity::Critical.vc_slash_percent(), 0.50);
        assert_eq!(SlashingSeverity::High.vc_slash_percent(), 0.25);
        assert_eq!(SlashingSeverity::Medium.vc_slash_percent(), 0.10);
        assert_eq!(SlashingSeverity::Low.vc_slash_percent(), 0.05);
    }

    #[test]
    fn test_event_severity() {
        let double_sign = SlashableEvent::DoubleSigning {
            slot: 1,
            epoch: 0,
            block_hash_1: [1; 32],
            block_hash_2: [2; 32],
        };
        assert_eq!(double_sign.severity(), SlashingSeverity::Critical);

        let downtime = SlashableEvent::ExtendedDowntime { epochs_offline: 15 };
        assert_eq!(downtime.severity(), SlashingSeverity::Medium);
    }

    #[test]
    fn test_vc_slash_proportional() {
        let mut manager = SlashingManager::new();
        let mut vc_record = ValidatorCreditsRecord::new(0, 0);

        // Give validator some VC
        vc_record.vote_credits = 50;
        vc_record.uptime_credits = 30;
        vc_record.arbitration_credits = 10;
        vc_record.seniority_credits = 10;
        // Total: 100 VC

        let validator_id = AccountId::from_bytes([1; 32]);
        let event = SlashableEvent::DoubleSigning {
            slot: 1,
            epoch: 0,
            block_hash_1: [1; 32],
            block_hash_2: [2; 32],
        };

        let record = manager
            .slash_validator(validator_id, event, 0, 0, &mut vc_record, 1_000_000)
            .unwrap();

        // Critical slash = 50% of 100 = 50 VC slashed
        assert_eq!(record.vc_slashed, 50);

        // Check proportional distribution
        let total_after = vc_record.total_vc();
        assert_eq!(total_after, 50); // 100 - 50

        // Each category should be roughly halved
        assert!(vc_record.vote_credits >= 20 && vc_record.vote_credits <= 30);
    }

    #[test]
    fn test_cooldown_mechanism() {
        let mut manager = SlashingManager::new();
        let validator_id = AccountId::from_bytes([1; 32]);

        // Not in cooldown initially
        assert!(!manager.is_in_cooldown(&validator_id, 0));

        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 100;

        let event = SlashableEvent::DoubleSigning {
            slot: 1,
            epoch: 0,
            block_hash_1: [1; 32],
            block_hash_2: [2; 32],
        };

        let record = manager
            .slash_validator(validator_id, event, 0, 0, &mut vc_record, 1_000_000)
            .unwrap();

        // Should enter cooldown
        assert!(record.cooldown_until_epoch.is_some());
        assert!(manager.is_in_cooldown(&validator_id, 0));
        assert!(manager.is_in_cooldown(&validator_id, 51));
        // SECURITY FIX #12: Cooldown is now inclusive of end_epoch (52)
        assert!(manager.is_in_cooldown(&validator_id, 52)); // end_epoch is included
        assert!(!manager.is_in_cooldown(&validator_id, 53)); // past end_epoch
    }

    #[test]
    fn test_ejection_after_repeated_critical() {
        let mut manager = SlashingManager::new();
        let validator_id = AccountId::from_bytes([1; 32]);
        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 300; // Enough for 3 slashes

        // First critical event
        let event1 = SlashableEvent::DoubleSigning {
            slot: 1,
            epoch: 0,
            block_hash_1: [1; 32],
            block_hash_2: [2; 32],
        };
        let record1 = manager
            .slash_validator(validator_id, event1, 0, 0, &mut vc_record, 1_000_000)
            .unwrap();
        assert!(!record1.ejected);

        // Second critical event
        let event2 = SlashableEvent::Equivocation {
            epoch: 1,
            evidence: vec![],
        };
        let record2 = manager
            .slash_validator(validator_id, event2, 1, 100, &mut vc_record, 1_000_000)
            .unwrap();
        assert!(!record2.ejected);

        // Third critical event → ejection
        let event3 = SlashableEvent::DoubleSigning {
            slot: 2,
            epoch: 2,
            block_hash_1: [3; 32],
            block_hash_2: [4; 32],
        };
        let record3 = manager
            .slash_validator(validator_id, event3, 2, 200, &mut vc_record, 1_000_000)
            .unwrap();
        assert!(record3.ejected);
    }

    #[test]
    fn test_stake_slash_only_when_vc_exhausted() {
        let mut manager = SlashingManager::new();
        let validator_id = AccountId::from_bytes([1; 32]);
        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 10; // Small VC

        // Medium severity event
        let event = SlashableEvent::ExtendedDowntime { epochs_offline: 15 };

        let record = manager
            .slash_validator(validator_id, event, 0, 0, &mut vc_record, 1_000_000)
            .unwrap();

        // VC > 0 after slash, so no stake slash
        assert_eq!(record.stake_slashed, 0);

        // Now exhaust VC with critical event
        let event2 = SlashableEvent::DoubleSigning {
            slot: 1,
            epoch: 1,
            block_hash_1: [1; 32],
            block_hash_2: [2; 32],
        };

        let record2 = manager
            .slash_validator(validator_id, event2, 1, 100, &mut vc_record, 1_000_000)
            .unwrap();

        // Critical event always slashes stake
        assert!(record2.stake_slashed > 0);
    }

    #[test]
    fn test_critical_count_decay() {
        let mut manager = SlashingManager::new();
        let validator_id = AccountId::from_bytes([1; 32]);
        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 500; // Enough for multiple slashes

        // First critical event at epoch 0
        let event1 = SlashableEvent::DoubleSigning {
            slot: 1,
            epoch: 0,
            block_hash_1: [1; 32],
            block_hash_2: [2; 32],
        };
        let record1 = manager
            .slash_validator(validator_id, event1, 0, 0, &mut vc_record, 1_000_000)
            .unwrap();
        assert!(!record1.ejected);

        // Second critical event at epoch 1 (no decay yet)
        let event2 = SlashableEvent::Equivocation {
            epoch: 1,
            evidence: vec![],
        };
        let record2 = manager
            .slash_validator(validator_id, event2, 1, 100, &mut vc_record, 1_000_000)
            .unwrap();
        assert!(!record2.ejected);
        // Now at count = 2

        // Wait long enough for decay (CRITICAL_COUNT_DECAY_EPOCHS = 26)
        // After 26 epochs, count decays by 1, so count goes from 2 -> 1

        // Third critical event at epoch 30 (after one decay period)
        let event3 = SlashableEvent::DoubleSigning {
            slot: 2,
            epoch: 30,
            block_hash_1: [3; 32],
            block_hash_2: [4; 32],
        };
        let record3 = manager
            .slash_validator(validator_id, event3, 30, 300, &mut vc_record, 1_000_000)
            .unwrap();
        // count was 2, decays by 1 (30-1=29 epochs, 29/26=1 decay), becomes 1
        // then +1 for new event = 2
        // So NOT ejected (need 3)
        assert!(!record3.ejected);

        // Fourth critical event at epoch 31 (right after, no more decay)
        let event4 = SlashableEvent::DoubleSigning {
            slot: 3,
            epoch: 31,
            block_hash_1: [5; 32],
            block_hash_2: [6; 32],
        };
        let record4 = manager
            .slash_validator(validator_id, event4, 31, 400, &mut vc_record, 1_000_000)
            .unwrap();
        // count was 2, no decay (only 1 epoch since last), becomes 3 -> EJECTED
        assert!(record4.ejected);
    }

    #[test]
    fn test_epoch_boundary_cleanup() {
        let mut manager = SlashingManager::new();
        let validator_id = AccountId::from_bytes([1; 32]);
        let mut vc_record = ValidatorCreditsRecord::new(0, 0);
        vc_record.vote_credits = 100;

        // Create a slashing record at epoch 0
        let event = SlashableEvent::ExtendedDowntime { epochs_offline: 15 };
        manager
            .slash_validator(validator_id, event, 0, 0, &mut vc_record, 1_000_000)
            .unwrap();

        assert_eq!(manager.get_all_slashes().len(), 1);

        // Epoch boundary at epoch 50 - record should still exist
        manager.on_epoch_boundary(50);
        assert_eq!(manager.get_all_slashes().len(), 1);

        // Epoch boundary at epoch 150 (after SLASHING_RECORD_RETENTION_EPOCHS = 104)
        manager.on_epoch_boundary(150);
        assert_eq!(manager.get_all_slashes().len(), 0); // Pruned
    }
}
