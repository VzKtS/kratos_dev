// Types for wallet - Compatible with kratos-core

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// AccountId wrapper that serializes as bytes (compatible with kratos-core AccountId)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountId32(pub [u8; 32]);

impl Serialize for AccountId32 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for AccountId32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        struct AccountId32Visitor;

        impl<'de> serde::de::Visitor<'de> for AccountId32Visitor {
            type Value = AccountId32;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("32 bytes")
            }

            fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
            where
                E: Error,
            {
                if value.len() != 32 {
                    return Err(E::custom(format!("Expected 32 bytes, got {}", value.len())));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(value);
                Ok(AccountId32(arr))
            }
        }

        deserializer.deserialize_bytes(AccountId32Visitor)
    }
}

impl From<[u8; 32]> for AccountId32 {
    fn from(bytes: [u8; 32]) -> Self {
        AccountId32(bytes)
    }
}

impl AsRef<[u8; 32]> for AccountId32 {
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Account information from RPC
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub free: String,
    pub reserved: String,
    pub total: String,
    pub nonce: u64,
}

/// Transaction call types - MUST match kratos-core order exactly for bincode compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionCall {
    /// Transfert simple
    Transfer {
        to: AccountId32,
        amount: u128,
    },
    /// Staking - Bond tokens
    Stake {
        amount: u128,
    },
    /// Unstake - Unbond tokens
    Unstake {
        amount: u128,
    },
    /// Withdraw unbonded
    WithdrawUnbonded,
    /// Enregistrement validateur
    RegisterValidator {
        stake: u128,
    },
    /// Désenregistrement validateur
    UnregisterValidator,
    /// Création de sidechain
    CreateSidechain {
        metadata: SidechainMetadata,
        deposit: u128,
    },
    /// Exit d'une sidechain
    ExitSidechain {
        chain_id: ChainId32,
    },
    /// Signal de fork
    SignalFork {
        name: String,
        description: String,
    },
    /// Propose a new early validator during bootstrap era
    ProposeEarlyValidator {
        candidate: AccountId32,
    },
    /// Vote for an early validator candidate during bootstrap era
    VoteEarlyValidator {
        candidate: AccountId32,
    },
}

/// ChainId wrapper (same as [u8; 32] but serialized as bytes)
pub type ChainId32 = AccountId32;

/// Métadonnées minimales d'une sidechain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidechainMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub parent_chain: Option<ChainId32>,
}

/// Unsigned transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub sender: AccountId32,
    pub nonce: u64,
    pub call: TransactionCall,
    pub timestamp: u64,
}

/// Signed transaction
#[derive(Debug, Clone)]
pub struct SignedTransaction {
    pub transaction: Transaction,
    pub signature: [u8; 64],
}

/// Transaction submission result
#[derive(Debug, Clone, Deserialize)]
pub struct TransactionSubmitResult {
    pub hash: String,
    pub message: String,
}

// =============================================================================
// TRANSACTION HISTORY TYPES
// =============================================================================

/// Direction of transaction relative to the wallet
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionDirection {
    /// Transaction sent from this wallet
    Sent,
    /// Transaction received by this wallet
    Received,
}

/// Status of a transaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Transaction is pending in mempool
    Pending,
    /// Transaction is confirmed in a block
    Confirmed,
    /// Transaction failed or was rejected
    Failed,
}

/// A transaction record for history display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    /// Transaction hash (hex with 0x prefix)
    pub hash: String,
    /// Direction (sent/received)
    pub direction: TransactionDirection,
    /// Status (pending/confirmed/failed)
    pub status: TransactionStatus,
    /// Counterparty address (recipient if sent, sender if received)
    pub counterparty: String,
    /// Amount in raw units (10^12 = 1 KRAT)
    pub amount: u128,
    /// Unix timestamp
    pub timestamp: u64,
    /// Block number (None if pending)
    pub block_number: Option<u64>,
    /// Nonce used for the transaction
    pub nonce: u64,
    /// Optional note/memo
    #[serde(default)]
    pub note: Option<String>,
}

impl TransactionRecord {
    /// Create a new sent transaction record (initially pending)
    pub fn new_sent(
        hash: String,
        recipient: String,
        amount: u128,
        timestamp: u64,
        nonce: u64,
    ) -> Self {
        Self {
            hash,
            direction: TransactionDirection::Sent,
            status: TransactionStatus::Pending,
            counterparty: recipient,
            amount,
            timestamp,
            block_number: None,
            nonce,
            note: None,
        }
    }

}

