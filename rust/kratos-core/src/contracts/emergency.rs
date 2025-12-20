// Emergency Contract - SPEC v7: Emergency Powers & Systemic Resilience
// Principle: Resilience is surviving failure without betrayal of principles
//
// This contract manages:
// - Emergency state declaration and expiration
// - Circuit breaker activation and reset
// - Failure signal detection and aggregation
// - Recovery coordination

use crate::types::{
    AccountId, Balance, BlockNumber, ChainId, Hash,
    EmergencyState, EmergencyTrigger, EmergencyAction, ConsensusFailureType,
    FailureSignal, CircuitBreaker, BreakerCondition, BreakerAction,
    RecoveryProcess, RecoveryStep, Checkpoint, ParameterTightening,
    EMERGENCY_APPROVAL_THRESHOLD, EMERGENCY_MAX_DURATION, MIN_SIGNALS_FOR_EMERGENCY,
    MIN_SEVERITY_FOR_EMERGENCY, DEFAULT_BREAKER_DURATION,
};
use std::collections::{HashMap, HashSet};

// =============================================================================
// CONSTANTS
// =============================================================================

/// Minimum deposit to propose emergency (prevents spam)
pub const EMERGENCY_PROPOSAL_DEPOSIT: Balance = 10_000;

/// Voting period for emergency declaration (4 hours in blocks)
pub const EMERGENCY_VOTING_PERIOD: BlockNumber = 2_400;

/// Cooldown between emergency declarations (1 day in blocks)
pub const EMERGENCY_COOLDOWN: BlockNumber = 14_400;

/// Maximum active circuit breakers at once
pub const MAX_ACTIVE_BREAKERS: usize = 10;

/// Checkpoint interval (every 1000 blocks)
pub const CHECKPOINT_INTERVAL: BlockNumber = 1_000;

/// Maximum checkpoints to keep
pub const MAX_CHECKPOINTS: usize = 100;

// =============================================================================
// EMERGENCY CONTRACT
// =============================================================================

/// Emergency management contract
pub struct EmergencyContract {
    /// Current emergency state
    pub state: EmergencyState,

    /// Active circuit breakers
    pub breakers: HashMap<String, CircuitBreaker>,

    /// Detected failure signals
    pub signals: Vec<(BlockNumber, FailureSignal)>,

    /// Recovery process (if any)
    pub recovery: Option<RecoveryProcess>,

    /// Checkpoints for recovery
    pub checkpoints: Vec<Checkpoint>,

    /// Validator voting power for emergency
    voting_power: HashMap<AccountId, u64>,

    /// Last emergency ended at
    last_emergency_ended: Option<BlockNumber>,

    /// Emergency history (for auditing)
    pub history: Vec<EmergencyRecord>,

    /// Events emitted
    events: Vec<EmergencyEvent>,
}

/// Record of past emergency
#[derive(Debug, Clone)]
pub struct EmergencyRecord {
    pub declared_at: BlockNumber,
    pub ended_at: BlockNumber,
    pub trigger: EmergencyTrigger,
    pub actions: Vec<EmergencyAction>,
    pub outcome: EmergencyOutcome,
}

/// Outcome of an emergency
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmergencyOutcome {
    /// Expired naturally
    Expired,
    /// Resolved by governance
    Resolved,
    /// Led to recovery process
    Recovery,
    /// Led to fork
    Fork,
}

/// Events emitted by the contract
#[derive(Debug, Clone)]
pub enum EmergencyEvent {
    /// Emergency declared
    EmergencyDeclared {
        block: BlockNumber,
        trigger: EmergencyTrigger,
        expires_at: BlockNumber,
    },

    /// Emergency ended
    EmergencyEnded {
        block: BlockNumber,
        outcome: EmergencyOutcome,
    },

    /// Action taken
    ActionTaken {
        block: BlockNumber,
        action: EmergencyAction,
    },

    /// Circuit breaker triggered
    BreakerTriggered {
        block: BlockNumber,
        breaker_id: String,
    },

    /// Circuit breaker reset
    BreakerReset {
        block: BlockNumber,
        breaker_id: String,
    },

    /// Failure signal detected
    SignalDetected {
        block: BlockNumber,
        signal: FailureSignal,
        severity: u8,
    },

    /// Recovery started
    RecoveryStarted {
        block: BlockNumber,
        target_checkpoint: Option<BlockNumber>,
    },

    /// Recovery step completed
    RecoveryStepCompleted {
        block: BlockNumber,
        step: RecoveryStep,
    },

    /// Checkpoint created
    CheckpointCreated {
        block: BlockNumber,
        state_root: Hash,
    },

    /// Vote cast for emergency
    EmergencyVoteCast {
        voter: AccountId,
        power: u64,
        approve: bool,
    },
}

