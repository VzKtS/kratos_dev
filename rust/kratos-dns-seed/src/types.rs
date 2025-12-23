//! Core types for DNS Seed communication
//!
//! These types define the protocol between KratOs nodes and DNS Seeds.
//! All messages that require authentication are signed with Ed25519.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Helper module for serializing [u8; 64] arrays
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

/// Helper module for serializing [u8; 32] arrays
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

/// Helper module for serializing Option<[u8; 32]> arrays
mod option_hash_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(data: &Option<[u8; 32]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match data {
            Some(d) => hex::encode(d).serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<[u8; 32]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => {
                let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
                if bytes.len() != 32 {
                    return Err(serde::de::Error::custom("hash/key must be 32 bytes"));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Ok(Some(arr))
            }
            None => Ok(None),
        }
    }
}

// =============================================================================
// PRIMITIVE TYPES (Compatible with kratos-core)
// =============================================================================

/// 32-byte hash (Blake3)
pub type Hash = [u8; 32];

/// 32-byte Ed25519 public key
pub type PublicKey = [u8; 32];

/// 64-byte Ed25519 signature
pub type Signature = [u8; 64];

/// Block number
pub type BlockNumber = u64;

/// Balance in base units (10^12 = 1 KRAT)
pub type Balance = u128;

/// Peer identifier (derived from public key)
pub type PeerId = [u8; 32];

/// DNS Seed identifier (derived from seed's public key)
pub type SeedId = [u8; 32];

// =============================================================================
// NETWORK SECURITY STATES (Matching kratos-core)
// =============================================================================

/// Network security state based on validator count
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityState {
    /// Bootstrap phase (first 60 days or < 50 validators)
    Bootstrap,

    /// Normal operation (>= 75 validators)
    Normal,

    /// Degraded security (50-74 validators)
    /// - Inflation boosted +1%
    /// - Governance timelocks doubled
    Degraded,

    /// Restricted mode (25-49 validators)
    /// - Governance frozen
    /// - Emergency mechanisms armed
    Restricted,

    /// Emergency mode (< 25 validators)
    /// - Emergency powers active
    /// - Exit always allowed
    Emergency,
}

impl SecurityState {
    /// Determine security state from validator count
    pub fn from_validator_count(count: u32, is_bootstrap: bool) -> Self {
        if is_bootstrap {
            return SecurityState::Bootstrap;
        }

        match count {
            0..=24 => SecurityState::Emergency,
            25..=49 => SecurityState::Restricted,
            50..=74 => SecurityState::Degraded,
            _ => SecurityState::Normal,
        }
    }

    /// Get the inflation adjustment for this state
    pub fn inflation_adjustment(&self) -> f64 {
        match self {
            SecurityState::Bootstrap => 0.065,  // Fixed 6.5%
            SecurityState::Normal => 0.0,       // Adaptive
            SecurityState::Degraded => 0.01,    // +1%
            SecurityState::Restricted => 0.02,  // +2%
            SecurityState::Emergency => 0.03,   // +3%
        }
    }
}

// =============================================================================
// HEARTBEAT PROTOCOL
// =============================================================================

/// Heartbeat message sent by nodes to DNS Seed
///
/// Nodes send this every 2 minutes to confirm their presence.
/// The message is signed with the node's Ed25519 key to prove identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    /// Protocol version for future compatibility
    pub version: u32,

    /// Node's peer ID (Ed25519 public key) - used for signature verification
    #[serde(with = "hash_serde")]
    pub peer_id: PeerId,

    /// Node's libp2p PeerId (base58 encoded) - used for peer discovery/connection
    /// This is the actual PeerId that other nodes need to connect
    pub libp2p_peer_id: String,

    /// Node's listening addresses
    pub addresses: Vec<String>,  // Multiaddr format

    /// Current block height
    pub current_height: BlockNumber,

    /// Best block hash
    #[serde(with = "hash_serde")]
    pub best_hash: Hash,

    /// Genesis hash (for network validation)
    #[serde(with = "hash_serde")]
    pub genesis_hash: Hash,

    /// Whether this node is a validator
    pub is_validator: bool,

    /// Number of active validators (as seen by this node)
    pub validator_count: Option<u32>,

    /// Total stake in the network (as seen by this node)
    pub total_stake: Option<Balance>,

    /// Node's protocol version
    pub protocol_version: u32,

    /// Timestamp of this heartbeat (Unix epoch seconds)
    pub timestamp: u64,

    /// Signature of the message (signs all fields above)
    #[serde(with = "sig_serde")]
    pub signature: Signature,
}

