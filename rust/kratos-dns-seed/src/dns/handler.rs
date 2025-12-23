//! DNS Request Handler
//!
//! Handles DNS queries and returns appropriate peer records.
//! Uses a simplified approach compatible with trust-dns-server.

use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

use crate::config::DnsSeedConfig;
use crate::registry::PeerRegistry;

/// DNS query result
#[derive(Debug, Clone)]
pub struct DnsQueryResult {
    /// IPv4 addresses
    pub ipv4_addrs: Vec<std::net::Ipv4Addr>,

    /// IPv6 addresses
    pub ipv6_addrs: Vec<std::net::Ipv6Addr>,

    /// TTL for records
    pub ttl: u32,
}

/// DNS handler for KratOs peer discovery
pub struct KratosDnsHandler {
    /// Peer registry
    registry: Arc<RwLock<PeerRegistry>>,

    /// Configuration
    config: Arc<DnsSeedConfig>,

    /// TTL for DNS records (seconds)
    ttl: u32,
}

impl KratosDnsHandler {
    /// Create a new DNS handler
    pub fn new(
        registry: Arc<RwLock<PeerRegistry>>,
        config: Arc<DnsSeedConfig>,
    ) -> Self {
        Self {
            registry,
            config,
            ttl: 60, // 1 minute TTL
        }
    }

    /// Get peer IP addresses for DNS response
    pub async fn query(&self, _want_ipv6: bool) -> DnsQueryResult {
        let registry = self.registry.read().await;
        let timeout = self.config.peer_timeout_secs;
        let max_peers = self.config.max_peers_in_dns_response;

        // Get top-scoring, geographically diverse peers
        let peers = registry.get_diverse_peers(
            max_peers,
            self.config.min_regions_in_response,
            timeout,
        );

        let mut ipv4_addrs = Vec::new();
        let mut ipv6_addrs = Vec::new();

        for peer in peers {
            if let Some(ip) = peer.first_ip() {
                match ip {
                    IpAddr::V4(v4) => ipv4_addrs.push(v4),
                    IpAddr::V6(v6) => ipv6_addrs.push(v6),
                }
            }
        }

        // Shuffle for load distribution
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        ipv4_addrs.shuffle(&mut rng);
        ipv6_addrs.shuffle(&mut rng);

        debug!(
            "DNS query result: {} IPv4, {} IPv6 addresses",
            ipv4_addrs.len(),
            ipv6_addrs.len()
        );

        DnsQueryResult {
            ipv4_addrs,
            ipv6_addrs,
            ttl: self.ttl,
        }
    }

    /// Get domain name
    pub fn domain(&self) -> &str {
        &self.config.dns_domain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_handler_query_empty() {
        let dir = tempdir().unwrap();
        let registry = PeerRegistry::open(dir.path().join("registry")).unwrap();
        let config = Arc::new(DnsSeedConfig::default());

        let handler = KratosDnsHandler::new(
            Arc::new(RwLock::new(registry)),
            config,
        );

        let result = handler.query(false).await;
        assert!(result.ipv4_addrs.is_empty());
        assert!(result.ipv6_addrs.is_empty());
    }
}
