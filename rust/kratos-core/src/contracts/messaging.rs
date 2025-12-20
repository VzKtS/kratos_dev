// Cross-Chain Messaging - SPEC v3.1 Phase 8
// Secure cross-chain communication with Merkle proof verification

use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash};
use crate::types::merkle::MerkleProof;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Unique identifier for a cross-chain message
pub type MessageId = u64;

/// Maximum messages per block per chain
pub const MAX_MESSAGES_PER_BLOCK: usize = 100;

/// Message expiry period (in blocks @ 6s/block)
/// Messages not delivered within 7 days are considered expired
pub const MESSAGE_EXPIRY: BlockNumber = 100_800;

/// Maximum payload size (64KB)
pub const MAX_PAYLOAD_SIZE: usize = 65_536;

/// Message fee (in KRAT units)
pub const BASE_MESSAGE_FEE: Balance = 1;

/// Cross-chain message structure (SPEC v3.1 Section 6.1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainMessage {
    /// Unique message ID
    pub id: MessageId,

    /// Source chain ID
    pub source_chain: ChainId,

    /// Target chain ID
    pub target_chain: ChainId,

    /// Sender account on source chain
    pub sender: AccountId,

    /// Recipient account on target chain (optional)
    pub recipient: Option<AccountId>,

    /// Message type
    pub message_type: MessageType,

    /// Payload hash (for verification)
    pub payload_hash: Hash,

    /// Actual payload (may be empty if only hash is needed)
    pub payload: Vec<u8>,

    /// State root of source chain when message was sent
    pub source_state_root: Hash,

    /// Merkle proof of message inclusion in source chain
    pub inclusion_proof: Option<MerkleProof>,

    /// Block number when message was created
    pub created_at: BlockNumber,

    /// Block number when message expires
    pub expires_at: BlockNumber,

    /// Current status
    pub status: MessageStatus,

    /// Fee paid for this message
    pub fee: Balance,

    /// Nonce for ordering
    pub nonce: u64,
}

/// Types of cross-chain messages
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// Simple data transfer
    DataTransfer,

    /// Asset transfer request
    AssetTransfer {
        amount: Balance,
        asset_id: Option<Hash>,
    },

    /// Governance notification (e.g., exit vote passed)
    GovernanceNotification {
        notification_type: GovernanceNotificationType,
    },

    /// Arbitration-related (dispute raised, verdict delivered)
    ArbitrationMessage {
        dispute_id: u64,
        action: ArbitrationAction,
    },

    /// State root commitment
    StateRootCommitment {
        block_number: BlockNumber,
        state_root: Hash,
    },

    /// Validator set update
    ValidatorSetUpdate {
        validators: Vec<AccountId>,
        threshold: u32,
    },

    /// Custom message with type tag
    Custom {
        type_tag: String,
    },
}

/// Governance notification types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GovernanceNotificationType {
    /// Chain is exiting
    ExitInitiated,
    /// Chain has been purged
    ChainPurged,
    /// Validator slashed
    ValidatorSlashed,
    /// Proposal passed
    ProposalPassed,
}

/// Arbitration actions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArbitrationAction {
    DisputeRaised,
    EvidenceSubmitted,
    JurySelected,
    VerdictDelivered,
    EnforcementExecuted,
}

/// Message delivery status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageStatus {
    /// Message created, pending delivery
    Pending,

    /// Message is in the outbox, awaiting relay
    InOutbox,

    /// Message relayed to target chain
    Relayed,

    /// Message verified and delivered
    Delivered,

    /// Message verification failed
    Failed,

    /// Message expired before delivery
    Expired,

    /// Message cancelled by sender
    Cancelled,
}

/// Message verification result
#[derive(Debug, Clone)]
pub struct MessageVerification {
    pub is_valid: bool,
    pub message_id: MessageId,
    pub source_chain: ChainId,
    pub error: Option<String>,
}

/// Cross-chain messaging contract
pub struct MessagingContract {
    /// All messages by ID
    messages: HashMap<MessageId, CrossChainMessage>,

