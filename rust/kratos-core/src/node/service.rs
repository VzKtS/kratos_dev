// Service - Main node orchestrator for KratOs
// Principle: Coordinate all components, handle network events, manage lifecycle

use crate::consensus::clock_health::{ClockStatus, LocalClockHealth};
use crate::consensus::validator::ValidatorSet;
use crate::contracts::{
    krat::TokenomicsState,
    sidechains::ChainRegistry,
    staking::StakingRegistry,
};
use crate::genesis::{ChainConfig, GenesisBuilder, GenesisSpec};
use crate::network::dns_seeds::{DnsSeedResolver, parse_bootnode};
use crate::network::service::{BlockProvider, NetworkEvent, NetworkService, SharedBlockProvider};
use crate::network::sync::SyncState;
use crate::node::mempool::TransactionPool;
use crate::node::producer::{TransactionExecutor, BlockValidator, ValidationError, apply_block_rewards_for_import};
use crate::storage::{db::Database, state::StateBackend};
use crate::types::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn, error};

// =============================================================================
// BLOCK PROVIDER WRAPPER
// =============================================================================

/// Wrapper to provide blocks from storage
struct StorageBlockProvider {
    storage: Arc<RwLock<StateBackend>>,
}

impl StorageBlockProvider {
    fn new(storage: Arc<RwLock<StateBackend>>) -> Self {
        Self { storage }
    }
}

