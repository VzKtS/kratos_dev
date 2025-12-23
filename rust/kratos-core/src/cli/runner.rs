// Runner - Main node execution logic
// Principle: Orchestrate node startup, RPC server, and graceful shutdown

use crate::cli::config::NodeConfig;
use crate::consensus::epoch::{EPOCH_DURATION_BLOCKS, SLOT_DURATION_SECS};
use crate::node::producer::BlockProducer;
use crate::node::service::{KratOsNode, NodeError};
use crate::rpc::{RpcCall, RpcServer};
use crate::rpc::types::{
    AccountInfoRpc, BlockWithTransactions, ChainInfo, HealthStatus, MempoolStats, MempoolStatus,
    NetworkStatus, SyncStatus, SystemInfo,
};
use crate::types::*;
use ed25519_dalek::SigningKey;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn, debug, error, trace};

/// Run the node with the given configuration
pub async fn run_node(config: NodeConfig) -> Result<(), RunnerError> {
    info!("🚀 Starting KratOs node: {}", config.name);
    info!("📁 Data path: {}", config.base_path.display());
    info!("⛓️  Chain: {}", config.chain.chain_name);

    // Ensure base path exists
    std::fs::create_dir_all(&config.base_path)
        .map_err(|e| RunnerError::Io(format!("Failed to create data dir: {}", e)))?;

    // Load validator key if in validator mode
    let validator_key = if config.validator {
        load_validator_key(&config)?
    } else {
        None
    };

    // Create node
    // genesis_mode = true  -> creates new network (genesis node)
    // genesis_mode = false -> joins existing network via DNS Seeds / bootnodes
    let node = Arc::new(
        KratOsNode::new(
            config.chain.clone(),
            &config.base_path,
            config.genesis.clone(),
            config.genesis_mode,
        )
        .await
        .map_err(RunnerError::Node)?,
    );

    info!("🔗 Genesis: {}", node.genesis_hash());
    info!("🆔 Peer ID: {}", node.local_peer_id().await);

    // Create RPC channel
    let (rpc_tx, rpc_rx) = mpsc::unbounded_channel::<RpcCall>();

    // Start RPC server if enabled
    let rpc_handle = if config.rpc.enabled {
        let rpc_server = RpcServer::with_address(config.rpc.port, config.rpc.address);
        info!(
            "🌐 RPC server: http://{}:{}",
            format_ip(config.rpc.address),
            config.rpc.port
        );

        let handle = rpc_server
            .start_background(rpc_tx.clone())
            .await
            .map_err(|e| RunnerError::Rpc(format!("RPC server error: {:?}", e)))?;

        Some(handle)
    } else {
        None
    };

    // Start the node
    node.start().await.map_err(RunnerError::Node)?;

    info!("✅ Node started successfully");
    info!("📡 P2P port: {}", config.chain.network.listen_port);

    if config.validator {
        if let Some(ref key) = validator_key {
            info!("⚡ Validator mode: ACTIVE");

            // Initialize finality gadget for validators
            node.initialize_finality(key.clone()).await;

            // Initialize DNS Seed client for heartbeats (use validator key)
            node.initialize_dns_client(key.clone()).await;
        } else {
            warn!("⚠️  Validator mode enabled but no key loaded - will not produce blocks");
        }
    } else {
        // Non-validator nodes: generate a network identity key for DNS heartbeats
        // This allows joining nodes to be discoverable via DNS Seeds
        let network_key = load_or_generate_network_key(&config.base_path);
        node.initialize_dns_client(network_key).await;
    }

    // Run the main event loop
    let result = run_event_loop(node.clone(), rpc_rx, &config, validator_key).await;

    // Cleanup
    info!("🛑 Shutting down...");

    if let Some(handle) = rpc_handle {
        handle.shutdown();
        info!("   RPC server stopped");
    }

    node.stop().await.map_err(RunnerError::Node)?;
    info!("👋 Node stopped cleanly");

    result
}

