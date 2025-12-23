// Allow dead code - many functions are kept for API completeness and future use
#![allow(dead_code)]

//! KratOs DNS Seed Service
//!
//! Independent application for decentralized peer discovery in the KratOs network.
//!
//! ## Philosophy (aligned with KratOs Constitution)
//!
//! - **Decentralization**: Multiple independent DNS Seeds prevent single point of failure
//! - **Sovereignty**: Nodes can always fallback to hardcoded bootnodes
//! - **Resilience**: Signed peer lists and heartbeat verification prevent attacks
//! - **Exit Always Possible**: Nodes never depend solely on DNS Seeds
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    KRATOS DNS SEED                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Heartbeat Receiver (30334) ←── Nodes send status every 2m │
//! │  Peer Registry (RocksDB)    ←── Stores peer metadata       │
//! │  Network State Aggregator   ←── Computes network health    │
//! │  DNS Server (53)            ←── Responds to DNS queries    │
//! │  Peers File Generator       ←── Creates signed IDpeers.json│
//! │  HTTP API (8080)            ←── Metrics and monitoring     │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

mod config;
mod types;
mod crypto;
mod heartbeat;
mod registry;
mod network_state;
mod distribution;
mod dns;
mod api;

use config::DnsSeedConfig;
use registry::PeerRegistry;
use network_state::NetworkStateAggregator;
use distribution::IdPeersGenerator;
use api::Metrics;

/// KratOs DNS Seed - Decentralized peer discovery service
#[derive(Parser, Debug)]
#[command(name = "kratos-dns-seed")]
#[command(author = "KratOs Contributors")]
#[command(version = "0.1.0")]
#[command(about = "Independent DNS Seed service for KratOs network", long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "dns-seed.toml")]
    config: PathBuf,

    /// Data directory for peer registry and keys
    #[arg(short, long, default_value = "./data")]
    data_dir: PathBuf,

    /// Heartbeat receiver port
    #[arg(long, default_value = "30334")]
    heartbeat_port: u16,

    /// DNS server port (requires root or CAP_NET_BIND_SERVICE for port 53)
    #[arg(long, default_value = "5353")]
    dns_port: u16,

    /// HTTP API port for metrics
    #[arg(long, default_value = "8080")]
    api_port: u16,

    /// Genesis hash of the network (required for validation)
    #[arg(long)]
    genesis_hash: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Generate new signing keypair and exit
    #[arg(long)]
    generate_key: bool,

    /// Path to signing key file
    #[arg(long)]
    key_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log_level.clone().into())
        )
        .init();

    info!("🌐 KratOs DNS Seed Service v{}", env!("CARGO_PKG_VERSION"));
    info!("   Aligned with KratOs Constitution - Decentralization, Sovereignty, Resilience");

    // Handle key generation
    if args.generate_key {
        return generate_keypair(&args.data_dir).await;
    }

    // Create data directory
    tokio::fs::create_dir_all(&args.data_dir).await?;

    // Load or generate signing keypair
    let keypair = crypto::load_or_generate_keypair(&args.data_dir, args.key_file.as_ref()).await?;
    let seed_id = crypto::keypair_to_seed_id(&keypair);
    info!("📝 DNS Seed ID: {}", hex::encode(&seed_id));

    // Load configuration
    let config = if args.config.exists() {
        DnsSeedConfig::load(&args.config)?
    } else {
        warn!("Config file not found, using defaults");
        DnsSeedConfig::default()
    };

    // Override config with CLI args
    let config = config
        .with_heartbeat_port(args.heartbeat_port)
        .with_dns_port(args.dns_port)
        .with_api_port(args.api_port)
        .with_genesis_hash(args.genesis_hash);

    config.validate()?;

    info!("⚙️  Configuration:");
    info!("   Heartbeat port: {}", config.heartbeat_port);
    info!("   DNS port: {}", config.dns_port);
    info!("   API port: {}", config.api_port);
    info!("   Heartbeat interval: {}s", config.heartbeat_interval_secs);
    info!("   Peer timeout: {}s", config.peer_timeout_secs);

    let shared_config = Arc::new(config);

    // Initialize peer registry
    let registry_path = args.data_dir.join("peer_registry");
    let registry = Arc::new(RwLock::new(
        PeerRegistry::open(&registry_path)?
    ));
    info!("📦 Peer registry opened at {:?}", registry_path);

    // Get genesis hash for network state
    let genesis_hash = shared_config.genesis_hash
        .as_ref()
        .and_then(|h| crypto::hex_to_hash(h).ok())
        .unwrap_or([0u8; 32]);

    let genesis_timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Initialize network state aggregator
    let network_state = Arc::new(RwLock::new(
        NetworkStateAggregator::new(genesis_hash, genesis_timestamp)
    ));

    // Initialize IDpeers generator
    let idpeers_path = args.data_dir.join("idpeers.json");
    let generator = Arc::new(RwLock::new(
        IdPeersGenerator::new(keypair, shared_config.clone(), idpeers_path)
    ));

    // Initialize metrics
    let metrics = Arc::new(Metrics::new());

    // Start all services concurrently
    let heartbeat_handle = tokio::spawn(heartbeat::run_receiver(
        shared_config.clone(),
        registry.clone(),
        network_state.clone(),
    ));

    let dns_handle = tokio::spawn(dns::run_dns_server(
        shared_config.clone(),
        registry.clone(),
    ));

    let idpeers_handle = tokio::spawn(distribution::generator::run_periodic_generation(
        generator.clone(),
        registry.clone(),
        network_state.clone(),
        shared_config.idpeers_update_interval_secs,
    ));

    let api_handle = tokio::spawn(api::run_api_server(
        shared_config.clone(),
        registry.clone(),
        network_state.clone(),
        generator.clone(),
        metrics.clone(),
    ));

    let maintenance_handle = tokio::spawn(run_maintenance(
        shared_config.clone(),
        registry.clone(),
        network_state.clone(),
    ));

    info!("✅ All services started");
    info!("   Press Ctrl+C to shutdown gracefully");

    // Wait for shutdown signal
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("🛑 Shutdown signal received");
        }
        result = heartbeat_handle => {
            error!("Heartbeat receiver exited: {:?}", result);
        }
        result = dns_handle => {
            error!("DNS server exited: {:?}", result);
        }
        result = idpeers_handle => {
            error!("IDpeers generator exited: {:?}", result);
        }
        result = api_handle => {
            error!("HTTP API exited: {:?}", result);
        }
        result = maintenance_handle => {
            error!("Maintenance task exited: {:?}", result);
        }
    }

    // Graceful shutdown: flush registry
    {
        let reg = registry.read().await;
        reg.flush()?;
        info!("📦 Peer registry flushed to disk");
    }

    info!("👋 KratOs DNS Seed shutting down");
    Ok(())
}

