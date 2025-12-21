// Request-Response Protocol - Direct peer-to-peer message exchange
// Principle: Request specific data from specific peers with timeout handling

use crate::types::{AccountId, Balance, Block, BlockNumber, Hash};
use futures::prelude::*;
use libp2p::request_response::{self, Codec, ProtocolSupport};
use libp2p::StreamProtocol;
use serde::{Deserialize, Serialize};
use std::io;
use std::time::Duration;

// =============================================================================
// CONSTANTS
// =============================================================================

/// Protocol name for block requests
pub const BLOCK_PROTOCOL: &str = "/kratos/block/1.0.0";

/// Protocol name for sync requests
pub const SYNC_PROTOCOL: &str = "/kratos/sync/1.0.0";

/// Protocol name for status requests
pub const STATUS_PROTOCOL: &str = "/kratos/status/1.0.0";

/// Protocol name for genesis requests (used by joining nodes)
pub const GENESIS_PROTOCOL: &str = "/kratos/genesis/1.0.0";

/// Request timeout
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum request size (1 MB - requests should be small)
/// SECURITY FIX #11: Reduced from 10MB to 1MB to limit DoS attack surface
pub const MAX_REQUEST_SIZE: u64 = 1 * 1024 * 1024;

/// Maximum response size (10 MB for batch sync)
/// SECURITY FIX #11: Reduced from 50MB to 10MB to limit memory exhaustion
pub const MAX_RESPONSE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum blocks per sync request
/// SECURITY FIX #11: Limit to prevent memory exhaustion attacks
pub const MAX_SYNC_BLOCKS: u32 = 100;

// =============================================================================
// BLOCK REQUEST/RESPONSE
// =============================================================================

/// Request types for block protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockRequest {
    /// Request block by hash
    ByHash(Hash),

    /// Request block by number
    ByNumber(BlockNumber),

    /// Request block header only
    HeaderByNumber(BlockNumber),
}

/// Response for block requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockResponse {
    /// Block found
    Block(Block),

    /// Block not found
    NotFound,

    /// Error occurred
    Error(String),
}

// =============================================================================
// SYNC REQUEST/RESPONSE
// =============================================================================

/// Request for sync protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    /// Start block number
    pub from_block: BlockNumber,

    /// Maximum blocks to return
    pub max_blocks: u32,

    /// Whether to include block bodies
    pub include_bodies: bool,
}

/// Response for sync requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Blocks (or headers if bodies not requested)
    pub blocks: Vec<Block>,

    /// Whether there are more blocks available
    pub has_more: bool,

    /// Peer's current best height
    pub best_height: BlockNumber,
}

// =============================================================================
// STATUS REQUEST/RESPONSE
// =============================================================================

/// Status request (lightweight peer info exchange)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusRequest {
    /// Our best block number
    pub best_block: BlockNumber,

    /// Our best block hash
    pub best_hash: Hash,

    /// Genesis hash for chain validation
    pub genesis_hash: Hash,

    /// Protocol version
    pub protocol_version: u32,
}

/// Status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    /// Peer's best block number
    pub best_block: BlockNumber,

    /// Peer's best block hash
    pub best_hash: Hash,

    /// Genesis hash (must match ours)
    pub genesis_hash: Hash,

    /// Protocol version
    pub protocol_version: u32,

    /// Number of peers the peer is connected to
    pub peer_count: u32,
}

// =============================================================================
// GENESIS REQUEST/RESPONSE
// Used by joining nodes to receive genesis info before initialization
// =============================================================================

/// Genesis request (sent by joining nodes)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisRequest {
    /// Protocol version for compatibility check
    pub protocol_version: u32,
}

/// Genesis response (sent by existing nodes)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisResponse {
    /// Genesis block hash - canonical chain identifier
    pub genesis_hash: Hash,

    /// Full genesis block for validation
    pub genesis_block: Block,

    /// Chain name for verification
    pub chain_name: String,

    /// Protocol version
    pub protocol_version: u32,

    /// Genesis validators (needed for state initialization)
    /// This ensures joining nodes use the same validator set as the genesis node
    #[serde(default)]
    pub genesis_validators: Vec<GenesisValidatorInfo>,

    /// Genesis balances (account -> balance mapping)
    #[serde(default)]
    pub genesis_balances: Vec<(AccountId, Balance)>,
}

/// Validator info for genesis response (simplified for network transmission)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidatorInfo {
    pub account: AccountId,
    pub stake: Balance,
    pub is_bootstrap_validator: bool,
}

// =============================================================================
// UNIFIED REQUEST/RESPONSE
// =============================================================================

