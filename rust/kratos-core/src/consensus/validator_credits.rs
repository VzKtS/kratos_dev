// Validator Credits - Merit-based validation system
// Principle: Allow becoming a validator through time and engagement, not just wealth

use crate::types::{AccountId, BlockNumber, EpochNumber};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Time windows for anti-spam (in epochs)
/// With 1 epoch = 1 hour (600 blocks at 6s/slot):
const EPOCHS_PER_DAY: u64 = 24;    // 24 hours = 24 epochs
const EPOCHS_PER_MONTH: u64 = 720; // 30 days × 24 hours = 720 epochs
const EPOCHS_PER_YEAR: u64 = 8_760; // 365 days × 24 hours = 8,760 epochs

/// Validator Credits Record
/// Non-transferable credits earned through participation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatorCreditsRecord {
    /// Credits from governance votes
    pub vote_credits: u32,

    /// Credits from uptime (consensus participation)
    pub uptime_credits: u32,

    /// Credits from accepted arbitrations
    pub arbitration_credits: u32,

    /// Credits from seniority (active epochs)
    pub seniority_credits: u32,

    /// Last update block number
    pub last_update: BlockNumber,

    /// Anti-spam tracking: votes per day (epoch-based)
    pub votes_today: u32,

    /// Anti-spam tracking: votes per month (epoch-based)
    pub votes_this_month: u32,

    /// Anti-spam tracking: arbitrations this year (epoch-based)
    pub arbitrations_this_year: u32,

    /// Epoch of last daily reset (for vote spam)
    pub last_daily_reset_epoch: EpochNumber,

    /// Epoch of last monthly reset (for vote spam)
    pub last_monthly_reset_epoch: EpochNumber,

    /// Epoch of last yearly reset (for arbitration spam)
    pub last_yearly_reset_epoch: EpochNumber,

    /// Number of active epochs (for seniority)
    pub active_epochs: u32,

    /// Block when validator became active
    pub activation_block: BlockNumber,

    /// Last seniority credit epoch
    pub last_seniority_credit_epoch: EpochNumber,
}

impl ValidatorCreditsRecord {
    /// Create new VC record
    pub fn new(block_number: BlockNumber, current_epoch: EpochNumber) -> Self {
        Self {
            vote_credits: 0,
            uptime_credits: 0,
            arbitration_credits: 0,
            seniority_credits: 0,
            last_update: block_number,
            votes_today: 0,
            votes_this_month: 0,
            arbitrations_this_year: 0,
            last_daily_reset_epoch: current_epoch,
            last_monthly_reset_epoch: current_epoch,
            last_yearly_reset_epoch: current_epoch,
            active_epochs: 0,
            activation_block: block_number,
            last_seniority_credit_epoch: current_epoch,
        }
    }

    /// Calculate total VC
    pub fn total_vc(&self) -> u64 {
        self.vote_credits as u64
            + self.uptime_credits as u64
            + self.arbitration_credits as u64
            + self.seniority_credits as u64
    }

    /// Reset daily counters if needed (epoch-based)
    fn maybe_reset_daily(&mut self, current_epoch: EpochNumber) {
        if current_epoch >= self.last_daily_reset_epoch + EPOCHS_PER_DAY {
            self.votes_today = 0;
            self.last_daily_reset_epoch = current_epoch;
        }
    }

    /// Reset monthly counters if needed (epoch-based)
    fn maybe_reset_monthly(&mut self, current_epoch: EpochNumber) {
        if current_epoch >= self.last_monthly_reset_epoch + EPOCHS_PER_MONTH {
            self.votes_this_month = 0;
            self.last_monthly_reset_epoch = current_epoch;
        }
    }

    /// Reset yearly counters if needed (epoch-based)
    fn maybe_reset_yearly(&mut self, current_epoch: EpochNumber) {
        if current_epoch >= self.last_yearly_reset_epoch + EPOCHS_PER_YEAR {
            self.arbitrations_this_year = 0;
            self.last_yearly_reset_epoch = current_epoch;
        }
    }

