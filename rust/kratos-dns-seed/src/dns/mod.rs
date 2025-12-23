//! DNS Server Module
//!
//! Provides DNS-based peer discovery for KratOs nodes.
//! Responds to DNS queries with peer IP addresses.
//!
//! ## DNS Records
//!
//! - A records: IPv4 addresses of active peers
//! - AAAA records: IPv6 addresses of active peers
//! - TXT records: Additional peer information (optional)

mod handler;
mod server;

pub use handler::KratosDnsHandler;
pub use server::run_dns_server;
