// Finality Integration - Connects GRANDPA finality gadget to the node
//
// SECURITY PRINCIPLES (per CONSTITUTION):
// - Sovereignty: Finality requires 2/3 supermajority, protecting against minority attacks
// - Resilience: Failed rounds advance gracefully, network continues operating
// - Security: Double-voting (equivocation) is detected and can trigger slashing
//
// This module provides:
// - NodeFinalitySigner: Signs finality votes with validator key
// - NodeFinalityBroadcaster: Broadcasts votes via P2P network
// - FinalityIntegration: Coordinates finality with node operations

use crate::consensus::finality::{
    FinalityGadget, FinalityMessage, FinalityVote,
    gadget::{FinalitySigner, FinalityBroadcaster, VoteResult},
    config::MIN_VALIDATORS_FOR_FINALITY,
};
use crate::network::protocol::NetworkMessage;
use crate::types::account::AccountId;
use crate::types::primitives::{BlockNumber, EpochNumber, Hash};
use crate::types::signature::Signature64;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock, Mutex};
use tracing::{debug, info, warn, error, trace};

// =============================================================================
// FINALITY SIGNER
// =============================================================================

/// Signs finality votes using the validator's keypair
///
/// SECURITY: The private key never leaves this struct
/// Only signature operations are exposed
pub struct NodeFinalitySigner {
    /// Validator account ID (public)
    validator_id: AccountId,

    /// Signing function (encapsulates private key)
    /// This closure holds the keypair and performs signing internally
    sign_fn: Arc<dyn Fn(&[u8]) -> Signature64 + Send + Sync>,
}

impl NodeFinalitySigner {
    /// Create a new signer with the validator's keypair
    ///
    /// The keypair is captured in a closure to prevent accidental exposure
    pub fn new<F>(validator_id: AccountId, sign_fn: F) -> Self
    where
        F: Fn(&[u8]) -> Signature64 + Send + Sync + 'static,
    {
        Self {
            validator_id,
            sign_fn: Arc::new(sign_fn),
        }
    }
}

impl FinalitySigner for NodeFinalitySigner {
    fn sign(&self, message: &[u8]) -> Signature64 {
        (self.sign_fn)(message)
    }

    fn validator_id(&self) -> AccountId {
        self.validator_id
    }
}

// =============================================================================
// FINALITY BROADCASTER
// =============================================================================

/// Channel for sending finality messages to the network layer
pub type FinalityMessageSender = mpsc::UnboundedSender<FinalityMessage>;

/// Broadcasts finality votes and justifications to the P2P network
///
/// RESILIENCE: Uses unbounded channel to prevent blocking finality
/// Messages are queued and processed by the network layer asynchronously
pub struct NodeFinalityBroadcaster {
    /// Channel to send messages to network layer
    tx: FinalityMessageSender,
}

impl NodeFinalityBroadcaster {
    pub fn new(tx: FinalityMessageSender) -> Self {
        Self { tx }
    }
}

impl FinalityBroadcaster for NodeFinalityBroadcaster {
    fn broadcast(&self, message: FinalityMessage) {
        if let Err(e) = self.tx.send(message) {
            warn!("Failed to queue finality message for broadcast: {:?}", e);
        }
    }
}

// =============================================================================
// FINALITY INTEGRATION
// =============================================================================

/// Coordinates the finality gadget with node operations
///
/// Responsibilities:
/// - Initialize and manage the FinalityGadget lifecycle
/// - Process incoming finality votes from network
/// - Notify gadget of new blocks
/// - Track finalized blocks for reward distribution
pub struct FinalityIntegration<S: FinalitySigner + 'static, B: FinalityBroadcaster + 'static> {
    /// The finality gadget
    gadget: RwLock<FinalityGadget<S, B>>,

    /// List of validators who participated in the last finalization
    /// Used for distributing finality rewards
    last_finality_voters: RwLock<Vec<AccountId>>,

    /// Last finalized block number (for tracking progress)
    last_finalized: RwLock<BlockNumber>,

    /// Whether finality is active (requires minimum validators)
    is_active: RwLock<bool>,
}

impl<S: FinalitySigner + 'static, B: FinalityBroadcaster + 'static> FinalityIntegration<S, B> {
    /// Create new finality integration
    ///
    /// SECURITY: Finality only activates when MIN_VALIDATORS_FOR_FINALITY is met
    /// This prevents attacks when validator set is too small
    pub fn new(
        signer: Arc<S>,
        broadcaster: Arc<B>,
        validators: HashSet<AccountId>,
        genesis_hash: Hash,
    ) -> Self {
        let validator_count = validators.len();
        let is_active = validator_count >= MIN_VALIDATORS_FOR_FINALITY;

        if is_active {
            info!(
                "🔐 Finality gadget ACTIVE with {} validators (min: {})",
                validator_count, MIN_VALIDATORS_FOR_FINALITY
            );
        } else {
            info!(
                "⏳ Finality gadget STANDBY - waiting for {} validators (have: {})",
                MIN_VALIDATORS_FOR_FINALITY, validator_count
            );
        }

        let gadget = FinalityGadget::new(signer, broadcaster, validators, genesis_hash);

        trace!("[GRANDPA] FinalityIntegration created, is_active={}", is_active);

        Self {
            gadget: RwLock::new(gadget),
            last_finality_voters: RwLock::new(Vec::new()),
            last_finalized: RwLock::new(0),
            is_active: RwLock::new(is_active),
        }
    }