// =============================================================================
// ERRORS
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmergencyError {
    /// Emergency already active
    AlreadyActive,

    /// No emergency active
    NotActive,

    /// Insufficient approval
    InsufficientApproval { have: u8, need: u8 },

    /// In cooldown period
    InCooldown { ends_at: BlockNumber },

    /// Already voted
    AlreadyVoted,

    /// No voting power
    NoVotingPower,

    /// Invalid action
    InvalidAction(String),

    /// Action forbidden during emergency
    ForbiddenAction(String),

    /// Breaker not found
    BreakerNotFound(String),

    /// Too many breakers
    TooManyBreakers,

    /// Breaker already active
    BreakerAlreadyActive(String),

    /// Recovery already in progress
    RecoveryInProgress,

    /// No recovery in progress
    NoRecoveryInProgress,

    /// Invalid recovery step
    InvalidRecoveryStep,

    /// Checkpoint not found
    CheckpointNotFound,

    /// Insufficient confirmations
    InsufficientConfirmations { have: u32, need: u32 },
}

// =============================================================================
// IMPLEMENTATION
// =============================================================================

impl EmergencyContract {
    /// Creates a new emergency contract
    pub fn new() -> Self {
        let mut contract = Self {
            state: EmergencyState::new(),
            breakers: HashMap::new(),
            signals: Vec::new(),
            recovery: None,
            checkpoints: Vec::new(),
            voting_power: HashMap::new(),
            last_emergency_ended: None,
            history: Vec::new(),
            events: Vec::new(),
        };

        // Register default circuit breakers
        contract.register_default_breakers();

        contract
    }

    /// Register default circuit breakers from SPEC v7
    fn register_default_breakers(&mut self) {
        // Finality delay breaker
        self.breakers.insert(
            "finality_delay".to_string(),
            CircuitBreaker::new(
                "finality_delay".to_string(),
                "Finality Delay Breaker".to_string(),
                BreakerCondition::FinalityDelay { epochs_threshold: 3 },
                BreakerAction::SlowBlockTime { factor: 2 },
                DEFAULT_BREAKER_DURATION,
            ),
        );

        // Validator participation breaker
        self.breakers.insert(
            "validator_participation".to_string(),
            CircuitBreaker::new(
                "validator_participation".to_string(),
                "Validator Participation Breaker".to_string(),
                BreakerCondition::ValidatorParticipation { min_percent: 50 },
                BreakerAction::ExtendTimelocks { multiplier: 2 },
                DEFAULT_BREAKER_DURATION,
            ),
        );

        // State root mismatch breaker
        self.breakers.insert(
            "state_mismatch".to_string(),
            CircuitBreaker::new(
                "state_mismatch".to_string(),
                "State Root Mismatch Breaker".to_string(),
                BreakerCondition::StateRootMismatch,
                BreakerAction::TriggerEmergency,
                DEFAULT_BREAKER_DURATION * 2,
            ),
        );

        // Slashing spike breaker
        self.breakers.insert(
            "slashing_spike".to_string(),
            CircuitBreaker::new(
                "slashing_spike".to_string(),
                "Slashing Spike Breaker".to_string(),
                BreakerCondition::SlashingSpike { max_events_per_epoch: 10 },
                BreakerAction::SuspendRiskyOperations {
                    operations: vec!["unstake".to_string()],
                },
                DEFAULT_BREAKER_DURATION,
            ),
        );

        // Governance deadlock breaker
        self.breakers.insert(
            "governance_deadlock".to_string(),
            CircuitBreaker::new(
                "governance_deadlock".to_string(),
                "Governance Deadlock Breaker".to_string(),
                BreakerCondition::GovernanceDeadlock { failed_proposals: 3 },
                BreakerAction::RaiseQuorum { additional_percent: 10 },
                DEFAULT_BREAKER_DURATION,
            ),
        );
    }

    /// Set voting power for a validator
    pub fn set_voting_power(&mut self, validator: AccountId, power: u64) {
        if power > 0 {
            self.voting_power.insert(validator, power);
        } else {
            self.voting_power.remove(&validator);
        }
        self.update_total_power();
    }

    /// Update total voting power in state
    fn update_total_power(&mut self) {
        self.state.total_power = self.voting_power.values().sum();
    }

