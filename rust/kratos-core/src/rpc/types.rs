// Types RPC - Structures for JSON-RPC 2.0 requests and responses
use crate::types::*;
use serde::{Deserialize, Serialize};

// =============================================================================
// JSON-RPC 2.0 PROTOCOL TYPES
// =============================================================================

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (must be "2.0")
    pub jsonrpc: String,

    /// Method name (e.g., "chain_getBlock")
    pub method: String,

    /// Method parameters
    #[serde(default)]
    pub params: serde_json::Value,

    /// Request ID
    pub id: JsonRpcId,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version
    pub jsonrpc: String,

    /// Result (present on success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,

    /// Error (present on failure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,

    /// Request ID
    pub id: JsonRpcId,
}

impl JsonRpcResponse {
    pub fn success<T: Serialize>(id: JsonRpcId, result: T) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(serde_json::to_value(result).unwrap_or(serde_json::Value::Null)),
            error: None,
            id,
        }
    }

    pub fn error(id: JsonRpcId, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }
}

/// JSON-RPC Request ID (can be string, number, or null)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(i64),
    String(String),
    Null,
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Optional additional data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// Parse error (-32700)
    pub fn parse_error(message: &str) -> Self {
        Self {
            code: -32700,
            message: format!("Parse error: {}", message),
            data: None,
        }
    }

    /// Invalid request (-32600)
    pub fn invalid_request(message: &str) -> Self {
        Self {
            code: -32600,
            message: format!("Invalid request: {}", message),
            data: None,
        }
    }

    /// Method not found (-32601)
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {}", method),
            data: None,
        }
    }

    /// Invalid params (-32602)
    pub fn invalid_params(message: &str) -> Self {
        Self {
            code: -32602,
            message: format!("Invalid params: {}", message),
            data: None,
        }
    }

    /// Internal error (-32603)
    pub fn internal_error(message: &str) -> Self {
        Self {
            code: -32603,
            message: format!("Internal error: {}", message),
            data: None,
        }
    }

    /// Block not found (-32001)
    pub fn block_not_found() -> Self {
        Self {
            code: -32001,
            message: "Block not found".to_string(),
            data: None,
        }
    }

    /// Transaction not found (-32002)
    pub fn transaction_not_found() -> Self {
        Self {
            code: -32002,
            message: "Transaction not found".to_string(),
            data: None,
        }
    }

    /// Account not found (-32003)
    pub fn account_not_found() -> Self {
        Self {
            code: -32003,
            message: "Account not found".to_string(),
            data: None,
        }
    }

    /// Transaction rejected (-32010)
    pub fn transaction_rejected(reason: &str) -> Self {
        Self {
            code: -32010,
            message: format!("Transaction rejected: {}", reason),
            data: None,
        }
    }

    /// SECURITY FIX #29: Rate limited (-32029)
    pub fn rate_limited(retry_after_seconds: u64) -> Self {
        Self {
            code: -32029,
            message: format!("Rate limit exceeded. Retry after {} seconds", retry_after_seconds),
            data: Some(serde_json::json!({ "retryAfter": retry_after_seconds })),
        }
    }
}

// =============================================================================
// CHAIN INFO TYPES
// =============================================================================

/// Chain information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainInfo {
    /// Chain name
    pub chain_name: String,
    /// Current height
    pub height: BlockNumber,
    /// Best block hash
    pub best_hash: String,
    /// Genesis hash
    pub genesis_hash: String,
    /// Current epoch
    pub current_epoch: EpochNumber,
    /// Current slot
    pub current_slot: SlotNumber,
    /// Is synced
    pub is_synced: bool,
    /// Sync gap (blocks behind)
    pub sync_gap: u64,
}

/// Block information (RPC format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    /// Block number
    pub number: BlockNumber,
    /// Block hash
    pub hash: String,
    /// Parent hash
    pub parent_hash: String,
    /// Timestamp
    pub timestamp: Timestamp,
    /// Block author
    pub author: String,
    /// Epoch
    pub epoch: EpochNumber,
    /// Slot
    pub slot: SlotNumber,
    /// Transaction count
    pub tx_count: usize,
    /// State root
    pub state_root: String,
    /// Transactions root
    pub transactions_root: String,
}

