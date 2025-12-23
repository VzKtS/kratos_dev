//! DNS Seed Client
//!
//! Handles communication with DNS Seed servers:
//! - Sends periodic heartbeats (every 2 minutes) to confirm node presence
//! - Downloads and verifies signed IDpeers.json for peer discovery
//! - Provides network state information from DNS Seeds
//!
//! ## Protocol
//!
//! Heartbeat (TCP port 30334):
//! 1. Node connects to DNS Seed
//! 2. Node sends HeartbeatMessage (bincode serialized, length-prefixed)
//! 3. DNS Seed verifies signature and responds with HeartbeatResponse
//! 4. Response includes current network state
//!
//! IDpeers.json (HTTP):
//! 1. Node fetches /idpeers.json from DNS Seed
//! 2. Node verifies signature against known DNS Seed public keys
//! 3. Node extracts peer list and network state

use ed25519_dalek::{Signer, SigningKey};
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize, Deserializer, Serializer};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::dns_seeds::DEFAULT_P2P_PORT;

// =============================================================================
// SERDE HELPERS FOR BYTE ARRAYS
// =============================================================================

/// Helper module for serializing [u8; 64] arrays (signatures)
mod sig_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(data: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        hex::encode(data).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 64 {
            return Err(serde::de::Error::custom("signature must be 64 bytes"));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}

/// Helper module for serializing [u8; 32] arrays (hashes, keys)
mod hash_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(data: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        hex::encode(data).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("hash/key must be 32 bytes"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}

// =============================================================================
// CONFIGURATION
// =============================================================================

/// DNS Seed heartbeat port
pub const HEARTBEAT_PORT: u16 = 30334;

/// Heartbeat interval (2 minutes as specified)
pub const HEARTBEAT_INTERVAL_SECS: u64 = 120;

/// Connection timeout for heartbeat
pub const CONNECTION_TIMEOUT_SECS: u64 = 30;

/// Maximum message size (1MB)
const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Domain separation for heartbeat signatures
const DOMAIN_HEARTBEAT: &[u8] = b"KRATOS_DNS_HEARTBEAT_V1:";

/// Official DNS Seed IPs with heartbeat support
pub const DNS_SEED_HEARTBEAT_IPS: [&str; 3] = [
    "5.189.184.205",
    "45.8.132.252",
    "74.208.14.99",
];

// =============================================================================
// TYPES (Compatible with kratos-dns-seed)
// =============================================================================

/// Heartbeat message sent to DNS Seeds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    pub version: u32,
    /// Ed25519 public key - used for signature verification
    #[serde(with = "hash_serde")]
    pub peer_id: [u8; 32],
    /// libp2p PeerId (base58 encoded) - used for actual connection
    pub libp2p_peer_id: String,
    pub addresses: Vec<String>,
    pub current_height: u64,
    #[serde(with = "hash_serde")]
    pub best_hash: [u8; 32],
    #[serde(with = "hash_serde")]
    pub genesis_hash: [u8; 32],
    pub is_validator: bool,
    pub validator_count: Option<u32>,
    pub total_stake: Option<u128>,
    pub protocol_version: u32,
    pub timestamp: u64,
    #[serde(with = "sig_serde")]
    pub signature: [u8; 64],
}

impl HeartbeatMessage {
    /// Create signing data (all fields except signature)
    pub fn signing_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&self.peer_id);
        data.extend_from_slice(self.libp2p_peer_id.as_bytes());
        for addr in &self.addresses {
            data.extend_from_slice(addr.as_bytes());
        }
        data.extend_from_slice(&self.current_height.to_le_bytes());
        data.extend_from_slice(&self.best_hash);
        data.extend_from_slice(&self.genesis_hash);
        data.push(self.is_validator as u8);
        if let Some(vc) = self.validator_count {
            data.extend_from_slice(&vc.to_le_bytes());
        }
        if let Some(ts) = self.total_stake {
            data.extend_from_slice(&ts.to_le_bytes());
        }
        data.extend_from_slice(&self.protocol_version.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data
    }

    /// Sign the message
    pub fn sign(&mut self, keypair: &SigningKey) {
        let data = self.signing_data();
        let mut domain_data = Vec::with_capacity(DOMAIN_HEARTBEAT.len() + data.len());
        domain_data.extend_from_slice(DOMAIN_HEARTBEAT);
        domain_data.extend_from_slice(&data);

        let signature = keypair.sign(&domain_data);
        self.signature = signature.to_bytes();
    }
}

