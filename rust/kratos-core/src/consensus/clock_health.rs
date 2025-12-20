// Clock Health - SPEC v6.1: Time Synchronization & Soft Degradation
//
// This module implements a dual-layer clock health system:
// 1. LocalClockHealth: Per-node monitoring with soft degradation
// 2. ValidatorClockRecord: Consensus state for VC impact tracking
//
// SECURITY FIX #36: Prevents gaming via node restarts by persisting local state

use crate::types::{AccountId, BlockNumber, EpochNumber};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;
use tracing::{debug, info, warn, error};

// =============================================================================
// CONSTANTS (SPEC v6.1 Section 3.2 & 3.3)
// =============================================================================

/// Below this drift = Healthy status
pub const HEALTHY_MAX_DRIFT_MS: i64 = 1000;

/// Below this drift = Degraded, above = Excluded
pub const DEGRADED_MAX_DRIFT_MS: i64 = 1500;

/// Must be below this to start recovering from Excluded
pub const RECOVERY_DRIFT_MS: i64 = 1300;

/// Must be below this to return to Healthy from Degraded
pub const HEALTHY_DRIFT_MS: i64 = 800;

/// Rolling window for drift calculation (seconds)
pub const MEASUREMENT_WINDOW_SECS: u64 = 120;

/// Time at <1.3s drift to exit Excluded → Recovering
pub const EXCLUDED_TO_RECOVERING_SECS: u64 = 120;

/// Time at <0.8s drift to return Degraded → Healthy
pub const DEGRADED_TO_HEALTHY_SECS: u64 = 240;

/// Time at <0.8s drift to return Recovering → Healthy
pub const RECOVERING_TO_HEALTHY_SECS: u64 = 240;

/// Exponential moving average smoothing factor
pub const EMA_ALPHA: f64 = 0.1;

/// Maximum samples to keep in rolling window
pub const MAX_SAMPLES: usize = 120;

/// Threshold for emergency mode activation (30% validators degraded/excluded)
pub const EMERGENCY_THRESHOLD_RATIO: f64 = 0.30;

/// Threshold for emergency mode deactivation (15% validators degraded/excluded)
pub const EMERGENCY_RECOVERY_RATIO: f64 = 0.15;

/// Time to sustain recovery ratio before exiting emergency mode (seconds)
pub const EMERGENCY_RECOVERY_SUSTAIN_SECS: u64 = 600;

/// VC penalty per clock sync failure
pub const CLOCK_FAILURE_VC_PENALTY: u64 = 1;

// =============================================================================
// CLOCK STATUS
// =============================================================================

/// Clock synchronization status for a validator node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClockStatus {
    /// Drift < 1.0s - Normal operation
    Healthy,

    /// Drift 1.0s - 1.5s - Reduced priority, warning state
    Degraded,

    /// Drift > 1.5s - No block production, VC impact
    Excluded,

    /// Was Excluded, drift improving, observation period
    Recovering,
}

impl ClockStatus {
    /// Can this status produce blocks?
    pub fn can_produce_blocks(&self) -> bool {
        matches!(self, ClockStatus::Healthy | ClockStatus::Degraded)
    }

    /// Is this status in a warning state?
    pub fn is_warning(&self) -> bool {
        !matches!(self, ClockStatus::Healthy)
    }

    /// Get log level for this status
    pub fn log_level(&self) -> &'static str {
        match self {
            ClockStatus::Healthy => "DEBUG",
            ClockStatus::Degraded => "WARN",
            ClockStatus::Excluded => "ERROR",
            ClockStatus::Recovering => "INFO",
        }
    }
}

impl std::fmt::Display for ClockStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClockStatus::Healthy => write!(f, "Healthy"),
            ClockStatus::Degraded => write!(f, "Degraded"),
            ClockStatus::Excluded => write!(f, "Excluded"),
            ClockStatus::Recovering => write!(f, "Recovering"),
        }
    }
}

// =============================================================================
// DRIFT SAMPLE
// =============================================================================

/// A single drift measurement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftSample {
    /// Unix timestamp when sample was taken
    pub timestamp: u64,

    /// Measured drift in milliseconds (signed)
    pub drift_ms: i64,
}

