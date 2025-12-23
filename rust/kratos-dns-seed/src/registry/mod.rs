//! Peer Registry Module
//!
//! Stores peer information in RocksDB for persistence across restarts.
//! Tracks peer health scores, last seen times, and network metadata.

mod storage;
pub mod scoring;

pub use storage::PeerRegistry;

/// Result of a registry query
#[derive(Debug, Clone)]
pub struct RegistryStats {
    /// Total peers in registry
    pub total_peers: usize,

    /// Active peers (not stale)
    pub active_peers: usize,

    /// Number of validators
    pub validator_count: usize,

    /// Best known block height
    pub best_height: u64,

    /// Average peer score
    pub average_score: f64,

    /// Number of unique regions
    pub unique_regions: usize,
}

impl Default for RegistryStats {
    fn default() -> Self {
        Self {
            total_peers: 0,
            active_peers: 0,
            validator_count: 0,
            best_height: 0,
            average_score: 0.0,
            unique_regions: 0,
        }
    }
}