impl From<&Block> for BlockInfo {
    fn from(block: &Block) -> Self {
        Self {
            number: block.header.number,
            hash: format!("0x{}", hex::encode(block.hash().as_bytes())),
            parent_hash: format!("0x{}", hex::encode(block.header.parent_hash.as_bytes())),
            timestamp: block.header.timestamp,
            author: format!("0x{}", hex::encode(block.header.author.as_bytes())),
            epoch: block.header.epoch,
            slot: block.header.slot,
            tx_count: block.body.transactions.len(),
            state_root: format!("0x{}", hex::encode(block.header.state_root.as_bytes())),
            transactions_root: format!("0x{}", hex::encode(block.header.transactions_root.as_bytes())),
        }
    }
}

/// Block with transactions (full block info)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockWithTransactions {
    /// Block header info
    #[serde(flatten)]
    pub header: BlockInfo,
    /// Transactions
    pub transactions: Vec<TransactionInfo>,
}

impl From<&Block> for BlockWithTransactions {
    fn from(block: &Block) -> Self {
        Self {
            header: BlockInfo::from(block),
            transactions: block
                .body
                .transactions
                .iter()
                .map(TransactionInfo::from)
                .collect(),
        }
    }
}

// =============================================================================
// TRANSACTION TYPES
// =============================================================================

/// Transaction information (RPC format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionInfo {
    /// Transaction hash
    pub hash: String,
    /// Sender
    pub from: String,
    /// Nonce
    pub nonce: Nonce,
    /// Transaction type
    pub tx_type: String,
    /// Transaction details
    pub details: TransactionDetails,
    /// Timestamp
    pub timestamp: Timestamp,
    /// Fee
    pub fee: Balance,
}

impl From<&SignedTransaction> for TransactionInfo {
    fn from(tx: &SignedTransaction) -> Self {
        let (tx_type, details) = match &tx.transaction.call {
            TransactionCall::Transfer { to, amount } => (
                "transfer".to_string(),
                TransactionDetails::Transfer {
                    to: format!("0x{}", hex::encode(to.as_bytes())),
                    amount: *amount,
                },
            ),
            TransactionCall::Stake { amount } => (
                "stake".to_string(),
                TransactionDetails::Stake { amount: *amount },
            ),
            TransactionCall::Unstake { amount } => (
                "unstake".to_string(),
                TransactionDetails::Unstake { amount: *amount },
            ),
            TransactionCall::WithdrawUnbonded => (
                "withdrawUnbonded".to_string(),
                TransactionDetails::WithdrawUnbonded,
            ),
            // Handle other transaction types generically
            _ => (
                "other".to_string(),
                TransactionDetails::Other {
                    description: format!("{:?}", tx.transaction.call),
                },
            ),
        };

        Self {
            hash: tx
                .hash
                .map(|h| format!("0x{}", hex::encode(h.as_bytes())))
                .unwrap_or_else(|| "0x".to_string()),
            from: format!("0x{}", hex::encode(tx.transaction.sender.as_bytes())),
            nonce: tx.transaction.nonce,
            tx_type,
            details,
            timestamp: tx.transaction.timestamp,
            fee: tx.transaction.call.base_fee(),
        }
    }
}

/// Transaction details
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TransactionDetails {
    Transfer { to: String, amount: Balance },
    Stake { amount: Balance },
    Unstake { amount: Balance },
    WithdrawUnbonded,
    Other { description: String },
}

/// Transaction receipt (result of execution)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionReceipt {
    /// Transaction hash
    pub tx_hash: String,
    /// Block number (if included)
    pub block_number: Option<BlockNumber>,
    /// Block hash (if included)
    pub block_hash: Option<String>,
    /// Success status
    pub success: bool,
    /// Fee paid
    pub fee_paid: Balance,
    /// Error message (if failed)
    pub error: Option<String>,
}

/// Transaction submit result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSubmitResult {
    /// Transaction hash
    pub hash: String,
    /// Message
    pub message: String,
}

// =============================================================================
// ACCOUNT TYPES
// =============================================================================

use crate::types::primitives::KRAT;

