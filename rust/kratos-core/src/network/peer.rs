// Peer Management - Peer tracking, scoring, and connection management
// Principle: Track peer behavior, prioritize good actors, manage connections

use libp2p::PeerId;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

// =============================================================================
// CONSTANTS
// =============================================================================

/// Maximum number of peers to connect to
pub const MAX_PEERS: usize = 50;

/// Minimum number of peers for healthy operation
pub const MIN_PEERS: usize = 3;

/// Peer connection timeout
pub const PEER_TIMEOUT: Duration = Duration::from_secs(30);

/// Peer score decay interval
pub const SCORE_DECAY_INTERVAL: Duration = Duration::from_secs(60);

/// Initial peer score
pub const INITIAL_SCORE: i32 = 100;

/// Minimum score before disconnection
pub const MIN_SCORE: i32 = -100;

/// Score increase for good block
pub const GOOD_BLOCK_SCORE: i32 = 5;

/// Score decrease for invalid block
pub const BAD_BLOCK_SCORE: i32 = -50;

/// Score increase for good transaction
pub const GOOD_TX_SCORE: i32 = 1;

/// Score decrease for invalid transaction
pub const BAD_TX_SCORE: i32 = -10;

/// Score decrease for timeout
pub const TIMEOUT_SCORE: i32 = -20;

// =============================================================================
// PEER INFO
// =============================================================================

/// Peer connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// Attempting to connect
    Connecting,
    /// Connected and healthy
    Connected,
    /// Disconnected (may reconnect)
    Disconnected,
    /// Banned (will not reconnect)
    Banned,
}

/// Information about a peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer ID
    pub id: PeerId,

    /// Current state
    pub state: PeerState,

    /// Reputation score (higher is better)
    pub score: i32,

    /// Best known block height
    pub best_height: u64,

    /// When the peer was first seen
    pub first_seen: Instant,

    /// When the peer was last seen active
    pub last_seen: Instant,

    /// Number of blocks received from this peer
    pub blocks_received: u64,

    /// Number of transactions received from this peer
    pub transactions_received: u64,

    /// Number of failed requests
    pub failed_requests: u32,

    /// Number of invalid messages received
    pub invalid_messages: u32,

    /// Whether this is a bootstrap node
    pub is_bootstrap: bool,

    /// Multiaddrs for this peer
    pub addresses: Vec<libp2p::Multiaddr>,

    /// Latency estimate (ms)
    pub latency_ms: Option<u32>,
}

impl PeerInfo {
    /// Create a new peer info
    pub fn new(id: PeerId) -> Self {
        let now = Instant::now();
        Self {
            id,
            state: PeerState::Connecting,
            score: INITIAL_SCORE,
            best_height: 0,
            first_seen: now,
            last_seen: now,
            blocks_received: 0,
            transactions_received: 0,
            failed_requests: 0,
            invalid_messages: 0,
            is_bootstrap: false,
            addresses: Vec::new(),
            latency_ms: None,
        }
    }

    /// Create a bootstrap peer
    pub fn bootstrap(id: PeerId, addr: libp2p::Multiaddr) -> Self {
        let mut info = Self::new(id);
        info.is_bootstrap = true;
        info.addresses.push(addr);
        info
    }

    /// Update last seen timestamp
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Check if peer is active
    pub fn is_active(&self) -> bool {
        matches!(self.state, PeerState::Connected)
    }

    /// Check if peer should be disconnected
    pub fn should_disconnect(&self) -> bool {
        self.score < MIN_SCORE || matches!(self.state, PeerState::Banned)
    }

    /// Check if peer is stale (no activity for timeout)
    pub fn is_stale(&self) -> bool {
        self.last_seen.elapsed() > PEER_TIMEOUT
    }

    /// Record a good block
    pub fn good_block(&mut self) {
        self.score = self.score.saturating_add(GOOD_BLOCK_SCORE);
        self.blocks_received += 1;
        self.touch();
    }

    /// Record a bad block
    pub fn bad_block(&mut self) {
        self.score = self.score.saturating_sub(BAD_BLOCK_SCORE.abs());
        self.invalid_messages += 1;
        self.touch();
    }

    /// Record a good transaction
    pub fn good_transaction(&mut self) {
        self.score = self.score.saturating_add(GOOD_TX_SCORE);
        self.transactions_received += 1;
        self.touch();
    }

    /// Record a bad transaction
    pub fn bad_transaction(&mut self) {
        self.score = self.score.saturating_sub(BAD_TX_SCORE.abs());
        self.invalid_messages += 1;
        self.touch();
    }

    /// Record a timeout
    pub fn timeout(&mut self) {
        self.score = self.score.saturating_sub(TIMEOUT_SCORE.abs());
        self.failed_requests += 1;
    }

    /// Update best known height
    pub fn update_height(&mut self, height: u64) {
        if height > self.best_height {
            self.best_height = height;
            self.touch();
        }
    }
}

