// Finality Rounds - Manages the round lifecycle
//
// Each finality round progresses through phases:
// 1. Prevoting - validators broadcast their preferred block
// 2. Precommitting - after 2/3 prevotes, validators precommit
// 3. Finalization - after 2/3 precommits, block is finalized
//
// If a round times out, a new round starts with round+1

use super::types::{FinalityVote, RoundState, RoundSummary, VoteType};
use super::votes::{VoteCollector, VoteError};
use super::config;
use crate::types::account::AccountId;
use crate::types::block::{FinalityJustification, ValidatorSignature};
use crate::types::primitives::{BlockNumber, EpochNumber, Hash, Timestamp};
use crate::types::signature::Signature64;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Manages a single finality round
#[derive(Debug)]
pub struct FinalityRound {
    /// The vote collector for this round
    collector: VoteCollector,

    /// When this round started
    started_at: Instant,

    /// Round timeout duration
    timeout: Duration,

    /// Our validator ID (if we're a validator)
    our_validator: Option<AccountId>,

    /// Have we prevoted this round?
    have_prevoted: bool,

    /// Have we precommitted this round?
    have_precommitted: bool,
}

impl FinalityRound {
    /// Create a new finality round
    pub fn new(
        epoch: EpochNumber,
        round: u32,
        validators: HashSet<AccountId>,
        our_validator: Option<AccountId>,
    ) -> Self {
        Self {
            collector: VoteCollector::new(epoch, round, validators),
            started_at: Instant::now(),
            timeout: Duration::from_millis(config::ROUND_TIMEOUT_MS),
            our_validator,
            have_prevoted: false,
            have_precommitted: false,
        }
    }

    /// Get the epoch number
    pub fn epoch(&self) -> EpochNumber {
        self.collector.epoch()
    }

    /// Get the round number
    pub fn round(&self) -> u32 {
        self.collector.round()
    }

    /// Get the current state
    pub fn state(&self) -> RoundState {
        self.collector.state()
    }

    /// Check if round has timed out
    pub fn is_timed_out(&self) -> bool {
        self.started_at.elapsed() > self.timeout
    }

    /// Check if round is complete (finalized or failed)
    pub fn is_done(&self) -> bool {
        self.collector.is_done() || self.is_timed_out()
    }

    /// Get time remaining in this round
    pub fn time_remaining(&self) -> Duration {
        self.timeout.saturating_sub(self.started_at.elapsed())
    }

    /// Add a vote to this round
    pub fn add_vote(&mut self, vote: FinalityVote) -> Result<bool, VoteError> {
        self.collector.add_vote(vote)
    }

    /// Create our prevote for a target block
    ///
    /// Returns None if we're not a validator or already prevoted
    pub fn create_prevote(
        &mut self,
        target_number: BlockNumber,
        target_hash: Hash,
        sign_fn: impl FnOnce(&[u8]) -> Signature64,
    ) -> Option<FinalityVote> {
        let our_validator = self.our_validator?;

        if self.have_prevoted {
            debug!("Already prevoted in round {}", self.round());
            return None;
        }

        if self.state() != RoundState::Prevoting {
            debug!("Not in prevoting phase, state: {:?}", self.state());
            return None;
        }

        let mut vote = FinalityVote::new(
            VoteType::Prevote,
            target_number,
            target_hash,
            self.round(),
            self.epoch(),
            our_validator,
        );

        // Sign the vote
        let message = vote.signing_message();
        vote.signature = sign_fn(&message);

        self.have_prevoted = true;

        info!(
            "📋 Created prevote for block #{} in round {}",
            target_number,
            self.round()
        );

        Some(vote)
    }