/// All request types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KratosRequest {
    Block(BlockRequest),
    Sync(SyncRequest),
    Status(StatusRequest),
    /// Genesis request - used by joining nodes to get genesis info
    Genesis(GenesisRequest),
}

/// All response types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KratosResponse {
    Block(BlockResponse),
    Sync(SyncResponse),
    Status(StatusResponse),
    /// Genesis response - sent to joining nodes
    Genesis(GenesisResponse),
}

// =============================================================================
// CODEC
// =============================================================================

/// Codec for KratOs request-response protocol
#[derive(Debug, Clone, Default)]
pub struct KratosCodec;

impl Codec for KratosCodec {
    type Protocol = StreamProtocol;
    type Request = KratosRequest;
    type Response = KratosResponse;

    fn read_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 Self::Protocol,
        io: &'life2 mut T,
    ) -> std::pin::Pin<Box<dyn Future<Output = io::Result<Self::Request>> + Send + 'async_trait>>
    where
        T: AsyncRead + Unpin + Send + 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            // Read length prefix (4 bytes)
            let mut len_buf = [0u8; 4];
            io.read_exact(&mut len_buf).await?;
            let len = u32::from_be_bytes(len_buf) as usize;

            // Validate size
            if len > MAX_REQUEST_SIZE as usize {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Request too large"));
            }

            // Read body
            let mut buf = vec![0u8; len];
            io.read_exact(&mut buf).await?;

            // Deserialize
            bincode::deserialize(&buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        })
    }

    fn read_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 Self::Protocol,
        io: &'life2 mut T,
    ) -> std::pin::Pin<Box<dyn Future<Output = io::Result<Self::Response>> + Send + 'async_trait>>
    where
        T: AsyncRead + Unpin + Send + 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            // Read length prefix
            let mut len_buf = [0u8; 4];
            io.read_exact(&mut len_buf).await?;
            let len = u32::from_be_bytes(len_buf) as usize;

            // Validate size
            if len > MAX_RESPONSE_SIZE as usize {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Response too large"));
            }

            // Read body
            let mut buf = vec![0u8; len];
            io.read_exact(&mut buf).await?;

            // Deserialize
            bincode::deserialize(&buf)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        })
    }

    fn write_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 Self::Protocol,
        io: &'life2 mut T,
        req: Self::Request,
    ) -> std::pin::Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'async_trait>>
    where
        T: AsyncWrite + Unpin + Send + 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            // Serialize
            let data = bincode::serialize(&req)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            // Write length prefix
            let len = data.len() as u32;
            io.write_all(&len.to_be_bytes()).await?;

            // Write body
            io.write_all(&data).await?;
            io.flush().await?;

            Ok(())
        })
    }

    fn write_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _protocol: &'life1 Self::Protocol,
        io: &'life2 mut T,
        res: Self::Response,
    ) -> std::pin::Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'async_trait>>
    where
        T: AsyncWrite + Unpin + Send + 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            // Serialize
            let data = bincode::serialize(&res)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            // Write length prefix
            let len = data.len() as u32;
            io.write_all(&len.to_be_bytes()).await?;

            // Write body
            io.write_all(&data).await?;
            io.flush().await?;

            Ok(())
        })
    }
}

// =============================================================================
// BEHAVIOUR CONFIGURATION
// =============================================================================

/// Create request-response behaviour configuration
pub fn create_request_response_config() -> request_response::Config {
    request_response::Config::default()
        .with_request_timeout(REQUEST_TIMEOUT)
}

