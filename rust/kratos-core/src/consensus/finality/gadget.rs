// Finality Gadget - Main coordinator for GRANDPA-style finality
//
// The FinalityGadget is responsible for:
// 1. Coordinating finality rounds
// 2. Processing incoming votes
// 3. Generating and broadcasting our votes
// 4. Creating finality justifications
// 5. Detecting and reporting equivocations

use super::rounds::{FinalityRound, RoundManager};
use super::types::{EquivocationProof, FinalityMessage, FinalityVote, RoundState, VoteType};
use super::votes::VoteError;
use super::config;
use crate::types::account::AccountId;
use crate::types::block::FinalityJustification;
use crate::types::primitives::{BlockNumber, EpochNumber, Hash};
use crate::types::signature::Signature64;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn, trace};

/// Trait for signing messages (implemented by key holder)
pub trait FinalitySigner: Send + Sync {
    /// Sign a message with the validator key
    fn sign(&self, message: &[u8]) -> Signature64;

    /// Get our validator ID
    fn validator_id(&self) -> AccountId;
}

/// Trait for broadcasting finality messages
pub trait FinalityBroadcaster: Send + Sync {
    /// Broadcast a finality message to peers
    fn broadcast(&self, message: FinalityMessage);
}

/// Result of processing a finality vote
#[derive(Debug)]
pub enum VoteResult {
    /// Vote accepted
    Accepted,
    /// Vote triggered state change (prevote -> precommit)
    StateChanged(RoundState),
    /// Vote completed finalization
    Finalized(BlockNumber, Hash),
    /// Vote rejected
    Rejected(VoteError),
}

/// The main finality gadget
pub struct FinalityGadget<S: FinalitySigner, B: FinalityBroadcaster> {
    /// Round manager
    rounds: RoundManager,

    /// Signer for creating votes
    signer: Arc<S>,

    /// Broadcaster for sending messages
    broadcaster: Arc<B>,

    /// Pending outbound messages
    outbound_queue: VecDeque<FinalityMessage>,

    /// Finality justifications we've created
    justifications: Vec<FinalityJustification>,

    /// Equivocations detected
    equivocations: Vec<EquivocationProof>,

    /// Whether we're an active validator
    is_validator: bool,

    /// Last block we've seen
    last_block: (BlockNumber, Hash),

    /// Blocks pending finality (not yet targeted by a round)
    pending_blocks: Vec<(BlockNumber, Hash)>,
}

impl<S: FinalitySigner, B: FinalityBroadcaster> FinalityGadget<S, B> {
    /// Create a new finality gadget
    pub fn new(
        signer: Arc<S>,
        broadcaster: Arc<B>,
        validators: HashSet<AccountId>,
        genesis_hash: Hash,
    ) -> Self {
        let validator_id = signer.validator_id();
        let is_validator = validators.contains(&validator_id);

        trace!(
            "[GRANDPA] FinalityGadget::new - validator_id=0x{}..{}, is_validator={}, validator_count={}",
            hex::encode(&validator_id.as_bytes()[..4]),
            hex::encode(&validator_id.as_bytes()[28..]),
            is_validator,
            validators.len()
        );

        info!(
            "🔧 Finality gadget initialized. Validator: {}, Active: {}",
            validator_id, is_validator
        );

        Self {
            rounds: RoundManager::new(
                if is_validator { Some(validator_id) } else { None },
                validators,
                (0, genesis_hash),
            ),
            signer,
            broadcaster,
            outbound_queue: VecDeque::new(),
            justifications: Vec::new(),
            equivocations: Vec::new(),
            is_validator,
            last_block: (0, genesis_hash),
            pending_blocks: Vec::new(),
        }
    }

    /// Notify the gadget of a new imported block
    pub fn on_block_imported(&mut self, number: BlockNumber, hash: Hash) {
        trace!(
            "[GRANDPA] on_block_imported: block #{}, hash={}, is_validator={}, has_active_round={}, pending_blocks={}",
            number, hash, self.is_validator, self.rounds.has_active_round(), self.pending_blocks.len()
        );

        self.last_block = (number, hash);
        self.pending_blocks.push((number, hash));

        // Start a finality round if we don't have one active
        if !self.rounds.has_active_round() && self.pending_blocks.len() >= 1 {
            trace!("[GRANDPA] on_block_imported: no active round, starting new round");
            self.start_finality_round();
        } else {
            trace!("[GRANDPA] on_block_imported: active round exists or no pending blocks");
        }
    }

