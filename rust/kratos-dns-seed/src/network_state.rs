//! Network State Aggregator
//!
//! Aggregates peer information to compute overall network health metrics.
//! Used to determine security state and adjust protocol parameters.

use tracing::{debug, info};

use crate::types::{
    Balance, BlockNumber, Hash, NetworkStateInfo, PeerInfo, SecurityState,
};

/// Network state aggregator
///
/// Computes network-wide metrics from individual peer reports.
/// Uses median/consensus-based aggregation to resist malicious reports.
pub struct NetworkStateAggregator {
    /// Genesis hash for this network
    genesis_hash: Hash,

    /// Whether we're still in bootstrap phase
    is_bootstrap: bool,

    /// Bootstrap end timestamp (60 days after genesis)
    bootstrap_end: u64,

    /// Last computed network state
    current_state: NetworkStateInfo,

    /// Historical validator counts for trend analysis
    validator_history: Vec<(u64, u32)>,

    /// Maximum history entries to keep
    max_history: usize,
}

impl NetworkStateAggregator {
    /// Create a new network state aggregator
    pub fn new(genesis_hash: Hash, genesis_timestamp: u64) -> Self {
        // Bootstrap phase lasts 60 days
        let bootstrap_duration = 60 * 24 * 60 * 60; // 60 days in seconds
        let bootstrap_end = genesis_timestamp + bootstrap_duration;

        let now = current_timestamp();
        let is_bootstrap = now < bootstrap_end;

        Self {
            genesis_hash,
            is_bootstrap,
            bootstrap_end,
            current_state: NetworkStateInfo {
                genesis_hash,
                best_height: 0,
                active_validators: 0,
                security_state: SecurityState::Bootstrap,
                total_stake: 0,
                participation_rate: 0.0,
                estimated_inflation: 0.065, // Bootstrap inflation
                active_peers: 0,
                timestamp: now,
            },
            validator_history: Vec::new(),
            max_history: 1000,
        }
    }

    /// Update network state from peer information
    pub fn update_from_peers(&mut self, peers: &[&PeerInfo]) {
        let now = current_timestamp();

        // Check if bootstrap phase ended
        if self.is_bootstrap && now >= self.bootstrap_end {
            self.is_bootstrap = false;
            info!("🎉 Bootstrap phase ended, entering normal operation");
        }

        // Compute aggregated metrics
        let active_peers = peers.len() as u32;

        // Best height: use median to resist outliers
        let best_height = self.compute_median_height(peers);

        // Validator count: count unique validators
        let validators: Vec<_> = peers.iter().filter(|p| p.is_validator).collect();
        let active_validators = validators.len() as u32;

        // Total stake: use median of reported stakes
        let total_stake = self.compute_median_stake(peers);

        // Participation rate: validators / expected validators
        // During bootstrap, expected is 500. After, it's governance-set
        let expected_validators = if self.is_bootstrap { 500.0 } else { 1000.0 };
        let participation_rate = (active_validators as f64 / expected_validators).min(1.0);

        // Determine security state
        let security_state = SecurityState::from_validator_count(
            active_validators,
            self.is_bootstrap,
        );

        // Compute estimated inflation based on security state
        let estimated_inflation = self.compute_inflation(
            &security_state,
            participation_rate,
        );

        // Update current state
        self.current_state = NetworkStateInfo {
            genesis_hash: self.genesis_hash,
            best_height,
            active_validators,
            security_state,
            total_stake,
            participation_rate,
            estimated_inflation,
            active_peers,
            timestamp: now,
        };

        // Record history
        self.validator_history.push((now, active_validators));
        if self.validator_history.len() > self.max_history {
            self.validator_history.remove(0);
        }

        debug!(
            "Network state updated: {} peers, {} validators, height {}, security: {:?}",
            active_peers, active_validators, best_height, security_state
        );
    }

