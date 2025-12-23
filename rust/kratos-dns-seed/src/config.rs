//! DNS Seed Configuration
//!
//! Configurable parameters for the DNS Seed service.
//! Default values are chosen to balance security, resilience, and performance.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Main configuration for the DNS Seed service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsSeedConfig {
    // === Timing ===

    /// Interval between node heartbeats (seconds)
    /// Nodes must send heartbeat within this interval
    pub heartbeat_interval_secs: u64,

    /// Time after which a peer is considered stale (seconds)
    /// Should be 2x heartbeat_interval to allow one missed heartbeat
    pub peer_timeout_secs: u64,

    /// Interval for updating IDpeers.json file (seconds)
    pub peers_file_update_secs: u64,

    /// Interval for maintenance tasks (seconds)
    pub maintenance_interval_secs: u64,

    // === Limits ===

    /// Maximum peers to return in DNS response
    pub max_peers_in_dns_response: usize,

    /// Maximum peers to include in IDpeers.json
    pub max_peers_in_file: usize,

    /// Minimum peer score to be included in responses
    pub min_peer_score: i32,

    /// Maximum number of peers to track in registry
    pub max_peers_in_registry: usize,

    /// Maximum peers to include in IDpeers.json
    pub max_peers_in_idpeers: usize,

    /// Minimum regions to include in IDpeers.json
    pub min_regions_in_idpeers: usize,

    /// IDpeers.json update interval (seconds)
    pub idpeers_update_interval_secs: u64,

    // === Network ===

    /// Port for heartbeat receiver (TCP)
    pub heartbeat_port: u16,

    /// Port for DNS server
    pub dns_port: u16,

    /// Port for HTTP API
    pub api_port: u16,

    /// Port for HTTP distribution server (IDpeers.json)
    pub http_port: u16,

    /// DNS domain name for this seed
    pub dns_domain: String,

    /// Expected genesis hash (hex string, optional)
    pub genesis_hash: Option<String>,

    // === Security ===

    /// Require signed heartbeats from nodes
    pub require_signed_heartbeats: bool,

    /// Maximum heartbeat requests per IP per minute (rate limiting)
    pub rate_limit_per_minute: u32,

    /// Ban duration for misbehaving IPs (seconds)
    pub ban_duration_secs: u64,

    /// Maximum violations before ban
    pub max_violations_before_ban: u32,

    // === Scoring ===

    /// Initial score for new peers
    pub initial_peer_score: i32,

    /// Score bonus for successful heartbeat
    pub heartbeat_score_bonus: i32,

    /// Score penalty for timeout
    pub timeout_score_penalty: i32,

    /// Score bonus for being a validator
    pub validator_score_bonus: i32,

    /// Maximum peer score
    pub max_peer_score: i32,

    /// Minimum peer score (disconnect threshold)
    pub min_peer_score_threshold: i32,

    // === Geographic Diversity ===

    /// Minimum number of different regions in responses
    pub min_regions_in_response: usize,

    /// Enable GeoIP-based region detection
    pub enable_geoip: bool,

    // === Official Seeds (for governance integration) ===

    /// List of official DNS Seed IDs (hex-encoded public keys)
    /// Will be populated via governance in the future
    pub official_seed_ids: Vec<String>,

    /// Fallback bootnodes (always included)
    pub fallback_bootnodes: Vec<String>,
}

impl Default for DnsSeedConfig {
    fn default() -> Self {
        Self {
            // Timing - 2 minute heartbeat as specified
            heartbeat_interval_secs: 120,      // 2 minutes
            peer_timeout_secs: 240,            // 4 minutes (2 missed heartbeats)
            peers_file_update_secs: 60,        // 1 minute
            maintenance_interval_secs: 30,     // 30 seconds

            // Limits
            max_peers_in_dns_response: 25,
            max_peers_in_file: 100,
            min_peer_score: 50,
            max_peers_in_registry: 10000,
            max_peers_in_idpeers: 100,
            min_regions_in_idpeers: 3,
            idpeers_update_interval_secs: 60,

            // Network
            heartbeat_port: 30334,
            dns_port: 5353,  // Use 53 in production with proper permissions
            api_port: 8080,
            http_port: 8080,  // Same as API by default
            dns_domain: "seed.kratos.network".to_string(),
            genesis_hash: None,

            // Security
            require_signed_heartbeats: true,
            rate_limit_per_minute: 30,  // Allow some burst for reconnections
            ban_duration_secs: 3600,    // 1 hour
            max_violations_before_ban: 5,

            // Scoring
            initial_peer_score: 100,
            heartbeat_score_bonus: 1,
            timeout_score_penalty: 10,
            validator_score_bonus: 20,
            max_peer_score: 200,
            min_peer_score_threshold: 0,

            // Geographic diversity
            min_regions_in_response: 2,
            enable_geoip: false,  // Requires GeoIP database

            // Official seeds (empty until governance)
            official_seed_ids: vec![],

            // Fallback bootnodes (current KratOs bootstrap)
            fallback_bootnodes: vec![
                "/ip4/45.8.132.252/tcp/30333/p2p/12D3KooWQqYkkyLGuFS6YZprPShuVhn8Wrc1PUxbJ8pRisAYLndK".to_string(),
            ],
        }
    }
}

impl DnsSeedConfig {
    /// Load configuration from TOML file
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to TOML file
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    // Builder-style methods for CLI overrides

    pub fn with_heartbeat_port(mut self, port: u16) -> Self {
        self.heartbeat_port = port;
        self
    }

    pub fn with_dns_port(mut self, port: u16) -> Self {
        self.dns_port = port;
        self
    }

    pub fn with_api_port(mut self, port: u16) -> Self {
        self.api_port = port;
        self
    }

    pub fn with_genesis_hash(mut self, hash: Option<String>) -> Self {
        self.genesis_hash = hash;
        self
    }

    /// Validate configuration values
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.peer_timeout_secs <= self.heartbeat_interval_secs {
            anyhow::bail!(
                "peer_timeout_secs ({}) must be greater than heartbeat_interval_secs ({})",
                self.peer_timeout_secs,
                self.heartbeat_interval_secs
            );
        }

        if self.max_peers_in_dns_response > self.max_peers_in_file {
            anyhow::bail!(
                "max_peers_in_dns_response ({}) should not exceed max_peers_in_file ({})",
                self.max_peers_in_dns_response,
                self.max_peers_in_file
            );
        }

        if self.min_peer_score >= self.initial_peer_score {
            anyhow::bail!(
                "min_peer_score ({}) must be less than initial_peer_score ({})",
                self.min_peer_score,
                self.initial_peer_score
            );
        }

        Ok(())
    }
}

// Add toml dependency
fn _toml_placeholder() {
    // This function exists to remind us to add toml to Cargo.toml
    // It will be removed once we use toml::from_str properly
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DnsSeedConfig::default();
        assert_eq!(config.heartbeat_interval_secs, 120);
        assert_eq!(config.peer_timeout_secs, 240);
        assert!(config.require_signed_heartbeats);
    }

    #[test]
    fn test_config_validation() {
        let mut config = DnsSeedConfig::default();
        assert!(config.validate().is_ok());

        // Invalid: timeout <= heartbeat
        config.peer_timeout_secs = 60;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_builder_methods() {
        let config = DnsSeedConfig::default()
            .with_heartbeat_port(31000)
            .with_dns_port(5354)
            .with_api_port(9090);

        assert_eq!(config.heartbeat_port, 31000);
        assert_eq!(config.dns_port, 5354);
        assert_eq!(config.api_port, 9090);
    }
}
