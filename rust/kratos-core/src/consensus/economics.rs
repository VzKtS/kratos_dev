// Economics - Bootstrap era and adaptive inflation
// Principle: Early flexibility, long-term sobriety, recoverable failure

use crate::types::{Balance, EpochNumber, KRAT};
use serde::{Deserialize, Serialize};

// =============================================================================
// UNIFIED CONFIGURATION - No dev/testnet/mainnet distinction
// =============================================================================

/// Get the bootstrap config - single unified configuration
pub fn get_bootstrap_config() -> BootstrapConfig {
    BootstrapConfig::default_config()
}

// =============================================================================
// NETWORK SECURITY STATE - Validator Population Safety & Degradation Handling
// SPEC v7.1: Validator Population Safety, Bootstrap Exit & Network Degradation
// =============================================================================

// --- SPEC v7.1 §2: Canonical Thresholds ---

/// BootstrapMinValidators: Minimum validators during bootstrap
/// SPEC v7.1 §2.1: BootstrapMinValidators = 1
pub const BOOTSTRAP_MIN_VALIDATORS: u32 = 1;

/// PostBootstrapMinValidators: Minimum validators after bootstrap exit
/// SPEC v7.1 §2.1: PostBootstrapMinValidators = 50
pub const POST_BOOTSTRAP_MIN_VALIDATORS: u32 = 50;

/// SafeValidators: Threshold for Normal state (above this = safe)
/// SPEC v7.1 §2.1: SafeValidators = 75
pub const SAFE_VALIDATORS: u32 = 75;

/// OptimalValidators: Target validator set size
/// SPEC v7.1 §2.1: OptimalValidators = 101
pub const OPTIMAL_VALIDATORS: u32 = 101;

/// EmergencyValidators: Threshold for automatic Emergency state
/// SPEC v7.1 §2.1: EmergencyValidators = 25
pub const EMERGENCY_VALIDATORS: u32 = 25;

// Legacy aliases for backward compatibility with v6.x
// FIX: Consolidated and documented validator thresholds to prevent inconsistencies
//
// Canonical thresholds (SPEC v7.1):
// - EMERGENCY_VALIDATORS = 25 (auto-emergency trigger)
// - POST_BOOTSTRAP_MIN_VALIDATORS = 50 (minimum after bootstrap)
// - SAFE_VALIDATORS = 75 (normal operation threshold)
// - OPTIMAL_VALIDATORS = 101 (target validator set size)
//
// NOTE: TARGET_VALIDATORS was incorrectly set to 1000 in older code.
// The correct target is OPTIMAL_VALIDATORS (101) per SPEC v7.1.
pub const V_MIN_ABSOLUTE: u32 = EMERGENCY_VALIDATORS; // 25
pub const V_MIN_OPERATIONAL: u32 = POST_BOOTSTRAP_MIN_VALIDATORS; // 50
pub const V_TARGET: u32 = OPTIMAL_VALIDATORS; // FIX: Changed from OPTIMAL_VALIDATORS - 1 to OPTIMAL_VALIDATORS
pub const V_ABS: u32 = V_MIN_ABSOLUTE; // 25
pub const V_MIN: u32 = V_MIN_OPERATIONAL; // 50
pub const V_OPT: u32 = OPTIMAL_VALIDATORS; // 101
pub const MIN_VALIDATORS: u32 = V_MIN_OPERATIONAL; // 50
/// FIX: Changed from 1000 to OPTIMAL_VALIDATORS for consistency with SPEC v7.1
/// Use OPTIMAL_VALIDATORS (101) as the target, not an arbitrary 1000
#[deprecated(since = "0.2.0", note = "Use OPTIMAL_VALIDATORS instead for consistency")]
pub const TARGET_VALIDATORS: u32 = OPTIMAL_VALIDATORS;
pub const CRITICAL_VALIDATORS: u32 = V_MIN_ABSOLUTE; // 25

/// Minimum bootstrap epochs before exit can be considered
/// SPEC v2.3: Bootstrap exit requires epoch ≥ 1440 (60 days at 1h/epoch)
pub const BOOTSTRAP_EPOCHS_MIN: EpochNumber = 1440;

/// Minimum average participation for bootstrap exit
/// SPEC v7.1 §3.2: average_participation >= 90% (last 100 epochs)
pub const MIN_PARTICIPATION_PERCENT: u32 = 90;

/// Window for calculating average participation
/// SPEC v7.1 §3.2: Calculated over last 100 epochs
pub const PARTICIPATION_WINDOW: EpochNumber = 100;

// Backward compatibility - kept for older spec compatibility
pub const BOOTSTRAP_EXTENSION_STEP: EpochNumber = 90;
pub const MIN_FINALITY_RATE_PERCENT: u32 = 95;
pub const FINALITY_RATE_WINDOW: EpochNumber = 100;

/// Consecutive epochs at SafeValidators to return to Normal
/// SPEC v7.1 §7.1: 100 consecutive epochs at ≥75 validators
pub const NORMAL_RECOVERY_EPOCHS: EpochNumber = 100;

/// Consecutive epochs below PostBootstrapMinValidators to trigger collapse detection
/// SPEC v7.1 §4.1: 10 consecutive epochs triggers Validator Population Collapse
pub const COLLAPSE_DETECTION_EPOCHS: EpochNumber = 10;

/// Epochs without finality to enter Terminal state
/// SPEC v7.1: Retained from v6.5 for terminal transition
pub const TERMINAL_NO_FINALITY_EPOCHS: EpochNumber = 24;

// Legacy aliases for backward compatibility
pub const DEGRADED_RECOVERY_EPOCHS: EpochNumber = NORMAL_RECOVERY_EPOCHS;
pub const DSM_RECOVERY_EPOCHS: EpochNumber = NORMAL_RECOVERY_EPOCHS;
pub const RECOVERY_EPOCHS: EpochNumber = NORMAL_RECOVERY_EPOCHS;
pub const BOOTSTRAP_REENTRY_EPOCHS: EpochNumber = COLLAPSE_DETECTION_EPOCHS;

/// Global network security state
///
/// SPEC v7.1: Validator Population Safety, Bootstrap Exit & Network Degradation
///
/// This enum represents the security states of the network per SPEC v7.1 §5:
/// - Bootstrap: Initial phase, building validator set
/// - Normal: Full functionality, V_active ≥ SafeValidators (75)
/// - Degraded: Reduced security, SafeValidators > V_active ≥ PostBootstrapMin (50-74)
/// - Restricted: Critical security, PostBootstrapMin > V_active ≥ EmergencyValidators (25-49)
/// - Emergency: Automatic emergency, V_active < EmergencyValidators (< 25)
///
/// State Machine (SPEC v7.1 §5.1):
/// ```text
/// Bootstrap
///   ↓ (Epoch ≥ 1440 AND Validators ≥ 50 AND Participation ≥ 90%)
/// Normal
///   ↓ (validators < 75)
/// Degraded
///   ↓ (validators < 50)
/// Restricted
///   ↓ (validators < 25)
/// Emergency
/// ```
///
/// INVARIANTS (SPEC v7.1 §9):
/// 1. No insecure bootstrap exit
/// 2. No silent validator collapse
/// 3. Automatic security degradation
/// 4. Emergency without capture
/// 5. Exit without permission
/// 6. Fork without punishment
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkSecurityState {
    /// Initial bootstrap phase - building validator set
    /// SPEC v2.3: Bootstrap exits when epoch ≥ 1440 AND validators ≥ 50 AND participation ≥ 90%
    Bootstrap,

    /// Normal operation - sufficient validators for full security
    /// SPEC v7.1 §5.2: V_active ≥ SafeValidators (75)
    /// Effects: Full functionality, Normal inflation, Governance enabled
    Normal,

    /// Degraded state - reduced validator set but above minimum
    /// SPEC v7.1 §5.2: PostBootstrapMin ≤ V_active < SafeValidators (50-74)
    /// Effects: Inflation +1%, Governance timelocks ×2, New sidechains paused
    /// Aliased as "DegradedSecurityMode" for backward compatibility
    DegradedSecurityMode {
        /// Epoch when Degraded was entered
        entered_at: EpochNumber,
        /// Number of consecutive epochs in Degraded
        epochs_in_dsm: EpochNumber,
        /// Current validator count
        current_validators: u32,
        /// Validators needed to exit Degraded (SafeValidators - current)
        validators_needed: u32,
        /// Consecutive epochs at SafeValidators for recovery
        /// SPEC v7.1 §7.1: Requires 100 consecutive epochs at ≥75
        consecutive_epochs_above_safe: EpochNumber,
    },

    /// Restricted state - below minimum but above emergency
    /// SPEC v7.1 §5.2: EmergencyValidators ≤ V_active < PostBootstrapMin (25-49)
    /// Effects: Governance frozen, Validator entry incentives boosted, Emergency circuit breakers armed
    /// Aliased as "SafetyHaltMode" for backward compatibility
    SafetyHaltMode {
        /// Epoch when Restricted was entered
        entered_at: EpochNumber,
        /// Number of consecutive epochs in Restricted
        epochs_in_shm: EpochNumber,
        /// Current validator count
        current_validators: u32,
        /// Epochs without finality (for Terminal transition)
        epochs_without_finality: EpochNumber,
    },

    /// Emergency state - automatic trigger, no governance vote required
    /// SPEC v7.1 §5.2 & §6.1: V_active < EmergencyValidators (< 25)
    /// Effects: SPEC v7 Emergency automatically triggered, Fork declaration allowed, Asset exit ALWAYS permitted
    /// Note: Replaces "TerminalMode" from v6.5 - Emergency is the severe state
    TerminalMode {
        /// Epoch when Emergency was entered
        entered_at: EpochNumber,
        /// Number of epochs in Emergency
        epochs_in_terminal: EpochNumber,
        /// Current validator count
        current_validators: u32,
        /// State root at emergency entry (for fork continuity)
        terminal_state_root: Option<[u8; 32]>,
    },

    /// Bootstrap Recovery Mode - extended degraded state with bootstrap incentives
    /// SPEC v7.1 §4.1: Triggered after 10 consecutive epochs below PostBootstrapMin
    /// Effects: Bootstrap economics re-enabled, 100% inflation to validators
    BootstrapRecoveryMode {
        /// Epoch when recovery mode was entered
        entered_at: EpochNumber,
        /// Number of consecutive epochs in recovery
        epochs_in_recovery: EpochNumber,
        /// Current validator count
        current_validators: u32,
        /// Validators needed to exit recovery
        validators_needed: u32,
    },
}

// Backward compatibility: map old variant names
impl NetworkSecurityState {
    /// Backward compat: check if ValidatorDegraded (now DegradedSecurityMode)
    pub fn is_validator_degraded_variant(&self) -> bool {
        matches!(self, NetworkSecurityState::DegradedSecurityMode { .. })
    }

    /// Backward compat: check if SurvivalMode (now SafetyHaltMode/Restricted)
    pub fn is_survival_mode_variant(&self) -> bool {
        matches!(self, NetworkSecurityState::SafetyHaltMode { .. })
    }

    /// SPEC v7.1: Check if in Emergency state (replaces Terminal)
    pub fn is_terminal_variant(&self) -> bool {
        matches!(self, NetworkSecurityState::TerminalMode { .. })
    }

    /// SPEC v7.1: Check if in Restricted state (alias for SafetyHaltMode)
    pub fn is_restricted_variant(&self) -> bool {
        matches!(self, NetworkSecurityState::SafetyHaltMode { .. })
    }

    /// SPEC v7.1: Check if in Normal state (V_active ≥ SafeValidators)
    pub fn is_normal_variant(&self) -> bool {
        matches!(self, NetworkSecurityState::Normal)
    }

    /// SPEC v7.1: Check if in Degraded state (alias for DegradedSecurityMode)
    pub fn is_degraded_variant(&self) -> bool {
        matches!(self, NetworkSecurityState::DegradedSecurityMode { .. })
    }

    /// SPEC v7.1: Check if in Emergency state (V_active < EmergencyValidators)
    pub fn is_emergency_variant(&self) -> bool {
        matches!(self, NetworkSecurityState::TerminalMode { .. })
    }

    // Backward compatibility aliases for v6.x
    /// Backward compat: is_critical_variant (now is_restricted_variant)
    pub fn is_critical_variant(&self) -> bool {
        self.is_restricted_variant()
    }

    /// Backward compat: is_healthy_variant (now is_normal_variant)
    pub fn is_healthy_variant(&self) -> bool {
        self.is_normal_variant()
    }
}

// Backward compatibility aliases
pub type ValidatorScarcityState = NetworkSecurityState;
pub type EmergencySafetyMode = NetworkSecurityState;

// Backward compatibility aliases
pub type SafetyMode = NetworkSecurityState;
pub type BootstrapRecovery = NetworkSecurityState;
// Keep VSS_RECOVERY_EPOCHS for backward compat
pub const VSS_RECOVERY_EPOCHS: EpochNumber = RECOVERY_EPOCHS;

/// Configuration for network security states
/// SPEC v7.1: Validator Population Safety, Bootstrap Exit & Network Degradation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradedSecurityConfig {
    // --- SPEC v7.1 §5.2: Degraded State Effects (50-74 validators) ---

    /// Inflation boost in Degraded state
    /// SPEC v7.1 §5.2: Inflation +1%
    pub degraded_inflation_boost_percent: u32,

    /// Governance timelock multiplier in Degraded state
    /// SPEC v7.1 §5.2: Governance timelocks ×2
    pub degraded_governance_timelock_multiplier: u32,

    /// New sidechains paused in Degraded state
    /// SPEC v7.1 §5.2: New sidechains paused
    pub degraded_sidechains_paused: bool,

    // --- SPEC v7.1 §5.2: Restricted State Effects (25-49 validators) ---

    /// Governance frozen in Restricted state
    /// SPEC v7.1 §5.2: Governance frozen
    pub restricted_governance_frozen: bool,

    /// Validator entry incentives boosted in Restricted state
    /// SPEC v7.1 §5.2: Validator entry incentives boosted
    pub restricted_validator_incentives_boosted: bool,

    /// Emergency circuit breakers armed in Restricted state
    /// SPEC v7.1 §5.2: Emergency circuit breakers armed
    pub restricted_emergency_armed: bool,

    // --- SPEC v7.1 §5.2 & §6: Emergency State Effects (< 25 validators) ---

    /// SPEC v7 Emergency automatically triggered
    /// SPEC v7.1 §6.1: Automatic emergency trigger, no governance vote
    pub emergency_auto_trigger: bool,

    /// Fork declaration allowed in Emergency state
    /// SPEC v7.1 §6.2: Fork declaration allowed
    pub emergency_fork_allowed: bool,

    /// Asset exit ALWAYS permitted in Emergency state
    /// SPEC v7.1 §6.2: No asset lock-in
    pub emergency_exit_always_allowed: bool,

    /// No slashing escalation in Emergency state
    /// SPEC v7.1 §6.2: No slashing escalation
    pub emergency_no_slashing_escalation: bool,

    /// No identity freezing in Emergency state
    /// SPEC v7.1 §6.2: No identity freezing
    pub emergency_no_identity_freeze: bool,

    /// No fork suppression in Emergency state
    /// SPEC v7.1 §6.2: No fork suppression
    pub emergency_no_fork_suppression: bool,

    // --- SPEC v7.1 §7: Recovery Conditions ---

    /// Consecutive epochs at SafeValidators (75) to return to Normal
    /// SPEC v7.1 §7.1: 100 consecutive epochs at ≥75 validators
    pub normal_recovery_epochs: EpochNumber,

    // --- SPEC v7.1 §4: Collapse Detection ---

    /// Consecutive epochs below PostBootstrapMin to trigger collapse detection
    /// SPEC v7.1 §4.1: 10 consecutive epochs
    pub collapse_detection_epochs: EpochNumber,

    // --- SPEC v7.1 §3: Bootstrap Exit ---

    /// Minimum average participation for bootstrap exit
    /// SPEC v7.1 §3.2: 90% average participation (last 100 epochs)
    pub min_participation_percent: u32,

    /// Window for calculating average participation
    /// SPEC v7.1 §3.2: 100 epochs
    pub participation_window: EpochNumber,

    // --- Backward compatibility with v6.x ---

    /// Block time multiplier in Degraded state (legacy)
    pub dsm_block_time_multiplier: u32,
    /// Epoch duration multiplier in Degraded state (legacy)
    pub dsm_epoch_duration_multiplier: u32,
    /// Governance timelock multiplier (alias for degraded_governance_timelock_multiplier)
    pub dsm_governance_timelock_multiplier: u32,
    /// Slashing thresholds tightened in Degraded state (legacy)
    pub dsm_slashing_tightened: bool,
    /// Inflation capped in Degraded state (legacy - now boosted)
    pub dsm_inflation_capped: bool,
    /// Emergency disabled in Degraded state (legacy)
    pub dsm_emergency_disabled: bool,
    /// Block time multiplier in Restricted state (legacy: critical)
    pub critical_block_time_multiplier: u32,
    /// Governance frozen in Restricted state (alias)
    pub critical_governance_frozen: bool,
    /// Fork enabled in Restricted state (legacy)
    pub critical_fork_enabled: bool,
    /// Fork threshold reduction percent (legacy)
    pub critical_fork_threshold_reduction_percent: u32,
    /// No slashing in Restricted state (legacy)
    pub shm_no_slashing: bool,
    /// Exits allowed in Restricted state (legacy)
    pub shm_exits_allowed: bool,
    /// Fork allowed in Restricted state (legacy)
    pub shm_fork_allowed: bool,
    /// Epochs without finality to enter Emergency (legacy: terminal)
    pub terminal_no_finality_epochs: EpochNumber,
    /// Read-only in Emergency (legacy)
    pub terminal_read_only: bool,
    /// Fork mandatory in Emergency (legacy)
    pub terminal_fork_mandatory: bool,
    /// Fork threshold reduction in Emergency (legacy)
    pub terminal_fork_threshold_reduction_percent: u32,
    /// Recovery epochs for Degraded (alias for normal_recovery_epochs)
    pub dsm_recovery_epochs: EpochNumber,
    /// Recovery epochs for Restricted state (legacy)
    pub shm_recovery_epochs: EpochNumber,
    /// Bootstrap reentry epochs (alias for collapse_detection_epochs)
    pub bootstrap_reentry_epochs: EpochNumber,
    /// Stake reduction percent (legacy)
    pub dsm_stake_reduction_percent: u32,
    /// Inflation redirect to validators (legacy)
    pub inflation_redirect_to_validators_percent: u32,
    /// Grace period before entering Degraded (legacy)
    pub floor_grace_epochs: EpochNumber,
    /// Block halted in Restricted (legacy - unused in v7.1)
    pub shm_block_halted: bool,
    /// No governance in Restricted (legacy - use restricted_governance_frozen)
    pub shm_no_governance: bool,
}