impl HeartbeatMessage {
    /// Get the data to be signed (all fields except signature)
    pub fn signing_data(&self) -> Vec<u8> {
        // Use bincode for deterministic serialization
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
}

/// Response to heartbeat (sent by DNS Seed to node)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    /// Whether the heartbeat was accepted
    pub accepted: bool,

    /// Error message if not accepted
    pub error: Option<String>,

    /// Current network state as seen by DNS Seed
    pub network_state: Option<NetworkStateInfo>,

    /// Timestamp
    pub timestamp: u64,
}

// =============================================================================
// NETWORK STATE INFO
// =============================================================================

/// Aggregated network state information
///
/// This is computed by the DNS Seed from all received heartbeats.
/// Provides nodes with a view of the overall network health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStateInfo {
    /// Genesis hash of the network
    #[serde(with = "hash_serde")]
    pub genesis_hash: Hash,

    /// Best known block height across all peers
    pub best_height: BlockNumber,

    /// Number of active validators
    pub active_validators: u32,

    /// Current security state
    pub security_state: SecurityState,

    /// Total stake in the network
    pub total_stake: Balance,

    /// Average participation rate (0.0 - 1.0)
    pub participation_rate: f64,

    /// Estimated current inflation rate
    pub estimated_inflation: f64,

    /// Number of active peers
    pub active_peers: u32,

    /// Timestamp of this snapshot
    pub timestamp: u64,
}

// =============================================================================
// PEER INFORMATION
// =============================================================================

/// Information about a known peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer's ID (Ed25519 public key - for signature verification)
    #[serde(with = "hash_serde")]
    pub peer_id: PeerId,

    /// Peer's libp2p PeerId (base58 encoded) - for actual connection
    pub libp2p_peer_id: String,

    /// Peer's addresses (Multiaddr format)
    pub addresses: Vec<String>,

    /// Last seen timestamp
    pub last_seen: u64,

    /// Current block height
    pub height: BlockNumber,

    /// Whether peer is a validator
    pub is_validator: bool,

    /// Peer's health score (0-200)
    pub score: i32,

    /// Geographic region (if known)
    pub region: Option<String>,

    /// Protocol version
    pub protocol_version: u32,
}

impl PeerInfo {
    /// Create a new peer info from a heartbeat
    pub fn from_heartbeat(msg: &HeartbeatMessage, score: i32) -> Self {
        Self {
            peer_id: msg.peer_id,
            libp2p_peer_id: msg.libp2p_peer_id.clone(),
            addresses: msg.addresses.clone(),
            last_seen: msg.timestamp,
            height: msg.current_height,
            is_validator: msg.is_validator,
            score,
            region: None,
            protocol_version: msg.protocol_version,
        }
    }

    /// Check if peer is stale (hasn't been seen recently)
    pub fn is_stale(&self, timeout_secs: u64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now.saturating_sub(self.last_seen) > timeout_secs
    }

    /// Get the first IP address if available
    pub fn first_ip(&self) -> Option<IpAddr> {
        for addr in &self.addresses {
            // Parse multiaddr to extract IP
            // Format: /ip4/1.2.3.4/tcp/30333 or /ip6/.../tcp/30333
            if addr.starts_with("/ip4/") {
                let parts: Vec<&str> = addr.split('/').collect();
                if parts.len() >= 3 {
                    if let Ok(ip) = parts[2].parse::<IpAddr>() {
                        return Some(ip);
                    }
                }
            } else if addr.starts_with("/ip6/") {
                let parts: Vec<&str> = addr.split('/').collect();
                if parts.len() >= 3 {
                    if let Ok(ip) = parts[2].parse::<IpAddr>() {
                        return Some(ip);
                    }
                }
            }
        }
        None
    }
}

// =============================================================================
// ID PEERS FILE FORMAT
// =============================================================================

/// The IDpeers.json file format
///
/// This file is signed by the DNS Seed and distributed to nodes.
/// Nodes use this for initial peer discovery and network state awareness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdPeersFile {
    /// File format version
    pub version: u32,

    /// When this file was generated (Unix timestamp)
    pub generated_at: u64,

    /// DNS Seed ID that generated this file
    #[serde(with = "hash_serde")]
    pub dns_seed_id: SeedId,

    /// Signature of the file content
    #[serde(with = "sig_serde")]
    pub signature: Signature,

    /// Current network state
    pub network_state: NetworkStateInfo,

    /// List of known peers
    pub peers: Vec<PeerInfo>,

    /// Fallback bootnodes (always included)
    pub fallback_bootnodes: Vec<String>,
}

impl IdPeersFile {
    /// Get the data to be signed
    pub fn signing_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&self.generated_at.to_le_bytes());
        data.extend_from_slice(&self.dns_seed_id);
        // Serialize network_state
        data.extend_from_slice(&bincode::serialize(&self.network_state).unwrap_or_default());
        // Serialize peers
        data.extend_from_slice(&bincode::serialize(&self.peers).unwrap_or_default());
        // Serialize fallbacks
        for fb in &self.fallback_bootnodes {
            data.extend_from_slice(fb.as_bytes());
        }
        data
    }
}