/// Main event loop handling node events, RPC calls, and shutdown
async fn run_event_loop(
    node: Arc<KratOsNode>,
    mut rpc_rx: mpsc::UnboundedReceiver<RpcCall>,
    config: &NodeConfig,
    validator_key: Option<SigningKey>,
) -> Result<(), RunnerError> {
    let mut maintenance_interval = tokio::time::interval(std::time::Duration::from_secs(30));
    let mut stats_interval = tokio::time::interval(std::time::Duration::from_secs(60));

    // Block production interval (every slot = 6 seconds)
    let mut slot_interval = tokio::time::interval(std::time::Duration::from_secs(SLOT_DURATION_SECS));

    // Network polling interval - poll frequently to ensure responsive network
    // CRITICAL: Without this, peer connections and genesis requests don't work!
    let mut network_poll_interval = tokio::time::interval(std::time::Duration::from_millis(100));

    // Finality tick interval - matches round timeout (6 seconds = 1 slot)
    // This handles finality round timeouts and state transitions
    let mut finality_tick_interval = tokio::time::interval(std::time::Duration::from_secs(SLOT_DURATION_SECS));

    // Get genesis timestamp for slot calculation
    // CRITICAL: Must use the canonical genesis timestamp, not the current block timestamp
    // The genesis timestamp is the reference point for all slot calculations
    let genesis_timestamp = node.genesis_timestamp().await;

    if config.validator && validator_key.is_some() {
        info!("⏱️  Block production: every {}s", SLOT_DURATION_SECS);
    }

    loop {
        tokio::select! {
            // Handle shutdown signals
            _ = signal::ctrl_c() => {
                info!("\n⚠️  Ctrl+C received, shutting down...");
                break;
            }

            // Handle RPC calls
            Some(call) = rpc_rx.recv() => {
                handle_rpc_call(&node, call, config).await;
            }

            // CRITICAL: Network polling - processes connections, genesis requests, sync
            // Without this, the node cannot:
            // - Accept incoming connections
            // - Respond to genesis requests from joining nodes
            // - Process sync requests
            // - Handle gossipsub messages
            _ = network_poll_interval.tick() => {
                // Poll the network to process pending swarm events
                node.poll_network().await;

                // Process any resulting network events
                while let Some(event) = node.next_network_event().await {
                    node.process_network_event(event).await;
                }
            }

            // Block production (every slot)
            _ = slot_interval.tick() => {
                if config.validator {
                    if let Some(ref key) = validator_key {
                        try_produce_block(&node, key, genesis_timestamp).await;
                    }
                }
            }

            // Periodic maintenance
            _ = maintenance_interval.tick() => {
                perform_maintenance(&node).await;
            }

            // Periodic stats logging
            _ = stats_interval.tick() => {
                log_stats(&node).await;
            }

            // Finality tick - handles round timeouts and broadcasts pending messages
            _ = finality_tick_interval.tick() => {
                if config.validator && validator_key.is_some() {
                    trace!("[GRANDPA] runner: finality tick triggered");
                    // Tick the finality gadget to handle timeouts
                    node.tick_finality().await;
                    // Broadcast any pending finality messages
                    node.broadcast_finality_messages().await;
                    trace!("[GRANDPA] runner: finality tick completed");
                }
            }
        }
    }

    Ok(())
}

/// Try to produce a block if we are the slot leader
async fn try_produce_block(node: &Arc<KratOsNode>, validator_key: &SigningKey, genesis_timestamp: u64) {
    // Calculate current slot from time for slot assignment
    let now = chrono::Utc::now().timestamp() as u64;
    let elapsed = now.saturating_sub(genesis_timestamp);
    let current_slot = elapsed / SLOT_DURATION_SECS;

    // Calculate epoch from BLOCK HEIGHT (not time) for economics/bootstrap checks
    // This ensures epoch-based features (rewards, bootstrap) work correctly even when
    // the node has been idle and time has passed without blocks
    let current_block_height = node.chain_height().await;
    let current_epoch = current_block_height / EPOCH_DURATION_BLOCKS;

    // Get validator ID from key
    let validator_id = AccountId::from_bytes(validator_key.verifying_key().to_bytes());

    // Check if we are synced before producing
    if !node.is_synced().await {
        debug!("Not synced, skipping block production");
        return;
    }

    // Try to produce block
    match node.try_produce_block(validator_key.clone(), current_epoch, current_slot).await {
        Ok(Some(block)) => {
            // Broadcast the block
            if let Err(e) = node.broadcast_block(block).await {
                // InsufficientPeers is expected when no peers are connected - don't spam logs
                let err_str = format!("{:?}", e);
                if err_str.contains("InsufficientPeers") {
                    debug!("Broadcast skipped (no peers connected)");
                } else {
                    warn!("⚠️  Broadcast failed: {:?}", e);
                }
            }
        }
        Ok(None) => {
            // Not our turn to produce (normal)
        }
        Err(e) => {
            warn!("⚠️  Block error: {:?}", e);
        }
    }
}