impl Default for DegradedSecurityConfig {
    /// Default configuration per SPEC v7.1
    fn default() -> Self {
        Self {
            // SPEC v7.1 §5.2: Degraded State Effects (50-74 validators)
            degraded_inflation_boost_percent: 1, // +1% inflation
            degraded_governance_timelock_multiplier: 2, // ×2 timelocks
            degraded_sidechains_paused: true, // New sidechains paused

            // SPEC v7.1 §5.2: Restricted State Effects (25-49 validators)
            restricted_governance_frozen: true, // Governance frozen
            restricted_validator_incentives_boosted: true, // Incentives boosted
            restricted_emergency_armed: true, // Emergency circuit breakers armed

            // SPEC v7.1 §5.2 & §6: Emergency State Effects (< 25 validators)
            emergency_auto_trigger: true, // SPEC v7 Emergency auto-triggered
            emergency_fork_allowed: true, // Fork declaration allowed
            emergency_exit_always_allowed: true, // Asset exit ALWAYS permitted
            emergency_no_slashing_escalation: true, // No slashing escalation
            emergency_no_identity_freeze: true, // No identity freezing
            emergency_no_fork_suppression: true, // No fork suppression

            // SPEC v7.1 §7: Recovery Conditions
            normal_recovery_epochs: NORMAL_RECOVERY_EPOCHS, // 100 epochs at ≥75 validators

            // SPEC v7.1 §4: Collapse Detection
            collapse_detection_epochs: COLLAPSE_DETECTION_EPOCHS, // 10 epochs

            // SPEC v7.1 §3: Bootstrap Exit
            min_participation_percent: MIN_PARTICIPATION_PERCENT, // 90%
            participation_window: PARTICIPATION_WINDOW, // 100 epochs

            // Backward compatibility with v6.x
            dsm_block_time_multiplier: 2,
            dsm_epoch_duration_multiplier: 2,
            dsm_governance_timelock_multiplier: 2,
            dsm_slashing_tightened: true,
            dsm_inflation_capped: false, // Changed: inflation is BOOSTED in v7.1
            dsm_emergency_disabled: true,
            critical_block_time_multiplier: 4,
            critical_governance_frozen: true,
            critical_fork_enabled: true,
            critical_fork_threshold_reduction_percent: 25,
            shm_no_slashing: true,
            shm_exits_allowed: true,
            shm_fork_allowed: true,
            terminal_no_finality_epochs: TERMINAL_NO_FINALITY_EPOCHS,
            terminal_read_only: true,
            terminal_fork_mandatory: true,
            terminal_fork_threshold_reduction_percent: 25,
            dsm_recovery_epochs: NORMAL_RECOVERY_EPOCHS, // 100 epochs (v7.1)
            shm_recovery_epochs: 1,
            bootstrap_reentry_epochs: COLLAPSE_DETECTION_EPOCHS,
            dsm_stake_reduction_percent: 30,
            inflation_redirect_to_validators_percent: 100,
            floor_grace_epochs: 1,
            shm_block_halted: false,
            shm_no_governance: true,
        }
    }
}

impl DegradedSecurityConfig {
    /// Create a lenient config for testing
    pub fn lenient() -> Self {
        Self {
            // SPEC v7.1 §5.2: Degraded State (lenient)
            degraded_inflation_boost_percent: 0, // No boost
            degraded_governance_timelock_multiplier: 1, // No extra timelock
            degraded_sidechains_paused: false, // Allow sidechains

            // SPEC v7.1 §5.2: Restricted State (lenient)
            restricted_governance_frozen: false, // Allow governance
            restricted_validator_incentives_boosted: true,
            restricted_emergency_armed: false, // Not armed

            // SPEC v7.1 §6: Emergency State (lenient)
            emergency_auto_trigger: true, // Always auto-trigger
            emergency_fork_allowed: true,
            emergency_exit_always_allowed: true,
            emergency_no_slashing_escalation: true,
            emergency_no_identity_freeze: true,
            emergency_no_fork_suppression: true,

            // Recovery (lenient - faster)
            normal_recovery_epochs: 20, // Faster recovery for testing
            collapse_detection_epochs: 3,
            min_participation_percent: 80, // Lower threshold
            participation_window: 50,

            // Backward compat (lenient)
            dsm_block_time_multiplier: 1,
            dsm_epoch_duration_multiplier: 1,
            dsm_governance_timelock_multiplier: 1,
            dsm_slashing_tightened: false,
            dsm_inflation_capped: false,
            dsm_emergency_disabled: false,
            critical_block_time_multiplier: 2,
            critical_governance_frozen: false,
            critical_fork_enabled: true,
            critical_fork_threshold_reduction_percent: 10,
            shm_no_slashing: true,
            shm_exits_allowed: true,
            shm_fork_allowed: true,
            terminal_no_finality_epochs: 12,
            terminal_read_only: true,
            terminal_fork_mandatory: true,
            terminal_fork_threshold_reduction_percent: 10,
            dsm_recovery_epochs: 20,
            shm_recovery_epochs: 1,
            bootstrap_reentry_epochs: 3,
            dsm_stake_reduction_percent: 20,
            inflation_redirect_to_validators_percent: 100,
            floor_grace_epochs: 1,
            shm_block_halted: false,
            shm_no_governance: false,
        }
    }

    /// Create a strict config for high-security networks
    pub fn strict() -> Self {
        Self {
            // SPEC v7.1 §5.2: Degraded State (strict)
            degraded_inflation_boost_percent: 2, // +2% inflation
            degraded_governance_timelock_multiplier: 3, // ×3 timelocks
            degraded_sidechains_paused: true,

            // SPEC v7.1 §5.2: Restricted State (strict)
            restricted_governance_frozen: true,
            restricted_validator_incentives_boosted: true,
            restricted_emergency_armed: true,

            // SPEC v7.1 §6: Emergency State (strict)
            emergency_auto_trigger: true,
            emergency_fork_allowed: true,
            emergency_exit_always_allowed: true,
            emergency_no_slashing_escalation: true,
            emergency_no_identity_freeze: true,
            emergency_no_fork_suppression: true,

            // Recovery (strict - slower)
            normal_recovery_epochs: 150, // More conservative
            collapse_detection_epochs: 5,
            min_participation_percent: 95, // Higher threshold
            participation_window: 100,

            // Backward compat (strict)
            dsm_block_time_multiplier: 3,
            dsm_epoch_duration_multiplier: 3,
            dsm_governance_timelock_multiplier: 3,
            dsm_slashing_tightened: true,
            dsm_inflation_capped: false, // v7.1: inflation BOOSTED
            dsm_emergency_disabled: true,
            critical_block_time_multiplier: 6,
            critical_governance_frozen: true,
            critical_fork_enabled: true,
            critical_fork_threshold_reduction_percent: 30,
            shm_no_slashing: true,
            shm_exits_allowed: true,
            shm_fork_allowed: true,
            terminal_no_finality_epochs: 12,
            terminal_read_only: true,
            terminal_fork_mandatory: true,
            terminal_fork_threshold_reduction_percent: 30,
            dsm_recovery_epochs: 150,
            shm_recovery_epochs: 2,
            bootstrap_reentry_epochs: 5,
            dsm_stake_reduction_percent: 40,
            inflation_redirect_to_validators_percent: 100,
            floor_grace_epochs: 1,
            shm_block_halted: false,
            shm_no_governance: true,
        }
    }

    // SPEC v7.1 methods

    /// Get inflation boost percentage in Degraded state
    /// SPEC v7.1 §5.2: +1% inflation in Degraded
    pub fn inflation_boost(&self) -> f64 {
        self.degraded_inflation_boost_percent as f64 / 100.0
    }

    /// Get validator rewards multiplier (1.0 base + boost in Restricted)
    pub fn rewards_multiplier(&self) -> f64 {
        if self.restricted_validator_incentives_boosted {
            1.25 // +25% in Restricted state
        } else {
            1.0
        }
    }

    /// Check if emergency should auto-trigger
    /// SPEC v7.1 §6.1: Automatic emergency trigger when validators < 25
    pub fn should_auto_trigger_emergency(&self, active_validators: u32) -> bool {
        self.emergency_auto_trigger && active_validators < EMERGENCY_VALIDATORS
    }
}

// Backward compatibility aliases
pub type ValidatorDegradedConfig = DegradedSecurityConfig;
pub type ValidatorScarcityConfig = DegradedSecurityConfig;

/// Network health status for RPC/signaling
/// SPEC v6.4 §10: Implementation Notes (Events)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkHealthStatus {
    /// Network is fully healthy (Normal state)
    /// SPEC v6.4 §4.1: V_active ≥ V_min_operational
    Healthy,

    /// Network is in bootstrap phase
    /// SPEC v6.4 §6.1: Building validator set
    Bootstrapping {
        validators_current: u32,
        validators_needed: u32,
        epochs_elapsed: EpochNumber,
        epochs_remaining: Option<EpochNumber>,
    },

    /// Network is in Degraded Security Mode (DSM)
    /// SPEC v6.4 §4.2: V_min_absolute ≤ V_active < V_min_operational
    DegradedSecurityMode {
        severity: SecuritySeverity,
        validators_current: u32,
        validators_needed: u32,
        epochs_in_state: EpochNumber,
        recovery_progress: (EpochNumber, EpochNumber), // (current, required)
        effects_active: Vec<String>,
    },

    /// Network is recovering from DSM
    /// SPEC v6.4 §5.2: Counting consecutive epochs
    Recovering {
        epochs_until_normal: EpochNumber,
        epochs_consecutive: EpochNumber,
        validators_current: u32,
    },

    /// Network is in Safety Halt Mode (SHM)
    /// SPEC v6.4 §4.3: V_active < V_min_absolute (21)
    /// Block production halted, state readable, funds not frozen
    SafetyHalt {
        epochs_in_shm: EpochNumber,
        validators_current: u32,
        absolute_minimum: u32,
        fork_allowed: bool,
        exit_allowed: bool,
    },

    /// Network is in Bootstrap Recovery Mode
    /// SPEC v6.5 §6.2: Re-entry after prolonged validator shortage
    BootstrapRecovery {
        epochs_in_recovery: EpochNumber,
        validators_current: u32,
        validators_needed: u32,
        incentives_active: Vec<String>,
    },

    /// Network is in Critical state
    /// SPEC v6.5 §4.3: V_active < V_min_absolute (< 21)
    /// Block time ×4, Governance frozen, Fork enabled
    Critical {
        epochs_in_critical: EpochNumber,
        validators_current: u32,
        absolute_minimum: u32,
        fork_enabled: bool,
        fork_threshold_reduction: u32,
        epochs_without_finality: EpochNumber,
    },

    /// Network is in Terminal state
    /// SPEC v6.5 §4.4: No finality for 24+ epochs
    /// Chain read-only, Fork mandatory
    Terminal {
        epochs_in_terminal: EpochNumber,
        validators_current: u32,
        fork_mandatory: bool,
        state_snapshot_finalized: bool,
    },
}

// Backward compatibility aliases for health status
impl NetworkHealthStatus {
    /// Create ValidatorScarcity variant (backward compat alias for DSM)
    pub fn validator_scarcity(
        severity: SecuritySeverity,
        validators_current: u32,
        validators_needed: u32,
        epochs_in_state: EpochNumber,
        recovery_progress: (EpochNumber, EpochNumber),
        effects_active: Vec<String>,
    ) -> Self {
        NetworkHealthStatus::DegradedSecurityMode {
            severity,
            validators_current,
            validators_needed,
            epochs_in_state,
            recovery_progress,
            effects_active,
        }
    }

    /// Create ValidatorDegraded variant (backward compat alias for DSM)
    pub fn validator_degraded(
        severity: SecuritySeverity,
        validators_current: u32,
        validators_needed: u32,
        epochs_in_state: EpochNumber,
        recovery_progress: (EpochNumber, EpochNumber),
        effects_active: Vec<String>,
    ) -> Self {
        NetworkHealthStatus::DegradedSecurityMode {
            severity,
            validators_current,
            validators_needed,
            epochs_in_state,
            recovery_progress,
            effects_active,
        }
    }

    /// Create Emergency variant (backward compat alias for SHM)
    pub fn emergency(
        epochs_in_emergency: EpochNumber,
        validators_current: u32,
        critical_threshold: u32,
        fork_allowed: bool,
    ) -> Self {
        NetworkHealthStatus::SafetyHalt {
            epochs_in_shm: epochs_in_emergency,
            validators_current,
            absolute_minimum: critical_threshold,
            fork_allowed,
            exit_allowed: true, // SPEC v6.4 §4.3: Exits still allowed
        }
    }

    /// Create Survival variant (backward compat alias for SHM)
    pub fn survival(
        epochs_in_survival: EpochNumber,
        validators_current: u32,
        absolute_minimum: u32,
        fork_allowed: bool,
        exit_allowed: bool,
    ) -> Self {
        NetworkHealthStatus::SafetyHalt {
            epochs_in_shm: epochs_in_survival,
            validators_current,
            absolute_minimum,
            fork_allowed,
            exit_allowed,
        }
    }
}

/// Severity levels for degraded states
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecuritySeverity {
    /// Warning: Just entered, recovery likely
    Warning,
    /// Elevated: Extended time in state
    Elevated,
    /// Critical: Very few validators, network at risk
    Critical,
    /// Emergency: Recovery failed, fork may be necessary
    Emergency,
}

/// Tracks the security state machine with hysteresis
/// SPEC v6.5: Validator Liveness Collapse, Consensus Survival & Anti-Fragility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityStateTracker {
    /// Current security state
    pub state: NetworkSecurityState,

    /// Configuration for security states
    pub config: DegradedSecurityConfig,

    /// Epochs consecutively below V_min_operational (for entering Degraded)
    pub epochs_below_minimum: EpochNumber,

    /// Epochs consecutively above V_min_operational while in Degraded (for recovery)
    /// SPEC v6.5 §5.2: Anti-false-recovery - 30 consecutive epochs
    pub epochs_above_minimum_in_dsm: EpochNumber,

    /// Whether bootstrap has ever completed
    pub bootstrap_completed: bool,

    /// V_min_operational: Healthy state threshold
    /// SPEC v6.5 §2.1: V_min_operational = 50
    pub min_validators: u32,

    /// V_min_absolute: Critical state threshold
    /// SPEC v6.5 §2.1: V_min_absolute = 21
    pub absolute_min_validators: u32,

    /// Whether Critical state is active (alias: shm_active)
    /// SPEC v6.5 §4.3: V_active < V_min_absolute
    pub shm_active: bool,

    /// Whether Terminal state is active
    /// SPEC v6.5 §4.4: No finality for 24+ epochs
    pub terminal_active: bool,

    /// Whether Bootstrap Recovery Mode is active
    /// SPEC v6.5 §6.2: After 10 epochs below V_min_operational
    pub bootstrap_recovery_active: bool,

    /// Kept for backward compatibility
    pub bootstrap_extensions: u32,

    /// Historical record of state transitions for auditing
    /// SPEC v6.5 §10: Events
    pub state_transitions: Vec<StateTransition>,
}

// Backward compatibility aliases
impl SecurityStateTracker {
    #[inline]
    pub fn epochs_above_minimum_in_vss(&self) -> EpochNumber {
        self.epochs_above_minimum_in_dsm
    }

    #[inline]
    pub fn epochs_above_minimum_in_degraded(&self) -> EpochNumber {
        self.epochs_above_minimum_in_dsm
    }

    #[inline]
    pub fn critical_validators(&self) -> u32 {
        self.absolute_min_validators
    }

    #[inline]
    pub fn emergency_active(&self) -> bool {
        self.shm_active
    }

    #[inline]
    pub fn survival_mode_active(&self) -> bool {
        self.shm_active
    }

    /// Check if in Terminal state
    /// SPEC v6.5 §4.4
    #[inline]
    pub fn terminal_mode_active(&self) -> bool {
        self.terminal_active
    }

    /// Check if Critical state (alias for shm_active)
    /// SPEC v6.5 §4.3
    #[inline]
    pub fn critical_state_active(&self) -> bool {
        self.shm_active
    }

    // SPEC v6.5 §4.2-4.4: Block time multipliers
    // Degraded: ×2, Critical: ×4, Terminal: 0 (read-only)
    #[inline]
    pub fn get_block_time_multiplier(&self) -> u32 {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode { .. } |
            NetworkSecurityState::BootstrapRecoveryMode { .. } => {
                self.config.dsm_block_time_multiplier // ×2
            }
            NetworkSecurityState::SafetyHaltMode { .. } => {
                // SPEC v6.5 §4.3: Critical state - block time ×4
                self.config.critical_block_time_multiplier
            }
            NetworkSecurityState::TerminalMode { .. } => {
                // SPEC v6.5 §4.4: Terminal - read-only, no new blocks
                0
            }
            _ => 1,
        }
    }

    // SPEC v6.5 §4.2-4.4: Governance timelocks
    // Degraded: ×2, Critical: frozen (0), Terminal: frozen (0)
    #[inline]
    pub fn get_governance_timelock_multiplier(&self) -> u32 {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode { .. } |
            NetworkSecurityState::BootstrapRecoveryMode { .. } => {
                self.config.dsm_governance_timelock_multiplier // ×2
            }
            NetworkSecurityState::SafetyHaltMode { .. } |
            NetworkSecurityState::TerminalMode { .. } => {
                // SPEC v6.5 §4.3-4.4: Governance frozen
                0
            }
            _ => 1,
        }
    }

    // Legacy: block time increase percent (now multiplier-based)
    #[inline]
    pub fn get_block_time_increase_percent(&self) -> u32 {
        (self.get_block_time_multiplier().saturating_sub(1)) * 100
    }

    // No validator reward boost in v6.4 (only in bootstrap recovery)
    #[inline]
    pub fn get_validator_reward_boost(&self) -> u32 {
        match &self.state {
            NetworkSecurityState::BootstrapRecoveryMode { .. } => 25, // SPEC v6.4 §7
            _ => 0,
        }
    }

    // SPEC v6.4 §4.3: No slashing in SHM
    #[inline]
    pub fn get_slashing_reduction(&self) -> u32 {
        match &self.state {
            NetworkSecurityState::SafetyHaltMode { .. } if self.config.shm_no_slashing => 100,
            NetworkSecurityState::DegradedSecurityMode { .. } if self.config.dsm_slashing_tightened => 0,
            _ => 0,
        }
    }

    // SPEC v6.4 §7: New validators prioritized in DSM/Bootstrap Recovery
    #[inline]
    pub fn is_fast_track_onboarding(&self) -> bool {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode { .. } |
            NetworkSecurityState::SafetyHaltMode { .. } |
            NetworkSecurityState::BootstrapRecoveryMode { .. } => true,
            _ => false,
        }
    }

    // SPEC v6.4 §4.2: Inflation capped in DSM
    #[inline]
    pub fn get_inflation_cap_increase_x10(&self) -> u32 {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode { .. } if self.config.dsm_inflation_capped => 0,
            NetworkSecurityState::BootstrapRecoveryMode { .. } => 10, // +1% in recovery
            _ => 0,
        }
    }

    // VC multiplier for bootstrap recovery
    #[inline]
    pub fn get_vc_multiplier(&self) -> f64 {
        match &self.state {
            NetworkSecurityState::BootstrapRecoveryMode { .. } => 2.0, // SPEC v6.4 §6.2
            _ => 1.0,
        }
    }

    // SPEC v7.1 §5.2/§6: No parameter changes in Degraded/Restricted/Emergency
    #[inline]
    pub fn is_inflation_change_allowed(&self) -> bool {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode { .. } |
            NetworkSecurityState::SafetyHaltMode { .. } |
            NetworkSecurityState::TerminalMode { .. } => false,
            _ => true,
        }
    }
}