    /// Start a new finality round for pending blocks
    fn start_finality_round(&mut self) {
        trace!("[GRANDPA] start_finality_round called, pending_blocks={}", self.pending_blocks.len());

        if self.pending_blocks.is_empty() {
            trace!("[GRANDPA] start_finality_round: no pending blocks, returning");
            return;
        }

        // Target the highest pending block
        let (target_number, target_hash) = self
            .pending_blocks
            .iter()
            .max_by_key(|(n, _)| n)
            .copied()
            .unwrap();

        let epoch = self.rounds.current_epoch();
        let round = self.rounds.current_round();

        trace!(
            "[GRANDPA] start_finality_round: target_block=#{}, epoch={}, round={}, is_validator={}",
            target_number, epoch, round, self.is_validator
        );

        info!(
            "🎯 Starting finality round {} for block #{}",
            round, target_number
        );

        self.rounds.start_round(epoch, round);

        // If we're a validator, create and broadcast our prevote
        if self.is_validator {
            trace!("[GRANDPA] start_finality_round: creating and broadcasting prevote");
            self.create_and_broadcast_prevote(target_number, target_hash);
        } else {
            trace!("[GRANDPA] start_finality_round: not a validator, not sending prevote");
        }
    }

    /// Create and broadcast a prevote
    fn create_and_broadcast_prevote(&mut self, target_number: BlockNumber, target_hash: Hash) {
        trace!("[GRANDPA] create_and_broadcast_prevote: target=#{}, hash={}", target_number, target_hash);

        if let Some(round) = self.rounds.active_round_mut() {
            let signer = self.signer.clone();
            trace!("[GRANDPA] create_and_broadcast_prevote: calling round.create_prevote");
            if let Some(vote) = round.create_prevote(target_number, target_hash, |msg| {
                signer.sign(msg)
            }) {
                trace!(
                    "[GRANDPA] create_and_broadcast_prevote: prevote created, voter=0x{}..{}, broadcasting",
                    hex::encode(&vote.voter.as_bytes()[..4]),
                    hex::encode(&vote.voter.as_bytes()[28..])
                );
                info!(
                    "🗳️ PREVOTE: block #{} by validator 0x{}..{}",
                    vote.target_number,
                    hex::encode(&vote.voter.as_bytes()[..4]),
                    hex::encode(&vote.voter.as_bytes()[28..])
                );
                let message = FinalityMessage::Vote(vote);
                self.broadcaster.broadcast(message.clone());
                self.outbound_queue.push_back(message);
                trace!("[GRANDPA] create_and_broadcast_prevote: message queued for broadcast");
            } else {
                trace!("[GRANDPA] create_and_broadcast_prevote: round.create_prevote returned None");
            }
        } else {
            trace!("[GRANDPA] create_and_broadcast_prevote: no active round");
        }
    }

    /// Create and broadcast a precommit
    fn create_and_broadcast_precommit(&mut self) {
        trace!("[GRANDPA] create_and_broadcast_precommit called");

        if let Some(round) = self.rounds.active_round_mut() {
            let signer = self.signer.clone();
            trace!("[GRANDPA] create_and_broadcast_precommit: calling round.create_precommit");
            if let Some(vote) = round.create_precommit(|msg| signer.sign(msg)) {
                trace!(
                    "[GRANDPA] create_and_broadcast_precommit: precommit created, voter=0x{}..{}, block=#{}",
                    hex::encode(&vote.voter.as_bytes()[..4]),
                    hex::encode(&vote.voter.as_bytes()[28..]),
                    vote.target_number
                );
                info!(
                    "✍️ PRECOMMIT: block #{} by validator 0x{}..{}",
                    vote.target_number,
                    hex::encode(&vote.voter.as_bytes()[..4]),
                    hex::encode(&vote.voter.as_bytes()[28..])
                );
                let message = FinalityMessage::Vote(vote);
                self.broadcaster.broadcast(message.clone());
                self.outbound_queue.push_back(message);
                trace!("[GRANDPA] create_and_broadcast_precommit: message queued for broadcast");
            } else {
                trace!("[GRANDPA] create_and_broadcast_precommit: round.create_precommit returned None");
            }
        } else {
            trace!("[GRANDPA] create_and_broadcast_precommit: no active round");
        }
    }

