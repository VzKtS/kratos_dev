// Server RPC - JSON-RPC HTTP Server using warp
//
// Note: Due to libp2p's Swarm not being Sync, we cannot directly share the node
// with warp handlers. Instead, we use a channel-based approach where requests
// are sent to the node's async context for processing.

use crate::rpc::rate_limit::{RateLimitConfig, RpcRateLimiter};
use crate::rpc::types::{
    BlockInfo, BlockWithTransactions, ChainInfo, HealthStatus, JsonRpcError, JsonRpcId,
    JsonRpcRequest, JsonRpcResponse, MempoolStats, MempoolStatus, NetworkStatus, SyncStatus,
    SystemInfo, TransactionSubmitResult, AccountInfoRpc, parse_account_id, parse_hash,
};
use crate::types::*;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{debug, info, warn};
use warp::Filter;

// =============================================================================
// RPC REQUEST/RESPONSE TYPES FOR CHANNEL
// =============================================================================

/// Internal RPC request sent over channel
pub enum RpcCall {
    ChainGetInfo(oneshot::Sender<Result<ChainInfo, String>>),
    ChainGetBlock(BlockNumber, oneshot::Sender<Result<BlockWithTransactions, String>>),
    ChainGetLatestBlock(oneshot::Sender<Result<BlockWithTransactions, String>>),
    StateGetBalance(AccountId, oneshot::Sender<Result<Balance, String>>),
    StateGetAccount(AccountId, oneshot::Sender<Result<AccountInfoRpc, String>>),
    SystemHealth(oneshot::Sender<HealthStatus>),
    SystemInfo(oneshot::Sender<Result<SystemInfo, String>>),
    SystemPeers(oneshot::Sender<(usize, Vec<String>)>),
    SyncState(oneshot::Sender<SyncStatus>),
    MempoolStatus(oneshot::Sender<MempoolStatus>),
    SubmitTransaction(SignedTransaction, oneshot::Sender<Result<Hash, String>>),
    GetVersion(oneshot::Sender<String>),
}

/// Channel sender for RPC calls
pub type RpcSender = mpsc::UnboundedSender<RpcCall>;

// =============================================================================
// SIMPLE RPC STATE (For handlers that don't need node access)
// =============================================================================

/// Simple state that can be shared with warp
#[derive(Clone)]
pub struct RpcState {
    /// Channel to send requests to the node
    pub tx: RpcSender,
    /// SECURITY FIX #29: Rate limiter for DoS protection
    pub rate_limiter: Option<RpcRateLimiter>,
}

impl RpcState {
    pub fn new(tx: RpcSender) -> Self {
        Self {
            tx,
            rate_limiter: None,
        }
    }

    /// Create with rate limiting enabled (SECURITY FIX #29)
    pub fn with_rate_limiter(tx: RpcSender, config: RateLimitConfig) -> Self {
        Self {
            tx,
            rate_limiter: Some(RpcRateLimiter::new(config)),
        }
    }
}

// =============================================================================
// RPC SERVER
// =============================================================================

/// JSON-RPC HTTP Server
pub struct RpcServer {
    /// Listen port
    port: u16,
    /// Listen address
    address: [u8; 4],
    /// SECURITY FIX #3: Allowed CORS origins (empty = localhost only)
    allowed_origins: Vec<String>,
}

impl RpcServer {
    /// Create a new RPC server
    pub fn new(port: u16) -> Self {
        Self {
            port,
            address: [127, 0, 0, 1], // Default: localhost only
            allowed_origins: vec![], // SECURITY: No external origins by default
        }
    }

    /// Create a new RPC server with custom address
    pub fn with_address(port: u16, address: [u8; 4]) -> Self {
        Self {
            port,
            address,
            allowed_origins: vec![],
        }
    }

    /// SECURITY FIX #3: Create RPC server with explicit allowed origins
    /// Use this for production deployments where you need CORS access
    pub fn with_cors_origins(port: u16, address: [u8; 4], allowed_origins: Vec<String>) -> Self {
        Self {
            port,
            address,
            allowed_origins,
        }
    }

