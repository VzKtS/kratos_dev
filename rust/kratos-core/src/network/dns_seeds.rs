// DNS Seeds - Decentralized peer discovery via DNS
// Principle: Multiple independent DNS seeds for resilient network bootstrap
//
// SPEC v3.2: DNS Seeds provide decentralized initial peer discovery.
// - Each seed is operated independently by different community members
// - Seeds resolve to currently active, reachable nodes
// - Nodes cache discovered peers for future use
// - No single point of failure - if one seed fails, others work

use libp2p::{Multiaddr, PeerId};
use std::collections::HashSet;
use std::net::{IpAddr, ToSocketAddrs};
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, info, warn};

// =============================================================================
// DNS SEED CONFIGURATION
// =============================================================================

/// Default P2P port for KratOs nodes
pub const DEFAULT_P2P_PORT: u16 = 30333;

/// Maximum time to wait for DNS resolution
pub const DNS_TIMEOUT_SECS: u64 = 10;

/// Maximum number of peers to return from DNS seeds
pub const MAX_DNS_PEERS: usize = 25;

/// Minimum number of DNS seeds that must be configured
pub const MIN_DNS_SEEDS: usize = 1;

/// Official KratOs DNS seeds
/// These are operated by independent community members
/// Each seed runs a crawler that tracks active nodes
///
/// IMPORTANT: These seeds will be populated when the mainnet launches.
/// For now, they serve as placeholders and documentation.
///
/// To become a DNS seed operator:
/// 1. Run a DNS seed server (see docs/DNS_SEED_OPERATOR.md)
/// 2. Submit a PR to add your seed to this list
/// 3. Seeds are reviewed for reliability and independence
pub const OFFICIAL_DNS_SEEDS: &[&str] = &[
    "45.8.132.252",  // KratOs bootstrap node
];

/// Fallback hardcoded bootnodes for when DNS fails
/// These are stable, long-running nodes operated by known entities
///
/// Format: /ip4/<IP>/tcp/<PORT>/p2p/<PEER_ID>
pub const FALLBACK_BOOTNODES: &[&str] = &[
    "/ip4/45.8.132.252/tcp/30333/p2p/12D3KooWAiEmjd2mEHoXKgEBfaXkcXiv4dDiyecowZQ47fZRztfY",
];

// =============================================================================
// DNS SEED RESOLVER
// =============================================================================

/// Result of DNS seed resolution
#[derive(Debug, Clone)]
pub struct DnsResolutionResult {
    /// Successfully resolved peers
    pub peers: Vec<(PeerId, Multiaddr)>,

    /// Seeds that were queried
    pub seeds_queried: usize,

    /// Seeds that responded successfully
    pub seeds_responded: usize,

    /// Any errors encountered
    pub errors: Vec<String>,
}

impl DnsResolutionResult {
    pub fn empty() -> Self {
        Self {
            peers: Vec::new(),
            seeds_queried: 0,
            seeds_responded: 0,
            errors: Vec::new(),
        }
    }

    pub fn success(&self) -> bool {
        !self.peers.is_empty()
    }
}

/// DNS Seed Resolver - discovers peers via DNS
///
/// The resolver queries multiple DNS seeds in parallel and aggregates results.
/// This provides resilience against individual seed failures.
pub struct DnsSeedResolver {
    /// DNS seeds to query (hostnames)
    seeds: Vec<String>,

    /// Fallback bootnodes (multiaddrs with peer IDs)
    fallback_bootnodes: Vec<String>,

    /// Timeout for DNS queries
    timeout: Duration,

    /// Cache of previously discovered peers
    cached_peers: HashSet<String>,
}

impl DnsSeedResolver {
    /// Create a new resolver with official seeds
    pub fn new() -> Self {
        Self {
            seeds: OFFICIAL_DNS_SEEDS.iter().map(|s| s.to_string()).collect(),
            fallback_bootnodes: FALLBACK_BOOTNODES.iter().map(|s| s.to_string()).collect(),
            timeout: Duration::from_secs(DNS_TIMEOUT_SECS),
            cached_peers: HashSet::new(),
        }
    }

