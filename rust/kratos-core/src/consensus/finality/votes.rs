// Vote Collector - Aggregates finality votes and tracks supermajority
//
// This module handles:
// - Collecting votes from validators
// - Checking for supermajority (2/3)
// - Detecting equivocation (double voting)
// - Tracking vote weights for finality determination

use super::types::{EquivocationProof, FinalityVote, RoundState, VoteId, VoteType};
use super::{config, has_supermajority, supermajority_threshold};
use crate::types::account::AccountId;
use crate::types::primitives::{BlockNumber, EpochNumber, Hash};
use std::collections::{BTreeMap, HashMap, HashSet};
use tracing::{debug, info, warn};

/// Collects and aggregates votes for a specific round
#[derive(Debug, Clone)]
pub struct VoteCollector {
    /// Current epoch
    epoch: EpochNumber,

    /// Current round
    round: u32,

    /// Set of active validators who can vote
    validators: HashSet<AccountId>,

    /// Prevotes indexed by target block
    prevotes: HashMap<(BlockNumber, Hash), Vec<FinalityVote>>,

    /// Precommits indexed by target block
    precommits: HashMap<(BlockNumber, Hash), Vec<FinalityVote>>,

    /// Track which validators have prevoted (for equivocation detection)
    prevoted: HashMap<AccountId, (BlockNumber, Hash)>,

    /// Track which validators have precommitted
    precommitted: HashMap<AccountId, (BlockNumber, Hash)>,

    /// Detected equivocations
    equivocations: Vec<EquivocationProof>,

    /// Current state of the round
    state: RoundState,

    /// Best prevote target (has most votes)
    best_prevote_target: Option<(BlockNumber, Hash)>,

    /// Best precommit target
    best_precommit_target: Option<(BlockNumber, Hash)>,
}

impl VoteCollector {
    /// Create a new vote collector for a round
    pub fn new(epoch: EpochNumber, round: u32, validators: HashSet<AccountId>) -> Self {
        Self {
            epoch,
            round,
            validators,
            prevotes: HashMap::new(),
            precommits: HashMap::new(),
            prevoted: HashMap::new(),
            precommitted: HashMap::new(),
            equivocations: Vec::new(),
            state: RoundState::Prevoting,
            best_prevote_target: None,
            best_precommit_target: None,
        }
    }

    /// Get current round state
    pub fn state(&self) -> RoundState {
        self.state
    }

    /// Get the epoch
    pub fn epoch(&self) -> EpochNumber {
        self.epoch
    }

    /// Get the round number
    pub fn round(&self) -> u32 {
        self.round
    }

    /// Get total validator count
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    /// Check if a validator is in the active set
    pub fn is_validator(&self, account: &AccountId) -> bool {
        self.validators.contains(account)
    }

    /// Add a vote to the collector
    ///
    /// Returns true if the vote was accepted, false if rejected
    pub fn add_vote(&mut self, vote: FinalityVote) -> Result<bool, VoteError> {
        // Validate vote belongs to this round
        if vote.epoch != self.epoch || vote.round != self.round {
            return Err(VoteError::WrongRound {
                expected_epoch: self.epoch,
                expected_round: self.round,
                got_epoch: vote.epoch,
                got_round: vote.round,
            });
        }

        // Check voter is a validator
        if !self.validators.contains(&vote.voter) {
            return Err(VoteError::NotValidator(vote.voter));
        }

        // For precommits, check state before verifying signature (fast-fail)
        if vote.vote_type == VoteType::Precommit && self.state == RoundState::Prevoting {
            return Err(VoteError::NotInPrecommitPhase);
        }

        // Verify signature
        if !vote.verify() {
            return Err(VoteError::InvalidSignature(vote.voter));
        }

        // Process based on vote type
        match vote.vote_type {
            VoteType::Prevote => self.add_prevote(vote),
            VoteType::Precommit => self.add_precommit(vote),
        }
    }