// =============================================================================
// PEER MANAGER
// =============================================================================

/// Manages peer connections and reputation
pub struct PeerManager {
    /// Known peers
    peers: HashMap<PeerId, PeerInfo>,

    /// Bootstrap nodes
    bootstrap_nodes: Vec<(PeerId, libp2p::Multiaddr)>,

    /// Maximum peers
    max_peers: usize,

    /// Last score decay time
    last_decay: Instant,
}

impl PeerManager {
    /// Create a new peer manager
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
            bootstrap_nodes: Vec::new(),
            max_peers: MAX_PEERS,
            last_decay: Instant::now(),
        }
    }

    /// Add bootstrap nodes
    pub fn add_bootstrap_nodes(&mut self, nodes: Vec<(PeerId, libp2p::Multiaddr)>) {
        for (peer_id, addr) in nodes {
            if !self.peers.contains_key(&peer_id) {
                let info = PeerInfo::bootstrap(peer_id, addr.clone());
                self.peers.insert(peer_id, info);
            }
            self.bootstrap_nodes.push((peer_id, addr));
        }
        info!("Added {} bootstrap nodes", self.bootstrap_nodes.len());
    }

    /// Get bootstrap nodes for connection
    pub fn get_bootstrap_nodes(&self) -> &[(PeerId, libp2p::Multiaddr)] {
        &self.bootstrap_nodes
    }

    /// Register a new peer connection
    pub fn peer_connected(&mut self, peer_id: PeerId) {
        let info = self.peers.entry(peer_id).or_insert_with(|| PeerInfo::new(peer_id));
        info.state = PeerState::Connected;
        info.touch();
        debug!("Peer connected: {}", peer_id);
    }

    /// Handle peer disconnection
    pub fn peer_disconnected(&mut self, peer_id: &PeerId) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            info.state = PeerState::Disconnected;
            debug!("Peer disconnected: {}", peer_id);
        }
    }

    /// Add address for a peer
    pub fn add_peer_address(&mut self, peer_id: PeerId, addr: libp2p::Multiaddr) {
        let info = self.peers.entry(peer_id).or_insert_with(|| PeerInfo::new(peer_id));
        if !info.addresses.contains(&addr) {
            info.addresses.push(addr);
        }
    }

    /// Get peer info
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    /// Get peer info mutably
    pub fn get_peer_mut(&mut self, peer_id: &PeerId) -> Option<&mut PeerInfo> {
        self.peers.get_mut(peer_id)
    }

    /// Get all connected peers
    pub fn connected_peers(&self) -> Vec<&PeerInfo> {
        self.peers.values()
            .filter(|p| p.is_active())
            .collect()
    }

    /// Count connected peers
    pub fn connected_count(&self) -> usize {
        self.peers.values().filter(|p| p.is_active()).count()
    }

    /// Check if a specific peer is connected
    pub fn is_connected(&self, peer_id: &PeerId) -> bool {
        self.peers.get(peer_id).map(|p| p.is_active()).unwrap_or(false)
    }

    /// Check if we need more peers
    pub fn needs_more_peers(&self) -> bool {
        self.connected_count() < MIN_PEERS
    }

    /// Check if we can accept more peers
    pub fn can_accept_peer(&self) -> bool {
        self.connected_count() < self.max_peers
    }

    /// Get best peer for sync (highest score among those with blocks)
    pub fn best_sync_peer(&self) -> Option<&PeerInfo> {
        self.peers.values()
            .filter(|p| p.is_active() && p.best_height > 0)
            .max_by_key(|p| (p.best_height, p.score))
    }

    /// Get peers to disconnect (low score or stale)
    pub fn peers_to_disconnect(&self) -> Vec<PeerId> {
        self.peers.values()
            .filter(|p| p.should_disconnect() || (p.is_active() && p.is_stale()))
            .map(|p| p.id)
            .collect()
    }

    /// Ban a peer
    pub fn ban_peer(&mut self, peer_id: &PeerId, reason: &str) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            info.state = PeerState::Banned;
            info.score = MIN_SCORE - 1;
            warn!("Peer {} banned: {}", peer_id, reason);
        }
    }

    /// Update peer's best height
    pub fn update_peer_height(&mut self, peer_id: &PeerId, height: u64) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            info.update_height(height);
        }
    }

    /// Record good block from peer
    pub fn record_good_block(&mut self, peer_id: &PeerId) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            info.good_block();
        }
    }

    /// Record bad block from peer
    pub fn record_bad_block(&mut self, peer_id: &PeerId) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            info.bad_block();
        }
    }

    /// Record good transaction from peer
    pub fn record_good_transaction(&mut self, peer_id: &PeerId) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            info.good_transaction();
        }
    }

    /// Record bad transaction from peer
    pub fn record_bad_transaction(&mut self, peer_id: &PeerId) {
        if let Some(info) = self.peers.get_mut(peer_id) {
            info.bad_transaction();
        }
    }

    /// Perform periodic maintenance
    pub fn tick(&mut self) {
        // Decay scores periodically
        if self.last_decay.elapsed() > SCORE_DECAY_INTERVAL {
            self.decay_scores();
            self.last_decay = Instant::now();
        }

        // Clean up very old disconnected peers
        self.cleanup_stale_peers();
    }

    /// Decay all peer scores toward baseline
    fn decay_scores(&mut self) {
        for info in self.peers.values_mut() {
            if info.score > INITIAL_SCORE {
                info.score -= 1;
            } else if info.score < INITIAL_SCORE {
                info.score += 1;
            }
        }
    }

    /// Remove very old disconnected peers (keep bootstrap)
    fn cleanup_stale_peers(&mut self) {
        let stale_threshold = Duration::from_secs(3600); // 1 hour
        let to_remove: Vec<PeerId> = self.peers.iter()
            .filter(|(_, info)| {
                !info.is_bootstrap
                    && matches!(info.state, PeerState::Disconnected)
                    && info.last_seen.elapsed() > stale_threshold
            })
            .map(|(id, _)| *id)
            .collect();

        for peer_id in to_remove {
            self.peers.remove(&peer_id);
            debug!("Removed stale peer: {}", peer_id);
        }
    }

    /// Get best known network height
    pub fn best_network_height(&self) -> u64 {
        self.peers.values()
            .filter(|p| p.is_active())
            .map(|p| p.best_height)
            .max()
            .unwrap_or(0)
    }

    /// Get statistics
    pub fn stats(&self) -> PeerStats {
        let connected = self.connected_count();
        let total = self.peers.len();
        let avg_score = if connected > 0 {
            self.connected_peers().iter().map(|p| p.score as i64).sum::<i64>() / connected as i64
        } else {
            0
        };

        PeerStats {
            connected,
            total,
            average_score: avg_score as i32,
            best_height: self.best_network_height(),
        }
    }
}