// =============================================================================
// LOCAL CLOCK HEALTH (Per-Node, Persisted to File)
// =============================================================================

/// Local clock health state - persisted to file to survive restarts
///
/// SPEC v6.1 Section 3.4: This is stored locally (not in consensus)
/// because each node has its own clock and network latency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalClockHealth {
    /// Current status
    pub status: ClockStatus,

    /// Exponential moving average of drift (milliseconds)
    pub ema_drift_ms: f64,

    /// Rolling window of samples
    pub samples: VecDeque<DriftSample>,

    /// Unix timestamp when current status started
    pub status_since: u64,

    /// Timestamp when recovery/healthy conditions started being met
    /// Used for hysteresis (must sustain good drift for X seconds)
    pub improvement_since: Option<u64>,

    /// Historical counter: total times excluded
    pub lifetime_excluded_count: u32,

    /// Historical counter: total seconds in degraded state
    pub lifetime_degraded_seconds: u64,

    /// Is emergency threshold expansion active?
    pub emergency_mode: bool,

    /// When emergency recovery conditions started being met
    pub emergency_recovery_since: Option<u64>,
}

impl Default for LocalClockHealth {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalClockHealth {
    /// Create a new LocalClockHealth with default Healthy status
    pub fn new() -> Self {
        let now = chrono::Utc::now().timestamp() as u64;
        Self {
            status: ClockStatus::Healthy,
            ema_drift_ms: 0.0,
            samples: VecDeque::with_capacity(MAX_SAMPLES),
            status_since: now,
            improvement_since: None,
            lifetime_excluded_count: 0,
            lifetime_degraded_seconds: 0,
            emergency_mode: false,
            emergency_recovery_since: None,
        }
    }

    /// Load from file or create new if not exists
    pub fn load_or_create(data_dir: &Path) -> Self {
        let path = data_dir.join("clock_health.json");

        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    match serde_json::from_str(&content) {
                        Ok(health) => {
                            info!("Loaded clock health from {:?}", path);
                            return health;
                        }
                        Err(e) => {
                            warn!("Failed to parse clock_health.json: {:?}, creating new", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read clock_health.json: {:?}, creating new", e);
                }
            }
        }

        info!("Creating new clock health state");
        Self::new()
    }

    /// Save to file
    pub fn save(&self, data_dir: &Path) -> Result<(), ClockHealthError> {
        let path = data_dir.join("clock_health.json");

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ClockHealthError::SerializationFailed(e.to_string()))?;

        std::fs::write(&path, content)
            .map_err(|e| ClockHealthError::IoError(e.to_string()))?;

        Ok(())
    }

    /// Record a new drift sample and update status
    /// Returns the new status and whether it changed
    pub fn record_drift(&mut self, drift_ms: i64) -> (ClockStatus, bool) {
        let now = chrono::Utc::now().timestamp() as u64;
        let old_status = self.status;

        // Add sample to rolling window
        self.samples.push_back(DriftSample {
            timestamp: now,
            drift_ms,
        });

        // Trim old samples outside window
        let window_start = now.saturating_sub(MEASUREMENT_WINDOW_SECS);
        while let Some(front) = self.samples.front() {
            if front.timestamp < window_start {
                self.samples.pop_front();
            } else {
                break;
            }
        }

        // Limit max samples
        while self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }

        // Update EMA
        let drift_abs = drift_ms.abs() as f64;
        self.ema_drift_ms = EMA_ALPHA * drift_abs + (1.0 - EMA_ALPHA) * self.ema_drift_ms;

        // Get effective thresholds (may be expanded in emergency mode)
        let (healthy_max, degraded_max, recovery_max, healthy_target) = self.get_effective_thresholds();

        // State machine transition
        let new_status = self.compute_new_status(
            now,
            healthy_max,
            degraded_max,
            recovery_max,
            healthy_target,
        );

        // Handle status change
        if new_status != old_status {
            self.handle_status_change(old_status, new_status, now);
        }

