// Methods RPC - JSON-RPC method implementations
use crate::node::service::KratOsNode;
use crate::rpc::types::*;
use crate::types::*;
use std::sync::Arc;

// =============================================================================
// RPC METHODS
// =============================================================================

/// RPC method handler
pub struct RpcMethods {
    /// Node reference
    node: Arc<KratOsNode>,
}

impl RpcMethods {
    /// Create new RPC methods handler
    pub fn new(node: Arc<KratOsNode>) -> Self {
        Self { node }
    }

    /// Handle a JSON-RPC request
    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        // Validate JSON-RPC version
        if request.jsonrpc != "2.0" {
            return JsonRpcResponse::error(
                request.id,
                JsonRpcError::invalid_request("Invalid JSON-RPC version"),
            );
        }

        // Route to appropriate method
        match request.method.as_str() {
            // Chain methods
            "chain_getInfo" => self.chain_get_info(request.id).await,
            "chain_getBlock" => self.chain_get_block(request.id, request.params).await,
            "chain_getBlockByHash" => self.chain_get_block_by_hash(request.id, request.params).await,
            "chain_getBlockByNumber" => self.chain_get_block_by_number(request.id, request.params).await,
            "chain_getLatestBlock" => self.chain_get_latest_block(request.id).await,
            "chain_getHeader" => self.chain_get_header(request.id, request.params).await,

            // State methods
            "state_getAccount" => self.state_get_account(request.id, request.params).await,
            "state_getBalance" => self.state_get_balance(request.id, request.params).await,
            "state_getNonce" => self.state_get_nonce(request.id, request.params).await,

            // Author methods (transaction submission)
            "author_submitTransaction" => self.author_submit_transaction(request.id, request.params).await,
            "author_pendingTransactions" => self.author_pending_transactions(request.id).await,
            "author_removeTransaction" => self.author_remove_transaction(request.id, request.params).await,

            // System methods
            "system_info" => self.system_info(request.id).await,
            "system_health" => self.system_health(request.id).await,
            "system_peers" => self.system_peers(request.id).await,
            "system_syncState" => self.system_sync_state(request.id).await,
            "system_version" => self.system_version(request.id).await,
            "system_name" => self.system_name(request.id).await,

            // Mempool methods
            "mempool_status" => self.mempool_status(request.id).await,
            "mempool_content" => self.mempool_content(request.id).await,

            // Clock health methods (SPEC v6.1)
            "clock_getHealth" => self.clock_get_health(request.id).await,
            "clock_getValidatorRecord" => self.clock_get_validator_record(request.id, request.params).await,

            // Early Validator Voting methods (Bootstrap Era)
            "validator_getEarlyVotingStatus" => self.validator_get_early_voting_status(request.id).await,
            "validator_getPendingCandidates" => self.validator_get_pending_candidates(request.id).await,
            "validator_getCandidateVotes" => self.validator_get_candidate_votes(request.id, request.params).await,
            "validator_canVote" => self.validator_can_vote(request.id, request.params).await,

            // Unknown method
            _ => JsonRpcResponse::error(request.id, JsonRpcError::method_not_found(&request.method)),
        }
    }

    // =========================================================================
    // CHAIN METHODS
    // =========================================================================

    /// Get chain information
    async fn chain_get_info(&self, id: JsonRpcId) -> JsonRpcResponse {
        let height = self.node.chain_height().await;
        let sync_gap = self.node.sync_gap().await;
        let is_synced = self.node.is_synced().await;
        let genesis_hash = self.node.genesis_hash();

        match self.node.current_block().await {
            Some(block) => {
                let info = ChainInfo {
                    chain_name: "KratOs".to_string(),
                    height,
                    best_hash: format!("0x{}", hex::encode(block.hash().as_bytes())),
                    genesis_hash: format!("0x{}", hex::encode(genesis_hash.as_bytes())),
                    current_epoch: block.header.epoch,
                    current_slot: block.header.slot,
                    is_synced,
                    sync_gap,
                };
                JsonRpcResponse::success(id, info)
            }
            None => JsonRpcResponse::error(id, JsonRpcError::internal_error("No current block")),
        }
    }

    /// Get block by number or "latest"
    async fn chain_get_block(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        // Parse params: can be number, "latest", or hash
        let block_id: String = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match &arr[0] {
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::String(s) => s.clone(),
                    _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected block number or 'latest'")),
                }
            }
            serde_json::Value::String(s) => s,
            serde_json::Value::Number(n) => n.to_string(),
            _ => "latest".to_string(),
        };

        if block_id == "latest" {
            return self.chain_get_latest_block(id).await;
        }

        // Try parse as number
        if let Ok(number) = block_id.parse::<u64>() {
            return self.chain_get_block_by_number(id, serde_json::json!([number])).await;
        }

        // Try parse as hash
        if block_id.starts_with("0x") {
            return self.chain_get_block_by_hash(id, serde_json::json!([block_id])).await;
        }

        JsonRpcResponse::error(id, JsonRpcError::invalid_params("Invalid block identifier"))
    }

    /// Get block by hash
    async fn chain_get_block_by_hash(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        let hash_str: String = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match arr[0].as_str() {
                    Some(s) => s.to_string(),
                    None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected hash string")),
                }
            }
            _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [hash]")),
        };

        let hash = match parse_hash(&hash_str) {
            Ok(h) => h,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        // Check current block first
        if let Some(block) = self.node.current_block().await {
            if block.hash() == hash {
                return JsonRpcResponse::success(id, BlockWithTransactions::from(&block));
            }
        }

        // FIX: Query storage for historical blocks
        match self.node.get_block_by_hash(&hash).await {
            Ok(Some(block)) => JsonRpcResponse::success(id, BlockWithTransactions::from(&block)),
            Ok(None) => JsonRpcResponse::error(id, JsonRpcError::block_not_found()),
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&format!("{:?}", e))),
        }
    }

    /// Get block by number
    async fn chain_get_block_by_number(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        let number: u64 = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match arr[0].as_u64() {
                    Some(n) => n,
                    None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected block number")),
                }
            }
            _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [number]")),
        };

        // Check current block first
        if let Some(block) = self.node.current_block().await {
            if block.header.number == number {
                return JsonRpcResponse::success(id, BlockWithTransactions::from(&block));
            }
        }

        // FIX: Query storage for historical blocks
        match self.node.get_block_by_number(number).await {
            Ok(Some(block)) => JsonRpcResponse::success(id, BlockWithTransactions::from(&block)),
            Ok(None) => JsonRpcResponse::error(id, JsonRpcError::block_not_found()),
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&format!("{:?}", e))),
        }
    }

    /// Get latest block
    async fn chain_get_latest_block(&self, id: JsonRpcId) -> JsonRpcResponse {
        match self.node.current_block().await {
            Some(block) => JsonRpcResponse::success(id, BlockWithTransactions::from(&block)),
            None => JsonRpcResponse::error(id, JsonRpcError::block_not_found()),
        }
    }

    /// Get block header by number
    async fn chain_get_header(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        let number: Option<u64> = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => arr[0].as_u64(),
            _ => None,
        };

        match self.node.current_block().await {
            Some(block) => {
                if number.is_none() || number == Some(block.header.number) {
                    JsonRpcResponse::success(id, BlockInfo::from(&block))
                } else {
                    JsonRpcResponse::error(id, JsonRpcError::block_not_found())
                }
            }
            None => JsonRpcResponse::error(id, JsonRpcError::block_not_found()),
        }
    }

    // =========================================================================
    // STATE METHODS
    // =========================================================================

    /// Get account information
    async fn state_get_account(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        let address_str: String = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match arr[0].as_str() {
                    Some(s) => s.to_string(),
                    None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected address string")),
                }
            }
            _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [address]")),
        };

        let account_id = match parse_account_id(&address_str) {
            Ok(a) => a,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        // Get account info - returns empty account if not found
        match self.node.get_balance(&account_id).await {
            Ok(balance) => {
                // For now, we only have balance. In the future, get full account info
                let info = AccountInfoRpc::from_balance(address_str, balance);
                JsonRpcResponse::success(id, info)
            }
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&format!("{:?}", e))),
        }
    }

    /// Get account balance
    async fn state_get_balance(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        let address_str: String = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match arr[0].as_str() {
                    Some(s) => s.to_string(),
                    None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected address string")),
                }
            }
            _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [address]")),
        };

        let account_id = match parse_account_id(&address_str) {
            Ok(a) => a,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        match self.node.get_balance(&account_id).await {
            Ok(balance) => JsonRpcResponse::success(id, balance),
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&format!("{:?}", e))),
        }
    }

    /// Get account nonce
    async fn state_get_nonce(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        let address_str: String = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match arr[0].as_str() {
                    Some(s) => s.to_string(),
                    None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected address string")),
                }
            }
            _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [address]")),
        };

        let account_id = match parse_account_id(&address_str) {
            Ok(a) => a,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        // FIX: Get nonce from storage instead of returning 0
        match self.node.get_nonce(&account_id).await {
            Ok(nonce) => JsonRpcResponse::success(id, nonce),
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&format!("{:?}", e))),
        }
    }

    // =========================================================================
    // AUTHOR METHODS (Transaction Submission)
    // =========================================================================

    /// Submit a transaction
    async fn author_submit_transaction(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        // Parse signed transaction from params
        let tx: SignedTransaction = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match serde_json::from_value(arr[0].clone()) {
                    Ok(tx) => tx,
                    Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&format!("Invalid transaction: {}", e))),
                }
            }
            serde_json::Value::Object(_) => {
                match serde_json::from_value(params) {
                    Ok(tx) => tx,
                    Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&format!("Invalid transaction: {}", e))),
                }
            }
            _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected transaction object")),
        };

        // Submit to node
        match self.node.submit_transaction(tx).await {
            Ok(hash) => {
                let result = TransactionSubmitResult {
                    hash: format!("0x{}", hex::encode(hash.as_bytes())),
                    message: "Transaction submitted successfully".to_string(),
                };
                JsonRpcResponse::success(id, result)
            }
            Err(e) => JsonRpcResponse::error(id, JsonRpcError::transaction_rejected(&format!("{:?}", e))),
        }
    }

    /// Get pending transaction count
    async fn author_pending_transactions(&self, id: JsonRpcId) -> JsonRpcResponse {
        let count = self.node.mempool_size().await;
        JsonRpcResponse::success(id, count)
    }

    /// Remove a transaction from mempool (by hash)
    async fn author_remove_transaction(&self, id: JsonRpcId, _params: serde_json::Value) -> JsonRpcResponse {
        // TODO: Implement transaction removal
        JsonRpcResponse::error(id, JsonRpcError::internal_error("Not implemented"))
    }

    // =========================================================================
    // SYSTEM METHODS
    // =========================================================================

    /// Get full system info
    async fn system_info(&self, id: JsonRpcId) -> JsonRpcResponse {
        let height = self.node.chain_height().await;
        let sync_gap = self.node.sync_gap().await;
        let is_synced = self.node.is_synced().await;
        let genesis_hash = self.node.genesis_hash();
        let peer_count = self.node.peer_count().await;
        let network_stats = self.node.network_stats().await;
        let local_peer_id = self.node.local_peer_id().await;
        let pending_txs = self.node.mempool_size().await;

        let block = self.node.current_block().await;
        let (best_hash, epoch, slot) = match &block {
            Some(b) => (
                format!("0x{}", hex::encode(b.hash().as_bytes())),
                b.header.epoch,
                b.header.slot,
            ),
            None => ("0x".to_string(), 0, 0),
        };

        let info = SystemInfo {
            name: "KratOs Node".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            chain: ChainInfo {
                chain_name: "KratOs".to_string(),
                height,
                best_hash,
                genesis_hash: format!("0x{}", hex::encode(genesis_hash.as_bytes())),
                current_epoch: epoch,
                current_slot: slot,
                is_synced,
                sync_gap,
            },
            network: NetworkStatus {
                local_peer_id: local_peer_id.to_string(),
                listening_addresses: vec![], // TODO: Get from network
                peer_count,
                network_best_height: network_stats.best_height,
                average_peer_score: network_stats.average_score,
            },
            pending_txs,
        };

        JsonRpcResponse::success(id, info)
    }

    /// Health check
    async fn system_health(&self, id: JsonRpcId) -> JsonRpcResponse {
        let height = self.node.chain_height().await;
        let peer_count = self.node.peer_count().await;
        let is_synced = self.node.is_synced().await;

        let health = HealthStatus {
            healthy: true, // Basic health check
            is_synced,
            has_peers: peer_count > 0,
            block_height: height,
            peer_count,
        };

        JsonRpcResponse::success(id, health)
    }

    /// Get connected peers
    async fn system_peers(&self, id: JsonRpcId) -> JsonRpcResponse {
        let peers = self.node.connected_peers().await;
        let peer_count = peers.len();

        // Return just count and IDs for now
        let peer_ids: Vec<String> = peers.iter().map(|p| p.to_string()).collect();

        JsonRpcResponse::success(id, serde_json::json!({
            "count": peer_count,
            "peers": peer_ids
        }))
    }

    /// Get sync state
    async fn system_sync_state(&self, id: JsonRpcId) -> JsonRpcResponse {
        let height = self.node.chain_height().await;
        let sync_gap = self.node.sync_gap().await;
        let network_stats = self.node.network_stats().await;

        let state_str = if sync_gap == 0 {
            "synced"
        } else if sync_gap > 1000 {
            "far_behind"
        } else {
            "syncing"
        };

        let status = SyncStatus {
            syncing: sync_gap > 0,
            current_block: height,
            highest_block: network_stats.best_height,
            blocks_behind: sync_gap,
            state: state_str.to_string(),
        };

        JsonRpcResponse::success(id, status)
    }

    /// Get node version
    async fn system_version(&self, id: JsonRpcId) -> JsonRpcResponse {
        JsonRpcResponse::success(id, env!("CARGO_PKG_VERSION"))
    }

    /// Get node name
    async fn system_name(&self, id: JsonRpcId) -> JsonRpcResponse {
        JsonRpcResponse::success(id, "KratOs Node")
    }

    // =========================================================================
    // MEMPOOL METHODS
    // =========================================================================

    /// Get mempool status
    async fn mempool_status(&self, id: JsonRpcId) -> JsonRpcResponse {
        let count = self.node.mempool_size().await;

        // TODO: Get actual stats from mempool
        let status = MempoolStatus {
            pending_count: count,
            total_fees: 0, // TODO
            stats: MempoolStats {
                total_added: 0,
                total_removed: 0,
                total_evicted: 0,
                total_rejected: 0,
                total_replaced: 0,
            },
        };

        JsonRpcResponse::success(id, status)
    }

    /// Get mempool content (pending transactions)
    async fn mempool_content(&self, id: JsonRpcId) -> JsonRpcResponse {
        // TODO: Get actual pending transactions from mempool
        let count = self.node.mempool_size().await;

        JsonRpcResponse::success(id, serde_json::json!({
            "pending_count": count,
            "transactions": []  // TODO: Return actual transactions
        }))
    }

    // =========================================================================
    // CLOCK HEALTH METHODS (SPEC v6.1)
    // =========================================================================

    /// Get local clock health status
    ///
    /// Returns the node's clock synchronization status including:
    /// - status: Healthy, Degraded, Excluded, or Recovering
    /// - drift_ms: EMA of time drift in milliseconds
    /// - can_produce_blocks: Whether block production is allowed
    /// - emergency_mode: Whether network-wide emergency thresholds are active
    async fn clock_get_health(&self, id: JsonRpcId) -> JsonRpcResponse {
        let health = self.node.clock_health().await;

        JsonRpcResponse::success(id, serde_json::json!({
            "status": format!("{}", health.status()),
            "drift_ms": health.ema_drift_ms(),
            "can_produce_blocks": health.can_produce_blocks(),
            "emergency_mode": health.is_emergency_mode(),
            "status_since": health.status_since,
            "priority_modifier": health.priority_modifier(),
            "lifetime_stats": {
                "excluded_count": health.lifetime_excluded_count,
                "degraded_seconds": health.lifetime_degraded_seconds
            },
            "sample_count": health.samples.len()
        }))
    }

    /// Get validator clock record from consensus state
    ///
    /// Returns the consensus-stored clock failure record for a validator including:
    /// - clock_sync_failures: Number of times the validator was excluded
    /// - total_excluded_slots: Total slots missed due to clock issues
    /// - vc_penalty: Calculated VC penalty from clock failures
    async fn clock_get_validator_record(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        // Parse validator account ID from params
        let account_str: String = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match arr[0].as_str() {
                    Some(s) => s.to_string(),
                    None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected account address string")),
                }
            }
            _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [account_address]")),
        };

        // Parse account ID
        let account_id = match parse_account_id(&account_str) {
            Ok(acc) => acc,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        // Get record from storage
        let storage = self.node.storage();
        let storage_guard = storage.read().await;

        match storage_guard.get_clock_record(&account_id) {
            Ok(Some(record)) => {
                JsonRpcResponse::success(id, serde_json::json!({
                    "validator": account_str,
                    "clock_sync_failures": record.clock_sync_failures,
                    "last_exclusion_epoch": record.last_exclusion_epoch,
                    "total_excluded_slots": record.total_excluded_slots,
                    "vc_penalty": record.vc_penalty()
                }))
            }
            Ok(None) => {
                // No record = never excluded, return empty record
                JsonRpcResponse::success(id, serde_json::json!({
                    "validator": account_str,
                    "clock_sync_failures": 0,
                    "last_exclusion_epoch": null,
                    "total_excluded_slots": 0,
                    "vc_penalty": 0
                }))
            }
            Err(e) => {
                JsonRpcResponse::error(id, JsonRpcError::internal_error(&format!("Storage error: {:?}", e)))
            }
        }
    }

    // =========================================================================
    // EARLY VALIDATOR VOTING METHODS (Bootstrap Era)
    // Constitutional: Progressive decentralization through voting
    // =========================================================================

    /// Get early voting status
    ///
    /// Returns the current status of the early validator voting system:
    /// - is_bootstrap_era: Whether voting is currently allowed
    /// - current_block: Current block height
    /// - bootstrap_end_block: When bootstrap era ends
    /// - votes_required: Current threshold for new validators
    /// - validator_count: Active validator count
    /// - max_validators: Maximum early validators allowed
    async fn validator_get_early_voting_status(&self, id: JsonRpcId) -> JsonRpcResponse {
        use crate::consensus::validator::{BOOTSTRAP_ERA_BLOCKS, MAX_EARLY_VALIDATORS, ValidatorSet};

        let height = self.node.chain_height().await;
        let is_bootstrap = ValidatorSet::is_bootstrap_era(height);
        let validators_arc = self.node.validators();
        let validators = validators_arc.read().await;
        let active_count = validators.active_count();
        let votes_required = validators.votes_required_for_new_validator();
        let pending_count = validators.pending_candidates().len();

        JsonRpcResponse::success(id, serde_json::json!({
            "is_bootstrap_era": is_bootstrap,
            "current_block": height,
            "bootstrap_end_block": BOOTSTRAP_ERA_BLOCKS,
            "blocks_until_end": if is_bootstrap { BOOTSTRAP_ERA_BLOCKS.saturating_sub(height) } else { 0 },
            "votes_required": votes_required,
            "validator_count": active_count,
            "max_validators": MAX_EARLY_VALIDATORS,
            "pending_candidates": pending_count,
            "can_add_validators": is_bootstrap && active_count < MAX_EARLY_VALIDATORS
        }))
    }

    /// Get pending early validator candidates
    ///
    /// Returns list of all pending candidates with their vote counts
    async fn validator_get_pending_candidates(&self, id: JsonRpcId) -> JsonRpcResponse {
        let validators_arc = self.node.validators();
        let validators = validators_arc.read().await;
        let candidates: Vec<_> = validators.pending_candidates()
            .iter()
            .map(|c| {
                serde_json::json!({
                    "candidate": format!("0x{}", hex::encode(c.candidate.as_bytes())),
                    "proposer": format!("0x{}", hex::encode(c.proposer.as_bytes())),
                    "vote_count": c.vote_count(),
                    "votes_required": c.votes_required,
                    "has_quorum": c.has_quorum(),
                    "created_at": c.created_at,
                    "voters": c.voters.iter()
                        .map(|v| format!("0x{}", hex::encode(v.as_bytes())))
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        JsonRpcResponse::success(id, serde_json::json!({
            "candidates": candidates,
            "count": candidates.len()
        }))
    }

    /// Get votes for a specific candidate
    ///
    /// Returns detailed voting info for a candidate
    async fn validator_get_candidate_votes(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        // Parse candidate address from params
        let candidate_str: String = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match arr[0].as_str() {
                    Some(s) => s.to_string(),
                    None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected candidate address string")),
                }
            }
            _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [candidate_address]")),
        };

        // Parse account ID
        let candidate_id = match parse_account_id(&candidate_str) {
            Ok(acc) => acc,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        let validators_arc = self.node.validators();
        let validators = validators_arc.read().await;

        match validators.get_candidate(&candidate_id) {
            Some(candidacy) => {
                JsonRpcResponse::success(id, serde_json::json!({
                    "candidate": candidate_str,
                    "proposer": format!("0x{}", hex::encode(candidacy.proposer.as_bytes())),
                    "status": format!("{:?}", candidacy.status),
                    "vote_count": candidacy.vote_count(),
                    "votes_required": candidacy.votes_required,
                    "has_quorum": candidacy.has_quorum(),
                    "created_at": candidacy.created_at,
                    "approved_at": candidacy.approved_at,
                    "voters": candidacy.voters.iter()
                        .map(|v| format!("0x{}", hex::encode(v.as_bytes())))
                        .collect::<Vec<_>>()
                }))
            }
            None => {
                JsonRpcResponse::success(id, serde_json::json!({
                    "candidate": candidate_str,
                    "status": "not_found",
                    "error": "No candidacy found for this account"
                }))
            }
        }
    }

    /// Check if an account can vote for early validators
    ///
    /// Returns whether the account is an active validator who can vote
    async fn validator_can_vote(&self, id: JsonRpcId, params: serde_json::Value) -> JsonRpcResponse {
        // Parse account address from params
        let account_str: String = match params {
            serde_json::Value::Array(arr) if !arr.is_empty() => {
                match arr[0].as_str() {
                    Some(s) => s.to_string(),
                    None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected account address string")),
                }
            }
            _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [account_address]")),
        };

        // Parse account ID
        let account_id = match parse_account_id(&account_str) {
            Ok(acc) => acc,
            Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
        };

        let height = self.node.chain_height().await;
        let validators_arc = self.node.validators();
        let validators = validators_arc.read().await;
        let can_vote = validators.can_vote_early_validator(&account_id, height);
        let is_validator = validators.is_active(&account_id);
        let is_bootstrap = crate::consensus::validator::ValidatorSet::is_bootstrap_era(height);

        JsonRpcResponse::success(id, serde_json::json!({
            "account": account_str,
            "can_vote": can_vote,
            "is_validator": is_validator,
            "is_bootstrap_era": is_bootstrap,
            "reason": if can_vote {
                "Account is an active validator during bootstrap era"
            } else if !is_validator {
                "Account is not an active validator"
            } else if !is_bootstrap {
                "Bootstrap era has ended"
            } else {
                "Unknown reason"
            }
        }))
    }

    // =========================================================================
    // PUBLIC API (for direct method calls)
    // =========================================================================

    /// Get chain info (public)
    pub async fn chain_get_info_direct(&self) -> Result<ChainInfo, String> {
        let height = self.node.chain_height().await;
        let sync_gap = self.node.sync_gap().await;
        let is_synced = self.node.is_synced().await;
        let genesis_hash = self.node.genesis_hash();

        match self.node.current_block().await {
            Some(block) => Ok(ChainInfo {
                chain_name: "KratOs".to_string(),
                height,
                best_hash: format!("0x{}", hex::encode(block.hash().as_bytes())),
                genesis_hash: format!("0x{}", hex::encode(genesis_hash.as_bytes())),
                current_epoch: block.header.epoch,
                current_slot: block.header.slot,
                is_synced,
                sync_gap,
            }),
            None => Err("No current block".to_string()),
        }
    }

    /// Get balance (public)
    pub async fn state_get_balance_direct(&self, account: &AccountId) -> Result<Balance, String> {
        self.node
            .get_balance(account)
            .await
            .map_err(|e| format!("{:?}", e))
    }

    /// Get mempool size (public)
    pub async fn mempool_size(&self) -> usize {
        self.node.mempool_size().await
    }

    /// Get peer count (public)
    pub async fn peer_count(&self) -> usize {
        self.node.peer_count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis::{ChainConfig, GenesisSpec};
    use tempfile::tempdir;
    use std::sync::atomic::{AtomicU16, Ordering};

    // Counter to generate unique ports for each test
    static RPC_PORT_COUNTER: AtomicU16 = AtomicU16::new(42000);

    fn get_test_config() -> ChainConfig {
        let port = RPC_PORT_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut config = ChainConfig::mainnet();
        config.network.listen_port = port;
        config
    }

    #[tokio::test]
    async fn test_chain_get_info() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = Arc::new(
            KratOsNode::new(config, dir.path(), genesis, true)
                .await
                .unwrap(),
        );
        let methods = RpcMethods::new(node);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "chain_getInfo".to_string(),
            params: serde_json::Value::Null,
            id: JsonRpcId::Number(1),
        };

        let response = methods.handle_request(request).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        assert_eq!(result["chainName"], "KratOs");
        assert_eq!(result["height"], 0);
    }

    #[tokio::test]
    async fn test_state_get_balance() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = Arc::new(
            KratOsNode::new(config, dir.path(), genesis, true)
                .await
                .unwrap(),
        );
        let methods = RpcMethods::new(node);

        let alice = "0x0101010101010101010101010101010101010101010101010101010101010101";
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "state_getBalance".to_string(),
            params: serde_json::json!([alice]),
            id: JsonRpcId::Number(1),
        };

        let response = methods.handle_request(request).await;
        assert!(response.result.is_some());

        let balance: u128 = serde_json::from_value(response.result.unwrap()).unwrap();
        // Alice has 1M - 50k staked
        // SECURITY FIX: Updated to use new MIN_VALIDATOR_STAKE (50,000 KRAT)
        assert_eq!(balance, (1_000_000 - 50_000) * KRAT);
    }

    #[tokio::test]
    async fn test_system_health() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = Arc::new(
            KratOsNode::new(config, dir.path(), genesis, true)
                .await
                .unwrap(),
        );
        let methods = RpcMethods::new(node);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "system_health".to_string(),
            params: serde_json::Value::Null,
            id: JsonRpcId::Number(1),
        };

        let response = methods.handle_request(request).await;
        assert!(response.result.is_some());

        let result = response.result.unwrap();
        assert_eq!(result["healthy"], true);
        assert_eq!(result["blockHeight"], 0);
    }

    #[tokio::test]
    async fn test_chain_get_latest_block() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = Arc::new(
            KratOsNode::new(config, dir.path(), genesis, true)
                .await
                .unwrap(),
        );
        let methods = RpcMethods::new(node);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "chain_getLatestBlock".to_string(),
            params: serde_json::Value::Null,
            id: JsonRpcId::Number(1),
        };

        let response = methods.handle_request(request).await;
        assert!(response.result.is_some());

        let result = response.result.unwrap();
        assert_eq!(result["number"], 0); // Genesis block
    }

    #[tokio::test]
    async fn test_method_not_found() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = Arc::new(
            KratOsNode::new(config, dir.path(), genesis, true)
                .await
                .unwrap(),
        );
        let methods = RpcMethods::new(node);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "unknown_method".to_string(),
            params: serde_json::Value::Null,
            id: JsonRpcId::Number(1),
        };

        let response = methods.handle_request(request).await;
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn test_invalid_params() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = Arc::new(
            KratOsNode::new(config, dir.path(), genesis, true)
                .await
                .unwrap(),
        );
        let methods = RpcMethods::new(node);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "state_getBalance".to_string(),
            params: serde_json::json!(["invalid_address"]),
            id: JsonRpcId::Number(1),
        };

        let response = methods.handle_request(request).await;
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, -32602);
    }

    #[tokio::test]
    async fn test_system_version() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = Arc::new(
            KratOsNode::new(config, dir.path(), genesis, true)
                .await
                .unwrap(),
        );
        let methods = RpcMethods::new(node);

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "system_version".to_string(),
            params: serde_json::Value::Null,
            id: JsonRpcId::Number(1),
        };

        let response = methods.handle_request(request).await;
        assert!(response.result.is_some());

        let version: String = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn test_direct_methods() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = Arc::new(
            KratOsNode::new(config, dir.path(), genesis, true)
                .await
                .unwrap(),
        );
        let methods = RpcMethods::new(node);

        // Test direct chain info
        let info = methods.chain_get_info_direct().await.unwrap();
        assert_eq!(info.chain_name, "KratOs");
        assert_eq!(info.height, 0);

        // Test direct balance
        // SECURITY FIX: Updated to use new MIN_VALIDATOR_STAKE (50,000 KRAT)
        let alice = AccountId::from_bytes([1u8; 32]);
        let balance = methods.state_get_balance_direct(&alice).await.unwrap();
        assert_eq!(balance, (1_000_000 - 50_000) * KRAT);

        // Test mempool size
        assert_eq!(methods.mempool_size().await, 0);
    }
}
