// CLI Configuration - Convert CLI args to node config
// Principle: Clear mapping between user input and internal configuration

use crate::genesis::{ChainConfig, GenesisSpec};
use crate::cli::RunCmd;
use crate::rpc::RpcConfig;
use crate::types::AccountId;
use std::path::PathBuf;
use tracing::info;

/// Complete node configuration derived from CLI arguments
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// Genesis mode - creates a new network (generates genesis block)
    /// When false, node joins existing network via DNS Seeds / bootnodes
    pub genesis_mode: bool,
    /// Chain configuration
    pub chain: ChainConfig,
    /// Genesis specification
    pub genesis: GenesisSpec,
    /// Base data path
    pub base_path: PathBuf,
    /// RPC configuration
    pub rpc: RpcConfig,
    /// Node name
    pub name: String,
    /// Enable validator mode
    pub validator: bool,
    /// Validator key path
    pub validator_key: Option<PathBuf>,
    /// Sync mode
    pub sync_mode: SyncMode,
    /// Pruning mode
    pub pruning: PruningMode,
    /// Database cache size in MB
    pub db_cache_mb: u32,
    /// Enable GRANDPA finality debug traces
    pub debug_grandpa: bool,
}

/// Sync modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Full sync - download and verify all blocks
    Full,
    /// Light client - only headers and proofs
    Light,
    /// Warp sync - download finalized state then sync recent blocks
    Warp,
}

/// Pruning modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruningMode {
    /// Keep all historical state
    Archive,
    /// Keep last N blocks of state
    Blocks(u32),
}

impl NodeConfig {
    /// Create configuration from CLI run command
    pub fn from_run_cmd(cmd: &RunCmd) -> Result<Self, ConfigError> {
        // Try to load validator account from key file if specified
        let validator_account = if cmd.validator {
            if let Some(ref key_path) = cmd.validator_key {
                Self::load_validator_account_from_key(key_path)?
            } else {
                None
            }
        } else {
            None
        };

        // Determine chain config and genesis
        let (chain, genesis) = match cmd.chain.as_str() {
            "kratos" => {
                let chain = ChainConfig::mainnet();
                let genesis = if let Some(validator_id) = validator_account {
                    info!("Using custom validator in genesis: 0x{}", hex::encode(validator_id.as_bytes()));
                    GenesisSpec::with_validator(validator_id)
                } else {
                    GenesisSpec::default()
                };
                (chain, genesis)
            }
            path => {
                // Try to load from file
                Self::load_chain_spec(path)?
            }
        };

        // Override network config with CLI args
        let mut chain = chain;
        chain.network.listen_port = cmd.port;
        chain.network.bootnodes = cmd.bootnodes.clone();

        // Parse sync mode
        let sync_mode = match cmd.sync.as_str() {
            "full" => SyncMode::Full,
            "light" => SyncMode::Light,
            "warp" => SyncMode::Warp,
            _ => {
                return Err(ConfigError::InvalidSyncMode(cmd.sync.clone()));
            }
        };

        // Parse pruning mode
        let pruning = match cmd.pruning.as_str() {
            "archive" => PruningMode::Archive,
            n => {
                let blocks = n
                    .parse::<u32>()
                    .map_err(|_| ConfigError::InvalidPruningMode(cmd.pruning.clone()))?;
                PruningMode::Blocks(blocks)
            }
        };

        // RPC configuration
        let rpc_addr: [u8; 4] = match cmd.rpc_addr.as_str() {
            "127.0.0.1" | "localhost" => [127, 0, 0, 1],
            "0.0.0.0" => [0, 0, 0, 0],
            addr => Self::parse_ip_addr(addr)?,
        };

        let rpc = RpcConfig {
            enabled: cmd.rpc,
            port: cmd.rpc_port,
            address: rpc_addr,
            cors: cmd.rpc_cors_all,
            cors_origins: vec![], // SECURITY FIX #3: Empty = localhost only
            max_request_size: 10 * 1024 * 1024, // 10 MB
            rate_limit: Some(100),
        };

        // Generate node name
        // SECURITY NOTE #18: This uses non-cryptographic randomness intentionally
        // Node names are cosmetic/identifiers only and don't require CSPRNG
        // Cryptographic operations (key generation, VRF) use OsRng elsewhere
        let name = cmd.name.clone().unwrap_or_else(|| {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            let adjectives = ["Swift", "Brave", "Noble", "Wise", "Bold"];
            let nouns = ["Phoenix", "Dragon", "Eagle", "Lion", "Wolf"];
            let adj = adjectives[rng.gen_range(0..adjectives.len())];
            let noun = nouns[rng.gen_range(0..nouns.len())];
            let id: u16 = rng.gen();
            format!("{}-{}-{}", adj, noun, id)
        });

        Ok(Self {
            genesis_mode: cmd.genesis,
            chain,
            genesis,
            base_path: cmd.get_base_path(),
            rpc,
            name,
            validator: cmd.validator,
            validator_key: cmd.validator_key.clone(),
            sync_mode,
            pruning,
            db_cache_mb: cmd.db_cache,
            debug_grandpa: cmd.debug_grandpa,
        })
    }