    /// Add vote credit with anti-spam protection (epoch-based)
    /// Returns true if credit was added, false if limit reached
    ///
    /// SPEC v2.3: During bootstrap (first 1440 epochs = 60 days), vote credits are multiplied by 2x
    pub fn add_vote_credit(
        &mut self,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
    ) -> Result<bool, VCError> {
        self.add_vote_credit_with_multiplier(block_number, current_epoch, 1)
    }

    /// Add vote credit with configurable multiplier (for bootstrap era)
    /// multiplier: 2 during bootstrap, 1 post-bootstrap
    pub fn add_vote_credit_with_multiplier(
        &mut self,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
        multiplier: u32,
    ) -> Result<bool, VCError> {
        self.maybe_reset_daily(current_epoch);
        self.maybe_reset_monthly(current_epoch);

        // Check daily limit: max 3 votes per day (epoch)
        if self.votes_today >= 3 {
            return Ok(false);
        }

        // Check monthly limit: max 50 votes per month (4 epochs)
        if self.votes_this_month >= 50 {
            return Ok(false);
        }

        // Add credit with multiplier (SPEC v2: 2x during bootstrap)
        // FIX: Removed redundant "1 *" multiplication
        let credits_to_add = multiplier.max(1);
        self.vote_credits = self.vote_credits.saturating_add(credits_to_add);
        self.votes_today += 1;
        self.votes_this_month += 1;
        self.last_update = block_number;

        Ok(true)
    }

    /// Add uptime credit (1 per epoch with ≥95% participation)
    ///
    /// SPEC v2.3: During bootstrap (first 1440 epochs = 60 days), uptime credits are multiplied by 2x
    pub fn add_uptime_credit(
        &mut self,
        block_number: BlockNumber,
        participation_rate: f64,
    ) -> Result<bool, VCError> {
        self.add_uptime_credit_with_multiplier(block_number, participation_rate, 1)
    }

    /// Add uptime credit with configurable multiplier (for bootstrap era)
    /// multiplier: 2 during bootstrap, 1 post-bootstrap
    pub fn add_uptime_credit_with_multiplier(
        &mut self,
        block_number: BlockNumber,
        participation_rate: f64,
        multiplier: u32,
    ) -> Result<bool, VCError> {
        if participation_rate < 0.95 {
            return Ok(false);
        }

        // Add credit with multiplier (SPEC v2: 2x during bootstrap)
        // FIX: Removed redundant "1 *" multiplication
        let credits_to_add = multiplier.max(1);
        self.uptime_credits = self.uptime_credits.saturating_add(credits_to_add);
        self.last_update = block_number;

        Ok(true)
    }

    /// Add arbitration credit with anti-spam protection (epoch-based)
    /// +5 VC per accepted arbitration, max 5 per year
    pub fn add_arbitration_credit(
        &mut self,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
    ) -> Result<bool, VCError> {
        self.maybe_reset_yearly(current_epoch);

        // Check yearly limit: max 5 arbitrations per year (52 epochs)
        if self.arbitrations_this_year >= 5 {
            return Ok(false);
        }

        // Add 5 credits
        self.arbitration_credits += 5;
        self.arbitrations_this_year += 1;
        self.last_update = block_number;

        Ok(true)
    }

    /// Add seniority credit (epoch-based)
    /// +5 VC per active period (4 epochs = ~1 month)
    /// Called when sufficient epochs have passed
    pub fn add_seniority_credit(
        &mut self,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
    ) -> Result<bool, VCError> {
        // Check if at least EPOCHS_PER_MONTH has passed since last seniority credit
        if current_epoch < self.last_seniority_credit_epoch + EPOCHS_PER_MONTH {
            return Ok(false);
        }

        // Add credit
        // FIX: Use saturating_add to prevent overflow after ~638 years
        self.seniority_credits = self.seniority_credits.saturating_add(5);
        self.active_epochs = self.active_epochs.saturating_add(EPOCHS_PER_MONTH as u32);
        self.last_seniority_credit_epoch = current_epoch;
        self.last_update = block_number;

        Ok(true)
    }
}

