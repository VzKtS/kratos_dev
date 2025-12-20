// RPC Rate Limiting - SECURITY FIX #29
// Prevents DoS attacks by limiting request rates per IP address

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::warn;

/// Rate limiter configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per window
    pub max_requests: u32,
    /// Time window duration
    pub window_duration: Duration,
    /// Ban duration after exceeding limit
    pub ban_duration: Duration,
    /// Maximum violations before ban
    pub max_violations: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window_duration: Duration::from_secs(10),
            ban_duration: Duration::from_secs(300), // 5 minutes
            max_violations: 3,
        }
    }
}

/// Rate limit entry for a single IP
#[derive(Debug, Clone)]
struct RateLimitEntry {
    /// Request count in current window
    request_count: u32,
    /// Window start time
    window_start: Instant,
    /// Number of violations
    violations: u32,
    /// Ban expiry time (if banned)
    banned_until: Option<Instant>,
}

impl RateLimitEntry {
    fn new() -> Self {
        Self {
            request_count: 0,
            window_start: Instant::now(),
            violations: 0,
            banned_until: None,
        }
    }
}

/// Thread-safe rate limiter
#[derive(Clone)]
pub struct RpcRateLimiter {
    config: RateLimitConfig,
    entries: Arc<RwLock<HashMap<IpAddr, RateLimitEntry>>>,
}

impl RpcRateLimiter {
    /// Create new rate limiter
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if request is allowed for IP address
    /// Returns Ok(()) if allowed, Err(remaining_seconds) if rate limited
    pub async fn check_rate_limit(&self, ip: IpAddr) -> Result<(), u64> {
        let mut entries = self.entries.write().await;
        let now = Instant::now();

        let entry = entries.entry(ip).or_insert_with(RateLimitEntry::new);

        // Check if banned
        if let Some(banned_until) = entry.banned_until {
            if now < banned_until {
                let remaining = banned_until.duration_since(now).as_secs();
                return Err(remaining);
            } else {
                // Ban expired, reset
                entry.banned_until = None;
                entry.violations = 0;
            }
        }

        // Check if window expired
        if now.duration_since(entry.window_start) >= self.config.window_duration {
            // Reset window
            entry.request_count = 0;
            entry.window_start = now;
        }

        // Increment counter
        entry.request_count += 1;

        // Check limit
        if entry.request_count > self.config.max_requests {
            entry.violations += 1;
            warn!(
                "Rate limit exceeded for IP {}: {} requests in window (violation #{})",
                ip, entry.request_count, entry.violations
            );

            // Ban if too many violations
            if entry.violations >= self.config.max_violations {
                entry.banned_until = Some(now + self.config.ban_duration);
                warn!("IP {} banned for {} seconds", ip, self.config.ban_duration.as_secs());
                return Err(self.config.ban_duration.as_secs());
            }

            // Return remaining time in current window
            let window_remaining = self.config.window_duration
                .saturating_sub(now.duration_since(entry.window_start));
            return Err(window_remaining.as_secs().max(1));
        }

        Ok(())
    }

    /// Get current stats for an IP
    pub async fn get_stats(&self, ip: &IpAddr) -> Option<(u32, bool)> {
        let entries = self.entries.read().await;
        entries.get(ip).map(|e| {
            let is_banned = e.banned_until.map(|t| Instant::now() < t).unwrap_or(false);
            (e.request_count, is_banned)
        })
    }

    /// Cleanup old entries (call periodically)
    pub async fn cleanup(&self) {
        let mut entries = self.entries.write().await;
        let now = Instant::now();
        let max_age = self.config.window_duration * 10; // Keep entries for 10 windows

        entries.retain(|_, entry| {
            // Keep if recently active or still banned
            let is_recent = now.duration_since(entry.window_start) < max_age;
            let is_banned = entry.banned_until.map(|t| now < t).unwrap_or(false);
            is_recent || is_banned
        });
    }
}

impl Default for RpcRateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn test_rate_limiting_allows_normal_traffic() {
        let config = RateLimitConfig {
            max_requests: 10,
            window_duration: Duration::from_secs(1),
            ban_duration: Duration::from_secs(60),
            max_violations: 3,
        };
        let limiter = RpcRateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Should allow 10 requests
        for _ in 0..10 {
            assert!(limiter.check_rate_limit(ip).await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_rate_limiting_blocks_excess_traffic() {
        let config = RateLimitConfig {
            max_requests: 5,
            window_duration: Duration::from_secs(10),
            ban_duration: Duration::from_secs(60),
            max_violations: 3,
        };
        let limiter = RpcRateLimiter::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // First 5 should pass
        for _ in 0..5 {
            assert!(limiter.check_rate_limit(ip).await.is_ok());
        }

        // 6th should fail
        assert!(limiter.check_rate_limit(ip).await.is_err());
    }

    #[tokio::test]
    async fn test_different_ips_have_separate_limits() {
        let config = RateLimitConfig {
            max_requests: 5,
            window_duration: Duration::from_secs(10),
            ban_duration: Duration::from_secs(60),
            max_violations: 3,
        };
        let limiter = RpcRateLimiter::new(config);
        let ip1 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));

        // Exhaust limit for ip1
        for _ in 0..6 {
            let _ = limiter.check_rate_limit(ip1).await;
        }

        // ip2 should still be allowed
        assert!(limiter.check_rate_limit(ip2).await.is_ok());
    }
}