/// Handle an RPC call by routing it to the appropriate node method
async fn handle_rpc_call(node: &Arc<KratOsNode>, call: RpcCall, config: &NodeConfig) {
    match call {
        RpcCall::ChainGetInfo(resp) => {
            let info = build_chain_info(node, config).await;
            let _ = resp.send(Ok(info));
        }

        RpcCall::ChainGetBlock(number, resp) => {
            match node.get_block_by_number(number).await {
                Ok(Some(block)) => {
                    let block_info = BlockWithTransactions::from(&block);
                    let _ = resp.send(Ok(block_info));
                }
                Ok(None) => {
                    let _ = resp.send(Err(format!("Block {} not found", number)));
                }
                Err(e) => {
                    let _ = resp.send(Err(format!("Failed to get block: {:?}", e)));
                }
            }
        }

        RpcCall::ChainGetLatestBlock(resp) => {
            if let Some(block) = node.current_block().await {
                let block_info = BlockWithTransactions::from(&block);
                let _ = resp.send(Ok(block_info));
            } else {
                let _ = resp.send(Err("No blocks available".to_string()));
            }
        }

        RpcCall::StateGetBalance(account, resp) => {
            match node.get_balance(&account).await {
                Ok(balance) => {
                    let _ = resp.send(Ok(balance));
                }
                Err(e) => {
                    let _ = resp.send(Err(format!("Failed to get balance: {:?}", e)));
                }
            }
        }

        RpcCall::StateGetAccount(account, resp) => {
            match node.get_account_info(&account).await {
                Ok(Some(account_info)) => {
                    let info = AccountInfoRpc::from_info(&account, &account_info);
                    let _ = resp.send(Ok(info));
                }
                Ok(None) => {
                    // Account doesn't exist, return zero values
                    let info = AccountInfoRpc::empty(&account);
                    let _ = resp.send(Ok(info));
                }
                Err(e) => {
                    let _ = resp.send(Err(format!("Failed to get account: {:?}", e)));
                }
            }
        }

        RpcCall::SystemHealth(resp) => {
            let health = HealthStatus {
                healthy: true,
                is_synced: node.is_synced().await,
                has_peers: node.peer_count().await > 0,
                block_height: node.chain_height().await,
                peer_count: node.peer_count().await,
            };
            let _ = resp.send(health);
        }

        RpcCall::SystemInfo(resp) => {
            let info = build_system_info(node, config).await;
            let _ = resp.send(Ok(info));
        }

        RpcCall::SystemPeers(resp) => {
            let peers = node.connected_peers().await;
            let peer_ids: Vec<String> = peers.iter().map(|p| p.to_string()).collect();
            let _ = resp.send((peers.len(), peer_ids));
        }

        RpcCall::SyncState(resp) => {
            let gap = node.sync_gap().await;
            let height = node.chain_height().await;
            let state = if gap < 5 {
                "synced".to_string()
            } else if gap < 100 {
                "syncing".to_string()
            } else {
                "far_behind".to_string()
            };
            let status = SyncStatus {
                syncing: gap >= 5,
                current_block: height,
                highest_block: height + gap,
                blocks_behind: gap,
                state,
            };
            let _ = resp.send(status);
        }

        RpcCall::MempoolStatus(resp) => {
            let size = node.mempool_size().await;
            let status = MempoolStatus {
                pending_count: size,
                total_fees: 0,
                stats: MempoolStats {
                    total_added: 0,
                    total_removed: 0,
                    total_evicted: 0,
                    total_rejected: 0,
                    total_replaced: 0,
                },
            };
            let _ = resp.send(status);
        }

        RpcCall::SubmitTransaction(tx, resp) => {
            match node.submit_transaction(tx).await {
                Ok(hash) => {
                    let _ = resp.send(Ok(hash));
                }
                Err(e) => {
                    let _ = resp.send(Err(format!("Failed to submit: {:?}", e)));
                }
            }
        }

        RpcCall::GetVersion(resp) => {
            let _ = resp.send(env!("CARGO_PKG_VERSION").to_string());
        }

        RpcCall::StateGetNonce(account_id, resp) => {
            match node.get_nonce(&account_id).await {
                Ok(nonce) => {
                    let _ = resp.send(Ok(nonce));
                }
                Err(e) => {
                    let _ = resp.send(Err(format!("Failed to get nonce: {:?}", e)));
                }
            }
        }

        RpcCall::StateGetTransactionHistory(account_id, limit, _offset, resp) => {
            use crate::types::TransactionCall;

            // Get current block height
            let height = node.chain_height().await;

            // Scan recent blocks (limit to last 1000 blocks for performance)
            let max_scan_blocks: u64 = 1000;
            let start_block = height.saturating_sub(max_scan_blocks);

            let storage = node.storage();
            let storage_guard = storage.read().await;

            let mut transactions = Vec::new();
            let limit = limit as usize;

            // Scan blocks from newest to oldest
            for block_num in (start_block..=height).rev() {
                if transactions.len() >= limit {
                    break;
                }

                if let Ok(Some(block)) = storage_guard.get_block_by_number(block_num) {
                    for tx in &block.body.transactions {
                        // Check if this transaction involves the account
                        let is_sender = tx.transaction.sender == account_id;
                        let is_recipient = match &tx.transaction.call {
                            TransactionCall::Transfer { to, .. } => *to == account_id,
                            _ => false,
                        };

                        if is_sender || is_recipient {
                            // Build transaction record
                            let (tx_type, counterparty, amount) = match &tx.transaction.call {
                                TransactionCall::Transfer { to, amount } => {
                                    let cp = if is_sender {
                                        format!("0x{}", hex::encode(to.as_bytes()))
                                    } else {
                                        format!("0x{}", hex::encode(tx.transaction.sender.as_bytes()))
                                    };
                                    ("Transfer", cp, *amount)
                                }
                                TransactionCall::Stake { amount } => ("Stake", format!("0x{}", hex::encode(account_id.as_bytes())), *amount),
                                TransactionCall::Unstake { amount } => ("Unstake", format!("0x{}", hex::encode(account_id.as_bytes())), *amount),
                                TransactionCall::RegisterValidator { stake } => ("RegisterValidator", format!("0x{}", hex::encode(account_id.as_bytes())), *stake),
                                TransactionCall::ProposeEarlyValidator { candidate } => {
                                    ("ProposeEarlyValidator", format!("0x{}", hex::encode(candidate.as_bytes())), 0)
                                }
                                TransactionCall::VoteEarlyValidator { candidate } => {
                                    ("VoteEarlyValidator", format!("0x{}", hex::encode(candidate.as_bytes())), 0)
                                }
                                _ => ("Other", format!("0x{}", hex::encode(account_id.as_bytes())), 0),
                            };

                            let direction = if is_sender { "sent" } else { "received" };

                            transactions.push(serde_json::json!({
                                "hash": format!("0x{}", hex::encode(tx.hash().as_bytes())),
                                "from": format!("0x{}", hex::encode(tx.transaction.sender.as_bytes())),
                                "to": counterparty,
                                "amount": amount,
                                "amountFormatted": format!("{:.6} KRAT", amount as f64 / 1_000_000_000_000.0),
                                "type": tx_type,
                                "direction": direction,
                                "nonce": tx.transaction.nonce,
                                "timestamp": tx.transaction.timestamp,
                                "blockNumber": block_num,
                                "blockHash": format!("0x{}", hex::encode(block.hash().as_bytes())),
                            }));

                            if transactions.len() >= limit {
                                break;
                            }
                        }
                    }
                }
            }

            let result = serde_json::json!({
                "address": format!("0x{}", hex::encode(account_id.as_bytes())),
                "transactions": transactions,
                "count": transactions.len(),
                "scannedBlocks": max_scan_blocks.min(height - start_block + 1),
                "fromBlock": start_block,
                "toBlock": height,
            });
            let _ = resp.send(Ok(result));
        }

        // Early Validator Voting methods (Bootstrap Era)
        RpcCall::ValidatorGetEarlyVotingStatus(resp) => {
            use crate::consensus::validator::{BOOTSTRAP_ERA_BLOCKS, MAX_EARLY_VALIDATORS, ValidatorSet};

            let height = node.chain_height().await;
            let is_bootstrap = ValidatorSet::is_bootstrap_era(height);
            let validators_arc = node.validators();
            let validators = validators_arc.read().await;
            let active_count = validators.active_count();
            let votes_required = validators.votes_required_for_new_validator();
            let pending_count = validators.pending_candidates().len();

            let result = serde_json::json!({
                "is_bootstrap_era": is_bootstrap,
                "current_block": height,
                "bootstrap_end_block": BOOTSTRAP_ERA_BLOCKS,
                "blocks_until_end": if is_bootstrap { BOOTSTRAP_ERA_BLOCKS.saturating_sub(height) } else { 0 },
                "votes_required": votes_required,
                "validator_count": active_count,
                "max_validators": MAX_EARLY_VALIDATORS,
                "pending_candidates": pending_count,
                "can_add_validators": is_bootstrap && active_count < MAX_EARLY_VALIDATORS
            });
            let _ = resp.send(Ok(result));
        }

        RpcCall::ValidatorGetPendingCandidates(resp) => {
            let validators_arc = node.validators();
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

            let result = serde_json::json!({
                "candidates": candidates,
                "count": candidates.len()
            });
            let _ = resp.send(Ok(result));
        }

        RpcCall::ValidatorGetCandidateVotes(candidate_id, resp) => {
            let validators_arc = node.validators();
            let validators = validators_arc.read().await;

            let result = match validators.get_candidate(&candidate_id) {
                Some(candidacy) => {
                    serde_json::json!({
                        "candidate": format!("0x{}", hex::encode(candidate_id.as_bytes())),
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
                    })
                }
                None => {
                    serde_json::json!({
                        "candidate": format!("0x{}", hex::encode(candidate_id.as_bytes())),
                        "status": "not_found",
                        "error": "No candidacy found for this account"
                    })
                }
            };
            let _ = resp.send(Ok(result));
        }

        RpcCall::ValidatorCanVote(account_id, resp) => {
            use crate::consensus::validator::ValidatorSet;

            let height = node.chain_height().await;
            let validators_arc = node.validators();
            let validators = validators_arc.read().await;
            let can_vote = validators.can_vote_early_validator(&account_id, height);
            let is_validator = validators.is_active(&account_id);
            let is_bootstrap = ValidatorSet::is_bootstrap_era(height);

            let result = serde_json::json!({
                "account": format!("0x{}", hex::encode(account_id.as_bytes())),
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
            });
            let _ = resp.send(Ok(result));
        }
    }
}