        let changed = new_status != old_status;
        (new_status, changed)
    }

    /// Get effective thresholds (normal or emergency)
    fn get_effective_thresholds(&self) -> (i64, i64, i64, i64) {
        if self.emergency_mode {
            // SPEC v6.1 Section 5.2: Emergency threshold expansion
            (1500, 2000, 1800, 1200)
        } else {
            (HEALTHY_MAX_DRIFT_MS, DEGRADED_MAX_DRIFT_MS, RECOVERY_DRIFT_MS, HEALTHY_DRIFT_MS)
        }
    }

    /// Compute new status based on current EMA and state
    fn compute_new_status(
        &mut self,
        now: u64,
        healthy_max: i64,
        degraded_max: i64,
        recovery_max: i64,
        healthy_target: i64,
    ) -> ClockStatus {
        let drift = self.ema_drift_ms as i64;

        match self.status {
            ClockStatus::Healthy => {
                if drift > degraded_max {
                    ClockStatus::Excluded
                } else if drift > healthy_max {
                    ClockStatus::Degraded
                } else {
                    ClockStatus::Healthy
                }
            }

            ClockStatus::Degraded => {
                if drift > degraded_max {
                    ClockStatus::Excluded
                } else if drift < healthy_target {
                    // Check hysteresis: must sustain for DEGRADED_TO_HEALTHY_SECS
                    if let Some(since) = self.improvement_since {
                        if now.saturating_sub(since) >= DEGRADED_TO_HEALTHY_SECS {
                            self.improvement_since = None;
                            return ClockStatus::Healthy;
                        }
                    } else {
                        self.improvement_since = Some(now);
                    }
                    ClockStatus::Degraded
                } else {
                    self.improvement_since = None;
                    ClockStatus::Degraded
                }
            }

            ClockStatus::Excluded => {
                if drift < recovery_max {
                    // Check hysteresis: must sustain for EXCLUDED_TO_RECOVERING_SECS
                    if let Some(since) = self.improvement_since {
                        if now.saturating_sub(since) >= EXCLUDED_TO_RECOVERING_SECS {
                            self.improvement_since = None;
                            return ClockStatus::Recovering;
                        }
                    } else {
                        self.improvement_since = Some(now);
                    }
                } else {
                    self.improvement_since = None;
                }
                ClockStatus::Excluded
            }

            ClockStatus::Recovering => {
                if drift > degraded_max {
                    // Got worse again
                    ClockStatus::Excluded
                } else if drift < healthy_target {
                    // Check hysteresis: must sustain for RECOVERING_TO_HEALTHY_SECS
                    if let Some(since) = self.improvement_since {
                        if now.saturating_sub(since) >= RECOVERING_TO_HEALTHY_SECS {
                            self.improvement_since = None;
                            return ClockStatus::Healthy;
                        }
                    } else {
                        self.improvement_since = Some(now);
                    }
                    ClockStatus::Recovering
                } else if drift > healthy_max {
                    // Still bad but not excluded-level
                    self.improvement_since = None;
                    ClockStatus::Degraded
                } else {
                    self.improvement_since = None;
                    ClockStatus::Recovering
                }
            }
        }
    }

    /// Handle status change - update counters and log
    fn handle_status_change(&mut self, old: ClockStatus, new: ClockStatus, now: u64) {
        // Update degraded time counter
        if old == ClockStatus::Degraded {
            let duration = now.saturating_sub(self.status_since);
            self.lifetime_degraded_seconds = self.lifetime_degraded_seconds.saturating_add(duration);
        }

        // Update excluded counter
        if new == ClockStatus::Excluded && old != ClockStatus::Excluded {
            self.lifetime_excluded_count += 1;
        }

        // Log the transition
        match new {
            ClockStatus::Healthy => {
                info!("Clock health: {} → {} (drift: {:.0}ms)", old, new, self.ema_drift_ms);
            }
            ClockStatus::Degraded => {
                warn!(
                    "⚠️ Time sync warning: {} → {} (drift: {:.0}ms) - Block production at reduced priority",
                    old, new, self.ema_drift_ms
                );
            }
            ClockStatus::Excluded => {
                error!(
                    "🚫 Time sync critical: {} → {} (drift: {:.0}ms) - Block production SUSPENDED",
                    old, new, self.ema_drift_ms
                );
            }
            ClockStatus::Recovering => {
                info!(
                    "↗️ Time sync recovering: {} → {} (drift: {:.0}ms) - Observation period",
                    old, new, self.ema_drift_ms
                );
            }
        }

        // Update status to new value
        self.status = new;

        // Update status timestamp
        self.status_since = now;
        self.improvement_since = None;
    }

    /// Update emergency mode based on network statistics
    pub fn update_emergency_mode(&mut self, degraded_ratio: f64) {
        let now = chrono::Utc::now().timestamp() as u64;

        if !self.emergency_mode && degraded_ratio > EMERGENCY_THRESHOLD_RATIO {
            self.emergency_mode = true;
            self.emergency_recovery_since = None;
            warn!(
                "⚠️ Emergency threshold expansion ACTIVATED: {:.1}% validators degraded/excluded",
                degraded_ratio * 100.0
            );
        } else if self.emergency_mode && degraded_ratio < EMERGENCY_RECOVERY_RATIO {
            // Check if we've sustained recovery for long enough
            if let Some(since) = self.emergency_recovery_since {
                if now.saturating_sub(since) >= EMERGENCY_RECOVERY_SUSTAIN_SECS {
                    self.emergency_mode = false;
                    self.emergency_recovery_since = None;
                    info!(
                        "Emergency threshold expansion DEACTIVATED: {:.1}% validators degraded/excluded",
                        degraded_ratio * 100.0
                    );
                }
            } else {
                self.emergency_recovery_since = Some(now);
            }
        } else if self.emergency_mode && degraded_ratio >= EMERGENCY_RECOVERY_RATIO {
            // Reset recovery timer if ratio goes back up
            self.emergency_recovery_since = None;
        }
    }

    /// Get current status
    pub fn status(&self) -> ClockStatus {
        self.status
    }

    /// Get current EMA drift in milliseconds
    pub fn ema_drift_ms(&self) -> f64 {
        self.ema_drift_ms
    }

    /// Check if block production is allowed
    pub fn can_produce_blocks(&self) -> bool {
        self.status.can_produce_blocks()
    }

    /// Check if in emergency mode
    pub fn is_emergency_mode(&self) -> bool {
        self.emergency_mode
    }

    /// Get priority modifier for block production
    /// Returns 1.0 for normal priority, 0.5 for degraded (end of pool)
    pub fn priority_modifier(&self) -> f64 {
        match self.status {
            ClockStatus::Healthy => 1.0,
            ClockStatus::Degraded => 0.5,
            ClockStatus::Excluded | ClockStatus::Recovering => 0.0,
        }
    }
}