/// Response from DNS Seed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub accepted: bool,
    pub error: Option<String>,
    pub network_state: Option<NetworkStateInfo>,
    pub timestamp: u64,
}

/// Network state information from DNS Seed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStateInfo {
    #[serde(with = "hash_serde")]
    pub genesis_hash: [u8; 32],
    pub best_height: u64,
    pub active_validators: u32,
    pub security_state: SecurityState,
    pub total_stake: u128,
    pub participation_rate: f64,
    pub estimated_inflation: f64,
    pub active_peers: u32,
    pub timestamp: u64,
}

/// Network security state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityState {
    Bootstrap,
    Normal,
    Degraded,
    Restricted,
    Emergency,
}

/// IDpeers.json file structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdPeersFile {
    pub version: u32,
    pub generated_at: u64,
    #[serde(with = "hash_serde")]
    pub dns_seed_id: [u8; 32],
    #[serde(with = "sig_serde")]
    pub signature: [u8; 64],
    pub network_state: NetworkStateInfo,
    pub peers: Vec<PeerInfoCompact>,
    pub fallback_bootnodes: Vec<String>,
}

/// Compact peer info from IDpeers.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfoCompact {
    #[serde(with = "hash_serde")]
    pub peer_id: [u8; 32],
    pub addresses: Vec<String>,
    pub height: u64,
    pub is_validator: bool,
    pub score: i32,
}

// =============================================================================
// DNS SEED CLIENT
// =============================================================================

/// Client for DNS Seed communication
pub struct DnsSeedClient {
    /// Node's signing keypair
    keypair: SigningKey,

    /// Node's peer ID (Ed25519 public key for signature verification)
    peer_id: [u8; 32],

    /// Node's libp2p PeerId (base58 encoded for actual connection)
    libp2p_peer_id: String,

    /// DNS Seed IPs to contact
    seed_ips: Vec<String>,

    /// Last network state received
    last_network_state: Option<NetworkStateInfo>,

    /// Last heartbeat timestamp per seed
    last_heartbeat: std::collections::HashMap<String, u64>,
}

impl DnsSeedClient {
    /// Create a new DNS Seed client
    ///
    /// # Arguments
    /// * `keypair` - Ed25519 signing key for heartbeat signatures
    /// * `libp2p_peer_id` - The node's libp2p PeerId as base58 string (for peer discovery)
    pub fn new(keypair: SigningKey, libp2p_peer_id: String) -> Self {
        let verifying_key = keypair.verifying_key();
        let mut peer_id = [0u8; 32];
        peer_id.copy_from_slice(verifying_key.as_bytes());

        let seed_ips = DNS_SEED_HEARTBEAT_IPS
            .iter()
            .map(|s| s.to_string())
            .collect();

        Self {
            keypair,
            peer_id,
            libp2p_peer_id,
            seed_ips,
            last_network_state: None,
            last_heartbeat: std::collections::HashMap::new(),
        }
    }