/// Account information (RPC format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfoRpc {
    /// Account address
    pub address: String,
    /// Free balance in KRAT (human-readable)
    pub free: String,
    /// Reserved balance in KRAT (human-readable)
    pub reserved: String,
    /// Total balance in KRAT (human-readable)
    pub total: String,
    /// Free balance in base units (for precision)
    pub free_raw: Balance,
    /// Reserved balance in base units (for precision)
    pub reserved_raw: Balance,
    /// Total balance in base units (for precision)
    pub total_raw: Balance,
    /// Nonce
    pub nonce: Nonce,
}

impl AccountInfoRpc {
    pub fn from_info(address: &AccountId, info: &AccountInfo) -> Self {
        let total_raw = info.free + info.reserved;
        Self {
            address: format!("0x{}", hex::encode(address.as_bytes())),
            free: format_krat(info.free),
            reserved: format_krat(info.reserved),
            total: format_krat(total_raw),
            free_raw: info.free,
            reserved_raw: info.reserved,
            total_raw,
            nonce: info.nonce,
        }
    }

    pub fn empty(address: &AccountId) -> Self {
        Self {
            address: format!("0x{}", hex::encode(address.as_bytes())),
            free: "0 KRAT".to_string(),
            reserved: "0 KRAT".to_string(),
            total: "0 KRAT".to_string(),
            free_raw: 0,
            reserved_raw: 0,
            total_raw: 0,
            nonce: 0,
        }
    }

    /// Create from address string and balance only (no full AccountInfo available)
    pub fn from_balance(address: String, balance: Balance) -> Self {
        Self {
            address,
            free: format_krat(balance),
            reserved: "0 KRAT".to_string(),
            total: format_krat(balance),
            free_raw: balance,
            reserved_raw: 0,
            total_raw: balance,
            nonce: 0,
        }
    }
}

/// Format balance in KRAT with proper decimal places
fn format_krat(amount: Balance) -> String {
    let whole = amount / KRAT;
    let frac = amount % KRAT;

    if frac == 0 {
        format!("{} KRAT", whole)
    } else {
        // Show up to 6 decimal places, trimming trailing zeros
        let frac_str = format!("{:012}", frac);
        let trimmed = frac_str.trim_end_matches('0');
        let decimals = if trimmed.len() > 6 { &trimmed[..6] } else { trimmed };
        format!("{}.{} KRAT", whole, decimals)
    }
}

// =============================================================================
// NETWORK/SYSTEM TYPES
// =============================================================================

/// Peer information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    /// Peer ID
    pub peer_id: String,
    /// Addresses
    pub addresses: Vec<String>,
    /// Best block number
    pub best_block: BlockNumber,
    /// Reputation score
    pub score: i32,
    /// Is bootstrap node
    pub is_bootstrap: bool,
}

/// Network status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStatus {
    /// Local peer ID
    pub local_peer_id: String,
    /// Listening addresses
    pub listening_addresses: Vec<String>,
    /// Connected peers count
    pub peer_count: usize,
    /// Network best height
    pub network_best_height: BlockNumber,
    /// Average peer score
    pub average_peer_score: i32,
}

/// System information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    /// Node name
    pub name: String,
    /// Version
    pub version: String,
    /// Chain info
    pub chain: ChainInfo,
    /// Network status
    pub network: NetworkStatus,
    /// Pending transactions
    pub pending_txs: usize,
}

/// Sync status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// Is syncing
    pub syncing: bool,
    /// Current block
    pub current_block: BlockNumber,
    /// Highest block (from peers)
    pub highest_block: BlockNumber,
    /// Blocks behind
    pub blocks_behind: u64,
    /// Sync state
    pub state: String,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatus {
    /// Is healthy
    pub healthy: bool,
    /// Is synced
    pub is_synced: bool,
    /// Has peers
    pub has_peers: bool,
    /// Block height
    pub block_height: BlockNumber,
    /// Peer count
    pub peer_count: usize,
}

// =============================================================================
// MEMPOOL TYPES
// =============================================================================

/// Mempool status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MempoolStatus {
    /// Pending transaction count
    pub pending_count: usize,
    /// Total fees
    pub total_fees: Balance,
    /// Stats
    pub stats: MempoolStats,
}

/// Mempool statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MempoolStats {
    /// Total added
    pub total_added: u64,
    /// Total removed
    pub total_removed: u64,
    /// Total evicted
    pub total_evicted: u64,
    /// Total rejected
    pub total_rejected: u64,
    /// Total replaced (RBF)
    pub total_replaced: u64,
}