    /// Get current network state
    pub fn current_state(&self) -> NetworkStateInfo {
        self.current_state.clone()
    }

    /// Get security state
    pub fn security_state(&self) -> SecurityState {
        self.current_state.security_state
    }

    /// Check if network is in emergency mode
    pub fn is_emergency(&self) -> bool {
        matches!(self.current_state.security_state, SecurityState::Emergency)
    }

    /// Check if network is degraded
    pub fn is_degraded(&self) -> bool {
        matches!(
            self.current_state.security_state,
            SecurityState::Degraded | SecurityState::Restricted | SecurityState::Emergency
        )
    }

    /// Get validator trend (positive = growing, negative = shrinking)
    pub fn validator_trend(&self) -> i32 {
        if self.validator_history.len() < 10 {
            return 0;
        }

        let recent: Vec<_> = self.validator_history.iter().rev().take(10).collect();
        let older: Vec<_> = self.validator_history.iter().rev().skip(10).take(10).collect();

        if older.is_empty() {
            return 0;
        }

        let recent_avg: f64 = recent.iter().map(|(_, v)| *v as f64).sum::<f64>() / recent.len() as f64;
        let older_avg: f64 = older.iter().map(|(_, v)| *v as f64).sum::<f64>() / older.len() as f64;

        (recent_avg - older_avg).round() as i32
    }

    /// Compute median block height from peer reports
    fn compute_median_height(&self, peers: &[&PeerInfo]) -> BlockNumber {
        if peers.is_empty() {
            return 0;
        }

        let mut heights: Vec<_> = peers.iter().map(|p| p.height).collect();
        heights.sort();

        heights[heights.len() / 2]
    }

    /// Compute median stake from peer reports
    fn compute_median_stake(&self, peers: &[&PeerInfo]) -> Balance {
        // Peers don't directly report stake, so we estimate from validator count
        // Using 32,000 KRAT minimum stake per validator
        let validator_count = peers.iter().filter(|p| p.is_validator).count() as u128;
        let min_stake_per_validator: u128 = 32_000 * 1_000_000_000_000; // 32,000 KRAT in base units

        validator_count * min_stake_per_validator
    }

    /// Compute inflation rate based on security state and participation
    fn compute_inflation(
        &self,
        security_state: &SecurityState,
        participation_rate: f64,
    ) -> f64 {
        match security_state {
            SecurityState::Bootstrap => {
                // Fixed 6.5% during bootstrap
                0.065
            }
            SecurityState::Normal => {
                // Adaptive inflation based on participation
                // Target: 5% at 50% participation, scales down as participation increases
                let base_inflation = 0.05;
                let participation_factor = 1.0 - (participation_rate * 0.5);
                base_inflation * participation_factor.max(0.5)
            }
            SecurityState::Degraded => {
                // +1% boost to attract validators
                let base = self.compute_inflation(&SecurityState::Normal, participation_rate);
                base + 0.01
            }
            SecurityState::Restricted => {
                // +2% boost
                let base = self.compute_inflation(&SecurityState::Normal, participation_rate);
                base + 0.02
            }
            SecurityState::Emergency => {
                // +3% boost, maximum incentive to attract validators
                let base = self.compute_inflation(&SecurityState::Normal, participation_rate);
                base + 0.03
            }
        }
    }

    /// Get genesis hash
    pub fn genesis_hash(&self) -> Hash {
        self.genesis_hash
    }

    /// Check if we're in bootstrap phase
    pub fn is_bootstrap(&self) -> bool {
        self.is_bootstrap
    }
}

