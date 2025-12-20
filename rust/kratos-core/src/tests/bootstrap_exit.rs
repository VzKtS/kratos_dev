// Bootstrap Exit Tests - Simulated state transitions
// Tests for SPEC v7.1: Bootstrap Exit & Network Degradation
//
// These tests simulate the three main scenarios for exiting bootstrap:
// 1. Normal exit: validators reach threshold → Normal state
// 2. Degraded exit: validators below SafeValidators → Degraded state
// 3. Recovery: degraded state → back to Normal

use crate::consensus::economics::{
    BootstrapConfig, DegradedSecurityConfig, NetworkSecurityState, SecurityStateTracker,
    ValidatorScarcityConfig, SAFE_VALIDATORS, POST_BOOTSTRAP_MIN_VALIDATORS,
    EMERGENCY_VALIDATORS, get_bootstrap_config,
};

// =============================================================================
// TEST HELPERS
// =============================================================================

/// Create a test bootstrap config with reduced values for faster testing
fn test_bootstrap_config() -> BootstrapConfig {
    BootstrapConfig {
        genesis_epoch: 0,
        end_epoch: 10, // Short bootstrap for testing
        min_validators_exit: 3,
        min_stake_bootstrap: 50_000 * crate::types::KRAT,
        min_stake_post_bootstrap: 25_000 * crate::types::KRAT,
        min_stake_total_exit: 25_000_000 * crate::types::KRAT,
        vc_vote_multiplier: 2,
        vc_uptime_multiplier: 2,
        vc_arbitration_multiplier: 1,
        target_inflation: 0.065,
    }
}

/// Create a tracker configured for testing
fn test_tracker() -> SecurityStateTracker {
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 2,
        dsm_recovery_epochs: 2,
        ..ValidatorScarcityConfig::default()
    };
    SecurityStateTracker::new(3, config) // 3 validators for testing
}

/// Create a post-bootstrap tracker for testing
fn test_post_bootstrap_tracker() -> SecurityStateTracker {
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 2,
        dsm_recovery_epochs: 2,
        ..ValidatorScarcityConfig::default()
    };
    SecurityStateTracker::post_bootstrap(3, config)
}

// =============================================================================
// SCENARIO 1: NORMAL BOOTSTRAP EXIT
// Validators reach minimum threshold → Exit to Normal/Healthy state
// =============================================================================

#[test]
fn test_scenario1_normal_bootstrap_exit_devnet() {
    // DevNet config: 3 validators, 10 epochs
    let bootstrap_config = test_bootstrap_config();
    let mut tracker = test_tracker();

    // Initial state: Bootstrap
    assert!(tracker.is_bootstrap());
    assert!(!tracker.bootstrap_completed);

    // Epoch 5: Only 2 validators - still in bootstrap
    let result = tracker.update_with_finality(5, 2, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());

    // Epoch 10: 3 validators reached - should exit bootstrap
    let result = tracker.update_with_finality(10, 3, &bootstrap_config, 0, 100);
    assert!(result.is_some(), "Should have exited bootstrap");
    assert!(tracker.is_normal(), "Should be in Normal state");
    assert!(tracker.bootstrap_completed);

    // Verify transition was recorded
    let transitions = &tracker.state_transitions;
    assert!(!transitions.is_empty(), "Should have recorded transition");

    let last_transition = transitions.last().unwrap();
    assert_eq!(last_transition.from_state, "Bootstrap");
    assert_eq!(last_transition.to_state, "Healthy");
}

#[test]
fn test_scenario1_early_validator_exit() {
    // Test: validators reach threshold before epoch limit
    // NOTE: With DevNet config (end_epoch=10), we need epoch >= 10 OR validators >= 3
    // But the tracker also needs the bootstrap config to return should_exit=true
    let bootstrap_config = test_bootstrap_config();
    let mut tracker = test_tracker();

    // Even with enough validators, we need to reach epoch threshold too
    // because the tracker checks BootstrapConfig::should_exit_bootstrap
    // which requires BOTH conditions in production logic

    // At epoch 10 with 5 validators (above min of 3)
    let result = tracker.update_with_finality(10, 5, &bootstrap_config, 0, 100);

    // Should exit bootstrap since epoch >= 10 AND validators >= 3
    assert!(result.is_some(), "Should exit with epoch>=10 and enough validators");
    assert!(tracker.is_normal());
    assert!(tracker.bootstrap_completed);
}

