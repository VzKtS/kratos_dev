//! Distribution Module
//!
//! Generates and serves the IDpeers.json file to nodes.
//! This file is signed by the DNS Seed and provides:
//! - Current network state
//! - List of active peers
//! - Fallback bootnodes

pub mod generator;
mod server;

pub use generator::{IdPeersGenerator, run_periodic_generation};