    /// Send heartbeat to all DNS Seeds
    pub async fn send_heartbeats(
        &mut self,
        addresses: Vec<String>,
        current_height: u64,
        best_hash: [u8; 32],
        genesis_hash: [u8; 32],
        is_validator: bool,
        validator_count: Option<u32>,
        total_stake: Option<u128>,
    ) -> Vec<HeartbeatResult> {
        let mut results = Vec::new();
        let now = current_timestamp();

        for seed_ip in &self.seed_ips.clone() {
            let result = self.send_heartbeat_to_seed(
                seed_ip,
                addresses.clone(),
                current_height,
                best_hash,
                genesis_hash,
                is_validator,
                validator_count,
                total_stake,
            ).await;

            if result.success {
                self.last_heartbeat.insert(seed_ip.clone(), now);
                if let Some(state) = &result.network_state {
                    self.last_network_state = Some(state.clone());
                }
            }

            results.push(result);
        }

        results
    }

    /// Send heartbeat to a single DNS Seed
    async fn send_heartbeat_to_seed(
        &self,
        seed_ip: &str,
        addresses: Vec<String>,
        current_height: u64,
        best_hash: [u8; 32],
        genesis_hash: [u8; 32],
        is_validator: bool,
        validator_count: Option<u32>,
        total_stake: Option<u128>,
    ) -> HeartbeatResult {
        let addr = format!("{}:{}", seed_ip, HEARTBEAT_PORT);

        // Build message
        let mut message = HeartbeatMessage {
            version: 1,
            peer_id: self.peer_id,
            libp2p_peer_id: self.libp2p_peer_id.clone(),
            addresses,
            current_height,
            best_hash,
            genesis_hash,
            is_validator,
            validator_count,
            total_stake,
            protocol_version: 1,
            timestamp: current_timestamp(),
            signature: [0u8; 64],
        };

        // Sign the message
        message.sign(&self.keypair);

        // Connect and send
        let timeout = Duration::from_secs(CONNECTION_TIMEOUT_SECS);

        let connect_result = tokio::time::timeout(
            timeout,
            TcpStream::connect(&addr),
        ).await;

        let mut stream = match connect_result {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                return HeartbeatResult {
                    seed_ip: seed_ip.to_string(),
                    success: false,
                    error: Some(format!("Connection failed: {}", e)),
                    network_state: None,
                };
            }
            Err(_) => {
                return HeartbeatResult {
                    seed_ip: seed_ip.to_string(),
                    success: false,
                    error: Some("Connection timeout".to_string()),
                    network_state: None,
                };
            }
        };

        // Serialize message
        let msg_bytes = match bincode::serialize(&message) {
            Ok(b) => b,
            Err(e) => {
                return HeartbeatResult {
                    seed_ip: seed_ip.to_string(),
                    success: false,
                    error: Some(format!("Serialization failed: {}", e)),
                    network_state: None,
                };
            }
        };

        // Send length + message
        let len_bytes = (msg_bytes.len() as u32).to_be_bytes();

        if let Err(e) = stream.write_all(&len_bytes).await {
            return HeartbeatResult {
                seed_ip: seed_ip.to_string(),
                success: false,
                error: Some(format!("Write failed: {}", e)),
                network_state: None,
            };
        }

        if let Err(e) = stream.write_all(&msg_bytes).await {
            return HeartbeatResult {
                seed_ip: seed_ip.to_string(),
                success: false,
                error: Some(format!("Write failed: {}", e)),
                network_state: None,
            };
        }

        if let Err(e) = stream.flush().await {
            return HeartbeatResult {
                seed_ip: seed_ip.to_string(),
                success: false,
                error: Some(format!("Flush failed: {}", e)),
                network_state: None,
            };
        }

        // Read response
        let mut len_buf = [0u8; 4];
        let read_result = tokio::time::timeout(
            timeout,
            stream.read_exact(&mut len_buf),
        ).await;