/// Local transaction history (stored in wallet)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionHistory {
    /// List of transaction records (newest first)
    pub transactions: Vec<TransactionRecord>,
    /// Last synced block number
    pub last_synced_block: u64,
}

impl TransactionHistory {
    /// Create empty history
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
            last_synced_block: 0,
        }
    }

    /// Add a new transaction record
    pub fn add(&mut self, record: TransactionRecord) {
        // Check if transaction already exists (by hash)
        if !self.transactions.iter().any(|tx| tx.hash == record.hash) {
            self.transactions.insert(0, record); // Insert at beginning (newest first)
        }
    }

    /// Get transactions with pagination
    pub fn get_page(&self, offset: usize, limit: usize) -> &[TransactionRecord] {
        let start = offset.min(self.transactions.len());
        let end = (offset + limit).min(self.transactions.len());
        &self.transactions[start..end]
    }

    /// Get total count
    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}

/// Response from RPC for transaction history query
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionHistoryResponse {
    /// List of transactions
    pub transactions: Vec<RpcTransactionRecord>,
}

/// Single transaction from RPC response
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransactionRecord {
    /// Transaction hash
    pub hash: String,
    /// Sender address
    pub from: String,
    /// Recipient address
    pub to: String,
    /// Amount transferred
    pub amount: u128,
    /// Transaction timestamp
    pub timestamp: u64,
    /// Block number
    pub block_number: u64,
    /// Transaction nonce
    pub nonce: u64,
}

// =============================================================================
// EARLY VALIDATOR TYPES
// =============================================================================

/// Response from validator_getEarlyVotingStatus RPC
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EarlyVotingStatus {
    /// Whether we are still in bootstrap era
    pub is_bootstrap_era: bool,
    /// Current block height
    pub current_block: u64,
    /// Block at which bootstrap era ends
    pub bootstrap_end_block: u64,
    /// Blocks remaining until bootstrap ends
    pub blocks_until_end: u64,
    /// Number of votes required for next validator
    pub votes_required: usize,
    /// Current active validator count
    pub validator_count: usize,
    /// Maximum allowed early validators
    pub max_validators: usize,
    /// Number of pending candidates
    pub pending_candidates: usize,
    /// Whether new validators can still be added
    pub can_add_validators: bool,
}

/// A pending early validator candidate
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EarlyValidatorCandidate {
    /// Candidate account address
    pub candidate: String,
    /// Who proposed this candidate
    pub proposer: String,
    /// Current vote count
    pub vote_count: usize,
    /// Votes needed for approval
    pub votes_required: usize,
    /// Whether candidate has enough votes
    pub has_quorum: bool,
    /// Block when candidacy was created
    pub created_at: u64,
    /// List of voters who have voted for this candidate
    pub voters: Vec<String>,
}

/// Response from validator_getPendingCandidates RPC
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingCandidatesResponse {
    /// List of pending candidates
    pub candidates: Vec<EarlyValidatorCandidate>,
    /// Total count
    pub count: usize,
}

/// Response from validator_getCandidateVotes RPC
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CandidateVotesResponse {
    /// Candidate address
    pub candidate: String,
    /// Proposer address
    #[serde(default)]
    pub proposer: Option<String>,
    /// Status (Pending, Approved, Rejected, Expired, not_found)
    pub status: String,
    /// Current vote count
    #[serde(default)]
    pub vote_count: Option<usize>,
    /// Votes required
    #[serde(default)]
    pub votes_required: Option<usize>,
    /// Whether has quorum
    #[serde(default)]
    pub has_quorum: Option<bool>,
    /// When created
    #[serde(default)]
    pub created_at: Option<u64>,
    /// When approved (if approved)
    #[serde(default)]
    pub approved_at: Option<u64>,
    /// List of voters
    #[serde(default)]
    pub voters: Vec<String>,
    /// Error message if not found
    #[serde(default)]
    pub error: Option<String>,
}

/// Response from validator_canVote RPC
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CanVoteResponse {
    /// Account address queried
    pub account: String,
    /// Whether the account can vote
    pub can_vote: bool,
    /// Whether the account is an active validator
    pub is_validator: bool,
    /// Whether we are in bootstrap era
    pub is_bootstrap_era: bool,
    /// Reason explaining the can_vote status
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_call_serialize() {
        let call = TransactionCall::Transfer {
            to: [1u8; 32],
            amount: 1000,
        };

        let serialized = bincode::serialize(&call).unwrap();
        let deserialized: TransactionCall = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            TransactionCall::Transfer { to, amount } => {
                assert_eq!(to, [1u8; 32]);
                assert_eq!(amount, 1000);
            }
            _ => panic!("Expected Transfer variant"),
        }
    }
}
