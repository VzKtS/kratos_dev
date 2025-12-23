//! Peer Registry Storage (RocksDB)
//!
//! Persistent storage for peer information.
//! Survives DNS Seed restarts to maintain peer knowledge.

use rocksdb::{DB, Options, IteratorMode};
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info, warn};

use crate::types::{PeerId, PeerInfo, BlockNumber};
use super::RegistryStats;

/// Key prefixes for different data types
const PREFIX_PEER: &[u8] = b"peer:";
const PREFIX_META: &[u8] = b"meta:";

/// Metadata keys
const META_BEST_HEIGHT: &[u8] = b"meta:best_height";
const META_PEER_COUNT: &[u8] = b"meta:peer_count";

/// Peer registry backed by RocksDB
pub struct PeerRegistry {
    /// RocksDB instance
    db: DB,

    /// In-memory cache for fast access
    cache: HashMap<PeerId, PeerInfo>,

    /// Maximum peers to store
    max_peers: usize,

    /// Best known block height
    best_height: BlockNumber,
}

impl PeerRegistry {
    /// Open or create a peer registry at the given path
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_open_files(100);
        opts.set_keep_log_file_num(3);

        let db = DB::open(&opts, path)?;

        // Load existing peers into cache
        let mut cache = HashMap::new();
        let mut best_height = 0u64;

        let iter = db.iterator(IteratorMode::From(PREFIX_PEER, rocksdb::Direction::Forward));

        for item in iter {
            let (key, value) = item?;

            // Check if this is a peer entry
            if !key.starts_with(PREFIX_PEER) {
                break;
            }

            // Deserialize peer info
            if let Ok(peer_info) = bincode::deserialize::<PeerInfo>(&value) {
                if peer_info.height > best_height {
                    best_height = peer_info.height;
                }
                cache.insert(peer_info.peer_id, peer_info);
            }
        }

        info!("📦 Loaded {} peers from registry, best height {}", cache.len(), best_height);

        Ok(Self {
            db,
            cache,
            max_peers: 10000,
            best_height,
        })
    }

    /// Update or insert a peer
    pub fn update_peer(&mut self, peer: PeerInfo) {
        let peer_id = peer.peer_id;

        // Update best height
        if peer.height > self.best_height {
            self.best_height = peer.height;
        }

        // Update score if peer exists
        let final_peer = if let Some(existing) = self.cache.get(&peer_id) {
            PeerInfo {
                score: (existing.score + 1).min(200), // Increment score, cap at 200
                ..peer
            }
        } else {
            peer
        };

        // Persist to RocksDB
        let key = peer_key(&peer_id);
        if let Ok(value) = bincode::serialize(&final_peer) {
            if let Err(e) = self.db.put(&key, &value) {
                warn!("Failed to persist peer: {}", e);
            }
        }

        // Update cache
        self.cache.insert(peer_id, final_peer);

        // Evict if over capacity
        if self.cache.len() > self.max_peers {
            self.evict_lowest_scoring_peer();
        }
    }

    /// Remove a peer
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        let key = peer_key(peer_id);
        let _ = self.db.delete(&key);
        self.cache.remove(peer_id);
    }

    /// Get a peer by ID
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.cache.get(peer_id)
    }

    /// Get all active peers (not stale)
    pub fn get_active_peers(&self, timeout_secs: u64) -> Vec<&PeerInfo> {
        self.cache
            .values()
            .filter(|p| !p.is_stale(timeout_secs))
            .collect()
    }

    /// Get top N peers by score
    pub fn get_top_peers(&self, n: usize, timeout_secs: u64) -> Vec<&PeerInfo> {
        let mut peers: Vec<_> = self.get_active_peers(timeout_secs);
        peers.sort_by(|a, b| b.score.cmp(&a.score));
        peers.into_iter().take(n).collect()
    }

    /// Get peers with geographic diversity
    pub fn get_diverse_peers(&self, n: usize, min_regions: usize, timeout_secs: u64) -> Vec<&PeerInfo> {
        let mut result = Vec::new();
        let mut regions_seen: HashMap<String, usize> = HashMap::new();

        // First pass: get peers from different regions
        let mut all_peers: Vec<_> = self.get_active_peers(timeout_secs);
        all_peers.sort_by(|a, b| b.score.cmp(&a.score));

        for peer in &all_peers {
            let region = peer.region.clone().unwrap_or_else(|| "unknown".to_string());
            let count = regions_seen.entry(region.clone()).or_insert(0);

            // Limit peers per region initially
            if *count < n / min_regions.max(1) {
                result.push(*peer);
                *count += 1;

                if result.len() >= n {
                    break;
                }
            }
        }

        // Second pass: fill remaining slots with highest scoring
        if result.len() < n {
            for peer in &all_peers {
                if !result.iter().any(|p| p.peer_id == peer.peer_id) {
                    result.push(*peer);
                    if result.len() >= n {
                        break;
                    }
                }
            }
        }

        result
    }

    /// Remove stale peers (haven't been seen recently)
    pub fn remove_stale_peers(&mut self, timeout_secs: u64) -> usize {
        let stale_ids: Vec<PeerId> = self.cache
            .iter()
            .filter(|(_, p)| p.is_stale(timeout_secs))
            .map(|(id, _)| *id)
            .collect();

        let count = stale_ids.len();

        for id in stale_ids {
            self.remove_peer(&id);
        }

        if count > 0 {
            debug!("Removed {} stale peers", count);
        }

        count
    }

    /// Get count of active peers
    pub fn active_peer_count(&self) -> usize {
        // Use 4 minutes as default timeout (2 missed heartbeats at 2min interval)
        self.cache.values().filter(|p| !p.is_stale(240)).count()
    }

    /// Get total peer count
    pub fn total_peer_count(&self) -> usize {
        self.cache.len()
    }

    /// Get validator count
    pub fn validator_count(&self, timeout_secs: u64) -> usize {
        self.cache
            .values()
            .filter(|p| p.is_validator && !p.is_stale(timeout_secs))
            .count()
    }

    /// Get best known block height
    pub fn best_height(&self) -> BlockNumber {
        self.best_height
    }

    /// Get registry statistics
    pub fn stats(&self, timeout_secs: u64) -> RegistryStats {
        let active_peers: Vec<_> = self.get_active_peers(timeout_secs);

        let validator_count = active_peers.iter().filter(|p| p.is_validator).count();

        let best_height = active_peers
            .iter()
            .map(|p| p.height)
            .max()
            .unwrap_or(0);

        let average_score = if active_peers.is_empty() {
            0.0
        } else {
            active_peers.iter().map(|p| p.score as f64).sum::<f64>() / active_peers.len() as f64
        };

        let unique_regions = active_peers
            .iter()
            .filter_map(|p| p.region.as_ref())
            .collect::<std::collections::HashSet<_>>()
            .len();

        RegistryStats {
            total_peers: self.cache.len(),
            active_peers: active_peers.len(),
            validator_count,
            best_height,
            average_score,
            unique_regions,
        }
    }

    /// Decrease score for a peer (e.g., timeout)
    pub fn decrease_peer_score(&mut self, peer_id: &PeerId, amount: i32) {
        if let Some(peer) = self.cache.get_mut(peer_id) {
            peer.score = (peer.score - amount).max(0);

            // Persist change
            let key = peer_key(peer_id);
            if let Ok(value) = bincode::serialize(peer) {
                let _ = self.db.put(&key, &value);
            }
        }
    }

    /// Evict the lowest scoring peer
    fn evict_lowest_scoring_peer(&mut self) {
        if let Some((&peer_id, _)) = self.cache
            .iter()
            .min_by_key(|(_, p)| p.score)
        {
            debug!("Evicting peer {} due to capacity", hex::encode(&peer_id[..8]));
            self.remove_peer(&peer_id);
        }
    }

    /// Flush all changes to disk
    pub fn flush(&self) -> anyhow::Result<()> {
        self.db.flush()?;
        Ok(())
    }
}

