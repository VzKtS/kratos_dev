// Service - Main network service for KratOs
// Principle: Orchestrate all network protocols, emit events for application

use super::{
    behaviour::KratOsBehaviour,
    peer::PeerManager,
    protocol::{GossipTopic, NetworkMessage},
    rate_limit::{NetworkRateLimiter, RateLimitConfig},
    request::{
        BlockRequest, BlockResponse, KratosRequest, KratosResponse,
        StatusRequest, StatusResponse, SyncRequest, SyncResponse,
        GenesisRequest, GenesisResponse,
    },
    sync::SyncManager,
};
use crate::types::{Block, BlockNumber, Hash, SignedTransaction};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::RwLock;
use libp2p::{
    gossipsub::Event as GossipsubEvent,
    identity::Keypair,
    kad::Event as KadEvent,
    request_response::{self, Event as ReqResEvent, Message as ReqResMessage},
    swarm::SwarmEvent,
    Multiaddr, PeerId, Swarm, SwarmBuilder,
};
use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

// =============================================================================
// BLOCK PROVIDER TRAIT
// =============================================================================

/// Trait for providing blocks to network service (for sync responses)
pub trait BlockProvider: Send + Sync {
    /// Get blocks in range for sync
    fn get_blocks_range(&self, from: BlockNumber, max_count: u32) -> Vec<Block>;

    /// Get a specific block by hash
    fn get_block_by_hash(&self, hash: &Hash) -> Option<Block>;

    /// Get a specific block by number
    fn get_block_by_number(&self, number: BlockNumber) -> Option<Block>;
}

/// Type alias for the block provider
pub type SharedBlockProvider = Arc<RwLock<dyn BlockProvider>>;

// =============================================================================
// EVENTS
// =============================================================================

/// Network events for the application
#[derive(Debug)]
pub enum NetworkEvent {
    /// New block received via gossip
    BlockReceived {
        block: Block,
        from: PeerId,
    },

    /// New transaction received via gossip
    TransactionReceived {
        transaction: SignedTransaction,
        from: PeerId,
    },

    /// Blocks received via sync request
    SyncBlocksReceived {
        blocks: Vec<Block>,
        from: PeerId,
        has_more: bool,
    },

    /// Peer connected
    PeerConnected(PeerId),

    /// Peer disconnected
    PeerDisconnected(PeerId),

    /// Peer status received
    PeerStatus {
        peer: PeerId,
        best_block: BlockNumber,
        genesis_hash: Hash,
    },

    /// Sync needed
    SyncNeeded {
        local_height: BlockNumber,
        network_height: BlockNumber,
    },

    /// Genesis info received (for joining nodes)
    GenesisReceived {
        peer: PeerId,
        genesis_hash: Hash,
        genesis_block: Block,
        chain_name: String,
        /// Genesis validators (for state initialization)
        genesis_validators: Vec<super::request::GenesisValidatorInfo>,
        /// Genesis balances (for state initialization)
        genesis_balances: Vec<(crate::types::AccountId, crate::types::Balance)>,
    },
}

// =============================================================================
// PENDING REQUESTS
// =============================================================================

/// Tracks pending outbound requests
struct PendingRequest {
    peer: PeerId,
    request_type: RequestType,
    #[allow(dead_code)]
    sent_at: std::time::Instant,
}

#[derive(Debug, Clone)]
enum RequestType {
    Block(Hash),
    Sync { from: BlockNumber, max: u32 },
    Status,
    Genesis,
}

// =============================================================================
// NETWORK SERVICE
// =============================================================================

/// Main network service
pub struct NetworkService {
    /// libp2p swarm
    swarm: Swarm<KratOsBehaviour>,

    /// Event sender to application
    event_tx: mpsc::UnboundedSender<NetworkEvent>,

    /// Peer manager
    peer_manager: PeerManager,

    /// Sync manager
    sync_manager: SyncManager,

    /// Rate limiter
    rate_limiter: NetworkRateLimiter,

    /// Pending outbound requests
    pending_requests: HashMap<request_response::OutboundRequestId, PendingRequest>,