/// Record of a state transition for auditing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub from_state: String,
    pub to_state: String,
    pub epoch: EpochNumber,
    pub validator_count: u32,
    pub reason: String,
}

impl SecurityStateTracker {
    /// Create a new tracker starting in Bootstrap state
    pub fn new(min_validators: u32, config: DegradedSecurityConfig) -> Self {
        Self {
            state: NetworkSecurityState::Bootstrap,
            config,
            epochs_below_minimum: 0,
            epochs_above_minimum_in_dsm: 0,
            bootstrap_completed: false,
            min_validators,
            absolute_min_validators: V_MIN_ABSOLUTE,
            shm_active: false,
            terminal_active: false,
            bootstrap_recovery_active: false,
            bootstrap_extensions: 0,
            state_transitions: Vec::new(),
        }
    }

    /// Create a new tracker with custom absolute minimum threshold
    pub fn new_with_absolute_min(
        min_validators: u32,
        absolute_min_validators: u32,
        config: DegradedSecurityConfig,
    ) -> Self {
        Self {
            state: NetworkSecurityState::Bootstrap,
            config,
            epochs_below_minimum: 0,
            epochs_above_minimum_in_dsm: 0,
            bootstrap_completed: false,
            min_validators,
            absolute_min_validators,
            shm_active: false,
            terminal_active: false,
            bootstrap_recovery_active: false,
            bootstrap_extensions: 0,
            state_transitions: Vec::new(),
        }
    }

    /// Backward compatibility: create with critical threshold
    pub fn new_with_critical(
        min_validators: u32,
        critical_validators: u32,
        config: ValidatorDegradedConfig,
    ) -> Self {
        Self::new_with_absolute_min(min_validators, critical_validators, config)
    }

    /// Create a tracker that has already completed bootstrap (for testing)
    pub fn post_bootstrap(min_validators: u32, config: DegradedSecurityConfig) -> Self {
        Self {
            state: NetworkSecurityState::Normal,
            config,
            epochs_below_minimum: 0,
            epochs_above_minimum_in_dsm: 0,
            bootstrap_completed: true,
            min_validators,
            absolute_min_validators: V_MIN_ABSOLUTE,
            shm_active: false,
            terminal_active: false,
            bootstrap_recovery_active: false,
            bootstrap_extensions: 0,
            state_transitions: vec![StateTransition {
                from_state: "Bootstrap".to_string(),
                to_state: "Normal".to_string(),
                epoch: 0,
                validator_count: min_validators,
                reason: "Bootstrap completed".to_string(),
            }],
        }
    }

    /// Update the security state based on current validator count
    /// SPEC v6.4 State Machine
    /// Should be called at each epoch boundary
    ///
    /// State transitions (SPEC v6.4 §4):
    /// - Bootstrap → Normal: When Epoch ≥ 1440 AND Validators ≥ V_min_operational (§6.1)
    /// - Normal → DegradedSecurityMode: When validators < V_min_operational (§4.2)
    /// - DegradedSecurityMode → Normal: When validators ≥ V_min_operational for 3 epochs (§5.2)
    /// - DegradedSecurityMode → SafetyHaltMode: When validators < V_min_absolute (§4.3)
    /// - DegradedSecurityMode → BootstrapRecoveryMode: When in DSM for 10+ epochs (§6.2)
    /// - SafetyHaltMode → DegradedSecurityMode: When validators ≥ V_min_absolute (§5.1)
    pub fn update(
        &mut self,
        current_epoch: EpochNumber,
        active_validators: u32,
        bootstrap_config: &BootstrapConfig,
        _total_stake: Balance,
    ) -> Option<NetworkSecurityState> {
        let previous_state = self.state.clone();

        match &self.state {
            // --- BOOTSTRAP STATE ---
            // SPEC v6.4 §6.1: Bootstrap Exit Guard
            NetworkSecurityState::Bootstrap => {
                // Check if bootstrap should exit
                // SPEC v6.4 §6.1: Epoch ≥ bootstrap_epochs AND V_active ≥ V_min_operational
                let epoch_met = current_epoch >= bootstrap_config.end_epoch;
                let validators_met = active_validators >= self.min_validators;

                // SPEC v6.4 §6.1: Bootstrap exits when BOTH conditions met
                if epoch_met && validators_met {
                    self.bootstrap_completed = true;
                    self.state = NetworkSecurityState::Normal;
                    self.record_transition(&previous_state, current_epoch, active_validators,
                        &format!("Bootstrap completed: epoch {} ≥ {}, validators {} ≥ {} (SPEC v6.4 §6.1)",
                            current_epoch, bootstrap_config.end_epoch,
                            active_validators, self.min_validators));
                    return Some(self.state.clone());
                }

                // Bootstrap continues until conditions are met
            }

            // --- NORMAL STATE ---
            // SPEC v6.4 §4.1: Normal Operation
            NetworkSecurityState::Normal => {
                if active_validators < self.min_validators {
                    self.epochs_below_minimum += 1;

                    // Enter DSM or SHM after grace period
                    if self.epochs_below_minimum >= self.config.floor_grace_epochs {
                        // SPEC v6.5 §4.3: Check if should go directly to Critical
                        if active_validators < self.absolute_min_validators {
                            self.state = NetworkSecurityState::SafetyHaltMode {
                                entered_at: current_epoch,
                                epochs_in_shm: 0,
                                current_validators: active_validators,
                                epochs_without_finality: 0,
                            };
                            self.shm_active = true;
                            self.epochs_below_minimum = 0;
                            self.record_transition(&previous_state, current_epoch, active_validators,
                                &format!("Entered Critical state: validators {} < V_min_absolute {} (SPEC v6.5 §4.3)",
                                    active_validators, self.absolute_min_validators));
                        } else {
                            // SPEC v6.5 §4.2: Enter Degraded state
                            self.state = NetworkSecurityState::DegradedSecurityMode {
                                entered_at: current_epoch,
                                epochs_in_dsm: 0,
                                current_validators: active_validators,
                                validators_needed: self.min_validators - active_validators,
                                consecutive_epochs_above_safe: 0,
                            };
                            self.epochs_below_minimum = 0;
                            self.record_transition(&previous_state, current_epoch, active_validators,
                                &format!("Entered Degraded state: validators {} < SafeValidators {} (SPEC v7.1 §5.2)",
                                    active_validators, SAFE_VALIDATORS));
                        }
                        return Some(self.state.clone());
                    }
                } else {
                    // Reset counter if validators recovered
                    self.epochs_below_minimum = 0;
                }
            }

            // --- DEGRADED SECURITY MODE (DSM) ---
            // SPEC v7.1 §5.2: Degraded state behavior and recovery
            NetworkSecurityState::DegradedSecurityMode {
                entered_at,
                epochs_in_dsm,
                consecutive_epochs_above_safe,
                ..
            } => {
                let entered = *entered_at;
                let in_dsm = *epochs_in_dsm;
                let consecutive = *consecutive_epochs_above_safe;

                // SPEC v7.1 §5.2: Check for Restricted/Emergency state escalation
                // Restricted: 25-49 validators, Emergency: < 25 validators
                if active_validators < EMERGENCY_VALIDATORS {
                    // SPEC v7.1 §6.1: Emergency auto-trigger when < 25 validators
                    self.state = NetworkSecurityState::TerminalMode {
                        entered_at: current_epoch,
                        epochs_in_terminal: 0,
                        current_validators: active_validators,
                        terminal_state_root: None,
                    };
                    self.terminal_active = true;
                    self.shm_active = false;
                    self.epochs_above_minimum_in_dsm = 0;
                    self.record_transition(&previous_state, current_epoch, active_validators,
                        &format!("Entered Emergency state: validators {} < EmergencyValidators {} (SPEC v7.1 §6.1)",
                            active_validators, EMERGENCY_VALIDATORS));
                    return Some(self.state.clone());
                } else if active_validators < POST_BOOTSTRAP_MIN_VALIDATORS {
                    // SPEC v7.1 §5.2: Restricted state (25-49 validators)
                    self.state = NetworkSecurityState::SafetyHaltMode {
                        entered_at: current_epoch,
                        epochs_in_shm: 0,
                        current_validators: active_validators,
                        epochs_without_finality: 0,
                    };
                    self.shm_active = true;
                    self.epochs_above_minimum_in_dsm = 0;
                    self.record_transition(&previous_state, current_epoch, active_validators,
                        &format!("Entered Restricted state: validators {} < PostBootstrapMin {} (SPEC v7.1 §5.2)",
                            active_validators, POST_BOOTSTRAP_MIN_VALIDATORS));
                    return Some(self.state.clone());
                }

                // SPEC v7.1 §4.1: Check for Bootstrap Recovery Mode (Validator Population Collapse)
                if in_dsm >= self.config.collapse_detection_epochs && active_validators < SAFE_VALIDATORS {
                    self.state = NetworkSecurityState::BootstrapRecoveryMode {
                        entered_at: current_epoch,
                        epochs_in_recovery: 0,
                        current_validators: active_validators,
                        validators_needed: SAFE_VALIDATORS - active_validators,
                    };
                    self.bootstrap_recovery_active = true;
                    self.epochs_above_minimum_in_dsm = 0;
                    self.record_transition(&previous_state, current_epoch, active_validators,
                        &format!("Entered Bootstrap Recovery Mode: {} epochs in Degraded (SPEC v7.1 §4.1)",
                            in_dsm));
                    return Some(self.state.clone());
                }

                // SPEC v7.1 §7.1: Recovery check - need SafeValidators (75) for 100 epochs
                if active_validators >= SAFE_VALIDATORS {
                    let new_consecutive = consecutive + 1;
                    self.epochs_above_minimum_in_dsm = new_consecutive;

                    // SPEC v7.1 §7.1: Exit Degraded after 100 consecutive epochs at SafeValidators
                    if new_consecutive >= self.config.normal_recovery_epochs {
                        self.state = NetworkSecurityState::Normal;
                        self.epochs_above_minimum_in_dsm = 0;
                        self.record_transition(&previous_state, current_epoch, active_validators,
                            &format!("Recovered to Normal: validators {} ≥ SafeValidators {} for {} epochs (SPEC v7.1 §7.1)",
                                active_validators, SAFE_VALIDATORS, self.config.normal_recovery_epochs));
                        return Some(self.state.clone());
                    }

                    // Still in Degraded but recovering
                    self.state = NetworkSecurityState::DegradedSecurityMode {
                        entered_at: entered,
                        epochs_in_dsm: in_dsm + 1,
                        current_validators: active_validators,
                        validators_needed: 0, // At SafeValidators, waiting for recovery
                        consecutive_epochs_above_safe: new_consecutive,
                    };
                } else {
                    // Still below SafeValidators - reset consecutive counter
                    self.epochs_above_minimum_in_dsm = 0;
                    self.state = NetworkSecurityState::DegradedSecurityMode {
                        entered_at: entered,
                        epochs_in_dsm: in_dsm + 1,
                        current_validators: active_validators,
                        validators_needed: SAFE_VALIDATORS - active_validators,
                        consecutive_epochs_above_safe: 0,
                    };
                }
            }

            // --- RESTRICTED STATE (SafetyHaltMode) ---
            // SPEC v7.1 §5.2: Governance frozen, Validator incentives boosted, Emergency armed
            NetworkSecurityState::SafetyHaltMode {
                entered_at,
                epochs_in_shm,
                epochs_without_finality,
                ..
            } => {
                let entered = *entered_at;
                let in_shm = *epochs_in_shm;
                let no_finality = *epochs_without_finality;

                // SPEC v7.1 §6.1: Check for Emergency auto-trigger
                if active_validators < EMERGENCY_VALIDATORS {
                    self.state = NetworkSecurityState::TerminalMode {
                        entered_at: current_epoch,
                        epochs_in_terminal: 0,
                        current_validators: active_validators,
                        terminal_state_root: None,
                    };
                    self.terminal_active = true;
                    self.shm_active = false;
                    self.record_transition(&previous_state, current_epoch, active_validators,
                        &format!("Entered Emergency state: validators {} < EmergencyValidators {} (SPEC v7.1 §6.1)",
                            active_validators, EMERGENCY_VALIDATORS));
                    return Some(self.state.clone());
                }

                // SPEC v7.1 §7.2: Check if can exit Restricted → Degraded
                if active_validators >= POST_BOOTSTRAP_MIN_VALIDATORS {
                    if in_shm >= self.config.shm_recovery_epochs {
                        // Go to Degraded state
                        if active_validators >= SAFE_VALIDATORS {
                            // At SafeValidators - go to Degraded with recovery tracking
                            self.state = NetworkSecurityState::DegradedSecurityMode {
                                entered_at: current_epoch,
                                epochs_in_dsm: 0,
                                current_validators: active_validators,
                                validators_needed: 0,
                                consecutive_epochs_above_safe: 1,
                            };
                            self.epochs_above_minimum_in_dsm = 1;
                        } else {
                            // Still below SafeValidators - go to Degraded
                            self.state = NetworkSecurityState::DegradedSecurityMode {
                                entered_at: current_epoch,
                                epochs_in_dsm: 0,
                                current_validators: active_validators,
                                validators_needed: SAFE_VALIDATORS - active_validators,
                                consecutive_epochs_above_safe: 0,
                            };
                            self.epochs_above_minimum_in_dsm = 0;
                        }
                        self.shm_active = false;
                        self.record_transition(&previous_state, current_epoch, active_validators,
                            &format!("Exited Restricted → Degraded: validators {} ≥ PostBootstrapMin {} (SPEC v7.1 §7.2)",
                                active_validators, POST_BOOTSTRAP_MIN_VALIDATORS));
                        return Some(self.state.clone());
                    }
                }

                // SPEC v7.1 §5.2: Remain in Restricted
                // Governance frozen, validator incentives boosted, emergency armed
                self.state = NetworkSecurityState::SafetyHaltMode {
                    entered_at: entered,
                    epochs_in_shm: in_shm + 1,
                    current_validators: active_validators,
                    epochs_without_finality: no_finality + 1,
                };
            }

            // --- EMERGENCY STATE (TerminalMode) ---
            // SPEC v7.1 §6: Auto-trigger, fork allowed, exit always permitted
            NetworkSecurityState::TerminalMode {
                entered_at,
                epochs_in_terminal,
                terminal_state_root,
                ..
            } => {
                let entered = *entered_at;
                let in_terminal = *epochs_in_terminal;
                let state_root = *terminal_state_root;

                // SPEC v7.1 §6.2: Emergency state - recovery only via fork or validator return
                // Check for recovery to Restricted if validators >= EmergencyValidators
                if active_validators >= EMERGENCY_VALIDATORS {
                    // Can exit Emergency → Restricted
                    self.state = NetworkSecurityState::SafetyHaltMode {
                        entered_at: current_epoch,
                        epochs_in_shm: 0,
                        current_validators: active_validators,
                        epochs_without_finality: 0,
                    };
                    self.shm_active = true;
                    self.terminal_active = false;
                    self.record_transition(&previous_state, current_epoch, active_validators,
                        &format!("Exited Emergency → Restricted: validators {} ≥ EmergencyValidators {} (SPEC v7.1 §7.2)",
                            active_validators, EMERGENCY_VALIDATORS));
                    return Some(self.state.clone());
                }

                // Remain in Emergency state
                self.state = NetworkSecurityState::TerminalMode {
                    entered_at: entered,
                    epochs_in_terminal: in_terminal + 1,
                    current_validators: active_validators,
                    terminal_state_root: state_root,
                };
            }

            // --- BOOTSTRAP RECOVERY MODE ---
            // SPEC v7.1 §4.1: Validator Population Collapse recovery
            NetworkSecurityState::BootstrapRecoveryMode {
                entered_at,
                epochs_in_recovery,
                ..
            } => {
                let entered = *entered_at;
                let in_recovery = *epochs_in_recovery;

                // Check for Emergency escalation
                if active_validators < EMERGENCY_VALIDATORS {
                    self.state = NetworkSecurityState::TerminalMode {
                        entered_at: current_epoch,
                        epochs_in_terminal: 0,
                        current_validators: active_validators,
                        terminal_state_root: None,
                    };
                    self.terminal_active = true;
                    self.bootstrap_recovery_active = false;
                    self.record_transition(&previous_state, current_epoch, active_validators,
                        &format!("Escalated from Bootstrap Recovery → Emergency: validators {} < EmergencyValidators {} (SPEC v7.1 §6.1)",
                            active_validators, EMERGENCY_VALIDATORS));
                    return Some(self.state.clone());
                }

                // Check for Restricted escalation
                if active_validators < POST_BOOTSTRAP_MIN_VALIDATORS {
                    self.state = NetworkSecurityState::SafetyHaltMode {
                        entered_at: current_epoch,
                        epochs_in_shm: 0,
                        current_validators: active_validators,
                        epochs_without_finality: 0,
                    };
                    self.shm_active = true;
                    self.bootstrap_recovery_active = false;
                    self.record_transition(&previous_state, current_epoch, active_validators,
                        &format!("Escalated from Bootstrap Recovery → Restricted: validators {} < PostBootstrapMin {} (SPEC v7.1 §5.2)",
                            active_validators, POST_BOOTSTRAP_MIN_VALIDATORS));
                    return Some(self.state.clone());
                }

                // Check if recovery achieved (at SafeValidators)
                if active_validators >= SAFE_VALIDATORS {
                    // Go to Degraded with recovery tracking
                    self.state = NetworkSecurityState::DegradedSecurityMode {
                        entered_at: current_epoch,
                        epochs_in_dsm: 0,
                        current_validators: active_validators,
                        validators_needed: 0,
                        consecutive_epochs_above_safe: 1,
                    };
                    self.epochs_above_minimum_in_dsm = 1;
                    self.bootstrap_recovery_active = false;
                    self.record_transition(&previous_state, current_epoch, active_validators,
                        &format!("Exited Bootstrap Recovery → Degraded: validators {} ≥ SafeValidators {} (SPEC v7.1 §7.1)",
                            active_validators, SAFE_VALIDATORS));
                    return Some(self.state.clone());
                }

                // Remain in Bootstrap Recovery (50-74 validators)
                self.state = NetworkSecurityState::BootstrapRecoveryMode {
                    entered_at: entered,
                    epochs_in_recovery: in_recovery + 1,
                    current_validators: active_validators,
                    validators_needed: SAFE_VALIDATORS - active_validators,
                };
            }
        }

        None // No state change
    }