/// Validator Credits Manager
/// Manages VC records for all validators
pub struct ValidatorCreditsManager {
    /// VC records indexed by validator account ID
    records: HashMap<AccountId, ValidatorCreditsRecord>,
}

impl ValidatorCreditsManager {
    /// Create new manager
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    /// Initialize VC record for new validator
    pub fn initialize_validator(
        &mut self,
        validator_id: AccountId,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
    ) {
        self.records
            .insert(validator_id, ValidatorCreditsRecord::new(block_number, current_epoch));
    }

    /// Get VC record for validator
    pub fn get_record(&self, validator_id: &AccountId) -> Option<&ValidatorCreditsRecord> {
        self.records.get(validator_id)
    }

    /// Get mutable VC record for validator
    pub fn get_record_mut(
        &mut self,
        validator_id: &AccountId,
    ) -> Option<&mut ValidatorCreditsRecord> {
        self.records.get_mut(validator_id)
    }

    /// Get total VC for validator
    pub fn get_total_vc(&self, validator_id: &AccountId) -> u64 {
        self.records
            .get(validator_id)
            .map(|r| r.total_vc())
            .unwrap_or(0)
    }

    /// Process governance vote for validator
    pub fn record_governance_vote(
        &mut self,
        validator_id: &AccountId,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
    ) -> Result<bool, VCError> {
        self.record_governance_vote_with_multiplier(validator_id, block_number, current_epoch, 1)
    }

    /// Process governance vote with bootstrap multiplier
    /// SPEC v2: During bootstrap, vote credits are multiplied by 2x
    pub fn record_governance_vote_with_multiplier(
        &mut self,
        validator_id: &AccountId,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
        multiplier: u32,
    ) -> Result<bool, VCError> {
        let record = self
            .records
            .get_mut(validator_id)
            .ok_or(VCError::ValidatorNotFound)?;

        record.add_vote_credit_with_multiplier(block_number, current_epoch, multiplier)
    }

    /// Process uptime for epoch
    pub fn record_uptime(
        &mut self,
        validator_id: &AccountId,
        block_number: BlockNumber,
        participation_rate: f64,
    ) -> Result<bool, VCError> {
        self.record_uptime_with_multiplier(validator_id, block_number, participation_rate, 1)
    }

    /// Process uptime with bootstrap multiplier
    /// SPEC v2: During bootstrap, uptime credits are multiplied by 2x
    pub fn record_uptime_with_multiplier(
        &mut self,
        validator_id: &AccountId,
        block_number: BlockNumber,
        participation_rate: f64,
        multiplier: u32,
    ) -> Result<bool, VCError> {
        let record = self
            .records
            .get_mut(validator_id)
            .ok_or(VCError::ValidatorNotFound)?;

        record.add_uptime_credit_with_multiplier(block_number, participation_rate, multiplier)
    }

    /// Process accepted arbitration
    pub fn record_arbitration(
        &mut self,
        validator_id: &AccountId,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
    ) -> Result<bool, VCError> {
        let record = self
            .records
            .get_mut(validator_id)
            .ok_or(VCError::ValidatorNotFound)?;

        record.add_arbitration_credit(block_number, current_epoch)
    }

    /// Process seniority credits (called periodically at epoch boundaries)
    pub fn update_seniority(
        &mut self,
        validator_id: &AccountId,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
    ) -> Result<bool, VCError> {
        let record = self
            .records
            .get_mut(validator_id)
            .ok_or(VCError::ValidatorNotFound)?;

        record.add_seniority_credit(block_number, current_epoch)
    }

    /// Update seniority for all validators (called at epoch boundaries)
    pub fn update_all_seniority(
        &mut self,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
    ) -> Vec<(AccountId, bool)> {
        self.records
            .iter_mut()
            .map(|(id, record)| {
                let added = record
                    .add_seniority_credit(block_number, current_epoch)
                    .unwrap_or(false);
                (*id, added)
            })
            .collect()
    }
}