    /// Process an incoming finality message
    pub fn on_message(&mut self, message: FinalityMessage) -> Option<VoteResult> {
        trace!("[GRANDPA] on_message: received {:?}", std::mem::discriminant(&message));

        match message {
            FinalityMessage::Vote(vote) => {
                trace!("[GRANDPA] on_message: Vote message, delegating to on_vote");
                self.on_vote(vote)
            }
            FinalityMessage::RequestVotes { epoch, round } => {
                trace!("[GRANDPA] on_message: RequestVotes for epoch={}, round={}", epoch, round);
                self.handle_vote_request(epoch, round);
                None
            }
            FinalityMessage::Finalized {
                block_number,
                block_hash,
                epoch,
                round,
            } => {
                trace!("[GRANDPA] on_message: Finalized announcement for block #{}", block_number);
                self.handle_finalized_announcement(block_number, block_hash, epoch, round);
                None
            }
            FinalityMessage::CatchUpRequest {
                from_round,
                to_round,
                epoch,
            } => {
                trace!("[GRANDPA] on_message: CatchUpRequest from_round={}, to_round={}", from_round, to_round);
                self.handle_catchup_request(from_round, to_round, epoch);
                None
            }
            FinalityMessage::CatchUpResponse { votes, epoch } => {
                trace!("[GRANDPA] on_message: CatchUpResponse with {} votes", votes.len());
                self.handle_catchup_response(votes, epoch);
                None
            }
        }
    }

    /// Process an incoming vote
    fn on_vote(&mut self, vote: FinalityVote) -> Option<VoteResult> {
        trace!(
            "[GRANDPA] on_vote: type={:?}, voter=0x{}..{}, block=#{}, epoch={}, round={}",
            vote.vote_type,
            hex::encode(&vote.voter.as_bytes()[..4]),
            hex::encode(&vote.voter.as_bytes()[28..]),
            vote.target_number,
            vote.epoch,
            vote.round
        );

        let round = match self.rounds.active_round_mut() {
            Some(r) => r,
            None => {
                trace!("[GRANDPA] on_vote: no active round, ignoring vote");
                return None;
            }
        };

        // Check if vote is for current round
        if vote.epoch != round.epoch() || vote.round != round.round() {
            trace!(
                "[GRANDPA] on_vote: vote for different round: got ({}, {}), expected ({}, {})",
                vote.epoch, vote.round, round.epoch(), round.round()
            );
            debug!(
                "Vote for different round: got ({}, {}), expected ({}, {})",
                vote.epoch,
                vote.round,
                round.epoch(),
                round.round()
            );
            return None;
        }

        let prev_state = round.state();
        trace!("[GRANDPA] on_vote: prev_state={:?}, calling round.add_vote", prev_state);

        match round.add_vote(vote) {
            Ok(true) => {
                let new_state = round.state();
                trace!("[GRANDPA] on_vote: vote added successfully, new_state={:?}", new_state);

                // Check for state transition
                if new_state != prev_state {
                    trace!("[GRANDPA] on_vote: state transition from {:?} to {:?}", prev_state, new_state);
                    match new_state {
                        RoundState::Precommitting => {
                            trace!("[GRANDPA] on_vote: entering Precommitting phase, is_validator={}", self.is_validator);
                            info!("📊 Prevote threshold reached, moving to precommit phase");
                            // Create our precommit
                            if self.is_validator {
                                self.create_and_broadcast_precommit();
                            }
                            return Some(VoteResult::StateChanged(new_state));
                        }
                        RoundState::Completed => {
                            trace!("[GRANDPA] on_vote: entering Completed phase");
                            if let Some((block, hash)) = round.finalized_target() {
                                // Get list of voters who participated in finalization
                                let precommits = round.collector().get_precommits_for_best();
                                let voter_count = precommits.len();
                                let voters: Vec<String> = precommits.iter()
                                    .map(|v| format!("0x{}..{}",
                                        hex::encode(&v.voter.as_bytes()[..4]),
                                        hex::encode(&v.voter.as_bytes()[28..])))
                                    .collect();

                                info!(
                                    "🔒 FINALIZED! Block #{} confirmed by {} voters: [{}]",
                                    block, voter_count, voters.join(", ")
                                );
                                info!(
                                    "💰 Finality voters will receive 10% of fees (shared equally among {} participants)",
                                    voter_count
                                );

                                // Create justification
                                if let Some(justification) = round.create_justification() {
                                    self.justifications.push(justification);
                                }

                                // Announce finalization
                                let msg = FinalityMessage::Finalized {
                                    block_number: block,
                                    block_hash: hash,
                                    epoch: round.epoch(),
                                    round: round.round(),
                                };
                                self.broadcaster.broadcast(msg);

                                // Complete the round
                                self.complete_current_round(Some((block, hash)));

                                return Some(VoteResult::Finalized(block, hash));
                            }
                        }
                        _ => {}
                    }
                }
                Some(VoteResult::Accepted)
            }
            Ok(false) => {
                trace!("[GRANDPA] on_vote: duplicate vote");
                Some(VoteResult::Accepted) // Duplicate vote
            }
            Err(e) => {
                trace!("[GRANDPA] on_vote: vote rejected with error: {:?}", e);
                warn!("Vote rejected: {}", e);
                Some(VoteResult::Rejected(e))
            }
        }
    }

