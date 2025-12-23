// Finality Types - Core data structures for GRANDPA-style finality
//
// This module defines the fundamental types used in the finality protocol:
// - VoteType: Prevote or Precommit
// - FinalityVote: A signed vote from a validator
// - RoundState: State of a finality round
// - FinalityMessage: Network messages for finality gossip

use crate::types::account::AccountId;
use crate::types::primitives::{BlockNumber, EpochNumber, Hash, Timestamp};
use crate::types::signature::{domain_separate, Signature64, DOMAIN_FINALITY};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Type of finality vote
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VoteType {
    /// First phase: declare intent to finalize a block
    Prevote,
    /// Second phase: commit to finalize after seeing supermajority prevotes
    Precommit,
}

impl std::fmt::Display for VoteType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoteType::Prevote => write!(f, "prevote"),
            VoteType::Precommit => write!(f, "precommit"),
        }
    }
}

/// A finality vote from a validator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalityVote {
    /// Type of vote (prevote or precommit)
    pub vote_type: VoteType,

    /// Target block number to finalize
    pub target_number: BlockNumber,

    /// Target block hash to finalize
    pub target_hash: Hash,

    /// Round number within the epoch
    pub round: u32,

    /// Epoch number
    pub epoch: EpochNumber,

    /// Validator who cast this vote
    pub voter: AccountId,

    /// Signature over the vote
    pub signature: Signature64,

    /// Timestamp when vote was created
    pub timestamp: Timestamp,
}

impl FinalityVote {
    /// Create a new unsigned vote
    pub fn new(
        vote_type: VoteType,
        target_number: BlockNumber,
        target_hash: Hash,
        round: u32,
        epoch: EpochNumber,
        voter: AccountId,
    ) -> Self {
        Self {
            vote_type,
            target_number,
            target_hash,
            round,
            epoch,
            voter,
            signature: Signature64::zero(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Create the message to sign (with domain separation)
    pub fn signing_message(&self) -> Vec<u8> {
        let vote_type_byte = match self.vote_type {
            VoteType::Prevote => 0u8,
            VoteType::Precommit => 1u8,
        };

        let message = bincode::serialize(&(
            vote_type_byte,
            self.target_number,
            self.target_hash,
            self.round,
            self.epoch,
        ))
        .expect("Vote serialization should not fail");

        domain_separate(DOMAIN_FINALITY, &message)
    }

    /// Verify the signature on this vote
    pub fn verify(&self) -> bool {
        let message = self.signing_message();
        self.voter.verify(&message, self.signature.as_bytes())
    }

    /// Create a unique identifier for this vote (for deduplication)
    pub fn id(&self) -> VoteId {
        VoteId {
            vote_type: self.vote_type,
            round: self.round,
            epoch: self.epoch,
            voter: self.voter,
        }
    }
}

impl PartialEq for FinalityVote {
    fn eq(&self, other: &Self) -> bool {
        self.vote_type == other.vote_type
            && self.target_number == other.target_number
            && self.target_hash == other.target_hash
            && self.round == other.round
            && self.epoch == other.epoch
            && self.voter == other.voter
    }
}

impl Eq for FinalityVote {}

impl std::hash::Hash for FinalityVote {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.vote_type.hash(state);
        self.target_number.hash(state);
        self.target_hash.hash(state);
        self.round.hash(state);
        self.epoch.hash(state);
        self.voter.hash(state);
    }
}

/// Unique identifier for a vote (used for deduplication)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VoteId {
    pub vote_type: VoteType,
    pub round: u32,
    pub epoch: EpochNumber,
    pub voter: AccountId,
}

/// State of a finality round
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundState {
    /// Collecting prevotes
    Prevoting,
    /// Got supermajority prevotes, now collecting precommits
    Precommitting,
    /// Round completed with finalization
    Completed,
    /// Round failed (timeout or insufficient votes)
    Failed,
}

impl Default for RoundState {
    fn default() -> Self {
        RoundState::Prevoting
    }
}

/// Summary of a finality round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundSummary {
    pub round: u32,
    pub epoch: EpochNumber,
    pub state: RoundState,
    pub prevote_count: usize,
    pub precommit_count: usize,
    pub target_block: Option<(BlockNumber, Hash)>,
    pub total_validators: usize,
}