    /// Create our precommit for the best prevote target
    ///
    /// Returns None if we're not a validator, already precommitted, or not in precommit phase
    pub fn create_precommit(
        &mut self,
        sign_fn: impl FnOnce(&[u8]) -> Signature64,
    ) -> Option<FinalityVote> {
        let our_validator = self.our_validator?;

        if self.have_precommitted {
            debug!("Already precommitted in round {}", self.round());
            return None;
        }

        if self.state() != RoundState::Precommitting {
            debug!("Not in precommitting phase, state: {:?}", self.state());
            return None;
        }

        // Precommit for the best prevote target
        let (target_number, target_hash) = self.collector.best_prevote()?;

        let mut vote = FinalityVote::new(
            VoteType::Precommit,
            target_number,
            target_hash,
            self.round(),
            self.epoch(),
            our_validator,
        );

        let message = vote.signing_message();
        vote.signature = sign_fn(&message);

        self.have_precommitted = true;

        info!(
            "🔏 Created precommit for block #{} in round {}",
            target_number,
            self.round()
        );

        Some(vote)
    }

    /// Get the finalized target if round completed successfully
    pub fn finalized_target(&self) -> Option<(BlockNumber, Hash)> {
        self.collector.finalized_target()
    }

    /// Create a finality justification from the completed round
    ///
    /// Returns None if round is not completed
    pub fn create_justification(&self) -> Option<FinalityJustification> {
        if self.state() != RoundState::Completed {
            return None;
        }

        let (block_number, block_hash) = self.finalized_target()?;

        // Collect signatures from precommits
        let signatures: Vec<ValidatorSignature> = self
            .collector
            .get_precommits_for_best()
            .into_iter()
            .map(|vote| ValidatorSignature {
                validator: vote.voter,
                signature: vote.signature,
            })
            .collect();

        Some(FinalityJustification {
            block_number,
            block_hash,
            signatures,
            epoch: self.epoch(),
        })
    }

    /// Get a summary of the round state
    pub fn summary(&self) -> RoundSummary {
        RoundSummary {
            round: self.round(),
            epoch: self.epoch(),
            state: self.state(),
            prevote_count: self.collector.total_prevotes(),
            precommit_count: self.collector.total_precommits(),
            target_block: self.collector.best_prevote(),
            total_validators: self.collector.validator_count(),
        }
    }

    /// Get the vote collector (for read access)
    pub fn collector(&self) -> &VoteCollector {
        &self.collector
    }

    /// Mark round as failed (timeout)
    pub fn mark_failed(&mut self) {
        self.collector.mark_failed();
    }

    /// Get all votes for gossip/catch-up
    pub fn all_votes(&self) -> Vec<FinalityVote> {
        self.collector.all_votes()
    }

    /// Check if we should create a precommit (state just changed to precommitting)
    pub fn should_precommit(&self) -> bool {
        self.state() == RoundState::Precommitting
            && !self.have_precommitted
            && self.our_validator.is_some()
    }

    /// Get best prevote target (for precommit decisions)
    pub fn best_prevote_target(&self) -> Option<(BlockNumber, Hash)> {
        self.collector.best_prevote()
    }
}

/// Manages multiple rounds across epochs
#[derive(Debug)]
pub struct RoundManager {
    /// Current epoch
    current_epoch: EpochNumber,

    /// Current round within epoch
    current_round: u32,

    /// Current active round
    active_round: Option<FinalityRound>,

    /// Historical rounds (for catch-up)
    completed_rounds: Vec<RoundSummary>,

    /// Last finalized block
    last_finalized: (BlockNumber, Hash),

    /// Our validator ID
    our_validator: Option<AccountId>,

    /// Current validator set
    validators: HashSet<AccountId>,
}

impl RoundManager {
    /// Create a new round manager
    pub fn new(
        our_validator: Option<AccountId>,
        validators: HashSet<AccountId>,
        last_finalized: (BlockNumber, Hash),
    ) -> Self {
        Self {
            current_epoch: 0,
            current_round: 0,
            active_round: None,
            completed_rounds: Vec::new(),
            last_finalized,
            our_validator,
            validators,
        }
    }

    /// Start a new round
    pub fn start_round(&mut self, epoch: EpochNumber, round: u32) -> &mut FinalityRound {
        info!("🔄 Starting finality round {} in epoch {}", round, epoch);

        self.current_epoch = epoch;
        self.current_round = round;

        self.active_round = Some(FinalityRound::new(
            epoch,
            round,
            self.validators.clone(),
            self.our_validator,
        ));

        self.active_round.as_mut().unwrap()
    }