// =============================================================================
// VALIDATOR CLOCK RECORD (Consensus State)
// =============================================================================

/// Validator clock record - stored in StateBackend (consensus)
///
/// SPEC v6.1 Section 4: This records the consequences of clock issues
/// for VC impact calculation, shared across all nodes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidatorClockRecord {
    /// Number of clock sync failures (impacts VC)
    pub clock_sync_failures: u32,

    /// Last epoch when validator was excluded for clock issues
    pub last_exclusion_epoch: Option<EpochNumber>,

    /// Total slots missed due to clock exclusion
    pub total_excluded_slots: u64,
}

impl ValidatorClockRecord {
    /// Create a new empty record
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a clock sync failure (transition to Excluded)
    pub fn record_failure(&mut self, epoch: EpochNumber) {
        self.clock_sync_failures += 1;
        self.last_exclusion_epoch = Some(epoch);
    }

    /// Record a missed slot due to exclusion
    pub fn record_missed_slot(&mut self) {
        self.total_excluded_slots += 1;
    }

    /// Calculate VC penalty from clock failures
    pub fn vc_penalty(&self) -> u64 {
        self.clock_sync_failures as u64 * CLOCK_FAILURE_VC_PENALTY
    }
}

// =============================================================================
// NETWORK CLOCK STATUS
// =============================================================================