/// Network message for finality gossip
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FinalityMessage {
    /// A single vote
    Vote(FinalityVote),

    /// Request votes for a specific round
    RequestVotes {
        epoch: EpochNumber,
        round: u32,
    },

    /// Announce finalization
    Finalized {
        block_number: BlockNumber,
        block_hash: Hash,
        epoch: EpochNumber,
        round: u32,
    },

    /// Catch-up request for nodes that are behind
    CatchUpRequest {
        from_round: u32,
        to_round: u32,
        epoch: EpochNumber,
    },

    /// Catch-up response with historical votes
    CatchUpResponse {
        votes: Vec<FinalityVote>,
        epoch: EpochNumber,
    },
}

impl FinalityMessage {
    /// Serialize for network transmission
    pub fn encode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("FinalityMessage serialization should not fail")
    }

    /// Deserialize from network
    pub fn decode(data: &[u8]) -> Result<Self, String> {
        bincode::deserialize(data).map_err(|e| format!("Failed to decode FinalityMessage: {}", e))
    }
}

/// Equivocation proof - evidence that a validator voted twice in the same round
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquivocationProof {
    /// The offending validator
    pub validator: AccountId,

    /// First vote
    pub vote1: FinalityVote,

    /// Second conflicting vote
    pub vote2: FinalityVote,

    /// Round where equivocation occurred
    pub round: u32,

    /// Epoch where equivocation occurred
    pub epoch: EpochNumber,
}

impl EquivocationProof {
    /// Verify that this is a valid equivocation proof
    pub fn is_valid(&self) -> bool {
        // Same voter
        if self.vote1.voter != self.vote2.voter {
            return false;
        }

        // Same round and epoch
        if self.vote1.round != self.vote2.round || self.vote1.epoch != self.vote2.epoch {
            return false;
        }

        // Same vote type
        if self.vote1.vote_type != self.vote2.vote_type {
            return false;
        }

        // Different targets (the actual equivocation)
        if self.vote1.target_hash == self.vote2.target_hash {
            return false;
        }

        // Both signatures must be valid
        self.vote1.verify() && self.vote2.verify()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vote_type_display() {
        assert_eq!(format!("{}", VoteType::Prevote), "prevote");
        assert_eq!(format!("{}", VoteType::Precommit), "precommit");
    }

    #[test]
    fn test_vote_id_uniqueness() {
        let voter = AccountId::from_bytes([1; 32]);

        let vote1 = FinalityVote::new(
            VoteType::Prevote,
            100,
            Hash::ZERO,
            1,
            0,
            voter,
        );

        let vote2 = FinalityVote::new(
            VoteType::Precommit,
            100,
            Hash::ZERO,
            1,
            0,
            voter,
        );

        // Different vote types should have different IDs
        assert_ne!(vote1.id(), vote2.id());

        // Same parameters should have same ID
        let vote3 = FinalityVote::new(
            VoteType::Prevote,
            100,
            Hash::ZERO,
            1,
            0,
            voter,
        );
        assert_eq!(vote1.id(), vote3.id());
    }

    #[test]
    fn test_round_state_default() {
        assert_eq!(RoundState::default(), RoundState::Prevoting);
    }

    #[test]
    fn test_finality_message_encode_decode() {
        let voter = AccountId::from_bytes([1; 32]);
        let vote = FinalityVote::new(
            VoteType::Prevote,
            100,
            Hash::ZERO,
            1,
            0,
            voter,
        );

        let msg = FinalityMessage::Vote(vote.clone());
        let encoded = msg.encode();
        let decoded = FinalityMessage::decode(&encoded).unwrap();

        match decoded {
            FinalityMessage::Vote(v) => {
                assert_eq!(v.voter, vote.voter);
                assert_eq!(v.target_number, vote.target_number);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_signing_message_domain_separation() {
        let voter = AccountId::from_bytes([1; 32]);
        let vote = FinalityVote::new(
            VoteType::Prevote,
            100,
            Hash::ZERO,
            1,
            0,
            voter,
        );

        let msg = vote.signing_message();
        // Should start with domain separator
        assert!(msg.starts_with(DOMAIN_FINALITY));
    }
}
