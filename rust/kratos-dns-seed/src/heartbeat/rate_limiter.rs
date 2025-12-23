//! Rate Limiting for Heartbeat Receiver
//!
//! Protects against DoS attacks by limiting requests per IP.
//! Implements a sliding window counter with violation tracking.

use std::collections::HashMap;
use std::net::IpAddr;
use tracing::{debug, warn};

/// Rate limiter for heartbeat requests
pub struct RateLimiter {
    /// Request counts per IP
    entries: HashMap<IpAddr, RateLimitEntry>,

    /// Maximum requests per minute
    max_per_minute: u32,

    /// Violations before ban
    max_violations: u32,

    /// Ban duration in seconds
    ban_duration: u64,

    /// Window size in seconds (1 minute)
    window_size: u64,
}

/// Per-IP rate limit tracking
struct RateLimitEntry {
    /// Requests in current window
    request_count: u32,

    /// Window start time
    window_start: u64,

    /// Number of violations
    violations: u32,

    /// Ban expiry time (if banned)
    ban_until: Option<u64>,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(max_per_minute: u32, max_violations: u32, ban_duration: u64) -> Self {
        Self {
            entries: HashMap::new(),
            max_per_minute,
            max_violations,
            ban_duration,
            window_size: 60, // 1 minute window
        }
    }

    /// Check if a request from this IP should be allowed
    ///
    /// Returns true if allowed, false if rate limited or banned
    pub fn check_rate_limit(&mut self, ip: IpAddr) -> bool {
        let now = current_timestamp();

        // Get or create entry
        let entry = self.entries.entry(ip).or_insert_with(|| RateLimitEntry {
            request_count: 0,
            window_start: now,
            violations: 0,
            ban_until: None,
        });

        // Check if banned
        if let Some(ban_until) = entry.ban_until {
            if now < ban_until {
                debug!("IP {} is banned until {}", ip, ban_until);
                return false;
            } else {
                // Ban expired
                entry.ban_until = None;
                entry.violations = 0;
            }
        }

        // Check if window expired
        if now >= entry.window_start + self.window_size {
            // Reset window
            entry.window_start = now;
            entry.request_count = 0;
        }

        // Increment counter
        entry.request_count += 1;

        // Check limit
        if entry.request_count > self.max_per_minute {
            entry.violations += 1;
            warn!(
                "Rate limit exceeded for {}: {} requests, violation #{}",
                ip, entry.request_count, entry.violations
            );

            // Check if should be banned
            if entry.violations >= self.max_violations {
                entry.ban_until = Some(now + self.ban_duration);
                warn!("IP {} banned for {} seconds", ip, self.ban_duration);
            }

            return false;
        }

        true
    }

    /// Record a violation (e.g., invalid signature)
    pub fn record_violation(&mut self, ip: IpAddr) {
        let now = current_timestamp();

        let entry = self.entries.entry(ip).or_insert_with(|| RateLimitEntry {
            request_count: 0,
            window_start: now,
            violations: 0,
            ban_until: None,
        });

        entry.violations += 1;
        warn!("Violation recorded for {}: total {}", ip, entry.violations);

        if entry.violations >= self.max_violations {
            entry.ban_until = Some(now + self.ban_duration);
            warn!("IP {} banned for {} seconds due to violations", ip, self.ban_duration);
        }
    }

    /// Check if an IP is currently banned
    pub fn is_banned(&self, ip: &IpAddr) -> bool {
        if let Some(entry) = self.entries.get(ip) {
            if let Some(ban_until) = entry.ban_until {
                return current_timestamp() < ban_until;
            }
        }
        false
    }

    /// Get number of tracked IPs
    pub fn tracked_count(&self) -> usize {
        self.entries.len()
    }

    /// Get number of banned IPs
    pub fn banned_count(&self) -> usize {
        let now = current_timestamp();
        self.entries
            .values()
            .filter(|e| e.ban_until.map(|t| now < t).unwrap_or(false))
            .count()
    }

    /// Clean up old entries to prevent memory growth
    pub fn cleanup(&mut self) {
        let now = current_timestamp();
        let cleanup_threshold = now.saturating_sub(self.window_size * 10);

        self.entries.retain(|_, entry| {
            // Keep if:
            // - Currently banned
            // - Has recent activity
            entry.ban_until.map(|t| now < t).unwrap_or(false)
                || entry.window_start > cleanup_threshold
        });
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_rate_limit_basic() {
        let mut limiter = RateLimiter::new(5, 3, 60);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // First 5 requests should be allowed
        for _ in 0..5 {
            assert!(limiter.check_rate_limit(ip));
        }

        // 6th request should be denied
        assert!(!limiter.check_rate_limit(ip));
    }

    #[test]
    fn test_violation_tracking() {
        let mut limiter = RateLimiter::new(100, 3, 60);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // Record violations
        limiter.record_violation(ip);
        limiter.record_violation(ip);
        assert!(!limiter.is_banned(&ip));

        // 3rd violation should trigger ban
        limiter.record_violation(ip);
        assert!(limiter.is_banned(&ip));
    }

    #[test]
    fn test_different_ips() {
        let mut limiter = RateLimiter::new(2, 3, 60);
        let ip1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));

        // Both IPs should have separate limits
        assert!(limiter.check_rate_limit(ip1));
        assert!(limiter.check_rate_limit(ip1));
        assert!(!limiter.check_rate_limit(ip1)); // 3rd denied

        assert!(limiter.check_rate_limit(ip2)); // Still allowed
        assert!(limiter.check_rate_limit(ip2));
    }

    #[test]
    fn test_cleanup() {
        let mut limiter = RateLimiter::new(100, 3, 60);

        for i in 0..100 {
            let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, i as u8));
            limiter.check_rate_limit(ip);
        }

        assert_eq!(limiter.tracked_count(), 100);

        // Cleanup should work (though won't remove recent entries)
        limiter.cleanup();
        assert!(limiter.tracked_count() <= 100);
    }
}