// =============================================================================
// DNS SEED REGISTRY (for governance integration)
// =============================================================================

/// Information about a registered DNS Seed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsSeedRegistration {
    /// DNS Seed ID (public key)
    #[serde(with = "hash_serde")]
    pub seed_id: SeedId,

    /// Operator's account ID (for rewards)
    #[serde(default, skip_serializing_if = "Option::is_none", with = "option_hash_serde")]
    pub operator_account: Option<PublicKey>,

    /// DNS hostname
    pub hostname: String,

    /// IP addresses
    pub ip_addresses: Vec<IpAddr>,

    /// Geographic region
    pub region: String,

    /// Is this an official (foundation) seed?
    pub is_official: bool,

    /// Registration timestamp
    pub registered_at: u64,

    /// Last successful health check
    pub last_health_check: Option<u64>,

    /// Uptime percentage (0.0 - 1.0)
    pub uptime: f64,

    /// Number of peers served
    pub peers_served: u64,
}

// =============================================================================
// OFFICIAL DNS SEEDS (hardcoded for bootstrap)
// =============================================================================

/// Official DNS Seed IP addresses
/// These are the 3 DNS Seeds you provided
pub const OFFICIAL_DNS_SEED_IPS: [&str; 3] = [
    "5.189.184.205",   // DNS Seed 1
    "45.8.132.252",    // DNS Seed 2 (also the genesis node)
    "74.208.14.99",    // DNS Seed 3
];

/// Default DNS Seed port for heartbeat
pub const DEFAULT_HEARTBEAT_PORT: u16 = 30334;

/// Default P2P port for nodes
pub const DEFAULT_P2P_PORT: u16 = 30333;

// =============================================================================
// RATE LIMITING
// =============================================================================

/// Rate limit violation tracking
#[derive(Debug, Clone)]
pub struct RateLimitEntry {
    /// IP address
    pub ip: IpAddr,

    /// Number of requests in current window
    pub request_count: u32,

    /// Window start time
    pub window_start: u64,

    /// Number of violations
    pub violations: u32,

    /// Ban expiry time (if banned)
    pub ban_until: Option<u64>,
}

impl RateLimitEntry {
    pub fn new(ip: IpAddr, now: u64) -> Self {
        Self {
            ip,
            request_count: 1,
            window_start: now,
            violations: 0,
            ban_until: None,
        }
    }

    pub fn is_banned(&self, now: u64) -> bool {
        self.ban_until.map(|t| now < t).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_state_from_validators() {
        assert_eq!(
            SecurityState::from_validator_count(100, false),
            SecurityState::Normal
        );
        assert_eq!(
            SecurityState::from_validator_count(60, false),
            SecurityState::Degraded
        );
        assert_eq!(
            SecurityState::from_validator_count(30, false),
            SecurityState::Restricted
        );
        assert_eq!(
            SecurityState::from_validator_count(10, false),
            SecurityState::Emergency
        );
        assert_eq!(
            SecurityState::from_validator_count(100, true),
            SecurityState::Bootstrap
        );
    }

    #[test]
    fn test_peer_info_stale_check() {
        let mut peer = PeerInfo {
            peer_id: [0u8; 32],
            libp2p_peer_id: "12D3KooWTestPeerId".to_string(),
            addresses: vec!["/ip4/1.2.3.4/tcp/30333".to_string()],
            last_seen: 0,
            height: 100,
            is_validator: false,
            score: 100,
            region: None,
            protocol_version: 1,
        };

        // Very old timestamp should be stale
        assert!(peer.is_stale(240));

        // Set to current time
        peer.last_seen = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(!peer.is_stale(240));
    }

    #[test]
    fn test_peer_info_extract_ip() {
        let peer = PeerInfo {
            peer_id: [0u8; 32],
            libp2p_peer_id: "12D3KooWTestPeerId".to_string(),
            addresses: vec!["/ip4/192.168.1.1/tcp/30333".to_string()],
            last_seen: 0,
            height: 100,
            is_validator: false,
            score: 100,
            region: None,
            protocol_version: 1,
        };

        let ip = peer.first_ip().unwrap();
        assert_eq!(ip.to_string(), "192.168.1.1");
    }

    #[test]
    fn test_official_dns_seeds() {
        assert_eq!(OFFICIAL_DNS_SEED_IPS.len(), 3);
        assert!(OFFICIAL_DNS_SEED_IPS.contains(&"45.8.132.252"));
    }
}