    /// Get the socket address
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::from((self.address, self.port))
    }

    /// SECURITY FIX #3: Build a secure CORS filter
    /// Only allows specified origins, or localhost-only if none specified
    fn build_cors_filter(&self) -> warp::cors::Builder {
        let mut cors = warp::cors()
            .allow_methods(vec!["GET", "POST", "OPTIONS"])
            .allow_headers(vec!["Content-Type", "Accept"]);

        if self.allowed_origins.is_empty() {
            // SECURITY: When no origins specified, only allow localhost
            // This prevents CSRF attacks from malicious websites
            cors = cors
                .allow_origin("http://localhost")
                .allow_origin("http://127.0.0.1")
                .allow_origin("http://localhost:3000") // Common dev frontend port
                .allow_origin("http://127.0.0.1:3000");
            info!("CORS: Restricted to localhost only");
        } else {
            // Explicitly specified origins
            for origin in &self.allowed_origins {
                cors = cors.allow_origin(origin.as_str());
            }
            info!("CORS: Allowed origins: {:?}", self.allowed_origins);
        }

        cors
    }

    /// Start the server (blocking) with an RPC channel
    /// SECURITY FIX #29: Now includes rate limiting by default
    pub async fn start(self, rpc_tx: RpcSender) -> Result<(), RpcServerError> {
        let addr = self.socket_addr();
        info!("Starting RPC server on {}", addr);

        // SECURITY FIX #29: Create state with rate limiting enabled
        let rate_limit_config = RateLimitConfig {
            max_requests: 100,
            window_duration: Duration::from_secs(10),
            ban_duration: Duration::from_secs(300),
            max_violations: 3,
        };
        let state = RpcState::with_rate_limiter(rpc_tx, rate_limit_config);
        info!("Rate limiting enabled: 100 req/10s");

        // JSON-RPC endpoint with rate limiting
        let rpc = warp::path::end()
            .and(warp::post())
            .and(warp::addr::remote())
            .and(warp::body::json())
            .and(with_state(state.clone()))
            .and_then(handle_rpc_request_with_rate_limit);

        // Health check endpoint (no rate limiting for health checks)
        let health = warp::path("health")
            .and(warp::get())
            .and(with_state(state.clone()))
            .and_then(handle_health_check);

        // SECURITY FIX #3: Build secure CORS configuration
        let cors = self.build_cors_filter();

        // Combine routes
        let routes = rpc.or(health).with(cors).with(warp::log("rpc"));

        // Start server
        info!("RPC server ready on http://{}", addr);
        warp::serve(routes).run(addr).await;

        Ok(())
    }

    /// Start the server in background, returns shutdown handle
    pub async fn start_background(self, rpc_tx: RpcSender) -> Result<RpcServerHandle, RpcServerError> {
        let addr = self.socket_addr();
        info!("Starting RPC server on {} (background)", addr);

        // SECURITY FIX #3: Build secure CORS configuration before moving self
        let cors = self.build_cors_filter();

        let state = RpcState::new(rpc_tx);

        // JSON-RPC endpoint
        let rpc = warp::path::end()
            .and(warp::post())
            .and(warp::body::json())
            .and(with_state(state.clone()))
            .and_then(handle_rpc_request);

        // Health check endpoint
        let health = warp::path("health")
            .and(warp::get())
            .and(with_state(state.clone()))
            .and_then(handle_health_check);

        // Combine routes
        let routes = rpc.or(health).with(cors);

        // Create shutdown channel
        let (tx, rx) = oneshot::channel::<()>();

        // Start server with graceful shutdown
        let (bound_addr, server) = warp::serve(routes).bind_with_graceful_shutdown(addr, async {
            rx.await.ok();
        });

        info!("RPC server ready on http://{}", bound_addr);

        // Spawn server task
        tokio::spawn(server);

        Ok(RpcServerHandle {
            addr: bound_addr,
            shutdown_tx: Some(tx),
        })
    }
}

