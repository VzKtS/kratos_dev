//! HTTP Server for IDpeers.json Distribution
//!
//! Serves the IDpeers.json file over HTTP/HTTPS.
//! Also provides a simple health check endpoint.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::config::DnsSeedConfig;
use crate::distribution::IdPeersGenerator;
use crate::network_state::NetworkStateAggregator;
use crate::registry::PeerRegistry;

/// Shared application state
pub struct AppState {
    pub generator: Arc<RwLock<IdPeersGenerator>>,
    pub registry: Arc<RwLock<PeerRegistry>>,
    pub network_state: Arc<RwLock<NetworkStateAggregator>>,
    pub config: Arc<DnsSeedConfig>,
}

/// Run the HTTP distribution server
pub async fn run_distribution_server(
    config: Arc<DnsSeedConfig>,
    generator: Arc<RwLock<IdPeersGenerator>>,
    registry: Arc<RwLock<PeerRegistry>>,
    network_state: Arc<RwLock<NetworkStateAggregator>>,
) -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        generator,
        registry,
        network_state,
        config: config.clone(),
    });

    let app = Router::new()
        .route("/idpeers.json", get(get_idpeers))
        .route("/IDpeers.json", get(get_idpeers)) // Case-insensitive alias
        .route("/health", get(health_check))
        .route("/status", get(get_status))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.http_port));
    info!("🌐 HTTP server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// GET /idpeers.json - Returns the signed peer list
async fn get_idpeers(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Try to get from cache first
    {
        let gen = state.generator.read().await;
        if let Some(cached) = gen.get_cached() {
            return (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                cached.to_vec(),
            );
        }
    }

    // Generate fresh content
    let result = {
        let reg = state.registry.read().await;
        let net_state = state.network_state.read().await;
        let mut gen = state.generator.write().await;

        gen.generate(&reg, &net_state).await
    };

    match result {
        Ok(file) => {
            let json = serde_json::to_vec_pretty(&file).unwrap_or_default();
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                json,
            )
        }
        Err(e) => {
            error!("Failed to generate IDpeers.json: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                b"{\"error\": \"Failed to generate peer list\"}".to_vec(),
            )
        }
    }
}

/// GET /health - Simple health check
async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Status response
#[derive(serde::Serialize)]
struct StatusResponse {
    status: String,
    active_peers: usize,
    active_validators: u32,
    best_height: u64,
    security_state: String,
    uptime_secs: u64,
}

/// GET /status - Returns current DNS Seed status
async fn get_status(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let (active_peers, active_validators, best_height, security_state) = {
        let reg = state.registry.read().await;
        let net_state = state.network_state.read().await;
        let current = net_state.current_state();

        (
            reg.active_peer_count(),
            current.active_validators,
            current.best_height,
            format!("{:?}", current.security_state),
        )
    };

    let response = StatusResponse {
        status: "healthy".to_string(),
        active_peers,
        active_validators,
        best_height,
        security_state,
        uptime_secs: 0, // TODO: Track actual uptime
    };

    (StatusCode::OK, Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_serialization() {
        let status = StatusResponse {
            status: "healthy".to_string(),
            active_peers: 100,
            active_validators: 75,
            best_height: 12345,
            security_state: "Normal".to_string(),
            uptime_secs: 3600,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("healthy"));
        assert!(json.contains("12345"));
    }
}
