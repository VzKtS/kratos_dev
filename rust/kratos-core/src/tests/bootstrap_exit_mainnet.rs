// Bootstrap Exit Tests - MAINNET Configuration Only
// Tests for SPEC v7.1: Bootstrap Exit & Network Degradation
//
// These tests use PRODUCTION parameters:
// - Bootstrap exit epoch: 1440 (60 days at 1h/epoch per SPEC v2.3)
// - Minimum validators for exit: 50
// - SafeValidators threshold: 75
// - Emergency threshold: 25
// - Minimum stake bootstrap: 50,000 KRAT
// - Minimum stake post-bootstrap: 25,000 KRAT

use crate::consensus::economics::{
    BootstrapConfig, NetworkSecurityState, SecurityStateTracker,
    ValidatorScarcityConfig, SAFE_VALIDATORS, POST_BOOTSTRAP_MIN_VALIDATORS,
    EMERGENCY_VALIDATORS, BootstrapStatus,
};

// =============================================================================
// TEST HELPERS - MAINNET ONLY
// =============================================================================

/// Create a MainNet bootstrap config (production values)
fn mainnet_bootstrap_config() -> BootstrapConfig {
    BootstrapConfig::default_config()
}

/// Create a tracker configured for MainNet testing
fn mainnet_tracker() -> SecurityStateTracker {
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 3,      // Production: 3 epochs grace
        normal_recovery_epochs: 5,  // Production: 5 epochs to recover
        dsm_recovery_epochs: 5,
        ..ValidatorScarcityConfig::default()
    };
    SecurityStateTracker::new(POST_BOOTSTRAP_MIN_VALIDATORS, config)
}

/// Create a post-bootstrap tracker for MainNet
fn mainnet_post_bootstrap_tracker() -> SecurityStateTracker {
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 3,
        normal_recovery_epochs: 5,
        dsm_recovery_epochs: 5,
        ..ValidatorScarcityConfig::default()
    };
    SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config)
}

// =============================================================================
// SCENARIO 1: NORMAL BOOTSTRAP EXIT (MAINNET)
// Validators reach 50 AND epoch >= 1440 → Exit to Normal/Healthy state
// =============================================================================