    /// Notify the gadget that a new block has been imported
    ///
    /// This triggers finality voting if we're an active validator
    pub async fn on_block_imported(&self, block_number: BlockNumber, block_hash: Hash) {
        trace!("[GRANDPA] on_block_imported called: block #{}, hash={}", block_number, block_hash);

        let is_active = *self.is_active.read().await;
        if !is_active {
            trace!("[GRANDPA] on_block_imported: gadget not active, skipping");
            return;
        }

        trace!("[GRANDPA] on_block_imported: acquiring gadget write lock");
        let mut gadget = self.gadget.write().await;
        trace!("[GRANDPA] on_block_imported: calling gadget.on_block_imported");
        gadget.on_block_imported(block_number, block_hash);
        trace!("[GRANDPA] on_block_imported: done");
    }

    /// Process an incoming finality vote from the network
    ///
    /// Returns the list of voters if this vote completed finalization
    pub async fn on_finality_vote(&self, vote: FinalityVote) -> Option<Vec<AccountId>> {
        trace!(
            "[GRANDPA] on_finality_vote: type={:?}, voter=0x{}..{}, epoch={}, round={}, block=#{}",
            vote.vote_type,
            hex::encode(&vote.voter.as_bytes()[..4]),
            hex::encode(&vote.voter.as_bytes()[28..]),
            vote.epoch,
            vote.round,
            vote.target_number
        );

        if !*self.is_active.read().await {
            trace!("[GRANDPA] on_finality_vote: gadget not active, ignoring");
            debug!("Ignoring finality vote - gadget not active");
            return None;
        }

        trace!("[GRANDPA] on_finality_vote: acquiring gadget write lock");
        let mut gadget = self.gadget.write().await;
        let message = FinalityMessage::Vote(vote);

        trace!("[GRANDPA] on_finality_vote: calling gadget.on_message");
        match gadget.on_message(message) {
            Some(VoteResult::Finalized(block_number, _hash)) => {
                trace!("[GRANDPA] on_finality_vote: VoteResult::Finalized for block #{}", block_number);
                // Get the voters who participated
                if let Some(summary) = gadget.round_summary() {
                    info!(
                        "🔒 Block #{} finalized with {} prevotes, {} precommits",
                        block_number, summary.prevote_count, summary.precommit_count
                    );
                }

                // Collect voter list from justification
                if let Some(justification) = gadget.get_justification(block_number) {
                    let voters: Vec<AccountId> = justification.signatures
                        .iter()
                        .map(|sig| sig.validator)
                        .collect();

                    // Store for reward distribution
                    *self.last_finality_voters.write().await = voters.clone();
                    *self.last_finalized.write().await = block_number;

                    return Some(voters);
                }

                None
            }
            Some(VoteResult::StateChanged(state)) => {
                trace!("[GRANDPA] on_finality_vote: VoteResult::StateChanged to {:?}", state);
                debug!("Finality round state changed to {:?}", state);
                None
            }
            Some(VoteResult::Accepted) => {
                trace!("[GRANDPA] on_finality_vote: VoteResult::Accepted");
                debug!("Finality vote accepted");
                None
            }
            Some(VoteResult::Rejected(error)) => {
                trace!("[GRANDPA] on_finality_vote: VoteResult::Rejected({:?})", error);
                warn!("Finality vote rejected: {:?}", error);
                None
            }
            None => {
                trace!("[GRANDPA] on_finality_vote: no result from gadget.on_message");
                None
            }
        }
    }

    /// Process a finality message from the network
    pub async fn on_finality_message(&self, message: FinalityMessage) -> Option<Vec<AccountId>> {
        trace!("[GRANDPA] on_finality_message: {:?}", std::mem::discriminant(&message));
        match message {
            FinalityMessage::Vote(vote) => self.on_finality_vote(vote).await,
            other => {
                trace!("[GRANDPA] on_finality_message: handling non-vote message");
                let mut gadget = self.gadget.write().await;
                gadget.on_message(other);
                None
            }
        }
    }

    /// Periodic tick for timeout handling
    ///
    /// RESILIENCE: Advances to next round if current round times out
    /// Prevents finality from stalling due to network issues
    pub async fn tick(&self) -> bool {
        let is_active = *self.is_active.read().await;
        if !is_active {
            return false;
        }

        trace!("[GRANDPA] tick: acquiring gadget write lock");
        let mut gadget = self.gadget.write().await;
        let result = gadget.tick();
        if result {
            trace!("[GRANDPA] tick: round timed out, advanced to next round");
        }
        result
    }