    /// Create a resolver with custom seeds
    pub fn with_seeds(seeds: Vec<String>) -> Self {
        Self {
            seeds,
            fallback_bootnodes: FALLBACK_BOOTNODES.iter().map(|s| s.to_string()).collect(),
            timeout: Duration::from_secs(DNS_TIMEOUT_SECS),
            cached_peers: HashSet::new(),
        }
    }

    /// Add additional DNS seeds
    pub fn add_seed(&mut self, seed: String) {
        if !self.seeds.contains(&seed) {
            self.seeds.push(seed);
        }
    }

    /// Add additional fallback bootnode
    pub fn add_fallback(&mut self, bootnode: String) {
        if !self.fallback_bootnodes.contains(&bootnode) {
            self.fallback_bootnodes.push(bootnode);
        }
    }

    /// Resolve all DNS seeds and return discovered peers
    ///
    /// This queries DNS seeds which return A/AAAA records pointing to active nodes.
    /// Each IP is converted to a multiaddr for libp2p connection.
    ///
    /// Note: DNS seeds return IP addresses, not peer IDs. The peer ID is
    /// discovered during the libp2p handshake.
    pub fn resolve(&mut self) -> DnsResolutionResult {
        let mut result = DnsResolutionResult::empty();
        let mut discovered_ips: HashSet<IpAddr> = HashSet::new();

        if self.seeds.is_empty() {
            debug!("No DNS seeds configured, using fallback bootnodes");
            return self.resolve_fallbacks();
        }

        info!("🔍 Resolving {} DNS seeds...", self.seeds.len());
        result.seeds_queried = self.seeds.len();

        for seed in &self.seeds {
            match self.resolve_seed(seed) {
                Ok(ips) => {
                    debug!("DNS seed {} returned {} IPs", seed, ips.len());
                    result.seeds_responded += 1;

                    for ip in ips {
                        discovered_ips.insert(ip);
                    }
                }
                Err(e) => {
                    warn!("Failed to resolve DNS seed {}: {}", seed, e);
                    result.errors.push(format!("{}: {}", seed, e));
                }
            }
        }

        // Convert IPs to multiaddrs
        // Note: We don't have peer IDs from DNS, so we create dial addresses
        // The peer ID will be learned during connection
        for ip in discovered_ips.iter().take(MAX_DNS_PEERS) {
            let addr_str = match ip {
                IpAddr::V4(v4) => format!("/ip4/{}/tcp/{}", v4, DEFAULT_P2P_PORT),
                IpAddr::V6(v6) => format!("/ip6/{}/tcp/{}", v6, DEFAULT_P2P_PORT),
            };

            if let Ok(addr) = Multiaddr::from_str(&addr_str) {
                // For DNS-discovered peers, we don't have a peer ID yet
                // We'll need to dial the address and discover the peer ID
                self.cached_peers.insert(addr_str.clone());
                // We can't add to result.peers without a PeerId
                // Instead, we return addresses to dial
            }
        }

        info!(
            "📡 DNS resolution complete: {} seeds responded, {} unique IPs discovered",
            result.seeds_responded,
            discovered_ips.len()
        );

        // ALWAYS include fallback bootnodes (they have PeerIds which DNS cannot provide)
        // DNS seeds only return IPs, but libp2p needs PeerIds to connect properly
        info!("Adding fallback bootnodes with PeerIds...");
        let fallback_result = self.resolve_fallbacks();
        result.peers.extend(fallback_result.peers);

        result
    }