    /// Load validator account ID from key file
    fn load_validator_account_from_key(key_path: &PathBuf) -> Result<Option<AccountId>, ConfigError> {
        use ed25519_dalek::SigningKey;

        let content = std::fs::read_to_string(key_path)
            .map_err(|e| ConfigError::KeyLoadError(format!("Failed to read key file: {}", e)))?;

        // Try to parse as JSON
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            // Look for secretKey field
            if let Some(secret_hex) = json.get("secretKey").and_then(|v| v.as_str()) {
                let hex_str = secret_hex.strip_prefix("0x").unwrap_or(secret_hex);
                let key_bytes = hex::decode(hex_str)
                    .map_err(|e| ConfigError::KeyLoadError(format!("Invalid hex: {}", e)))?;

                if key_bytes.len() != 32 {
                    return Err(ConfigError::KeyLoadError(format!(
                        "Invalid key length: {} bytes",
                        key_bytes.len()
                    )));
                }

                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&key_bytes);
                let signing_key = SigningKey::from_bytes(&bytes);
                let account_id = AccountId::from_bytes(signing_key.verifying_key().to_bytes());

                return Ok(Some(account_id));
            }

            // Try publicKey field if secretKey not available
            if let Some(public_hex) = json.get("publicKey").and_then(|v| v.as_str()) {
                let hex_str = public_hex.strip_prefix("0x").unwrap_or(public_hex);
                let key_bytes = hex::decode(hex_str)
                    .map_err(|e| ConfigError::KeyLoadError(format!("Invalid hex: {}", e)))?;

                if key_bytes.len() != 32 {
                    return Err(ConfigError::KeyLoadError(format!(
                        "Invalid key length: {} bytes",
                        key_bytes.len()
                    )));
                }

                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&key_bytes);
                let account_id = AccountId::from_bytes(bytes);

                return Ok(Some(account_id));
            }

            return Err(ConfigError::KeyLoadError(
                "Key file missing 'secretKey' or 'publicKey' field".to_string(),
            ));
        }

        // Try to parse as raw hex
        let hex_str = content.trim().strip_prefix("0x").unwrap_or(content.trim());
        let key_bytes = hex::decode(hex_str)
            .map_err(|e| ConfigError::KeyLoadError(format!("Invalid hex: {}", e)))?;

        if key_bytes.len() != 32 {
            return Err(ConfigError::KeyLoadError(format!(
                "Invalid key length: {} bytes",
                key_bytes.len()
            )));
        }

        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&key_bytes);
        let signing_key = SigningKey::from_bytes(&bytes);
        let account_id = AccountId::from_bytes(signing_key.verifying_key().to_bytes());

        Ok(Some(account_id))
    }

    /// Load chain spec from file
    fn load_chain_spec(path: &str) -> Result<(ChainConfig, GenesisSpec), ConfigError> {
        use std::fs;
        use std::path::Path;

        let spec_path = Path::new(path);
        if !spec_path.exists() {
            return Err(ConfigError::ChainSpecNotFound(path.to_string()));
        }

        let content = fs::read_to_string(spec_path)
            .map_err(|e| ConfigError::ChainSpecReadError(e.to_string()))?;

        // Parse JSON chain spec
        let _spec: ChainSpecJson = serde_json::from_str(&content)
            .map_err(|e| ConfigError::ChainSpecParseError(e.to_string()))?;

        // Use mainnet config as base
        let chain = ChainConfig::mainnet();
        let genesis = GenesisSpec::default();

        Ok((chain, genesis))
    }

    /// Parse IP address string to bytes
    fn parse_ip_addr(addr: &str) -> Result<[u8; 4], ConfigError> {
        let parts: Vec<&str> = addr.split('.').collect();
        if parts.len() != 4 {
            return Err(ConfigError::InvalidIpAddress(addr.to_string()));
        }

        let mut bytes = [0u8; 4];
        for (i, part) in parts.iter().enumerate() {
            bytes[i] = part
                .parse()
                .map_err(|_| ConfigError::InvalidIpAddress(addr.to_string()))?;
        }

        Ok(bytes)
    }
}

/// Chain specification JSON format
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct ChainSpecJson {
    name: String,
    id: String,
    #[serde(default)]
    network: ChainSpecNetwork,
}

