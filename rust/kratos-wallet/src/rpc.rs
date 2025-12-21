// RPC client for communicating with KratOs node

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::types::{
    AccountInfo, CanVoteResponse, CandidateVotesResponse, EarlyVotingStatus,
    PendingCandidatesResponse, RpcTransactionRecord, SignedTransaction, TransactionDirection,
    TransactionHistoryResponse, TransactionRecord, TransactionStatus, TransactionSubmitResult,
};

/// JSON-RPC request
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    method: String,
    params: serde_json::Value,
    id: u64,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    #[allow(dead_code)]
    jsonrpc: String,
    result: Option<T>,
    error: Option<JsonRpcError>,
    #[allow(dead_code)]
    id: u64,
}

/// JSON-RPC error
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    #[allow(dead_code)]
    code: i32,
    message: String,
}

/// RPC client for KratOs node
pub struct RpcClient {
    url: String,
    client: Client,
    request_id: AtomicU64,
}

impl RpcClient {
    /// Create new RPC client
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            client: Client::new(),
            request_id: AtomicU64::new(1),
        }
    }

    /// Get next request ID
    fn next_id(&self) -> u64 {
        self.request_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Make a JSON-RPC call
    fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
            id: self.next_id(),
        };

        let response = self
            .client
            .post(&self.url)
            .json(&request)
            .send()
            .map_err(|e| format!("Network error: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let json_response: JsonRpcResponse<T> = response
            .json()
            .map_err(|e| format!("Parse error: {}", e))?;

        if let Some(error) = json_response.error {
            return Err(error.message);
        }

        json_response.result.ok_or_else(|| "Empty response".to_string())
    }

    /// Get account information
    pub fn get_account(&self, address: &str) -> Result<AccountInfo, String> {
        let address = if address.starts_with("0x") {
            address.to_string()
        } else {
            format!("0x{}", address)
        };

        self.call("state_getAccount", serde_json::json!([address]))
    }

    /// Get account nonce
    pub fn get_nonce(&self, address: &str) -> Result<u64, String> {
        let address = if address.starts_with("0x") {
            address.to_string()
        } else {
            format!("0x{}", address)
        };

        self.call("state_getNonce", serde_json::json!([address]))
    }

    /// Submit a signed transaction
    pub fn submit_transaction(&self, tx: &SignedTransaction) -> Result<TransactionSubmitResult, String> {
        // Convert to JSON format expected by RPC
        let tx_json = serde_json::json!({
            "transaction": {
                "sender": format!("0x{}", hex::encode(tx.transaction.sender.0)),
                "nonce": tx.transaction.nonce,
                "call": match &tx.transaction.call {
                    crate::types::TransactionCall::Transfer { to, amount } => {
                        serde_json::json!({
                            "Transfer": {
                                "to": format!("0x{}", hex::encode(to.0)),
                                "amount": amount
                            }
                        })
                    }
                    crate::types::TransactionCall::ProposeEarlyValidator { candidate } => {
                        serde_json::json!({
                            "ProposeEarlyValidator": {
                                "candidate": format!("0x{}", hex::encode(candidate.0))
                            }
                        })
                    }
                    crate::types::TransactionCall::VoteEarlyValidator { candidate } => {
                        serde_json::json!({
                            "VoteEarlyValidator": {
                                "candidate": format!("0x{}", hex::encode(candidate.0))
                            }
                        })
                    }
                    _ => return Err("Unsupported transaction type".to_string()),
                },
                "timestamp": tx.transaction.timestamp
            },
            "signature": format!("0x{}", hex::encode(tx.signature))
        });

        self.call("author_submitTransaction", serde_json::json!([tx_json]))
    }

    /// Check if node is healthy
    #[allow(dead_code)]
    pub fn health(&self) -> Result<bool, String> {
        #[derive(Deserialize)]
        struct HealthStatus {
            healthy: bool,
        }

        let status: HealthStatus = self.call("system_health", serde_json::Value::Null)?;
        Ok(status.healthy)
    }

    /// Get chain info
    #[allow(dead_code)]
    pub fn chain_info(&self) -> Result<ChainInfo, String> {
        self.call("chain_getInfo", serde_json::Value::Null)
    }

    /// Get transaction history for an address
    ///
    /// This queries the node for transaction history. If the node doesn't support
    /// this method yet, it will return an error and the wallet will fall back to
    /// local history only.
    pub fn get_transaction_history(
        &self,
        address: &str,
        limit: u32,
        offset: u32,
    ) -> Result<TransactionHistoryResponse, String> {
        let address = if address.starts_with("0x") {
            address.to_string()
        } else {
            format!("0x{}", address)
        };

        self.call(
            "state_getTransactionHistory",
            serde_json::json!([address, limit, offset]),
        )
    }

    /// Get current block height
    pub fn get_block_height(&self) -> Result<u64, String> {
        let info: ChainInfo = self.call("chain_getInfo", serde_json::Value::Null)?;
        Ok(info.height)
    }

    // =========================================================================
    // EARLY VALIDATOR RPC METHODS
    // =========================================================================

    /// Get early validator voting status
    ///
    /// Returns information about the bootstrap era and voting requirements
    pub fn get_early_voting_status(&self) -> Result<EarlyVotingStatus, String> {
        self.call("validator_getEarlyVotingStatus", serde_json::Value::Null)
    }

    /// Get pending early validator candidates
    ///
    /// Returns list of all pending candidates with their vote counts
    pub fn get_pending_candidates(&self) -> Result<PendingCandidatesResponse, String> {
        self.call("validator_getPendingCandidates", serde_json::Value::Null)
    }

    /// Get votes for a specific candidate
    ///
    /// Returns detailed voting info for a candidate
    pub fn get_candidate_votes(&self, candidate: &str) -> Result<CandidateVotesResponse, String> {
        let candidate = if candidate.starts_with("0x") {
            candidate.to_string()
        } else {
            format!("0x{}", candidate)
        };

        self.call("validator_getCandidateVotes", serde_json::json!([candidate]))
    }

    /// Check if account can vote for early validators
    ///
    /// Returns whether the account is an active validator who can vote
    pub fn can_vote(&self, account: &str) -> Result<CanVoteResponse, String> {
        let account = if account.starts_with("0x") {
            account.to_string()
        } else {
            format!("0x{}", account)
        };

        self.call("validator_canVote", serde_json::json!([account]))
    }

    /// Submit a propose early validator transaction
    pub fn submit_propose_early_validator(
        &self,
        tx: &SignedTransaction,
    ) -> Result<TransactionSubmitResult, String> {
        // Get the candidate from the transaction
        let candidate_hex = match &tx.transaction.call {
            crate::types::TransactionCall::ProposeEarlyValidator { candidate } => {
                format!("0x{}", hex::encode(candidate.0))
            }
            _ => return Err("Expected ProposeEarlyValidator transaction".to_string()),
        };

        let tx_json = serde_json::json!({
            "transaction": {
                "sender": format!("0x{}", hex::encode(tx.transaction.sender.0)),
                "nonce": tx.transaction.nonce,
                "call": {
                    "ProposeEarlyValidator": {
                        "candidate": candidate_hex
                    }
                },
                "timestamp": tx.transaction.timestamp
            },
            "signature": format!("0x{}", hex::encode(tx.signature))
        });

        self.call("author_submitTransaction", serde_json::json!([tx_json]))
    }

    /// Submit a vote early validator transaction
    pub fn submit_vote_early_validator(
        &self,
        tx: &SignedTransaction,
    ) -> Result<TransactionSubmitResult, String> {
        // Get the candidate from the transaction
        let candidate_hex = match &tx.transaction.call {
            crate::types::TransactionCall::VoteEarlyValidator { candidate } => {
                format!("0x{}", hex::encode(candidate.0))
            }
            _ => return Err("Expected VoteEarlyValidator transaction".to_string()),
        };

        let tx_json = serde_json::json!({
            "transaction": {
                "sender": format!("0x{}", hex::encode(tx.transaction.sender.0)),
                "nonce": tx.transaction.nonce,
                "call": {
                    "VoteEarlyValidator": {
                        "candidate": candidate_hex
                    }
                },
                "timestamp": tx.transaction.timestamp
            },
            "signature": format!("0x{}", hex::encode(tx.signature))
        });

        self.call("author_submitTransaction", serde_json::json!([tx_json]))
    }

    /// Convert RPC transaction records to wallet transaction records
    pub fn convert_rpc_transactions(
        &self,
        rpc_txs: Vec<RpcTransactionRecord>,
        my_address: &str,
    ) -> Vec<TransactionRecord> {
        let my_addr = if my_address.starts_with("0x") {
            my_address.to_string()
        } else {
            format!("0x{}", my_address)
        };

        rpc_txs
            .into_iter()
            .map(|tx| {
                let direction = if tx.from.eq_ignore_ascii_case(&my_addr) {
                    TransactionDirection::Sent
                } else {
                    TransactionDirection::Received
                };

                let counterparty = if direction == TransactionDirection::Sent {
                    tx.to.clone()
                } else {
                    tx.from.clone()
                };

                TransactionRecord {
                    hash: tx.hash,
                    direction,
                    status: TransactionStatus::Confirmed,
                    counterparty,
                    amount: tx.amount,
                    timestamp: tx.timestamp,
                    block_number: Some(tx.block_number),
                    nonce: tx.nonce,
                    note: None,
                }
            })
            .collect()
    }
}

/// Chain information
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ChainInfo {
    pub chain_name: String,
    pub height: u64,
    pub best_hash: String,
    pub genesis_hash: String,
    pub is_synced: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_client_creation() {
        let client = RpcClient::new("http://127.0.0.1:9933");
        assert_eq!(client.url, "http://127.0.0.1:9933");
    }

    #[test]
    fn test_request_id_increment() {
        let client = RpcClient::new("http://localhost");
        assert_eq!(client.next_id(), 1);
        assert_eq!(client.next_id(), 2);
        assert_eq!(client.next_id(), 3);
    }
}