impl Default for ValidatorCreditsManager {
    fn default() -> Self {
        Self::new()
    }
}

/// VC-related errors
#[derive(Debug, thiserror::Error)]
pub enum VCError {
    #[error("Validator not found")]
    ValidatorNotFound,

    #[error("Daily vote limit reached (max 3)")]
    DailyVoteLimitReached,

    #[error("Monthly vote limit reached (max 50)")]
    MonthlyVoteLimitReached,

    #[error("Yearly arbitration limit reached (max 5)")]
    YearlyArbitrationLimitReached,

    #[error("Insufficient participation rate (need ≥95%)")]
    InsufficientParticipation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vc_record_creation() {
        let record = ValidatorCreditsRecord::new(0, 0);
        assert_eq!(record.total_vc(), 0);
        assert_eq!(record.vote_credits, 0);
        assert_eq!(record.uptime_credits, 0);
        assert_eq!(record.arbitration_credits, 0);
        assert_eq!(record.seniority_credits, 0);
    }

    #[test]
    fn test_vote_credit_limit() {
        let mut record = ValidatorCreditsRecord::new(0, 0);

        // Add 3 votes (daily limit per epoch)
        assert!(record.add_vote_credit(1, 0).unwrap());
        assert!(record.add_vote_credit(2, 0).unwrap());
        assert!(record.add_vote_credit(3, 0).unwrap());

        // 4th vote should be rejected (daily limit)
        assert!(!record.add_vote_credit(4, 0).unwrap());
        assert_eq!(record.vote_credits, 3);

        // After EPOCHS_PER_DAY (24 epochs), should be able to vote again
        assert!(record.add_vote_credit(5, 24).unwrap());
        assert_eq!(record.vote_credits, 4);
    }

    #[test]
    fn test_uptime_credit_threshold() {
        let mut record = ValidatorCreditsRecord::new(0, 0);

        // Below 95% should not grant credit
        assert!(!record.add_uptime_credit(1, 0.94).unwrap());
        assert_eq!(record.uptime_credits, 0);

        // At or above 95% should grant credit
        assert!(record.add_uptime_credit(2, 0.95).unwrap());
        assert_eq!(record.uptime_credits, 1);

        assert!(record.add_uptime_credit(3, 1.0).unwrap());
        assert_eq!(record.uptime_credits, 2);
    }

    #[test]
    fn test_arbitration_credit() {
        let mut record = ValidatorCreditsRecord::new(0, 0);

        // Each arbitration gives +5 VC
        assert!(record.add_arbitration_credit(1, 0).unwrap());
        assert_eq!(record.arbitration_credits, 5);

        // Max 5 arbitrations per year (8760 epochs)
        for _ in 1..5 {
            assert!(record.add_arbitration_credit(2, 0).unwrap());
        }
        assert_eq!(record.arbitration_credits, 25);

        // 6th should be rejected
        assert!(!record.add_arbitration_credit(3, 0).unwrap());
        assert_eq!(record.arbitration_credits, 25);
    }

    #[test]
    fn test_seniority_credit() {
        let mut record = ValidatorCreditsRecord::new(0, 0);

        // Too soon, no credit (same epoch)
        assert!(!record.add_seniority_credit(1, 0).unwrap());
        assert_eq!(record.seniority_credits, 0);

        // After EPOCHS_PER_MONTH (720 epochs), should grant +5 VC
        assert!(record.add_seniority_credit(2, 720).unwrap());
        assert_eq!(record.seniority_credits, 5);
        assert_eq!(record.active_epochs, 720);

        // Another period, another +5 VC
        assert!(record.add_seniority_credit(3, 1440).unwrap());
        assert_eq!(record.seniority_credits, 10);
        assert_eq!(record.active_epochs, 1440);
    }