    /// Backward compatibility: update with finality rate (ignored in v6.4)
    pub fn update_with_finality(
        &mut self,
        current_epoch: EpochNumber,
        active_validators: u32,
        bootstrap_config: &BootstrapConfig,
        total_stake: Balance,
        _finality_rate_percent: u32, // Ignored
    ) -> Option<NetworkSecurityState> {
        self.update(current_epoch, active_validators, bootstrap_config, total_stake)
    }

    /// Record a state transition
    fn record_transition(
        &mut self,
        from: &NetworkSecurityState,
        epoch: EpochNumber,
        validator_count: u32,
        reason: &str,
    ) {
        let from_str = match from {
            NetworkSecurityState::Bootstrap => "Bootstrap",
            NetworkSecurityState::Normal => "Healthy",
            NetworkSecurityState::DegradedSecurityMode { .. } => "Degraded",
            NetworkSecurityState::SafetyHaltMode { .. } => "Critical",
            NetworkSecurityState::TerminalMode { .. } => "Terminal",
            NetworkSecurityState::BootstrapRecoveryMode { .. } => "BootstrapRecovery",
        };
        let to_str = match &self.state {
            NetworkSecurityState::Bootstrap => "Bootstrap",
            NetworkSecurityState::Normal => "Healthy",
            NetworkSecurityState::DegradedSecurityMode { .. } => "Degraded",
            NetworkSecurityState::SafetyHaltMode { .. } => "Critical",
            NetworkSecurityState::TerminalMode { .. } => "Terminal",
            NetworkSecurityState::BootstrapRecoveryMode { .. } => "BootstrapRecovery",
        };

        self.state_transitions.push(StateTransition {
            from_state: from_str.to_string(),
            to_state: to_str.to_string(),
            epoch,
            validator_count,
            reason: reason.to_string(),
        });

        // Keep only last 100 transitions to prevent unbounded growth
        if self.state_transitions.len() > 100 {
            self.state_transitions.remove(0);
        }
    }

    // --- State Query Methods ---

    /// Check if in Degraded state
    /// SPEC v6.5 §4.2: V_min_absolute ≤ V_active < V_min_operational (21-49)
    pub fn is_dsm(&self) -> bool {
        matches!(self.state, NetworkSecurityState::DegradedSecurityMode { .. })
    }

    /// Check if in Critical state (SafetyHaltMode)
    /// SPEC v6.5 §4.3: V_active < V_min_absolute (< 21)
    pub fn is_shm(&self) -> bool {
        matches!(self.state, NetworkSecurityState::SafetyHaltMode { .. })
    }

    /// Check if in Critical state
    /// SPEC v6.5 §4.3: V_active < V_min_absolute (< 21)
    pub fn is_critical(&self) -> bool {
        matches!(self.state, NetworkSecurityState::SafetyHaltMode { .. })
    }

    /// Check if in Terminal state
    /// SPEC v6.5 §4.4: No finality for 24+ epochs
    pub fn is_terminal(&self) -> bool {
        matches!(self.state, NetworkSecurityState::TerminalMode { .. })
    }

    /// Check if in Bootstrap Recovery Mode
    /// SPEC v6.5 §6.2: After 10 epochs in Degraded
    pub fn is_bootstrap_recovery_mode(&self) -> bool {
        matches!(self.state, NetworkSecurityState::BootstrapRecoveryMode { .. })
    }

    /// Check if in any degraded state (Degraded, Critical, Terminal, or Bootstrap Recovery)
    pub fn is_degraded(&self) -> bool {
        self.is_dsm() || self.is_shm() || self.is_terminal() || self.is_bootstrap_recovery_mode()
    }

    /// Check if network is in Healthy state
    pub fn is_normal(&self) -> bool {
        matches!(self.state, NetworkSecurityState::Normal)
    }

    /// Check if network is in Healthy state (v6.5 naming)
    pub fn is_healthy(&self) -> bool {
        matches!(self.state, NetworkSecurityState::Normal)
    }

    /// Check if still in bootstrap
    pub fn is_bootstrap(&self) -> bool {
        matches!(self.state, NetworkSecurityState::Bootstrap)
    }

    /// Check if block production is halted
    /// SPEC v6.5 §4.4: Chain read-only in Terminal state
    pub fn is_halted(&self) -> bool {
        self.is_terminal() && self.config.terminal_read_only
    }

    /// Check if chain is read-only
    /// SPEC v6.5 §4.4: Chain read-only in Terminal state
    pub fn is_read_only(&self) -> bool {
        self.is_terminal() && self.config.terminal_read_only
    }

    // Backward compatibility aliases
    pub fn is_validator_degraded(&self) -> bool { self.is_dsm() }
    pub fn is_survival_mode(&self) -> bool { self.is_shm() }
    pub fn is_survival(&self) -> bool { self.shm_active }
    pub fn is_vss(&self) -> bool { self.is_dsm() }
    pub fn is_safety_mode(&self) -> bool { self.is_dsm() }
    pub fn is_bootstrap_recovery(&self) -> bool { self.is_dsm() || self.is_bootstrap_recovery_mode() }
    pub fn is_emergency_mode(&self) -> bool { self.is_shm() || self.is_terminal() }

    // --- SPEC v6.5 §8: Governance Restrictions ---

    /// Check if governance is allowed
    /// SPEC v6.5 §4.2: Governance allowed in Degraded (with doubled timelocks)
    /// SPEC v6.5 §4.3: Governance frozen in Critical
    /// SPEC v6.5 §4.4: Governance frozen in Terminal
    pub fn is_governance_allowed(&self, _is_constitutional: bool) -> bool {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode { .. } |
            NetworkSecurityState::BootstrapRecoveryMode { .. } => {
                // SPEC v6.5 §4.2: Governance allowed but timelocks doubled
                true
            }
            NetworkSecurityState::SafetyHaltMode { .. } => {
                // SPEC v6.5 §4.3: Governance frozen in Critical
                !self.config.critical_governance_frozen
            }
            NetworkSecurityState::TerminalMode { .. } => {
                // SPEC v6.5 §4.4: Governance frozen in Terminal
                false
            }
            _ => true, // Allowed in Bootstrap and Healthy
        }
    }

    /// Check if parameter changes are allowed
    /// SPEC v6.5 §8: No parameter changes in Critical/Terminal
    pub fn is_parameter_change_allowed(&self) -> bool {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode { .. } |
            NetworkSecurityState::SafetyHaltMode { .. } |
            NetworkSecurityState::TerminalMode { .. } |
            NetworkSecurityState::BootstrapRecoveryMode { .. } => false,
            _ => true,
        }
    }

    /// Check if slashing is allowed
    /// SPEC v6.5 §4.3: No slashing in Critical/Terminal
    pub fn is_slashing_allowed(&self) -> bool {
        match &self.state {
            NetworkSecurityState::SafetyHaltMode { .. } |
            NetworkSecurityState::TerminalMode { .. } => !self.config.shm_no_slashing,
            _ => true,
        }
    }

    // --- SPEC v6.5 Effects ---

    /// Get validator rewards multiplier
    /// SPEC v6.5 §7.2: 100% inflation to validators when below TARGET
    pub fn get_validator_rewards_multiplier(&self) -> f64 {
        match &self.state {
            NetworkSecurityState::BootstrapRecoveryMode { .. } => 1.25, // +25% in recovery
            _ => 1.0,
        }
    }

    /// Get inflation redirect percentage to validators
    /// SPEC v6.5 §7.2: 100% inflation → validators when V < TARGET
    pub fn get_inflation_redirect_to_validators_percent(&self) -> u32 {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode { .. } |
            NetworkSecurityState::SafetyHaltMode { .. } |
            NetworkSecurityState::BootstrapRecoveryMode { .. } => {
                self.config.inflation_redirect_to_validators_percent
            }
            _ => 0,
        }
    }

    /// Get the inflation boost (0 in v6.4 - capped to maintenance)
    /// SPEC v6.4 §4.2: Inflation capped at maintenance-only in DSM
    pub fn get_inflation_boost(&self) -> f64 {
        match &self.state {
            NetworkSecurityState::BootstrapRecoveryMode { .. } => 1.0, // +1% in recovery
            _ => 0.0,
        }
    }

    /// Get the stake minimum reduction percentage
    /// SPEC v6.4 §7: Stake minimum temporarily reduced
    pub fn get_stake_reduction(&self) -> u32 {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode { .. } |
            NetworkSecurityState::SafetyHaltMode { .. } |
            NetworkSecurityState::BootstrapRecoveryMode { .. } => {
                self.config.dsm_stake_reduction_percent
            }
            _ => 0,
        }
    }

    // --- SPEC v6.4 §9: Exit & Fork Guarantees ---

    /// Check if fork is allowed
    /// SPEC v6.4 §9.5: Exit and fork remain possible
    pub fn is_fork_allowed(&self) -> bool {
        // Fork is ALWAYS allowed per SPEC v6.4 §9
        true
    }

    /// Check if exit is allowed
    /// SPEC v6.4 §9.4: Funds are never frozen
    pub fn is_exit_allowed(&self) -> bool {
        // Exit is ALWAYS allowed per SPEC v6.4 §9
        // Even in SHM - SPEC v6.4 §4.3: Exits still allowed
        true
    }

    /// Get recovery progress (consecutive epochs above SafeValidators)
    /// SPEC v7.1 §7.1: Degraded exit requires 100 consecutive epochs at ≥75 validators
    pub fn get_recovery_progress(&self) -> Option<(EpochNumber, EpochNumber)> {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode {
                consecutive_epochs_above_safe, ..
            } => Some((*consecutive_epochs_above_safe, self.config.normal_recovery_epochs)),
            _ => None,
        }
    }

    // --- Health Status for RPC ---

    /// Get network health status for RPC/signaling
    /// SPEC v6.4 §10: Implementation Notes (Events)
    pub fn get_health_status(&self, current_epoch: EpochNumber) -> NetworkHealthStatus {
        match &self.state {
            NetworkSecurityState::Bootstrap => NetworkHealthStatus::Bootstrapping {
                validators_current: 0, // Will be updated by caller
                validators_needed: self.min_validators,
                epochs_elapsed: current_epoch,
                epochs_remaining: None,
            },

            NetworkSecurityState::Normal => NetworkHealthStatus::Healthy,

            NetworkSecurityState::DegradedSecurityMode {
                epochs_in_dsm,
                current_validators,
                validators_needed,
                consecutive_epochs_above_safe,
                ..
            } => {
                // Check if recovering (above SafeValidators, counting consecutive epochs)
                if *validators_needed == 0 {
                    return NetworkHealthStatus::Recovering {
                        epochs_until_normal: self.config.normal_recovery_epochs
                            .saturating_sub(*consecutive_epochs_above_safe),
                        epochs_consecutive: *consecutive_epochs_above_safe,
                        validators_current: *current_validators,
                    };
                }

                let severity = self.calculate_severity(*current_validators, *epochs_in_dsm);

                // SPEC v6.4 §4.2: DSM Effects
                let mut effects = Vec::new();
                effects.push(format!("Block time ×{}", self.config.dsm_block_time_multiplier));
                effects.push(format!("Epoch duration ×{}", self.config.dsm_epoch_duration_multiplier));
                effects.push(format!("Governance timelocks ×{}", self.config.dsm_governance_timelock_multiplier));
                if self.config.dsm_slashing_tightened {
                    effects.push("Slashing thresholds tightened".to_string());
                }
                if self.config.dsm_inflation_capped {
                    effects.push("Inflation capped at maintenance-only".to_string());
                }
                effects.push(format!("Stake minimum -{}%", self.config.dsm_stake_reduction_percent));

                NetworkHealthStatus::DegradedSecurityMode {
                    severity,
                    validators_current: *current_validators,
                    validators_needed: *validators_needed,
                    epochs_in_state: *epochs_in_dsm,
                    recovery_progress: (*consecutive_epochs_above_safe, self.config.normal_recovery_epochs),
                    effects_active: effects,
                }
            }

            NetworkSecurityState::SafetyHaltMode {
                epochs_in_shm,
                current_validators,
                epochs_without_finality,
                ..
            } => {
                // SPEC v6.5 §4.3: Use Critical health status for SafetyHaltMode
                NetworkHealthStatus::Critical {
                    epochs_in_critical: *epochs_in_shm,
                    validators_current: *current_validators,
                    absolute_minimum: self.absolute_min_validators,
                    fork_enabled: self.config.critical_fork_enabled,
                    fork_threshold_reduction: self.config.critical_fork_threshold_reduction_percent,
                    epochs_without_finality: *epochs_without_finality,
                }
            }

            NetworkSecurityState::TerminalMode {
                epochs_in_terminal,
                current_validators,
                terminal_state_root,
                ..
            } => {
                // SPEC v6.5 §4.4: Terminal state - chain read-only, fork mandatory
                NetworkHealthStatus::Terminal {
                    epochs_in_terminal: *epochs_in_terminal,
                    validators_current: *current_validators,
                    fork_mandatory: self.config.terminal_fork_mandatory,
                    state_snapshot_finalized: terminal_state_root.is_some(),
                }
            }

            NetworkSecurityState::BootstrapRecoveryMode {
                epochs_in_recovery,
                current_validators,
                validators_needed,
                ..
            } => {
                let mut incentives = Vec::new();
                incentives.push("Bootstrap incentives re-enabled".to_string());
                incentives.push(format!("Stake minimum -{}%", self.config.dsm_stake_reduction_percent));
                incentives.push("VC accumulation boost reactivated".to_string());

                NetworkHealthStatus::BootstrapRecovery {
                    epochs_in_recovery: *epochs_in_recovery,
                    validators_current: *current_validators,
                    validators_needed: *validators_needed,
                    incentives_active: incentives,
                }
            }
        }
    }

    // Backward compat: get_health_status without epoch
    pub fn get_health_status_simple(&self) -> NetworkHealthStatus {
        self.get_health_status(0)
    }

    /// Calculate severity based on validator count and time in state
    fn calculate_severity(&self, current_validators: u32, epochs_in_state: EpochNumber) -> SecuritySeverity {
        if self.shm_active {
            SecuritySeverity::Emergency
        } else if current_validators < self.absolute_min_validators {
            SecuritySeverity::Critical
        } else if current_validators < self.min_validators / 2 {
            SecuritySeverity::Elevated
        } else if epochs_in_state > 50 {
            SecuritySeverity::Elevated
        } else {
            SecuritySeverity::Warning
        }
    }

    /// Get detailed info about current degraded state
    /// SPEC v6.4 compliant
    pub fn get_degraded_info(&self) -> Option<DegradedInfo> {
        match &self.state {
            NetworkSecurityState::DegradedSecurityMode {
                entered_at,
                epochs_in_dsm,
                current_validators,
                validators_needed,
                consecutive_epochs_above_safe,
            } => Some(DegradedInfo {
                mode: DegradedMode::DegradedSecurityMode,
                entered_at: *entered_at,
                epochs_in_state: *epochs_in_dsm,
                current_validators: *current_validators,
                validators_needed: *validators_needed,
                consecutive_epochs_above_min: *consecutive_epochs_above_safe, // v7.1: now tracks SafeValidators
                epochs_until_recovery: if *validators_needed == 0 {
                    Some(self.config.normal_recovery_epochs.saturating_sub(*consecutive_epochs_above_safe))
                } else {
                    None
                },
                emergency_active: self.emergency_active(),
            }),
            NetworkSecurityState::SafetyHaltMode {
                entered_at,
                epochs_in_shm,
                current_validators,
                epochs_without_finality: _, // SPEC v6.5
            } => Some(DegradedInfo {
                mode: DegradedMode::SafetyHalt,
                entered_at: *entered_at,
                epochs_in_state: *epochs_in_shm,
                current_validators: *current_validators,
                validators_needed: self.critical_validators().saturating_sub(*current_validators),
                consecutive_epochs_above_min: 0,
                epochs_until_recovery: None, // Must exit safety halt first
                emergency_active: true,
            }),
            NetworkSecurityState::TerminalMode {
                entered_at,
                epochs_in_terminal,
                current_validators,
                ..
            } => Some(DegradedInfo {
                mode: DegradedMode::Terminal,
                entered_at: *entered_at,
                epochs_in_state: *epochs_in_terminal,
                current_validators: *current_validators,
                validators_needed: self.min_validators,
                consecutive_epochs_above_min: 0,
                epochs_until_recovery: None, // Terminal is permanent until fork
                emergency_active: true,
            }),
            _ => None,
        }
    }
}