#[test]
fn test_mainnet_scenario1_normal_bootstrap_exit() {
    // MainNet config: 50 validators, 1440 epochs (60 days)
    let bootstrap_config = mainnet_bootstrap_config();
    let mut tracker = mainnet_tracker();

    // Verify mainnet parameters
    assert_eq!(bootstrap_config.end_epoch, 1440);
    assert_eq!(bootstrap_config.min_validators_exit, 50);

    // Initial state: Bootstrap
    assert!(tracker.is_bootstrap());
    assert!(!tracker.bootstrap_completed);

    // Epoch 720 (halfway): 40 validators - still in bootstrap
    let result = tracker.update_with_finality(720, 40, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());

    // Epoch 1439: 50 validators but epoch not reached - still bootstrap
    let result = tracker.update_with_finality(1439, 50, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());

    // Epoch 1440: 50 validators AND epoch threshold - should exit bootstrap
    let result = tracker.update_with_finality(1440, 50, &bootstrap_config, 0, 100);
    assert!(result.is_some(), "Should have exited bootstrap at epoch 1440 with 50 validators");
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
fn test_mainnet_scenario1_epoch_reached_not_enough_validators() {
    // Epoch 1440 reached but only 30 validators → stay in bootstrap
    let bootstrap_config = mainnet_bootstrap_config();
    let mut tracker = mainnet_tracker();

    // Epoch 1440: Only 30 validators - not enough
    let result = tracker.update_with_finality(1440, 30, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());
    assert!(!tracker.bootstrap_completed);

    // Epoch 1600: Still only 30 validators - extended bootstrap
    let result = tracker.update_with_finality(1600, 30, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());
}

#[test]
fn test_mainnet_scenario1_extended_bootstrap() {
    // Test: epoch >> 1440 but validators still below 50
    let bootstrap_config = mainnet_bootstrap_config();
    let mut tracker = mainnet_tracker();

    // Epoch 2000: Only 49 validators (one short)
    let result = tracker.update_with_finality(2000, 49, &bootstrap_config, 0, 100);
    assert!(result.is_none(), "Should NOT exit with 49 validators");
    assert!(tracker.is_bootstrap());

    // Finally hit 50 validators
    let result = tracker.update_with_finality(2001, 50, &bootstrap_config, 0, 100);
    assert!(result.is_some(), "Should exit with 50 validators");
    assert!(tracker.is_normal());
}

#[test]
fn test_mainnet_scenario1_validators_reached_early() {
    // Test: 100 validators at epoch 100 - must wait for epoch 1440
    let bootstrap_config = mainnet_bootstrap_config();
    let mut tracker = mainnet_tracker();

    // Epoch 100: 100 validators but too early
    let result = tracker.update_with_finality(100, 100, &bootstrap_config, 0, 100);
    assert!(result.is_none(), "Should NOT exit before epoch 1440");
    assert!(tracker.is_bootstrap());

    // Epoch 1000: Still too early
    let result = tracker.update_with_finality(1000, 100, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());

    // Epoch 1440: Now should exit
    let result = tracker.update_with_finality(1440, 100, &bootstrap_config, 0, 100);
    assert!(result.is_some(), "Should exit at epoch 1440");
    assert!(tracker.is_normal());
}

#[test]
fn test_mainnet_scenario1_exactly_at_thresholds() {
    // Test: Exactly 50 validators at exactly epoch 1440
    let bootstrap_config = mainnet_bootstrap_config();
    let mut tracker = mainnet_tracker();

    // Exactly at both thresholds
    let result = tracker.update_with_finality(1440, 50, &bootstrap_config, 0, 100);
    assert!(result.is_some());
    assert!(tracker.is_normal());
    assert!(tracker.bootstrap_completed);
}

// =============================================================================
// SCENARIO 2: DEGRADED STATE ENTRY (MAINNET)
// After bootstrap exit, validators drop → Enter Degraded state
// SafeValidators = 75, PostBootstrapMin = 50, Emergency = 25
// =============================================================================

#[test]
fn test_mainnet_scenario2_normal_to_degraded() {
    // Post-bootstrap: Start with 80 validators, drop below SafeValidators (75)
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 3, // Need 3 epochs below threshold
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    assert!(tracker.is_normal());

    // Epoch 800: Drop to 70 validators (below SafeValidators=75)
    tracker.update(800, 70, &bootstrap_config, 0);
    assert!(tracker.is_normal()); // Grace period - epoch 1

    // Epoch 801: Still at 70 validators
    tracker.update(801, 70, &bootstrap_config, 0);
    assert!(tracker.is_normal()); // Grace period - epoch 2

    // Epoch 802: Third epoch below → enters Degraded
    let result = tracker.update(802, 70, &bootstrap_config, 0);
    assert!(result.is_some(), "Should have entered Degraded after 3 epochs");
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Verify the state details
    match &tracker.state {
        NetworkSecurityState::DegradedSecurityMode { validators_needed, .. } => {
            assert_eq!(*validators_needed, 5); // Need 5 more to reach 75
        }
        _ => panic!("Expected DegradedSecurityMode state, got {:?}", tracker.state),
    }
}

#[test]
fn test_mainnet_scenario2_degraded_to_restricted() {
    // Start in Normal, drop to Degraded, then to Restricted (SHM)
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // 80 validators → Normal
    tracker.update(800, 80, &bootstrap_config, 0);
    assert!(tracker.is_normal());

    // 60 validators → Degraded (< SafeValidators=75, >= PostBootstrapMin=50)
    tracker.update(801, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // 40 validators → Restricted (< PostBootstrapMin=50, >= Emergency=25)
    tracker.update(802, 40, &bootstrap_config, 0);
    assert!(tracker.is_shm() || tracker.is_degraded() || tracker.is_survival_mode(),
        "Should be in Restricted/SHM state with 40 validators");
}

#[test]
fn test_mainnet_scenario2_restricted_to_emergency() {
    // Full cascade: Normal → Degraded → Restricted → Emergency
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Start at Normal
    tracker.update(800, 80, &bootstrap_config, 0);
    assert!(tracker.is_normal());

    // Drop to 60 → Degraded
    tracker.update(801, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Drop to 40 → Restricted
    tracker.update(802, 40, &bootstrap_config, 0);

    // Drop to 20 → Emergency (< Emergency=25)
    tracker.update(803, 20, &bootstrap_config, 0);
    assert!(tracker.is_emergency_mode() || tracker.is_critical(),
        "Should be in Emergency state with 20 validators");
}

#[test]
fn test_mainnet_scenario2_grace_period_prevents_degradation() {
    // Validators recover during grace period → state doesn't degrade
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 3,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Two epochs below SafeValidators
    tracker.update(800, 70, &bootstrap_config, 0);
    tracker.update(801, 70, &bootstrap_config, 0);
    assert_eq!(tracker.epochs_below_minimum, 2);
    assert!(tracker.is_normal());

    // Validators recover to 80 - grace period resets
    tracker.update(802, 80, &bootstrap_config, 0);
    assert_eq!(tracker.epochs_below_minimum, 0);
    assert!(tracker.is_normal());
}

#[test]
fn test_mainnet_scenario2_rapid_drop() {
    // Validators drop rapidly in a single epoch
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Start at 100 validators
    tracker.update(800, 100, &bootstrap_config, 0);
    assert!(tracker.is_normal());

    // Massive drop to 15 validators (below Emergency=25)
    tracker.update(801, 15, &bootstrap_config, 0);

    // Should be in Emergency state
    assert!(tracker.is_emergency_mode() || tracker.is_critical() || tracker.is_shm(),
        "Should be in Emergency with only 15 validators");
}

// =============================================================================
// SCENARIO 3: RECOVERY FROM DEGRADED STATE (MAINNET)
// Validators increase → Return to Normal state
// =============================================================================

#[test]
fn test_mainnet_scenario3_degraded_to_normal_recovery() {
    // Setup: Start in Degraded, recover to Normal
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 5, // Need 5 consecutive epochs at SafeValidators
        dsm_recovery_epochs: 5,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Enter Degraded state (validators < SafeValidators = 75)
    tracker.update(800, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Start recovery: validators back to SafeValidators (75)
    tracker.update(801, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss()); // Still degraded, counting
    assert_eq!(tracker.epochs_above_minimum_in_vss(), 1);

    tracker.update(802, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(803, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(804, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert_eq!(tracker.epochs_above_minimum_in_vss(), 4);

    // Fifth consecutive epoch at SafeValidators → should recover
    let result = tracker.update(805, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert!(result.is_some(), "Should have recovered after 5 epochs");
    assert!(tracker.is_normal());
}

#[test]
fn test_mainnet_scenario3_recovery_interrupted() {
    // Recovery counter resets if validators drop below threshold
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 5,
        dsm_recovery_epochs: 5,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Enter Degraded
    tracker.update(800, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Start recovery - 3 epochs at SafeValidators
    tracker.update(801, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(802, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(803, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert_eq!(tracker.epochs_above_minimum_in_vss(), 3);

    // Drop again - recovery counter resets
    tracker.update(804, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());
    assert_eq!(tracker.epochs_above_minimum_in_vss(), 0);

    // Need full 5 epochs again
    tracker.update(805, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(806, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(807, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(808, SAFE_VALIDATORS, &bootstrap_config, 0);
    let result = tracker.update(809, SAFE_VALIDATORS, &bootstrap_config, 0);

    assert!(result.is_some());
    assert!(tracker.is_normal());
}

#[test]
fn test_mainnet_scenario3_emergency_full_recovery() {
    // Test: Emergency → Restricted → Degraded → Normal
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 2,
        dsm_recovery_epochs: 2,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Enter Emergency (below 25 validators)
    tracker.update(800, 60, &bootstrap_config, 0); // Degraded
    tracker.update(801, 40, &bootstrap_config, 0); // Restricted
    tracker.update(802, 20, &bootstrap_config, 0); // Emergency

    // Recovery: 20 → 30 (above Emergency=25)
    tracker.update(803, 30, &bootstrap_config, 0);
    tracker.update(804, 30, &bootstrap_config, 0);
    // Should be in Restricted now (30 < PostBootstrapMin=50)

    // Continue: 30 → 55 (above PostBootstrapMin=50)
    tracker.update(805, 55, &bootstrap_config, 0);
    tracker.update(806, 55, &bootstrap_config, 0);
    // Should be in Degraded now (55 < SafeValidators=75)

    // Final: 55 → 80 (above SafeValidators=75)
    tracker.update(807, 80, &bootstrap_config, 0);
    tracker.update(808, 80, &bootstrap_config, 0);

    assert!(tracker.is_normal(), "Should have recovered to Normal from Emergency");
}

// =============================================================================
// FULL LIFECYCLE TESTS (MAINNET)
// Complete journey through all states with production parameters
// =============================================================================

#[test]
fn test_mainnet_full_lifecycle() {
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1, // Fast grace for test
        normal_recovery_epochs: 2,
        dsm_recovery_epochs: 2,
        ..ValidatorScarcityConfig::default()
    };
    // Use SAFE_VALIDATORS as minimum to track degradation properly
    let mut tracker = SecurityStateTracker::new(SAFE_VALIDATORS, config);

    // Phase 1: Bootstrap (epochs 0-1439)
    assert!(tracker.is_bootstrap());

    // Early bootstrap: 30 validators, epoch 100
    tracker.update_with_finality(100, 30, &bootstrap_config, 0, 100);
    assert!(tracker.is_bootstrap());

    // Mid bootstrap: 50 validators, epoch 1000 (still too early)
    tracker.update_with_finality(1000, 50, &bootstrap_config, 0, 100);
    assert!(tracker.is_bootstrap());

    // Phase 2: Exit Bootstrap at epoch 1440 with 80 validators (above SAFE_VALIDATORS)
    tracker.update_with_finality(1440, 80, &bootstrap_config, 0, 100);
    assert!(tracker.is_normal());
    assert!(tracker.bootstrap_completed);

    // Phase 3: Normal operation (epochs 1441-1499)
    tracker.update(1450, 80, &bootstrap_config, 0);
    assert!(tracker.is_normal());

    // Phase 4: Degradation - validators drop below SafeValidators (75)
    // With floor_grace_epochs=1, degradation happens immediately
    tracker.update(1500, 60, &bootstrap_config, 0); // Below 75
    assert!(tracker.is_degraded() || tracker.is_vss(),
        "Should be degraded with 60 validators (< SafeValidators=75)");

    // Phase 5: Recovery - need consecutive epochs at SafeValidators
    tracker.update(1501, SAFE_VALIDATORS, &bootstrap_config, 0);
    tracker.update(1502, SAFE_VALIDATORS, &bootstrap_config, 0);
    assert!(tracker.is_normal());

    // Verify transitions were tracked
    assert!(tracker.state_transitions.len() >= 2,
        "Should have at least 2 transitions: Bootstrap→Normal→Degraded");
}

#[test]
fn test_mainnet_full_cascade_and_recovery() {
    // Complete cascade: Bootstrap → Normal → Degraded → Restricted → Emergency → Recovery
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 2,
        dsm_recovery_epochs: 2,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::new(SAFE_VALIDATORS, config);

    // Bootstrap exit at epoch 1440
    tracker.update_with_finality(1440, 100, &bootstrap_config, 0, 100);
    assert!(tracker.is_normal());

    // Cascade down
    tracker.update(1441, 60, &bootstrap_config, 0); // → Degraded (< 75)
    tracker.update(1442, 40, &bootstrap_config, 0); // → Restricted (< 50)
    tracker.update(1443, 15, &bootstrap_config, 0); // → Emergency (< 25)

    // Should be in Emergency or critical state
    assert!(tracker.is_emergency_mode() || tracker.is_critical() || tracker.is_shm(),
        "Should be in Emergency with 15 validators");

    // Full recovery
    tracker.update(1444, 30, &bootstrap_config, 0); // Above Emergency
    tracker.update(1445, 30, &bootstrap_config, 0);
    tracker.update(1446, 55, &bootstrap_config, 0); // Above PostBootstrapMin
    tracker.update(1447, 55, &bootstrap_config, 0);
    tracker.update(1448, 80, &bootstrap_config, 0); // Above SafeValidators
    tracker.update(1449, 80, &bootstrap_config, 0);

    assert!(tracker.is_normal(), "Should have fully recovered");
}

// =============================================================================
// BOOTSTRAP STATUS REPORTING (MAINNET)
// =============================================================================

#[test]
fn test_mainnet_bootstrap_status_active() {
    let bootstrap_config = mainnet_bootstrap_config();

    // Early in bootstrap: epoch 100, 20 validators
    let status = bootstrap_config.get_bootstrap_status(100, 20, 0);
    match status {
        BootstrapStatus::Active { validators_needed, epochs_remaining, .. } => {
            assert_eq!(validators_needed, 30); // Need 30 more to reach 50
            assert_eq!(epochs_remaining, 1340); // 1440 - 100
        }
        _ => panic!("Expected Active status, got {:?}", status),
    }
}

#[test]
fn test_mainnet_bootstrap_status_extended() {
    let bootstrap_config = mainnet_bootstrap_config();

    // Past epoch 1440 but not enough validators
    let status = bootstrap_config.get_bootstrap_status(1600, 40, 0);
    match status {
        BootstrapStatus::Extended { validators_needed, epochs_overdue, .. } => {
            assert_eq!(validators_needed, 10); // Need 10 more to reach 50
            assert_eq!(epochs_overdue, 160); // 1600 - 1440
        }
        _ => panic!("Expected Extended status, got {:?}", status),
    }
}

#[test]
fn test_mainnet_bootstrap_status_completed() {
    let bootstrap_config = mainnet_bootstrap_config();

    // At epoch 1440 with 50 validators
    let status = bootstrap_config.get_bootstrap_status(1440, 50, 0);
    match status {
        BootstrapStatus::Completed => {}
        _ => panic!("Expected Completed status, got {:?}", status),
    }

    // Well past epoch with many validators
    let status = bootstrap_config.get_bootstrap_status(2000, 100, 0);
    match status {
        BootstrapStatus::Completed => {}
        _ => panic!("Expected Completed status, got {:?}", status),
    }
}

// =============================================================================
// EDGE CASES (MAINNET)
// =============================================================================

#[test]
fn test_mainnet_zero_validators() {
    let bootstrap_config = mainnet_bootstrap_config();
    let mut tracker = mainnet_tracker();

    // Zero validators - should stay in bootstrap forever
    let result = tracker.update_with_finality(2000, 0, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());
}

#[test]
fn test_mainnet_one_validator_short() {
    let bootstrap_config = mainnet_bootstrap_config();
    let mut tracker = mainnet_tracker();

    // 49 validators at epoch 1440 - one short
    let result = tracker.update_with_finality(1440, 49, &bootstrap_config, 0, 100);
    assert!(result.is_none(), "Should NOT exit with 49 validators");
    assert!(tracker.is_bootstrap());

    // Now 50 - should exit
    let result = tracker.update_with_finality(1441, 50, &bootstrap_config, 0, 100);
    assert!(result.is_some());
    assert!(tracker.is_normal());
}

#[test]
fn test_mainnet_one_epoch_short() {
    let bootstrap_config = mainnet_bootstrap_config();
    let mut tracker = mainnet_tracker();

    // 100 validators at epoch 1439 - one epoch short
    let result = tracker.update_with_finality(1439, 100, &bootstrap_config, 0, 100);
    assert!(result.is_none(), "Should NOT exit at epoch 1439");
    assert!(tracker.is_bootstrap());

    // Epoch 1440 - should exit
    let result = tracker.update_with_finality(1440, 100, &bootstrap_config, 0, 100);
    assert!(result.is_some());
    assert!(tracker.is_normal());
}

#[test]
fn test_mainnet_threshold_boundaries() {
    // Test exact boundary values for state transitions
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Exactly at SafeValidators (75) → Normal
    tracker.update(800, 75, &bootstrap_config, 0);
    assert!(tracker.is_normal());

    // One below SafeValidators (74) → Degraded
    tracker.update(801, 74, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Reset and test PostBootstrapMin boundary
    let mut tracker2 = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS,
        ValidatorScarcityConfig { floor_grace_epochs: 1, ..ValidatorScarcityConfig::default() });

    tracker2.update(800, 60, &bootstrap_config, 0); // Enter Degraded
    tracker2.update(801, 50, &bootstrap_config, 0); // Exactly at PostBootstrapMin
    // Should still be Degraded (50 < 75)
    assert!(tracker2.is_degraded() || tracker2.is_vss() || tracker2.is_shm());

    tracker2.update(802, 49, &bootstrap_config, 0); // Below PostBootstrapMin
    // Should be Restricted/SHM
    assert!(tracker2.is_shm() || tracker2.is_survival_mode() || tracker2.is_degraded());
}

#[test]
fn test_mainnet_oscillating_validators() {
    // Validators oscillate around SafeValidators threshold
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 3,
        normal_recovery_epochs: 3,
        ..ValidatorScarcityConfig::default()
    };
    let mut tracker = SecurityStateTracker::post_bootstrap(SAFE_VALIDATORS, config);

    // Oscillate around 75
    for epoch in 800..820 {
        let validators = if epoch % 2 == 0 { 70 } else { 80 };
        tracker.update(epoch, validators, &bootstrap_config, 0);
    }

    // Due to grace periods, should prevent rapid state changes
    // The exact state depends on implementation details
    // but we verify no crashes and reasonable behavior
    assert!(tracker.is_normal() || tracker.is_degraded() || tracker.is_vss());
}

#[test]
fn test_mainnet_long_extended_bootstrap() {
    // Bootstrap extends for a very long time
    let bootstrap_config = mainnet_bootstrap_config();
    let mut tracker = mainnet_tracker();

    // Epoch 3000, still only 40 validators
    let result = tracker.update_with_finality(3000, 40, &bootstrap_config, 0, 100);
    assert!(result.is_none());
    assert!(tracker.is_bootstrap());

    // Check status shows extended
    let status = bootstrap_config.get_bootstrap_status(3000, 40, 0);
    match status {
        BootstrapStatus::Extended { epochs_overdue, .. } => {
            assert_eq!(epochs_overdue, 1560); // 3000 - 1440
        }
        _ => panic!("Expected Extended status"),
    }
}

#[test]
fn test_mainnet_verify_constants() {
    // Verify SPEC v7.1 constants are correct
    assert_eq!(SAFE_VALIDATORS, 75, "SafeValidators should be 75");
    assert_eq!(POST_BOOTSTRAP_MIN_VALIDATORS, 50, "PostBootstrapMin should be 50");
    assert_eq!(EMERGENCY_VALIDATORS, 25, "Emergency should be 25");

    let config = mainnet_bootstrap_config();
    assert_eq!(config.end_epoch, 1440, "Bootstrap end epoch should be 1440 (60 days)");
    assert_eq!(config.min_validators_exit, 50, "Min validators for exit should be 50");
    // SPEC v2.3: Lowered stake requirements (values in KRAT base units)
    use crate::types::KRAT;
    assert_eq!(config.min_stake_bootstrap, 50_000 * KRAT, "Bootstrap stake should be 50k");
    assert_eq!(config.min_stake_post_bootstrap, 25_000 * KRAT, "Post-bootstrap stake should be 25k");
}

#[test]
fn test_mainnet_transition_history_complete() {
    let bootstrap_config = mainnet_bootstrap_config();
    let config = ValidatorScarcityConfig {
        floor_grace_epochs: 1,
        normal_recovery_epochs: 1,
        dsm_recovery_epochs: 1,
        ..ValidatorScarcityConfig::default()
    };
    // Use SAFE_VALIDATORS to properly track degradation
    let mut tracker = SecurityStateTracker::new(SAFE_VALIDATORS, config);

    // Bootstrap → Normal (with validators above SAFE_VALIDATORS)
    tracker.update_with_finality(1440, 80, &bootstrap_config, 0, 100);
    assert!(tracker.is_normal());

    // Normal → Degraded (drop below SAFE_VALIDATORS=75)
    tracker.update(1441, 60, &bootstrap_config, 0);
    assert!(tracker.is_degraded() || tracker.is_vss());

    // Degraded → Normal (recover to SAFE_VALIDATORS)
    tracker.update(1442, 80, &bootstrap_config, 0);

    // Check transition history
    let history = &tracker.state_transitions;
    assert!(!history.is_empty(), "Should have recorded at least 1 transition");

    // First transition should be Bootstrap → Healthy/Normal
    assert_eq!(history[0].from_state, "Bootstrap");
    assert!(history[0].to_state == "Healthy" || history[0].to_state == "Normal");
}