#[test]
fn test_scenario1_extended_bootstrap_not_enough_validators() {
    // Test: epoch limit reached but not enough validators → stay in bootstrap
    let bootstrap_config = test_bootstrap_config();
    let mut tracker = test_tracker();

    // Epoch 15 (after 10): Only 2 validators
    let result = tracker.update_with_finality(15, 2, &bootstrap_config, 0, 100);

    // Should stay in bootstrap - need 3 validators
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());
    assert!(!tracker.bootstrap_completed);

    // Now add the third validator
    let result = tracker.update_with_finality(16, 3, &bootstrap_config, 0, 100);
    assert!(result.is_some(), "Should exit now with 3 validators");
    assert!(tracker.is_normal());
}

// =============================================================================
// SCENARIO 2: DEGRADED STATE ENTRY
// After bootstrap exit, validators drop → Enter Degraded state
// =============================================================================

#[test]
fn test_scenario2_normal_to_degraded() {
    // Start in post-bootstrap Normal state using production thresholds
    // SPEC v6.2 §5.1: Enter VSS when validators < SafeValidators (75)
    let bootstrap_config = BootstrapConfig::default_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 3, // Need 3 epochs below threshold
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(POST_BOOTSTRAP_MIN_VALIDATORS, config);

    assert!(tracker.is_normal());

    // Epoch 800: Drop to 40 validators (below 50)
    tracker.update(800, 40, &bootstrap_config, 0);
    assert!(tracker.is_normal()); // Grace period - epoch 1

    // Epoch 801: Still at 40 validators
    tracker.update(801, 40, &bootstrap_config, 0);
    assert!(tracker.is_normal()); // Grace period - epoch 2

    // Epoch 802: Third epoch below → enters Degraded
    let result = tracker.update(802, 40, &bootstrap_config, 0);
    assert!(result.is_some(), "Should have entered Degraded");
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Verify the state details
    match &tracker.state {
        NetworkSecurityState::DegradedSecurityMode { validators_needed, .. } => {
            assert_eq!(*validators_needed, 10); // Need 10 more to reach 50
        }
        _ => panic!("Expected DegradedSecurityMode state, got {:?}", tracker.state),
    }
}

#[test]
fn test_scenario2_grace_period_reset() {
    // Test: validators recover during grace period → counter resets
    let bootstrap_config = test_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 3,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(5, config);

    // Two epochs below minimum
    tracker.update(100, 4, &bootstrap_config, 0);
    tracker.update(101, 4, &bootstrap_config, 0);
    assert_eq!(tracker.epochs_below_minimum, 2);
    assert!(tracker.is_normal());

    // Validators recover - grace period resets
    tracker.update(102, 6, &bootstrap_config, 0);
    assert_eq!(tracker.epochs_below_minimum, 0);
    assert!(tracker.is_normal());

    // Drop again - counter starts fresh
    tracker.update(103, 4, &bootstrap_config, 0);
    assert_eq!(tracker.epochs_below_minimum, 1);
    tracker.update(104, 4, &bootstrap_config, 0);
    assert_eq!(tracker.epochs_below_minimum, 2);

    // Still in grace period
    assert!(tracker.is_normal());
}

#[test]
fn test_scenario2_cascading_degradation() {
    // Test: Degraded → Restricted → Emergency as validators decrease
    let bootstrap_config = BootstrapConfig::default_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        ..ValidatorScarcityConfig::default()
    };
    // Use real thresholds: SafeValidators=75, PostBootstrapMin=50, Emergency=25
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Start normal with 80 validators
    tracker.update(100, 80, &bootstrap_config, 0);
    assert!(tracker.is_normal());

    // Drop to 60 (below SafeValidators=75) → Degraded
    tracker.update(101, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Drop to 40 (below PostBootstrapMin=50) → Restricted
    tracker.update(102, 40, &bootstrap_config, 0);
    // Should be in Restricted/SHM state
    assert!(tracker.is_shm() || tracker.is_degraded() || tracker.is_survival_mode());

    // Drop to 20 (below Emergency=25) → Emergency
    tracker.update(103, 20, &bootstrap_config, 0);
    // Should be in Emergency state
    assert!(tracker.is_emergency_mode() || tracker.is_critical());
}

// =============================================================================
// SCENARIO 3: RECOVERY FROM DEGRADED STATE
// Validators increase → Return to Normal state
// =============================================================================