/// Build chain info response
async fn build_chain_info(node: &Arc<KratOsNode>, config: &NodeConfig) -> ChainInfo {
    let height = node.chain_height().await;
    let current_block = node.current_block().await;
    let best_hash = current_block
        .as_ref()
        .map(|b| b.hash())
        .unwrap_or(node.genesis_hash());

    ChainInfo {
        chain_name: config.chain.chain_name.clone(),
        height,
        best_hash: format!("0x{}", hex::encode(best_hash.as_bytes())),
        genesis_hash: format!("0x{}", hex::encode(node.genesis_hash().as_bytes())),
        current_epoch: height / 2400, // ~4 hours per epoch
        current_slot: height,
        is_synced: node.is_synced().await,
        sync_gap: node.sync_gap().await,
    }
}

/// Build system info response
async fn build_system_info(node: &Arc<KratOsNode>, config: &NodeConfig) -> SystemInfo {
    let stats = node.network_stats().await;
    let chain_info = build_chain_info(node, config).await;

    let network = NetworkStatus {
        local_peer_id: node.local_peer_id().await.to_string(),
        listening_addresses: vec![format!("/ip4/0.0.0.0/tcp/{}", config.chain.network.listen_port)],
        peer_count: stats.connected,
        network_best_height: stats.best_height,
        average_peer_score: stats.average_score,
    };

    SystemInfo {
        name: config.name.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        chain: chain_info,
        network,
        pending_txs: node.mempool_size().await,
    }
}

