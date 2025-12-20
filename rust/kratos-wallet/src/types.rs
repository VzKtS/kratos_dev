// Types for wallet - Compatible with kratos-core

use serde::{Deserialize, Serialize};

/// Account information from RPC
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub free: String,
    pub reserved: String,
    pub total: String,
    pub nonce: u64,
}

/// Transaction call types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionCall {
    Transfer {
        to: [u8; 32],
        amount: u128,
    },
}

/// Unsigned transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub sender: [u8; 32],
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
        }
    }
}