    /// Get the active round
    pub fn active_round(&self) -> Option<&FinalityRound> {
        self.active_round.as_ref()
    }

    /// Get mutable access to active round
    pub fn active_round_mut(&mut self) -> Option<&mut FinalityRound> {
        self.active_round.as_mut()
    }

    /// Complete the current round and optionally start a new one
    pub fn complete_round(&mut self, finalized: Option<(BlockNumber, Hash)>) {
        if let Some(round) = self.active_round.take() {
            // Store summary
            self.completed_rounds.push(round.summary());

            // Keep only recent rounds
            if self.completed_rounds.len() > 100 {
                self.completed_rounds.remove(0);
            }

            // Update finalized if we got a result
            if let Some((block, hash)) = finalized {
                self.last_finalized = (block, hash);
                info!("🔒 Block #{} finalized in round {}", block, round.round());
            }
        }
    }

    /// Advance to next round (after timeout or completion)
    pub fn next_round(&mut self) -> &mut FinalityRound {
        self.current_round += 1;
        self.start_round(self.current_epoch, self.current_round)
    }

    /// Set new epoch (resets round to 0)
    pub fn new_epoch(&mut self, epoch: EpochNumber) {
        self.current_epoch = epoch;
        self.current_round = 0;
    }

    /// Update validator set and our validator status
    pub fn update_validators(&mut self, validators: HashSet<AccountId>, our_validator_id: AccountId) {
        // Check if we're now in the validator set
        if validators.contains(&our_validator_id) {
            self.our_validator = Some(our_validator_id);
        } else {
            self.our_validator = None;
        }
        self.validators = validators;
    }

    /// Get last finalized block
    pub fn last_finalized(&self) -> (BlockNumber, Hash) {
        self.last_finalized
    }

    /// Get current round number
    pub fn current_round(&self) -> u32 {
        self.current_round
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> EpochNumber {
        self.current_epoch
    }

    /// Check if there's an active round
    pub fn has_active_round(&self) -> bool {
        self.active_round.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_validators(count: usize) -> HashSet<AccountId> {
        (0..count)
            .map(|i| {
                let mut bytes = [0u8; 32];
                bytes[0] = i as u8;
                AccountId::from_bytes(bytes)
            })
            .collect()
    }

    #[test]
    fn test_new_round() {
        let validators = make_validators(3);
        let round = FinalityRound::new(0, 1, validators.clone(), None);

        assert_eq!(round.epoch(), 0);
        assert_eq!(round.round(), 1);
        assert_eq!(round.state(), RoundState::Prevoting);
        assert!(!round.is_done());
    }

    #[test]
    fn test_round_manager() {
        let validators = make_validators(3);
        let mut manager = RoundManager::new(None, validators, (0, Hash::ZERO));

        assert_eq!(manager.current_round(), 0);
        assert!(!manager.has_active_round());

        manager.start_round(0, 1);
        assert!(manager.has_active_round());
        assert_eq!(manager.active_round().unwrap().round(), 1);
    }

    #[test]
    fn test_round_advancement() {
        let validators = make_validators(3);
        let mut manager = RoundManager::new(None, validators, (0, Hash::ZERO));

        manager.start_round(0, 0);
        assert_eq!(manager.current_round(), 0);

        manager.complete_round(None);
        manager.next_round();
        assert_eq!(manager.current_round(), 1);
    }

    #[test]
    fn test_epoch_change() {
        let validators = make_validators(3);
        let mut manager = RoundManager::new(None, validators, (0, Hash::ZERO));

        manager.start_round(0, 5);
        assert_eq!(manager.current_epoch(), 0);
        assert_eq!(manager.current_round(), 5);

        manager.new_epoch(1);
        assert_eq!(manager.current_epoch(), 1);
        assert_eq!(manager.current_round(), 0);
    }

    #[test]
    fn test_round_summary() {
        let validators = make_validators(3);
        let round = FinalityRound::new(0, 1, validators, None);
        let summary = round.summary();

        assert_eq!(summary.epoch, 0);
        assert_eq!(summary.round, 1);
        assert_eq!(summary.state, RoundState::Prevoting);
        assert_eq!(summary.total_validators, 3);
    }
}