/// Perform periodic maintenance
async fn perform_maintenance(node: &Arc<KratOsNode>) {
    // Trigger sync check
    if !node.is_synced().await {
        node.start_sync().await;
    }

    // Send heartbeats to DNS Seeds (every 4 cycles = 120 seconds)
    node.send_dns_heartbeats().await;
}

/// Log node statistics
async fn log_stats(node: &Arc<KratOsNode>) {
    let height = node.chain_height().await;
    let peers = node.peer_count().await;
    let mempool = node.mempool_size().await;
    let gap = node.sync_gap().await;

    let sync_status = if gap == 0 {
        "synced".to_string()
    } else {
        format!("-{} blocks", gap)
    };

    info!(
        "📊 Block #{} | 👥 {} peers | 📬 {} pending | {}",
        height, peers, mempool, sync_status
    );
}

/// Format IP address bytes to string
fn format_ip(addr: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", addr[0], addr[1], addr[2], addr[3])
}

/// Runner errors
#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("IO error: {0}")]
    Io(String),

    #[error("Node error: {0}")]
    Node(#[from] NodeError),

    #[error("RPC error: {0}")]
    Rpc(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Key error: {0}")]
    Key(String),
}

/// Load validator key from file
fn load_validator_key(config: &NodeConfig) -> Result<Option<SigningKey>, RunnerError> {
    // Check if a key file was specified
    if let Some(ref key_path) = config.validator_key {
        info!("🔑 Loading key: {}", key_path.display());

        let content = std::fs::read_to_string(key_path)
            .map_err(|e| RunnerError::Key(format!("Failed to read key file: {}", e)))?;

        // Try to parse as JSON (format from key generate command)
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            // Look for secretKey field (ed25519 format)
            if let Some(secret_hex) = json.get("secretKey").and_then(|v| v.as_str()) {
                let hex_str = secret_hex.strip_prefix("0x").unwrap_or(secret_hex);
                let key_bytes = hex::decode(hex_str)
                    .map_err(|e| RunnerError::Key(format!("Invalid hex in key file: {}", e)))?;

                if key_bytes.len() != 32 {
                    return Err(RunnerError::Key(format!(
                        "Invalid key length: {} bytes (expected 32)",
                        key_bytes.len()
                    )));
                }

                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&key_bytes);
                let signing_key = SigningKey::from_bytes(&bytes);
                let account_hex = hex::encode(signing_key.verifying_key().to_bytes());

                info!("🏦 Account: 0x{}...{}", &account_hex[..8], &account_hex[56..]);

                return Ok(Some(signing_key));
            }

            return Err(RunnerError::Key(
                "Key file missing 'secretKey' field".to_string(),
            ));
        }

        // Try to parse as raw hex
        let hex_str = content.trim().strip_prefix("0x").unwrap_or(content.trim());
        let key_bytes = hex::decode(hex_str)
            .map_err(|e| RunnerError::Key(format!("Invalid hex in key file: {}", e)))?;

        if key_bytes.len() != 32 {
            return Err(RunnerError::Key(format!(
                "Invalid key length: {} bytes (expected 32)",
                key_bytes.len()
            )));
        }

        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&key_bytes);
        let signing_key = SigningKey::from_bytes(&bytes);
        let account_hex = hex::encode(signing_key.verifying_key().to_bytes());

        info!("🏦 Account: 0x{}...{}", &account_hex[..8], &account_hex[56..]);

        return Ok(Some(signing_key));
    }

    // No key available
    warn!("⚠️  No validator key! Use --validator-key <path>");
    warn!("   Generate with: kratos-node key generate -o validator.json");

    Ok(None)
}