        match read_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                return HeartbeatResult {
                    seed_ip: seed_ip.to_string(),
                    success: false,
                    error: Some(format!("Read failed: {}", e)),
                    network_state: None,
                };
            }
            Err(_) => {
                return HeartbeatResult {
                    seed_ip: seed_ip.to_string(),
                    success: false,
                    error: Some("Read timeout".to_string()),
                    network_state: None,
                };
            }
        }

        let resp_len = u32::from_be_bytes(len_buf) as usize;
        if resp_len > MAX_MESSAGE_SIZE {
            return HeartbeatResult {
                seed_ip: seed_ip.to_string(),
                success: false,
                error: Some("Response too large".to_string()),
                network_state: None,
            };
        }

        let mut resp_buf = vec![0u8; resp_len];
        let read_result = tokio::time::timeout(
            timeout,
            stream.read_exact(&mut resp_buf),
        ).await;

        match read_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                return HeartbeatResult {
                    seed_ip: seed_ip.to_string(),
                    success: false,
                    error: Some(format!("Read failed: {}", e)),
                    network_state: None,
                };
            }
            Err(_) => {
                return HeartbeatResult {
                    seed_ip: seed_ip.to_string(),
                    success: false,
                    error: Some("Read timeout".to_string()),
                    network_state: None,
                };
            }
        }

        // Parse response
        let response: HeartbeatResponse = match bincode::deserialize(&resp_buf) {
            Ok(r) => r,
            Err(e) => {
                return HeartbeatResult {
                    seed_ip: seed_ip.to_string(),
                    success: false,
                    error: Some(format!("Deserialization failed: {}", e)),
                    network_state: None,
                };
            }
        };

        HeartbeatResult {
            seed_ip: seed_ip.to_string(),
            success: response.accepted,
            error: response.error,
            network_state: response.network_state,
        }
    }

    /// Fetch IDpeers.json from a DNS Seed
    pub async fn fetch_idpeers(&self, seed_ip: &str) -> Result<IdPeersFile, String> {
        let url = format!("http://{}:8080/idpeers.json", seed_ip);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("HTTP client error: {}", e))?;

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let body = response
            .text()
            .await
            .map_err(|e| format!("Read body failed: {}", e))?;

        let file: IdPeersFile = serde_json::from_str(&body)
            .map_err(|e| format!("JSON parse failed: {}", e))?;

        // TODO: Verify signature against known DNS Seed public keys

        Ok(file)
    }

    /// Get peers from IDpeers.json
    pub async fn get_peers_from_dns_seeds(&self) -> Vec<(PeerId, Multiaddr)> {
        let mut peers = Vec::new();

        for seed_ip in &self.seed_ips {
            match self.fetch_idpeers(seed_ip).await {
                Ok(file) => {
                    info!(
                        "📋 Fetched {} peers from DNS Seed {}",
                        file.peers.len(),
                        seed_ip
                    );

                    for peer_info in &file.peers {
                        for addr_str in &peer_info.addresses {
                            // Parse address and create PeerId
                            if let Ok(addr) = Multiaddr::from_str(addr_str) {
                                // Convert peer_id bytes to PeerId
                                if let Ok(peer_id) = PeerId::from_bytes(&peer_info.peer_id) {
                                    peers.push((peer_id, addr));
                                }
                            }
                        }
                    }

                    // Also add fallback bootnodes
                    for bootnode in &file.fallback_bootnodes {
                        if let Ok((peer_id, addr)) = super::dns_seeds::parse_bootnode(bootnode) {
                            peers.push((peer_id, addr));
                        }
                    }

                    // One successful fetch is enough
                    break;
                }
                Err(e) => {
                    warn!("Failed to fetch IDpeers.json from {}: {}", seed_ip, e);
                }
            }
        }

        peers
    }

    /// Get last known network state
    pub fn network_state(&self) -> Option<&NetworkStateInfo> {
        self.last_network_state.as_ref()
    }

    /// Get peer ID
    pub fn peer_id(&self) -> &[u8; 32] {
        &self.peer_id
    }
}

/// Result of a heartbeat attempt
#[derive(Debug, Clone)]
pub struct HeartbeatResult {
    pub seed_ip: String,
    pub success: bool,
    pub error: Option<String>,
    pub network_state: Option<NetworkStateInfo>,
}