/// Which degraded mode the network is in
/// SPEC v6.5: Degraded, Critical, Terminal, or BootstrapRecoveryMode
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DegradedMode {
    /// Degraded Security Mode (SPEC v6.5 §4.2)
    /// V_min_absolute ≤ V_active < V_min_operational
    DegradedSecurityMode,
    /// Critical / Safety Halt Mode (SPEC v6.5 §4.3)
    /// V_active < V_min_absolute
    SafetyHalt,
    /// Terminal Mode (SPEC v6.5 §4.4)
    /// No finality for 24+ epochs - chain read-only, fork mandatory
    Terminal,
    /// Bootstrap Recovery Mode (SPEC v6.5 §6.2)
    /// After 10 consecutive epochs in DSM
    BootstrapRecoveryMode,
    // Backward compat aliases for older specs
    #[serde(alias = "ValidatorDegraded")]
    ValidatorDegraded,
    #[serde(alias = "Survival")]
    Survival,
    #[serde(alias = "ValidatorScarcity")]
    ValidatorScarcity,
    #[serde(alias = "EmergencySafety")]
    EmergencySafety,
    #[serde(alias = "SafetyMode")]
    SafetyMode,
    #[serde(alias = "BootstrapRecovery")]
    BootstrapRecovery,
    #[serde(alias = "Critical")]
    Critical,
}

/// Detailed information about degraded state
/// SPEC v6.2 compliant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradedInfo {
    pub mode: DegradedMode,
    pub entered_at: EpochNumber,
    pub epochs_in_state: EpochNumber,
    pub current_validators: u32,
    pub validators_needed: u32,
    /// SPEC v6.2 §7: Consecutive epochs above minimum
    pub consecutive_epochs_above_min: EpochNumber,
    pub epochs_until_recovery: Option<EpochNumber>,
    pub emergency_active: bool,
}

// =============================================================================
// BOOTSTRAP STATUS (existing code)
// =============================================================================

/// Bootstrap phase status
/// Tracks whether the network is in normal bootstrap, extended bootstrap, or completed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BootstrapStatus {
    /// Normal bootstrap phase - within the 1440 epoch window (60 days)
    Active {
        /// Epochs remaining until time limit
        epochs_remaining: EpochNumber,
        /// Additional validators needed to meet minimum
        validators_needed: u32,
    },

    /// Extended bootstrap - time limit passed but not enough validators
    /// Network continues in bootstrap mode until minimum validators join
    Extended {
        /// Epochs past the original end date
        epochs_overdue: EpochNumber,
        /// Additional validators needed to exit bootstrap
        validators_needed: u32,
    },

    /// Bootstrap completed - network is fully operational
    Completed,
}

/// Bootstrap era configuration
/// Fixed at genesis, cannot be changed by governance
///
/// SPEC v2.3: Bootstrap exits automatically when ANY condition is met:
/// - Epochs elapsed >= end_epoch (1440 = 60 days)
/// - OR active validators >= min_validators_exit (50)
/// - OR total network stake >= min_stake_total_exit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapConfig {
    /// Genesis epoch
    pub genesis_epoch: EpochNumber,

    /// Bootstrap end epoch (fixed, 1440 epochs = 60 days per SPEC v2.3)
    pub end_epoch: EpochNumber,

    /// Minimum validators to exit bootstrap (50 per SPEC v2.1)
    pub min_validators_exit: u32,

    /// Minimum total network stake to exit bootstrap (in KRAT)
    pub min_stake_total_exit: Balance,

    /// Minimum stake floor during bootstrap (in KRAT)
    pub min_stake_bootstrap: Balance,

    /// Minimum stake floor post-bootstrap (in KRAT)
    pub min_stake_post_bootstrap: Balance,

    /// VC multipliers during bootstrap
    pub vc_vote_multiplier: u32,
    pub vc_uptime_multiplier: u32,
    pub vc_arbitration_multiplier: u32,

    /// Target inflation during bootstrap (0.0 - 1.0)
    pub target_inflation: f64,
}

/// Stake requirement configuration
/// Based on SPEC v2.1 stake reduction function f(VC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeRequirementConfig {
    /// Nominal stake (default: 1M KRAT)
    pub nominal_stake: Balance,

    /// VC target for full reduction (default: 5000)
    pub vc_target: u64,

    /// Maximum stake reduction during bootstrap (default: 99%)
    pub max_reduction_bootstrap: f64,

    /// Maximum stake reduction post-bootstrap (default: 95%)
    pub max_reduction_post_bootstrap: f64,
}

impl Default for StakeRequirementConfig {
    fn default() -> Self {
        Self {
            // SPEC v2.2: Lowered stake requirements for accessibility
            // Previous: 1M nominal, 50K floor (at 95% reduction)
            // New: 500K nominal, 25K floor (at 95% reduction)
            //
            // This change:
            // - Makes validator entry more accessible
            // - Reduces cold start barrier
            // - Maintains security through VC bonus incentives
            nominal_stake: 500_000 * KRAT,   // 500K KRAT (was 1M in SPEC v2.1)
            vc_target: 5_000,                // 5000 VC for full reduction
            max_reduction_bootstrap: 0.99,   // 99% max reduction during bootstrap
            max_reduction_post_bootstrap: 0.95, // 95% max reduction post-bootstrap
            // Floor calculation: 500K × (1 - 0.95) = 25K KRAT (was 50K)
        }
    }
}

impl StakeRequirementConfig {
    /// Calculate required stake based on validator credits
    /// Formula from SPEC v2.1:
    ///   VC_norm = min(TotalVC / vc_target, 1.0)
    ///   StakeReduction = MaxReduction × VC_norm
    ///   RequiredStake = max(NominalStake × (1 − StakeReduction), StakeFloor)
    pub fn calculate_required_stake(
        &self,
        total_vc: u64,
        current_epoch: EpochNumber,
        bootstrap_config: &BootstrapConfig,
    ) -> Balance {
        // Determine max reduction based on era
        let max_reduction = if bootstrap_config.is_bootstrap(current_epoch) {
            self.max_reduction_bootstrap
        } else {
            self.max_reduction_post_bootstrap
        };

        // Calculate normalized VC (0.0 to 1.0)
        let vc_norm = ((total_vc as f64) / (self.vc_target as f64)).min(1.0);

        // Calculate stake reduction
        let stake_reduction = max_reduction * vc_norm;

        // Calculate required stake
        let required_stake = self.nominal_stake as f64 * (1.0 - stake_reduction);

        // Apply floor
        let stake_floor = bootstrap_config.get_min_stake(current_epoch);
        (required_stake as Balance).max(stake_floor)
    }

    /// Check if validator meets stake requirement
    pub fn meets_requirement(
        &self,
        validator_stake: Balance,
        total_vc: u64,
        current_epoch: EpochNumber,
        bootstrap_config: &BootstrapConfig,
    ) -> bool {
        let required = self.calculate_required_stake(total_vc, current_epoch, bootstrap_config);
        validator_stake >= required
    }

    /// Get stake deficiency if below requirement
    pub fn stake_deficiency(
        &self,
        validator_stake: Balance,
        total_vc: u64,
        current_epoch: EpochNumber,
        bootstrap_config: &BootstrapConfig,
    ) -> Option<Balance> {
        let required = self.calculate_required_stake(total_vc, current_epoch, bootstrap_config);
        if validator_stake < required {
            Some(required - validator_stake)
        } else {
            None
        }
    }
}

impl BootstrapConfig {
    /// Bootstrap configuration (unified)
    /// Bootstrap period: Genesis → 1440 epochs = 60 days (SPEC v2.3)
    /// Exit conditions: 1440 epochs OR 50 validators OR stake threshold
    pub fn mainnet_config() -> Self {
        Self {
            genesis_epoch: 0,
            end_epoch: 1440, // 1440 epochs = 60 days per SPEC v2.3 (1h/epoch)
            min_validators_exit: 50, // 50 validators to exit bootstrap
            min_stake_total_exit: 25_000_000 * KRAT, // 25M KRAT total stake
            min_stake_bootstrap: 50_000 * KRAT, // 50k KRAT
            min_stake_post_bootstrap: 25_000 * KRAT, // 25k KRAT
            vc_vote_multiplier: 2,
            vc_uptime_multiplier: 2,
            vc_arbitration_multiplier: 1,
            target_inflation: 0.065, // 6.5%
        }
    }

    /// Default config = mainnet config
    pub fn default_config() -> Self {
        Self::mainnet_config()
    }

    /// Check if currently in bootstrap era (time-based only)
    /// NOTE: This is a simplified check. Use `is_bootstrap_with_state()` for the
    /// comprehensive check that considers validator count requirements.
    pub fn is_bootstrap(&self, current_epoch: EpochNumber) -> bool {
        current_epoch < self.end_epoch
    }

    /// Check if bootstrap should exit based on ALL conditions
    ///
    /// SPEC v2.3 with safety constraint:
    /// - Exit is ALLOWED when: (epochs >= 1440 OR validators >= 50 OR stake >= 25M)
    /// - Exit is BLOCKED when: validators < min_validators_exit (constitutional minimum)
    ///
    /// This prevents the network from exiting bootstrap in an unsafe state where
    /// there aren't enough validators for proper decentralization.
    pub fn should_exit_bootstrap(
        &self,
        current_epoch: EpochNumber,
        active_validators: u32,
        total_stake: Balance,
    ) -> bool {
        // SAFETY CONSTRAINT: Cannot exit bootstrap if below minimum validator threshold
        // This is a constitutional requirement for decentralization
        if active_validators < self.min_validators_exit {
            return false;
        }

        // Exit if ANY positive condition is met (OR logic per SPEC)
        // AND the safety constraint above is satisfied
        current_epoch >= self.end_epoch
            || active_validators >= self.min_validators_exit
            || total_stake >= self.min_stake_total_exit
    }

    /// Comprehensive bootstrap check with all conditions
    /// Returns true if still in bootstrap, false if ready to exit
    pub fn is_bootstrap_with_state(
        &self,
        current_epoch: EpochNumber,
        active_validators: u32,
        total_stake: Balance,
    ) -> bool {
        !self.should_exit_bootstrap(current_epoch, active_validators, total_stake)
    }

    /// Check if bootstrap is in "extended" mode
    /// Extended bootstrap occurs when the time limit (1440 epochs) has passed
    /// but the network doesn't have enough validators to safely exit
    pub fn is_extended_bootstrap(
        &self,
        current_epoch: EpochNumber,
        active_validators: u32,
    ) -> bool {
        current_epoch >= self.end_epoch && active_validators < self.min_validators_exit
    }

    /// Get the bootstrap status with detailed information
    pub fn get_bootstrap_status(
        &self,
        current_epoch: EpochNumber,
        active_validators: u32,
        total_stake: Balance,
    ) -> BootstrapStatus {
        if self.should_exit_bootstrap(current_epoch, active_validators, total_stake) {
            BootstrapStatus::Completed
        } else if self.is_extended_bootstrap(current_epoch, active_validators) {
            BootstrapStatus::Extended {
                epochs_overdue: current_epoch.saturating_sub(self.end_epoch),
                validators_needed: self.min_validators_exit.saturating_sub(active_validators),
            }
        } else {
            BootstrapStatus::Active {
                epochs_remaining: self.end_epoch.saturating_sub(current_epoch),
                validators_needed: self.min_validators_exit.saturating_sub(active_validators),
            }
        }
    }

    /// Get minimum stake for current era
    pub fn get_min_stake(&self, current_epoch: EpochNumber) -> Balance {
        if self.is_bootstrap(current_epoch) {
            self.min_stake_bootstrap
        } else {
            self.min_stake_post_bootstrap
        }
    }

    /// Get VC multiplier for vote credits
    pub fn get_vote_multiplier(&self, current_epoch: EpochNumber) -> u32 {
        if self.is_bootstrap(current_epoch) {
            self.vc_vote_multiplier
        } else {
            1
        }
    }

    /// Get VC multiplier for uptime credits
    pub fn get_uptime_multiplier(&self, current_epoch: EpochNumber) -> u32 {
        if self.is_bootstrap(current_epoch) {
            self.vc_uptime_multiplier
        } else {
            1
        }
    }

    /// Get VC multiplier for arbitration credits
    pub fn get_arbitration_multiplier(&self, current_epoch: EpochNumber) -> u32 {
        if self.is_bootstrap(current_epoch) {
            self.vc_arbitration_multiplier
        } else {
            1
        }
    }
}

/// Adaptive inflation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InflationConfig {
    /// Target number of validators
    pub target_validators: u32,

    /// Average annual cost per validator (in KRAT)
    pub avg_validator_cost: Balance,

    /// Target stake ratio (total_staked / total_supply)
    pub target_stake_ratio: f64,

    /// Target active users
    pub target_active_users: u64,

    /// Minimum inflation floor (0.0 - 1.0)
    pub min_inflation: f64,

    /// Maximum inflation cap (0.0 - 1.0)
    pub max_inflation: f64,
}

impl Default for InflationConfig {
    fn default() -> Self {
        Self {
            target_validators: 100,
            avg_validator_cost: 10_000 * KRAT, // 10k KRAT/year
            target_stake_ratio: 0.30,    // 30% of supply staked
            target_active_users: 10_000,
            min_inflation: 0.005, // 0.5% floor
            max_inflation: 0.10,  // 10% cap
        }
    }
}

/// Network metrics for adaptive inflation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    /// Total supply (in KRAT)
    pub total_supply: Balance,

    /// Total staked (in KRAT)
    pub total_staked: Balance,

    /// Number of active validators
    pub active_validators: u32,

    /// Active users in recent epoch
    pub active_users: u64,

    /// Transaction count in recent epoch
    pub transactions_count: u64,
}

/// Adaptive inflation calculator
pub struct InflationCalculator {
    config: InflationConfig,
}

impl InflationCalculator {
    /// Create new inflation calculator
    pub fn new(config: InflationConfig) -> Self {
        Self { config }
    }

    /// Calculate annual emission based on network state
    pub fn calculate_annual_emission(&self, metrics: &NetworkMetrics) -> Balance {
        // Formula: AnnualEmission = BaseSecurityBudget × SecurityGapFactor × ActivityFactor

        let base_security_budget = self.calculate_base_security_budget();
        let security_gap_factor = self.calculate_security_gap_factor(metrics);
        let activity_factor = self.calculate_activity_factor(metrics);

        let raw_emission = (base_security_budget as f64 * security_gap_factor * activity_factor) as Balance;

        // Apply min/max bounds
        let min_emission = (metrics.total_supply as f64 * self.config.min_inflation) as Balance;
        let max_emission = (metrics.total_supply as f64 * self.config.max_inflation) as Balance;

        raw_emission.clamp(min_emission, max_emission)
    }

    /// Calculate base security budget
    fn calculate_base_security_budget(&self) -> Balance {
        self.config.target_validators as Balance * self.config.avg_validator_cost
    }

    /// Calculate security gap factor
    /// Over-secured → less inflation
    /// Under-secured → more inflation
    fn calculate_security_gap_factor(&self, metrics: &NetworkMetrics) -> f64 {
        if metrics.total_staked == 0 {
            return 1.5; // Maximum boost if no stake
        }

        let actual_stake_ratio = metrics.total_staked as f64 / metrics.total_supply as f64;
        let factor = self.config.target_stake_ratio / actual_stake_ratio;

        // Clamp to [0.3, 1.5]
        factor.clamp(0.3, 1.5)
    }

    /// Calculate activity factor
    /// More activity → justified inflation
    /// Less activity → reduced inflation
    fn calculate_activity_factor(&self, metrics: &NetworkMetrics) -> f64 {
        let activity_ratio = metrics.active_users as f64 / self.config.target_active_users as f64;

        // Square root dampens extreme values
        let factor = activity_ratio.sqrt();

        // Clamp to [0.5, 1.2]
        factor.clamp(0.5, 1.2)
    }

    /// Calculate effective annual inflation rate
    pub fn calculate_inflation_rate(&self, metrics: &NetworkMetrics) -> f64 {
        let emission = self.calculate_annual_emission(metrics);
        emission as f64 / metrics.total_supply as f64
    }

    /// Calculate per-epoch emission
    /// Assuming 52 epochs per year
    pub fn calculate_epoch_emission(&self, metrics: &NetworkMetrics) -> Balance {
        self.calculate_annual_emission(metrics) / 52
    }

    /// Get config
    pub fn config(&self) -> &InflationConfig {
        &self.config
    }
}

/// Fee distribution configuration
///
/// SPEC v3.1: Fee distribution follows the 60/30/10 rule:
/// - 60% to validators
/// - 30% burned (removed from circulation)
/// - 10% to treasury
///
/// SECURITY FIX #27: Added validation in constructor and distribute()
/// to ensure distribution always sums to 100% and no fees are lost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeDistribution {
    /// Percentage to validators (0.0 - 1.0)
    pub validators_share: f64,

    /// Percentage to burn (0.0 - 1.0)
    pub burn_share: f64,

    /// Percentage to treasury (0.0 - 1.0)
    pub treasury_share: f64,
}

/// Error type for FeeDistribution validation
#[derive(Debug, thiserror::Error)]
pub enum FeeDistributionError {
    #[error("Distribution shares must sum to 1.0 (got {0})")]
    InvalidSum(f64),

    #[error("All shares must be non-negative (validators={0}, burn={1}, treasury={2})")]
    NegativeShare(f64, f64, f64),
}

impl FeeDistribution {
    /// Create default distribution (60/30/10)
    /// SPEC v3.1: This is the canonical distribution
    pub fn default_distribution() -> Self {
        Self {
            validators_share: 0.60, // 60%
            burn_share: 0.30,       // 30%
            treasury_share: 0.10,   // 10%
        }
    }

    /// SECURITY FIX #27: Create with validation
    /// Returns error if shares don't sum to 1.0 or are negative
    pub fn new(
        validators_share: f64,
        burn_share: f64,
        treasury_share: f64,
    ) -> Result<Self, FeeDistributionError> {
        // Check for negative values
        if validators_share < 0.0 || burn_share < 0.0 || treasury_share < 0.0 {
            return Err(FeeDistributionError::NegativeShare(
                validators_share,
                burn_share,
                treasury_share,
            ));
        }

        let sum = validators_share + burn_share + treasury_share;
        if (sum - 1.0).abs() >= 0.001 {
            return Err(FeeDistributionError::InvalidSum(sum));
        }

        Ok(Self {
            validators_share,
            burn_share,
            treasury_share,
        })
    }