// =============================================================================
// VALIDATOR TYPES
// =============================================================================

/// Validator information (RPC format)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorInfoRpc {
    /// Validator address
    pub address: String,
    /// Staked amount
    pub stake: Balance,
    /// Validator credits
    pub validator_credits: u64,
    /// Blocks produced
    pub blocks_produced: u64,
    /// Blocks missed
    pub blocks_missed: u32,
    /// Is active
    pub is_active: bool,
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Parse hex string to AccountId
pub fn parse_account_id(s: &str) -> Result<AccountId, String> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).map_err(|e| format!("Invalid hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!("Expected 32 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(AccountId::from_bytes(arr))
}

/// Parse hex string to Hash
pub fn parse_hash(s: &str) -> Result<Hash, String> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(s).map_err(|e| format!("Invalid hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!("Expected 32 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(Hash::from_bytes(arr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_response_success() {
        let response = JsonRpcResponse::success(JsonRpcId::Number(1), "test");
        assert!(response.result.is_some());
        assert!(response.error.is_none());
        assert_eq!(response.id, JsonRpcId::Number(1));
    }

    #[test]
    fn test_json_rpc_response_error() {
        let error = JsonRpcError::method_not_found("test_method");
        let response = JsonRpcResponse::error(JsonRpcId::Number(1), error);
        assert!(response.result.is_none());
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32601);
    }

    #[test]
    fn test_block_info_from_block() {
        let block = Block {
            header: BlockHeader {
                number: 1,
                parent_hash: Hash::ZERO,
                transactions_root: Hash::ZERO,
                state_root: Hash::ZERO,
                timestamp: 12345,
                epoch: 0,
                slot: 1,
                author: AccountId::from_bytes([1; 32]),
                signature: Signature64([0; 64]),
            },
            body: BlockBody {
                transactions: vec![],
            },
        };

        let info = BlockInfo::from(&block);
        assert_eq!(info.number, 1);
        assert_eq!(info.timestamp, 12345);
        assert_eq!(info.tx_count, 0);
        assert!(info.hash.starts_with("0x"));
    }

    #[test]
    fn test_parse_account_id() {
        let hex = "0x0101010101010101010101010101010101010101010101010101010101010101";
        let account = parse_account_id(hex).unwrap();
        assert_eq!(account.as_bytes(), &[1u8; 32]);
    }

    #[test]
    fn test_parse_hash() {
        let hex = "0x0000000000000000000000000000000000000000000000000000000000000000";
        let hash = parse_hash(hex).unwrap();
        assert_eq!(hash, Hash::ZERO);
    }

    #[test]
    fn test_json_rpc_request_deserialize() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "chain_getBlock",
            "params": [1],
            "id": 1
        }"#;
        let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.method, "chain_getBlock");
        assert_eq!(request.id, JsonRpcId::Number(1));
    }

    #[test]
    fn test_account_info_rpc() {
        let account = AccountId::from_bytes([1; 32]);
        let mut info = AccountInfo::new();
        info.free = 1000 * KRAT;
        info.reserved = 500 * KRAT;
        info.nonce = 5;

        let rpc_info = AccountInfoRpc::from_info(&account, &info);
        assert_eq!(rpc_info.free, "1000 KRAT");
        assert_eq!(rpc_info.reserved, "500 KRAT");
        assert_eq!(rpc_info.total, "1500 KRAT");
        assert_eq!(rpc_info.free_raw, 1000 * KRAT);
        assert_eq!(rpc_info.reserved_raw, 500 * KRAT);
        assert_eq!(rpc_info.total_raw, 1500 * KRAT);
        assert_eq!(rpc_info.nonce, 5);
    }

    #[test]
    fn test_format_krat() {
        assert_eq!(format_krat(0), "0 KRAT");
        assert_eq!(format_krat(1 * KRAT), "1 KRAT");
        assert_eq!(format_krat(1000 * KRAT), "1000 KRAT");
        assert_eq!(format_krat(KRAT + KRAT / 2), "1.5 KRAT");
        assert_eq!(format_krat(KRAT / 10), "0.1 KRAT");
    }
}