    /// Update the validator set (e.g., at epoch boundary)
    ///
    /// SECURITY: Re-evaluates whether finality can be active
    pub async fn update_validators(&self, validators: HashSet<AccountId>) {
        trace!("[GRANDPA] update_validators called with {} validators", validators.len());
        let validator_count = validators.len();
        let should_be_active = validator_count >= MIN_VALIDATORS_FOR_FINALITY;

        let was_active = *self.is_active.read().await;

        if should_be_active && !was_active {
            info!(
                "🔐 Finality gadget ACTIVATING - {} validators now available",
                validator_count
            );
        } else if !should_be_active && was_active {
            warn!(
                "⚠️  Finality gadget DEACTIVATING - only {} validators (need {})",
                validator_count, MIN_VALIDATORS_FOR_FINALITY
            );
        }

        *self.is_active.write().await = should_be_active;
        trace!("[GRANDPA] update_validators: is_active set to {}", should_be_active);

        let mut gadget = self.gadget.write().await;
        gadget.update_validators(validators);
        trace!("[GRANDPA] update_validators: done");
    }

    /// Start a new epoch
    pub async fn new_epoch(&self, epoch: EpochNumber) {
        let mut gadget = self.gadget.write().await;
        gadget.new_epoch(epoch);
    }

    /// Get the voters from the last finalization
    ///
    /// Used for distributing finality rewards (10% of fees)
    pub async fn get_last_finality_voters(&self) -> Vec<AccountId> {
        self.last_finality_voters.read().await.clone()
    }

    /// Get the last finalized block number
    pub async fn get_last_finalized(&self) -> BlockNumber {
        *self.last_finalized.read().await
    }

    /// Check if finality is currently active
    pub async fn is_active(&self) -> bool {
        *self.is_active.read().await
    }

    /// Get current finality status for RPC
    pub async fn status(&self) -> FinalityStatus {
        let gadget = self.gadget.read().await;
        let (last_finalized_block, last_finalized_hash) = gadget.last_finalized();

        FinalityStatus {
            is_active: *self.is_active.read().await,
            last_finalized_block,
            last_finalized_hash,
            current_round: gadget.current_round(),
            current_epoch: gadget.current_epoch(),
        }
    }

    /// Drain outbound finality messages for network transmission
    pub async fn drain_outbound(&self) -> Vec<FinalityMessage> {
        let mut gadget = self.gadget.write().await;
        let messages = gadget.drain_outbound();
        if !messages.is_empty() {
            trace!("[GRANDPA] drain_outbound: {} messages to send", messages.len());
        }
        messages
    }
}

/// Finality status for RPC responses
#[derive(Debug, Clone)]
pub struct FinalityStatus {
    pub is_active: bool,
    pub last_finalized_block: BlockNumber,
    pub last_finalized_hash: Hash,
    pub current_round: u32,
    pub current_epoch: EpochNumber,
}

// =============================================================================
// TESTS
// =============================================================================

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

    struct MockBroadcaster;

    impl FinalityBroadcaster for MockBroadcaster {
        fn broadcast(&self, _message: FinalityMessage) {
            // No-op for tests
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

    #[tokio::test]
    async fn test_finality_activation() {
        let validators = make_validators(2);
        let validator_id = validators.iter().next().unwrap().clone();

        let signer = Arc::new(MockSigner { id: validator_id });
        let broadcaster = Arc::new(MockBroadcaster);

        // With only 2 validators, finality should be inactive (need 3)
        let integration = FinalityIntegration::new(
            signer,
            broadcaster,
            validators,
            Hash::ZERO,
        );

        assert!(!integration.is_active().await);
    }

    #[tokio::test]
    async fn test_finality_with_enough_validators() {
        let validators = make_validators(3);
        let validator_id = validators.iter().next().unwrap().clone();

        let signer = Arc::new(MockSigner { id: validator_id });
        let broadcaster = Arc::new(MockBroadcaster);

        // With 3 validators, finality should be active
        let integration = FinalityIntegration::new(
            signer,
            broadcaster,
            validators,
            Hash::ZERO,
        );

        assert!(integration.is_active().await);
    }

    #[tokio::test]
    async fn test_validator_set_update() {
        let validators = make_validators(2);
        let validator_id = validators.iter().next().unwrap().clone();

        let signer = Arc::new(MockSigner { id: validator_id });
        let broadcaster = Arc::new(MockBroadcaster);

        let integration = FinalityIntegration::new(
            signer,
            broadcaster,
            validators,
            Hash::ZERO,
        );

        // Initially inactive
        assert!(!integration.is_active().await);

        // Add a third validator
        let new_validators = make_validators(3);
        integration.update_validators(new_validators).await;

        // Now should be active
        assert!(integration.is_active().await);
    }
}