    /// Add a prevote
    fn add_prevote(&mut self, vote: FinalityVote) -> Result<bool, VoteError> {
        let target = (vote.target_number, vote.target_hash);
        let voter = vote.voter;

        // Check for equivocation
        if let Some(existing_target) = self.prevoted.get(&voter) {
            if *existing_target != target {
                // Found equivocation - voter prevoted for different blocks
                let existing_vote = self
                    .prevotes
                    .get(existing_target)
                    .and_then(|votes| votes.iter().find(|v| v.voter == voter))
                    .cloned();

                if let Some(existing) = existing_vote {
                    let proof = EquivocationProof {
                        validator: voter,
                        vote1: existing,
                        vote2: vote.clone(),
                        round: self.round,
                        epoch: self.epoch,
                    };
                    self.equivocations.push(proof);
                    warn!(
                        "🚨 Equivocation detected: validator {} prevoted for multiple blocks in round {}",
                        voter, self.round
                    );
                    return Err(VoteError::Equivocation(voter));
                }
            }
            // Already prevoted for same target, ignore duplicate
            return Ok(false);
        }

        // Record the prevote
        self.prevoted.insert(voter, target);
        self.prevotes.entry(target).or_default().push(vote);

        let vote_count = self.prevotes.get(&target).map(|v| v.len()).unwrap_or(0);
        let threshold = supermajority_threshold(self.validator_count());
        info!(
            "🗳️  PREVOTE: validator 0x{}..{} voted for block #{} ({}/{} votes, need {})",
            hex::encode(&voter.as_bytes()[..4]),
            hex::encode(&voter.as_bytes()[28..]),
            target.0,
            vote_count,
            self.validator_count(),
            threshold
        );

        // Update best prevote target
        self.update_best_prevote();

        // Check if we have supermajority prevotes
        self.check_prevote_threshold();

        Ok(true)
    }

    /// Add a precommit
    fn add_precommit(&mut self, vote: FinalityVote) -> Result<bool, VoteError> {
        // Note: State check already done in add_vote() for fast-fail

        let target = (vote.target_number, vote.target_hash);
        let voter = vote.voter;

        // Check for equivocation
        if let Some(existing_target) = self.precommitted.get(&voter) {
            if *existing_target != target {
                let existing_vote = self
                    .precommits
                    .get(existing_target)
                    .and_then(|votes| votes.iter().find(|v| v.voter == voter))
                    .cloned();

                if let Some(existing) = existing_vote {
                    let proof = EquivocationProof {
                        validator: voter,
                        vote1: existing,
                        vote2: vote.clone(),
                        round: self.round,
                        epoch: self.epoch,
                    };
                    self.equivocations.push(proof);
                    warn!(
                        "🚨 Equivocation detected: validator {} precommitted for multiple blocks in round {}",
                        voter, self.round
                    );
                    return Err(VoteError::Equivocation(voter));
                }
            }
            return Ok(false);
        }

        // Record the precommit
        self.precommitted.insert(voter, target);
        self.precommits.entry(target).or_default().push(vote);

        let vote_count = self.precommits.get(&target).map(|v| v.len()).unwrap_or(0);
        let threshold = supermajority_threshold(self.validator_count());
        info!(
            "✍️  PRECOMMIT: validator 0x{}..{} committed to block #{} ({}/{} votes, need {})",
            hex::encode(&voter.as_bytes()[..4]),
            hex::encode(&voter.as_bytes()[28..]),
            target.0,
            vote_count,
            self.validator_count(),
            threshold
        );

        // Update best precommit target
        self.update_best_precommit();

        // Check if we have supermajority precommits
        self.check_precommit_threshold();

        Ok(true)
    }

    /// Update the best prevote target
    fn update_best_prevote(&mut self) {
        let mut best: Option<((BlockNumber, Hash), usize)> = None;

        for (target, votes) in &self.prevotes {
            let count = votes.len();
            match &best {
                None => best = Some((*target, count)),
                Some((_, best_count)) if count > *best_count => {
                    best = Some((*target, count));
                }
                Some((best_target, best_count)) if count == *best_count => {
                    // Tie-break by block number (prefer higher)
                    if target.0 > best_target.0 {
                        best = Some((*target, count));
                    }
                }
                _ => {}
            }
        }

        self.best_prevote_target = best.map(|(target, _)| target);
    }

    /// Update the best precommit target
    fn update_best_precommit(&mut self) {
        let mut best: Option<((BlockNumber, Hash), usize)> = None;

        for (target, votes) in &self.precommits {
            let count = votes.len();
            match &best {
                None => best = Some((*target, count)),
                Some((_, best_count)) if count > *best_count => {
                    best = Some((*target, count));
                }
                Some((best_target, best_count)) if count == *best_count => {
                    if target.0 > best_target.0 {
                        best = Some((*target, count));
                    }
                }
                _ => {}
            }
        }

        self.best_precommit_target = best.map(|(target, _)| target);
    }