    /// Our local peer ID
    local_peer_id: PeerId,

    /// Genesis hash for chain validation
    genesis_hash: Hash,

    /// Genesis block for serving to joining nodes
    genesis_block: Option<Block>,

    /// Genesis validators (for serving to joining nodes)
    genesis_validators: Vec<super::request::GenesisValidatorInfo>,

    /// Genesis balances (for serving to joining nodes)
    genesis_balances: Vec<(crate::types::AccountId, crate::types::Balance)>,

    /// Chain name
    chain_name: String,

    /// Current best block height
    local_height: BlockNumber,

    /// Current best block hash
    local_hash: Hash,

    /// Block provider for serving sync requests (optional)
    block_provider: Option<SharedBlockProvider>,
}

// =============================================================================
// NETWORK IDENTITY PERSISTENCE
// =============================================================================

/// Default filename for network identity key
const NETWORK_KEY_FILENAME: &str = "network_key";

/// Load or generate a persistent network identity keypair
///
/// The keypair is stored in the data directory to ensure the PeerId
/// remains stable across node restarts.
fn load_or_generate_keypair(data_dir: Option<&PathBuf>) -> Result<Keypair, Box<dyn Error>> {
    if let Some(dir) = data_dir {
        let network_dir = dir.join("network");
        let key_path = network_dir.join(NETWORK_KEY_FILENAME);

        if key_path.exists() {
            // Load existing keypair
            let key_bytes = std::fs::read(&key_path)?;
            let keypair = Keypair::ed25519_from_bytes(key_bytes.clone())
                .map_err(|e| format!("Failed to decode network key: {}", e))?;
            info!("🔑 Loaded network identity from {:?}", key_path);
            Ok(keypair)
        } else {
            // Generate and save new keypair
            std::fs::create_dir_all(&network_dir)?;
            let keypair = Keypair::generate_ed25519();

            // Extract the secret key bytes for ed25519
            if let Some(ed25519_keypair) = keypair.clone().try_into_ed25519().ok() {
                let secret_bytes = ed25519_keypair.secret().as_ref().to_vec();
                std::fs::write(&key_path, &secret_bytes)?;

                // Set restrictive permissions on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
                }

                info!("🔑 Generated new network identity, saved to {:?}", key_path);
            }
            Ok(keypair)
        }
    } else {
        // No data directory - generate ephemeral keypair
        warn!("⚠️ No data directory specified - using ephemeral network identity (PeerId will change on restart)");
        Ok(Keypair::generate_ed25519())
    }
}

impl NetworkService {
    /// Create a new network service
    pub async fn new(
        listen_addr: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<NetworkEvent>), Box<dyn Error>> {
        Self::with_genesis_and_datadir(listen_addr, Hash::ZERO, None).await
    }

    /// Create a new network service with genesis hash
    pub async fn with_genesis(
        listen_addr: &str,
        genesis_hash: Hash,
    ) -> Result<(Self, mpsc::UnboundedReceiver<NetworkEvent>), Box<dyn Error>> {
        Self::with_genesis_and_datadir(listen_addr, genesis_hash, None).await
    }

    /// Create a new network service with genesis hash and data directory for persistent identity
    pub async fn with_genesis_and_datadir(
        listen_addr: &str,
        genesis_hash: Hash,
        data_dir: Option<PathBuf>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<NetworkEvent>), Box<dyn Error>> {
        // Load or generate keypair for node identity (persistent if data_dir is provided)
        let local_key = load_or_generate_keypair(data_dir.as_ref())?;
        let local_peer_id = PeerId::from(local_key.public());

        info!("Local peer id: {}", local_peer_id);

        // Create behaviour
        let behaviour = KratOsBehaviour::new(local_peer_id)?;

        // Create swarm
        let swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )?
            .with_behaviour(|_| behaviour)?
            .with_swarm_config(|c| c.with_idle_connection_timeout(std::time::Duration::from_secs(60)))
            .build();

        // Event channel
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Create components
        let peer_manager = PeerManager::new();
        let sync_manager = SyncManager::new(0);
        let rate_limiter = NetworkRateLimiter::new(RateLimitConfig::default());

