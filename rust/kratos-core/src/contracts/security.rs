// Security Contract - SPEC v9: Runtime Security Monitoring & Invariant Checking
// Principle: The protocol MUST always allow exit, fork, and power decay
//
// This contract provides:
// - Runtime invariant verification
// - Threat detection and assessment
// - Slow capture monitoring
// - Security audit tracking
// - Protocol health monitoring

use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash};
use crate::types::security::{
    AcceptableFailure, AdversaryClass, AuditRecord, AuditType, DetectedThreat,
    InvariantCheckResult, Mitigation, NonAssumption, ProtocolHealthState,
    ProtocolInvariant, ReviewCycle, SlowCaptureCountermeasure, SlowCaptureIndicator,
    ThreatAssessment, ThreatLevel, ThreatSeverity, ThreatType, UnacceptableFailure,
};
use std::collections::{HashMap, HashSet, VecDeque};

// =============================================================================
// SECURITY CONTRACT
// =============================================================================

/// Security contract for runtime monitoring and invariant checking
pub struct SecurityContract {
    /// Current block number
    current_block: BlockNumber,

    /// Protocol health state
    health_state: ProtocolHealthState,

    /// Active threat assessments
    threat_assessments: VecDeque<ThreatAssessment>,

    /// Slow capture indicators per entity
    capture_indicators: HashMap<AccountId, SlowCaptureIndicator>,

    /// Historical stake snapshots for capture detection
    stake_history: HashMap<AccountId, Vec<(BlockNumber, Balance)>>,

    /// Historical VC snapshots
    vc_history: HashMap<AccountId, Vec<(BlockNumber, u64)>>,

    /// Audit records
    audit_records: Vec<AuditRecord>,

    /// Review cycles
    review_cycles: Vec<ReviewCycle>,

    /// Current review cycle
    current_review_cycle: Option<ReviewCycle>,

    /// Detected threats (active)
    active_threats: Vec<DetectedThreat>,

    /// Configuration
    config: SecurityConfig,

    /// Events
    events: Vec<SecurityEvent>,
}

/// Security configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Blocks between health checks
    pub health_check_interval: BlockNumber,

    /// Epochs for capture detection window
    pub capture_detection_window: u64,

    /// Alert threshold for capture indicators
    pub capture_alert_threshold: f64,

    /// Max threat assessments to keep
    pub max_threat_history: usize,

    /// Max stake history entries per account
    pub max_stake_history: usize,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            health_check_interval: 14_400, // ~1 day
            capture_detection_window: 100,  // 100 epochs
            capture_alert_threshold: 0.25,
            max_threat_history: 100,
            max_stake_history: 365, // ~1 year of daily snapshots
        }
    }
}

/// Security events
#[derive(Debug, Clone)]
pub enum SecurityEvent {
    InvariantViolation {
        invariant: ProtocolInvariant,
        block: BlockNumber,
        details: String,
    },
    UnacceptableFailureDetected {
        failure: UnacceptableFailure,
        block: BlockNumber,
    },
    ThreatDetected {
        threat_type: ThreatType,
        severity: ThreatSeverity,
        block: BlockNumber,
    },
    SlowCaptureAlert {
        entity: AccountId,
        risk_score: f64,
        block: BlockNumber,
    },
    HealthCheckPassed {
        block: BlockNumber,
    },
    HealthCheckFailed {
        block: BlockNumber,
        failed_invariants: Vec<ProtocolInvariant>,
    },
    AuditRecorded {
        audit_type: AuditType,
        passed: bool,
        block: BlockNumber,
    },
    ReviewCycleStarted {
        cycle: u32,
        block: BlockNumber,
    },
    ReviewCycleCompleted {
        cycle: u32,
        block: BlockNumber,
    },
}