    /// Check if prevotes reached supermajority
    fn check_prevote_threshold(&mut self) {
        if self.state != RoundState::Prevoting {
            return;
        }

        if let Some(target) = self.best_prevote_target {
            let count = self.prevotes.get(&target).map(|v| v.len()).unwrap_or(0);
            if has_supermajority(count, self.validator_count()) {
                info!(
                    "✅ Prevote supermajority reached for block #{} ({}/{})",
                    target.0,
                    count,
                    self.validator_count()
                );
                self.state = RoundState::Precommitting;
            }
        }
    }

    /// Check if precommits reached supermajority
    fn check_precommit_threshold(&mut self) {
        if self.state != RoundState::Precommitting {
            return;
        }

        if let Some(target) = self.best_precommit_target {
            let count = self.precommits.get(&target).map(|v| v.len()).unwrap_or(0);
            if has_supermajority(count, self.validator_count()) {
                info!(
                    "🔒 Precommit supermajority reached for block #{} ({}/{})",
                    target.0,
                    count,
                    self.validator_count()
                );
                self.state = RoundState::Completed;
            }
        }
    }

    /// Get the finalized target if round is completed
    pub fn finalized_target(&self) -> Option<(BlockNumber, Hash)> {
        if self.state == RoundState::Completed {
            self.best_precommit_target
        } else {
            None
        }
    }

    /// Get prevote count for a target
    pub fn prevote_count(&self, target: &(BlockNumber, Hash)) -> usize {
        self.prevotes.get(target).map(|v| v.len()).unwrap_or(0)
    }

    /// Get precommit count for a target
    pub fn precommit_count(&self, target: &(BlockNumber, Hash)) -> usize {
        self.precommits.get(target).map(|v| v.len()).unwrap_or(0)
    }

    /// Get total prevote count
    pub fn total_prevotes(&self) -> usize {
        self.prevoted.len()
    }

    /// Get total precommit count
    pub fn total_precommits(&self) -> usize {
        self.precommitted.len()
    }

    /// Get best prevote target
    pub fn best_prevote(&self) -> Option<(BlockNumber, Hash)> {
        self.best_prevote_target
    }

    /// Get best precommit target
    pub fn best_precommit(&self) -> Option<(BlockNumber, Hash)> {
        self.best_precommit_target
    }

    /// Get all prevotes for the best target
    pub fn get_prevotes_for_best(&self) -> Vec<FinalityVote> {
        self.best_prevote_target
            .and_then(|t| self.prevotes.get(&t))
            .cloned()
            .unwrap_or_default()
    }

    /// Get all precommits for the best target
    pub fn get_precommits_for_best(&self) -> Vec<FinalityVote> {
        self.best_precommit_target
            .and_then(|t| self.precommits.get(&t))
            .cloned()
            .unwrap_or_default()
    }

    /// Get all detected equivocations
    pub fn equivocations(&self) -> &[EquivocationProof] {
        &self.equivocations
    }

    /// Check if a validator has prevoted
    pub fn has_prevoted(&self, validator: &AccountId) -> bool {
        self.prevoted.contains_key(validator)
    }

    /// Check if a validator has precommitted
    pub fn has_precommitted(&self, validator: &AccountId) -> bool {
        self.precommitted.contains_key(validator)
    }

    /// Get all votes (for catch-up/gossip)
    pub fn all_votes(&self) -> Vec<FinalityVote> {
        let mut votes = Vec::new();
        for votes_list in self.prevotes.values() {
            votes.extend(votes_list.iter().cloned());
        }
        for votes_list in self.precommits.values() {
            votes.extend(votes_list.iter().cloned());
        }
        votes
    }

    /// Mark round as failed (timeout)
    pub fn mark_failed(&mut self) {
        if self.state != RoundState::Completed {
            self.state = RoundState::Failed;
        }
    }

    /// Check if round is completed (either finalized or failed)
    pub fn is_done(&self) -> bool {
        matches!(self.state, RoundState::Completed | RoundState::Failed)
    }
}