    /// Handle a vote request
    fn handle_vote_request(&mut self, epoch: EpochNumber, round: u32) {
        if let Some(active_round) = self.rounds.active_round() {
            if active_round.epoch() == epoch && active_round.round() == round {
                let votes = active_round.all_votes();
                if !votes.is_empty() {
                    let response = FinalityMessage::CatchUpResponse { votes, epoch };
                    self.broadcaster.broadcast(response);
                }
            }
        }
    }

    /// Handle a finalized announcement
    fn handle_finalized_announcement(
        &mut self,
        block_number: BlockNumber,
        block_hash: Hash,
        epoch: EpochNumber,
        round: u32,
    ) {
        let last_finalized = self.rounds.last_finalized();

        // Only process if this is newer than what we have
        if block_number > last_finalized.0 {
            info!(
                "📢 Received finalization announcement for block #{} (epoch {}, round {})",
                block_number, epoch, round
            );

            // Update our finalized state
            self.complete_current_round(Some((block_number, block_hash)));

            // Remove finalized blocks from pending
            self.pending_blocks
                .retain(|(n, _)| *n > block_number);
        }
    }

    /// Handle a catch-up request
    fn handle_catchup_request(&mut self, from_round: u32, to_round: u32, epoch: EpochNumber) {
        debug!(
            "Catch-up request received for rounds {}-{} epoch {}",
            from_round, to_round, epoch
        );
        // In a full implementation, we'd send historical votes
        // For now, just send current round votes if applicable
        if let Some(round) = self.rounds.active_round() {
            if round.epoch() == epoch && round.round() >= from_round && round.round() <= to_round {
                let votes = round.all_votes();
                if !votes.is_empty() {
                    let response = FinalityMessage::CatchUpResponse { votes, epoch };
                    self.broadcaster.broadcast(response);
                }
            }
        }
    }

    /// Handle a catch-up response
    fn handle_catchup_response(&mut self, votes: Vec<FinalityVote>, epoch: EpochNumber) {
        debug!(
            "Catch-up response received with {} votes for epoch {}",
            votes.len(),
            epoch
        );

        for vote in votes {
            let _ = self.on_vote(vote);
        }
    }

    /// Complete the current round
    fn complete_current_round(&mut self, finalized: Option<(BlockNumber, Hash)>) {
        self.rounds.complete_round(finalized);

        // Remove finalized blocks from pending
        if let Some((block_num, _)) = finalized {
            self.pending_blocks.retain(|(n, _)| *n > block_num);
        }

        // Start next round if there are pending blocks
        if !self.pending_blocks.is_empty() {
            self.rounds.next_round();
            self.start_finality_round();
        }
    }

    /// Tick the gadget (called periodically)
    ///
    /// Returns true if a round timed out and was advanced
    pub fn tick(&mut self) -> bool {
        if let Some(round) = self.rounds.active_round_mut() {
            let is_timed_out = round.is_timed_out();
            let is_done = round.is_done();
            let should_precommit = round.should_precommit();

            trace!(
                "[GRANDPA] tick: round={}, is_timed_out={}, is_done={}, should_precommit={}, is_validator={}",
                round.round(), is_timed_out, is_done, should_precommit, self.is_validator
            );

            if is_timed_out && !is_done {
                trace!("[GRANDPA] tick: round timed out, advancing to next round");
                warn!(
                    "⏰ Round {} timed out, advancing to next round",
                    round.round()
                );
                round.mark_failed();
                self.complete_current_round(None);

                // Check for stuck finality
                if self.rounds.current_round() > config::MAX_ROUNDS_BEFORE_FORCE {
                    warn!(
                        "🚨 Finality stuck for {} rounds!",
                        self.rounds.current_round()
                    );
                }

                return true;
            }

            // Check if we should precommit
            if should_precommit && self.is_validator {
                trace!("[GRANDPA] tick: should_precommit=true, creating precommit");
                self.create_and_broadcast_precommit();
            }
        } else {
            trace!("[GRANDPA] tick: no active round");
        }

        false
    }

    /// Get last finalized block
    pub fn last_finalized(&self) -> (BlockNumber, Hash) {
        self.rounds.last_finalized()
    }