    /// Validate distribution (must sum to 1.0)
    pub fn validate(&self) -> bool {
        let sum = self.validators_share + self.burn_share + self.treasury_share;
        (sum - 1.0).abs() < 0.001
            && self.validators_share >= 0.0
            && self.burn_share >= 0.0
            && self.treasury_share >= 0.0
    }

    /// Distribute fee amount
    /// SECURITY FIX #27: Ensures no fees are lost due to rounding
    pub fn distribute(&self, total_fee: Balance) -> (Balance, Balance, Balance) {
        debug_assert!(self.validate(), "FeeDistribution must be valid");

        let validators = (total_fee as f64 * self.validators_share) as Balance;
        let burn = (total_fee as f64 * self.burn_share) as Balance;

        // SECURITY FIX #27: Treasury gets the remainder to prevent fee loss
        // This handles rounding errors by assigning any residual to treasury
        let treasury = total_fee.saturating_sub(validators).saturating_sub(burn);

        // Sanity check: total distributed should equal total fee
        debug_assert_eq!(
            validators.saturating_add(burn).saturating_add(treasury),
            total_fee,
            "Fee distribution must not lose any fees"
        );

        (validators, burn, treasury)
    }
}

/// Economics manager
pub struct EconomicsManager {
    bootstrap_config: BootstrapConfig,
    stake_requirement_config: StakeRequirementConfig,
    inflation_calculator: InflationCalculator,
    fee_distribution: FeeDistribution,
}

impl EconomicsManager {
    /// Create new economics manager with defaults
    pub fn new() -> Self {
        Self {
            bootstrap_config: BootstrapConfig::default_config(),
            stake_requirement_config: StakeRequirementConfig::default(),
            inflation_calculator: InflationCalculator::new(InflationConfig::default()),
            fee_distribution: FeeDistribution::default_distribution(),
        }
    }

    /// Create with custom configs
    pub fn with_config(
        bootstrap_config: BootstrapConfig,
        stake_requirement_config: StakeRequirementConfig,
        inflation_config: InflationConfig,
        fee_distribution: FeeDistribution,
    ) -> Self {
        Self {
            bootstrap_config,
            stake_requirement_config,
            inflation_calculator: InflationCalculator::new(inflation_config),
            fee_distribution,
        }
    }

    /// Check if in bootstrap era
    pub fn is_bootstrap(&self, current_epoch: EpochNumber) -> bool {
        self.bootstrap_config.is_bootstrap(current_epoch)
    }

    /// Get minimum stake for current era
    pub fn get_min_stake(&self, current_epoch: EpochNumber) -> Balance {
        self.bootstrap_config.get_min_stake(current_epoch)
    }

    /// Apply VC multiplier for vote credits
    pub fn apply_vote_multiplier(&self, base_credits: u32, current_epoch: EpochNumber) -> u32 {
        let multiplier = self.bootstrap_config.get_vote_multiplier(current_epoch);
        base_credits * multiplier
    }

    /// Apply VC multiplier for uptime credits
    pub fn apply_uptime_multiplier(&self, base_credits: u32, current_epoch: EpochNumber) -> u32 {
        let multiplier = self.bootstrap_config.get_uptime_multiplier(current_epoch);
        base_credits * multiplier
    }

    /// Calculate current inflation and emission
    pub fn calculate_emission(&self, metrics: &NetworkMetrics) -> Balance {
        self.inflation_calculator.calculate_annual_emission(metrics)
    }

    /// Calculate epoch emission
    pub fn calculate_epoch_emission(&self, metrics: &NetworkMetrics) -> Balance {
        self.inflation_calculator.calculate_epoch_emission(metrics)
    }

    /// Distribute fee
    pub fn distribute_fee(&self, fee: Balance) -> (Balance, Balance, Balance) {
        self.fee_distribution.distribute(fee)
    }

    /// Get bootstrap config
    pub fn bootstrap_config(&self) -> &BootstrapConfig {
        &self.bootstrap_config
    }

    /// Get inflation config
    pub fn inflation_config(&self) -> &InflationConfig {
        self.inflation_calculator.config()
    }

    /// Calculate required stake for validator based on VC
    pub fn calculate_required_stake(&self, total_vc: u64, current_epoch: EpochNumber) -> Balance {
        self.stake_requirement_config.calculate_required_stake(
            total_vc,
            current_epoch,
            &self.bootstrap_config,
        )
    }

    /// Check if validator meets stake requirement
    pub fn meets_stake_requirement(
        &self,
        validator_stake: Balance,
        total_vc: u64,
        current_epoch: EpochNumber,
    ) -> bool {
        self.stake_requirement_config.meets_requirement(
            validator_stake,
            total_vc,
            current_epoch,
            &self.bootstrap_config,
        )
    }

    /// Get stake deficiency if below requirement
    pub fn get_stake_deficiency(
        &self,
        validator_stake: Balance,
        total_vc: u64,
        current_epoch: EpochNumber,
    ) -> Option<Balance> {
        self.stake_requirement_config.stake_deficiency(
            validator_stake,
            total_vc,
            current_epoch,
            &self.bootstrap_config,
        )
    }

    /// Get stake requirement config
    pub fn stake_requirement_config(&self) -> &StakeRequirementConfig {
        &self.stake_requirement_config
    }
}

impl Default for EconomicsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_era() {
        let config = BootstrapConfig::default_config();

        // Epoch 0: in bootstrap
        // SPEC v2.2: Bootstrap floor is 50K KRAT (was 100K)
        assert!(config.is_bootstrap(0));
        assert_eq!(config.get_min_stake(0), 50_000 * KRAT);

        // Epoch 1439: still in bootstrap (1440 epochs total per SPEC v2.3)
        assert!(config.is_bootstrap(1439));