impl Default for PeerManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Peer statistics
#[derive(Debug, Clone)]
pub struct PeerStats {
    pub connected: usize,
    pub total: usize,
    pub average_score: i32,
    pub best_height: u64,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::PeerId;

    fn create_peer_id(seed: u8) -> PeerId {
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        PeerId::from(keypair.public())
    }

    #[test]
    fn test_peer_info_creation() {
        let peer_id = create_peer_id(1);
        let info = PeerInfo::new(peer_id);

        assert_eq!(info.score, INITIAL_SCORE);
        assert_eq!(info.state, PeerState::Connecting);
        assert_eq!(info.best_height, 0);
    }

    #[test]
    fn test_peer_score_changes() {
        let peer_id = create_peer_id(1);
        let mut info = PeerInfo::new(peer_id);

        info.good_block();
        assert_eq!(info.score, INITIAL_SCORE + GOOD_BLOCK_SCORE);
        assert_eq!(info.blocks_received, 1);

        info.bad_block();
        assert!(info.score < INITIAL_SCORE); // Went below initial
    }

    #[test]
    fn test_peer_manager_connection() {
        let mut manager = PeerManager::new();
        let peer_id = create_peer_id(1);

        manager.peer_connected(peer_id);
        assert_eq!(manager.connected_count(), 1);

        manager.peer_disconnected(&peer_id);
        assert_eq!(manager.connected_count(), 0);
    }

    #[test]
    fn test_peer_manager_needs_peers() {
        let manager = PeerManager::new();
        assert!(manager.needs_more_peers());
        assert!(manager.can_accept_peer());
    }

    #[test]
    fn test_peer_ban() {
        let mut manager = PeerManager::new();
        let peer_id = create_peer_id(1);

        manager.peer_connected(peer_id);
        manager.ban_peer(&peer_id, "test ban");

        let info = manager.get_peer(&peer_id).unwrap();
        assert_eq!(info.state, PeerState::Banned);
        assert!(info.should_disconnect());
    }

    #[test]
    fn test_best_sync_peer() {
        let mut manager = PeerManager::new();

        let peer1 = create_peer_id(1);
        let peer2 = create_peer_id(2);

        manager.peer_connected(peer1);
        manager.peer_connected(peer2);

        manager.update_peer_height(&peer1, 100);
        manager.update_peer_height(&peer2, 200);

        let best = manager.best_sync_peer().unwrap();
        assert_eq!(best.best_height, 200);
    }

    #[test]
    fn test_peer_stats() {
        let mut manager = PeerManager::new();
        let peer_id = create_peer_id(1);

        manager.peer_connected(peer_id);
        manager.update_peer_height(&peer_id, 100);

        let stats = manager.stats();
        assert_eq!(stats.connected, 1);
        assert_eq!(stats.best_height, 100);
    }
}