    /// Resolve a single DNS seed
    fn resolve_seed(&self, seed: &str) -> Result<Vec<IpAddr>, String> {
        // DNS lookup with timeout
        let lookup_target = format!("{}:{}", seed, DEFAULT_P2P_PORT);

        match lookup_target.to_socket_addrs() {
            Ok(addrs) => {
                let ips: Vec<IpAddr> = addrs.map(|a| a.ip()).collect();
                Ok(ips)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Resolve fallback bootnodes
    fn resolve_fallbacks(&self) -> DnsResolutionResult {
        let mut result = DnsResolutionResult::empty();

        if self.fallback_bootnodes.is_empty() {
            debug!("No fallback bootnodes configured");
            return result;
        }

        info!("🔄 Using {} fallback bootnodes", self.fallback_bootnodes.len());

        for bootnode in &self.fallback_bootnodes {
            match parse_bootnode(bootnode) {
                Ok((peer_id, addr)) => {
                    result.peers.push((peer_id, addr));
                }
                Err(e) => {
                    warn!("Failed to parse bootnode {}: {}", bootnode, e);
                    result.errors.push(format!("{}: {}", bootnode, e));
                }
            }
        }

        result
    }

    /// Get dial-only addresses (IPs without peer IDs)
    /// Used when we need to dial addresses discovered via DNS
    pub fn get_dial_addresses(&self) -> Vec<Multiaddr> {
        self.cached_peers
            .iter()
            .filter_map(|s| Multiaddr::from_str(s).ok())
            .collect()
    }

    /// Clear cached peers
    pub fn clear_cache(&mut self) {
        self.cached_peers.clear();
    }

    /// Get number of configured seeds
    pub fn seed_count(&self) -> usize {
        self.seeds.len()
    }

    /// Get number of fallback bootnodes
    pub fn fallback_count(&self) -> usize {
        self.fallback_bootnodes.len()
    }
}

impl Default for DnsSeedResolver {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// HELPERS
// =============================================================================

/// Parse a bootnode multiaddr string into PeerId and Multiaddr
///
/// Format: /ip4/<IP>/tcp/<PORT>/p2p/<PEER_ID>
/// Example: /ip4/1.2.3.4/tcp/30333/p2p/12D3KooWExample...
pub fn parse_bootnode(bootnode: &str) -> Result<(PeerId, Multiaddr), String> {
    let addr = Multiaddr::from_str(bootnode)
        .map_err(|e| format!("Invalid multiaddr: {}", e))?;

    // Extract peer ID from the multiaddr
    let peer_id = extract_peer_id(&addr)
        .ok_or_else(|| "No peer ID in multiaddr".to_string())?;

    // Return base address without /p2p/... for dialing
    let dial_addr = remove_peer_id(&addr);

    Ok((peer_id, dial_addr))
}

/// Extract PeerId from a multiaddr containing /p2p/<peer_id>
fn extract_peer_id(addr: &Multiaddr) -> Option<PeerId> {
    for proto in addr.iter() {
        if let libp2p::multiaddr::Protocol::P2p(peer_id) = proto {
            return Some(peer_id);
        }
    }
    None
}

/// Remove /p2p/<peer_id> from multiaddr
fn remove_peer_id(addr: &Multiaddr) -> Multiaddr {
    addr.iter()
        .filter(|p| !matches!(p, libp2p::multiaddr::Protocol::P2p(_)))
        .collect()
}

// =============================================================================
// COMMUNITY SEED REGISTRY
// =============================================================================

/// Information about a DNS seed operator
#[derive(Debug, Clone)]
pub struct DnsSeedInfo {
    /// DNS hostname (e.g., "seed1.kratos.network")
    pub hostname: String,

    /// Operator name/organization
    pub operator: String,

    /// Geographic region (for diversity)
    pub region: String,

    /// Whether this is an official Foundation seed
    pub is_official: bool,

    /// Last successful resolution timestamp (Unix epoch)
    pub last_success: Option<u64>,
}

impl DnsSeedInfo {
    pub fn new(hostname: &str, operator: &str, region: &str, is_official: bool) -> Self {
        Self {
            hostname: hostname.to_string(),
            operator: operator.to_string(),
            region: region.to_string(),
            is_official,
            last_success: None,
        }
    }
}

/// Registry of known DNS seeds with metadata
/// Used for monitoring and diversity tracking
#[derive(Debug, Default)]
pub struct DnsSeedRegistry {
    seeds: Vec<DnsSeedInfo>,
}

impl DnsSeedRegistry {
    pub fn new() -> Self {
        Self { seeds: Vec::new() }
    }

    /// Create registry with official seeds
    #[allow(unused_mut)]
    pub fn with_official_seeds() -> Self {
        let mut registry = Self::new();

        // These will be populated at mainnet launch
        // Example entries (commented out until real seeds exist):
        //
        // registry.add(DnsSeedInfo::new(
        //     "seed1.kratos.network",
        //     "KratOs Foundation",
        //     "EU-West",
        //     true,
        // ));
        //
        // registry.add(DnsSeedInfo::new(
        //     "seed2.kratos.community",
        //     "Community Operator A",
        //     "US-East",
        //     false,
        // ));

        registry
    }

    pub fn add(&mut self, seed: DnsSeedInfo) {
        self.seeds.push(seed);
    }

    pub fn seeds(&self) -> &[DnsSeedInfo] {
        &self.seeds
    }

    pub fn hostnames(&self) -> Vec<String> {
        self.seeds.iter().map(|s| s.hostname.clone()).collect()
    }

    /// Get seeds by region for geographic diversity
    pub fn seeds_by_region(&self, region: &str) -> Vec<&DnsSeedInfo> {
        self.seeds.iter().filter(|s| s.region == region).collect()
    }

    /// Get official (Foundation-operated) seeds only
    pub fn official_seeds(&self) -> Vec<&DnsSeedInfo> {
        self.seeds.iter().filter(|s| s.is_official).collect()
    }

    /// Get community-operated seeds only
    pub fn community_seeds(&self) -> Vec<&DnsSeedInfo> {
        self.seeds.iter().filter(|s| !s.is_official).collect()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_resolver_creation() {
        let resolver = DnsSeedResolver::new();
        assert_eq!(resolver.seed_count(), OFFICIAL_DNS_SEEDS.len());
    }

    #[test]
    fn test_add_custom_seed() {
        let mut resolver = DnsSeedResolver::new();
        resolver.add_seed("custom.seed.example".to_string());
        assert_eq!(resolver.seed_count(), OFFICIAL_DNS_SEEDS.len() + 1);
    }

    #[test]
    fn test_parse_bootnode_valid() {
        let bootnode = "/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
        let result = parse_bootnode(bootnode);
        assert!(result.is_ok());

        let (peer_id, addr) = result.unwrap();
        assert!(!peer_id.to_string().is_empty());
        assert!(addr.to_string().contains("/ip4/127.0.0.1/tcp/30333"));
    }

    #[test]
    fn test_parse_bootnode_invalid() {
        let bootnode = "invalid-multiaddr";
        let result = parse_bootnode(bootnode);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_bootnode_no_peer_id() {
        let bootnode = "/ip4/127.0.0.1/tcp/30333";
        let result = parse_bootnode(bootnode);
        assert!(result.is_err());
    }

    #[test]
    fn test_dns_seed_registry() {
        let mut registry = DnsSeedRegistry::new();

        registry.add(DnsSeedInfo::new(
            "seed1.test.local",
            "Test Operator",
            "EU-West",
            true,
        ));

        registry.add(DnsSeedInfo::new(
            "seed2.test.local",
            "Community Test",
            "US-East",
            false,
        ));

        assert_eq!(registry.seeds().len(), 2);
        assert_eq!(registry.official_seeds().len(), 1);
        assert_eq!(registry.community_seeds().len(), 1);
        assert_eq!(registry.seeds_by_region("EU-West").len(), 1);
    }

    #[test]
    fn test_empty_resolution() {
        let mut resolver = DnsSeedResolver::with_seeds(vec![]);
        let result = resolver.resolve();
        // Should try fallbacks (which are also empty in test)
        assert!(result.peers.is_empty());
    }

    #[test]
    fn test_resolver_with_fallbacks() {
        let mut resolver = DnsSeedResolver::with_seeds(vec![]);
        resolver.add_fallback(
            "/ip4/127.0.0.1/tcp/30333/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN".to_string()
        );

        let result = resolver.resolve();
        assert_eq!(result.peers.len(), 1);
    }

    #[test]
    fn test_resolution_result() {
        let result = DnsResolutionResult::empty();
        assert!(!result.success());
        assert_eq!(result.peers.len(), 0);
        assert_eq!(result.seeds_queried, 0);
    }
}