    /// Get current round number
    pub fn current_round(&self) -> u32 {
        self.rounds.current_round()
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> EpochNumber {
        self.rounds.current_epoch()
    }

    /// Get all stored justifications
    pub fn justifications(&self) -> &[FinalityJustification] {
        &self.justifications
    }

    /// Get justification for a specific block
    pub fn get_justification(&self, block_number: BlockNumber) -> Option<&FinalityJustification> {
        self.justifications
            .iter()
            .find(|j| j.block_number == block_number)
    }

    /// Get all detected equivocations
    pub fn equivocations(&self) -> &[EquivocationProof] {
        &self.equivocations
    }

    /// Update validator set (e.g., at epoch boundary)
    pub fn update_validators(&mut self, validators: HashSet<AccountId>) {
        let validator_id = self.signer.validator_id();
        let was_validator = self.is_validator;
        self.is_validator = validators.contains(&validator_id);
        self.rounds.update_validators(validators.clone(), validator_id);

        trace!(
            "[GRANDPA] update_validators: validator_count={}, was_validator={}, is_validator={}",
            validators.len(), was_validator, self.is_validator
        );

        info!(
            "Validator set updated. We are {}a validator",
            if self.is_validator { "" } else { "not " }
        );
    }

    /// Start a new epoch
    pub fn new_epoch(&mut self, epoch: EpochNumber) {
        info!("📅 Starting new finality epoch {}", epoch);
        self.rounds.new_epoch(epoch);
    }

    /// Get pending outbound messages
    pub fn drain_outbound(&mut self) -> Vec<FinalityMessage> {
        self.outbound_queue.drain(..).collect()
    }

    /// Check if gadget is actively participating
    pub fn is_active(&self) -> bool {
        self.is_validator && self.rounds.has_active_round()
    }

    /// Get round summary if active
    pub fn round_summary(&self) -> Option<super::types::RoundSummary> {
        self.rounds.active_round().map(|r| r.summary())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockSigner {
        id: AccountId,
    }

    impl FinalitySigner for MockSigner {
        fn sign(&self, _message: &[u8]) -> Signature64 {
            Signature64::zero()
        }

        fn validator_id(&self) -> AccountId {
            self.id
        }
    }

    struct MockBroadcaster {
        messages: std::sync::Mutex<Vec<FinalityMessage>>,
    }

    impl MockBroadcaster {
        fn new() -> Self {
            Self {
                messages: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn message_count(&self) -> usize {
            self.messages.lock().unwrap().len()
        }
    }

    impl FinalityBroadcaster for MockBroadcaster {
        fn broadcast(&self, message: FinalityMessage) {
            self.messages.lock().unwrap().push(message);
        }
    }

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
    fn test_gadget_creation() {
        let validators = make_validators(3);
        let validator_id = validators.iter().next().unwrap().clone();

        let signer = Arc::new(MockSigner { id: validator_id });
        let broadcaster = Arc::new(MockBroadcaster::new());

        let gadget = FinalityGadget::new(signer, broadcaster, validators.clone(), Hash::ZERO);

        assert!(gadget.is_validator);
        assert_eq!(gadget.last_finalized(), (0, Hash::ZERO));
    }

    #[test]
    fn test_block_import_triggers_round() {
        let validators = make_validators(3);
        let validator_id = validators.iter().next().unwrap().clone();

        let signer = Arc::new(MockSigner { id: validator_id });
        let broadcaster = Arc::new(MockBroadcaster::new());

        let mut gadget = FinalityGadget::new(signer, broadcaster.clone(), validators, Hash::ZERO);

        // Import a block
        let block_hash = Hash::hash(b"block1");
        gadget.on_block_imported(1, block_hash);

        // Should have started a round and broadcast a prevote
        assert!(broadcaster.message_count() > 0);
    }

    #[test]
    fn test_non_validator_no_votes() {
        let validators = make_validators(3);
        // Use a non-validator ID
        let non_validator_id = AccountId::from_bytes([99; 32]);

        let signer = Arc::new(MockSigner {
            id: non_validator_id,
        });
        let broadcaster = Arc::new(MockBroadcaster::new());

        let mut gadget = FinalityGadget::new(signer, broadcaster.clone(), validators, Hash::ZERO);

        assert!(!gadget.is_validator);

        // Import a block
        gadget.on_block_imported(1, Hash::hash(b"block1"));

        // Non-validator should not broadcast votes
        assert_eq!(broadcaster.message_count(), 0);
    }
}