/// Get supported protocols
pub fn get_protocols() -> Vec<(StreamProtocol, ProtocolSupport)> {
    vec![
        (StreamProtocol::new(BLOCK_PROTOCOL), ProtocolSupport::Full),
        (StreamProtocol::new(SYNC_PROTOCOL), ProtocolSupport::Full),
        (StreamProtocol::new(STATUS_PROTOCOL), ProtocolSupport::Full),
        (StreamProtocol::new(GENESIS_PROTOCOL), ProtocolSupport::Full),
    ]
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

impl BlockRequest {
    /// Create a request for block by hash
    pub fn by_hash(hash: Hash) -> KratosRequest {
        KratosRequest::Block(BlockRequest::ByHash(hash))
    }

    /// Create a request for block by number
    pub fn by_number(number: BlockNumber) -> KratosRequest {
        KratosRequest::Block(BlockRequest::ByNumber(number))
    }
}

impl SyncRequest {
    /// Create a sync request
    pub fn new(from_block: BlockNumber, max_blocks: u32) -> KratosRequest {
        KratosRequest::Sync(SyncRequest {
            from_block,
            max_blocks,
            include_bodies: true,
        })
    }

    /// Create a header-only sync request
    pub fn headers_only(from_block: BlockNumber, max_blocks: u32) -> KratosRequest {
        KratosRequest::Sync(SyncRequest {
            from_block,
            max_blocks,
            include_bodies: false,
        })
    }
}

impl StatusRequest {
    /// Create a status request
    pub fn new(best_block: BlockNumber, best_hash: Hash, genesis_hash: Hash) -> KratosRequest {
        KratosRequest::Status(StatusRequest {
            best_block,
            best_hash,
            genesis_hash,
            protocol_version: 1,
        })
    }
}

impl BlockResponse {
    /// Create a success response with block
    pub fn found(block: Block) -> KratosResponse {
        KratosResponse::Block(BlockResponse::Block(block))
    }

    /// Create a not found response
    pub fn not_found() -> KratosResponse {
        KratosResponse::Block(BlockResponse::NotFound)
    }
}

impl SyncResponse {
    /// Create a sync response
    pub fn new(blocks: Vec<Block>, has_more: bool, best_height: BlockNumber) -> KratosResponse {
        KratosResponse::Sync(SyncResponse {
            blocks,
            has_more,
            best_height,
        })
    }
}

impl StatusResponse {
    /// Create a status response
    pub fn new(
        best_block: BlockNumber,
        best_hash: Hash,
        genesis_hash: Hash,
        peer_count: u32,
    ) -> KratosResponse {
        KratosResponse::Status(StatusResponse {
            best_block,
            best_hash,
            genesis_hash,
            protocol_version: 1,
            peer_count,
        })
    }
}

impl GenesisRequest {
    /// Create a genesis request
    pub fn new() -> KratosRequest {
        KratosRequest::Genesis(GenesisRequest {
            protocol_version: 1,
        })
    }
}

impl Default for GenesisRequest {
    fn default() -> Self {
        Self { protocol_version: 1 }
    }
}

impl GenesisResponse {
    /// Create a genesis response
    pub fn new(genesis_hash: Hash, genesis_block: Block, chain_name: String) -> KratosResponse {
        KratosResponse::Genesis(GenesisResponse {
            genesis_hash,
            genesis_block,
            chain_name,
            protocol_version: 1,
            genesis_validators: Vec::new(),
            genesis_balances: Vec::new(),
        })
    }

    /// Create a genesis response with validator info (for joining nodes)
    pub fn with_validators(
        genesis_hash: Hash,
        genesis_block: Block,
        chain_name: String,
        validators: Vec<GenesisValidatorInfo>,
        balances: Vec<(AccountId, Balance)>,
    ) -> KratosResponse {
        KratosResponse::Genesis(GenesisResponse {
            genesis_hash,
            genesis_block,
            chain_name,
            protocol_version: 1,
            genesis_validators: validators,
            genesis_balances: balances,
        })
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn test_block_request_serialization() {
        let hash = Hash::hash(b"test");
        let request = BlockRequest::by_hash(hash);

        let serialized = bincode::serialize(&request).unwrap();
        let deserialized: KratosRequest = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            KratosRequest::Block(BlockRequest::ByHash(h)) => assert_eq!(h, hash),
            _ => panic!("Wrong request type"),
        }
    }

    #[test]
    fn test_sync_request_serialization() {
        let request = SyncRequest::new(100, 50);

        let serialized = bincode::serialize(&request).unwrap();
        let deserialized: KratosRequest = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            KratosRequest::Sync(req) => {
                assert_eq!(req.from_block, 100);
                assert_eq!(req.max_blocks, 50);
                assert!(req.include_bodies);
            }
            _ => panic!("Wrong request type"),
        }
    }

    #[test]
    fn test_status_request_serialization() {
        let request = StatusRequest::new(100, Hash::ZERO, Hash::ZERO);

        let serialized = bincode::serialize(&request).unwrap();
        let deserialized: KratosRequest = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            KratosRequest::Status(req) => {
                assert_eq!(req.best_block, 100);
                assert_eq!(req.protocol_version, 1);
            }
            _ => panic!("Wrong request type"),
        }
    }

    #[test]
    fn test_block_response_serialization() {
        let response = BlockResponse::not_found();

        let serialized = bincode::serialize(&response).unwrap();
        let deserialized: KratosResponse = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            KratosResponse::Block(BlockResponse::NotFound) => {}
            _ => panic!("Wrong response type"),
        }
    }

    #[test]
    fn test_sync_response_serialization() {
        let response = SyncResponse::new(vec![], false, 100);

        let serialized = bincode::serialize(&response).unwrap();
        let deserialized: KratosResponse = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            KratosResponse::Sync(res) => {
                assert!(res.blocks.is_empty());
                assert!(!res.has_more);
                assert_eq!(res.best_height, 100);
            }
            _ => panic!("Wrong response type"),
        }
    }
}