/// Get current Unix timestamp
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_peer(id: u8, height: u64, is_validator: bool) -> PeerInfo {
        let mut peer_id = [0u8; 32];
        peer_id[0] = id;

        PeerInfo {
            peer_id,
            addresses: vec![format!("/ip4/192.168.1.{}/tcp/30333", id)],
            last_seen: current_timestamp(),
            height,
            is_validator,
            score: 100,
            region: None,
            protocol_version: 1,
        }
    }

    #[test]
    fn test_aggregator_creation() {
        let genesis_hash = [1u8; 32];
        let genesis_ts = current_timestamp() - 1000;

        let aggregator = NetworkStateAggregator::new(genesis_hash, genesis_ts);

        assert!(aggregator.is_bootstrap());
        assert_eq!(aggregator.genesis_hash(), genesis_hash);
    }

    #[test]
    fn test_update_from_peers() {
        let genesis_hash = [1u8; 32];
        let genesis_ts = current_timestamp() - 1000;

        let mut aggregator = NetworkStateAggregator::new(genesis_hash, genesis_ts);

        let peers: Vec<PeerInfo> = (0..100)
            .map(|i| create_test_peer(i, 1000 + i as u64, i < 80))
            .collect();

        let peer_refs: Vec<&PeerInfo> = peers.iter().collect();
        aggregator.update_from_peers(&peer_refs);

        let state = aggregator.current_state();
        assert_eq!(state.active_peers, 100);
        assert_eq!(state.active_validators, 80);
        assert!(state.best_height >= 1000);
    }

    #[test]
    fn test_security_states() {
        let genesis_hash = [1u8; 32];
        // Set genesis far in the past to exit bootstrap
        let genesis_ts = current_timestamp() - (61 * 24 * 60 * 60);

        let mut aggregator = NetworkStateAggregator::new(genesis_hash, genesis_ts);

        // Test Emergency (< 25 validators)
        let peers: Vec<PeerInfo> = (0..20)
            .map(|i| create_test_peer(i, 1000, true))
            .collect();
        let peer_refs: Vec<&PeerInfo> = peers.iter().collect();
        aggregator.update_from_peers(&peer_refs);
        assert!(aggregator.is_emergency());

        // Test Normal (>= 75 validators)
        let peers: Vec<PeerInfo> = (0..100)
            .map(|i| create_test_peer(i, 1000, true))
            .collect();
        let peer_refs: Vec<&PeerInfo> = peers.iter().collect();
        aggregator.update_from_peers(&peer_refs);
        assert_eq!(aggregator.security_state(), SecurityState::Normal);
    }

    #[test]
    fn test_inflation_computation() {
        let genesis_hash = [1u8; 32];
        let genesis_ts = current_timestamp() - 1000;

        let aggregator = NetworkStateAggregator::new(genesis_hash, genesis_ts);

        // Bootstrap inflation
        let bootstrap_inflation = aggregator.compute_inflation(
            &SecurityState::Bootstrap,
            0.5,
        );
        assert!((bootstrap_inflation - 0.065).abs() < 0.001);

        // Normal inflation at 50% participation
        let normal_inflation = aggregator.compute_inflation(
            &SecurityState::Normal,
            0.5,
        );
        assert!(normal_inflation < 0.065);

        // Emergency adds +3%
        let emergency_inflation = aggregator.compute_inflation(
            &SecurityState::Emergency,
            0.5,
        );
        assert!(emergency_inflation > normal_inflation);
    }

    #[test]
    fn test_median_height() {
        let genesis_hash = [1u8; 32];
        let genesis_ts = current_timestamp() - 1000;

        let aggregator = NetworkStateAggregator::new(genesis_hash, genesis_ts);

        let peers: Vec<PeerInfo> = vec![
            create_test_peer(1, 100, false),
            create_test_peer(2, 200, false),
            create_test_peer(3, 300, false),
            create_test_peer(4, 1000, false), // Outlier
        ];

        let peer_refs: Vec<&PeerInfo> = peers.iter().collect();
        let median = aggregator.compute_median_height(&peer_refs);

        // Median of [100, 200, 300, 1000] = 200 or 300 (depending on even/odd handling)
        assert!(median >= 200 && median <= 300);
    }
}
