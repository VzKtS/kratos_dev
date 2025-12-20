// Rate Limiting - Protection against network flooding attacks
use libp2p::PeerId;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum messages per time window
    pub max_messages_per_window: usize,

    /// Time window duration
    pub window_duration: Duration,

    /// Maximum message size (in bytes)
    pub max_message_size: usize,

    /// Maximum connections per peer
    pub max_connections_per_peer: usize,

    /// Maximum bandwidth per peer (bytes/sec)
    pub max_bandwidth_per_peer: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_messages_per_window: 100,
            window_duration: Duration::from_secs(10),
            max_message_size: 10 * 1024 * 1024, // 10 MB
            max_connections_per_peer: 3,
            max_bandwidth_per_peer: 1024 * 1024, // 1 MB/s
        }
    }
}

/// Statistics per peer
#[derive(Debug, Clone)]
pub struct PeerStats {
    /// Number of messages in current window
    messages_in_window: usize,

    /// Start of current window
    window_start: Instant,

    /// Bytes received in current window
    bytes_in_window: usize,

    /// Number of violations
    violations: usize,

    /// Last violation timestamp
    last_violation: Option<Instant>,
}

impl PeerStats {
    fn new() -> Self {
        Self {
            messages_in_window: 0,
            window_start: Instant::now(),
            bytes_in_window: 0,
            violations: 0,
            last_violation: None,
        }
    }

    /// Reset window if necessary
    fn maybe_reset_window(&mut self, window_duration: Duration) {
        if self.window_start.elapsed() >= window_duration {
            self.messages_in_window = 0;
            self.bytes_in_window = 0;
            self.window_start = Instant::now();
        }
    }

    /// Record a violation
    fn record_violation(&mut self) {
        self.violations += 1;
        self.last_violation = Some(Instant::now());
    }

    /// Check if peer should be banned
    fn should_ban(&self) -> bool {
        // Ban after 5 violations
        self.violations >= 5
    }
}

/// Rate limiter for P2P network
pub struct NetworkRateLimiter {
    /// Configuration
    config: RateLimitConfig,

    /// Statistics per peer
    peer_stats: HashMap<PeerId, PeerStats>,

    /// Temporarily banned peers
    banned_peers: HashMap<PeerId, Instant>,

    /// Ban duration
    ban_duration: Duration,
}

impl NetworkRateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            peer_stats: HashMap::new(),
            banned_peers: HashMap::new(),
            ban_duration: Duration::from_secs(3600), // 1 hour
        }
    }

    /// Check if a message is allowed
    pub fn check_message(
        &mut self,
        peer_id: &PeerId,
        message_size: usize,
    ) -> Result<(), RateLimitError> {
        // Check if peer is banned
        if self.is_banned(peer_id) {
            return Err(RateLimitError::PeerBanned);
        }

        // Check message size
        if message_size > self.config.max_message_size {
            self.record_violation(peer_id);
            return Err(RateLimitError::MessageTooLarge {
                size: message_size,
                max: self.config.max_message_size,
            });
        }

        // Get or create peer stats
        let stats = self.peer_stats.entry(*peer_id).or_insert_with(PeerStats::new);

        // Reset window if necessary
        stats.maybe_reset_window(self.config.window_duration);

        // Check message count
        if stats.messages_in_window >= self.config.max_messages_per_window {
            let count = stats.messages_in_window;
            let max = self.config.max_messages_per_window;
            // Drop stats borrow before recording violation
            let _ = stats;
            self.record_violation(peer_id);
            return Err(RateLimitError::TooManyMessages { count, max });
        }

        // Check bandwidth
        if stats.bytes_in_window + message_size
            > self.config.max_bandwidth_per_peer * self.config.window_duration.as_secs() as usize
        {
            // Drop stats borrow before recording violation
            let _ = stats;
            self.record_violation(peer_id);
            return Err(RateLimitError::BandwidthExceeded);
        }

        // Record message
        stats.messages_in_window += 1;
        stats.bytes_in_window += message_size;

        Ok(())
    }

    /// Check if a peer is banned
    pub fn is_banned(&mut self, peer_id: &PeerId) -> bool {
        if let Some(ban_time) = self.banned_peers.get(peer_id) {
            if ban_time.elapsed() < self.ban_duration {
                return true;
            } else {
                // Ban expired
                self.banned_peers.remove(peer_id);
                // Reset violations
                if let Some(stats) = self.peer_stats.get_mut(peer_id) {
                    stats.violations = 0;
                }
            }
        }
        false
    }

    /// Record a violation
    fn record_violation(&mut self, peer_id: &PeerId) {
        let stats = self.peer_stats.entry(*peer_id).or_insert_with(PeerStats::new);
        stats.record_violation();

        // Ban if too many violations
        if stats.should_ban() {
            self.banned_peers.insert(*peer_id, Instant::now());
            tracing::warn!("⚠️  Peer {:?} banned for repeated violations", peer_id);
        }
    }

    /// Manually ban a peer
    pub fn ban_peer(&mut self, peer_id: PeerId) {
        self.banned_peers.insert(peer_id, Instant::now());
    }

    /// Unban a peer
    pub fn unban_peer(&mut self, peer_id: &PeerId) {
        self.banned_peers.remove(peer_id);
        if let Some(stats) = self.peer_stats.get_mut(peer_id) {
            stats.violations = 0;
        }
    }

    /// Clean up old entries
    pub fn cleanup(&mut self) {
        let now = Instant::now();

        // Remove inactive peers after 1 hour
        self.peer_stats.retain(|_, stats| {
            stats.window_start.elapsed() < Duration::from_secs(3600)
        });

        // Remove expired bans
        self.banned_peers
            .retain(|_, ban_time| now.duration_since(*ban_time) < self.ban_duration);
    }

    /// Get peer statistics
    pub fn get_peer_stats(&self, peer_id: &PeerId) -> Option<&PeerStats> {
        self.peer_stats.get(peer_id)
    }

    /// Number of banned peers
    pub fn banned_count(&self) -> usize {
        self.banned_peers.len()
    }

    /// List of banned peers
    pub fn banned_peers(&self) -> Vec<PeerId> {
        self.banned_peers.keys().copied().collect()
    }
}