/// Generate a new signing keypair for the DNS Seed
async fn generate_keypair(data_dir: &PathBuf) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(data_dir).await?;

    let keypair = crypto::generate_keypair();
    let seed_id = crypto::keypair_to_seed_id(&keypair);

    let key_path = data_dir.join("dns_seed.key");
    crypto::save_keypair(&keypair, &key_path).await?;

    info!("🔑 New keypair generated");
    info!("   Seed ID: {}", hex::encode(&seed_id));
    info!("   Key saved to: {:?}", key_path);
    info!("");
    info!("   Add this Seed ID to the official DNS Seeds list via governance.");

    Ok(())
}

/// Periodic maintenance tasks
async fn run_maintenance(
    config: Arc<DnsSeedConfig>,
    registry: Arc<RwLock<PeerRegistry>>,
    network_state: Arc<RwLock<NetworkStateAggregator>>,
) -> anyhow::Result<()> {
    let mut interval = tokio::time::interval(
        std::time::Duration::from_secs(config.maintenance_interval_secs)
    );

    loop {
        interval.tick().await;

        // Remove stale peers
        {
            let mut reg = registry.write().await;
            let removed = reg.remove_stale_peers(config.peer_timeout_secs);
            if removed > 0 {
                info!("🧹 Removed {} stale peers", removed);
            }
        }

        // Update network state aggregation
        {
            let reg = registry.read().await;
            let mut state = network_state.write().await;
            let peers = reg.get_active_peers(config.peer_timeout_secs);
            state.update_from_peers(&peers);
        }

        // Log current status periodically
        {
            let reg = registry.read().await;
            let state = network_state.read().await;
            let current = state.current_state();
            info!(
                "📊 Status: {} active peers, {} validators, height={}, state={:?}",
                reg.active_peer_count(),
                current.active_validators,
                current.best_height,
                current.security_state
            );
        }
    }
}