        let mut service = Self {
            swarm,
            event_tx,
            peer_manager,
            sync_manager,
            rate_limiter,
            pending_requests: HashMap::new(),
            local_peer_id,
            genesis_hash,
            genesis_block: None,
            genesis_validators: Vec::new(),
            genesis_balances: Vec::new(),
            chain_name: "kratos".to_string(),
            local_height: 0,
            local_hash: Hash::ZERO,
            block_provider: None,
        };

        // Start listening
        service.swarm.listen_on(listen_addr.parse()?)?;

        Ok((service, event_rx))
    }

    /// Add bootstrap nodes
    pub fn add_bootstrap_nodes(&mut self, nodes: Vec<(PeerId, Multiaddr)>) {
        for (peer_id, addr) in &nodes {
            self.swarm.behaviour_mut().add_address(*peer_id, addr.clone());
        }
        self.peer_manager.add_bootstrap_nodes(nodes);
    }

    /// Connect to a specific address
    pub fn dial(&mut self, addr: Multiaddr) -> Result<(), Box<dyn Error>> {
        self.swarm.dial(addr)?;
        Ok(())
    }

    /// Update local chain state
    pub fn update_local_state(&mut self, height: BlockNumber, hash: Hash) {
        self.local_height = height;
        self.local_hash = hash;
        self.sync_manager.update_local_height(height);
    }

    /// Set block provider for serving sync requests
    pub fn set_block_provider(&mut self, provider: SharedBlockProvider) {
        self.block_provider = Some(provider);
    }

    /// Set genesis info for serving to joining nodes
    pub fn set_genesis_info(&mut self, genesis_block: Block, chain_name: String) {
        self.genesis_hash = genesis_block.hash();
        self.genesis_block = Some(genesis_block);
        self.chain_name = chain_name;
    }

    /// Set genesis info with validators for serving to joining nodes
    pub fn set_genesis_info_with_validators(
        &mut self,
        genesis_block: Block,
        chain_name: String,
        validators: Vec<super::request::GenesisValidatorInfo>,
        balances: Vec<(crate::types::AccountId, crate::types::Balance)>,
    ) {
        self.genesis_hash = genesis_block.hash();
        self.genesis_block = Some(genesis_block);
        self.chain_name = chain_name;
        self.genesis_validators = validators;
        self.genesis_balances = balances;
    }

    /// Request genesis info from a peer (for joining nodes)
    pub fn request_genesis(&mut self, peer_id: &PeerId) {
        let request = GenesisRequest::new();
        let request_id = self.swarm.behaviour_mut().send_request(peer_id, request);

        self.pending_requests.insert(request_id, PendingRequest {
            peer: *peer_id,
            request_type: RequestType::Genesis,
            sent_at: std::time::Instant::now(),
        });

        info!("📥 Requesting genesis info from peer {}", peer_id);
    }

    /// Broadcast a block via gossip
    pub fn broadcast_block(&mut self, block: Block) -> Result<(), Box<dyn Error>> {
        let msg = NetworkMessage::NewBlock(block);
        let data = msg.encode()?;
        self.swarm.behaviour_mut().publish(GossipTopic::Blocks, data)?;
        Ok(())
    }

    /// Broadcast a transaction via gossip
    pub fn broadcast_transaction(&mut self, tx: SignedTransaction) -> Result<(), Box<dyn Error>> {
        let msg = NetworkMessage::NewTransaction(tx);
        let data = msg.encode()?;
        self.swarm.behaviour_mut().publish(GossipTopic::Transactions, data)?;
        Ok(())
    }

    /// Request a specific block from a peer
    pub fn request_block(&mut self, peer_id: &PeerId, hash: Hash) {
        let request = KratosRequest::Block(BlockRequest::ByHash(hash));
        let request_id = self.swarm.behaviour_mut().send_request(peer_id, request);

        self.pending_requests.insert(request_id, PendingRequest {
            peer: *peer_id,
            request_type: RequestType::Block(hash),
            sent_at: std::time::Instant::now(),
        });

        debug!("Requested block {:?} from {}", hash, peer_id);
    }

    /// Request sync from a peer
    pub fn request_sync(&mut self, peer_id: &PeerId, from_block: BlockNumber, max_blocks: u32) {
        let request = KratosRequest::Sync(SyncRequest {
            from_block,
            max_blocks,
            include_bodies: true,
        });
        let request_id = self.swarm.behaviour_mut().send_request(peer_id, request);

        self.pending_requests.insert(request_id, PendingRequest {
            peer: *peer_id,
            request_type: RequestType::Sync { from: from_block, max: max_blocks },
            sent_at: std::time::Instant::now(),
        });

        debug!("Requested sync from {} starting at block {}", peer_id, from_block);
    }

    /// Request status from a peer
    pub fn request_status(&mut self, peer_id: &PeerId) {
        let request = KratosRequest::Status(StatusRequest {
            best_block: self.local_height,
            best_hash: self.local_hash,
            genesis_hash: self.genesis_hash,
            protocol_version: 1,
        });
        let request_id = self.swarm.behaviour_mut().send_request(peer_id, request);

        self.pending_requests.insert(request_id, PendingRequest {
            peer: *peer_id,
            request_type: RequestType::Status,
            sent_at: std::time::Instant::now(),
        });

        debug!("Requested status from {}", peer_id);
    }

    /// Start sync if needed
    pub fn maybe_start_sync(&mut self) {
        if !self.sync_manager.should_sync() {
            return;
        }

        // Find best peer for sync
        if let Some(peer) = self.peer_manager.best_sync_peer() {
            let peer_id = peer.id;
            let from_block = self.local_height + 1;
            self.request_sync(&peer_id, from_block, 50);
        }
    }

    /// Run the network event loop
    pub async fn run(&mut self) -> Result<(), Box<dyn Error>> {
        let mut maintenance_interval = tokio::time::interval(std::time::Duration::from_secs(30));

        loop {
            tokio::select! {
                event = self.swarm.next() => {
                    if let Some(event) = event {
                        self.handle_swarm_event(event);
                    } else {
                        break;
                    }
                }
                _ = maintenance_interval.tick() => {
                    self.perform_maintenance();
                }
            }
        }

        Ok(())
    }

    /// Poll the network once - processes pending events without blocking
    /// Used during genesis exchange when we need to run the network event loop
    /// but also check for other conditions (like timeout)
    pub async fn poll_once(&mut self) {
        use futures::future::poll_fn;
        use std::task::Poll;

        // Poll the swarm once, processing any ready events
        poll_fn(|cx| {
            // Process as many events as are immediately available
            loop {
                match self.swarm.poll_next_unpin(cx) {
                    Poll::Ready(Some(event)) => {
                        self.handle_swarm_event(event);
                    }
                    Poll::Ready(None) => {
                        // Swarm closed
                        return Poll::Ready(());
                    }
                    Poll::Pending => {
                        // No more events ready
                        return Poll::Ready(());
                    }
                }
            }
        }).await
    }

    /// Handle swarm events
    fn handle_swarm_event(&mut self, event: SwarmEvent<super::behaviour::KratOsBehaviourEvent>) {
        match event {
            SwarmEvent::Behaviour(behaviour_event) => {
                self.handle_behaviour_event(behaviour_event);
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                debug!("Connection established with peer: {}", peer_id);
                self.peer_manager.peer_connected(peer_id);
                let _ = self.event_tx.send(NetworkEvent::PeerConnected(peer_id));

                // Request status from new peer
                self.request_status(&peer_id);
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                debug!("Connection closed with peer: {}", peer_id);
                self.peer_manager.peer_disconnected(&peer_id);
                let _ = self.event_tx.send(NetworkEvent::PeerDisconnected(peer_id));
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                if let Some(peer_id) = peer_id {
                    warn!("Failed to connect to {}: {}", peer_id, error);
                    if let Some(info) = self.peer_manager.get_peer_mut(&peer_id) {
                        info.timeout();
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle behaviour events
    fn handle_behaviour_event(&mut self, event: super::behaviour::KratOsBehaviourEvent) {
        match event {
            // Gossipsub events
            super::behaviour::KratOsBehaviourEvent::Gossipsub(GossipsubEvent::Message {
                propagation_source: peer_id,
                message,
                ..
            }) => {
                // Rate limit check
                if let Err(e) = self.rate_limiter.check_message(&peer_id, message.data.len()) {
                    warn!("Rate limit error from peer {}: {:?}", peer_id, e);
                    return;
                }

                self.handle_gossip_message(&peer_id, &message.data);
            }

            // Request-response events
            super::behaviour::KratOsBehaviourEvent::RequestResponse(event) => {
                self.handle_request_response_event(event);
            }

            // Kademlia events
            super::behaviour::KratOsBehaviourEvent::Kad(KadEvent::RoutingUpdated { peer, .. }) => {
                debug!("Kademlia routing updated for {}", peer);
            }

            _ => {}
        }
    }

    /// Handle gossip messages
    fn handle_gossip_message(&mut self, from: &PeerId, data: &[u8]) {
        match NetworkMessage::decode(data) {
            Ok(NetworkMessage::NewBlock(block)) => {
                debug!("Received new block #{} from {}", block.header.number, from);

                // Update peer height (always, for sync tracking)
                self.peer_manager.update_peer_height(from, block.header.number);
                self.sync_manager.peer_height_update(block.header.number);

                // During initial sync, defer gossip blocks if they're too far ahead
                // This prevents "block number mismatch" errors when we receive
                // new blocks before the sync protocol delivers historical blocks
                if self.sync_manager.should_sync() {
                    let expected = self.local_height + 1;
                    if block.header.number > expected {
                        debug!(
                            "⏸️ Deferring gossip block #{} during sync (expected: #{})",
                            block.header.number, expected
                        );
                        // Trigger sync to catch up
                        self.maybe_start_sync();
                        return;
                    }
                }

                let _ = self.event_tx.send(NetworkEvent::BlockReceived {
                    block,
                    from: *from,
                });
            }
            Ok(NetworkMessage::NewTransaction(tx)) => {
                debug!("Received new transaction from {}", from);
                let _ = self.event_tx.send(NetworkEvent::TransactionReceived {
                    transaction: tx,
                    from: *from,
                });
            }
            Ok(msg) => {
                debug!("Received other gossip message: {:?}", msg);
            }
            Err(e) => {
                warn!("Failed to decode gossip message from {}: {}", from, e);
                self.peer_manager.record_bad_transaction(from);
            }
        }
    }

    /// Handle request-response events
    fn handle_request_response_event(&mut self, event: ReqResEvent<KratosRequest, KratosResponse>) {
        match event {
            ReqResEvent::Message { peer, message } => {
                match message {
                    ReqResMessage::Request { request, channel, .. } => {
                        self.handle_incoming_request(peer, request, channel);
                    }
                    ReqResMessage::Response { request_id, response } => {
                        self.handle_response(request_id, peer, response);
                    }
                }
            }
            ReqResEvent::OutboundFailure { peer, request_id, error, .. } => {
                warn!("Request to {} failed: {:?}", peer, error);
                self.pending_requests.remove(&request_id);

                if let Some(info) = self.peer_manager.get_peer_mut(&peer) {
                    info.timeout();
                }
            }
            ReqResEvent::InboundFailure { peer, error, .. } => {
                warn!("Inbound request from {} failed: {:?}", peer, error);
            }
            _ => {}
        }
    }

    /// Handle incoming requests
    fn handle_incoming_request(
        &mut self,
        peer: PeerId,
        request: KratosRequest,
        channel: request_response::ResponseChannel<KratosResponse>,
    ) {
        match request {
            KratosRequest::Block(block_req) => {
                let response = if let Some(ref provider) = self.block_provider {
                    // Try to acquire lock without blocking
                    if let Ok(guard) = provider.try_read() {
                        match block_req {
                            BlockRequest::ByHash(hash) => {
                                match guard.get_block_by_hash(&hash) {
                                    Some(block) => KratosResponse::Block(BlockResponse::Block(block)),
                                    None => KratosResponse::Block(BlockResponse::NotFound),
                                }
                            }
                            BlockRequest::ByNumber(num) | BlockRequest::HeaderByNumber(num) => {
                                match guard.get_block_by_number(num) {
                                    Some(block) => KratosResponse::Block(BlockResponse::Block(block)),
                                    None => KratosResponse::Block(BlockResponse::NotFound),
                                }
                            }
                        }
                    } else {
                        debug!("Block provider busy, responding not found to {}", peer);
                        KratosResponse::Block(BlockResponse::NotFound)
                    }
                } else {
                    KratosResponse::Block(BlockResponse::NotFound)
                };
                let _ = self.swarm.behaviour_mut().send_response(channel, response);
            }
            KratosRequest::Sync(sync_req) => {
                // SECURITY FIX #11: Validate max_blocks to prevent memory exhaustion
                use super::request::MAX_SYNC_BLOCKS;
                let safe_max_blocks = sync_req.max_blocks.min(MAX_SYNC_BLOCKS);

                if sync_req.max_blocks > MAX_SYNC_BLOCKS {
                    warn!("Peer {} requested {} blocks (max: {}), limiting to {}",
                          peer, sync_req.max_blocks, MAX_SYNC_BLOCKS, safe_max_blocks);
                }

                let response = if let Some(ref provider) = self.block_provider {
                    // Try to acquire lock without blocking
                    if let Ok(guard) = provider.try_read() {
                        let blocks = guard.get_blocks_range(sync_req.from_block, safe_max_blocks);
                        let has_more = blocks.len() as u32 == safe_max_blocks;
                        info!("📤 Serving {} blocks to {} (from={})", blocks.len(), peer, sync_req.from_block);
                        KratosResponse::Sync(SyncResponse {
                            blocks,
                            has_more,
                            best_height: self.local_height,
                        })
                    } else {
                        debug!("Block provider busy, responding empty to {}", peer);
                        KratosResponse::Sync(SyncResponse {
                            blocks: vec![],
                            has_more: false,
                            best_height: self.local_height,
                        })
                    }
                } else {
                    KratosResponse::Sync(SyncResponse {
                        blocks: vec![],
                        has_more: false,
                        best_height: self.local_height,
                    })
                };
                let _ = self.swarm.behaviour_mut().send_response(channel, response);
            }
            KratosRequest::Status(status_req) => {
                // Validate genesis hash
                // Allow peers with Hash::ZERO - they are still syncing/requesting genesis
                if status_req.genesis_hash != self.genesis_hash
                    && self.genesis_hash != Hash::ZERO
                    && status_req.genesis_hash != Hash::ZERO
                {
                    warn!("Peer {} has different genesis hash (theirs: {}, ours: {})!",
                          peer, status_req.genesis_hash, self.genesis_hash);
                    self.peer_manager.ban_peer(&peer, "Different genesis");
                    return;
                }

                // Update peer height
                self.peer_manager.update_peer_height(&peer, status_req.best_block);
                self.sync_manager.peer_height_update(status_req.best_block);

                // Send our status
                let response = KratosResponse::Status(StatusResponse {
                    best_block: self.local_height,
                    best_hash: self.local_hash,
                    genesis_hash: self.genesis_hash,
                    protocol_version: 1,
                    peer_count: self.peer_manager.connected_count() as u32,
                });
                let _ = self.swarm.behaviour_mut().send_response(channel, response);
            }
            KratosRequest::Genesis(_genesis_req) => {
                // A joining node is requesting our genesis info
                info!("📤 Peer {} requesting genesis info", peer);

                if let Some(ref genesis_block) = self.genesis_block {
                    let response = KratosResponse::Genesis(GenesisResponse {
                        genesis_hash: self.genesis_hash,
                        genesis_block: genesis_block.clone(),
                        chain_name: self.chain_name.clone(),
                        protocol_version: 1,
                        genesis_validators: self.genesis_validators.clone(),
                        genesis_balances: self.genesis_balances.clone(),
                    });
                    let _ = self.swarm.behaviour_mut().send_response(channel, response);
                    info!("📤 Sent genesis info to peer {} (hash={}, validators={})",
                          peer, self.genesis_hash, self.genesis_validators.len());
                } else {
                    // We don't have genesis block yet - we shouldn't serve genesis requests
                    warn!("Cannot serve genesis to {}: no genesis block available (we may still be syncing)", peer);
                    // Don't respond - let the request timeout so the peer tries another node
                }
            }
        }
    }

    /// Handle responses to our requests
    fn handle_response(
        &mut self,
        request_id: request_response::OutboundRequestId,
        peer: PeerId,
        response: KratosResponse,
    ) {
        let _pending = match self.pending_requests.remove(&request_id) {
            Some(p) => p,
            None => {
                warn!("Received response for unknown request");
                return;
            }
        };

        match response {
            KratosResponse::Block(block_res) => {
                match block_res {
                    BlockResponse::Block(block) => {
                        debug!("Received block #{} from {}", block.header.number, peer);
                        self.peer_manager.record_good_block(&peer);
                        self.sync_manager.add_downloaded_block(block.clone());

                        let _ = self.event_tx.send(NetworkEvent::SyncBlocksReceived {
                            blocks: vec![block],
                            from: peer,
                            has_more: false,
                        });
                    }
                    BlockResponse::NotFound => {
                        debug!("Block not found at peer {}", peer);
                    }
                    BlockResponse::Error(e) => {
                        warn!("Block request error from {}: {}", peer, e);
                    }
                }
            }
            KratosResponse::Sync(sync_res) => {
                info!("Received {} blocks from {} (has_more: {})",
                    sync_res.blocks.len(), peer, sync_res.has_more);

                self.peer_manager.update_peer_height(&peer, sync_res.best_height);
                self.sync_manager.peer_height_update(sync_res.best_height);
                self.sync_manager.handle_sync_response(sync_res.blocks.clone(), sync_res.has_more);

                let _ = self.event_tx.send(NetworkEvent::SyncBlocksReceived {
                    blocks: sync_res.blocks,
                    from: peer,
                    has_more: sync_res.has_more,
                });

                // Continue sync if needed
                if sync_res.has_more {
                    self.maybe_start_sync();
                }
            }
            KratosResponse::Status(status_res) => {
                // Validate genesis
                if status_res.genesis_hash != self.genesis_hash && self.genesis_hash != Hash::ZERO {
                    warn!("Peer {} has different genesis hash!", peer);
                    self.peer_manager.ban_peer(&peer, "Different genesis");
                    return;
                }

                // Update peer info
                self.peer_manager.update_peer_height(&peer, status_res.best_block);
                self.sync_manager.peer_height_update(status_res.best_block);

                debug!("Peer {} status: height={}, peers={}",
                    peer, status_res.best_block, status_res.peer_count);

                let _ = self.event_tx.send(NetworkEvent::PeerStatus {
                    peer,
                    best_block: status_res.best_block,
                    genesis_hash: status_res.genesis_hash,
                });

                // Check if we need to sync
                // For initial sync (local_height == 0), sync immediately if peer is ahead
                // For normal operation, use threshold of 10 blocks
                let sync_threshold = if self.local_height == 0 { 0 } else { 10 };
                if status_res.best_block > self.local_height + sync_threshold {
                    info!("🔄 Sync triggered: local={}, network={}, threshold={}",
                        self.local_height, status_res.best_block, sync_threshold);
                    let _ = self.event_tx.send(NetworkEvent::SyncNeeded {
                        local_height: self.local_height,
                        network_height: status_res.best_block,
                    });
                    // Start sync immediately
                    self.maybe_start_sync();
                }
            }
            KratosResponse::Genesis(genesis_res) => {
                // We received genesis info from a peer - this is for joining nodes
                info!("📥 Received genesis from peer {}: hash={}, chain={}",
                    peer, genesis_res.genesis_hash, genesis_res.chain_name);

                // Validate the genesis block hash matches what they claim
                let computed_hash = genesis_res.genesis_block.hash();
                if computed_hash != genesis_res.genesis_hash {
                    warn!("Genesis block hash mismatch from {}: claimed={}, computed={}",
                        peer, genesis_res.genesis_hash, computed_hash);
                    self.peer_manager.ban_peer(&peer, "Genesis hash mismatch");
                    return;
                }

                // Emit event for the node to handle
                let _ = self.event_tx.send(NetworkEvent::GenesisReceived {
                    peer,
                    genesis_hash: genesis_res.genesis_hash,
                    genesis_block: genesis_res.genesis_block,
                    chain_name: genesis_res.chain_name,
                    genesis_validators: genesis_res.genesis_validators,
                    genesis_balances: genesis_res.genesis_balances,
                });
            }
        }
    }

    /// Perform periodic maintenance
    fn perform_maintenance(&mut self) {
        // Clean up rate limiter
        self.rate_limiter.cleanup();

        // Peer manager tick
        self.peer_manager.tick();

        // Disconnect bad peers
        let to_disconnect = self.peer_manager.peers_to_disconnect();
        for peer_id in to_disconnect {
            debug!("Disconnecting peer {} due to low score or staleness", peer_id);
            let _ = self.swarm.disconnect_peer_id(peer_id);
        }

        // Try to connect to more peers if needed
        if self.peer_manager.needs_more_peers() {
            // Try bootstrap nodes
            for (peer_id, addr) in self.peer_manager.get_bootstrap_nodes().to_vec() {
                if self.peer_manager.get_peer(&peer_id).map(|p| !p.is_active()).unwrap_or(true) {
                    debug!("Attempting to connect to bootstrap node {}", peer_id);
                    let _ = self.swarm.dial(addr);
                }
            }

            // Try Kademlia bootstrap
            let _ = self.swarm.behaviour_mut().bootstrap_kad();
        }

        // Maybe start sync
        self.maybe_start_sync();

        // Log stats
        let stats = self.peer_manager.stats();
        debug!("Network: {} peers connected, best height: {}", stats.connected, stats.best_height);
    }

    // =========================================================================
    // QUERIES
    // =========================================================================

    /// Get number of connected peers
    pub fn peer_count(&self) -> usize {
        self.peer_manager.connected_count()
    }

    /// Get connected peer IDs
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peer_manager.connected_peers().iter().map(|p| p.id).collect()
    }

    /// Get peer stats
    pub fn peer_stats(&self) -> super::peer::PeerStats {
        self.peer_manager.stats()
    }

    /// Get sync state
    pub fn sync_state(&self) -> super::sync::SyncState {
        self.sync_manager.state()
    }

    /// Get sync gap
    pub fn sync_gap(&self) -> u64 {
        self.sync_manager.sync_gap()
    }

    /// Get banned peers
    pub fn banned_peers(&self) -> Vec<PeerId> {
        self.rate_limiter.banned_peers()
    }

    /// Ban a peer manually
    pub fn ban_peer(&mut self, peer_id: PeerId, reason: &str) {
        self.peer_manager.ban_peer(&peer_id, reason);
        self.rate_limiter.ban_peer(peer_id);
        let _ = self.swarm.disconnect_peer_id(peer_id);
        info!("Peer {} banned: {}", peer_id, reason);
    }

    /// Get local peer ID
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Cleanup rate limiter
    pub fn cleanup_rate_limiter(&mut self) {
        self.rate_limiter.cleanup();
    }

    /// Get banned peer list (for compatibility)
    pub fn get_banned_peers(&self) -> Vec<PeerId> {
        self.rate_limiter.banned_peers()
    }

    /// Buffer a block that arrived out of order
    /// The sync manager will hold it until it can be imported sequentially
    pub fn buffer_block(&mut self, block: Block) {
        debug!("Buffering out-of-order block #{}", block.header.number);
        self.sync_manager.add_downloaded_block(block);
    }

    /// Get the next buffered block to import (if any is sequential)
    pub fn next_buffered_block(&mut self, expected_height: BlockNumber) -> Option<Block> {
        // Check if we have the next sequential block
        self.sync_manager.next_block_to_import()
    }

    /// Update local height in sync manager
    pub fn update_sync_local_height(&mut self, height: BlockNumber) {
        self.sync_manager.update_local_height(height);
    }
}