impl BlockProvider for StorageBlockProvider {
    fn get_blocks_range(&self, from: BlockNumber, max_count: u32) -> Vec<Block> {
        // Use try_read to avoid blocking
        if let Ok(storage) = self.storage.try_read() {
            match storage.get_blocks_range(from, max_count) {
                Ok(blocks) => blocks,
                Err(e) => {
                    tracing::warn!("Failed to get blocks range: {:?}", e);
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    fn get_block_by_hash(&self, hash: &Hash) -> Option<Block> {
        if let Ok(storage) = self.storage.try_read() {
            storage.get_block_by_hash(hash).ok().flatten()
        } else {
            None
        }
    }

    fn get_block_by_number(&self, number: BlockNumber) -> Option<Block> {
        if let Ok(storage) = self.storage.try_read() {
            storage.get_block_by_number(number).ok().flatten()
        } else {
            None
        }
    }
}

/// KratOs Node state
pub struct KratOsNode {
    /// Chain configuration
    config: ChainConfig,

    /// Data directory path
    data_path: PathBuf,

    /// Storage backend
    storage: Arc<RwLock<StateBackend>>,

    /// Network service
    network: Arc<RwLock<NetworkService>>,

    /// Network event receiver
    network_rx: Arc<RwLock<mpsc::UnboundedReceiver<NetworkEvent>>>,

    /// Transaction pool
    mempool: Arc<RwLock<TransactionPool>>,

    /// Validator set
    validators: Arc<RwLock<ValidatorSet>>,

    /// Staking registry
    staking: Arc<RwLock<StakingRegistry>>,

    /// Sidechain registry
    sidechains: Arc<RwLock<ChainRegistry>>,

    /// Tokenomics state
    tokenomics: Arc<RwLock<TokenomicsState>>,

    /// Current block
    current_block: Arc<RwLock<Option<Block>>>,

    /// Chain height
    chain_height: Arc<RwLock<BlockNumber>>,

    /// Genesis hash
    genesis_hash: Hash,

    /// Shutdown signal
    shutdown: Arc<RwLock<bool>>,

    /// Producer database for double-signing protection (persistent across block production attempts)
    producer_db: Arc<Database>,

    /// SECURITY FIX #36: Clock health tracking for soft degradation
    /// Persisted to file to survive node restarts
    clock_health: Arc<RwLock<LocalClockHealth>>,
}

impl KratOsNode {
    /// Create a new node
    ///
    /// - `genesis_mode`: If true, creates a new network (genesis node).
    ///   If false, joins existing network via DNS Seeds / bootnodes.
    ///
    /// ## Startup Sequence
    ///
    /// **Genesis Mode (`--genesis`)**:
    /// 1. Create genesis block locally
    /// 2. Start network and serve genesis to joining nodes
    ///
    /// **Join Mode (default)**:
    /// 1. Check if we have an existing genesis in DB
    /// 2. If yes: use stored genesis
    /// 3. If no: connect to network, fetch genesis from peers, then initialize
    pub async fn new(
        config: ChainConfig,
        data_path: &Path,
        genesis_spec: GenesisSpec,
        genesis_mode: bool,
    ) -> Result<Self, NodeError> {
        if genesis_mode {
            info!("🌟 GENESIS MODE - Creating new network");
        } else {
            info!("🔗 Joining existing network via peer discovery");
        }
        info!("Initializing KratOs node");
        info!("Chain: {}", config.chain_name);

        // Open database
        let db = Database::open(data_path.to_str().unwrap())
            .map_err(|e| NodeError::Storage(format!("DB error: {:?}", e)))?;

        let mut state = StateBackend::new(db);

        // Check if we have an existing genesis hash in storage
        let existing_genesis = state.get_genesis_hash().ok().flatten();

        // Type alias for genesis network info (validators and balances for network transmission)
        type GenesisNetworkInfo = (
            Vec<crate::network::request::GenesisValidatorInfo>,
            Vec<(AccountId, Balance)>,
        );

        // Determine genesis block and hash based on mode
        let (genesis_block, genesis_hash, genesis_validators, genesis_network_info) = if genesis_mode {
            // GENESIS MODE: Create new genesis block
            info!("Building genesis block (genesis mode)");
            let (block, validators) = GenesisBuilder::new(genesis_spec.clone())
                .build(&mut state)
                .map_err(|e| NodeError::Genesis(e))?;
            let hash = block.hash();
            info!("Genesis validators: {} active", validators.active_count());
            info!("Genesis built: hash={}, state_root={}", hash, block.header.state_root);

            // Convert spec to network format for serving to joining nodes
            let network_validators: Vec<_> = genesis_spec.validators.iter().map(|v| {
                crate::network::request::GenesisValidatorInfo {
                    account: v.account,
                    stake: v.stake,
                    is_bootstrap_validator: v.is_bootstrap_validator,
                }
            }).collect();
            let network_balances: Vec<_> = genesis_spec.balances.iter()
                .map(|(k, v)| (*k, *v))
                .collect();

            (block, hash, validators, (network_validators, network_balances))
        } else if let Some(stored_hash) = existing_genesis {
            // JOIN MODE + EXISTING DB: Use stored genesis
            info!("📂 Found existing genesis hash in storage: {}", stored_hash);
            let stored_block = state.get_block_by_number(0)
                .map_err(|e| NodeError::Storage(format!("Failed to read genesis block: {:?}", e)))?
                .ok_or_else(|| NodeError::Genesis("Genesis hash exists but block not found".to_string()))?;

            // Verify stored block hash matches
            if stored_block.hash() != stored_hash {
                return Err(NodeError::Genesis(format!(
                    "Stored genesis block hash mismatch: stored={}, computed={}",
                    stored_hash, stored_block.hash()
                )));
            }

            // Build validators from genesis spec
            // The spec contains the canonical validator list for this chain
            let validators = genesis_spec.build_validator_set();
            info!("Loaded {} validators from genesis spec", validators.active_count());

            // Convert spec to network format for serving to joining nodes
            let network_validators: Vec<_> = genesis_spec.validators.iter().map(|v| {
                crate::network::request::GenesisValidatorInfo {
                    account: v.account,
                    stake: v.stake,
                    is_bootstrap_validator: v.is_bootstrap_validator,
                }
            }).collect();
            let network_balances: Vec<_> = genesis_spec.balances.iter()
                .map(|(k, v)| (*k, *v))
                .collect();

            info!("Using stored genesis: hash={}", stored_hash);
            (stored_block, stored_hash, validators, (network_validators, network_balances))
        } else {
            // JOIN MODE + FRESH DB: Need to fetch genesis from network
            info!("🔄 No local genesis found - will fetch from network");

            // First, initialize network to connect to peers
            let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", config.network.listen_port);
            let (mut network, mut network_rx) = NetworkService::with_genesis_and_datadir(
                &listen_addr,
                Hash::ZERO,
                Some(data_path.to_path_buf()),
            )
                .await
                .map_err(|e| NodeError::Network(format!("Network error: {:?}", e)))?;

            // Try to discover peers via DNS seeds and bootnodes
            let bootstrap_addrs = Self::discover_peers(&config);

            // If we have bootnodes, add them and dial
            if !bootstrap_addrs.is_empty() {
                info!("🌐 Found {} bootstrap peers, connecting...", bootstrap_addrs.len());
                network.add_bootstrap_nodes(bootstrap_addrs.clone());

                // Dial first bootnode to request genesis
                if let Some((peer_id, addr)) = bootstrap_addrs.first() {
                    info!("📡 Dialing peer {} at {} to request genesis...", peer_id, addr);
                    if let Err(e) = network.dial(addr.clone()) {
                        warn!("Failed to dial {}: {:?}", peer_id, e);
                    }
                }
            } else {
                // No bootnodes - DNS seeds should provide them
                warn!("⚠️  No bootstrap nodes configured - check DNS seed configuration");
                info!("   Ensure DNS seeds are reachable or configure --bootnode manually");
            }

            // Wait for genesis info from network (with timeout)
            info!("⏳ Waiting for genesis info from network...");
            let genesis_timeout = std::time::Duration::from_secs(60);
            let start_time = std::time::Instant::now();
            let mut last_status_log = std::time::Instant::now();

            // Struct to hold all genesis info received from network
            struct ReceivedGenesis {
                block: Block,
                hash: Hash,
                validators: Vec<crate::network::request::GenesisValidatorInfo>,
                balances: Vec<(AccountId, Balance)>,
            }
            let mut received_genesis: Option<ReceivedGenesis> = None;

            // Run network and wait for genesis response
            while received_genesis.is_none() {
                if start_time.elapsed() > genesis_timeout {
                    let peers = network.peer_count();
                    return Err(NodeError::Network(format!(
                        "Timeout waiting for genesis from network after {}s (connected peers: {}). Ensure:\n\
                         1. A genesis node is running (started with --genesis flag)\n\
                         2. DNS seeds are configured and reachable\n\
                         3. Or configure a bootnode with --bootnode flag pointing to a reachable node",
                        genesis_timeout.as_secs(), peers
                    )));
                }

                // Log status every 10 seconds
                if last_status_log.elapsed() > std::time::Duration::from_secs(10) {
                    let elapsed = start_time.elapsed().as_secs();
                    let peers = network.peer_count();
                    info!("⏳ Still waiting for genesis... ({}s elapsed, {} peers)", elapsed, peers);
                    last_status_log = std::time::Instant::now();
                }

                // Poll the network to process connection and protocol events
                for _ in 0..5 {
                    network.poll_once().await;
                }

                // Process ALL pending events without blocking
                loop {
                    match network_rx.try_recv() {
                        Ok(NetworkEvent::PeerConnected(peer_id)) => {
                            info!("📶 Connected to peer {}, requesting genesis...", peer_id);
                            network.request_genesis(&peer_id);
                        }
                        Ok(NetworkEvent::GenesisReceived {
                            genesis_hash, genesis_block, chain_name,
                            genesis_validators, genesis_balances, ..
                        }) => {
                            info!("✅ Received genesis from network!");
                            info!("   Chain: {}", chain_name);
                            info!("   Hash: {}", genesis_hash);
                            info!("   Validators: {}", genesis_validators.len());
                            info!("   Balances: {}", genesis_balances.len());
                            received_genesis = Some(ReceivedGenesis {
                                block: genesis_block,
                                hash: genesis_hash,
                                validators: genesis_validators,
                                balances: genesis_balances,
                            });
                            break;  // Exit inner loop, outer while will exit due to received_genesis
                        }
                        Ok(other_event) => {
                            // Log other events for debugging
                            debug!("Network event while waiting for genesis: {:?}", other_event);
                        }
                        Err(mpsc::error::TryRecvError::Empty) => {
                            // No more events, continue outer loop
                            break;
                        }
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            return Err(NodeError::Network("Network channel closed".to_string()));
                        }
                    }
                }

                // Small sleep to prevent CPU spinning
                if received_genesis.is_none() {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }

            let received_genesis = received_genesis;

            match received_genesis {
                Some(genesis_info) => {
                    // Store the received genesis
                    state.store_block(&genesis_info.block)
                        .map_err(|e| NodeError::Storage(format!("Failed to store genesis: {:?}", e)))?;
                    state.set_genesis_hash(genesis_info.hash)
                        .map_err(|e| NodeError::Storage(format!("Failed to set genesis hash: {:?}", e)))?;

                    // Initialize drift tracker with genesis timestamp
                    // This is required for validating block timestamps during sync
                    state.init_drift_tracker(genesis_info.block.header.timestamp)
                        .map_err(|e| NodeError::Storage(format!("Failed to init drift tracker: {:?}", e)))?;

                    // Apply received genesis state (validators and balances from network)
                    // This uses the EXACT state from the genesis node, not hardcoded spec
                    // Pass expected state root from genesis block header for verification
                    let validators = apply_received_genesis_state(
                        &mut state,
                        &genesis_info.validators,
                        &genesis_info.balances,
                        genesis_info.block.header.state_root,
                    ).map_err(|e| NodeError::Storage(format!("Failed to apply genesis state: {}", e)))?;

                    info!("💾 Genesis stored locally");
                    info!("Loaded {} validators from genesis node", validators.active_count());

                    // Return - we'll continue initialization below with this genesis
                    // Note: We need to re-create the network with the correct genesis hash
                    drop(network);
                    drop(network_rx);

                    // Network info from received genesis (to pass along to other joining nodes)
                    let network_info = (genesis_info.validators, genesis_info.balances);
                    (genesis_info.block, genesis_info.hash, validators, network_info)
                }
                None => {
                    return Err(NodeError::Network("Failed to receive genesis from network".to_string()));
                }
            }
        };

        // Initialize network with correct genesis hash and persistent identity
        let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", config.network.listen_port);
        let (mut network, network_rx) = NetworkService::with_genesis_and_datadir(
            &listen_addr,
            genesis_hash,
            Some(data_path.to_path_buf()),
        )
            .await
            .map_err(|e| NodeError::Network(format!("Network error: {:?}", e)))?;

        // Setup peer discovery (for non-genesis mode)
        // Also set genesis info with validators so we can serve it to joining nodes
        let (network_validators, network_balances) = genesis_network_info;
        if genesis_mode {
            info!("📌 Genesis mode: skipping peer discovery (new network)");
            // In genesis mode, set ourselves as the genesis provider with validator info
            network.set_genesis_info_with_validators(
                genesis_block.clone(),
                config.chain_name.clone(),
                network_validators,
                network_balances,
            );
        } else {
            let bootstrap_addrs = Self::discover_peers(&config);
            if !bootstrap_addrs.is_empty() {
                info!("🌐 Total bootstrap nodes: {}", bootstrap_addrs.len());
                network.add_bootstrap_nodes(bootstrap_addrs.clone());

                // IMPORTANT: After recreating network with genesis hash, we must dial bootnodes
                // add_bootstrap_nodes only adds addresses, it doesn't connect
                info!("📞 Dialing bootstrap nodes to reconnect...");
                for (peer_id, addr) in &bootstrap_addrs {
                    debug!("   Dialing {} at {}", peer_id, addr);
                    if let Err(e) = network.dial(addr.clone()) {
                        warn!("Failed to dial {}: {:?}", peer_id, e);
                    }
                }
            } else {
                info!("ℹ️  No bootstrap nodes configured - check DNS seed configuration");
            }
            // Also set genesis info with validators so we can serve it to other joining nodes
            network.set_genesis_info_with_validators(
                genesis_block.clone(),
                config.chain_name.clone(),
                network_validators,
                network_balances,
            );
        }

        // Update network with genesis state
        network.update_local_state(0, genesis_hash);

        // Wrap storage in Arc<RwLock>
        let storage = Arc::new(RwLock::new(state));

        // Store genesis block in storage (if not already stored)
        {
            let storage_guard = storage.write().await;
            // Only store if not already present
            if storage_guard.get_block_by_number(0).ok().flatten().is_none() {
                storage_guard.store_block(&genesis_block)
                    .map_err(|e| NodeError::Storage(format!("Failed to store genesis: {:?}", e)))?;
            }
            if storage_guard.get_genesis_hash().ok().flatten().is_none() {
                storage_guard.set_genesis_hash(genesis_hash)
                    .map_err(|e| NodeError::Storage(format!("Failed to set genesis hash: {:?}", e)))?;
            }
        }

        // Set block provider for network sync
        let block_provider: SharedBlockProvider = Arc::new(RwLock::new(StorageBlockProvider::new(storage.clone())));
        network.set_block_provider(block_provider);

        // Initialize components
        let mempool = TransactionPool::default();
        let validators = genesis_validators;
        let staking = StakingRegistry::new();
        let sidechains = ChainRegistry::new();
        let tokenomics = genesis_spec.tokenomics;

        // Initialize producer database for double-signing protection
        let producer_db_path = data_path.join("producer");
        let producer_db = Database::open(producer_db_path.to_str().unwrap())
            .map_err(|e| NodeError::Storage(format!("Producer DB error: {:?}", e)))?;

        // SECURITY FIX #36: Initialize clock health from file (or create new)
        let clock_health = LocalClockHealth::load_or_create(data_path);
        info!("Clock health status: {} (drift: {:.0}ms)", clock_health.status(), clock_health.ema_drift_ms());

        Ok(Self {
            config,
            data_path: data_path.to_path_buf(),
            storage,
            network: Arc::new(RwLock::new(network)),
            network_rx: Arc::new(RwLock::new(network_rx)),
            mempool: Arc::new(RwLock::new(mempool)),
            validators: Arc::new(RwLock::new(validators)),
            staking: Arc::new(RwLock::new(staking)),
            sidechains: Arc::new(RwLock::new(sidechains)),
            tokenomics: Arc::new(RwLock::new(tokenomics)),
            current_block: Arc::new(RwLock::new(Some(genesis_block))),
            chain_height: Arc::new(RwLock::new(0)),
            genesis_hash,
            shutdown: Arc::new(RwLock::new(false)),
            producer_db: Arc::new(producer_db),
            clock_health: Arc::new(RwLock::new(clock_health)),
        })
    }

    /// Discover peers via DNS seeds and configured bootnodes
    fn discover_peers(config: &ChainConfig) -> Vec<(libp2p::PeerId, libp2p::Multiaddr)> {
        let mut bootstrap_addrs: Vec<(libp2p::PeerId, libp2p::Multiaddr)> = Vec::new();

        // 1. Try DNS Seeds for decentralized discovery
        info!("🔍 Resolving DNS seeds for peer discovery...");
        let mut dns_resolver = DnsSeedResolver::new();
        let dns_result = dns_resolver.resolve();

        if dns_result.success() {
            info!("📡 DNS seeds: {} peers discovered from {} seeds",
                  dns_result.peers.len(), dns_result.seeds_responded);
            bootstrap_addrs.extend(dns_result.peers);
        } else if !dns_result.errors.is_empty() {
            debug!("DNS seed resolution had errors: {:?}", dns_result.errors);
        }

        // 2. Add configured bootnodes (from CLI --bootnode or config file)
        if !config.network.bootnodes.is_empty() {
            info!("Adding {} configured bootstrap nodes", config.network.bootnodes.len());
            for bootnode in &config.network.bootnodes {
                match parse_bootnode(bootnode) {
                    Ok((peer_id, addr)) => {
                        bootstrap_addrs.push((peer_id, addr));
                    }
                    Err(e) => {
                        warn!("Failed to parse bootnode {}: {}", bootnode, e);
                    }
                }
            }
        }

        bootstrap_addrs
    }

    /// Start the node
    pub async fn start(&self) -> Result<(), NodeError> {
        info!("Starting KratOs node");

        // Reset shutdown flag
        *self.shutdown.write().await = false;

        info!("Node started successfully");

        Ok(())
    }

    /// Run the main event loop (blocking)
    pub async fn run(&self) -> Result<(), NodeError> {
        info!("Running node event loop");

        let mut maintenance_interval = tokio::time::interval(std::time::Duration::from_secs(30));

        loop {
            // Check shutdown flag
            if *self.shutdown.read().await {
                info!("Shutdown signal received");
                break;
            }

            tokio::select! {
                // Handle network events
                event = async {
                    let mut rx = self.network_rx.write().await;
                    rx.recv().await
                } => {
                    if let Some(event) = event {
                        self.handle_network_event(event).await;
                    }
                }

                // Periodic maintenance
                _ = maintenance_interval.tick() => {
                    self.perform_maintenance().await;
                }
            }
        }

        Ok(())
    }

    /// Handle a network event
    async fn handle_network_event(&self, event: NetworkEvent) {
        match event {
            NetworkEvent::BlockReceived { block, from } => {
                debug!("Received block #{} from {}", block.header.number, from);

                let current_height = *self.chain_height.read().await;
                let block_number = block.header.number;

                // Check if block is already imported (duplicate)
                if block_number <= current_height {
                    debug!("Block #{} already imported, ignoring duplicate from {}", block_number, from);
                    return;
                }

                // Check if block is too far ahead (out of order)
                // Don't ban - this is normal during sync race conditions
                if block_number > current_height + 1 {
                    debug!("Block #{} is ahead of local height {}, buffering for later",
                           block_number, current_height);
                    // Add to sync manager's pending blocks for later import
                    let mut network = self.network.write().await;
                    network.buffer_block(block);
                    return;
                }

                // Validate and import block
                match self.import_block(block.clone()).await {
                    Ok(()) => {
                        debug!("Block #{} imported successfully", block.header.number);
                        // Try to import any buffered blocks that are now sequential
                        self.try_import_buffered_blocks().await;
                    }
                    Err(e) => {
                        // Only ban for serious validation errors, not sequencing issues
                        let error_str = format!("{:?}", e);
                        if error_str.contains("Block number mismatch") {
                            // Race condition - block was already imported or another arrived first
                            debug!("Block #{} sequencing issue, not banning peer: {}", block_number, e);
                        } else if error_str.contains("Invalid signature")
                            || error_str.contains("Invalid parent hash")
                            || error_str.contains("Invalid transactions root") {
                            // These are real validation failures - ban the peer
                            warn!("Failed to import block from {}: {:?}", from, e);
                            let mut network = self.network.write().await;
                            network.ban_peer(from, &format!("Invalid block: {:?}", e));
                        } else {
                            // Other errors (state mismatch, etc) - log but don't ban
                            warn!("Failed to import block #{}: {:?}", block_number, e);
                        }
                    }
                }
            }

            NetworkEvent::TransactionReceived { transaction, from } => {
                debug!("Received transaction from {}", from);

                // Add to mempool
                let mut mempool = self.mempool.write().await;
                if let Err(e) = mempool.add(transaction) {
                    debug!("Failed to add transaction to mempool: {:?}", e);
                }
            }

            NetworkEvent::SyncBlocksReceived { blocks, from, has_more } => {
                info!("Received {} sync blocks from {} (has_more: {})", blocks.len(), from, has_more);

                let current_height = *self.chain_height.read().await;
                let mut imported_count = 0;
                let mut buffered_count = 0;

                // Import blocks in order, buffer out-of-order ones
                for block in blocks {
                    let block_number = block.header.number;

                    // Skip already imported blocks (duplicates from multiple peers)
                    if block_number <= current_height {
                        debug!("Skipping already imported block #{}", block_number);
                        continue;
                    }

                    // Get current height (may have changed after imports)
                    let current = *self.chain_height.read().await;

                    if block_number == current + 1 {
                        // This is the next sequential block - import it
                        match self.import_block(block.clone()).await {
                            Ok(()) => {
                                imported_count += 1;
                            }
                            Err(e) => {
                                // Log but don't break - try remaining blocks
                                warn!("Failed to import sync block #{}: {:?}", block_number, e);
                            }
                        }
                    } else {
                        // Block is ahead - buffer it for later
                        debug!("Buffering ahead block #{} (current: {})", block_number, current);
                        let mut network = self.network.write().await;
                        network.buffer_block(block);
                        buffered_count += 1;
                    }
                }

                if imported_count > 0 || buffered_count > 0 {
                    info!("Sync: imported {} blocks, buffered {} blocks", imported_count, buffered_count);
                }

                // Try to import any buffered blocks that are now sequential
                if imported_count > 0 {
                    self.try_import_buffered_blocks().await;
                }

                // Continue sync if needed
                if has_more {
                    let mut network = self.network.write().await;
                    network.maybe_start_sync();
                }
            }

            NetworkEvent::PeerConnected(peer_id) => {
                info!("Peer connected: {}", peer_id);
            }

            NetworkEvent::PeerDisconnected(peer_id) => {
                info!("Peer disconnected: {}", peer_id);
            }

            NetworkEvent::PeerStatus { peer, best_block, genesis_hash } => {
                debug!("Peer {} status: height={}, genesis={}", peer, best_block, genesis_hash);

                // Validate genesis
                if genesis_hash != self.genesis_hash {
                    warn!("Peer {} has different genesis hash!", peer);
                    let mut network = self.network.write().await;
                    network.ban_peer(peer, "Different genesis");
                }
            }

            NetworkEvent::SyncNeeded { local_height, network_height } => {
                info!("Sync needed: local={}, network={}", local_height, network_height);

                // Start sync
                let mut network = self.network.write().await;
                network.maybe_start_sync();
            }

            NetworkEvent::GenesisReceived { peer, genesis_hash, genesis_block, chain_name, .. } => {
                // This event is handled during node initialization (join mode).
                // If we receive it during normal operation, it means another node is joining.
                // We just log it since we already have our genesis.
                info!("📥 Received genesis info from {} (already initialized)", peer);
                debug!("   Chain: {}, Hash: {}", chain_name, genesis_hash);

                // Verify it matches our genesis
                if genesis_hash != self.genesis_hash {
                    warn!("Peer {} sent different genesis hash: {} (ours: {})",
                        peer, genesis_hash, self.genesis_hash);
                    // Note: We don't ban them here since they might be on a different network
                    // The status exchange will handle genesis validation
                } else {
                    debug!("Genesis matches - peer {} is on same network", peer);
                }
            }
        }
    }

    /// Import a block into the chain
    ///
    /// This method:
    /// 1. Validates block structure (number, parent hash, signature)
    /// 2. Executes all transactions against state
    /// 3. Validates the computed state root matches block header
    /// 4. Persists block and updates chain state
    async fn import_block(&self, block: Block) -> Result<(), NodeError> {
        let current_height = *self.chain_height.read().await;
        let block_number = block.header.number;
        let block_hash = block.hash();

        // 1. Check block number is sequential
        if block_number != current_height + 1 {
            return Err(NodeError::Consensus(format!(
                "Block number mismatch: expected {}, got {}",
                current_height + 1,
                block_number
            )));
        }

        // 1b. Idempotency check: if block already exists in storage with same hash, skip
        // This prevents cumulative state corruption when import_block() is retried after failure
        {
            let storage = self.storage.read().await;
            if let Ok(Some(existing)) = storage.get_block_by_number(block_number) {
                if existing.hash() == block_hash {
                    debug!("Block #{} already imported (idempotency check), skipping", block_number);
                    return Ok(());
                }
            }
        }

        // 2. Get parent block and validate parent hash
        let parent_block = self.current_block.read().await.clone();
        let parent = parent_block.as_ref().ok_or_else(|| {
            NodeError::Consensus("No parent block found".to_string())
        })?;

        if block.header.parent_hash != parent.hash() {
            return Err(NodeError::Consensus(format!(
                "Parent hash mismatch: expected {}, got {}",
                parent.hash(),
                block.header.parent_hash
            )));
        }

        // 3. Validate block structure (signature, transactions root, etc.)
        // During initial sync, we may not have the complete validator set yet.
        // If this is block #1 and the validator is unknown, add them dynamically
        // (they are the bootstrap validator who produced the genesis).
        {
            let mut validators = self.validators.write().await;

            // Check if author is known and active
            if !validators.is_active(&block.header.author) {
                // During initial sync (blocks 1-N), trust the block authors
                // They must be validators who were active when these blocks were produced
                if block_number <= 100 {
                    // Ensure validator is registered and active for sync
                    info!("🔄 Ensuring validator {} is active from synced block #{}",
                          block.header.author, block_number);
                    if let Err(e) = validators.ensure_bootstrap_validator(block.header.author, 0) {
                        warn!("Failed to ensure validator from sync: {:?}", e);
                    }
                }
            }

            if let Err(e) = BlockValidator::validate(&block, parent, &validators) {
                return Err(NodeError::Consensus(format!("Block validation failed: {:?}", e)));
            }
        }

        // 3b. Validate timestamp drift (only for real-time blocks, not historical sync)
        // This prevents gradual timestamp manipulation attacks on new blocks
        // For historical blocks during initial sync, we trust the network consensus
        {
            use crate::consensus::epoch::SLOT_DURATION_SECS;
            let storage = self.storage.write().await;

            // During initial sync (blocks below threshold), update drift tracker without strict validation
            // These blocks were already validated by the network when they were produced
            let is_historical_sync = block_number <= 100 && current_height == 0;

            if is_historical_sync {
                // Just update the tracker state for historical blocks
                storage.update_drift_tracker_for_sync(&block)
                    .map_err(|e| NodeError::Consensus(format!("Drift tracker update failed: {:?}", e)))?;
            } else {
                // Full validation for real-time blocks
                if let Err(e) = storage.validate_block_drift(&block, SLOT_DURATION_SECS) {
                    return Err(NodeError::Consensus(format!("Drift validation failed: {:?}", e)));
                }
            }
        }

        // 4. Execute all transactions, apply rewards, and validate state root
        {
            let mut storage = self.storage.write().await;

            // Execute each transaction and collect fees
            let mut total_fees: Balance = 0;
            for (idx, tx) in block.body.transactions.iter().enumerate() {
                let result = TransactionExecutor::execute(&mut storage, tx, block_number);

                if !result.success {
                    error!(
                        "Transaction {} in block #{} failed: {:?}",
                        idx, block_number, result.error
                    );
                    return Err(NodeError::Consensus(format!(
                        "Transaction {} execution failed: {:?}",
                        idx, result.error
                    )));
                }
                total_fees = total_fees.saturating_add(result.fee_paid);
            }

            // Process early validator voting transactions
            // These are executed separately because they need access to ValidatorSet
            for tx in block.body.transactions.iter() {
                match &tx.transaction.call {
                    TransactionCall::ProposeEarlyValidator { candidate } => {
                        let mut validators = self.validators.write().await;
                        match validators.propose_early_validator(*candidate, tx.transaction.sender, block_number) {
                            Ok(()) => {
                                info!(
                                    "✅ Early validator proposed: {} by {} at block #{}",
                                    candidate, tx.transaction.sender, block_number
                                );
                                // Check if quorum already reached (proposer counts as first vote)
                                // With 1-2 validators, only 1 vote is needed, so proposal = approval
                                if let Some(candidacy) = validators.get_candidate(candidate) {
                                    if candidacy.has_quorum() {
                                        match validators.approve_early_validator(*candidate, block_number) {
                                            Ok(()) => {
                                                info!(
                                                    "🎉 Early validator {} auto-approved (quorum=1) at block #{}",
                                                    candidate, block_number
                                                );
                                                // Initialize bootstrap VC so the new validator can be selected via VRF
                                                // Bootstrap validators need BOOTSTRAP_MIN_VC_REQUIREMENT (100 VC)
                                                // NOTE: We use the already-held `storage` lock from the outer scope
                                                let current_epoch = block_number / 600; // EPOCH_DURATION_BLOCKS
                                                if let Err(e) = storage.initialize_bootstrap_vc(*candidate, block_number, current_epoch) {
                                                    warn!("Failed to initialize bootstrap VC for {}: {:?}", candidate, e);
                                                } else {
                                                    info!("🎯 Initialized 100 VC for new validator {}", candidate);
                                                }
                                            }
                                            Err(e) => {
                                                warn!(
                                                    "Failed to auto-approve early validator {}: {:?}",
                                                    candidate, e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                // Log but don't fail block import - transaction fee was already paid
                                warn!(
                                    "Early validator proposal failed: {} - {:?}",
                                    candidate, e
                                );
                            }
                        }
                    }
                    TransactionCall::VoteEarlyValidator { candidate } => {
                        let mut validators = self.validators.write().await;
                        match validators.vote_early_validator(*candidate, tx.transaction.sender, block_number) {
                            Ok(has_quorum) => {
                                if has_quorum {
                                    // Quorum reached - approve and add to validator set
                                    match validators.approve_early_validator(*candidate, block_number) {
                                        Ok(()) => {
                                            info!(
                                                "🎉 Early validator {} APPROVED and added to validator set at block #{}",
                                                candidate, block_number
                                            );
                                            // Initialize bootstrap VC so the new validator can be selected via VRF
                                            // Bootstrap validators need BOOTSTRAP_MIN_VC_REQUIREMENT (100 VC)
                                            // NOTE: We use the already-held `storage` lock from the outer scope
                                            let current_epoch = block_number / 600; // EPOCH_DURATION_BLOCKS
                                            if let Err(e) = storage.initialize_bootstrap_vc(*candidate, block_number, current_epoch) {
                                                warn!("Failed to initialize bootstrap VC for {}: {:?}", candidate, e);
                                            } else {
                                                info!("🎯 Initialized 100 VC for new validator {}", candidate);
                                            }
                                        }
                                        Err(e) => {
                                            warn!(
                                                "Failed to approve early validator {}: {:?}",
                                                candidate, e
                                            );
                                        }
                                    }
                                } else {
                                    info!(
                                        "✅ Vote recorded for early validator {} by {} at block #{}",
                                        candidate, tx.transaction.sender, block_number
                                    );
                                }
                            }
                            Err(e) => {
                                // Log but don't fail block import - transaction fee was already paid
                                warn!(
                                    "Early validator vote failed: {} - {:?}",
                                    candidate, e
                                );
                            }
                        }
                    }
                    _ => {} // Other transaction types already executed
                }
            }

            // Apply block rewards (same as during production)
            // This credits the block author with block reward + fee share
            if let Err(e) = apply_block_rewards_for_import(
                &mut storage,
                block.header.author,
                block.header.epoch,
                total_fees,
            ) {
                error!("Failed to apply block rewards for block #{}: {}", block_number, e);
                return Err(NodeError::Consensus(format!("Block reward application failed: {}", e)));
            }

            // Compute state root after executing all transactions and applying rewards
            let chain_id = ChainId(0); // TODO: Get from config
            let computed_state_root = storage.compute_state_root(block_number, chain_id);

            // Validate state root matches block header
            if computed_state_root.root != block.header.state_root {
                error!(
                    "State root mismatch for block #{}: expected {}, computed {}",
                    block_number, block.header.state_root, computed_state_root.root
                );
                return Err(NodeError::Consensus(format!(
                    "State root mismatch: expected {}, computed {}",
                    block.header.state_root, computed_state_root.root
                )));
            }

            // Store state root for this block
            storage.store_state_root(block_number, computed_state_root)
                .map_err(|e| NodeError::Storage(format!("Failed to store state root: {:?}", e)))?;

            // Persist block to storage
            storage.store_block(&block)
                .map_err(|e| NodeError::Storage(format!("Failed to store block: {:?}", e)))?;

            // Update best block in storage
            storage.set_best_block(block_number)
                .map_err(|e| NodeError::Storage(format!("Failed to set best block: {:?}", e)))?;
        }

        // 5. Remove executed transactions from mempool
        {
            let mut mempool = self.mempool.write().await;
            mempool.remove_included(&block.body.transactions);
        }

        // 6. Update chain state
        *self.current_block.write().await = Some(block.clone());
        *self.chain_height.write().await = block_number;

        // 7. Update network with new state
        {
            let mut network = self.network.write().await;
            network.update_local_state(block_number, block_hash);
        }

        let tx_count = block.body.transactions.len();
        if tx_count > 0 {
            info!(
                "📥 Imported block #{} ({}) with {} transactions",
                block_number, block_hash, tx_count
            );
        } else {
            debug!("📥 Imported block #{} ({})", block_number, block_hash);
        }

        Ok(())
    }

    /// Store a block that was just produced locally
    ///
    /// Unlike import_block, this method does NOT re-execute transactions or re-apply rewards
    /// because the state was already modified during block production.
    /// It only persists the block and updates chain state.
    async fn store_produced_block(&self, block: Block) -> Result<(), NodeError> {
        let block_number = block.header.number;
        let block_hash = block.hash();

        {
            let storage = self.storage.write().await;

            // Persist block to storage (state root already stored by produce_block)
            storage.store_block(&block)
                .map_err(|e| NodeError::Storage(format!("Failed to store block: {:?}", e)))?;

            // Update best block in storage
            storage.set_best_block(block_number)
                .map_err(|e| NodeError::Storage(format!("Failed to set best block: {:?}", e)))?;
        }

        // Remove executed transactions from mempool
        {
            let mut mempool = self.mempool.write().await;
            mempool.remove_included(&block.body.transactions);
        }

        // Process early validator voting transactions
        // These are executed after block production because they need access to ValidatorSet
        for tx in block.body.transactions.iter() {
            match &tx.transaction.call {
                TransactionCall::ProposeEarlyValidator { candidate } => {
                    let mut validators = self.validators.write().await;
                    match validators.propose_early_validator(*candidate, tx.transaction.sender, block_number) {
                        Ok(()) => {
                            info!(
                                "✅ Early validator proposed: {} by {} at block #{}",
                                candidate, tx.transaction.sender, block_number
                            );
                            // Check if quorum already reached (proposer counts as first vote)
                            // With 1-2 validators, only 1 vote is needed, so proposal = approval
                            if let Some(candidacy) = validators.get_candidate(candidate) {
                                if candidacy.has_quorum() {
                                    match validators.approve_early_validator(*candidate, block_number) {
                                        Ok(()) => {
                                            info!(
                                                "🎉 Early validator {} auto-approved (quorum=1) at block #{}",
                                                candidate, block_number
                                            );
                                            // Initialize bootstrap VC so the new validator can be selected via VRF
                                            // Bootstrap validators need BOOTSTRAP_MIN_VC_REQUIREMENT (100 VC)
                                            drop(validators); // Release validators lock before acquiring storage lock
                                            let mut storage = self.storage.write().await;
                                            let current_epoch = block_number / 600; // EPOCH_DURATION_BLOCKS
                                            if let Err(e) = storage.initialize_bootstrap_vc(*candidate, block_number, current_epoch) {
                                                warn!("Failed to initialize bootstrap VC for {}: {:?}", candidate, e);
                                            } else {
                                                info!("🎯 Initialized 100 VC for new validator {}", candidate);
                                            }
                                        }
                                        Err(e) => {
                                            warn!(
                                                "Failed to auto-approve early validator {}: {:?}",
                                                candidate, e
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Early validator proposal failed: {} - {:?}",
                                candidate, e
                            );
                        }
                    }
                }
                TransactionCall::VoteEarlyValidator { candidate } => {
                    let mut validators = self.validators.write().await;
                    match validators.vote_early_validator(*candidate, tx.transaction.sender, block_number) {
                        Ok(has_quorum) => {
                            if has_quorum {
                                // Quorum reached - approve and add to validator set
                                match validators.approve_early_validator(*candidate, block_number) {
                                    Ok(()) => {
                                        info!(
                                            "🎉 Early validator {} APPROVED and added to validator set at block #{}",
                                            candidate, block_number
                                        );
                                        // Initialize bootstrap VC so the new validator can be selected via VRF
                                        // Bootstrap validators need BOOTSTRAP_MIN_VC_REQUIREMENT (100 VC)
                                        drop(validators); // Release validators lock before acquiring storage lock
                                        let mut storage = self.storage.write().await;
                                        let current_epoch = block_number / 600; // EPOCH_DURATION_BLOCKS
                                        if let Err(e) = storage.initialize_bootstrap_vc(*candidate, block_number, current_epoch) {
                                            warn!("Failed to initialize bootstrap VC for {}: {:?}", candidate, e);
                                        } else {
                                            info!("🎯 Initialized 100 VC for new validator {}", candidate);
                                        }
                                    }
                                    Err(e) => {
                                        warn!(
                                            "Failed to approve early validator {}: {:?}",
                                            candidate, e
                                        );
                                    }
                                }
                            } else {
                                info!(
                                    "✅ Vote recorded for early validator {} by {} at block #{}",
                                    candidate, tx.transaction.sender, block_number
                                );
                            }
                        }
                        Err(e) => {
                            warn!(
                                "Early validator vote failed: {} - {:?}",
                                candidate, e
                            );
                        }
                    }
                }
                _ => {} // Other transaction types already executed
            }
        }

        // Update chain state
        *self.current_block.write().await = Some(block.clone());
        *self.chain_height.write().await = block_number;

        // Update network with new state
        {
            let mut network = self.network.write().await;
            network.update_local_state(block_number, block_hash);
        }

        // Broadcast block to peers
        {
            let mut network = self.network.write().await;
            if let Err(e) = network.broadcast_block(block.clone()) {
                warn!("Failed to broadcast produced block: {:?}", e);
            }
        }

        let tx_count = block.body.transactions.len();
        if tx_count > 0 {
            info!(
                "⛏️  Produced and stored block #{} ({}) with {} transactions",
                block_number, block_hash, tx_count
            );
        } else {
            info!("⛏️  Produced and stored block #{} ({})", block_number, block_hash);
        }

        Ok(())
    }

    /// Perform periodic maintenance
    async fn perform_maintenance(&self) {
        // Clean up mempool
        let mempool = self.mempool.read().await;
        debug!("Mempool size: {}", mempool.len());
        drop(mempool);

        // Log network stats
        let network = self.network.read().await;
        let stats = network.peer_stats();
        debug!(
            "Network: {} peers, best_height={}, avg_score={}",
            stats.connected, stats.best_height, stats.average_score
        );

        // Check sync state
        let sync_state = network.sync_state();
        match sync_state {
            SyncState::Idle => debug!("Sync: idle"),
            SyncState::Synced => debug!("Sync: synced"),
            SyncState::Downloading => debug!("Sync: downloading"),
            SyncState::FarBehind => debug!("Sync: far behind, needs warp sync"),
        }
    }

    /// Stop the node gracefully
    pub async fn stop(&self) -> Result<(), NodeError> {
        info!("Stopping node");

        // Signal shutdown
        *self.shutdown.write().await = true;

        // Cleanup network
        let mut network = self.network.write().await;
        network.cleanup_rate_limiter();

        info!("Node stopped cleanly");

        Ok(())
    }

    /// Submit a transaction to the mempool
    pub async fn submit_transaction(&self, mut tx: SignedTransaction) -> Result<Hash, NodeError> {
        // Compute hash if not present (e.g., when deserialized from JSON RPC)
        let hash = match tx.hash {
            Some(h) => h,
            None => {
                let computed = tx.transaction.hash();
                tx.hash = Some(computed);
                computed
            }
        };

        // Add to mempool
        let mut mempool = self.mempool.write().await;
        mempool
            .add(tx.clone())
            .map_err(|e| NodeError::Transaction(format!("Mempool error: {:?}", e)))?;

        // Broadcast to network (skip in test mode)
        if cfg!(not(test)) {
            let mut network = self.network.write().await;
            network
                .broadcast_transaction(tx)
                .map_err(|e| NodeError::Network(format!("Broadcast error: {:?}", e)))?;
        }

        info!("Transaction {:?} submitted", hash);

        Ok(hash)
    }

    /// Get chain height
    pub async fn chain_height(&self) -> BlockNumber {
        *self.chain_height.read().await
    }

    /// Get current block
    pub async fn current_block(&self) -> Option<Block> {
        self.current_block.read().await.clone()
    }

    /// Get account balance
    pub async fn get_balance(&self, account: &AccountId) -> Result<Balance, NodeError> {
        let mut storage = self.storage.write().await;
        let account_info = storage
            .get_account(account)
            .map_err(|e| NodeError::Storage(format!("Read error: {:?}", e)))?;

        Ok(account_info.map(|a| a.free).unwrap_or(0))
    }

    /// Get mempool size
    pub async fn mempool_size(&self) -> usize {
        self.mempool.read().await.len()
    }

    /// Get connected peer count
    pub async fn peer_count(&self) -> usize {
        self.network.read().await.peer_count()
    }

    /// Get genesis hash
    pub fn genesis_hash(&self) -> Hash {
        self.genesis_hash
    }

    /// Get genesis timestamp from drift tracker
    /// This is the canonical reference for slot/time calculations
    pub async fn genesis_timestamp(&self) -> u64 {
        let storage = self.storage.read().await;
        storage.get_drift_tracker()
            .ok()
            .flatten()
            .map(|t| t.genesis_timestamp)
            .unwrap_or_else(|| {
                // Fallback: try to get from genesis block
                storage.get_block_by_number(0)
                    .ok()
                    .flatten()
                    .map(|b| b.header.timestamp)
                    .unwrap_or_else(|| chrono::Utc::now().timestamp() as u64)
            })
    }

    /// Get sync gap (how far behind we are)
    pub async fn sync_gap(&self) -> u64 {
        self.network.read().await.sync_gap()
    }

    /// Check if node is synced
    pub async fn is_synced(&self) -> bool {
        self.sync_gap().await < 5
    }

    /// Get connected peer IDs
    pub async fn connected_peers(&self) -> Vec<libp2p::PeerId> {
        self.network.read().await.connected_peers()
    }

    /// Get network stats
    pub async fn network_stats(&self) -> crate::network::peer::PeerStats {
        self.network.read().await.peer_stats()
    }

    /// Get local peer ID
    pub async fn local_peer_id(&self) -> libp2p::PeerId {
        self.network.read().await.local_peer_id()
    }

    /// Broadcast a produced block
    pub async fn broadcast_block(&self, block: Block) -> Result<(), NodeError> {
        let mut network = self.network.write().await;
        network
            .broadcast_block(block)
            .map_err(|e| NodeError::Network(format!("Broadcast error: {:?}", e)))
    }

    /// Request sync from best peer
    pub async fn start_sync(&self) {
        let mut network = self.network.write().await;
        network.maybe_start_sync();
    }

    /// Try to import any buffered blocks that are now sequential
    /// Called after successfully importing a block to drain the buffer
    async fn try_import_buffered_blocks(&self) {
        loop {
            let current_height = *self.chain_height.read().await;

            // Try to get the next sequential block from the buffer
            let next_block = {
                let mut network = self.network.write().await;
                network.update_sync_local_height(current_height);
                network.next_buffered_block(current_height + 1)
            };

            match next_block {
                Some(block) if block.header.number == current_height + 1 => {
                    // Import the buffered block
                    match self.import_block(block.clone()).await {
                        Ok(()) => {
                            debug!("Imported buffered block #{}", block.header.number);
                            // Continue loop to try importing more buffered blocks
                        }
                        Err(e) => {
                            warn!("Failed to import buffered block #{}: {:?}", block.header.number, e);
                            break;
                        }
                    }
                }
                _ => {
                    // No more sequential blocks in buffer
                    break;
                }
            }
        }
    }

    /// Get block by number from storage
    pub async fn get_block_by_number(&self, number: BlockNumber) -> Result<Option<Block>, NodeError> {
        let storage = self.storage.read().await;
        storage
            .get_block_by_number(number)
            .map_err(|e| NodeError::Storage(format!("Read error: {:?}", e)))
    }

    /// FIX: Get block by hash from storage
    pub async fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, NodeError> {
        let storage = self.storage.read().await;
        storage
            .get_block_by_hash(hash)
            .map_err(|e| NodeError::Storage(format!("Read error: {:?}", e)))
    }

    /// Get full account info (including nonce)
    pub async fn get_account_info(&self, account: &AccountId) -> Result<Option<AccountInfo>, NodeError> {
        let mut storage = self.storage.write().await;
        storage
            .get_account(account)
            .map_err(|e| NodeError::Storage(format!("Read error: {:?}", e)))
    }

    /// FIX: Get account nonce from storage
    pub async fn get_nonce(&self, account: &AccountId) -> Result<u64, NodeError> {
        let mut storage = self.storage.write().await;
        match storage.get_account(account) {
            Ok(Some(info)) => Ok(info.nonce),
            Ok(None) => Ok(0), // Account doesn't exist, nonce is 0
            Err(e) => Err(NodeError::Storage(format!("Read error: {:?}", e))),
        }
    }

    /// Get storage backend for block production
    pub fn storage(&self) -> Arc<RwLock<StateBackend>> {
        self.storage.clone()
    }

    /// Get mempool for block production
    pub fn mempool(&self) -> Arc<RwLock<TransactionPool>> {
        self.mempool.clone()
    }

    /// Get validator set
    pub fn validators(&self) -> Arc<RwLock<ValidatorSet>> {
        self.validators.clone()
    }

    /// Try to produce a block if we are the slot leader
    ///
    /// SECURITY FIX #36: Checks clock health before production.
    /// - Excluded/Recovering status: Block production suspended
    /// - Degraded status: Block production at reduced priority (logged)
    pub async fn try_produce_block(
        &self,
        validator_key: ed25519_dalek::SigningKey,
        epoch: EpochNumber,
        slot: SlotNumber,
    ) -> Result<Option<Block>, NodeError> {
        use crate::node::producer::BlockProducer;

        // Get validator ID from key
        let validator_id = AccountId::from_bytes(validator_key.verifying_key().to_bytes());

        // SECURITY FIX #36: Check clock health before attempting block production
        {
            let clock_health = self.clock_health.read().await;
            let status = clock_health.status();

            if !clock_health.can_produce_blocks() {
                // Record missed slot in consensus state
                let storage = self.storage.read().await;
                if let Err(e) = storage.record_clock_missed_slot(&validator_id) {
                    warn!("Failed to record clock missed slot: {:?}", e);
                }

                // Record failure transition if entering Excluded
                if status == ClockStatus::Excluded {
                    if let Err(e) = storage.record_clock_failure(&validator_id, epoch) {
                        warn!("Failed to record clock failure: {:?}", e);
                    }
                }

                debug!(
                    "Block production suspended: clock status={} (drift: {:.0}ms)",
                    status, clock_health.ema_drift_ms()
                );
                return Ok(None);
            }

            if status == ClockStatus::Degraded {
                warn!(
                    "⚠️ Block production at reduced priority: clock drift {:.0}ms",
                    clock_health.ema_drift_ms()
                );
            }
        }

        // Check if we are an active validator
        let validators = self.validators.read().await;

        if !validators.is_active(&validator_id) {
            // Not an active validator
            return Ok(None);
        }
        drop(validators);

        // Get parent block
        let parent_block = match self.current_block().await {
            Some(block) => block,
            None => return Err(NodeError::Consensus("No parent block".to_string())),
        };

        // Check if this slot is after the parent slot
        if slot <= parent_block.header.slot && epoch <= parent_block.header.epoch {
            return Ok(None); // Already produced for this slot
        }

        // Check if we are the slot leader via VRF selection
        // This is CRITICAL for consensus - only the selected validator should produce
        {
            let producer = BlockProducer::new(None, self.producer_db.clone());
            let validators = self.validators.read().await;
            let storage = self.storage.read().await;
            match producer.is_slot_leader(slot, epoch, &validators, &validator_id, &storage) {
                Ok(true) => {
                    // We are the slot leader, proceed with block production
                    debug!("🎯 We are slot leader for epoch {}, slot {}", epoch, slot);
                }
                Ok(false) => {
                    // Not our turn to produce
                    return Ok(None);
                }
                Err(e) => {
                    warn!("Failed to check slot leadership: {:?}", e);
                    return Ok(None);
                }
            }
        }

        // Use the persistent producer database for double-signing protection
        // This ensures signed slots are tracked across all block production attempts
        let mut producer = BlockProducer::new(Some(validator_key), self.producer_db.clone());
        match producer
            .produce_block(
                &parent_block,
                self.mempool.clone(),
                self.storage.clone(),
                validator_id,
                epoch,
                slot,
            )
            .await
        {
            Ok(block) => {
                // Store the block we just produced (without re-executing)
                // State was already modified during production
                self.store_produced_block(block.clone()).await?;
                Ok(Some(block))
            }
            Err(crate::node::producer::ProductionError::AlreadySignedThisSlot) => {
                // Already produced for this slot, that's OK
                Ok(None)
            }
            Err(e) => Err(NodeError::Consensus(format!("Block production error: {:?}", e))),
        }
    }

    // ===== Clock Health Methods (SECURITY FIX #36) =====

    /// Get current clock health status
    pub async fn clock_status(&self) -> ClockStatus {
        self.clock_health.read().await.status()
    }

    /// Get clock health EMA drift in milliseconds
    pub async fn clock_drift_ms(&self) -> f64 {
        self.clock_health.read().await.ema_drift_ms()
    }

    /// Check if clock health allows block production
    pub async fn can_produce_blocks(&self) -> bool {
        self.clock_health.read().await.can_produce_blocks()
    }

    /// Record a drift measurement and update clock health status
    /// Returns the new status and whether it changed
    pub async fn record_clock_drift(&self, drift_ms: i64) -> (ClockStatus, bool) {
        let mut clock_health = self.clock_health.write().await;
        let result = clock_health.record_drift(drift_ms);

        // Persist to file
        if let Err(e) = clock_health.save(&self.data_path) {
            warn!("Failed to save clock health: {:?}", e);
        }

        result
    }

    /// Get full clock health state (for RPC/monitoring)
    pub async fn clock_health(&self) -> LocalClockHealth {
        self.clock_health.read().await.clone()
    }

    // =========================================================================
    // NETWORK EVENT LOOP INTEGRATION
    // These methods allow runner.rs to integrate network polling into its event loop
    // =========================================================================

    /// Poll the network once - processes pending swarm events
    /// MUST be called regularly to:
    /// - Accept incoming connections
    /// - Handle request-response messages (genesis, sync, status)
    /// - Send/receive gossipsub messages
    /// - Process Kademlia DHT events
    pub async fn poll_network(&self) {
        let mut network = self.network.write().await;
        network.poll_once().await;
    }

    /// Get the next network event if available (non-blocking)
    /// Returns None if no event is pending
    pub async fn next_network_event(&self) -> Option<NetworkEvent> {
        let mut rx = self.network_rx.write().await;
        rx.try_recv().ok()
    }

    /// Process a network event
    /// This is the public wrapper around handle_network_event
    pub async fn process_network_event(&self, event: NetworkEvent) {
        self.handle_network_event(event).await;
    }
}

/// Apply genesis state received from network to local storage
///
/// This initializes the state with the EXACT balances and validators
/// from the genesis node, ensuring state root consistency.
fn apply_received_genesis_state(
    state: &mut StateBackend,
    validators: &[crate::network::request::GenesisValidatorInfo],
    balances: &[(AccountId, Balance)],
    expected_state_root: Hash,
) -> Result<ValidatorSet, String> {
    use crate::consensus::validator::ValidatorInfo;

    info!("🔍 Applying genesis state: {} balances, {} validators", balances.len(), validators.len());
    info!("   Expected state root: {}", expected_state_root);

    // Initialize balances
    for (account, balance) in balances {
        let mut account_info = AccountInfo::new();
        account_info.free = *balance;
        debug!("   Balance: {} = {} KRAT", account, *balance / crate::types::KRAT);
        state.set_account(*account, account_info)
            .map_err(|e| format!("Error setting account: {:?}", e))?;
    }

    // Initialize validators
    let mut validator_set = ValidatorSet::new();

    for validator in validators {
        debug!("   Validator: {} (bootstrap={}, stake={})",
               validator.account, validator.is_bootstrap_validator, validator.stake);
        if validator.is_bootstrap_validator {
            // Bootstrap validator: create account with 0 balance if needed
            let account_info = state
                .get_account(&validator.account)
                .map_err(|e| format!("Error getting account: {:?}", e))?
                .unwrap_or_else(AccountInfo::new);

            state.set_account(validator.account, account_info)
                .map_err(|e| format!("Error setting account: {:?}", e))?;

            // CRITICAL: Initialize bootstrap VC so the genesis validator can be selected via VRF
            // Bootstrap validators (stake=0) need BOOTSTRAP_MIN_VC_REQUIREMENT (100 VC)
            // This MUST match what the genesis node does in GenesisBuilder::build()
            state.initialize_bootstrap_vc(validator.account, 0, 0)
                .map_err(|e| format!("Error initialize_bootstrap_vc: {:?}", e))?;

            let validator_info = ValidatorInfo::new_bootstrap(validator.account, 0);
            validator_set.add_validator(validator_info)
                .map_err(|e| format!("Error adding validator: {:?}", e))?;
        } else {
            // Regular validator: reserve stake from balance
            let mut account_info = state
                .get_account(&validator.account)
                .map_err(|e| format!("Error getting account: {:?}", e))?
                .ok_or_else(|| format!("Validator account {:?} not found", validator.account))?;

            account_info
                .reserve(validator.stake)
                .map_err(|e| format!("Cannot reserve stake: {:?}", e))?;

            state.set_account(validator.account, account_info)
                .map_err(|e| format!("Error setting account: {:?}", e))?;

            let validator_info = ValidatorInfo::new(validator.account, validator.stake, 0);
            validator_set.add_validator(validator_info)
                .map_err(|e| format!("Error adding validator: {:?}", e))?;
        }
    }

    // CRITICAL: Compute and verify the state root for block 0
    // This MUST match what the genesis node computed
    let chain_id = crate::types::ChainId(0);
    let state_root = state.compute_state_root(0, chain_id);

    // Verify computed state root matches the expected one from genesis block header
    if state_root.root != expected_state_root {
        error!(
            "❌ Genesis state root mismatch! Expected: {}, Computed: {}",
            expected_state_root, state_root.root
        );
        error!("   This indicates the genesis state initialization differs from the genesis node");
        error!("   Check that all balances and validators are correctly transmitted and applied");
        return Err(format!(
            "Genesis state root mismatch: expected {}, computed {}",
            expected_state_root, state_root.root
        ));
    }

    state.store_state_root(0, state_root)
        .map_err(|e| format!("Error storing state root: {:?}", e))?;

    info!("📊 Genesis state root verified and stored: {}", state_root.root);

    Ok(validator_set)
}

/// Node errors
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("Genesis error: {0}")]
    Genesis(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Transaction error: {0}")]
    Transaction(String),

    #[error("Consensus error: {0}")]
    Consensus(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::sync::atomic::{AtomicU16, Ordering};

    // Counter to generate unique ports for each test
    static PORT_COUNTER: AtomicU16 = AtomicU16::new(40000);

    fn get_test_config() -> ChainConfig {
        let port = PORT_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut config = ChainConfig::mainnet();
        config.network.listen_port = port;
        config
    }

    #[tokio::test]
    async fn test_node_creation() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        // genesis_mode = true for test (no peer discovery needed)
        let node = KratOsNode::new(config, dir.path(), genesis, true).await;
        assert!(node.is_ok());

        let node = node.unwrap();
        assert_eq!(node.chain_height().await, 0);
    }

    #[tokio::test]
    async fn test_node_genesis_balance() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = KratOsNode::new(config, dir.path(), genesis, true)
            .await
            .unwrap();

        let alice = AccountId::from_bytes([1u8; 32]);
        let balance = node.get_balance(&alice).await.unwrap();

        // Alice should have 1M KRAT - 50k KRAT staked
        // SECURITY FIX: Updated to use new MIN_VALIDATOR_STAKE (50,000 KRAT)
        assert_eq!(balance, (1_000_000 - 50_000) * KRAT);
    }

    #[tokio::test]
    async fn test_submit_transaction() {
        let dir = tempdir().unwrap();
        let config = get_test_config();
        let genesis = GenesisSpec::with_validator(AccountId::from_bytes([1u8; 32]));

        let node = KratOsNode::new(config, dir.path(), genesis, true)
            .await
            .unwrap();

        // Disable signature verification for test
        {
            let mut mempool = node.mempool.write().await;
            mempool.config.verify_signatures = false;
        }

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
            hash: Some(Hash::hash(&[0])),
        };

        let result = node.submit_transaction(tx).await;
        assert!(result.is_ok());

        assert_eq!(node.mempool_size().await, 1);
    }
}
