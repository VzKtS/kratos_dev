//! API Routes
//!
//! HTTP endpoints for metrics, health checks, and IDpeers.json

use axum::{
    extract::State,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::api::Metrics;
use crate::config::DnsSeedConfig;
use crate::distribution::IdPeersGenerator;
use crate::network_state::NetworkStateAggregator;
use crate::registry::PeerRegistry;

/// Shared API state
pub struct ApiState {
    pub config: Arc<DnsSeedConfig>,
    pub registry: Arc<RwLock<PeerRegistry>>,
    pub network_state: Arc<RwLock<NetworkStateAggregator>>,
    pub generator: Arc<RwLock<IdPeersGenerator>>,
    pub metrics: Arc<Metrics>,
}

/// Run the HTTP API server
pub async fn run_api_server(
    config: Arc<DnsSeedConfig>,
    registry: Arc<RwLock<PeerRegistry>>,
    network_state: Arc<RwLock<NetworkStateAggregator>>,
    generator: Arc<RwLock<IdPeersGenerator>>,
    metrics: Arc<Metrics>,
) -> anyhow::Result<()> {
    let state = Arc::new(ApiState {
        config: config.clone(),
        registry,
        network_state,
        generator,
        metrics,
    });

    let app = Router::new()
        // Health & Status
        .route("/health", get(health_check))
        .route("/status", get(get_status))

        // IDpeers.json
        .route("/idpeers.json", get(get_idpeers))
        .route("/IDpeers.json", get(get_idpeers))

        // Metrics
        .route("/metrics", get(get_metrics_prometheus))
        .route("/metrics/json", get(get_metrics_json))

        // Network info
        .route("/network", get(get_network_info))
        .route("/peers", get(get_peers))

        .with_state(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], config.api_port));
    info!("📊 HTTP API server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// GET /health - Simple health check
async fn health_check() -> impl IntoResponse {
    "OK"
}

/// GET /status - Detailed status
async fn get_status(State(state): State<Arc<ApiState>>) -> impl IntoResponse {
    let reg = state.registry.read().await;
    let net = state.network_state.read().await;
    let current = net.current_state();

    let status = serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": state.metrics.uptime_secs(),
        "network": {
            "active_peers": reg.active_peer_count(),
            "total_peers": reg.total_peer_count(),
            "active_validators": current.active_validators,
            "best_height": current.best_height,
            "security_state": format!("{:?}", current.security_state),
        }
    });

    Json(status)
}

/// GET /idpeers.json - Get the signed peer list
async fn get_idpeers(State(state): State<Arc<ApiState>>) -> impl IntoResponse {
    state.metrics.inc_idpeers_downloads();

    // Try cache first
    {
        let gen = state.generator.read().await;
        if let Some(cached) = gen.get_cached() {
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                cached.to_vec(),
            );
        }
    }

    // Generate fresh
    let result = {
        let reg = state.registry.read().await;
        let net = state.network_state.read().await;
        let mut gen = state.generator.write().await;

        gen.generate(&reg, &net).await
    };

    match result {
        Ok(file) => {
            let json = serde_json::to_vec_pretty(&file).unwrap_or_default();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                json,
            )
        }
        Err(_) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(header::CONTENT_TYPE, "application/json")],
                b"{\"error\": \"Failed to generate peer list\"}".to_vec(),
            )
        }
    }
}

/// GET /metrics - Prometheus format metrics
async fn get_metrics_prometheus(State(state): State<Arc<ApiState>>) -> impl IntoResponse {
    // Update metrics from registry
    {
        let reg = state.registry.read().await;
        let net = state.network_state.read().await;
        let current = net.current_state();

        state.metrics.set_active_peers(reg.active_peer_count() as u64);
        state.metrics.set_active_validators(current.active_validators as u64);
        state.metrics.set_best_height(current.best_height);
    }

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        state.metrics.to_prometheus(),
    )
}

/// GET /metrics/json - JSON format metrics
async fn get_metrics_json(State(state): State<Arc<ApiState>>) -> impl IntoResponse {
    // Update metrics from registry
    {
        let reg = state.registry.read().await;
        let net = state.network_state.read().await;
        let current = net.current_state();

        state.metrics.set_active_peers(reg.active_peer_count() as u64);
        state.metrics.set_active_validators(current.active_validators as u64);
        state.metrics.set_best_height(current.best_height);
    }

    Json(state.metrics.to_json())
}

/// GET /network - Network state information
async fn get_network_info(State(state): State<Arc<ApiState>>) -> impl IntoResponse {
    let net = state.network_state.read().await;
    let current = net.current_state();

    let info = serde_json::json!({
        "genesis_hash": hex::encode(current.genesis_hash),
        "best_height": current.best_height,
        "active_validators": current.active_validators,
        "security_state": format!("{:?}", current.security_state),
        "total_stake": current.total_stake.to_string(),
        "participation_rate": current.participation_rate,
        "estimated_inflation": current.estimated_inflation,
        "active_peers": current.active_peers,
        "timestamp": current.timestamp,
        "is_bootstrap": net.is_bootstrap(),
        "validator_trend": net.validator_trend(),
    });

    Json(info)
}

/// GET /peers - List of active peers (limited info)
async fn get_peers(State(state): State<Arc<ApiState>>) -> impl IntoResponse {
    let reg = state.registry.read().await;
    let timeout = state.config.peer_timeout_secs;

    let peers: Vec<_> = reg.get_active_peers(timeout)
        .iter()
        .take(50) // Limit to 50 peers
        .map(|p| serde_json::json!({
            "peer_id": hex::encode(&p.peer_id[..8]), // Shortened
            "addresses": p.addresses,
            "height": p.height,
            "is_validator": p.is_validator,
            "score": p.score,
            "region": p.region,
        }))
        .collect();

    Json(serde_json::json!({
        "count": peers.len(),
        "peers": peers,
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_status_format() {
        let status = serde_json::json!({
            "status": "healthy",
            "version": "0.1.0",
        });

        assert_eq!(status["status"], "healthy");
    }
}