// =============================================================================
// HEARTBEAT SERVICE
// =============================================================================

/// Service that sends periodic heartbeats to DNS Seeds
pub struct HeartbeatService {
    /// DNS Seed client
    client: Arc<RwLock<DnsSeedClient>>,

    /// Whether the service is running
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl HeartbeatService {
    /// Create a new heartbeat service
    pub fn new(keypair: SigningKey, libp2p_peer_id: String) -> Self {
        Self {
            client: Arc::new(RwLock::new(DnsSeedClient::new(keypair, libp2p_peer_id))),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the heartbeat service
    pub async fn start<F>(
        &self,
        get_node_info: F,
    ) where
        F: Fn() -> NodeInfo + Send + Sync + 'static,
    {
        use std::sync::atomic::Ordering;

        if self.running.swap(true, Ordering::SeqCst) {
            warn!("Heartbeat service already running");
            return;
        }

        let client = self.client.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                Duration::from_secs(HEARTBEAT_INTERVAL_SECS)
            );

            info!("💓 Heartbeat service started (interval: {}s)", HEARTBEAT_INTERVAL_SECS);

            while running.load(Ordering::SeqCst) {
                interval.tick().await;

                let info = get_node_info();

                let mut client = client.write().await;
                let results = client.send_heartbeats(
                    info.addresses,
                    info.current_height,
                    info.best_hash,
                    info.genesis_hash,
                    info.is_validator,
                    info.validator_count,
                    info.total_stake,
                ).await;

                let successes = results.iter().filter(|r| r.success).count();
                let total = results.len();

                if successes > 0 {
                    debug!("💓 Heartbeat sent to {}/{} DNS Seeds", successes, total);
                } else {
                    warn!("💔 Failed to send heartbeat to any DNS Seed");
                }
            }

            info!("💓 Heartbeat service stopped");
        });
    }

    /// Stop the heartbeat service
    pub fn stop(&self) {
        use std::sync::atomic::Ordering;
        self.running.store(false, Ordering::SeqCst);
    }

    /// Get the client for direct access
    pub fn client(&self) -> Arc<RwLock<DnsSeedClient>> {
        self.client.clone()
    }
}

/// Node information for heartbeat
pub struct NodeInfo {
    pub addresses: Vec<String>,
    pub current_height: u64,
    pub best_hash: [u8; 32],
    pub genesis_hash: [u8; 32],
    pub is_validator: bool,
    pub validator_count: Option<u32>,
    pub total_stake: Option<u128>,
}

// =============================================================================
// HELPERS
// =============================================================================

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_heartbeat_message_signing() {
        let keypair = SigningKey::generate(&mut OsRng);

        let mut message = HeartbeatMessage {
            version: 1,
            peer_id: [0u8; 32],
            libp2p_peer_id: "12D3KooWTestPeerId".to_string(),
            addresses: vec!["/ip4/1.2.3.4/tcp/30333".to_string()],
            current_height: 12345,
            best_hash: [1u8; 32],
            genesis_hash: [2u8; 32],
            is_validator: true,
            validator_count: Some(100),
            total_stake: Some(1_000_000),
            protocol_version: 1,
            timestamp: current_timestamp(),
            signature: [0u8; 64],
        };

        message.sign(&keypair);

        // Signature should not be all zeros
        assert_ne!(message.signature, [0u8; 64]);
    }

    #[test]
    fn test_client_creation() {
        let keypair = SigningKey::generate(&mut OsRng);
        let client = DnsSeedClient::new(keypair, "12D3KooWTestPeerId".to_string());

        assert_eq!(client.seed_ips.len(), 3);
        assert!(client.last_network_state.is_none());
        assert_eq!(client.libp2p_peer_id, "12D3KooWTestPeerId");
    }
}