/// Handle for a running RPC server
pub struct RpcServerHandle {
    /// Server address
    pub addr: SocketAddr,
    /// Shutdown signal sender
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl RpcServerHandle {
    /// Shutdown the server
    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Get the server address
    pub fn address(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for RpcServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

// =============================================================================
// REQUEST HANDLERS
// =============================================================================

/// Filter to inject state into handlers
fn with_state(
    state: RpcState,
) -> impl Filter<Extract = (RpcState,), Error = Infallible> + Clone {
    warp::any().map(move || state.clone())
}

/// SECURITY FIX #29: Handle RPC request with rate limiting
async fn handle_rpc_request_with_rate_limit(
    remote_addr: Option<SocketAddr>,
    request: JsonRpcRequest,
    state: RpcState,
) -> Result<impl warp::Reply, Infallible> {
    // Extract client IP for rate limiting
    let client_ip = remote_addr.map(|addr| addr.ip()).unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

    // Check rate limit if enabled
    if let Some(ref rate_limiter) = state.rate_limiter {
        if let Err(retry_after) = rate_limiter.check_rate_limit(client_ip).await {
            warn!("Rate limit exceeded for IP {}, retry after {} seconds", client_ip, retry_after);
            let response = JsonRpcResponse::error(
                request.id,
                JsonRpcError::rate_limited(retry_after),
            );
            return Ok(warp::reply::json(&response));
        }
    }

    debug!("RPC request from {}: {}", client_ip, request.method);

    // Validate JSON-RPC version
    if request.jsonrpc != "2.0" {
        let response = JsonRpcResponse::error(
            request.id,
            JsonRpcError::invalid_request("Invalid JSON-RPC version"),
        );
        return Ok(warp::reply::json(&response));
    }

    let response = route_request(request, &state).await;
    Ok(warp::reply::json(&response))
}

/// Handle a single JSON-RPC request (without rate limiting - for internal use)
async fn handle_rpc_request(
    request: JsonRpcRequest,
    state: RpcState,
) -> Result<impl warp::Reply, Infallible> {
    debug!("RPC request: {}", request.method);

    // Validate JSON-RPC version
    if request.jsonrpc != "2.0" {
        let response = JsonRpcResponse::error(
            request.id,
            JsonRpcError::invalid_request("Invalid JSON-RPC version"),
        );
        return Ok(warp::reply::json(&response));
    }

    let response = route_request(request, &state).await;
    Ok(warp::reply::json(&response))
}

/// Route request to appropriate handler
async fn route_request(request: JsonRpcRequest, state: &RpcState) -> JsonRpcResponse {
    match request.method.as_str() {
        // Chain methods
        "chain_getInfo" => handle_chain_get_info(request.id, state).await,
        "chain_getBlock" => handle_chain_get_block(request.id, request.params, state).await,
        "chain_getBlockByNumber" => handle_chain_get_block(request.id, request.params, state).await,
        "chain_getLatestBlock" => handle_chain_get_latest_block(request.id, state).await,

        // State methods
        "state_getAccount" => handle_state_get_account(request.id, request.params, state).await,
        "state_getBalance" => handle_state_get_balance(request.id, request.params, state).await,

        // Author methods
        "author_submitTransaction" => handle_submit_transaction(request.id, request.params, state).await,
        "author_pendingTransactions" => handle_mempool_status(request.id, state).await,

        // System methods
        "system_info" => handle_system_info(request.id, state).await,
        "system_health" => handle_system_health(request.id, state).await,
        "system_peers" => handle_system_peers(request.id, state).await,
        "system_syncState" => handle_sync_state(request.id, state).await,
        "system_version" => handle_system_version(request.id, state).await,
        "system_name" => JsonRpcResponse::success(request.id, "KratOs Node"),

        // Mempool methods
        "mempool_status" => handle_mempool_status(request.id, state).await,

        // Unknown method
        _ => JsonRpcResponse::error(request.id, JsonRpcError::method_not_found(&request.method)),
    }
}

// =============================================================================
// INDIVIDUAL HANDLERS
// =============================================================================

async fn handle_chain_get_info(id: JsonRpcId, state: &RpcState) -> JsonRpcResponse {
    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::ChainGetInfo(tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(Ok(info)) => JsonRpcResponse::success(id, info),
        Ok(Err(e)) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&e)),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_chain_get_block(id: JsonRpcId, params: serde_json::Value, state: &RpcState) -> JsonRpcResponse {
    let number: u64 = match params {
        serde_json::Value::Array(arr) if !arr.is_empty() => {
            match arr[0].as_u64() {
                Some(n) => n,
                None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected block number")),
            }
        }
        _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [number]")),
    };

    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::ChainGetBlock(number, tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(Ok(block)) => JsonRpcResponse::success(id, block),
        Ok(Err(_)) => JsonRpcResponse::error(id, JsonRpcError::block_not_found()),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_chain_get_latest_block(id: JsonRpcId, state: &RpcState) -> JsonRpcResponse {
    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::ChainGetLatestBlock(tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(Ok(block)) => JsonRpcResponse::success(id, block),
        Ok(Err(e)) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&e)),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_state_get_account(id: JsonRpcId, params: serde_json::Value, state: &RpcState) -> JsonRpcResponse {
    let address_str: String = match params {
        serde_json::Value::Array(arr) if !arr.is_empty() => {
            match arr[0].as_str() {
                Some(s) => s.to_string(),
                None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected address string")),
            }
        }
        _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [address]")),
    };

    let account_id = match parse_account_id(&address_str) {
        Ok(a) => a,
        Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
    };

    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::StateGetAccount(account_id, tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(Ok(info)) => JsonRpcResponse::success(id, info),
        Ok(Err(e)) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&e)),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_state_get_balance(id: JsonRpcId, params: serde_json::Value, state: &RpcState) -> JsonRpcResponse {
    let address_str: String = match params {
        serde_json::Value::Array(arr) if !arr.is_empty() => {
            match arr[0].as_str() {
                Some(s) => s.to_string(),
                None => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected address string")),
            }
        }
        _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [address]")),
    };

    let account_id = match parse_account_id(&address_str) {
        Ok(a) => a,
        Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&e)),
    };

    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::StateGetBalance(account_id, tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(Ok(balance)) => JsonRpcResponse::success(id, balance),
        Ok(Err(e)) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&e)),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_submit_transaction(id: JsonRpcId, params: serde_json::Value, state: &RpcState) -> JsonRpcResponse {
    let tx_data: SignedTransaction = match params {
        serde_json::Value::Array(arr) if !arr.is_empty() => {
            match serde_json::from_value(arr[0].clone()) {
                Ok(tx) => tx,
                Err(e) => return JsonRpcResponse::error(id, JsonRpcError::invalid_params(&format!("Invalid transaction: {}", e))),
            }
        }
        _ => return JsonRpcResponse::error(id, JsonRpcError::invalid_params("Expected [transaction]")),
    };

    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::SubmitTransaction(tx_data, tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(Ok(hash)) => {
            let result = TransactionSubmitResult {
                hash: format!("0x{}", hex::encode(hash.as_bytes())),
                message: "Transaction submitted successfully".to_string(),
            };
            JsonRpcResponse::success(id, result)
        }
        Ok(Err(e)) => JsonRpcResponse::error(id, JsonRpcError::transaction_rejected(&e)),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_system_info(id: JsonRpcId, state: &RpcState) -> JsonRpcResponse {
    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::SystemInfo(tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(Ok(info)) => JsonRpcResponse::success(id, info),
        Ok(Err(e)) => JsonRpcResponse::error(id, JsonRpcError::internal_error(&e)),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_system_health(id: JsonRpcId, state: &RpcState) -> JsonRpcResponse {
    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::SystemHealth(tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(health) => JsonRpcResponse::success(id, health),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_system_peers(id: JsonRpcId, state: &RpcState) -> JsonRpcResponse {
    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::SystemPeers(tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok((count, peers)) => JsonRpcResponse::success(id, serde_json::json!({
            "count": count,
            "peers": peers
        })),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_sync_state(id: JsonRpcId, state: &RpcState) -> JsonRpcResponse {
    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::SyncState(tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(status) => JsonRpcResponse::success(id, status),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

async fn handle_system_version(id: JsonRpcId, state: &RpcState) -> JsonRpcResponse {
    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::GetVersion(tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(version) => JsonRpcResponse::success(id, version),
        Err(_) => JsonRpcResponse::success(id, env!("CARGO_PKG_VERSION")),
    }
}

async fn handle_mempool_status(id: JsonRpcId, state: &RpcState) -> JsonRpcResponse {
    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::MempoolStatus(tx)).is_err() {
        return JsonRpcResponse::error(id, JsonRpcError::internal_error("Node unavailable"));
    }
    match rx.await {
        Ok(status) => JsonRpcResponse::success(id, status),
        Err(_) => JsonRpcResponse::error(id, JsonRpcError::internal_error("Request timeout")),
    }
}

/// Handle health check request
async fn handle_health_check(state: RpcState) -> Result<impl warp::Reply, Infallible> {
    let (tx, rx) = oneshot::channel();
    if state.tx.send(RpcCall::SystemHealth(tx)).is_err() {
        let health = HealthStatus {
            healthy: false,
            is_synced: false,
            has_peers: false,
            block_height: 0,
            peer_count: 0,
        };
        return Ok(warp::reply::json(&health));
    }
    match rx.await {
        Ok(health) => Ok(warp::reply::json(&health)),
        Err(_) => {
            let health = HealthStatus {
                healthy: false,
                is_synced: false,
                has_peers: false,
                block_height: 0,
                peer_count: 0,
            };
            Ok(warp::reply::json(&health))
        }
    }
}

// =============================================================================
// RPC SERVER ERROR
// =============================================================================

/// RPC Server errors
#[derive(Debug, thiserror::Error)]
pub enum RpcServerError {
    #[error("Bind error: {0}")]
    BindError(String),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

// =============================================================================
// RPC CONFIG
// =============================================================================

/// RPC server configuration
#[derive(Debug, Clone)]
pub struct RpcConfig {
    /// Enable RPC server
    pub enabled: bool,
    /// Listen port
    pub port: u16,
    /// Listen address (0.0.0.0 for all interfaces)
    pub address: [u8; 4],
    /// Enable CORS
    pub cors: bool,
    /// SECURITY FIX #3: Allowed CORS origins (empty = localhost only)
    /// Specify explicit origins like "https://myapp.example.com" for production
    pub cors_origins: Vec<String>,
    /// Max request size in bytes
    pub max_request_size: usize,
    /// Rate limiting (requests per second)
    pub rate_limit: Option<u32>,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 9933,
            address: [127, 0, 0, 1], // localhost only by default
            cors: true,
            cors_origins: vec![], // SECURITY: Localhost only by default
            max_request_size: 10 * 1024 * 1024, // 10 MB
            rate_limit: Some(100),
        }
    }
}

impl RpcConfig {
    /// Create config for public access
    pub fn public() -> Self {
        Self {
            address: [0, 0, 0, 0], // All interfaces
            ..Default::default()
        }
    }

    /// Create config for development
    pub fn dev() -> Self {
        Self {
            rate_limit: None, // No rate limiting in dev
            ..Default::default()
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_server_creation() {
        let server = RpcServer::new(9933);
        assert_eq!(server.port, 9933);
        assert_eq!(server.socket_addr().port(), 9933);
    }

    #[test]
    fn test_rpc_server_with_address() {
        let server = RpcServer::with_address(9934, [0, 0, 0, 0]);
        assert_eq!(server.socket_addr().ip().to_string(), "0.0.0.0");
    }

    #[test]
    fn test_rpc_config_default() {
        let config = RpcConfig::default();
        assert!(config.enabled);
        assert_eq!(config.port, 9933);
        assert_eq!(config.address, [127, 0, 0, 1]);
    }

    #[test]
    fn test_rpc_config_public() {
        let config = RpcConfig::public();
        assert_eq!(config.address, [0, 0, 0, 0]);
    }

    #[tokio::test]
    async fn test_rpc_state() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let state = RpcState::new(tx);
        // State should be cloneable
        let _state2 = state.clone();
    }

    #[tokio::test]
    async fn test_json_rpc_routing() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let state = RpcState::new(tx);

        // Spawn a task to handle the request
        tokio::spawn(async move {
            if let Some(call) = rx.recv().await {
                match call {
                    RpcCall::GetVersion(resp) => {
                        let _ = resp.send("0.1.0".to_string());
                    }
                    _ => {}
                }
            }
        });

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "system_version".to_string(),
            params: serde_json::Value::Null,
            id: JsonRpcId::Number(1),
        };

        let response = route_request(request, &state).await;
        assert!(response.result.is_some());
    }
}