/// Load or generate a network identity key for DNS Seed heartbeats
///
/// Non-validator nodes use this key to sign heartbeats.
/// The key is stored in the data directory and reused across restarts.
fn load_or_generate_network_key(base_path: &std::path::Path) -> SigningKey {
    use rand::rngs::OsRng;

    let key_path = base_path.join("network_identity.key");

    // Try to load existing key
    if key_path.exists() {
        if let Ok(key_bytes) = std::fs::read(&key_path) {
            if key_bytes.len() == 32 {
                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&key_bytes);
                debug!("🔑 Loaded network identity key from {:?}", key_path);
                return SigningKey::from_bytes(&bytes);
            }
        }
    }

    // Generate new key
    info!("🔑 Generating network identity key for DNS heartbeats");
    let signing_key = SigningKey::generate(&mut OsRng);

    // Save key for reuse
    if let Err(e) = std::fs::write(&key_path, signing_key.to_bytes()) {
        warn!("Failed to save network identity key: {}", e);
    } else {
        debug!("🔑 Saved network identity key to {:?}", key_path);
    }

    signing_key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_ip() {
        assert_eq!(format_ip([127, 0, 0, 1]), "127.0.0.1");
        assert_eq!(format_ip([0, 0, 0, 0]), "0.0.0.0");
        assert_eq!(format_ip([192, 168, 1, 1]), "192.168.1.1");
    }
}