    /// Outbox per chain (messages waiting to be relayed)
    outbox: HashMap<ChainId, VecDeque<MessageId>>,

    /// Inbox per chain (messages received)
    inbox: HashMap<ChainId, VecDeque<MessageId>>,

    /// Next message ID
    next_message_id: MessageId,

    /// Message nonce per (source_chain, sender) pair
    nonces: HashMap<(ChainId, AccountId), u64>,

    /// Known state roots per chain (for verification)
    known_state_roots: HashMap<(ChainId, BlockNumber), Hash>,

    /// Message count per block per chain
    messages_per_block: HashMap<(ChainId, BlockNumber), usize>,
}

impl MessagingContract {
    /// Create a new messaging contract
    pub fn new() -> Self {
        Self {
            messages: HashMap::new(),
            outbox: HashMap::new(),
            inbox: HashMap::new(),
            next_message_id: 1,
            nonces: HashMap::new(),
            known_state_roots: HashMap::new(),
            messages_per_block: HashMap::new(),
        }
    }

    /// Register a known state root for verification
    pub fn register_state_root(
        &mut self,
        chain_id: ChainId,
        block_number: BlockNumber,
        state_root: Hash,
    ) {
        self.known_state_roots.insert((chain_id, block_number), state_root);
    }

    /// Get current nonce for a sender
    pub fn get_nonce(&self, chain_id: ChainId, sender: &AccountId) -> u64 {
        self.nonces.get(&(chain_id, *sender)).copied().unwrap_or(0)
    }

    /// Send a cross-chain message
    pub fn send_message(
        &mut self,
        source_chain: ChainId,
        target_chain: ChainId,
        sender: AccountId,
        recipient: Option<AccountId>,
        message_type: MessageType,
        payload: Vec<u8>,
        source_state_root: Hash,
        current_block: BlockNumber,
        fee: Balance,
    ) -> Result<MessageId, MessagingError> {
        // Validate payload size
        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(MessagingError::PayloadTooLarge);
        }

        // Check message limit per block
        let block_count = self.messages_per_block
            .entry((source_chain, current_block))
            .or_insert(0);
        if *block_count >= MAX_MESSAGES_PER_BLOCK {
            return Err(MessagingError::TooManyMessagesInBlock);
        }

        // Validate fee
        if fee < BASE_MESSAGE_FEE {
            return Err(MessagingError::InsufficientFee);
        }

        // Cannot send to self
        if source_chain == target_chain {
            return Err(MessagingError::CannotMessageSelf);
        }

        // Get and increment nonce
        let nonce_key = (source_chain, sender);
        let nonce = self.nonces.entry(nonce_key).or_insert(0);
        let current_nonce = *nonce;
        *nonce += 1;

        // Compute payload hash
        let payload_hash = Hash::hash(&payload);

        // Create message
        let message_id = self.next_message_id;
        self.next_message_id += 1;

        let message = CrossChainMessage {
            id: message_id,
            source_chain,
            target_chain,
            sender,
            recipient,
            message_type,
            payload_hash,
            payload,
            source_state_root,
            inclusion_proof: None, // Set later when included in block
            created_at: current_block,
            expires_at: current_block + MESSAGE_EXPIRY,
            status: MessageStatus::Pending,
            fee,
            nonce: current_nonce,
        };

        self.messages.insert(message_id, message);

        // Add to outbox
        self.outbox
            .entry(source_chain)
            .or_insert_with(VecDeque::new)
            .push_back(message_id);

        // Increment block counter
        *block_count += 1;