    #[test]
    fn test_total_vc_calculation() {
        let mut record = ValidatorCreditsRecord::new(0, 0);

        record.add_vote_credit(1, 0).unwrap();
        assert_eq!(record.total_vc(), 1);

        record.add_uptime_credit(2, 0.95).unwrap();
        assert_eq!(record.total_vc(), 2);

        record.add_arbitration_credit(3, 0).unwrap();
        assert_eq!(record.total_vc(), 7); // 1 + 1 + 5

        record.add_seniority_credit(4, 720).unwrap();
        assert_eq!(record.total_vc(), 12); // 1 + 1 + 5 + 5
    }

    #[test]
    fn test_manager_operations() {
        let mut manager = ValidatorCreditsManager::new();
        let validator = AccountId::from_bytes([1; 32]);

        // Initialize validator
        manager.initialize_validator(validator, 0, 0);
        assert_eq!(manager.get_total_vc(&validator), 0);

        // Record vote
        assert!(manager.record_governance_vote(&validator, 1, 0).unwrap());
        assert_eq!(manager.get_total_vc(&validator), 1);

        // Record uptime
        assert!(manager.record_uptime(&validator, 2, 0.98).unwrap());
        assert_eq!(manager.get_total_vc(&validator), 2);

        // Record arbitration
        assert!(manager.record_arbitration(&validator, 3, 0).unwrap());
        assert_eq!(manager.get_total_vc(&validator), 7);
    }

    // ========================================================================
    // SECURITY INVARIANT TESTS
    // ========================================================================

    /// INVARIANT 1: VC components are always non-negative
    #[test]
    fn invariant_vc_non_negative() {
        let mut record = ValidatorCreditsRecord::new(0, 0);

        // Test all credit types
        for epoch in 0..100 {
            // Try adding credits
            let _ = record.add_vote_credit(epoch, epoch);
            let _ = record.add_uptime_credit(epoch, 0.96);
            let _ = record.add_arbitration_credit(epoch, epoch);
            let _ = record.add_seniority_credit(epoch, epoch);

            // INVARIANT: All components are u32 (unsigned), so always >= 0 by type definition
            // No need to assert >= 0 for unsigned types
        }
    }

    /// INVARIANT 2: VC total is monotonically increasing (no slashing implemented)
    #[test]
    fn invariant_vc_monotonic() {
        let mut record = ValidatorCreditsRecord::new(0, 0);
        let mut previous_total = record.total_vc();

        // Add various credits over time
        for epoch in 0..52 {
            let block = epoch;

            // Add credits (some may be rejected due to limits)
            let _ = record.add_vote_credit(block, epoch);
            let _ = record.add_uptime_credit(block, 0.96);
            if epoch % 10 == 0 {
                let _ = record.add_arbitration_credit(block, epoch);
            }
            if epoch % 4 == 0 && epoch > 0 {
                let _ = record.add_seniority_credit(block, epoch);
            }

            let current_total = record.total_vc();

            // INVARIANT: Total VC can only increase or stay the same (no slashing)
            assert!(current_total >= previous_total,
                "VC decreased from {} to {} at epoch {}", previous_total, current_total, epoch);

            previous_total = current_total;
        }
    }

    /// INVARIANT 3: Individual credit components never exceed expected maximums
    #[test]
    fn invariant_vc_component_bounds() {
        let mut record = ValidatorCreditsRecord::new(0, 0);

        // Simulate 1 year (52 epochs)
        for epoch in 0..52 {
            let block = epoch;

            // Try to spam all credit types
            for _ in 0..10 {
                let _ = record.add_vote_credit(block, epoch);
            }
            let _ = record.add_uptime_credit(block, 1.0);
            let _ = record.add_arbitration_credit(block, epoch);
            let _ = record.add_seniority_credit(block, epoch);
        }

        // INVARIANT: Vote credits respect limits
        // Max: 3 votes/epoch × 52 epochs = 156 votes
        assert!(record.vote_credits <= 156,
            "Vote credits exceeded maximum: {}", record.vote_credits);

        // INVARIANT: Uptime credits respect limits
        // Max: 1 per epoch × 52 epochs = 52
        assert!(record.uptime_credits <= 52,
            "Uptime credits exceeded maximum: {}", record.uptime_credits);

        // INVARIANT: Arbitration credits respect yearly limit
        // Max: 5 arbitrations × 5 VC = 25 VC per year
        assert!(record.arbitration_credits <= 25,
            "Arbitration credits exceeded maximum: {}", record.arbitration_credits);
    }

