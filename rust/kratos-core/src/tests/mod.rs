// Tests module
// SPEC v3.1 Phase 9: Integration & E2E Testing
// SPEC v5: Security Invariants & Threat Model Tests
// SPEC v7: Emergency Invariants & Systemic Resilience Tests
// SPEC v8: Fork Invariants & Protocol Survivability Tests
// SPEC v9: Adversary Model & Failure Mode Tests
// Networking: P2P, peer management, sync, and request-response tests
// Bootstrap Exit: State transition tests for bootstrap → normal → degraded → recovery

pub mod integration;
pub mod security_invariants;
pub mod emergency_invariants;
pub mod fork_invariants;
pub mod adversary_invariants;
pub mod networking;
pub mod bootstrap_exit;
pub mod bootstrap_exit_mainnet;
