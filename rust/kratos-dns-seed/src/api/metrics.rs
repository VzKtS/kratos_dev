//! Metrics Collection
//!
//! Collects and exposes metrics for monitoring the DNS Seed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Metrics collector for DNS Seed
#[derive(Default)]
pub struct Metrics {
    /// Start time for uptime calculation
    start_time: Option<Instant>,

    /// Total heartbeats received
    pub heartbeats_received: AtomicU64,

    /// Successful heartbeats (accepted)
    pub heartbeats_accepted: AtomicU64,

    /// Rejected heartbeats
    pub heartbeats_rejected: AtomicU64,

    /// Rate-limited requests
    pub rate_limited_requests: AtomicU64,

    /// DNS queries served
    pub dns_queries: AtomicU64,

    /// IDpeers.json downloads
    pub idpeers_downloads: AtomicU64,

    /// Current active peers
    pub active_peers: AtomicU64,

    /// Current validators
    pub active_validators: AtomicU64,

    /// Best known block height
    pub best_height: AtomicU64,

    /// Banned IPs count
    pub banned_ips: AtomicU64,
}

impl Metrics {
    /// Create new metrics collector
    pub fn new() -> Self {
        Self {
            start_time: Some(Instant::now()),
            ..Default::default()
        }
    }

    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.start_time
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0)
    }

    /// Increment heartbeats received
    pub fn inc_heartbeats_received(&self) {
        self.heartbeats_received.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment accepted heartbeats
    pub fn inc_heartbeats_accepted(&self) {
        self.heartbeats_accepted.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment rejected heartbeats
    pub fn inc_heartbeats_rejected(&self) {
        self.heartbeats_rejected.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment rate-limited requests
    pub fn inc_rate_limited(&self) {
        self.rate_limited_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment DNS queries
    pub fn inc_dns_queries(&self) {
        self.dns_queries.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment IDpeers downloads
    pub fn inc_idpeers_downloads(&self) {
        self.idpeers_downloads.fetch_add(1, Ordering::Relaxed);
    }

    /// Update active peers count
    pub fn set_active_peers(&self, count: u64) {
        self.active_peers.store(count, Ordering::Relaxed);
    }

    /// Update active validators count
    pub fn set_active_validators(&self, count: u64) {
        self.active_validators.store(count, Ordering::Relaxed);
    }

    /// Update best height
    pub fn set_best_height(&self, height: u64) {
        self.best_height.store(height, Ordering::Relaxed);
    }

    /// Update banned IPs count
    pub fn set_banned_ips(&self, count: u64) {
        self.banned_ips.store(count, Ordering::Relaxed);
    }

    /// Export metrics in Prometheus format
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        // Uptime
        output.push_str(&format!(
            "# HELP kratos_dns_seed_uptime_seconds DNS Seed uptime in seconds\n\
             # TYPE kratos_dns_seed_uptime_seconds gauge\n\
             kratos_dns_seed_uptime_seconds {}\n\n",
            self.uptime_secs()
        ));

        // Heartbeats
        output.push_str(&format!(
            "# HELP kratos_dns_seed_heartbeats_total Total heartbeats received\n\
             # TYPE kratos_dns_seed_heartbeats_total counter\n\
             kratos_dns_seed_heartbeats_total {}\n\n",
            self.heartbeats_received.load(Ordering::Relaxed)
        ));

        output.push_str(&format!(
            "# HELP kratos_dns_seed_heartbeats_accepted Accepted heartbeats\n\
             # TYPE kratos_dns_seed_heartbeats_accepted counter\n\
             kratos_dns_seed_heartbeats_accepted {}\n\n",
            self.heartbeats_accepted.load(Ordering::Relaxed)
        ));

        output.push_str(&format!(
            "# HELP kratos_dns_seed_heartbeats_rejected Rejected heartbeats\n\
             # TYPE kratos_dns_seed_heartbeats_rejected counter\n\
             kratos_dns_seed_heartbeats_rejected {}\n\n",
            self.heartbeats_rejected.load(Ordering::Relaxed)
        ));

        // Rate limiting
        output.push_str(&format!(
            "# HELP kratos_dns_seed_rate_limited Rate-limited requests\n\
             # TYPE kratos_dns_seed_rate_limited counter\n\
             kratos_dns_seed_rate_limited {}\n\n",
            self.rate_limited_requests.load(Ordering::Relaxed)
        ));

        // DNS
        output.push_str(&format!(
            "# HELP kratos_dns_seed_dns_queries DNS queries served\n\
             # TYPE kratos_dns_seed_dns_queries counter\n\
             kratos_dns_seed_dns_queries {}\n\n",
            self.dns_queries.load(Ordering::Relaxed)
        ));

        // IDpeers downloads
        output.push_str(&format!(
            "# HELP kratos_dns_seed_idpeers_downloads IDpeers.json downloads\n\
             # TYPE kratos_dns_seed_idpeers_downloads counter\n\
             kratos_dns_seed_idpeers_downloads {}\n\n",
            self.idpeers_downloads.load(Ordering::Relaxed)
        ));

        // Network state
        output.push_str(&format!(
            "# HELP kratos_dns_seed_active_peers Active peers count\n\
             # TYPE kratos_dns_seed_active_peers gauge\n\
             kratos_dns_seed_active_peers {}\n\n",
            self.active_peers.load(Ordering::Relaxed)
        ));

        output.push_str(&format!(
            "# HELP kratos_dns_seed_active_validators Active validators count\n\
             # TYPE kratos_dns_seed_active_validators gauge\n\
             kratos_dns_seed_active_validators {}\n\n",
            self.active_validators.load(Ordering::Relaxed)
        ));

        output.push_str(&format!(
            "# HELP kratos_dns_seed_best_height Best known block height\n\
             # TYPE kratos_dns_seed_best_height gauge\n\
             kratos_dns_seed_best_height {}\n\n",
            self.best_height.load(Ordering::Relaxed)
        ));

        // Security
        output.push_str(&format!(
            "# HELP kratos_dns_seed_banned_ips Currently banned IPs\n\
             # TYPE kratos_dns_seed_banned_ips gauge\n\
             kratos_dns_seed_banned_ips {}\n\n",
            self.banned_ips.load(Ordering::Relaxed)
        ));

        output
    }

    /// Export metrics as JSON
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "uptime_secs": self.uptime_secs(),
            "heartbeats": {
                "received": self.heartbeats_received.load(Ordering::Relaxed),
                "accepted": self.heartbeats_accepted.load(Ordering::Relaxed),
                "rejected": self.heartbeats_rejected.load(Ordering::Relaxed),
            },
            "rate_limited": self.rate_limited_requests.load(Ordering::Relaxed),
            "dns_queries": self.dns_queries.load(Ordering::Relaxed),
            "idpeers_downloads": self.idpeers_downloads.load(Ordering::Relaxed),
            "network": {
                "active_peers": self.active_peers.load(Ordering::Relaxed),
                "active_validators": self.active_validators.load(Ordering::Relaxed),
                "best_height": self.best_height.load(Ordering::Relaxed),
            },
            "banned_ips": self.banned_ips.load(Ordering::Relaxed),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_increment() {
        let metrics = Metrics::new();

        metrics.inc_heartbeats_received();
        metrics.inc_heartbeats_received();
        metrics.inc_heartbeats_accepted();

        assert_eq!(metrics.heartbeats_received.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.heartbeats_accepted.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_prometheus_format() {
        let metrics = Metrics::new();
        metrics.set_active_peers(100);
        metrics.set_best_height(12345);

        let output = metrics.to_prometheus();

        assert!(output.contains("kratos_dns_seed_active_peers 100"));
        assert!(output.contains("kratos_dns_seed_best_height 12345"));
    }

    #[test]
    fn test_json_format() {
        let metrics = Metrics::new();
        metrics.set_active_validators(50);

        let json = metrics.to_json();

        assert_eq!(json["network"]["active_validators"], 50);
    }
}
