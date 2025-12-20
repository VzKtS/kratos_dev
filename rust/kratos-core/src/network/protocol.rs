// Protocol - Messages et topics pour le réseau KratOs
use crate::types::{Block, SignedTransaction, Hash};
use serde::{Deserialize, Serialize};
use std::hash::Hash as StdHash;

// =============================================================================
// SECURITY FIX #17: Maximum message size for deserialization
// =============================================================================

/// Maximum allowed message size for network deserialization
/// SECURITY FIX #17: Prevents memory exhaustion from malicious large messages
pub const MAX_NETWORK_MESSAGE_SIZE: usize = 2 * 1024 * 1024; // 2 MB

/// Error type for protocol operations
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("Message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },

    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    #[error("Serialization failed: {0}")]
    SerializationFailed(String),
}

/// Topics de gossip
#[derive(Debug, Clone, Copy, PartialEq, Eq, StdHash)]
pub enum GossipTopic {
    /// Nouveaux blocs
    Blocks,
    /// Nouvelles transactions
    Transactions,
    /// Consensus messages
    Consensus,
}

impl GossipTopic {
    pub fn as_str(&self) -> &'static str {
        match self {
            GossipTopic::Blocks => "/kratos/blocks/1.0.0",
            GossipTopic::Transactions => "/kratos/transactions/1.0.0",
            GossipTopic::Consensus => "/kratos/consensus/1.0.0",
        }
    }
}

/// Messages réseau
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Nouveau bloc
    NewBlock(Block),

    /// Nouvelle transaction
    NewTransaction(SignedTransaction),

    /// Requête de bloc par hash
    BlockRequest(Hash),

    /// Réponse avec bloc
    BlockResponse(Option<Block>),

    /// Requête de sync depuis un numéro de bloc
    SyncRequest {
        from_block: u64,
        max_blocks: u32,
    },

    /// Réponse de sync avec liste de blocs
    SyncResponse {
        blocks: Vec<Block>,
        has_more: bool,
    },

    /// Message de consensus (attestation, etc.)
    ConsensusMessage {
        data: Vec<u8>,
    },

    // =========================================================================
    // GENESIS EXCHANGE PROTOCOL
    // Used by joining nodes to receive genesis hash before initialization
    // =========================================================================

    /// Request genesis info from peer (sent by joining node)
    GenesisRequest,

    /// Response with genesis info (sent by genesis/existing node)
    GenesisResponse {
        /// Genesis block hash - the canonical chain identifier
        genesis_hash: Hash,
        /// Genesis block for full validation
        genesis_block: Block,
        /// Chain name for verification
        chain_name: String,
    },
}

impl NetworkMessage {
    /// Encode le message en bytes
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        bincode::serialize(self)
            .map_err(|e| ProtocolError::SerializationFailed(e.to_string()))
    }

    /// Décode le message depuis bytes
    /// SECURITY FIX #17: Added size limit check before deserialization
    /// to prevent memory exhaustion attacks from malicious peers
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        // SECURITY FIX #17: Check size before deserializing
        if bytes.len() > MAX_NETWORK_MESSAGE_SIZE {
            return Err(ProtocolError::MessageTooLarge {
                size: bytes.len(),
                max: MAX_NETWORK_MESSAGE_SIZE,
            });
        }

        bincode::deserialize(bytes)
            .map_err(|e| ProtocolError::DeserializationFailed(e.to_string()))
    }

    /// Decode with custom size limit for specific contexts
    /// SECURITY FIX #17: Allows callers to specify stricter limits
    pub fn decode_with_limit(bytes: &[u8], max_size: usize) -> Result<Self, ProtocolError> {
        if bytes.len() > max_size {
            return Err(ProtocolError::MessageTooLarge {
                size: bytes.len(),
                max: max_size,
            });
        }

        bincode::deserialize(bytes)
            .map_err(|e| ProtocolError::DeserializationFailed(e.to_string()))
    }

    /// Retourne le topic pour ce message
    pub fn topic(&self) -> GossipTopic {
        match self {
            NetworkMessage::NewBlock(_) => GossipTopic::Blocks,
            NetworkMessage::NewTransaction(_) => GossipTopic::Transactions,
            NetworkMessage::ConsensusMessage { .. } => GossipTopic::Consensus,
            _ => GossipTopic::Blocks, // Par défaut
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn test_message_encode_decode() {
        let tx = SignedTransaction {
            transaction: Transaction {
                sender: AccountId::from_bytes([1; 32]),
                nonce: 0,
                call: TransactionCall::Transfer {
                    to: AccountId::from_bytes([2; 32]),
                    amount: 1000,
                },
                timestamp: 0,
            },
            signature: Signature64([0; 64]),
            hash: Some(Hash::from_bytes([0; 32])),
        };

        let msg = NetworkMessage::NewTransaction(tx.clone());
        let encoded = msg.encode().unwrap();
        let decoded = NetworkMessage::decode(&encoded).unwrap();

        match decoded {
            NetworkMessage::NewTransaction(decoded_tx) => {
                assert_eq!(decoded_tx.transaction.sender, tx.transaction.sender);
                assert_eq!(decoded_tx.transaction.nonce, tx.transaction.nonce);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_topic_strings() {
        assert_eq!(GossipTopic::Blocks.as_str(), "/kratos/blocks/1.0.0");
        assert_eq!(GossipTopic::Transactions.as_str(), "/kratos/transactions/1.0.0");
        assert_eq!(GossipTopic::Consensus.as_str(), "/kratos/consensus/1.0.0");
    }
}