    /// INVARIANT 4: Anti-spam limits are enforced correctly
    #[test]
    fn invariant_antispam_enforcement() {
        let mut record = ValidatorCreditsRecord::new(0, 0);

        // Test daily vote limit (3 per epoch)
        for i in 0..10 {
            let added = record.add_vote_credit(i, 0).unwrap();
            if i < 3 {
                assert!(added, "Should accept vote {} in same epoch", i);
            } else {
                assert!(!added, "Should reject vote {} (daily limit)", i);
            }
        }

        // INVARIANT: Daily counter never exceeds limit
        assert!(record.votes_today <= 3);

        // Test yearly arbitration limit (5 per year)
        let mut record2 = ValidatorCreditsRecord::new(0, 0);
        for i in 0..10 {
            let added = record2.add_arbitration_credit(i, 0).unwrap();
            if i < 5 {
                assert!(added, "Should accept arbitration {} in same year", i);
            } else {
                assert!(!added, "Should reject arbitration {} (yearly limit)", i);
            }
        }

        // INVARIANT: Yearly counter never exceeds limit
        assert!(record2.arbitrations_this_year <= 5);
    }

    /// INVARIANT 5: Epoch-based resets work correctly
    #[test]
    fn invariant_epoch_resets() {
        let mut record = ValidatorCreditsRecord::new(0, 0);

        // Fill daily limit in epoch 0
        for i in 0..3 {
            assert!(record.add_vote_credit(i, 0).unwrap());
        }
        assert_eq!(record.votes_today, 3);

        // Move to epoch 24 (EPOCHS_PER_DAY = 24, so epoch 24 starts a new day)
        assert!(record.add_vote_credit(10, 24).unwrap());

        // INVARIANT: Daily counter should have reset
        assert_eq!(record.votes_today, 1, "Daily counter should reset after epoch boundary");
        assert_eq!(record.last_daily_reset_epoch, 24);
    }
}

#[cfg(test)]
mod vrf_invariant_tests {
    use super::*;
    use crate::consensus::vrf_selection::{compute_vrf_weight, VRFSelector};

    /// INVARIANT 6: StakeCap is enforced (stake above cap doesn't increase weight linearly)
    #[test]
    fn invariant_stake_cap_enforced() {
        const STAKE_CAP: u128 = 1_000_000;
        let vc = 10;

        // Weight at cap
        let weight_at_cap = compute_vrf_weight(STAKE_CAP, vc);

        // Weight at 10x cap should be the same
        let weight_above_cap = compute_vrf_weight(STAKE_CAP * 10, vc);

        // INVARIANT: Stake cap prevents plutocracy
        assert_eq!(weight_at_cap, weight_above_cap,
            "Stake above cap should not increase weight");

        // Weight below cap should be less
        let weight_below_cap = compute_vrf_weight(STAKE_CAP / 2, vc);
        assert!(weight_below_cap < weight_at_cap,
            "Stake below cap should give lower weight");
    }