/// Rate limiting errors
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Peer temporarily banned")]
    PeerBanned,

    #[error("Message too large: {size} bytes (max: {max})")]
    MessageTooLarge { size: usize, max: usize },

    #[error("Too many messages: {count} (max: {max})")]
    TooManyMessages { count: usize, max: usize },

    #[error("Bandwidth exceeded")]
    BandwidthExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_peer_id() -> PeerId {
        PeerId::random()
    }

    #[test]
    fn test_rate_limiter_allows_normal_traffic() {
        let config = RateLimitConfig {
            max_messages_per_window: 10,
            window_duration: Duration::from_secs(10),
            max_message_size: 1024,
            max_connections_per_peer: 3,
            max_bandwidth_per_peer: 10240,
        };

        let mut limiter = NetworkRateLimiter::new(config);
        let peer = create_peer_id();

        // Send 5 messages of 100 bytes each
        for _ in 0..5 {
            assert!(limiter.check_message(&peer, 100).is_ok());
        }
    }

    #[test]
    fn test_rate_limiter_blocks_too_many_messages() {
        let config = RateLimitConfig {
            max_messages_per_window: 5,
            window_duration: Duration::from_secs(10),
            max_message_size: 1024,
            max_connections_per_peer: 3,
            max_bandwidth_per_peer: 10240,
        };

        let mut limiter = NetworkRateLimiter::new(config);
        let peer = create_peer_id();

        // Send 5 messages (max)
        for _ in 0..5 {
            assert!(limiter.check_message(&peer, 100).is_ok());
        }

        // The 6th should be blocked
        assert!(matches!(
            limiter.check_message(&peer, 100),
            Err(RateLimitError::TooManyMessages { .. })
        ));
    }

    #[test]
    fn test_rate_limiter_blocks_large_messages() {
        let config = RateLimitConfig {
            max_messages_per_window: 10,
            window_duration: Duration::from_secs(10),
            max_message_size: 1024,
            max_connections_per_peer: 3,
            max_bandwidth_per_peer: 10240,
        };

        let mut limiter = NetworkRateLimiter::new(config);
        let peer = create_peer_id();

        // Message too large
        assert!(matches!(
            limiter.check_message(&peer, 2048),
            Err(RateLimitError::MessageTooLarge { .. })
        ));
    }

    #[test]
    fn test_rate_limiter_bans_after_violations() {
        let config = RateLimitConfig {
            max_messages_per_window: 1,
            window_duration: Duration::from_secs(10),
            max_message_size: 1024,
            max_connections_per_peer: 3,
            max_bandwidth_per_peer: 10240,
        };

        let mut limiter = NetworkRateLimiter::new(config);
        let peer = create_peer_id();

        // Cause 5 violations
        for _ in 0..6 {
            let _ = limiter.check_message(&peer, 100);
        }

        // Peer should be banned now
        assert!(limiter.is_banned(&peer));
        assert!(matches!(
            limiter.check_message(&peer, 100),
            Err(RateLimitError::PeerBanned)
        ));
    }

    #[test]
    fn test_rate_limiter_cleanup() {
        let config = RateLimitConfig::default();
        let mut limiter = NetworkRateLimiter::new(config);

        let peer1 = create_peer_id();
        let peer2 = create_peer_id();

        limiter.check_message(&peer1, 100).unwrap();
        limiter.check_message(&peer2, 100).unwrap();

        assert_eq!(limiter.peer_stats.len(), 2);

        limiter.cleanup();

        // Recent peers should not be removed
        assert_eq!(limiter.peer_stats.len(), 2);
    }
}