        // Epoch 1440: bootstrap ended (time-based exit)
        // SPEC v2.2: Post-bootstrap floor is 25K KRAT (was 50K)
        assert!(!config.is_bootstrap(1440));
        assert_eq!(config.get_min_stake(1440), 25_000 * KRAT);
    }

    #[test]
    fn test_bootstrap_exit_conditions() {
        let config = BootstrapConfig::default_config();

        // Test time-based exit (1440 epochs) - REQUIRES minimum validators
        // With 50 validators at epoch 1439, bootstrap exits early because validator threshold is met
        assert!(config.should_exit_bootstrap(1439, 50, 0));  // 50 validators = exit even before 1440
        assert!(config.should_exit_bootstrap(1440, 50, 0));  // Epoch 1440 + 50 validators, exits

        // Test validator-based exit (50 validators) - this is the safety requirement
        assert!(!config.should_exit_bootstrap(100, 49, 0)); // 49 validators, still in bootstrap
        assert!(config.should_exit_bootstrap(100, 50, 0));  // 50 validators, exits bootstrap

        // Test that time alone is not enough - must have validators
        assert!(!config.should_exit_bootstrap(1440, 49, 0)); // 1440 epochs but only 49 validators = no exit

        // Test stake-based exit - but still needs minimum validators
        assert!(!config.should_exit_bootstrap(100, 49, 50_000_000)); // High stake but 49 validators = no exit
        assert!(config.should_exit_bootstrap(100, 50, 50_000_000));  // High stake + 50 validators = exit

        // Test comprehensive check
        assert!(config.is_bootstrap_with_state(100, 10, 1_000_000));  // Early, few validators, low stake
        assert!(!config.is_bootstrap_with_state(100, 50, 1_000_000)); // 50 validators triggers exit
    }

    #[test]
    fn test_bootstrap_safety_constraint() {
        let config = BootstrapConfig::default_config();

        // CRITICAL: Cannot exit bootstrap with insufficient validators
        // Even if 1440 epochs have passed, network stays in bootstrap if < 50 validators
        assert!(!config.should_exit_bootstrap(1440, 23, 0));  // 1440 epochs but only 23 validators
        assert!(!config.should_exit_bootstrap(2000, 49, 0)); // 2000 epochs but only 49 validators
        assert!(!config.should_exit_bootstrap(1440, 0, 100_000_000)); // High stake but 0 validators

        // Only exits when validator minimum is met
        assert!(config.should_exit_bootstrap(1440, 50, 0));   // 1440 epochs AND 50 validators
        assert!(config.should_exit_bootstrap(100, 50, 0));   // Before 1440 but 50 validators reached
    }

    #[test]
    fn test_extended_bootstrap() {
        let config = BootstrapConfig::default_config();

        // Not extended bootstrap during normal period
        assert!(!config.is_extended_bootstrap(100, 23));
        assert!(!config.is_extended_bootstrap(1439, 23));

        // Extended bootstrap after 1440 epochs with insufficient validators
        assert!(config.is_extended_bootstrap(1440, 23));  // 23 < 50 validators
        assert!(config.is_extended_bootstrap(1440, 49));  // 49 < 50 validators
        assert!(config.is_extended_bootstrap(2000, 30)); // Still not enough after 2000 epochs

        // Not extended if validators met
        assert!(!config.is_extended_bootstrap(1440, 50));  // Exactly 50 validators
        assert!(!config.is_extended_bootstrap(2000, 75)); // More than enough
    }

    #[test]
    fn test_bootstrap_status() {
        let config = BootstrapConfig::default_config();

        // Normal bootstrap
        let status = config.get_bootstrap_status(100, 23, 0);
        match status {
            BootstrapStatus::Active { epochs_remaining, validators_needed } => {
                assert_eq!(epochs_remaining, 1340); // 1440 - 100
                assert_eq!(validators_needed, 27);  // 50 - 23
            }
            _ => panic!("Expected Active status"),
        }

        // Extended bootstrap (time passed but not enough validators)
        let status = config.get_bootstrap_status(1600, 23, 0);
        match status {
            BootstrapStatus::Extended { epochs_overdue, validators_needed } => {
                assert_eq!(epochs_overdue, 160);     // 1600 - 1440
                assert_eq!(validators_needed, 27);  // 50 - 23
            }
            _ => panic!("Expected Extended status"),
        }

        // Completed (enough validators)
        let status = config.get_bootstrap_status(1600, 50, 0);
        assert_eq!(status, BootstrapStatus::Completed);

        // Also completed if under 1440 but 50 validators
        let status = config.get_bootstrap_status(100, 50, 0);
        assert_eq!(status, BootstrapStatus::Completed);
    }

    #[test]
    fn test_vc_multipliers() {
        let config = BootstrapConfig::default_config();

        // During bootstrap: 2x multiplier
        assert_eq!(config.get_vote_multiplier(0), 2);
        assert_eq!(config.get_uptime_multiplier(0), 2);

        // After bootstrap: 1x multiplier (1440 epochs)
        assert_eq!(config.get_vote_multiplier(1440), 1);
        assert_eq!(config.get_uptime_multiplier(1440), 1);
    }

    #[test]
    fn test_base_security_budget() {
        let config = InflationConfig::default();
        let calculator = InflationCalculator::new(config);

        let base = calculator.calculate_base_security_budget();
        // 100 validators × 10k KRAT = 1M KRAT
        assert_eq!(base, 1_000_000 * KRAT);
    }

    #[test]
    fn test_security_gap_factor() {
        let config = InflationConfig::default();
        let calculator = InflationCalculator::new(config);

        // Under-secured network (10% staked, target 30%)
        let metrics_undersecured = NetworkMetrics {
            total_supply: 100_000_000,
            total_staked: 10_000_000,
            active_validators: 50,
            active_users: 5_000,
            transactions_count: 10_000,
        };

        let factor = calculator.calculate_security_gap_factor(&metrics_undersecured);
        // 0.30 / 0.10 = 3.0 → clamped to 1.5
        assert_eq!(factor, 1.5);

        // Over-secured network (50% staked, target 30%)
        let metrics_oversecured = NetworkMetrics {
            total_supply: 100_000_000,
            total_staked: 50_000_000,
            active_validators: 150,
            active_users: 20_000,
            transactions_count: 50_000,
        };

        let factor = calculator.calculate_security_gap_factor(&metrics_oversecured);
        // 0.30 / 0.50 = 0.6
        assert!(factor >= 0.59 && factor <= 0.61);
    }

    #[test]
    fn test_activity_factor() {
        let config = InflationConfig::default();
        let calculator = InflationCalculator::new(config);

        // Low activity (1000 users, target 10000)
        let metrics_low = NetworkMetrics {
            total_supply: 100_000_000,
            total_staked: 30_000_000,
            active_validators: 100,
            active_users: 1_000,
            transactions_count: 5_000,
        };

        let factor = calculator.calculate_activity_factor(&metrics_low);
        // sqrt(1000/10000) = sqrt(0.1) ≈ 0.316 → clamped to 0.5
        assert_eq!(factor, 0.5);

        // High activity (50000 users, target 10000)
        let metrics_high = NetworkMetrics {
            total_supply: 100_000_000,
            total_staked: 30_000_000,
            active_validators: 100,
            active_users: 50_000,
            transactions_count: 100_000,
        };

        let factor = calculator.calculate_activity_factor(&metrics_high);
        // sqrt(50000/10000) = sqrt(5) ≈ 2.236 → clamped to 1.2
        assert_eq!(factor, 1.2);
    }

    #[test]
    fn test_annual_emission_calculation() {
        let config = InflationConfig::default();
        let calculator = InflationCalculator::new(config);

        let metrics = NetworkMetrics {
            total_supply: 100_000_000 * KRAT, // 100M KRAT
            total_staked: 30_000_000 * KRAT,  // 30% staked (target)
            active_validators: 100,
            active_users: 10_000, // Target users
            transactions_count: 50_000,
        };

        let emission = calculator.calculate_annual_emission(&metrics);

        // Base: 1M KRAT
        // Security factor: 1.0 (perfect staking)
        // Activity factor: 1.0 (target users)
        // Expected: ~1M KRAT
        assert!(emission >= 900_000 * KRAT && emission <= 1_100_000 * KRAT);
    }

    #[test]
    fn test_inflation_rate() {
        let config = InflationConfig::default();
        let calculator = InflationCalculator::new(config);

        let metrics = NetworkMetrics {
            total_supply: 100_000_000 * KRAT,
            total_staked: 30_000_000 * KRAT,
            active_validators: 100,
            active_users: 10_000,
            transactions_count: 50_000,
        };

        let rate = calculator.calculate_inflation_rate(&metrics);

        // Should be around 1% (1M KRAT / 100M KRAT)
        assert!(rate >= 0.008 && rate <= 0.012);
    }

    #[test]
    fn test_fee_distribution() {
        let dist = FeeDistribution::default_distribution();

        assert!(dist.validate());

        let (validators, burn, treasury) = dist.distribute(1_000_000);

        assert_eq!(validators, 600_000); // 60%
        assert_eq!(burn, 300_000);       // 30%
        assert_eq!(treasury, 100_000);   // 10%
    }

    #[test]
    fn test_economics_manager() {
        let manager = EconomicsManager::new();

        // Check bootstrap (SPEC v2.3: bootstrap ends at epoch 1440)
        assert!(manager.is_bootstrap(0));
        assert!(manager.is_bootstrap(1439)); // Still in bootstrap
        assert!(!manager.is_bootstrap(1440)); // Bootstrap ends at 1440

        // Check min stake (SPEC v2.2: lowered stakes)
        assert_eq!(manager.get_min_stake(0), 50_000 * KRAT); // Bootstrap: 50k (was 100k)
        assert_eq!(manager.get_min_stake(1439), 50_000 * KRAT); // Still bootstrap
        assert_eq!(manager.get_min_stake(1440), 25_000 * KRAT); // Post-bootstrap: 25k (was 50k)

        // Check VC multipliers
        assert_eq!(manager.apply_vote_multiplier(1, 0), 2); // Bootstrap: 2x
        assert_eq!(manager.apply_vote_multiplier(1, 1439), 2); // Still bootstrap
        assert_eq!(manager.apply_vote_multiplier(1, 1440), 1); // Post-bootstrap: 1x
    }

    #[test]
    fn test_inflation_bounds() {
        let config = InflationConfig::default();
        let calculator = InflationCalculator::new(config);

        // Extreme under-secured, low activity
        let metrics_min = NetworkMetrics {
            total_supply: 100_000_000,
            total_staked: 1_000_000, // 1% staked
            active_validators: 10,
            active_users: 100, // Very low
            transactions_count: 10,
        };

        let emission = calculator.calculate_annual_emission(&metrics_min);
        let rate = emission as f64 / metrics_min.total_supply as f64;

        // Should be capped at max_inflation (10%)
        assert!(rate <= 0.10);

        // Over-secured, no activity
        let metrics_max = NetworkMetrics {
            total_supply: 100_000_000,
            total_staked: 80_000_000, // 80% staked
            active_validators: 200,
            active_users: 0, // No activity
            transactions_count: 0,
        };

        let emission = calculator.calculate_annual_emission(&metrics_max);
        let rate = emission as f64 / metrics_max.total_supply as f64;

        // Should be at min_inflation (0.5%)
        assert!(rate >= 0.005);
    }

    #[test]
    fn test_stake_reduction_zero_vc() {
        let stake_config = StakeRequirementConfig::default();
        let bootstrap_config = BootstrapConfig::default_config();

        // SPEC v2.2: With 0 VC, required stake should be nominal stake (500K KRAT)
        let required = stake_config.calculate_required_stake(0, 0, &bootstrap_config);
        assert_eq!(required, 500_000 * KRAT);
    }

    #[test]
    fn test_stake_reduction_partial_vc() {
        let stake_config = StakeRequirementConfig::default();
        let bootstrap_config = BootstrapConfig::default_config();

        // SPEC v2.2: With 500 VC (10% of target 5000):
        // VC_norm = 500 / 5000 = 0.1
        // StakeReduction = 0.99 × 0.1 = 0.099
        // RequiredStake = 500K × (1 - 0.099) = 450,500 KRAT
        let required = stake_config.calculate_required_stake(500, 0, &bootstrap_config);

        let expected = 450_500 * KRAT;
        let tolerance = 1_000 * KRAT;
        assert!(
            required >= expected - tolerance && required <= expected + tolerance,
            "Expected ~{}, got {}",
            expected,
            required
        );
    }

    #[test]
    fn test_stake_reduction_half_target_vc() {
        let stake_config = StakeRequirementConfig::default();
        let bootstrap_config = BootstrapConfig::default_config();

        // SPEC v2.2: With 2500 VC (50% of target 5000):
        // VC_norm = 2500 / 5000 = 0.5
        // StakeReduction = 0.99 × 0.5 = 0.495
        // RequiredStake = 500K × (1 - 0.495) = 252,500 KRAT
        let required = stake_config.calculate_required_stake(2_500, 0, &bootstrap_config);

        let expected = 252_500 * KRAT;
        let tolerance = 5_000 * KRAT;
        assert!(
            required >= expected - tolerance && required <= expected + tolerance,
            "Expected ~{}, got {}",
            expected,
            required
        );
    }

    #[test]
    fn test_stake_reduction_full_vc() {
        let stake_config = StakeRequirementConfig::default();
        let bootstrap_config = BootstrapConfig::default_config();

        // SPEC v2.2: With 5000 VC (100% of target):
        // VC_norm = 5000 / 5000 = 1.0
        // StakeReduction = 0.99 × 1.0 = 0.99
        // RequiredStake = 500K × (1 - 0.99) = 5,000 KRAT
        // But floor is 50k during bootstrap, so required = 50k
        let required = stake_config.calculate_required_stake(5_000, 0, &bootstrap_config);

        assert_eq!(required, 50_000 * KRAT); // Floor applies
    }

    #[test]
    fn test_stake_reduction_exceeds_target() {
        let stake_config = StakeRequirementConfig::default();
        let bootstrap_config = BootstrapConfig::default_config();

        // SPEC v2.2: With 10,000 VC (200% of target):
        // VC_norm = min(10000 / 5000, 1.0) = 1.0 (capped)
        // Same as 5000 VC: required = floor = 50k
        let required = stake_config.calculate_required_stake(10_000, 0, &bootstrap_config);

        assert_eq!(required, 50_000 * KRAT); // Floor applies
    }

    #[test]
    fn test_stake_reduction_bootstrap_vs_post_bootstrap() {
        let stake_config = StakeRequirementConfig::default();
        let bootstrap_config = BootstrapConfig::default_config();

        // SPEC v2.2: With 5000 VC:
        // Bootstrap: max_reduction = 99%, floor = 50k
        // Bootstrap ends at epoch 1440
        let required_bootstrap = stake_config.calculate_required_stake(5_000, 0, &bootstrap_config);
        assert_eq!(required_bootstrap, 50_000 * KRAT);

        // Still in bootstrap at epoch 1439
        let required_bootstrap_1439 = stake_config.calculate_required_stake(5_000, 1439, &bootstrap_config);
        assert_eq!(required_bootstrap_1439, 50_000 * KRAT);

        // Post-bootstrap: max_reduction = 95%, floor = 25k
        // StakeReduction = 0.95 × 1.0 = 0.95
        // RequiredStake = 500K × (1 - 0.95) = 25,000 KRAT
        // Note: Small floating-point rounding may occur
        let required_post = stake_config.calculate_required_stake(5_000, 1440, &bootstrap_config);
        let expected_post = 25_000 * KRAT;
        let tolerance = 100; // Allow tiny rounding errors
        assert!(
            (required_post as i128 - expected_post as i128).abs() < tolerance,
            "Expected ~{}, got {}",
            expected_post,
            required_post
        );
    }

    #[test]
    fn test_stake_requirement_meets() {
        let stake_config = StakeRequirementConfig::default();
        let bootstrap_config = BootstrapConfig::default_config();

        // SPEC v2.2: Validator with 300k KRAT stake and 500 VC
        // Required: ~450,500 KRAT
        let meets = stake_config.meets_requirement(300_000 * KRAT, 500, 0, &bootstrap_config);
        assert!(!meets); // 300k < 450.5k

        // Validator with 475k KRAT stake and 500 VC
        let meets = stake_config.meets_requirement(475_000 * KRAT, 500, 0, &bootstrap_config);
        assert!(meets); // 475k > 450.5k
    }

    #[test]
    fn test_stake_deficiency() {
        let stake_config = StakeRequirementConfig::default();
        let bootstrap_config = BootstrapConfig::default_config();

        // SPEC v2.2: Validator with 300k KRAT stake and 500 VC
        // Required: ~450,500 KRAT
        // Deficiency: ~150,500 KRAT
        let deficiency = stake_config.stake_deficiency(300_000 * KRAT, 500, 0, &bootstrap_config);
        assert!(deficiency.is_some());

        let deficit = deficiency.unwrap();
        let expected_deficit = 150_500 * KRAT;
        let tolerance = 5_000 * KRAT;
        assert!(
            deficit >= expected_deficit - tolerance && deficit <= expected_deficit + tolerance,
            "Expected deficit ~{}, got {}",
            expected_deficit,
            deficit
        );

        // Validator with sufficient stake
        let deficiency = stake_config.stake_deficiency(475_000 * KRAT, 500, 0, &bootstrap_config);
        assert!(deficiency.is_none());
    }

    #[test]
    fn test_economics_manager_stake_calculation() {
        let manager = EconomicsManager::new();

        // SPEC v2.2: Test calculation through manager
        // With 2500 VC (50% of target): Required = 500K × (1 - 0.495) = 252,500 KRAT
        let required = manager.calculate_required_stake(2_500, 0);

        let expected = 252_500 * KRAT;
        let tolerance = 5_000 * KRAT;
        assert!(
            required >= expected - tolerance && required <= expected + tolerance,
            "Expected ~{}, got {}",
            expected,
            required
        );

        // Test meets requirement
        assert!(!manager.meets_stake_requirement(200_000 * KRAT, 2_500, 0));
        assert!(manager.meets_stake_requirement(300_000 * KRAT, 2_500, 0));

        // Test deficiency
        let deficiency = manager.get_stake_deficiency(200_000 * KRAT, 2_500, 0);
        assert!(deficiency.is_some());
    }

    #[test]
    fn test_spec_v22_examples() {
        // SPEC v2.2: Updated examples with lowered stake requirements
        // Nominal: 500K KRAT (was 1M), Floor: 25K KRAT (was 50K)
        let stake_config = StakeRequirementConfig::default();
        let bootstrap_config = BootstrapConfig::default_config();

        // Example 1: 0 VC → 500,000 KRAT (was 1M)
        let required = stake_config.calculate_required_stake(0, 0, &bootstrap_config);
        assert_eq!(required, 500_000 * KRAT);

        // Example 2: 500 VC → ~450,500 KRAT (was ~905K)
        let required = stake_config.calculate_required_stake(500, 0, &bootstrap_config);
        assert!(
            required >= 445_000 * KRAT && required <= 455_000 * KRAT,
            "Expected ~450.5k, got {}",
            required
        );

        // Example 3: 2500 VC → ~252,500 KRAT (was ~525K)
        let required = stake_config.calculate_required_stake(2_500, 0, &bootstrap_config);
        assert!(
            required >= 250_000 * KRAT && required <= 260_000 * KRAT,
            "Expected ~252.5k, got {}",
            required
        );

        // Example 4: 5000+ VC → floor
        // Bootstrap floor is 50k, post-bootstrap is 25k
        let required_bootstrap = stake_config.calculate_required_stake(5_000, 0, &bootstrap_config);
        assert_eq!(required_bootstrap, 50_000 * KRAT);

        let required_bootstrap_1439 = stake_config.calculate_required_stake(5_000, 1439, &bootstrap_config);
        assert_eq!(required_bootstrap_1439, 50_000 * KRAT); // Still in bootstrap

        // Note: Small floating-point rounding may occur
        let required_post = stake_config.calculate_required_stake(5_000, 1440, &bootstrap_config);
        let expected_post = 25_000 * KRAT;
        let tolerance = 100; // Allow tiny rounding errors
        assert!(
            (required_post as i128 - expected_post as i128).abs() < tolerance,
            "Expected ~{}, got {} (Post-bootstrap)",
            expected_post,
            required_post
        );
    }

    // =========================================================================
    // SECURITY DEGRADED MODE TESTS
    // =========================================================================

    #[test]
    fn test_security_state_tracker_initial() {
        let tracker = SecurityStateTracker::new(50, DegradedSecurityConfig::default());

        assert!(tracker.is_bootstrap());
        assert!(!tracker.is_normal());
        assert!(!tracker.is_degraded());
        assert!(!tracker.bootstrap_completed);
    }

    #[test]
    fn test_security_state_bootstrap_to_normal() {
        // SPEC v6.3: Bootstrap exit requires:
        // - Epoch >= 1440 (BOOTSTRAP_EPOCHS_MIN)
        // - Validators >= 50 (V_MIN)
        // No finality requirement in v6.3
        let mut tracker = SecurityStateTracker::new(50, ValidatorScarcityConfig::default());
        let bootstrap_config = BootstrapConfig::default_config();

        // Still in bootstrap - epoch too low (even with enough validators)
        let result = tracker.update_with_finality(100, 50, &bootstrap_config, 0, 100);
        assert!(result.is_none());
        assert!(tracker.is_bootstrap());

        // Still in bootstrap - validators too low (even with correct epoch)
        let result = tracker.update_with_finality(1440, 30, &bootstrap_config, 0, 100);
        assert!(result.is_none());
        assert!(tracker.is_bootstrap());

        // SPEC v6.3: No finality requirement - exits with epoch and validators
        // With epoch >= 1440 and validators >= 50, exits regardless of finality
        let result = tracker.update_with_finality(1441, 50, &bootstrap_config, 0, 90);
        assert!(result.is_some()); // Exits in v6.3 - no finality requirement
        assert!(tracker.is_normal());
        assert!(tracker.bootstrap_completed);

        // Verify at least one transition was recorded (Bootstrap -> Healthy)
        // SPEC v6.5: Normal is renamed to Healthy
        assert!(tracker.state_transitions.len() >= 1);
        // Find the Bootstrap -> Healthy transition
        let bootstrap_to_healthy = tracker.state_transitions.iter()
            .find(|t| t.from_state == "Bootstrap" && t.to_state == "Healthy");
        assert!(bootstrap_to_healthy.is_some(), "Expected Bootstrap -> Healthy transition");
    }

    #[test]
    fn test_security_state_normal_to_vss_grace_period() {
        // SPEC v6.2 §5.1: Enter VSS when validators < MIN
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 3,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Drop below minimum - should not immediately enter VSS
        tracker.update(800, 40, &bootstrap_config, 0);
        assert!(tracker.is_normal()); // Still normal, grace period

        tracker.update(801, 40, &bootstrap_config, 0);
        assert!(tracker.is_normal()); // Still normal, grace period

        // Third epoch below minimum - enters VSS
        let result = tracker.update(802, 40, &bootstrap_config, 0);
        assert!(result.is_some());
        assert!(tracker.is_vss());
        assert!(tracker.is_degraded());

        // Verify DSM state details
        if let NetworkSecurityState::DegradedSecurityMode { entered_at, validators_needed, .. } = &tracker.state {
            assert_eq!(*entered_at, 802);
            assert_eq!(*validators_needed, 10); // 50 - 40
        } else {
            panic!("Expected DegradedSecurityMode");
        }
    }

    #[test]
    fn test_security_state_grace_period_reset() {
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 3,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Two epochs below minimum
        tracker.update(800, 40, &bootstrap_config, 0);
        tracker.update(801, 40, &bootstrap_config, 0);
        assert!(tracker.is_normal());
        assert_eq!(tracker.epochs_below_minimum, 2);

        // Validators recover - grace period resets
        tracker.update(802, 55, &bootstrap_config, 0);
        assert!(tracker.is_normal());
        assert_eq!(tracker.epochs_below_minimum, 0);

        // Drop again - needs full grace period again
        tracker.update(803, 40, &bootstrap_config, 0);
        tracker.update(804, 40, &bootstrap_config, 0);
        assert!(tracker.is_normal()); // Still in grace period
    }

    #[test]
    fn test_security_state_vss_to_normal_recovery() {
        // SPEC v7.1 §7.1: Recovery requires consecutive epochs at SafeValidators (75)
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            normal_recovery_epochs: 3, // Use 3 for test
            dsm_recovery_epochs: 3,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter Degraded (validators < SafeValidators = 75)
        tracker.update(800, 60, &bootstrap_config, 0);
        assert!(tracker.is_vss());

        // Validators recover to SafeValidators - counting consecutive epochs
        tracker.update(801, SAFE_VALIDATORS, &bootstrap_config, 0);
        assert!(tracker.is_vss()); // Still in Degraded, counting
        assert_eq!(tracker.epochs_above_minimum_in_vss(), 1);

        tracker.update(802, SAFE_VALIDATORS, &bootstrap_config, 0);
        assert!(tracker.is_vss()); // Still counting

        // Third epoch at SafeValidators - exits to Normal
        let result = tracker.update(803, SAFE_VALIDATORS, &bootstrap_config, 0);
        assert!(result.is_some());
        assert!(tracker.is_normal());
    }

    #[test]
    fn test_security_state_recovery_reset() {
        // SPEC v7.1 §7.1: Consecutive epochs reset if validators drop below SafeValidators
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            normal_recovery_epochs: 3,
            dsm_recovery_epochs: 3,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter Degraded (validators < SafeValidators = 75)
        tracker.update(800, 60, &bootstrap_config, 0);
        assert!(tracker.is_vss());

        // Start recovery - at SafeValidators
        tracker.update(801, SAFE_VALIDATORS, &bootstrap_config, 0);
        tracker.update(802, SAFE_VALIDATORS, &bootstrap_config, 0);
        assert_eq!(tracker.epochs_above_minimum_in_vss(), 2);

        // Drop again below SafeValidators - consecutive count resets
        tracker.update(803, 60, &bootstrap_config, 0);
        assert!(tracker.is_vss());
        assert_eq!(tracker.epochs_above_minimum_in_vss(), 0);
    }

    #[test]
    fn test_governance_restrictions_in_vss() {
        // SPEC v6.4 §3.3: Governance timelocks doubled in DSM
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            dsm_governance_timelock_multiplier: 2,
            shm_no_governance: true,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // In normal mode, all governance allowed
        assert!(tracker.is_governance_allowed(true));  // Constitutional
        assert!(tracker.is_governance_allowed(false)); // Non-constitutional

        // Enter DegradedSecurityMode
        tracker.update(800, 40, &bootstrap_config, 0);
        assert!(tracker.is_vss());

        // SPEC v6.4 §3.3: Governance allowed but timelocks doubled in DSM
        assert!(tracker.is_governance_allowed(true));  // Allowed with longer timelock
        assert!(tracker.is_governance_allowed(false)); // Allowed with longer timelock
        assert_eq!(tracker.get_governance_timelock_multiplier(), 2);
    }

    #[test]
    fn test_inflation_in_degraded() {
        // SPEC v7.1 §5.2: Inflation capped in DegradedSecurityMode
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            dsm_inflation_capped: true,
            shm_no_governance: true,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // In normal mode, inflation changes allowed
        assert!(tracker.is_inflation_change_allowed());

        // Enter DegradedSecurityMode (validators < SafeValidators = 75)
        tracker.update(800, 60, &bootstrap_config, 0);
        assert!(!tracker.is_inflation_change_allowed()); // Capped per v7.1

        // In Emergency mode (validators < EmergencyValidators = 25), no changes allowed
        tracker.update(801, 20, &bootstrap_config, 0); // Drop to Emergency
        assert!(tracker.is_emergency_mode());
        assert!(!tracker.is_inflation_change_allowed());
    }

    #[test]
    fn test_reward_boosts_in_dsm() {
        // SPEC v6.4: Validator rewards not boosted in DSM (only in BootstrapRecoveryMode)
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // In normal mode, no boosts
        assert_eq!(tracker.get_validator_reward_boost(), 0);

        // Enter DSM - no boost in DSM per SPEC v6.4
        tracker.update(800, 40, &bootstrap_config, 0);
        assert!(tracker.is_vss());
        assert_eq!(tracker.get_validator_reward_boost(), 0); // No boost in DSM
    }

    #[test]
    fn test_block_time_in_dsm() {
        // SPEC v6.4 §3.1: Block time x2 in DSM
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            dsm_block_time_multiplier: 2,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // In normal mode, no increase
        assert_eq!(tracker.get_block_time_increase_percent(), 0);
        assert_eq!(tracker.get_block_time_multiplier(), 1);

        // Enter DSM
        tracker.update(800, 40, &bootstrap_config, 0);

        // In SPEC v6.4, block time is doubled
        assert_eq!(tracker.get_block_time_multiplier(), 2);
    }

    #[test]
    fn test_health_status_healthy() {
        let tracker = SecurityStateTracker::post_bootstrap(50, ValidatorScarcityConfig::default());

        let status = tracker.get_health_status(0);
        assert_eq!(status, NetworkHealthStatus::Healthy);
    }

    #[test]
    fn test_health_status_bootstrapping() {
        let tracker = SecurityStateTracker::new(50, ValidatorScarcityConfig::default());

        let status = tracker.get_health_status(0);
        match status {
            NetworkHealthStatus::Bootstrapping { validators_needed, .. } => {
                assert_eq!(validators_needed, 50);
            }
            _ => panic!("Expected Bootstrapping status"),
        }
    }

    // =============================================================================
    // SPEC v6.4 TESTS - DegradedSecurityMode & SafetyHaltMode
    // =============================================================================

    #[test]
    fn test_health_status_degraded() {
        // SPEC v6.4 §3: DegradedSecurityMode State
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            dsm_governance_timelock_multiplier: 2,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter DegradedSecurityMode
        tracker.update(800, 40, &bootstrap_config, 0);

        let status = tracker.get_health_status(800);
        match status {
            NetworkHealthStatus::DegradedSecurityMode {
                severity,
                validators_current,
                validators_needed,
                effects_active,
                ..
            } => {
                assert_eq!(severity, SecuritySeverity::Warning);
                assert_eq!(validators_current, 40);
                assert_eq!(validators_needed, 10);
                assert!(!effects_active.is_empty());
            }
            _ => panic!("Expected DegradedSecurityMode status, got {:?}", status),
        }
    }

    #[test]
    fn test_health_status_recovering() {
        // SPEC v7.1 §7.1: Recovery requires consecutive epochs at SafeValidators
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            normal_recovery_epochs: 5, // Use 5 for testing
            dsm_recovery_epochs: 5,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter DegradedSecurityMode (validators < SafeValidators = 75)
        tracker.update(800, 60, &bootstrap_config, 0);

        // Start recovery - validators at SafeValidators
        tracker.update(801, SAFE_VALIDATORS, &bootstrap_config, 0);

        let status = tracker.get_health_status(801);
        match status {
            NetworkHealthStatus::Recovering { epochs_until_normal, epochs_consecutive, validators_current } => {
                assert_eq!(epochs_until_normal, 4); // 5 - 1
                assert_eq!(epochs_consecutive, 1);
                assert_eq!(validators_current, SAFE_VALIDATORS);
            }
            _ => panic!("Expected Recovering status"),
        }
    }

    #[test]
    fn test_security_severity_critical() {
        // SPEC v6.5 §4.3: Critical when < V_min_absolute (21)
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter with very few validators (< 21 = V_min_absolute)
        tracker.update(800, 20, &bootstrap_config, 0);

        let status = tracker.get_health_status(800);
        match status {
            NetworkHealthStatus::Critical { validators_current, absolute_minimum, .. } => {
                // SPEC v6.5: 20 < 21 (V_min_absolute), so enters Critical (SafetyHaltMode)
                assert_eq!(validators_current, 20);
                assert_eq!(absolute_minimum, V_MIN_ABSOLUTE);
            }
            NetworkHealthStatus::DegradedSecurityMode { severity, .. } => {
                // If 20 >= V_min_absolute, it's in DegradedSecurityMode with Critical severity
                assert_eq!(severity, SecuritySeverity::Critical);
            }
            _ => panic!("Expected Critical or DegradedSecurityMode status"),
        }
    }

    #[test]
    fn test_degraded_info() {
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            dsm_governance_timelock_multiplier: 2,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Not in degraded - no info
        assert!(tracker.get_degraded_info().is_none());

        // Enter DSM
        tracker.update(800, 40, &bootstrap_config, 0);

        let info = tracker.get_degraded_info().unwrap();
        assert_eq!(info.entered_at, 800);
        assert_eq!(info.current_validators, 40);
        assert_eq!(info.validators_needed, 10);
        assert_eq!(info.mode, DegradedMode::DegradedSecurityMode);
        assert!(info.epochs_until_recovery.is_none()); // Not recovering yet
    }

    #[test]
    fn test_validator_degraded_config_variants() {
        // Test default config per SPEC v7.1
        let default_config = ValidatorDegradedConfig::default();
        assert_eq!(default_config.floor_grace_epochs, 1); // Immediate entry
        assert_eq!(default_config.dsm_block_time_multiplier, 2); // SPEC v7.1: Degraded ×2
        assert_eq!(default_config.dsm_epoch_duration_multiplier, 2); // SPEC v7.1
        assert_eq!(default_config.dsm_governance_timelock_multiplier, 2); // SPEC v7.1
        assert!(default_config.dsm_slashing_tightened); // SPEC v7.1
        // SPEC v7.1 §5.2: Inflation is BOOSTED (not capped) in degraded states
        assert!(!default_config.dsm_inflation_capped); // v7.1: Inflation boosted, not capped
        assert_eq!(default_config.dsm_stake_reduction_percent, 30); // Reduced stake
        assert!(default_config.shm_no_slashing); // SPEC v7.1 §6
        // SPEC v7.1 §7.1: Recovery requires 100 epochs at SafeValidators
        assert_eq!(default_config.normal_recovery_epochs, NORMAL_RECOVERY_EPOCHS); // 100 epochs
        assert_eq!(default_config.critical_block_time_multiplier, 4); // SPEC v7.1: Critical ×4

        let lenient_config = ValidatorDegradedConfig::lenient();
        assert_eq!(lenient_config.dsm_recovery_epochs, 20); // Quick recovery (v7.1)
        assert_eq!(lenient_config.normal_recovery_epochs, 20); // Same as dsm_recovery_epochs
        assert_eq!(lenient_config.dsm_governance_timelock_multiplier, 1); // No extra timelock

        let strict_config = ValidatorDegradedConfig::strict();
        assert_eq!(strict_config.dsm_recovery_epochs, 150); // Longer recovery (v7.1)
        assert_eq!(strict_config.dsm_governance_timelock_multiplier, 3); // Triple timelock
    }

    #[test]
    fn test_state_transition_history_limit() {
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            dsm_recovery_epochs: 1, // Quick recovery for testing
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Generate many transitions
        for i in 0..60 {
            let epoch = 800 + i * 3;
            tracker.update(epoch, 40, &bootstrap_config, 0);     // Enter VSS
            tracker.update(epoch + 1, 55, &bootstrap_config, 0); // Start recovery
            tracker.update(epoch + 2, 55, &bootstrap_config, 0); // Exit to normal (1 epoch needed)
        }

        // Should be capped at 100 transitions
        assert!(tracker.state_transitions.len() <= 100);
    }

    #[test]
    fn test_full_lifecycle_bootstrap_normal_dsm_normal() {
        // SPEC v7.1 state machine: Bootstrap → Normal → Degraded → Normal
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            normal_recovery_epochs: 3, // Quick recovery for testing
            dsm_recovery_epochs: 3, // Backward compat alias
            dsm_governance_timelock_multiplier: 2,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::new(SAFE_VALIDATORS, config); // v7.1: SafeValidators=75
        let bootstrap_config = BootstrapConfig::default_config();

        // Phase 1: Bootstrap
        assert!(tracker.is_bootstrap());
        tracker.update_with_finality(100, 30, &bootstrap_config, 0, 95);
        assert!(tracker.is_bootstrap());

        // Phase 2: Exit Bootstrap → Normal (needs epoch >= 1440, validators >= min_validators)
        tracker.update_with_finality(1440, SAFE_VALIDATORS, &bootstrap_config, 0, 95);
        assert!(tracker.is_normal());

        // Phase 3: Normal operation (above SafeValidators)
        tracker.update(800, SAFE_VALIDATORS + 5, &bootstrap_config, 0);
        assert!(tracker.is_normal());

        // Phase 4: Validators drop below SafeValidators - enter Degraded
        tracker.update(900, 60, &bootstrap_config, 0); // 60 < 75 = enters Degraded
        assert!(tracker.is_vss());

        // Phase 5: In Degraded - verify effects (SPEC v7.1 §5.2)
        assert_eq!(tracker.get_block_time_multiplier(), 2); // Block time x2
        assert!(tracker.is_governance_allowed(true)); // Governance allowed with doubled timelock
        assert_eq!(tracker.get_governance_timelock_multiplier(), 2); // Timelocks doubled

        // Phase 6: Recovery starts - count consecutive epochs at SafeValidators
        tracker.update(1000, SAFE_VALIDATORS, &bootstrap_config, 0);
        assert!(tracker.is_vss()); // Still in Degraded, counting
        tracker.update(1001, SAFE_VALIDATORS, &bootstrap_config, 0);
        assert!(tracker.is_vss()); // Still in Degraded
        tracker.update(1002, SAFE_VALIDATORS, &bootstrap_config, 0);
        assert!(tracker.is_normal()); // Recovered after 3 consecutive epochs at SafeValidators

        // Phase 7: Back to normal
        assert_eq!(tracker.get_block_time_multiplier(), 1);
        assert!(tracker.is_governance_allowed(true));
        assert_eq!(tracker.get_governance_timelock_multiplier(), 1);

        // Verify transitions recorded
        assert!(tracker.state_transitions.len() >= 3);
    }

    #[test]
    fn test_invariants_in_vss() {
        // Test that all SPEC v7.1 §9 invariants are respected in Degraded
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter Degraded (validators < SafeValidators)
        tracker.update(800, 60, &bootstrap_config, 0); // 60 < 75
        assert!(tracker.is_vss());

        // INVARIANT #5 (SPEC v7.1 §9): Exit without permission
        assert!(tracker.is_fork_allowed());

        // INVARIANT: Recovery is automatic
        tracker.update(900, SAFE_VALIDATORS, &bootstrap_config, 0); // Recovery starts at SafeValidators
        assert!(tracker.epochs_above_minimum_in_vss() >= 1);

        // INVARIANT: Network continues
        assert!(tracker.get_block_time_multiplier() >= 1);
    }

    #[test]
    fn test_network_security_state_enum_v64() {
        // Test enum variants per SPEC v6.4
        let bootstrap = NetworkSecurityState::Bootstrap;
        let normal = NetworkSecurityState::Normal;
        let degraded = NetworkSecurityState::DegradedSecurityMode {
            entered_at: 100,
            epochs_in_dsm: 5,
            current_validators: 40,
            validators_needed: 10,
            consecutive_epochs_above_safe: 0,
        };
        let safety_halt = NetworkSecurityState::SafetyHaltMode {
            entered_at: 150,
            epochs_in_shm: 10,
            current_validators: 15,
            epochs_without_finality: 0, // SPEC v6.5
        };

        // Test equality
        assert_eq!(bootstrap.clone(), NetworkSecurityState::Bootstrap);
        assert_eq!(normal.clone(), NetworkSecurityState::Normal);
        assert_ne!(bootstrap, normal);

        // Test debug formatting
        let _ = format!("{:?}", bootstrap);
        let _ = format!("{:?}", normal);
        let _ = format!("{:?}", degraded);
        let _ = format!("{:?}", safety_halt);
    }

    #[test]
    fn test_dsm_to_shm_escalation() {
        // SPEC v7.1 §5.2: Escalate from Degraded to Restricted when validators < PostBootstrapMin
        // SPEC v7.1 §6.1: Escalate to Emergency when validators < EmergencyValidators
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter Degraded with 60 validators (between 50 and 75)
        tracker.update(800, 60, &bootstrap_config, 0);
        assert!(tracker.is_vss()); // In Degraded
        assert!(!tracker.is_emergency_mode());

        // Drop below PostBootstrapMin (50) - escalate to Restricted
        tracker.update(801, 30, &bootstrap_config, 0);
        assert!(tracker.is_shm()); // Now in Restricted (SafetyHaltMode)
        assert!(tracker.is_degraded());

        // Drop below EmergencyValidators (25) - escalate to Emergency
        tracker.update(802, 20, &bootstrap_config, 0);
        assert!(tracker.is_emergency_mode()); // Now in Emergency (TerminalMode)
        assert!(tracker.is_terminal());

        // Verify health status - SPEC v7.1: Terminal for Emergency state
        let status = tracker.get_health_status(802);
        match status {
            NetworkHealthStatus::Terminal { validators_current, fork_mandatory, .. } => {
                assert_eq!(validators_current, 20);
                assert!(fork_mandatory);
            }
            _ => panic!("Expected Terminal status for Emergency state"),
        }
    }

    #[test]
    fn test_shm_to_dsm_recovery() {
        // SPEC v7.1 §7.2: Exit Restricted when validators >= PostBootstrapMin for 1 full epoch
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            shm_recovery_epochs: 1,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // First enter Degraded (validators < SafeValidators = 75)
        tracker.update(800, 60, &bootstrap_config, 0);
        assert!(tracker.is_vss()); // In Degraded

        // Now drop to Restricted (validators < PostBootstrapMin = 50)
        tracker.update(801, 30, &bootstrap_config, 0); // 30 < 50
        assert!(tracker.is_shm()); // In Restricted

        // First recovery epoch - still in Restricted (need 1 full epoch at >= 50)
        tracker.update(802, 55, &bootstrap_config, 0); // Above PostBootstrapMin
        assert!(tracker.is_shm()); // Still in Restricted

        // Second epoch above PostBootstrapMin - exits to Degraded
        tracker.update(803, 55, &bootstrap_config, 0);
        assert!(tracker.is_vss()); // Now in Degraded
        assert!(!tracker.is_shm());

        // Verify in DegradedSecurityMode with correct values (needs 75 - 55 = 20)
        match &tracker.state {
            NetworkSecurityState::DegradedSecurityMode { validators_needed, .. } => {
                assert_eq!(*validators_needed, SAFE_VALIDATORS - 55); // 75 - 55 = 20
            }
            _ => panic!("Expected DegradedSecurityMode state"),
        }
    }

    #[test]
    fn test_dsm_recovery_with_consecutive_epochs() {
        // SPEC v7.1 §7.1: Recovery requires 100 consecutive epochs at SafeValidators (using 5 for test)
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            normal_recovery_epochs: 5, // Use 5 for testing
            dsm_recovery_epochs: 5, // Backward compat alias
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter Degraded (validators < SafeValidators)
        tracker.update(800, 60, &bootstrap_config, 0);
        assert!(tracker.is_vss());

        // Start recovery - 4 epochs at SafeValidators
        for i in 0..4 {
            let result = tracker.update(801 + i, SAFE_VALIDATORS, &bootstrap_config, 0);
            assert!(tracker.is_vss(), "Should still be in Degraded after {} epochs", i + 1);
            assert!(result.is_none() || tracker.is_vss()); // Still in Degraded
        }

        // 5th epoch at SafeValidators - should exit to Normal
        let result = tracker.update(805, SAFE_VALIDATORS, &bootstrap_config, 0);
        assert!(result.is_some());
        assert!(tracker.is_normal());
    }

    #[test]
    fn test_dsm_recovery_resets_on_drop() {
        // SPEC v7.1 §7.1: Consecutive epochs reset if validators drop below SafeValidators
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            normal_recovery_epochs: 5,
            dsm_recovery_epochs: 5,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter Degraded
        tracker.update(800, 60, &bootstrap_config, 0);

        // Start recovery - 3 epochs at SafeValidators
        for i in 0..3 {
            tracker.update(801 + i, SAFE_VALIDATORS, &bootstrap_config, 0);
        }
        assert!(tracker.epochs_above_minimum_in_vss() == 3);

        // Drop back below SafeValidators - resets counter
        tracker.update(804, 60, &bootstrap_config, 0);
        assert!(tracker.epochs_above_minimum_in_vss() == 0);
        assert!(tracker.is_vss());

        // Must start counting again at SafeValidators
        for i in 0..5 {
            tracker.update(805 + i, SAFE_VALIDATORS, &bootstrap_config, 0);
        }
        assert!(tracker.is_normal());
    }

    #[test]
    fn test_vc_multiplier_in_degraded() {
        // Test VC multiplier in DegradedSecurityMode
        // SPEC v6.4: VC multiplier only boosted in BootstrapRecoveryMode (2.0x)
        let mut tracker = SecurityStateTracker::post_bootstrap(50, ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            ..ValidatorScarcityConfig::default()
        });
        let bootstrap_config = BootstrapConfig::default_config();

        // Normal mode: 1.0x
        assert!((tracker.get_vc_multiplier() - 1.0).abs() < 0.01);

        // Enter DegradedSecurityMode - no boost in DSM per SPEC v6.4
        tracker.update(800, 40, &bootstrap_config, 0);
        assert!((tracker.get_vc_multiplier() - 1.0).abs() < 0.01); // No boost in DSM
    }

    #[test]
    fn test_bootstrap_exit_no_finality_requirement() {
        // SPEC v6.3 §3: Bootstrap exit only requires epoch >= 1440 AND validators >= 50
        // No finality rate requirement in v6.3
        let config = ValidatorScarcityConfig::default();
        let mut tracker = SecurityStateTracker::new(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Has validators and epoch - exits bootstrap (no finality check in v6.3)
        tracker.update_with_finality(1440, 50, &bootstrap_config, 0, 90);
        assert!(tracker.is_normal()); // Exits even with low finality in v6.3
    }

    #[test]
    fn test_bootstrap_unbounded() {
        // SPEC v6.3 §3.2: Bootstrap is unbounded - no forced exit, no extensions
        let config = ValidatorScarcityConfig::default();
        let mut tracker = SecurityStateTracker::new(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // At epoch 1440, not enough validators - stays in bootstrap
        tracker.update_with_finality(1440, 30, &bootstrap_config, 0, 95);
        assert!(tracker.is_bootstrap());

        // At epoch 2000, still not enough - continues in bootstrap (no forced exit)
        tracker.update_with_finality(2000, 30, &bootstrap_config, 0, 95);
        assert!(tracker.is_bootstrap());

        // Finally enough validators - exits
        tracker.update_with_finality(2001, 50, &bootstrap_config, 0, 95);
        assert!(tracker.is_normal());
    }

    #[test]
    fn test_dsm_effects() {
        // SPEC v6.4 §3: Verify DegradedSecurityMode effects
        let config = ValidatorScarcityConfig::default();
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Normal - no effects
        assert_eq!(tracker.get_block_time_multiplier(), 1); // Normal block time
        assert_eq!(tracker.get_validator_reward_boost(), 0);
        assert_eq!(tracker.get_slashing_reduction(), 0);
        assert!(!tracker.is_fast_track_onboarding());

        // Enter DegradedSecurityMode
        tracker.update(800, 40, &bootstrap_config, 0);

        // Verify effects active (SPEC v6.4 §3)
        assert_eq!(tracker.get_block_time_multiplier(), 2); // Block time x2
        assert_eq!(tracker.get_validator_reward_boost(), 0); // No boost in DSM per v6.4
        assert_eq!(tracker.get_slashing_reduction(), 0); // Slashing tightened in v6.4
        assert!(tracker.is_fast_track_onboarding());
    }

    #[test]
    fn test_governance_constraints_in_dsm() {
        // SPEC v6.4 §3.3: Governance timelocks doubled in DSM
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            dsm_governance_timelock_multiplier: 2,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Normal - governance allowed, normal timelocks
        assert!(tracker.is_governance_allowed(true));
        assert!(tracker.is_governance_allowed(false));
        assert_eq!(tracker.get_governance_timelock_multiplier(), 1);

        // Enter DegradedSecurityMode
        tracker.update(800, 40, &bootstrap_config, 0);

        // SPEC v6.4 §3.3: Governance allowed but timelocks doubled
        assert!(tracker.is_governance_allowed(true));
        assert!(tracker.is_governance_allowed(false));
        assert_eq!(tracker.get_governance_timelock_multiplier(), 2);
    }

    #[test]
    fn test_governance_fully_blocked_in_shm() {
        // SPEC v6.4 §4.3: No governance in SafetyHaltMode
        let config = ValidatorScarcityConfig {
            floor_grace_epochs: 1,
            shm_no_governance: true,
            ..ValidatorScarcityConfig::default()
        };
        let mut tracker = SecurityStateTracker::post_bootstrap(50, config);
        let bootstrap_config = BootstrapConfig::default_config();

        // Enter SafetyHaltMode (validators < V_min_absolute)
        tracker.update(800, 15, &bootstrap_config, 0);
        assert!(tracker.is_emergency_mode());

        // All governance blocked
        assert!(!tracker.is_governance_allowed(true));
        assert!(!tracker.is_governance_allowed(false));
        assert!(!tracker.is_inflation_change_allowed());
    }
}
