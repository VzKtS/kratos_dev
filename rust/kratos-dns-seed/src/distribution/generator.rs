//! IDpeers.json Generator
//!
//! Generates and signs the IDpeers.json file for distribution to nodes.
//! The file contains current network state and a curated list of peers.

use ed25519_dalek::SigningKey;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::DnsSeedConfig;
use crate::crypto::{keypair_to_seed_id, sign_idpeers_file};
use crate::network_state::NetworkStateAggregator;
use crate::registry::PeerRegistry;
use crate::types::{IdPeersFile, PeerInfo, DEFAULT_P2P_PORT, OFFICIAL_DNS_SEED_IPS};

/// IDpeers.json generator
pub struct IdPeersGenerator {
    /// DNS Seed keypair for signing
    keypair: SigningKey,

    /// Seed ID (derived from keypair)
    seed_id: [u8; 32],

    /// Configuration
    config: Arc<DnsSeedConfig>,

    /// Output path for the file
    output_path: std::path::PathBuf,

    /// Last generation timestamp
    last_generated: u64,

    /// Cached file content
    cached_content: Option<Vec<u8>>,
}

impl IdPeersGenerator {
    /// Create a new generator
    pub fn new(
        keypair: SigningKey,
        config: Arc<DnsSeedConfig>,
        output_path: std::path::PathBuf,
    ) -> Self {
        let seed_id = keypair_to_seed_id(&keypair);

        Self {
            keypair,
            seed_id,
            config,
            output_path,
            last_generated: 0,
            cached_content: None,
        }
    }

    /// Generate the IDpeers.json file
    pub async fn generate(
        &mut self,
        registry: &PeerRegistry,
        network_state: &NetworkStateAggregator,
    ) -> anyhow::Result<IdPeersFile> {
        let now = current_timestamp();

        // Get diverse set of top peers
        let peers = self.select_peers(registry);

        // Build fallback bootnodes list
        let fallback_bootnodes = self.build_fallback_bootnodes();

        // Create the file structure
        let mut file = IdPeersFile {
            version: 1,
            generated_at: now,
            dns_seed_id: self.seed_id,
            signature: [0u8; 64], // Will be filled in
            network_state: network_state.current_state(),
            peers,
            fallback_bootnodes,
        };

        // Sign the file
        file.signature = sign_idpeers_file(&self.keypair, &file);

        // Update cache
        self.last_generated = now;
        self.cached_content = Some(serde_json::to_vec_pretty(&file)?);

        info!(
            "📝 Generated IDpeers.json: {} peers, {} validators, height {}",
            file.peers.len(),
            file.network_state.active_validators,
            file.network_state.best_height
        );

        Ok(file)
    }

    /// Generate and save to disk
    pub async fn generate_and_save(
        &mut self,
        registry: &PeerRegistry,
        network_state: &NetworkStateAggregator,
    ) -> anyhow::Result<()> {
        let file = self.generate(registry, network_state).await?;

        // Ensure output directory exists
        if let Some(parent) = self.output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Write file atomically (write to temp, then rename)
        let temp_path = self.output_path.with_extension("json.tmp");
        let content = serde_json::to_vec_pretty(&file)?;

        tokio::fs::write(&temp_path, &content).await?;
        tokio::fs::rename(&temp_path, &self.output_path).await?;

        debug!("Saved IDpeers.json to {:?}", self.output_path);

        Ok(())
    }

    /// Select peers for inclusion in the file
    fn select_peers(&self, registry: &PeerRegistry) -> Vec<PeerInfo> {
        let timeout = self.config.peer_timeout_secs;
        let max_peers = self.config.max_peers_in_idpeers;

        // Get diverse peers (different regions, high scores)
        let diverse = registry.get_diverse_peers(
            max_peers,
            self.config.min_regions_in_idpeers,
            timeout,
        );

        // Convert references to owned
        diverse.into_iter().cloned().collect()
    }

    /// Build fallback bootnode list
    fn build_fallback_bootnodes(&self) -> Vec<String> {
        let mut bootnodes = Vec::new();

        // Add configured fallback bootnodes
        for bootnode in &self.config.fallback_bootnodes {
            bootnodes.push(bootnode.clone());
        }

        // Always include official DNS seeds as fallback
        for ip in OFFICIAL_DNS_SEED_IPS.iter() {
            let addr = format!("/ip4/{}/tcp/{}", ip, DEFAULT_P2P_PORT);
            if !bootnodes.contains(&addr) {
                bootnodes.push(addr);
            }
        }

        bootnodes
    }

    /// Get cached content if still valid
    pub fn get_cached(&self) -> Option<&[u8]> {
        let now = current_timestamp();
        let cache_valid_secs = self.config.idpeers_update_interval_secs / 2;

        if now - self.last_generated < cache_valid_secs {
            self.cached_content.as_deref()
        } else {
            None
        }
    }

    /// Get output path
    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    /// Get seed ID
    pub fn seed_id(&self) -> &[u8; 32] {
        &self.seed_id
    }
}

/// Get current Unix timestamp
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Run periodic generation task
pub async fn run_periodic_generation(
    generator: Arc<RwLock<IdPeersGenerator>>,
    registry: Arc<RwLock<PeerRegistry>>,
    network_state: Arc<RwLock<NetworkStateAggregator>>,
    interval_secs: u64,
) {
    info!("📝 Starting periodic IDpeers.json generation (every {}s)", interval_secs);

    let mut interval = tokio::time::interval(
        tokio::time::Duration::from_secs(interval_secs)
    );

    loop {
        interval.tick().await;

        // Update network state first
        {
            let reg = registry.read().await;
            let mut state = network_state.write().await;
            let peers = reg.get_active_peers(240);
            state.update_from_peers(&peers);
        }

        // Generate new file
        {
            let reg = registry.read().await;
            let state = network_state.read().await;
            let mut gen = generator.write().await;

            if let Err(e) = gen.generate_and_save(&reg, &state).await {
                warn!("Failed to generate IDpeers.json: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::generate_keypair;
    use tempfile::tempdir;

    #[test]
    fn test_fallback_bootnodes() {
        let keypair = generate_keypair();
        let config = Arc::new(DnsSeedConfig::default());
        let output = std::path::PathBuf::from("/tmp/test.json");

        let generator = IdPeersGenerator::new(keypair, config, output);
        let bootnodes = generator.build_fallback_bootnodes();

        // Should include official DNS seeds
        assert!(!bootnodes.is_empty());
        assert!(bootnodes.iter().any(|b| b.contains("45.8.132.252")));
    }

    #[tokio::test]
    async fn test_generate_empty_registry() {
        let dir = tempdir().unwrap();
        let keypair = generate_keypair();
        let config = Arc::new(DnsSeedConfig::default());
        let output = dir.path().join("idpeers.json");

        let mut generator = IdPeersGenerator::new(keypair, config, output);

        let registry = PeerRegistry::open(dir.path().join("registry")).unwrap();
        let network_state = NetworkStateAggregator::new([0u8; 32], current_timestamp());

        let file = generator.generate(&registry, &network_state).await.unwrap();

        assert_eq!(file.version, 1);
        assert!(file.peers.is_empty());
        assert!(!file.fallback_bootnodes.is_empty());
    }
}
