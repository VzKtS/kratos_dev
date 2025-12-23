//! HTTP API Module
//!
//! Provides metrics and monitoring endpoints for the DNS Seed.
//! Also handles IDpeers.json distribution.

mod routes;
mod metrics;

pub use routes::run_api_server;
pub use metrics::Metrics;