#[derive(Debug, Default, serde::Deserialize)]
#[allow(dead_code)]
struct ChainSpecNetwork {
    listen_port: Option<u16>,
    #[serde(default)]
    bootnodes: Vec<String>,
    max_peers: Option<u32>,
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Chain spec not found: {0}")]
    ChainSpecNotFound(String),

    #[error("Failed to read chain spec: {0}")]
    ChainSpecReadError(String),

    #[error("Failed to parse chain spec: {0}")]
    ChainSpecParseError(String),

    #[error("Invalid sync mode: {0}")]
    InvalidSyncMode(String),

    #[error("Invalid pruning mode: {0}")]
    InvalidPruningMode(String),

    #[error("Invalid IP address: {0}")]
    InvalidIpAddress(String),

    #[error("Key load error: {0}")]
    KeyLoadError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_config_from_kratos() {
        let cmd = RunCmd {
            genesis: false,
            base_path: None,
            chain: "kratos".to_string(),
            name: Some("test-node".to_string()),
            port: 30333,
            rpc_port: 9933,
            rpc: true,
            rpc_addr: "127.0.0.1".to_string(),
            bootnodes: vec![],
            max_peers: 50,
            validator: false,
            validator_key: None,
            sync: "full".to_string(),
            pruning: "256".to_string(),
            db_cache: 128,
            prometheus_port: 0,
            public_addr: None,
            rpc_methods_unsafe: false,
            rpc_cors_all: false,
            debug_grandpa: false,
        };

        let config = NodeConfig::from_run_cmd(&cmd).unwrap();
        assert_eq!(config.name, "test-node");
        assert_eq!(config.rpc.port, 9933);
        assert_eq!(config.sync_mode, SyncMode::Full);
        assert_eq!(config.pruning, PruningMode::Blocks(256));
        assert!(!config.genesis_mode);
    }

    #[test]
    fn test_parse_ip_addr() {
        let addr = NodeConfig::parse_ip_addr("192.168.1.1").unwrap();
        assert_eq!(addr, [192, 168, 1, 1]);

        let addr = NodeConfig::parse_ip_addr("0.0.0.0").unwrap();
        assert_eq!(addr, [0, 0, 0, 0]);
    }

    #[test]
    fn test_sync_mode_parsing() {
        let cmd = RunCmd {
            genesis: false,
            base_path: None,
            chain: "kratos".to_string(),
            name: None,
            port: 30333,
            rpc_port: 9933,
            rpc: true,
            rpc_addr: "127.0.0.1".to_string(),
            bootnodes: vec![],
            max_peers: 50,
            validator: false,
            validator_key: None,
            sync: "warp".to_string(),
            pruning: "archive".to_string(),
            db_cache: 128,
            prometheus_port: 0,
            public_addr: None,
            rpc_methods_unsafe: false,
            rpc_cors_all: false,
            debug_grandpa: false,
        };

        let config = NodeConfig::from_run_cmd(&cmd).unwrap();
        assert_eq!(config.sync_mode, SyncMode::Warp);
        assert_eq!(config.pruning, PruningMode::Archive);
    }

    #[test]
    fn test_invalid_sync_mode() {
        let cmd = RunCmd {
            genesis: false,
            base_path: None,
            chain: "kratos".to_string(),
            name: None,
            port: 30333,
            rpc_port: 9933,
            rpc: true,
            rpc_addr: "127.0.0.1".to_string(),
            bootnodes: vec![],
            max_peers: 50,
            validator: false,
            validator_key: None,
            sync: "invalid".to_string(),
            pruning: "256".to_string(),
            db_cache: 128,
            prometheus_port: 0,
            public_addr: None,
            rpc_methods_unsafe: false,
            rpc_cors_all: false,
            debug_grandpa: false,
        };

        let result = NodeConfig::from_run_cmd(&cmd);
        assert!(result.is_err());
    }

    #[test]
    fn test_genesis_mode() {
        let cmd = RunCmd {
            genesis: true,
            base_path: None,
            chain: "kratos".to_string(),
            name: Some("genesis-node".to_string()),
            port: 30333,
            rpc_port: 9933,
            rpc: true,
            rpc_addr: "127.0.0.1".to_string(),
            bootnodes: vec![],
            max_peers: 50,
            validator: true,
            validator_key: None,
            sync: "full".to_string(),
            pruning: "256".to_string(),
            db_cache: 128,
            prometheus_port: 0,
            public_addr: None,
            rpc_methods_unsafe: false,
            rpc_cors_all: false,
            debug_grandpa: false,
        };

        let config = NodeConfig::from_run_cmd(&cmd).unwrap();
        assert!(config.genesis_mode);
        assert!(config.validator);
    }
}