/// Create storage key for a peer
fn peer_key(peer_id: &PeerId) -> Vec<u8> {
    let mut key = Vec::with_capacity(PREFIX_PEER.len() + 32);
    key.extend_from_slice(PREFIX_PEER);
    key.extend_from_slice(peer_id);
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_peer(id: u8, height: u64, is_validator: bool) -> PeerInfo {
        let mut peer_id = [0u8; 32];
        peer_id[0] = id;

        PeerInfo {
            peer_id,
            addresses: vec![format!("/ip4/192.168.1.{}/tcp/30333", id)],
            last_seen: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            height,
            is_validator,
            score: 100,
            region: Some(if id % 2 == 0 { "EU".to_string() } else { "US".to_string() }),
            protocol_version: 1,
        }
    }

    #[test]
    fn test_registry_open() {
        let dir = tempdir().unwrap();
        let registry = PeerRegistry::open(dir.path()).unwrap();
        assert_eq!(registry.total_peer_count(), 0);
    }

    #[test]
    fn test_add_and_get_peer() {
        let dir = tempdir().unwrap();
        let mut registry = PeerRegistry::open(dir.path()).unwrap();

        let peer = create_test_peer(1, 1000, true);
        let peer_id = peer.peer_id;

        registry.update_peer(peer);

        assert_eq!(registry.total_peer_count(), 1);
        assert!(registry.get_peer(&peer_id).is_some());
    }

    #[test]
    fn test_best_height() {
        let dir = tempdir().unwrap();
        let mut registry = PeerRegistry::open(dir.path()).unwrap();

        registry.update_peer(create_test_peer(1, 100, false));
        registry.update_peer(create_test_peer(2, 500, false));
        registry.update_peer(create_test_peer(3, 300, false));

        assert_eq!(registry.best_height(), 500);
    }

    #[test]
    fn test_validator_count() {
        let dir = tempdir().unwrap();
        let mut registry = PeerRegistry::open(dir.path()).unwrap();

        registry.update_peer(create_test_peer(1, 100, true));
        registry.update_peer(create_test_peer(2, 100, false));
        registry.update_peer(create_test_peer(3, 100, true));

        assert_eq!(registry.validator_count(240), 2);
    }

    #[test]
    fn test_top_peers() {
        let dir = tempdir().unwrap();
        let mut registry = PeerRegistry::open(dir.path()).unwrap();

        // Add peers with different scores
        for i in 0..10 {
            let mut peer = create_test_peer(i, 100, false);
            peer.score = (i as i32) * 10;
            registry.update_peer(peer);
        }

        let top = registry.get_top_peers(3, 240);
        assert_eq!(top.len(), 3);
        // Highest score should be first (after update bonus)
        assert!(top[0].score >= top[1].score);
    }

    #[test]
    fn test_persistence() {
        let dir = tempdir().unwrap();

        // Add peer and close
        {
            let mut registry = PeerRegistry::open(dir.path()).unwrap();
            registry.update_peer(create_test_peer(1, 1000, true));
            registry.flush().unwrap();
        }

        // Reopen and verify
        {
            let registry = PeerRegistry::open(dir.path()).unwrap();
            assert_eq!(registry.total_peer_count(), 1);
        }
    }
}