    /// INVARIANT 7: VC increases weight monotonically
    /// Note: Due to the cold-start fix (MIN_EFFECTIVE_VC=1), VC=0 and VC=1 give
    /// the same weight. This ensures new validators with VC=0 can participate.
    #[test]
    fn invariant_vc_increases_weight() {
        let stake = 500_000;

        let weight_vc0 = compute_vrf_weight(stake, 0);
        let weight_vc1 = compute_vrf_weight(stake, 1);
        let weight_vc10 = compute_vrf_weight(stake, 10);
        let weight_vc100 = compute_vrf_weight(stake, 100);

        // With MIN_EFFECTIVE_VC=1, VC=0 and VC=1 both use effective_vc=1
        // So weight_vc0 == weight_vc1 (this is intentional for cold-start)
        assert!((weight_vc0 - weight_vc1).abs() < 0.001,
            "VC=0 and VC=1 should have same weight due to MIN_EFFECTIVE_VC");

        // INVARIANT: More VC → higher weight (when VC > MIN_EFFECTIVE_VC)
        assert!(weight_vc10 > weight_vc1,
            "VC=10 should have higher weight than VC=1");
        assert!(weight_vc100 > weight_vc10,
            "VC=100 should have higher weight than VC=10");

        // Also verify VC=0 gives non-zero weight (cold-start fix)
        assert!(weight_vc0 > 0.0,
            "VC=0 should still have positive weight for cold-start");
    }

    /// INVARIANT 8: VRF weight is always non-negative
    #[test]
    fn invariant_vrf_weight_non_negative() {
        // Test various stake/VC combinations
        for stake in [0, 1000, 100_000, 1_000_000, 10_000_000] {
            for vc in [0, 1, 10, 100, 1000] {
                let weight = compute_vrf_weight(stake, vc);

                // INVARIANT: Weight is always >= 0
                assert!(weight >= 0.0,
                    "Weight should be non-negative for stake={}, vc={}", stake, vc);
            }
        }
    }

    /// INVARIANT 9: Identical validators have identical selection probability
    #[test]
    fn invariant_vrf_fairness() {
        let validator1 = AccountId::from_bytes([1; 32]);
        let validator2 = AccountId::from_bytes([1; 32]); // Same ID

        let stake = 500_000;
        let vc = 10;

        let candidates1 = vec![(validator1, stake, vc)];
        let candidates2 = vec![(validator2, stake, vc)];

        // Run selection for multiple slots
        for slot in 0..10 {
            let selected1 = VRFSelector::select_validator(slot, 0, &candidates1).unwrap();
            let selected2 = VRFSelector::select_validator(slot, 0, &candidates2).unwrap();

            // INVARIANT: Same inputs → same output
            assert_eq!(selected1, selected2,
                "Identical validators should be selected identically at slot {}", slot);
        }
    }

    /// INVARIANT 10: VRF selection is deterministic
    #[test]
    fn invariant_vrf_deterministic() {
        let validator = AccountId::from_bytes([42; 32]);
        let candidates = vec![(validator, 500_000, 10)];

        // Select multiple times for same slot/epoch
        let selected1 = VRFSelector::select_validator(5, 2, &candidates).unwrap();
        let selected2 = VRFSelector::select_validator(5, 2, &candidates).unwrap();
        let selected3 = VRFSelector::select_validator(5, 2, &candidates).unwrap();

        // INVARIANT: Same inputs always produce same output
        assert_eq!(selected1, selected2);
        assert_eq!(selected2, selected3);
    }

    /// INVARIANT 11: Higher weight increases selection probability
    #[test]
    fn invariant_higher_weight_favored() {
        let validator_low = AccountId::from_bytes([1; 32]);
        let validator_high = AccountId::from_bytes([2; 32]);

        // High validator has much higher weight
        let candidates = vec![
            (validator_low, 100_000, 0),   // Low stake, no VC → very low weight
            (validator_high, 900_000, 100), // High stake, high VC → high weight
        ];

        // Run selection over many slots
        let mut selections = std::collections::HashMap::new();
        for slot in 0..1000 {
            let selected = VRFSelector::select_validator(slot, 0, &candidates).unwrap();
            *selections.entry(selected).or_insert(0) += 1;
        }

        let low_count = *selections.get(&validator_low).unwrap_or(&0);
        let high_count = *selections.get(&validator_high).unwrap_or(&0);

        // INVARIANT: Higher weight validator should be selected more often
        // (This is probabilistic but with 1000 trials and large weight difference,
        //  it should hold with very high probability)
        assert!(high_count > low_count,
            "Higher weight validator should be selected more: high={}, low={}",
            high_count, low_count);
    }
}