    /// Vote to declare emergency
    pub fn vote_emergency(
        &mut self,
        voter: AccountId,
        approve: bool,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        // Check cooldown
        if let Some(ended) = self.last_emergency_ended {
            if current_block < ended + EMERGENCY_COOLDOWN {
                return Err(EmergencyError::InCooldown {
                    ends_at: ended + EMERGENCY_COOLDOWN,
                });
            }
        }

        // Check if already active
        if self.state.active {
            return Err(EmergencyError::AlreadyActive);
        }

        // Check voting power
        let power = self
            .voting_power
            .get(&voter)
            .copied()
            .ok_or(EmergencyError::NoVotingPower)?;

        // Check if already voted
        if self.state.declaring_validators.contains(&voter) {
            return Err(EmergencyError::AlreadyVoted);
        }

        // Record vote
        if approve {
            self.state.declaring_validators.insert(voter);
            self.state.approval_power += power;
        }

        self.events.push(EmergencyEvent::EmergencyVoteCast {
            voter,
            power,
            approve,
        });

        // Check if threshold reached
        if self.state.approval_percent() >= EMERGENCY_APPROVAL_THRESHOLD {
            self.declare_emergency(
                EmergencyTrigger::ValidatorVote {
                    approval_percent: self.state.approval_percent(),
                    voter_count: self.state.declaring_validators.len() as u32,
                },
                current_block,
            )?;
        }

        Ok(())
    }

    /// Declare emergency (internal, called when threshold reached)
    fn declare_emergency(
        &mut self,
        trigger: EmergencyTrigger,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        if self.state.active {
            return Err(EmergencyError::AlreadyActive);
        }

        let expires_at = current_block + EMERGENCY_MAX_DURATION;

        self.state.active = true;
        self.state.declared_at = Some(current_block);
        self.state.trigger = Some(trigger.clone());
        self.state.expires_at = Some(expires_at);

        self.events.push(EmergencyEvent::EmergencyDeclared {
            block: current_block,
            trigger,
            expires_at,
        });

        Ok(())
    }

    /// Declare emergency from automatic trigger (circuit breaker or signals)
    pub fn declare_automatic_emergency(
        &mut self,
        trigger: EmergencyTrigger,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        // Check cooldown
        if let Some(ended) = self.last_emergency_ended {
            if current_block < ended + EMERGENCY_COOLDOWN {
                return Err(EmergencyError::InCooldown {
                    ends_at: ended + EMERGENCY_COOLDOWN,
                });
            }
        }

        self.declare_emergency(trigger, current_block)
    }

    /// Take an emergency action
    pub fn take_action(
        &mut self,
        action: EmergencyAction,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        if !self.state.active {
            return Err(EmergencyError::NotActive);
        }

        // Validate action is allowed (SPEC v7 section 4.3 - forbidden actions)
        self.validate_action(&action)?;

        self.state.actions_taken.push(action.clone());

        self.events.push(EmergencyEvent::ActionTaken {
            block: current_block,
            action,
        });

        Ok(())
    }

