//! Heartbeat Receiver Module
//!
//! Listens for heartbeat messages from KratOs nodes on port 30334.
//! Nodes send heartbeats every 2 minutes to confirm their presence.
//!
//! ## Protocol
//!
//! 1. Node connects to DNS Seed on TCP port 30334
//! 2. Node sends HeartbeatMessage (bincode serialized)
//! 3. DNS Seed verifies signature
//! 4. DNS Seed updates peer registry
//! 5. DNS Seed responds with HeartbeatResponse
//!
//! ## Security
//!
//! - All heartbeats must be signed with the node's Ed25519 key
//! - Rate limiting per IP to prevent DoS
//! - Genesis hash validation to prevent wrong-chain peers

mod protocol;
mod rate_limiter;

pub use rate_limiter::RateLimiter;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::DnsSeedConfig;
use crate::crypto;
use crate::network_state::NetworkStateAggregator;
use crate::registry::PeerRegistry;
use crate::types::{HeartbeatMessage, HeartbeatResponse, PeerInfo};

/// Maximum message size (1MB)
const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Connection timeout (30 seconds)
const CONNECTION_TIMEOUT_SECS: u64 = 30;

/// Run the heartbeat receiver server
pub async fn run_receiver(
    config: Arc<DnsSeedConfig>,
    registry: Arc<RwLock<PeerRegistry>>,
    network_state: Arc<RwLock<NetworkStateAggregator>>,
) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], config.heartbeat_port));
    let listener = TcpListener::bind(addr).await?;

    info!("💓 Heartbeat receiver listening on {}", addr);

    // Rate limiter shared across all connections
    let rate_limiter = Arc::new(RwLock::new(RateLimiter::new(
        config.rate_limit_per_minute,
        config.max_violations_before_ban,
        config.ban_duration_secs,
    )));

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                let config = config.clone();
                let registry = registry.clone();
                let network_state = network_state.clone();
                let rate_limiter = rate_limiter.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_connection(
                        stream,
                        peer_addr,
                        config,
                        registry,
                        network_state,
                        rate_limiter,
                    ).await {
                        debug!("Connection error from {}: {}", peer_addr, e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}

/// Handle a single heartbeat connection
async fn handle_connection(
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    config: Arc<DnsSeedConfig>,
    registry: Arc<RwLock<PeerRegistry>>,
    network_state: Arc<RwLock<NetworkStateAggregator>>,
    rate_limiter: Arc<RwLock<RateLimiter>>,
) -> anyhow::Result<()> {
    let peer_ip = peer_addr.ip();

    // Check rate limit
    {
        let mut limiter = rate_limiter.write().await;
        if !limiter.check_rate_limit(peer_ip) {
            debug!("Rate limited: {}", peer_ip);
            let response = HeartbeatResponse {
                accepted: false,
                error: Some("Rate limited".to_string()),
                network_state: None,
                timestamp: current_timestamp(),
            };
            send_response(&mut stream, &response).await?;
            return Ok(());
        }
    }

    // Set connection timeout
    let timeout = tokio::time::Duration::from_secs(CONNECTION_TIMEOUT_SECS);

    // Read message length (4 bytes, big-endian)
    let mut len_buf = [0u8; 4];
    tokio::time::timeout(timeout, stream.read_exact(&mut len_buf)).await
        .map_err(|_| anyhow::anyhow!("Connection timeout"))??;

    let msg_len = u32::from_be_bytes(len_buf) as usize;

    if msg_len > MAX_MESSAGE_SIZE {
        warn!("Message too large from {}: {} bytes", peer_addr, msg_len);
        let response = HeartbeatResponse {
            accepted: false,
            error: Some("Message too large".to_string()),
            network_state: None,
            timestamp: current_timestamp(),
        };
        send_response(&mut stream, &response).await?;
        return Ok(());
    }

    // Read message body
    let mut msg_buf = vec![0u8; msg_len];
    tokio::time::timeout(timeout, stream.read_exact(&mut msg_buf)).await
        .map_err(|_| anyhow::anyhow!("Connection timeout"))??;

    // Deserialize message
    let message: HeartbeatMessage = match bincode::deserialize(&msg_buf) {
        Ok(m) => m,
        Err(e) => {
            warn!("Invalid message from {}: {}", peer_addr, e);
            let response = HeartbeatResponse {
                accepted: false,
                error: Some("Invalid message format".to_string()),
                network_state: None,
                timestamp: current_timestamp(),
            };
            send_response(&mut stream, &response).await?;
            return Ok(());
        }
    };

    // Validate message
    let validation_result = validate_heartbeat(&message, &config);

    if let Err(error) = validation_result {
        warn!("Invalid heartbeat from {}: {}", peer_addr, error);

        // Record violation for signature failures
        if error.contains("signature") {
            let mut limiter = rate_limiter.write().await;
            limiter.record_violation(peer_ip);
        }

        let response = HeartbeatResponse {
            accepted: false,
            error: Some(error),
            network_state: None,
            timestamp: current_timestamp(),
        };
        send_response(&mut stream, &response).await?;
        return Ok(());
    }

    // Create peer info and update registry
    let peer_info = PeerInfo::from_heartbeat(&message, config.initial_peer_score);

    {
        let mut reg = registry.write().await;
        reg.update_peer(peer_info);
    }

    debug!(
        "💓 Heartbeat from {} (height={}, validator={})",
        hex::encode(&message.peer_id[..8]),
        message.current_height,
        message.is_validator
    );

    // Prepare response with current network state
    let network_state_info = {
        let state = network_state.read().await;
        Some(state.current_state())
    };

    let response = HeartbeatResponse {
        accepted: true,
        error: None,
        network_state: network_state_info,
        timestamp: current_timestamp(),
    };

    send_response(&mut stream, &response).await?;

    Ok(())
}

/// Validate a heartbeat message
fn validate_heartbeat(message: &HeartbeatMessage, config: &DnsSeedConfig) -> Result<(), String> {
    // Check version
    if message.version != 1 {
        return Err(format!("Unsupported version: {}", message.version));
    }

    // Check timestamp (not too old, not too far in future)
    let now = current_timestamp();
    let max_age = config.heartbeat_interval_secs * 2;
    let max_future = 60; // Allow 1 minute clock skew

    if message.timestamp < now.saturating_sub(max_age) {
        return Err("Heartbeat too old".to_string());
    }

    if message.timestamp > now + max_future {
        return Err("Heartbeat timestamp in future".to_string());
    }

    // Check addresses
    if message.addresses.is_empty() {
        return Err("No addresses provided".to_string());
    }

    // Validate genesis hash if configured
    if let Some(expected_genesis) = &config.genesis_hash {
        let expected_bytes = crypto::hex_to_hash(expected_genesis)
            .map_err(|_| "Invalid genesis hash in config".to_string())?;

        if message.genesis_hash != expected_bytes {
            return Err("Genesis hash mismatch".to_string());
        }
    }

    // Verify signature (if required)
    if config.require_signed_heartbeats {
        crypto::verify_heartbeat(message)
            .map_err(|e| format!("Invalid signature: {}", e))?;
    }

    Ok(())
}

/// Send response to client
async fn send_response(stream: &mut TcpStream, response: &HeartbeatResponse) -> anyhow::Result<()> {
    let bytes = bincode::serialize(response)?;
    let len_bytes = (bytes.len() as u32).to_be_bytes();

    stream.write_all(&len_bytes).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;

    Ok(())
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

    #[test]
    fn test_current_timestamp() {
        let ts = current_timestamp();
        assert!(ts > 1700000000); // After Nov 2023
    }

    #[test]
    fn test_validate_heartbeat_version() {
        let config = DnsSeedConfig::default();
        let mut message = create_test_heartbeat();
        message.version = 999;

        let result = validate_heartbeat(&message, &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("version"));
    }

    #[test]
    fn test_validate_heartbeat_no_addresses() {
        let mut config = DnsSeedConfig::default();
        config.require_signed_heartbeats = false;

        let mut message = create_test_heartbeat();
        message.addresses.clear();

        let result = validate_heartbeat(&message, &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("addresses"));
    }

    fn create_test_heartbeat() -> HeartbeatMessage {
        HeartbeatMessage {
            version: 1,
            peer_id: [0u8; 32],
            libp2p_peer_id: "12D3KooWTestPeerId".to_string(),
            addresses: vec!["/ip4/1.2.3.4/tcp/30333".to_string()],
            current_height: 1000,
            best_hash: [0u8; 32],
            genesis_hash: [0u8; 32],
            is_validator: false,
            validator_count: None,
            total_stake: None,
            protocol_version: 1,
            timestamp: current_timestamp(),
            signature: [0u8; 64],
        }
    }
}