/// Network-wide clock health statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkClockStatus {
    /// Count of validators in Healthy state
    pub healthy_count: u32,

    /// Count of validators in Degraded state
    pub degraded_count: u32,

    /// Count of validators in Excluded state
    pub excluded_count: u32,

    /// Count of validators in Recovering state
    pub recovering_count: u32,

    /// Total active validators
    pub total_validators: u32,

    /// Is emergency mode active network-wide?
    pub is_emergency_mode: bool,

    /// Network average drift (milliseconds)
    pub network_avg_drift_ms: f64,
}

impl NetworkClockStatus {
    /// Calculate the ratio of degraded/excluded validators
    pub fn degraded_ratio(&self) -> f64 {
        if self.total_validators == 0 {
            return 0.0;
        }
        (self.degraded_count + self.excluded_count) as f64 / self.total_validators as f64
    }
}

// =============================================================================
// ERRORS
// =============================================================================

/// Clock health errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ClockHealthError {
    #[error("Failed to serialize clock health: {0}")]
    SerializationFailed(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Validator not found: {0:?}")]
    ValidatorNotFound(AccountId),
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_transitions_healthy_to_degraded() {
        let mut health = LocalClockHealth::new();
        assert_eq!(health.status(), ClockStatus::Healthy);

        // Record drift above healthy threshold (1000ms)
        // Directly set EMA to test state machine transition
        // This bypasses the slow EMA convergence which is tested separately
        health.ema_drift_ms = 1100.0;
        health.record_drift(1100);

        assert_eq!(health.status(), ClockStatus::Degraded);
    }

    #[test]
    fn test_status_transitions_degraded_to_excluded() {
        let mut health = LocalClockHealth::new();
        // Pre-set the EMA to degraded level to test transition
        health.ema_drift_ms = 1500.0;
        health.status = ClockStatus::Degraded;
        health.status_since = chrono::Utc::now().timestamp() as u64;

        // Record drift above excluded threshold (1500ms)
        // EMA is already at 1500, so just need to push it slightly higher
        health.record_drift(1600);

        assert_eq!(health.status(), ClockStatus::Excluded);
        assert_eq!(health.lifetime_excluded_count, 1);
    }

    #[test]
    fn test_ema_smoothing() {
        let mut health = LocalClockHealth::new();

        // Spike in drift
        health.record_drift(2000);
        let after_spike = health.ema_drift_ms();

        // EMA should smooth the spike (not jump to 2000)
        assert!(after_spike < 2000.0);
        assert!(after_spike > 0.0);

        // Gradual return to low drift
        for _ in 0..50 {
            health.record_drift(100);
        }

        // EMA should approach 100
        assert!(health.ema_drift_ms() < 500.0);
    }

    #[test]
    fn test_can_produce_blocks() {
        assert!(ClockStatus::Healthy.can_produce_blocks());
        assert!(ClockStatus::Degraded.can_produce_blocks());
        assert!(!ClockStatus::Excluded.can_produce_blocks());
        assert!(!ClockStatus::Recovering.can_produce_blocks());
    }

    #[test]
    fn test_validator_clock_record() {
        let mut record = ValidatorClockRecord::new();

        record.record_failure(10);
        assert_eq!(record.clock_sync_failures, 1);
        assert_eq!(record.last_exclusion_epoch, Some(10));

        record.record_missed_slot();
        record.record_missed_slot();
        assert_eq!(record.total_excluded_slots, 2);

        assert_eq!(record.vc_penalty(), 1);
    }

    #[test]
    fn test_priority_modifier() {
        let mut health = LocalClockHealth::new();
        assert_eq!(health.priority_modifier(), 1.0);

        health.status = ClockStatus::Degraded;
        assert_eq!(health.priority_modifier(), 0.5);

        health.status = ClockStatus::Excluded;
        assert_eq!(health.priority_modifier(), 0.0);
    }

    #[test]
    fn test_emergency_mode_activation() {
        let mut health = LocalClockHealth::new();
        assert!(!health.is_emergency_mode());

        // Activate emergency mode
        health.update_emergency_mode(0.35); // 35% > 30%
        assert!(health.is_emergency_mode());
    }
}