        Ok(message_id)
    }

    /// Mark a message as relayed (moved from outbox to target chain)
    pub fn relay_message(
        &mut self,
        message_id: MessageId,
        inclusion_proof: MerkleProof,
    ) -> Result<(), MessagingError> {
        let message = self.messages
            .get_mut(&message_id)
            .ok_or(MessagingError::MessageNotFound)?;

        if message.status != MessageStatus::Pending && message.status != MessageStatus::InOutbox {
            return Err(MessagingError::InvalidMessageState);
        }

        message.inclusion_proof = Some(inclusion_proof);
        message.status = MessageStatus::Relayed;

        // Add to target chain's inbox
        self.inbox
            .entry(message.target_chain)
            .or_insert_with(VecDeque::new)
            .push_back(message_id);

        Ok(())
    }

    /// Verify and deliver a message on the target chain
    /// SPEC v3.1 Section 6.2: Explicit verification required
    pub fn verify_and_deliver(
        &mut self,
        message_id: MessageId,
        current_block: BlockNumber,
    ) -> Result<MessageVerification, MessagingError> {
        let message = self.messages
            .get_mut(&message_id)
            .ok_or(MessagingError::MessageNotFound)?;

        // Check not expired
        if current_block > message.expires_at {
            message.status = MessageStatus::Expired;
            return Ok(MessageVerification {
                is_valid: false,
                message_id,
                source_chain: message.source_chain,
                error: Some("Message expired".to_string()),
            });
        }

        // Check status
        if message.status != MessageStatus::Relayed {
            return Err(MessagingError::InvalidMessageState);
        }

        // Verify inclusion proof
        let proof = message.inclusion_proof.as_ref()
            .ok_or(MessagingError::NoInclusionProof)?;

        // Check if we know the source state root
        let known_root = self.known_state_roots
            .get(&(message.source_chain, proof.block_number));

        if let Some(root) = known_root {
            // Verify the proof matches the known root
            if proof.root != *root {
                message.status = MessageStatus::Failed;
                return Ok(MessageVerification {
                    is_valid: false,
                    message_id,
                    source_chain: message.source_chain,
                    error: Some("State root mismatch".to_string()),
                });
            }
        }

        // Verify the Merkle proof itself
        if !proof.verify() {
            message.status = MessageStatus::Failed;
            return Ok(MessageVerification {
                is_valid: false,
                message_id,
                source_chain: message.source_chain,
                error: Some("Invalid Merkle proof".to_string()),
            });
        }

        // Message verified!
        message.status = MessageStatus::Delivered;

        Ok(MessageVerification {
            is_valid: true,
            message_id,
            source_chain: message.source_chain,
            error: None,
        })
    }

    /// Get a message by ID
    pub fn get_message(&self, message_id: MessageId) -> Option<&CrossChainMessage> {
        self.messages.get(&message_id)
    }

    /// Get outbox for a chain
    pub fn get_outbox(&self, chain_id: ChainId) -> Vec<&CrossChainMessage> {
        self.outbox
            .get(&chain_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.messages.get(id))
                    .filter(|m| matches!(m.status, MessageStatus::Pending | MessageStatus::InOutbox))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get inbox for a chain
    pub fn get_inbox(&self, chain_id: ChainId) -> Vec<&CrossChainMessage> {
        self.inbox
            .get(&chain_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.messages.get(id))
                    .filter(|m| m.status == MessageStatus::Relayed)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get pending messages between two chains
    pub fn get_messages_between(
        &self,
        source: ChainId,
        target: ChainId,
    ) -> Vec<&CrossChainMessage> {
        self.messages
            .values()
            .filter(|m| m.source_chain == source && m.target_chain == target)
            .collect()
    }

    /// Cancel a pending message (only by sender)
    pub fn cancel_message(
        &mut self,
        message_id: MessageId,
        caller: &AccountId,
    ) -> Result<Balance, MessagingError> {
        let message = self.messages
            .get_mut(&message_id)
            .ok_or(MessagingError::MessageNotFound)?;

        if &message.sender != caller {
            return Err(MessagingError::NotSender);
        }

        if message.status != MessageStatus::Pending {
            return Err(MessagingError::CannotCancel);
        }

        message.status = MessageStatus::Cancelled;

        // Return fee
        Ok(message.fee)
    }

    /// Expire old messages
    pub fn expire_old_messages(&mut self, current_block: BlockNumber) -> Vec<MessageId> {
        let mut expired = Vec::new();

        for (id, message) in self.messages.iter_mut() {
            if current_block > message.expires_at
                && matches!(message.status, MessageStatus::Pending | MessageStatus::InOutbox | MessageStatus::Relayed)
            {
                message.status = MessageStatus::Expired;
                expired.push(*id);
            }
        }

        expired
    }

    /// Get message statistics
    pub fn get_stats(&self, chain_id: ChainId) -> MessageStats {
        let mut stats = MessageStats::default();

        for message in self.messages.values() {
            if message.source_chain == chain_id {
                stats.sent += 1;
                match message.status {
                    MessageStatus::Delivered => stats.delivered += 1,
                    MessageStatus::Failed => stats.failed += 1,
                    MessageStatus::Expired => stats.expired += 1,
                    _ => stats.pending += 1,
                }
            }
            if message.target_chain == chain_id {
                stats.received += 1;
            }
        }

        stats
    }
}

impl Default for MessagingContract {
    fn default() -> Self {
        Self::new()
    }
}

/// Message statistics for a chain
#[derive(Debug, Default, Clone)]
pub struct MessageStats {
    pub sent: u64,
    pub received: u64,
    pub pending: u64,
    pub delivered: u64,
    pub failed: u64,
    pub expired: u64,
}

/// Errors that can occur during messaging
#[derive(Debug, Clone, thiserror::Error)]
pub enum MessagingError {
    #[error("Message not found")]
    MessageNotFound,

    #[error("Payload too large (max {MAX_PAYLOAD_SIZE} bytes)")]
    PayloadTooLarge,

    #[error("Too many messages in this block")]
    TooManyMessagesInBlock,

    #[error("Insufficient fee")]
    InsufficientFee,

    #[error("Cannot send message to self")]
    CannotMessageSelf,

    #[error("Invalid message state for this operation")]
    InvalidMessageState,

    #[error("No inclusion proof provided")]
    NoInclusionProof,

    #[error("Not the message sender")]
    NotSender,

    #[error("Cannot cancel message in current state")]
    CannotCancel,

    #[error("Message verification failed: {0}")]
    VerificationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> MessagingContract {
        MessagingContract::new()
    }

    fn create_valid_merkle_proof(block_number: BlockNumber, chain_id: ChainId) -> MerkleProof {
        use crate::types::merkle::StateMerkleTree;

        // Create a tree with some leaves
        let leaves = vec![
            b"leaf0".to_vec(),
            b"leaf1".to_vec(),
            b"leaf2".to_vec(),
            b"leaf3".to_vec(),
        ];

        let tree = StateMerkleTree::new(leaves);
        tree.generate_proof(0, block_number, chain_id).unwrap()
    }

    fn create_dummy_merkle_proof(block_number: BlockNumber, chain_id: ChainId) -> MerkleProof {
        // For tests that don't need actual verification, use a simple dummy
        MerkleProof::new(
            vec![1, 2, 3],
            0,
            vec![[0; 32]],
            Hash::from_bytes([0; 32]),
            block_number,
            chain_id,
        )
    }

    #[test]
    fn test_send_message() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);
        let source = ChainId(1);
        let target = ChainId(2);

        let message_id = messaging.send_message(
            source,
            target,
            sender,
            None,
            MessageType::DataTransfer,
            vec![1, 2, 3, 4],
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE,
        ).unwrap();

        assert_eq!(message_id, 1);

        let message = messaging.get_message(message_id).unwrap();
        assert_eq!(message.source_chain, source);
        assert_eq!(message.target_chain, target);
        assert_eq!(message.status, MessageStatus::Pending);
    }

    #[test]
    fn test_cannot_message_self() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);
        let chain = ChainId(1);

        let result = messaging.send_message(
            chain,
            chain, // Same chain
            sender,
            None,
            MessageType::DataTransfer,
            vec![],
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE,
        );

        assert!(matches!(result, Err(MessagingError::CannotMessageSelf)));
    }

    #[test]
    fn test_payload_too_large() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);

        let result = messaging.send_message(
            ChainId(1),
            ChainId(2),
            sender,
            None,
            MessageType::DataTransfer,
            vec![0; MAX_PAYLOAD_SIZE + 1],
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE,
        );

        assert!(matches!(result, Err(MessagingError::PayloadTooLarge)));
    }

    #[test]
    fn test_insufficient_fee() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);

        let result = messaging.send_message(
            ChainId(1),
            ChainId(2),
            sender,
            None,
            MessageType::DataTransfer,
            vec![],
            Hash::from_bytes([0; 32]),
            1000,
            0, // No fee
        );

        assert!(matches!(result, Err(MessagingError::InsufficientFee)));
    }

    #[test]
    fn test_relay_message() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);
        let source = ChainId(1);
        let target = ChainId(2);

        let message_id = messaging.send_message(
            source,
            target,
            sender,
            None,
            MessageType::DataTransfer,
            vec![1, 2, 3],
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE,
        ).unwrap();

        let proof = create_dummy_merkle_proof(1000, source);
        messaging.relay_message(message_id, proof).unwrap();

        let message = messaging.get_message(message_id).unwrap();
        assert_eq!(message.status, MessageStatus::Relayed);
        assert!(message.inclusion_proof.is_some());
    }

    #[test]
    fn test_verify_and_deliver() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);
        let source = ChainId(1);
        let target = ChainId(2);

        let message_id = messaging.send_message(
            source,
            target,
            sender,
            None,
            MessageType::DataTransfer,
            vec![1, 2, 3],
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE,
        ).unwrap();

        // Use a valid Merkle proof
        let proof = create_valid_merkle_proof(1000, source);
        messaging.relay_message(message_id, proof).unwrap();

        let verification = messaging.verify_and_deliver(message_id, 2000).unwrap();
        assert!(verification.is_valid);

        let message = messaging.get_message(message_id).unwrap();
        assert_eq!(message.status, MessageStatus::Delivered);
    }

    #[test]
    fn test_message_expiry() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);

        let message_id = messaging.send_message(
            ChainId(1),
            ChainId(2),
            sender,
            None,
            MessageType::DataTransfer,
            vec![],
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE,
        ).unwrap();

        // Expire after MESSAGE_EXPIRY blocks
        let expired = messaging.expire_old_messages(1000 + MESSAGE_EXPIRY + 1);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], message_id);

        let message = messaging.get_message(message_id).unwrap();
        assert_eq!(message.status, MessageStatus::Expired);
    }

    #[test]
    fn test_cancel_message() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);
        let other = AccountId::from_bytes([2; 32]);

        let message_id = messaging.send_message(
            ChainId(1),
            ChainId(2),
            sender,
            None,
            MessageType::DataTransfer,
            vec![],
            Hash::from_bytes([0; 32]),
            1000,
            100,
        ).unwrap();

        // Other cannot cancel
        let result = messaging.cancel_message(message_id, &other);
        assert!(matches!(result, Err(MessagingError::NotSender)));

        // Sender can cancel
        let refund = messaging.cancel_message(message_id, &sender).unwrap();
        assert_eq!(refund, 100);

        let message = messaging.get_message(message_id).unwrap();
        assert_eq!(message.status, MessageStatus::Cancelled);
    }

    #[test]
    fn test_nonce_tracking() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);
        let source = ChainId(1);

        assert_eq!(messaging.get_nonce(source, &sender), 0);

        messaging.send_message(
            source,
            ChainId(2),
            sender,
            None,
            MessageType::DataTransfer,
            vec![],
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE,
        ).unwrap();

        assert_eq!(messaging.get_nonce(source, &sender), 1);

        messaging.send_message(
            source,
            ChainId(3),
            sender,
            None,
            MessageType::DataTransfer,
            vec![],
            Hash::from_bytes([0; 32]),
            1001,
            BASE_MESSAGE_FEE,
        ).unwrap();

        assert_eq!(messaging.get_nonce(source, &sender), 2);
    }

    #[test]
    fn test_message_types() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);

        // Asset transfer
        let _id1 = messaging.send_message(
            ChainId(1),
            ChainId(2),
            sender,
            Some(AccountId::from_bytes([2; 32])),
            MessageType::AssetTransfer { amount: 1000, asset_id: None },
            vec![],
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE,
        ).unwrap();

        // Governance notification
        let _id2 = messaging.send_message(
            ChainId(1),
            ChainId(2),
            sender,
            None,
            MessageType::GovernanceNotification {
                notification_type: GovernanceNotificationType::ExitInitiated,
            },
            vec![],
            Hash::from_bytes([0; 32]),
            1001,
            BASE_MESSAGE_FEE,
        ).unwrap();

        // State root commitment
        let _id3 = messaging.send_message(
            ChainId(1),
            ChainId(0), // To root
            sender,
            None,
            MessageType::StateRootCommitment {
                block_number: 1000,
                state_root: Hash::from_bytes([1; 32]),
            },
            vec![],
            Hash::from_bytes([0; 32]),
            1002,
            BASE_MESSAGE_FEE,
        ).unwrap();

        assert_eq!(messaging.messages.len(), 3);
    }

    #[test]
    fn test_max_messages_per_block() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);
        let source = ChainId(1);

        // Send MAX_MESSAGES_PER_BLOCK messages
        for i in 0..MAX_MESSAGES_PER_BLOCK {
            messaging.send_message(
                source,
                ChainId(2),
                sender,
                None,
                MessageType::DataTransfer,
                vec![i as u8],
                Hash::from_bytes([0; 32]),
                1000, // Same block
                BASE_MESSAGE_FEE,
            ).unwrap();
        }

        // Next message should fail
        let result = messaging.send_message(
            source,
            ChainId(2),
            sender,
            None,
            MessageType::DataTransfer,
            vec![],
            Hash::from_bytes([0; 32]),
            1000, // Same block
            BASE_MESSAGE_FEE,
        );

        assert!(matches!(result, Err(MessagingError::TooManyMessagesInBlock)));

        // But next block should work
        let result = messaging.send_message(
            source,
            ChainId(2),
            sender,
            None,
            MessageType::DataTransfer,
            vec![],
            Hash::from_bytes([0; 32]),
            1001, // Different block
            BASE_MESSAGE_FEE,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_get_outbox_inbox() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);
        let source = ChainId(1);
        let target = ChainId(2);

        // Send messages
        let id1 = messaging.send_message(
            source, target, sender, None,
            MessageType::DataTransfer, vec![], Hash::from_bytes([0; 32]),
            1000, BASE_MESSAGE_FEE,
        ).unwrap();

        let id2 = messaging.send_message(
            source, target, sender, None,
            MessageType::DataTransfer, vec![], Hash::from_bytes([0; 32]),
            1001, BASE_MESSAGE_FEE,
        ).unwrap();

        // Check outbox
        let outbox = messaging.get_outbox(source);
        assert_eq!(outbox.len(), 2);

        // Relay one
        let proof = create_dummy_merkle_proof(1000, source);
        messaging.relay_message(id1, proof).unwrap();

        // Check inbox
        let inbox = messaging.get_inbox(target);
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].id, id1);
    }

    #[test]
    fn test_stats() {
        let mut messaging = setup();
        let sender = AccountId::from_bytes([1; 32]);
        let chain1 = ChainId(1);
        let chain2 = ChainId(2);

        // Send messages from chain1
        for _ in 0..5 {
            messaging.send_message(
                chain1, chain2, sender, None,
                MessageType::DataTransfer, vec![], Hash::from_bytes([0; 32]),
                1000, BASE_MESSAGE_FEE,
            ).unwrap();
        }

        let stats = messaging.get_stats(chain1);
        assert_eq!(stats.sent, 5);
        assert_eq!(stats.pending, 5);

        let stats2 = messaging.get_stats(chain2);
        assert_eq!(stats2.received, 5);
    }
}