impl SecurityContract {
    /// Create new security contract
    pub fn new(config: SecurityConfig) -> Self {
        Self {
            current_block: 0,
            health_state: ProtocolHealthState::new(0),
            threat_assessments: VecDeque::new(),
            capture_indicators: HashMap::new(),
            stake_history: HashMap::new(),
            vc_history: HashMap::new(),
            audit_records: Vec::new(),
            review_cycles: Vec::new(),
            current_review_cycle: None,
            active_threats: Vec::new(),
            config,
            events: Vec::new(),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(SecurityConfig::default())
    }

    /// Update block number
    pub fn set_block(&mut self, block: BlockNumber) {
        self.current_block = block;
        self.health_state.checked_at = block;
    }

    // =========================================================================
    // INVARIANT CHECKING
    // =========================================================================

    /// Verify all protocol invariants
    pub fn verify_invariants(&mut self) -> Vec<InvariantCheckResult> {
        let results = self.health_state.check_all_invariants();

        // Record violations
        for result in &results {
            if !result.holds {
                self.events.push(SecurityEvent::InvariantViolation {
                    invariant: result.invariant,
                    block: self.current_block,
                    details: format!("Invariant {:?} violated", result.invariant),
                });
            }
        }

        results
    }

    /// Check for unacceptable failures
    pub fn check_unacceptable_failures(&mut self) -> Vec<UnacceptableFailure> {
        let failures = self.health_state.check_unacceptable_failures();

        for failure in &failures {
            self.events.push(SecurityEvent::UnacceptableFailureDetected {
                failure: *failure,
                block: self.current_block,
            });
        }

        failures
    }

    /// Update health state from protocol state
    pub fn update_health_state(
        &mut self,
        exit_possible: bool,
        fork_without_slash: bool,
        decay_active: bool,
        failures_contained: bool,
        emergency_exceeded: bool,
        ossification_allows_exit: bool,
        frozen_assets: bool,
        identity_locked: bool,
        ideology_enforced: bool,
    ) {
        self.health_state.exit_always_possible = exit_possible;
        self.health_state.fork_without_slashing = fork_without_slash;
        self.health_state.decay_active = decay_active;
        self.health_state.failures_contained = failures_contained;
        self.health_state.emergency_duration_exceeded = emergency_exceeded;
        self.health_state.exit_possible_when_ossified = ossification_allows_exit;
        self.health_state.has_frozen_assets = frozen_assets;
        self.health_state.identity_exit_blocked = identity_locked;
        self.health_state.ideological_enforcement = ideology_enforced;
    }

    /// Run full health check
    pub fn run_health_check(&mut self) -> bool {
        let healthy = self.health_state.is_healthy();

        if healthy {
            self.events.push(SecurityEvent::HealthCheckPassed {
                block: self.current_block,
            });
        } else {
            let failed = self.verify_invariants()
                .into_iter()
                .filter(|r| !r.holds)
                .map(|r| r.invariant)
                .collect();

            self.events.push(SecurityEvent::HealthCheckFailed {
                block: self.current_block,
                failed_invariants: failed,
            });
        }

        healthy
    }

    // =========================================================================
    // SLOW CAPTURE DETECTION
    // =========================================================================

    /// Record stake snapshot for capture detection
    pub fn record_stake_snapshot(&mut self, account: &AccountId, stake: Balance) {
        let history = self.stake_history.entry(*account).or_insert_with(Vec::new);
        history.push((self.current_block, stake));

        // Trim history if too long
        if history.len() > self.config.max_stake_history {
            history.remove(0);
        }
    }

    /// Record VC snapshot for capture detection
    pub fn record_vc_snapshot(&mut self, account: &AccountId, vc: u64) {
        let history = self.vc_history.entry(*account).or_insert_with(Vec::new);
        history.push((self.current_block, vc));

        // Trim history if too long
        if history.len() > self.config.max_stake_history {
            history.remove(0);
        }
    }

    /// Calculate capture indicator for an entity
    pub fn calculate_capture_indicator(
        &mut self,
        account: &AccountId,
        current_stake: Balance,
        current_vc: u64,
        current_reputation: u64,
        current_gov_influence: u64,
        total_stake: Balance,
        total_vc: u64,
    ) -> Option<SlowCaptureIndicator> {
        let stake_history = self.stake_history.get(account)?;
        let vc_history = self.vc_history.get(account)?;

        if stake_history.len() < 2 || vc_history.len() < 2 {
            return None;
        }

        // Calculate accumulation rates
        let old_stake = stake_history.first()?.1;
        let stake_change = current_stake.saturating_sub(old_stake) as f64;
        let stake_rate = if total_stake > 0 {
            stake_change / total_stake as f64
        } else {
            0.0
        };

        let old_vc = vc_history.first()?.1;
        let vc_change = current_vc.saturating_sub(old_vc) as f64;
        let vc_rate = if total_vc > 0 {
            vc_change / total_vc as f64
        } else {
            0.0
        };

        let indicator = SlowCaptureIndicator {
            entity: *account,
            stake_accumulation_rate: stake_rate,
            vc_accumulation_rate: vc_rate,
            reputation_accumulation_rate: 0.0, // Would need reputation history
            governance_influence_rate: 0.0,     // Would need governance history
            monitoring_period: self.config.capture_detection_window,
            alert_threshold: self.config.capture_alert_threshold,
        };

        // Check for alert
        if indicator.is_alert() {
            self.events.push(SecurityEvent::SlowCaptureAlert {
                entity: *account,
                risk_score: indicator.capture_risk_score(),
                block: self.current_block,
            });
        }

        self.capture_indicators.insert(*account, indicator.clone());
        Some(indicator)
    }

    /// Get all entities with high capture risk
    pub fn get_high_risk_entities(&self) -> Vec<(&AccountId, f64)> {
        self.capture_indicators
            .iter()
            .filter(|(_, ind)| ind.is_alert())
            .map(|(acc, ind)| (acc, ind.capture_risk_score()))
            .collect()
    }

    // =========================================================================
    // THREAT DETECTION
    // =========================================================================

    /// Report a detected threat
    pub fn report_threat(
        &mut self,
        threat_type: ThreatType,
        severity: ThreatSeverity,
        evidence: String,
        affected_entities: Vec<AccountId>,
    ) {
        let threat = DetectedThreat {
            threat_type: threat_type.clone(),
            severity,
            evidence,
            detected_at: self.current_block,
            affected_entities,
        };

        self.active_threats.push(threat);

        self.events.push(SecurityEvent::ThreatDetected {
            threat_type,
            severity,
            block: self.current_block,
        });
    }

    /// Get mitigations for a threat
    pub fn get_threat_mitigations(&self, threat: &ThreatType) -> Vec<Mitigation> {
        threat.mitigations()
    }

    /// Resolve a threat (mark as handled)
    pub fn resolve_threat(&mut self, threat_type: &ThreatType) -> bool {
        let initial_len = self.active_threats.len();
        self.active_threats.retain(|t| &t.threat_type != threat_type);
        self.active_threats.len() < initial_len
    }

    /// Get active threats
    pub fn get_active_threats(&self) -> &[DetectedThreat] {
        &self.active_threats
    }

    /// Calculate overall threat level
    pub fn calculate_threat_level(&self) -> ThreatLevel {
        // Check for unacceptable failures first - this is always existential
        let has_unacceptable = !self.health_state.check_unacceptable_failures().is_empty();
        if has_unacceptable {
            return ThreatLevel::Existential;
        }

        if self.active_threats.is_empty() {
            return ThreatLevel::Normal;
        }

        let max_severity = self.active_threats
            .iter()
            .map(|t| t.severity)
            .max()
            .unwrap_or(ThreatSeverity::Low);

        let threat_count = self.active_threats.len();

        if max_severity == ThreatSeverity::Critical || threat_count > 5 {
            ThreatLevel::Critical
        } else if max_severity == ThreatSeverity::High || threat_count > 3 {
            ThreatLevel::High
        } else if max_severity == ThreatSeverity::Medium || threat_count > 1 {
            ThreatLevel::Elevated
        } else {
            ThreatLevel::Normal
        }
    }

    /// Create threat assessment
    pub fn create_threat_assessment(&mut self) -> ThreatAssessment {
        let threat_level = self.calculate_threat_level();
        let recommendations = self.generate_recommendations();

        let assessment = ThreatAssessment {
            assessed_at: self.current_block,
            active_threats: self.active_threats.clone(),
            capture_indicators: self.capture_indicators.values().cloned().collect(),
            threat_level,
            recommendations,
        };

        // Store assessment
        self.threat_assessments.push_back(assessment.clone());
        if self.threat_assessments.len() > self.config.max_threat_history {
            self.threat_assessments.pop_front();
        }

        assessment
    }

    /// Generate security recommendations based on current state
    fn generate_recommendations(&self) -> Vec<String> {
        let mut recommendations = Vec::new();

        // Check invariants
        if !self.health_state.exit_always_possible {
            recommendations.push(
                "CRITICAL: Exit path blocked - restore withdrawal functionality immediately".to_string()
            );
        }

        if !self.health_state.fork_without_slashing {
            recommendations.push(
                "CRITICAL: Fork slashing detected - remove fork penalties".to_string()
            );
        }

        if !self.health_state.decay_active {
            recommendations.push(
                "WARNING: Power decay not active - enable decay mechanisms".to_string()
            );
        }

        if self.health_state.emergency_duration_exceeded {
            recommendations.push(
                "CRITICAL: Emergency duration exceeded - end emergency state".to_string()
            );
        }

        // Check threats
        for threat in &self.active_threats {
            let mitigations = threat.threat_type.mitigations();
            recommendations.push(format!(
                "Threat {:?} detected - apply mitigations: {:?}",
                threat.threat_type, mitigations
            ));
        }

        // Check capture risk
        for (account, indicator) in &self.capture_indicators {
            if indicator.is_alert() {
                recommendations.push(format!(
                    "Slow capture risk for {:?} - monitor and consider countermeasures",
                    account
                ));
            }
        }

        recommendations
    }

    // =========================================================================
    // AUDIT TRACKING
    // =========================================================================

    /// Record an audit
    pub fn record_audit(
        &mut self,
        audit_type: AuditType,
        auditor: String,
        passed: bool,
        findings: String,
        report_hash: Hash,
    ) {
        let record = AuditRecord {
            audit_type,
            auditor,
            performed_at: self.current_block,
            passed,
            findings,
            report_hash,
        };

        self.audit_records.push(record);

        self.events.push(SecurityEvent::AuditRecorded {
            audit_type,
            passed,
            block: self.current_block,
        });
    }

    /// Get audits by type
    pub fn get_audits_by_type(&self, audit_type: AuditType) -> Vec<&AuditRecord> {
        self.audit_records
            .iter()
            .filter(|a| a.audit_type == audit_type)
            .collect()
    }

    /// Check if all required audits have been performed
    pub fn all_audits_complete(&self) -> bool {
        for audit_type in AuditType::all() {
            let audits = self.get_audits_by_type(audit_type);
            if audits.is_empty() || !audits.iter().any(|a| a.passed) {
                return false;
            }
        }
        true
    }

    // =========================================================================
    // REVIEW CYCLES
    // =========================================================================

    /// Start a new review cycle
    pub fn start_review_cycle(&mut self) {
        let cycle_number = self.review_cycles.len() as u32 + 1;
        let cycle = ReviewCycle::new(cycle_number, self.current_block);

        self.events.push(SecurityEvent::ReviewCycleStarted {
            cycle: cycle_number,
            block: self.current_block,
        });

        self.current_review_cycle = Some(cycle);
    }

    /// Mark axiom review complete
    pub fn complete_axiom_review(&mut self) {
        if let Some(ref mut cycle) = self.current_review_cycle {
            cycle.axiom_review_complete = true;
            self.check_cycle_completion();
        }
    }

    /// Mark fork consideration complete
    pub fn complete_fork_consideration(&mut self) {
        if let Some(ref mut cycle) = self.current_review_cycle {
            cycle.fork_consideration_complete = true;
            self.check_cycle_completion();
        }
    }

    /// Mark ossification review complete
    pub fn complete_ossification_review(&mut self) {
        if let Some(ref mut cycle) = self.current_review_cycle {
            cycle.ossification_review_complete = true;
            self.check_cycle_completion();
        }
    }

    /// Check if current cycle is complete
    fn check_cycle_completion(&mut self) {
        if let Some(ref cycle) = self.current_review_cycle {
            if cycle.is_complete() {
                self.events.push(SecurityEvent::ReviewCycleCompleted {
                    cycle: cycle.cycle,
                    block: self.current_block,
                });

                self.review_cycles.push(cycle.clone());
                self.current_review_cycle = None;
            }
        }
    }

    /// Check if review is due
    pub fn is_review_due(&self) -> bool {
        if let Some(last_cycle) = self.review_cycles.last() {
            last_cycle.is_due(self.current_block)
        } else {
            // First review should start after initial period
            self.current_block > ReviewCycle::REVIEW_PERIOD_BLOCKS
        }
    }

    // =========================================================================
    // COUNTERMEASURE VERIFICATION
    // =========================================================================

    /// Verify slow capture countermeasures are active
    pub fn verify_countermeasures(&self) -> HashMap<SlowCaptureCountermeasure, bool> {
        let mut status = HashMap::new();

        // These would be checked against actual protocol state
        status.insert(SlowCaptureCountermeasure::TimeWeightedPower, true);
        status.insert(SlowCaptureCountermeasure::VcDecay, self.health_state.decay_active);
        status.insert(SlowCaptureCountermeasure::ReputationDecay, self.health_state.decay_active);
        status.insert(SlowCaptureCountermeasure::ForkNeutrality, self.health_state.fork_without_slashing);
        status.insert(SlowCaptureCountermeasure::SidechainAutonomy, self.health_state.failures_contained);

        status
    }

    /// Get countermeasures that are not active
    pub fn get_inactive_countermeasures(&self) -> Vec<SlowCaptureCountermeasure> {
        self.verify_countermeasures()
            .into_iter()
            .filter(|(_, active)| !*active)
            .map(|(cm, _)| cm)
            .collect()
    }

    // =========================================================================
    // NON-ASSUMPTION VERIFICATION
    // =========================================================================

    /// Verify protocol works without assumptions
    pub fn verify_non_assumptions(&self) -> HashMap<NonAssumption, bool> {
        let mut verified = HashMap::new();

        // Protocol must work even without these assumptions
        for assumption in NonAssumption::all() {
            let works_without = match assumption {
                NonAssumption::HonestMajority => {
                    // Fork and exit must work even with dishonest majority
                    self.health_state.exit_always_possible
                        && self.health_state.fork_without_slashing
                }
                NonAssumption::BenevolentGovernance => {
                    // Exit must work even with hostile governance
                    self.health_state.exit_always_possible
                }
                NonAssumption::AlignedIncentives => {
                    // Decay must work to prevent accumulation
                    self.health_state.decay_active
                }
                NonAssumption::StableIdeology => {
                    // Fork must work for ideological divergence
                    self.health_state.fork_without_slashing
                }
                NonAssumption::PermanentParticipation => {
                    // Exit must always be possible
                    self.health_state.exit_always_possible
                }
            };
            verified.insert(assumption, works_without);
        }

        verified
    }

    // =========================================================================
    // EVENTS
    // =========================================================================

    /// Drain all events
    pub fn drain_events(&mut self) -> Vec<SecurityEvent> {
        std::mem::take(&mut self.events)
    }

    /// Get event count
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

// =============================================================================
// SECURITY CONTRACT ERRORS
// =============================================================================

#[derive(Debug, Clone, thiserror::Error)]
pub enum SecurityError {
    #[error("Invariant violation: {0:?}")]
    InvariantViolation(ProtocolInvariant),

    #[error("Unacceptable failure: {0:?}")]
    UnacceptableFailure(UnacceptableFailure),

    #[error("No active review cycle")]
    NoActiveReviewCycle,

    #[error("Audit not found")]
    AuditNotFound,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_account(seed: u8) -> AccountId {
        AccountId::from_bytes([seed; 32])
    }

    #[test]
    fn test_new_contract() {
        let contract = SecurityContract::with_defaults();
        assert!(contract.health_state.is_healthy());
        assert!(contract.active_threats.is_empty());
    }

    #[test]
    fn test_health_check_pass() {
        let mut contract = SecurityContract::with_defaults();
        contract.set_block(1000);

        assert!(contract.run_health_check());

        let events = contract.drain_events();
        assert!(events.iter().any(|e| matches!(e, SecurityEvent::HealthCheckPassed { .. })));
    }

    #[test]
    fn test_health_check_fail() {
        let mut contract = SecurityContract::with_defaults();
        contract.set_block(1000);

        // Break an invariant
        contract.health_state.exit_always_possible = false;

        assert!(!contract.run_health_check());

        let events = contract.drain_events();
        assert!(events.iter().any(|e| matches!(e, SecurityEvent::HealthCheckFailed { .. })));
    }

    #[test]
    fn test_invariant_verification() {
        let mut contract = SecurityContract::with_defaults();

        let results = contract.verify_invariants();
        assert!(results.iter().all(|r| r.holds));

        // Break invariant
        contract.health_state.fork_without_slashing = false;

        let results = contract.verify_invariants();
        let fork_result = results.iter()
            .find(|r| r.invariant == ProtocolInvariant::ForkWithoutPunishment)
            .unwrap();
        assert!(!fork_result.holds);
    }

    #[test]
    fn test_unacceptable_failure_detection() {
        let mut contract = SecurityContract::with_defaults();

        // No failures initially
        let failures = contract.check_unacceptable_failures();
        assert!(failures.is_empty());

        // Introduce failure
        contract.health_state.has_frozen_assets = true;

        let failures = contract.check_unacceptable_failures();
        assert!(failures.contains(&UnacceptableFailure::FrozenAssets));
    }

    #[test]
    fn test_threat_reporting() {
        let mut contract = SecurityContract::with_defaults();
        contract.set_block(1000);

        contract.report_threat(
            ThreatType::ValidatorCartelization,
            ThreatSeverity::High,
            "50% of stake controlled by 3 entities".to_string(),
            vec![create_account(1), create_account(2), create_account(3)],
        );

        assert_eq!(contract.get_active_threats().len(), 1);

        let events = contract.drain_events();
        assert!(events.iter().any(|e| matches!(e, SecurityEvent::ThreatDetected { .. })));
    }

    #[test]
    fn test_threat_resolution() {
        let mut contract = SecurityContract::with_defaults();

        contract.report_threat(
            ThreatType::ValidatorCartelization,
            ThreatSeverity::Medium,
            "Test".to_string(),
            vec![],
        );

        assert_eq!(contract.get_active_threats().len(), 1);

        let resolved = contract.resolve_threat(&ThreatType::ValidatorCartelization);
        assert!(resolved);
        assert!(contract.get_active_threats().is_empty());
    }

    #[test]
    fn test_threat_level_calculation() {
        let mut contract = SecurityContract::with_defaults();

        // No threats = normal
        assert_eq!(contract.calculate_threat_level(), ThreatLevel::Normal);

        // Add low threat
        contract.report_threat(
            ThreatType::ProposalSpam,
            ThreatSeverity::Low,
            "Minor spam".to_string(),
            vec![],
        );
        assert_eq!(contract.calculate_threat_level(), ThreatLevel::Normal);

        // Add critical threat
        contract.report_threat(
            ThreatType::ValidatorCartelization,
            ThreatSeverity::Critical,
            "Major cartel".to_string(),
            vec![],
        );
        assert_eq!(contract.calculate_threat_level(), ThreatLevel::Critical);
    }

    #[test]
    fn test_existential_threat_level() {
        let mut contract = SecurityContract::with_defaults();

        // Unacceptable failure = existential threat
        contract.health_state.has_frozen_assets = true;

        assert_eq!(contract.calculate_threat_level(), ThreatLevel::Existential);
    }

    #[test]
    fn test_slow_capture_detection() {
        let mut contract = SecurityContract::with_defaults();
        let account = create_account(1);

        // Record initial snapshot
        contract.set_block(0);
        contract.record_stake_snapshot(&account, 1000);
        contract.record_vc_snapshot(&account, 10);

        // Record later snapshot with growth
        contract.set_block(1000);
        contract.record_stake_snapshot(&account, 10000);
        contract.record_vc_snapshot(&account, 100);

        let indicator = contract.calculate_capture_indicator(
            &account,
            10000,  // current stake
            100,    // current VC
            50,     // current reputation
            10,     // current gov influence
            100000, // total stake
            1000,   // total VC
        );

        assert!(indicator.is_some());
    }

    #[test]
    fn test_audit_recording() {
        let mut contract = SecurityContract::with_defaults();
        contract.set_block(1000);

        contract.record_audit(
            AuditType::Cryptography,
            "Security Firm A".to_string(),
            true,
            "All crypto primitives secure".to_string(),
            Hash::hash(b"report"),
        );

        let audits = contract.get_audits_by_type(AuditType::Cryptography);
        assert_eq!(audits.len(), 1);
        assert!(audits[0].passed);
    }

    #[test]
    fn test_all_audits_complete() {
        let mut contract = SecurityContract::with_defaults();

        assert!(!contract.all_audits_complete());

        // Record all required audits
        for audit_type in AuditType::all() {
            contract.record_audit(
                audit_type,
                "Auditor".to_string(),
                true,
                "Passed".to_string(),
                Hash::hash(b"report"),
            );
        }

        assert!(contract.all_audits_complete());
    }

    #[test]
    fn test_review_cycle() {
        let mut contract = SecurityContract::with_defaults();
        contract.set_block(1000);

        contract.start_review_cycle();
        assert!(contract.current_review_cycle.is_some());

        contract.complete_axiom_review();
        contract.complete_fork_consideration();
        contract.complete_ossification_review();

        // Cycle should be complete and archived
        assert!(contract.current_review_cycle.is_none());
        assert_eq!(contract.review_cycles.len(), 1);
    }

    #[test]
    fn test_countermeasure_verification() {
        let mut contract = SecurityContract::with_defaults();

        let status = contract.verify_countermeasures();
        assert!(status.values().all(|v| *v));

        // Disable decay
        contract.health_state.decay_active = false;

        let inactive = contract.get_inactive_countermeasures();
        assert!(inactive.contains(&SlowCaptureCountermeasure::VcDecay));
        assert!(inactive.contains(&SlowCaptureCountermeasure::ReputationDecay));
    }

    #[test]
    fn test_non_assumption_verification() {
        let contract = SecurityContract::with_defaults();

        let verified = contract.verify_non_assumptions();

        // All should pass with healthy state
        assert!(verified.values().all(|v| *v));
    }

    #[test]
    fn test_threat_assessment_creation() {
        let mut contract = SecurityContract::with_defaults();
        contract.set_block(1000);

        contract.report_threat(
            ThreatType::VoterApathy,
            ThreatSeverity::Medium,
            "Low participation".to_string(),
            vec![],
        );

        let assessment = contract.create_threat_assessment();
        assert_eq!(assessment.active_threats.len(), 1);
        assert!(assessment.threat_level >= ThreatLevel::Normal);
    }

    #[test]
    fn test_recommendations_generation() {
        let mut contract = SecurityContract::with_defaults();

        // Break something
        contract.health_state.exit_always_possible = false;

        let assessment = contract.create_threat_assessment();
        assert!(!assessment.recommendations.is_empty());
        assert!(assessment.recommendations.iter().any(|r| r.contains("Exit path blocked")));
    }

    #[test]
    fn test_update_health_state() {
        let mut contract = SecurityContract::with_defaults();

        contract.update_health_state(
            true,   // exit
            true,   // fork
            true,   // decay
            true,   // contained
            false,  // emergency
            true,   // ossification exit
            false,  // frozen
            false,  // identity locked
            false,  // ideology
        );

        assert!(contract.health_state.is_healthy());

        contract.update_health_state(
            false,  // exit blocked
            true, true, true, false, true, false, false, false,
        );

        assert!(!contract.health_state.is_healthy());
    }

    #[test]
    fn test_review_due_check() {
        let mut contract = SecurityContract::with_defaults();

        // No review yet, check if due
        contract.set_block(0);
        assert!(!contract.is_review_due());

        // After review period
        contract.set_block(ReviewCycle::REVIEW_PERIOD_BLOCKS + 1);
        assert!(contract.is_review_due());
    }

    #[test]
    fn test_event_draining() {
        let mut contract = SecurityContract::with_defaults();
        contract.set_block(1000);

        contract.run_health_check();
        assert!(contract.event_count() > 0);

        let events = contract.drain_events();
        assert!(!events.is_empty());
        assert_eq!(contract.event_count(), 0);
    }

    #[test]
    fn test_get_threat_mitigations() {
        let contract = SecurityContract::with_defaults();

        let mitigations = contract.get_threat_mitigations(&ThreatType::ValidatorCartelization);
        assert!(mitigations.contains(&Mitigation::VcDecay));
        assert!(mitigations.contains(&Mitigation::StakeCaps));
    }
}