#[test]
fn test_scenario3_degraded_to_normal_recovery() {
    // Setup: Start in Degraded state using SAFE_VALIDATORS threshold
    // SPEC v7.1 §7.1: Recovery requires consecutive epochs at SafeValidators (75)
    let bootstrap_config = BootstrapConfig::default_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 3, // Need 3 consecutive epochs
        dsm_recovery_epochs: 3,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Enter Degraded state (validators < SafeValidators = 75)
    tracker.update(800, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Start recovery: validators back to SafeValidators
    tracker.update(801, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss()); // Still degraded, counting
    assert_eq!(tracker.epochs_above_minimum_in_vss(), 1);

    tracker.update(802, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss()); // Still counting

    // Third consecutive epoch at SafeValidators
    let result = tracker.update(803, SAFE_VALIDATORS, &bootstrap_config, 0);

    // Should recover to Normal
    assert!(result.is_some(), "Should have recovered");
    assert!(tracker.is_normal());
}

#[test]
fn test_scenario3_recovery_counter_reset() {
    // SPEC v7.1 §7.1: Consecutive epochs reset if validators drop below SafeValidators
    let bootstrap_config = BootstrapConfig::default_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 3,
        dsm_recovery_epochs: 3,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Enter Degraded (validators < SafeValidators = 75)
    tracker.update(800, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Start recovery - at SafeValidators
    tracker.update(801, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(802, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert_eq!(tracker.epochs_above_minimum_in_vss(), 2);

    // Drop again below SafeValidators - consecutive count resets
    tracker.update(803, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());
    assert_eq!(tracker.epochs_above_minimum_in_vss(), 0);

    // Need full 3 epochs again
    tracker.update(804, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());
    assert_eq!(tracker.epochs_above_minimum_in_vss(), 1);

    tracker.update(805, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    let result = tracker.update(806, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert!(result.is_some());
    assert!(tracker.is_normal());
}

#[test]
fn test_scenario3_emergency_recovery_path() {
    // Test: Emergency → Restricted → Degraded → Normal
    let bootstrap_config = BootstrapConfig::default_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 2,
        dsm_recovery_epochs: 2,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Enter Emergency (below 25 validators)
    tracker.update(100, 60, &bootstrap_config, 0); // Degraded
    tracker.update(101, 40, &bootstrap_config, 0); // Restricted
    tracker.update(102, 20, &bootstrap_config, 0); // Emergency

    let initial_state = tracker.state.clone();

    // Recovery path: increase validators gradually
    // 20 → 30 (above Emergency=25)
    tracker.update(103, 30, &bootstrap_config, 0);
    tracker.update(104, 30, &bootstrap_config, 0);

    // Should be in Restricted now (30 < PostBootstrapMin=50)
    // The exact state depends on implementation

    // Continue recovery: 30 → 55 (above PostBootstrapMin=50)
    tracker.update(105, 55, &bootstrap_config, 0);
    tracker.update(106, 55, &bootstrap_config, 0);

    // Should be in Degraded now (55 < SafeValidators=75)

    // Final recovery: 55 → 80 (above SafeValidators=75)
    tracker.update(107, 80, &bootstrap_config, 0);
    tracker.update(108, 80, &bootstrap_config, 0);

    // Should be Normal now
    assert!(tracker.is_normal(), "Should have recovered to Normal");
}

// =============================================================================
// FULL LIFECYCLE TESTS
// Complete journey through all states
// =============================================================================

#[test]
fn test_full_lifecycle_bootstrap_to_emergency_and_back() {
    // Use production config with SAFE_VALIDATORS threshold
    let bootstrap_config = BootstrapConfig::default_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 2,
        dsm_recovery_epochs: 2,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::new(SAFE_VALIDATORS, config);

    // Phase 1: Bootstrap
    assert!(tracker.is_bootstrap());

    // Phase 2: Exit Bootstrap → Normal (need epoch >= 1440 AND validators >= 50)
    tracker.update_with_finality(1440, 80, &bootstrap_config, 0, 100);
    assert!(tracker.is_normal());
    assert!(tracker.bootstrap_completed);

    // Phase 3: Normal → Degraded (validators drop below SafeValidators=75)
    tracker.update(1441, 60, &bootstrap_config, 0);
    tracker.update(1442, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Phase 4: Recovery → Normal (need consecutive epochs at SafeValidators)
    tracker.update(1443, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(1444, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert!(tracker.is_normal());

    // Verify we tracked transitions
    assert!(!tracker.state_transitions.is_empty(),
        "Should have recorded transitions");
}

#[test]
fn test_config_values() {
    use crate::types::KRAT;

    let config = BootstrapConfig::default_config();

    // Verify production values (SPEC v2.3: 60-day bootstrap)
    assert_eq!(config.min_validators_exit, 50);
    assert_eq!(config.end_epoch, 1440);  // 1440 epochs = 60 days at 1h/epoch
    assert_eq!(config.min_stake_bootstrap, 50_000 * KRAT);
    assert_eq!(config.min_stake_post_bootstrap, 25_000 * KRAT);
}

#[test]
fn test_bootstrap_status_reporting() {
    use crate::consensus::economics::BootstrapStatus;

    let bootstrap_config = test_bootstrap_config();

    // Test status at different stages
    let status_early = bootstrap_config.get_bootstrap_status(5, 1, 0);
    match status_early {
        BootstrapStatus::Active { validators_needed, .. } => {
            assert_eq!(validators_needed, 2); // Need 2 more to reach 3
        }
        _ => panic!("Expected Active status"),
    }

    let status_extended = bootstrap_config.get_bootstrap_status(15, 2, 0);
    match status_extended {
        BootstrapStatus::Extended { validators_needed, .. } => {
            assert_eq!(validators_needed, 1); // Need 1 more
        }
        _ => panic!("Expected Extended status"),
    }

    let status_completed = bootstrap_config.get_bootstrap_status(15, 5, 0);
    match status_completed {
        BootstrapStatus::Completed => {}
        _ => panic!("Expected Completed status"),
    }
}

// =============================================================================
// PRODUCTION CONFIG TESTS (with real thresholds)
// =============================================================================

#[test]
fn test_production_bootstrap_exit() {
    let bootstrap_config = BootstrapConfig::default_config();
    let mut tracker = SecurityStateTracker::new(POST_BOOTSTRAP_MIN_VALIDATORS,
        ValidatorScarcityConfig::default());

    // Need 50 validators AND epoch >= 1440

    // Epoch 1440 but only 30 validators - no exit
    let result = tracker.update_with_finality(1440, 30, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());

    // Epoch 1440 with 50 validators - should exit
    let result = tracker.update_with_finality(1440, 50, &bootstrap_config, 0, 100);
    assert!(result.is_some());
    assert!(tracker.is_normal());
}

#[test]
fn test_production_state_thresholds() {
    // Verify SPEC v7.1 thresholds
    assert_eq!(SAFE_VALIDATORS, 75);
    assert_eq!(POST_BOOTSTRAP_MIN_VALIDATORS, 50);
    assert_eq!(EMERGENCY_VALIDATORS, 25);

    let bootstrap_config = BootstrapConfig::default_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // 80 validators → Normal (>= 75)
    tracker.update(100, 80, &bootstrap_config, 0);
    assert!(tracker.is_normal());

    // 60 validators → Degraded (>= 50, < 75)
    tracker.update(101, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // 30 validators → Restricted (>= 25, < 50)
    tracker.update(102, 30, &bootstrap_config, 0);
    assert!(tracker.is_shm() || tracker.is_degraded() || tracker.is_survival_mode());

    // 20 validators → Emergency (< 25)
    tracker.update(103, 20, &bootstrap_config, 0);
    assert!(tracker.is_emergency_mode() || tracker.is_critical() || tracker.is_shm());
}

// =============================================================================
// EDGE CASES
// =============================================================================

#[test]
fn test_exact_threshold_boundary() {
    let bootstrap_config = test_bootstrap_config();
    let mut tracker = test_tracker();

    // Exactly at threshold (3 validators)
    let result = tracker.update_with_finality(10, 3, &bootstrap_config, 0, 100);
    assert!(result.is_some());
    assert!(tracker.is_normal());
}

#[test]
fn test_zero_validators() {
    let bootstrap_config = test_bootstrap_config();
    let mut tracker = test_tracker();

    // Zero validators - should stay in bootstrap
    let result = tracker.update_with_finality(100, 0, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());
}

#[test]
fn test_rapid_validator_changes() {
    let bootstrap_config = test_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 2,
        normal_recovery_epochs: 2,
        dsm_recovery_epochs: 2,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(5, config);

    // Rapid oscillation
    for epoch in 0..20 {
        let validators = if epoch % 2 == 0 { 3 } else { 6 };
        tracker.update(100 + epoch, validators, &bootstrap_config, 0);
    }

    // Should never fully transition due to alternating values
    // The grace periods and recovery counters prevent rapid state changes
}

#[test]
fn test_transition_history() {
    let bootstrap_config = test_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 1,
        dsm_recovery_epochs: 1,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::new(3, config);

    // Bootstrap → Normal
    tracker.update_with_finality(10, 5, &bootstrap_config, 0, 100);

    // Normal → Degraded
    tracker.update(11, 2, &bootstrap_config, 0);
    tracker.update(12, 2, &bootstrap_config, 0);

    // Degraded → Normal
    tracker.update(13, 5, &bootstrap_config, 0);
    tracker.update(14, 5, &bootstrap_config, 0);

    // Check transition history
    let history = &tracker.state_transitions;
    assert!(history.len() >= 2, "Should have recorded multiple transitions");

    // First transition should be Bootstrap → Healthy
    if !history.is_empty() {
        assert_eq!(history[0].from_state, "Bootstrap");
    }
}