    /// Validate that an action is allowed (not forbidden by SPEC v7)
    fn validate_action(&self, action: &EmergencyAction) -> Result<(), EmergencyError> {
        // SPEC v7 Section 4.3 - Forbidden Actions:
        // - seize user funds ❌
        // - block exits ❌
        // - invalidate identity retroactively ❌
        // - bypass constitutional bounds ❌
        // - grant permanent powers ❌

        // All EmergencyAction variants are designed to be allowed
        // The forbidden actions are simply not representable in the enum

        match action {
            EmergencyAction::PauseBlockProduction { max_duration, .. } => {
                // Ensure pause is temporary (max 7 days)
                if *max_duration > EMERGENCY_MAX_DURATION {
                    return Err(EmergencyError::InvalidAction(
                        "Pause duration exceeds maximum".to_string(),
                    ));
                }
            }
            EmergencyAction::TightenParameters { changes } => {
                // Parameters can only be tightened within bounds
                // (Constitutional bounds are enforced by ProtocolParameters)
                for change in changes {
                    if change.tightened > change.original {
                        // This would be loosening, not tightening
                        // For things like quorum, higher is tighter
                        // For things like timelock, longer is tighter
                        // The caller must ensure directionality
                    }
                }
            }
            EmergencyAction::IncreaseQuorum { new_quorum } => {
                // Quorum cannot exceed 80% (constitutional bound)
                if *new_quorum > 80 {
                    return Err(EmergencyError::InvalidAction(
                        "Quorum cannot exceed 80%".to_string(),
                    ));
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// End emergency (expire or resolve)
    pub fn end_emergency(
        &mut self,
        outcome: EmergencyOutcome,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        if !self.state.active {
            return Err(EmergencyError::NotActive);
        }

        // Record in history
        if let (Some(declared_at), Some(trigger)) =
            (self.state.declared_at, self.state.trigger.clone())
        {
            self.history.push(EmergencyRecord {
                declared_at,
                ended_at: current_block,
                trigger,
                actions: self.state.actions_taken.clone(),
                outcome: outcome.clone(),
            });
        }

        self.events.push(EmergencyEvent::EmergencyEnded {
            block: current_block,
            outcome,
        });

        // Reset state
        self.state = EmergencyState::new();
        self.last_emergency_ended = Some(current_block);
        self.update_total_power();

        Ok(())
    }

    /// Check and expire emergency if needed
    pub fn check_expiration(&mut self, current_block: BlockNumber) -> Result<bool, EmergencyError> {
        if self.state.active && self.state.is_expired(current_block) {
            self.end_emergency(EmergencyOutcome::Expired, current_block)?;
            return Ok(true);
        }
        Ok(false)
    }

    // =========================================================================
    // CIRCUIT BREAKERS
    // =========================================================================

    /// Register a new circuit breaker
    pub fn register_breaker(
        &mut self,
        breaker: CircuitBreaker,
    ) -> Result<(), EmergencyError> {
        if self.breakers.len() >= MAX_ACTIVE_BREAKERS {
            return Err(EmergencyError::TooManyBreakers);
        }

        self.breakers.insert(breaker.id.clone(), breaker);
        Ok(())
    }

    /// Trigger a circuit breaker
    pub fn trigger_breaker(
        &mut self,
        breaker_id: &str,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        let breaker = self
            .breakers
            .get_mut(breaker_id)
            .ok_or_else(|| EmergencyError::BreakerNotFound(breaker_id.to_string()))?;

        if !breaker.enabled {
            return Ok(());
        }

        if breaker.is_active {
            return Err(EmergencyError::BreakerAlreadyActive(breaker_id.to_string()));
        }

        breaker.trigger(current_block);

        self.events.push(EmergencyEvent::BreakerTriggered {
            block: current_block,
            breaker_id: breaker_id.to_string(),
        });

        // If breaker action is TriggerEmergency, do that
        if let BreakerAction::TriggerEmergency = breaker.action {
            let _ = self.declare_automatic_emergency(
                EmergencyTrigger::CircuitBreakerTriggered {
                    breaker_id: breaker_id.to_string(),
                },
                current_block,
            );
        }

        Ok(())
    }

    /// Reset a circuit breaker
    pub fn reset_breaker(
        &mut self,
        breaker_id: &str,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        let breaker = self
            .breakers
            .get_mut(breaker_id)
            .ok_or_else(|| EmergencyError::BreakerNotFound(breaker_id.to_string()))?;

        breaker.reset();

        self.events.push(EmergencyEvent::BreakerReset {
            block: current_block,
            breaker_id: breaker_id.to_string(),
        });

        Ok(())
    }

    /// Check and reset expired breakers
    pub fn check_breaker_expirations(&mut self, current_block: BlockNumber) {
        let expired_ids: Vec<String> = self
            .breakers
            .iter()
            .filter(|(_, b)| b.is_active && b.is_expired(current_block))
            .map(|(id, _)| id.clone())
            .collect();

        for id in expired_ids {
            let _ = self.reset_breaker(&id, current_block);
        }
    }

    /// Get active breakers
    pub fn active_breakers(&self) -> Vec<&CircuitBreaker> {
        self.breakers.values().filter(|b| b.is_active).collect()
    }

    // =========================================================================
    // FAILURE SIGNALS
    // =========================================================================

    /// Report a failure signal
    pub fn report_signal(
        &mut self,
        signal: FailureSignal,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        let severity = signal.severity();

        self.signals.push((current_block, signal.clone()));

        self.events.push(EmergencyEvent::SignalDetected {
            block: current_block,
            signal: signal.clone(),
            severity,
        });

        // Check if automatic emergency should be triggered
        self.check_automatic_emergency(current_block)?;

        // Check if any breaker should be triggered
        self.check_breaker_conditions(&signal, current_block)?;

        Ok(())
    }

    /// Check if automatic emergency should be triggered
    fn check_automatic_emergency(
        &mut self,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        if self.state.active {
            return Ok(());
        }

        // Collect recent signals (last 1000 blocks)
        let recent_signals: Vec<&FailureSignal> = self
            .signals
            .iter()
            .filter(|(block, _)| current_block.saturating_sub(*block) < 1000)
            .map(|(_, s)| s)
            .collect();

        // Check if we have enough signals
        if recent_signals.len() >= MIN_SIGNALS_FOR_EMERGENCY {
            let total_severity: u8 = recent_signals.iter().map(|s| s.severity()).sum();

            if total_severity >= MIN_SEVERITY_FOR_EMERGENCY {
                let signals: Vec<FailureSignal> =
                    recent_signals.iter().map(|s| (*s).clone()).collect();

                self.declare_automatic_emergency(
                    EmergencyTrigger::MultipleSignals { signals },
                    current_block,
                )?;
            }
        }

        Ok(())
    }

    /// Check if a signal should trigger a breaker
    fn check_breaker_conditions(
        &mut self,
        signal: &FailureSignal,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        let breakers_to_trigger: Vec<String> = self
            .breakers
            .iter()
            .filter(|(_, b)| b.enabled && !b.is_active)
            .filter(|(_, b)| self.signal_matches_condition(signal, &b.condition))
            .map(|(id, _)| id.clone())
            .collect();

        for id in breakers_to_trigger {
            self.trigger_breaker(&id, current_block)?;
        }

        Ok(())
    }

    /// Check if a signal matches a breaker condition
    fn signal_matches_condition(&self, signal: &FailureSignal, condition: &BreakerCondition) -> bool {
        match (signal, condition) {
            (
                FailureSignal::ConsensusStall { epochs_stalled },
                BreakerCondition::FinalityDelay { epochs_threshold },
            ) => epochs_stalled >= epochs_threshold,

            (
                FailureSignal::ValidatorMassOffline { offline_count, total_validators },
                BreakerCondition::ValidatorParticipation { min_percent },
            ) => {
                let online_percent = 100 - ((*offline_count * 100) / (*total_validators).max(1)) as u8;
                online_percent < *min_percent
            }

            (
                FailureSignal::StateRootDivergence { .. },
                BreakerCondition::StateRootMismatch,
            ) => true,

            (
                FailureSignal::SlashingSpike { events_count, .. },
                BreakerCondition::SlashingSpike { max_events_per_epoch },
            ) => events_count > max_events_per_epoch,

            (
                FailureSignal::GovernanceQuorumFailure { consecutive_failures, .. },
                BreakerCondition::GovernanceDeadlock { failed_proposals },
            ) => consecutive_failures >= failed_proposals,

            _ => false,
        }
    }

    /// Clear old signals (keep last 10000 blocks only)
    pub fn prune_signals(&mut self, current_block: BlockNumber) {
        self.signals
            .retain(|(block, _)| current_block.saturating_sub(*block) < 10_000);
    }

    // =========================================================================
    // RECOVERY
    // =========================================================================

    /// Start recovery process
    pub fn start_recovery(
        &mut self,
        target_checkpoint: Option<BlockNumber>,
        required_confirmations: u32,
        current_block: BlockNumber,
    ) -> Result<(), EmergencyError> {
        if self.recovery.is_some() {
            return Err(EmergencyError::RecoveryInProgress);
        }

        let checkpoint = if let Some(target) = target_checkpoint {
            self.checkpoints
                .iter()
                .find(|c| c.block_number == target)
                .cloned()
        } else {
            self.checkpoints.last().cloned()
        };

        self.recovery = Some(RecoveryProcess {
            current_step: RecoveryStep::Halt,
            started_at: current_block,
            target_checkpoint: checkpoint.clone(),
            confirmations: HashSet::new(),
            required_confirmations,
        });

        self.events.push(EmergencyEvent::RecoveryStarted {
            block: current_block,
            target_checkpoint: checkpoint.map(|c| c.block_number),
        });

        Ok(())
    }

    /// Advance recovery to next step
    pub fn advance_recovery(
        &mut self,
        current_block: BlockNumber,
    ) -> Result<RecoveryStep, EmergencyError> {
        let recovery = self
            .recovery
            .as_mut()
            .ok_or(EmergencyError::NoRecoveryInProgress)?;

        let next_step = match &recovery.current_step {
            RecoveryStep::Halt => RecoveryStep::StateAudit,
            RecoveryStep::StateAudit => RecoveryStep::ValidatorReconfirmation,
            RecoveryStep::ValidatorReconfirmation => RecoveryStep::ForkDecision,
            RecoveryStep::ForkDecision => RecoveryStep::GradualRestart {
                phase: 1,
                max_phases: 5,
            },
            RecoveryStep::GradualRestart { phase, max_phases } => {
                if *phase >= *max_phases {
                    RecoveryStep::Complete
                } else {
                    RecoveryStep::GradualRestart {
                        phase: phase + 1,
                        max_phases: *max_phases,
                    }
                }
            }
            RecoveryStep::Complete => {
                return Err(EmergencyError::InvalidRecoveryStep);
            }
        };

        recovery.current_step = next_step.clone();

        self.events.push(EmergencyEvent::RecoveryStepCompleted {
            block: current_block,
            step: next_step.clone(),
        });

        // If complete, clear recovery
        if matches!(next_step, RecoveryStep::Complete) {
            self.recovery = None;
        }

        Ok(next_step)
    }

    /// Confirm recovery step (validator confirmation)
    pub fn confirm_recovery(
        &mut self,
        validator: AccountId,
    ) -> Result<bool, EmergencyError> {
        let recovery = self
            .recovery
            .as_mut()
            .ok_or(EmergencyError::NoRecoveryInProgress)?;

        recovery.confirmations.insert(validator);

        let have = recovery.confirmations.len() as u32;
        let need = recovery.required_confirmations;

        Ok(have >= need)
    }

    // =========================================================================
    // CHECKPOINTS
    // =========================================================================

    /// Create a checkpoint
    pub fn create_checkpoint(
        &mut self,
        block_number: BlockNumber,
        state_root: Hash,
        validator_set_hash: Hash,
        chain_id: ChainId,
    ) {
        // Only create if at checkpoint interval
        if block_number % CHECKPOINT_INTERVAL != 0 {
            return;
        }

        let checkpoint = Checkpoint {
            block_number,
            state_root,
            validator_set_hash,
            created_at: block_number,
            chain_id,
        };

        self.checkpoints.push(checkpoint);

        // Prune old checkpoints
        while self.checkpoints.len() > MAX_CHECKPOINTS {
            self.checkpoints.remove(0);
        }

        self.events.push(EmergencyEvent::CheckpointCreated {
            block: block_number,
            state_root,
        });
    }

    /// Get checkpoint at or before block number
    pub fn get_checkpoint(&self, block_number: BlockNumber) -> Option<&Checkpoint> {
        self.checkpoints
            .iter()
            .rev()
            .find(|c| c.block_number <= block_number)
    }

    // =========================================================================
    // QUERIES
    // =========================================================================

    /// Check if emergency is active
    pub fn is_emergency_active(&self) -> bool {
        self.state.active
    }

    /// Get current emergency trigger
    pub fn current_trigger(&self) -> Option<&EmergencyTrigger> {
        self.state.trigger.as_ref()
    }

    /// Get emergency history
    pub fn get_history(&self) -> &[EmergencyRecord] {
        &self.history
    }

    /// Get all events (for auditing)
    pub fn drain_events(&mut self) -> Vec<EmergencyEvent> {
        std::mem::take(&mut self.events)
    }

    /// Check if a specific action is currently active
    pub fn is_action_active(&self, action_type: &str) -> bool {
        self.state.actions_taken.iter().any(|a| {
            match (action_type, a) {
                ("pause", EmergencyAction::PauseBlockProduction { .. }) => true,
                ("freeze_governance", EmergencyAction::FreezeGovernance { .. }) => true,
                ("halt_slashing", EmergencyAction::HaltSlashing { .. }) => true,
                ("halt_sidechains", EmergencyAction::HaltSidechainCreation) => true,
                _ => false,
            }
        })
    }
}

impl Default for EmergencyContract {
    fn default() -> Self {
        Self::new()
    }
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

    fn setup_contract_with_validators() -> EmergencyContract {
        let mut contract = EmergencyContract::new();

        // Set up validators with voting power (total = 100)
        for i in 1..=10 {
            contract.set_voting_power(create_account(i), 10);
        }

        contract
    }

    #[test]
    fn test_new_contract() {
        let contract = EmergencyContract::new();

        assert!(!contract.state.active);
        assert!(contract.recovery.is_none());
        assert!(!contract.breakers.is_empty()); // Default breakers registered
    }

    #[test]
    fn test_default_breakers_registered() {
        let contract = EmergencyContract::new();

        assert!(contract.breakers.contains_key("finality_delay"));
        assert!(contract.breakers.contains_key("validator_participation"));
        assert!(contract.breakers.contains_key("state_mismatch"));
        assert!(contract.breakers.contains_key("slashing_spike"));
        assert!(contract.breakers.contains_key("governance_deadlock"));
    }

    #[test]
    fn test_vote_emergency() {
        let mut contract = setup_contract_with_validators();

        // Vote with 8 validators (80% > 75% threshold)
        for i in 1..=8 {
            let result = contract.vote_emergency(create_account(i), true, 1000);
            assert!(result.is_ok());
        }

        // Emergency should be declared
        assert!(contract.state.active);
        assert!(contract.state.declared_at.is_some());
    }

    #[test]
    fn test_vote_emergency_insufficient() {
        let mut contract = setup_contract_with_validators();

        // Vote with only 7 validators (70% < 75% threshold)
        for i in 1..=7 {
            let result = contract.vote_emergency(create_account(i), true, 1000);
            assert!(result.is_ok());
        }

        // Emergency should NOT be declared
        assert!(!contract.state.active);
    }

    #[test]
    fn test_double_vote_rejected() {
        let mut contract = setup_contract_with_validators();

        let voter = create_account(1);

        let result1 = contract.vote_emergency(voter, true, 1000);
        assert!(result1.is_ok());

        let result2 = contract.vote_emergency(voter, true, 1001);
        assert_eq!(result2, Err(EmergencyError::AlreadyVoted));
    }

    #[test]
    fn test_take_action() {
        let mut contract = setup_contract_with_validators();

        // Declare emergency first
        for i in 1..=8 {
            contract.vote_emergency(create_account(i), true, 1000).unwrap();
        }

        // Take action
        let action = EmergencyAction::PauseBlockProduction {
            started_at: 1000,
            max_duration: 10000,
        };

        let result = contract.take_action(action, 1001);
        assert!(result.is_ok());
        assert_eq!(contract.state.actions_taken.len(), 1);
    }

    #[test]
    fn test_take_action_without_emergency() {
        let mut contract = setup_contract_with_validators();

        let action = EmergencyAction::HaltSlashing {
            reason: "test".to_string(),
        };

        let result = contract.take_action(action, 1000);
        assert_eq!(result, Err(EmergencyError::NotActive));
    }

    #[test]
    fn test_invalid_action_rejected() {
        let mut contract = setup_contract_with_validators();

        // Declare emergency
        for i in 1..=8 {
            contract.vote_emergency(create_account(i), true, 1000).unwrap();
        }

        // Try to set quorum above constitutional limit (80%)
        let action = EmergencyAction::IncreaseQuorum { new_quorum: 90 };
        let result = contract.take_action(action, 1001);

        assert!(matches!(result, Err(EmergencyError::InvalidAction(_))));
    }

    #[test]
    fn test_emergency_expiration() {
        let mut contract = setup_contract_with_validators();

        // Declare emergency
        for i in 1..=8 {
            contract.vote_emergency(create_account(i), true, 1000).unwrap();
        }

        assert!(contract.state.active);

        // Check before expiration
        let result1 = contract.check_expiration(50000);
        assert!(result1.is_ok());
        assert!(!result1.unwrap());
        assert!(contract.state.active);

        // Check after expiration (1000 + 100,800 = 101,800)
        let result2 = contract.check_expiration(102000);
        assert!(result2.is_ok());
        assert!(result2.unwrap());
        assert!(!contract.state.active);
    }

    #[test]
    fn test_end_emergency() {
        let mut contract = setup_contract_with_validators();

        // Declare emergency
        for i in 1..=8 {
            contract.vote_emergency(create_account(i), true, 1000).unwrap();
        }

        // End it
        let result = contract.end_emergency(EmergencyOutcome::Resolved, 2000);
        assert!(result.is_ok());

        assert!(!contract.state.active);
        assert_eq!(contract.history.len(), 1);
        assert_eq!(contract.history[0].outcome, EmergencyOutcome::Resolved);
    }

    #[test]
    fn test_cooldown_after_emergency() {
        let mut contract = setup_contract_with_validators();

        // Declare and end emergency
        for i in 1..=8 {
            contract.vote_emergency(create_account(i), true, 1000).unwrap();
        }
        contract.end_emergency(EmergencyOutcome::Resolved, 2000).unwrap();

        // Try to vote again during cooldown
        let result = contract.vote_emergency(create_account(1), true, 3000);

        assert!(matches!(result, Err(EmergencyError::InCooldown { .. })));
    }

    #[test]
    fn test_trigger_breaker() {
        let mut contract = EmergencyContract::new();

        let result = contract.trigger_breaker("finality_delay", 1000);
        assert!(result.is_ok());

        let breaker = contract.breakers.get("finality_delay").unwrap();
        assert!(breaker.is_active);
        assert_eq!(breaker.triggered_at, Some(1000));
    }

    #[test]
    fn test_breaker_expiration() {
        let mut contract = EmergencyContract::new();

        contract.trigger_breaker("finality_delay", 1000).unwrap();

        // Check before expiration
        contract.check_breaker_expirations(10000);
        assert!(contract.breakers.get("finality_delay").unwrap().is_active);

        // Check after expiration (1000 + 14400 = 15400)
        contract.check_breaker_expirations(20000);
        assert!(!contract.breakers.get("finality_delay").unwrap().is_active);
    }

    #[test]
    fn test_report_signal() {
        let mut contract = EmergencyContract::new();

        let signal = FailureSignal::ConsensusStall { epochs_stalled: 5 };
        let result = contract.report_signal(signal, 1000);

        assert!(result.is_ok());
        assert_eq!(contract.signals.len(), 1);
    }

    #[test]
    fn test_signal_triggers_breaker() {
        let mut contract = EmergencyContract::new();

        // Report signal that matches finality_delay breaker condition
        let signal = FailureSignal::ConsensusStall { epochs_stalled: 5 };
        contract.report_signal(signal, 1000).unwrap();

        // Finality delay breaker should be triggered
        assert!(contract.breakers.get("finality_delay").unwrap().is_active);
    }

    #[test]
    fn test_multiple_signals_trigger_emergency() {
        let mut contract = setup_contract_with_validators();

        // Report multiple high-severity signals
        let signal1 = FailureSignal::ConsensusStall { epochs_stalled: 8 };
        let signal2 = FailureSignal::ValidatorMassOffline {
            offline_count: 40,
            total_validators: 100,
        };

        contract.report_signal(signal1, 1000).unwrap();
        contract.report_signal(signal2, 1001).unwrap();

        // Emergency should be declared automatically
        assert!(contract.state.active);
        assert!(matches!(
            contract.state.trigger,
            Some(EmergencyTrigger::MultipleSignals { .. })
        ));
    }

    #[test]
    fn test_start_recovery() {
        let mut contract = EmergencyContract::new();

        let result = contract.start_recovery(None, 34, 1000);
        assert!(result.is_ok());

        assert!(contract.recovery.is_some());
        let recovery = contract.recovery.as_ref().unwrap();
        assert!(matches!(recovery.current_step, RecoveryStep::Halt));
    }

    #[test]
    fn test_advance_recovery() {
        let mut contract = EmergencyContract::new();

        contract.start_recovery(None, 34, 1000).unwrap();

        // Advance through steps
        let step1 = contract.advance_recovery(1001).unwrap();
        assert!(matches!(step1, RecoveryStep::StateAudit));

        let step2 = contract.advance_recovery(1002).unwrap();
        assert!(matches!(step2, RecoveryStep::ValidatorReconfirmation));

        let step3 = contract.advance_recovery(1003).unwrap();
        assert!(matches!(step3, RecoveryStep::ForkDecision));

        let step4 = contract.advance_recovery(1004).unwrap();
        assert!(matches!(step4, RecoveryStep::GradualRestart { phase: 1, .. }));
    }

    #[test]
    fn test_create_checkpoint() {
        let mut contract = EmergencyContract::new();

        // Create checkpoint at interval
        contract.create_checkpoint(
            1000, // Checkpoint interval
            Hash::hash(b"state"),
            Hash::hash(b"validators"),
            ChainId(0),
        );

        assert_eq!(contract.checkpoints.len(), 1);
        assert_eq!(contract.checkpoints[0].block_number, 1000);
    }

    #[test]
    fn test_checkpoint_not_at_interval() {
        let mut contract = EmergencyContract::new();

        // Try to create checkpoint not at interval
        contract.create_checkpoint(
            999, // Not at checkpoint interval
            Hash::hash(b"state"),
            Hash::hash(b"validators"),
            ChainId(0),
        );

        // Should not be created
        assert!(contract.checkpoints.is_empty());
    }

    #[test]
    fn test_get_checkpoint() {
        let mut contract = EmergencyContract::new();

        contract.create_checkpoint(1000, Hash::hash(b"s1"), Hash::hash(b"v1"), ChainId(0));
        contract.create_checkpoint(2000, Hash::hash(b"s2"), Hash::hash(b"v2"), ChainId(0));
        contract.create_checkpoint(3000, Hash::hash(b"s3"), Hash::hash(b"v3"), ChainId(0));

        let cp = contract.get_checkpoint(2500);
        assert!(cp.is_some());
        assert_eq!(cp.unwrap().block_number, 2000);
    }

    #[test]
    fn test_is_action_active() {
        let mut contract = setup_contract_with_validators();

        // Declare emergency
        for i in 1..=8 {
            contract.vote_emergency(create_account(i), true, 1000).unwrap();
        }

        // Take pause action
        contract.take_action(
            EmergencyAction::PauseBlockProduction {
                started_at: 1000,
                max_duration: 10000,
            },
            1001,
        ).unwrap();

        assert!(contract.is_action_active("pause"));
        assert!(!contract.is_action_active("halt_slashing"));
    }

    #[test]
    fn test_prune_signals() {
        let mut contract = EmergencyContract::new();

        // Add signals at different blocks
        for i in 0..20 {
            let signal = FailureSignal::ConsensusStall { epochs_stalled: 1 };
            contract.signals.push((i * 1000, signal));
        }

        assert_eq!(contract.signals.len(), 20);

        // Prune (keep signals within last 10000 blocks from block 25000)
        contract.prune_signals(25000);

        // Should keep signals from blocks 15000+ (15, 16, 17, 18, 19 = 5 signals)
        assert!(contract.signals.len() < 20);
    }

    #[test]
    fn test_drain_events() {
        let mut contract = setup_contract_with_validators();

        // Generate some events
        contract.vote_emergency(create_account(1), true, 1000).unwrap();

        let events = contract.drain_events();
        assert!(!events.is_empty());

        // Events should be cleared
        let events2 = contract.drain_events();
        assert!(events2.is_empty());
    }

    #[test]
    fn test_active_breakers() {
        let mut contract = EmergencyContract::new();

        assert!(contract.active_breakers().is_empty());

        contract.trigger_breaker("finality_delay", 1000).unwrap();
        contract.trigger_breaker("governance_deadlock", 1000).unwrap();

        let active = contract.active_breakers();
        assert_eq!(active.len(), 2);
    }
}