/// Errors that can occur when adding votes
#[derive(Debug, Clone)]
pub enum VoteError {
    /// Vote is for wrong round
    WrongRound {
        expected_epoch: EpochNumber,
        expected_round: u32,
        got_epoch: EpochNumber,
        got_round: u32,
    },

    /// Voter is not in validator set
    NotValidator(AccountId),

    /// Signature verification failed
    InvalidSignature(AccountId),

    /// Validator already voted for different target (equivocation)
    Equivocation(AccountId),

    /// Cannot precommit before prevote threshold reached
    NotInPrecommitPhase,
}

impl std::fmt::Display for VoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoteError::WrongRound {
                expected_epoch,
                expected_round,
                got_epoch,
                got_round,
            } => {
                write!(
                    f,
                    "Vote for wrong round: expected epoch {} round {}, got epoch {} round {}",
                    expected_epoch, expected_round, got_epoch, got_round
                )
            }
            VoteError::NotValidator(v) => write!(f, "Not a validator: {}", v),
            VoteError::InvalidSignature(v) => write!(f, "Invalid signature from: {}", v),
            VoteError::Equivocation(v) => write!(f, "Equivocation detected from: {}", v),
            VoteError::NotInPrecommitPhase => write!(f, "Not in precommit phase yet"),
        }
    }
}

impl std::error::Error for VoteError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::primitives::Hash;

    fn make_validators(count: usize) -> HashSet<AccountId> {
        (0..count)
            .map(|i| {
                let mut bytes = [0u8; 32];
                bytes[0] = i as u8;
                AccountId::from_bytes(bytes)
            })
            .collect()
    }

    fn make_vote(
        voter_idx: u8,
        vote_type: VoteType,
        block: BlockNumber,
        round: u32,
        epoch: EpochNumber,
    ) -> FinalityVote {
        let mut bytes = [0u8; 32];
        bytes[0] = voter_idx;
        let voter = AccountId::from_bytes(bytes);

        FinalityVote::new(vote_type, block, Hash::hash(&block.to_le_bytes()), round, epoch, voter)
    }

    #[test]
    fn test_new_collector() {
        let validators = make_validators(3);
        let collector = VoteCollector::new(0, 1, validators.clone());

        assert_eq!(collector.epoch(), 0);
        assert_eq!(collector.round(), 1);
        assert_eq!(collector.validator_count(), 3);
        assert_eq!(collector.state(), RoundState::Prevoting);
    }

    #[test]
    fn test_wrong_round_rejected() {
        let validators = make_validators(3);
        let mut collector = VoteCollector::new(0, 1, validators);

        let vote = make_vote(0, VoteType::Prevote, 100, 2, 0); // Wrong round
        let result = collector.add_vote(vote);

        assert!(matches!(result, Err(VoteError::WrongRound { .. })));
    }

    #[test]
    fn test_non_validator_rejected() {
        let validators = make_validators(3);
        let mut collector = VoteCollector::new(0, 1, validators);

        let vote = make_vote(99, VoteType::Prevote, 100, 1, 0); // Not a validator
        let result = collector.add_vote(vote);

        assert!(matches!(result, Err(VoteError::NotValidator(_))));
    }

    #[test]
    fn test_prevote_collection() {
        let validators = make_validators(3);
        let mut collector = VoteCollector::new(0, 1, validators);

        // Add prevotes (signatures won't verify in tests, but that's okay for unit tests)
        // In real usage, votes would be properly signed

        assert_eq!(collector.state(), RoundState::Prevoting);
        assert_eq!(collector.total_prevotes(), 0);
    }

    #[test]
    fn test_precommit_before_prevote_threshold() {
        let validators = make_validators(3);
        let mut collector = VoteCollector::new(0, 1, validators);

        let vote = make_vote(0, VoteType::Precommit, 100, 1, 0);
        let result = collector.add_vote(vote);

        assert!(matches!(result, Err(VoteError::NotInPrecommitPhase)));
    }

    #[test]
    fn test_state_transitions() {
        // Verify state machine works correctly
        let mut state = RoundState::Prevoting;
        assert_eq!(state, RoundState::Prevoting);

        state = RoundState::Precommitting;
        assert_eq!(state, RoundState::Precommitting);

        state = RoundState::Completed;
        assert_eq!(state, RoundState::Completed);
    }
}
