// CLI - Command Line Interface for KratOs Node
// Principle: Simple, clear, composable commands

pub mod config;
pub mod runner;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// KratOs Node - Minimal, Auditable, Durable blockchain protocol
#[derive(Parser, Debug)]
#[command(name = "kratos-node")]
#[command(author = "KratOs Contributors")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "KratOs blockchain node - A protocol for coexistence")]
#[command(long_about = r#"
KratOs is a minimal, auditable, and durable blockchain protocol.

Core Principles:
  - Power is slow: Governance changes require time
  - Failures are local: One chain's failure doesn't affect others
  - Exit is always possible: Capital is never permanently frozen

Start a new network (genesis node):
  kratos-node run --genesis --validator

Join an existing network (auto-discovers peers via DNS Seeds):
  kratos-node run

Join with explicit bootnode:
  kratos-node run --bootnode /ip4/1.2.3.4/tcp/30333/p2p/...
"#)]
pub struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true, default_value = "false")]
    pub verbose: bool,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, global = true, default_value = "info", env = "KRATOS_LOG")]
    pub log_level: String,
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run the node
    Run(RunCmd),

    /// Show node information
    Info(InfoCmd),

    /// Key management
    Key(KeyCmd),

    /// Export/import chain data
    Export(ExportCmd),

    /// Purge chain data
    Purge(PurgeCmd),
}

/// Run the node
#[derive(Parser, Debug)]
pub struct RunCmd {
    /// Genesis mode - create a new network (generates genesis block)
    /// Without this flag, the node joins an existing network via DNS Seeds
    #[arg(long)]
    pub genesis: bool,

    /// Base path for chain data
    #[arg(short = 'd', long, env = "KRATOS_BASE_PATH")]
    pub base_path: Option<PathBuf>,

    /// Chain specification (kratos or path to spec file)
    #[arg(long, default_value = "kratos")]
    pub chain: String,

    /// Node name for telemetry
    #[arg(long, env = "KRATOS_NAME")]
    pub name: Option<String>,

    /// P2P listen port
    #[arg(long, default_value = "30333", env = "KRATOS_P2P_PORT")]
    pub port: u16,

    /// RPC listen port
    #[arg(long, default_value = "9933", env = "KRATOS_RPC_PORT")]
    pub rpc_port: u16,

    /// Enable RPC server
    #[arg(long, default_value = "true")]
    pub rpc: bool,

    /// RPC listen address (use 0.0.0.0 for public)
    #[arg(long, default_value = "127.0.0.1")]
    pub rpc_addr: String,

    /// Bootstrap nodes (can be specified multiple times)
    #[arg(long = "bootnode", value_name = "MULTIADDR")]
    pub bootnodes: Vec<String>,

    /// Maximum number of peers
    #[arg(long, default_value = "50")]
    pub max_peers: u32,

    /// Enable block production (validator mode)
    #[arg(long)]
    pub validator: bool,

    /// Validator key file (required for --validator)
    #[arg(long, requires = "validator")]
    pub validator_key: Option<PathBuf>,

    /// Sync mode (full, light, warp)
    #[arg(long, default_value = "full")]
    pub sync: String,

    /// Pruning mode (archive, 256, 1000, etc.)
    #[arg(long, default_value = "256")]
    pub pruning: String,

    /// Database cache size in MB
    #[arg(long, default_value = "128")]
    pub db_cache: u32,

    /// Prometheus metrics port (0 to disable)
    #[arg(long, default_value = "0")]
    pub prometheus_port: u16,

    /// External address for P2P (useful behind NAT)
    #[arg(long)]
    pub public_addr: Option<String>,

    /// Force unsafe RPC methods
    #[arg(long)]
    pub rpc_methods_unsafe: bool,

    /// Allow connections from any origin (CORS)
    #[arg(long)]
    pub rpc_cors_all: bool,
}

/// Show node info
#[derive(Parser, Debug)]
pub struct InfoCmd {
    /// RPC endpoint to query
    #[arg(long, default_value = "http://127.0.0.1:9933")]
    pub rpc: String,

    /// Output format (text, json)
    #[arg(long, default_value = "text")]
    pub format: String,
}

/// Key management commands
#[derive(Parser, Debug)]
pub struct KeyCmd {
    #[command(subcommand)]
    pub subcommand: KeySubcommand,
}

#[derive(Subcommand, Debug)]
pub enum KeySubcommand {
    /// Generate a new keypair
    Generate {
        /// Key type (ed25519, sr25519)
        #[arg(long, default_value = "ed25519")]
        scheme: String,

        /// Output file (stdout if not specified)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Output format (hex, json)
        #[arg(long, default_value = "json")]
        format: String,
    },

    /// Inspect a key file or seed
    Inspect {
        /// Key file or seed phrase
        key: String,

        /// Key type (ed25519, sr25519)
        #[arg(long, default_value = "ed25519")]
        scheme: String,
    },

    /// Insert a key into the keystore
    Insert {
        /// Base path for chain data
        #[arg(short = 'd', long)]
        base_path: Option<PathBuf>,

        /// Key type (aura, babe, grandpa)
        #[arg(long)]
        key_type: String,

        /// Key scheme (ed25519, sr25519)
        #[arg(long, default_value = "ed25519")]
        scheme: String,

        /// Seed or key file
        #[arg(long)]
        suri: String,
    },

    /// List keys in keystore
    List {
        /// Base path for chain data
        #[arg(short = 'd', long)]
        base_path: Option<PathBuf>,
    },
}

/// Export chain data
#[derive(Parser, Debug)]
pub struct ExportCmd {
    /// Base path for chain data
    #[arg(short = 'd', long)]
    pub base_path: Option<PathBuf>,

    /// Output file
    #[arg(short, long)]
    pub output: PathBuf,

    /// Start block number
    #[arg(long, default_value = "0")]
    pub from: u64,

    /// End block number (latest if not specified)
    #[arg(long)]
    pub to: Option<u64>,

    /// Export format (binary, json)
    #[arg(long, default_value = "binary")]
    pub format: String,
}

/// Purge chain data
#[derive(Parser, Debug)]
pub struct PurgeCmd {
    /// Base path for chain data
    #[arg(short = 'd', long)]
    pub base_path: Option<PathBuf>,

    /// Chain to purge (chain name)
    #[arg(long, default_value = "kratos")]
    pub chain: String,

    /// Skip confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,
}

impl RunCmd {
    /// Get the base path, defaulting to platform-specific data directory
    pub fn get_base_path(&self) -> PathBuf {
        if let Some(ref path) = self.base_path {
            path.clone()
        } else {
            let data_dir = dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("kratos");

            data_dir.join("chains").join(&self.chain)
        }
    }
}

impl PurgeCmd {
    /// Get the base path for the chain to purge
    pub fn get_base_path(&self) -> PathBuf {
        if let Some(ref path) = self.base_path {
            path.clone()
        } else {
            let data_dir = dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("kratos");

            data_dir.join("chains").join(&self.chain)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_run() {
        let cli = Cli::try_parse_from(["kratos-node", "run"]).unwrap();
        match cli.command {
            Commands::Run(cmd) => {
                assert_eq!(cmd.chain, "kratos");
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_parse_run_with_port() {
        let cli = Cli::try_parse_from(["kratos-node", "run", "--port", "30334", "--rpc-port", "9934"]).unwrap();
        match cli.command {
            Commands::Run(cmd) => {
                assert_eq!(cmd.port, 30334);
                assert_eq!(cmd.rpc_port, 9934);
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_parse_run_with_bootnodes() {
        let cli = Cli::try_parse_from([
            "kratos-node",
            "run",
            "--bootnode", "/ip4/1.2.3.4/tcp/30333/p2p/12D3KooWPeer1",
            "--bootnode", "/ip4/5.6.7.8/tcp/30333/p2p/12D3KooWPeer2",
        ]).unwrap();
        match cli.command {
            Commands::Run(cmd) => {
                assert_eq!(cmd.bootnodes.len(), 2);
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_parse_key_generate() {
        let cli = Cli::try_parse_from(["kratos-node", "key", "generate"]).unwrap();
        match cli.command {
            Commands::Key(cmd) => {
                match cmd.subcommand {
                    KeySubcommand::Generate { scheme, .. } => {
                        assert_eq!(scheme, "ed25519");
                    }
                    _ => panic!("Expected Generate subcommand"),
                }
            }
            _ => panic!("Expected Key command"),
        }
    }

    #[test]
    fn test_cli_parse_purge() {
        let cli = Cli::try_parse_from(["kratos-node", "purge", "--chain", "kratos", "-y"]).unwrap();
        match cli.command {
            Commands::Purge(cmd) => {
                assert_eq!(cmd.chain, "kratos");
                assert!(cmd.yes);
            }
            _ => panic!("Expected Purge command"),
        }
    }

    #[test]
    fn test_run_cmd_base_path() {
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
            sync: "full".to_string(),
            pruning: "256".to_string(),
            db_cache: 128,
            prometheus_port: 0,
            public_addr: None,
            rpc_methods_unsafe: false,
            rpc_cors_all: false,
        };

        let path = cmd.get_base_path();
        assert!(path.to_string_lossy().contains("kratos"));
    }

    #[test]
    fn test_cli_parse_genesis_mode() {
        let cli = Cli::try_parse_from(["kratos-node", "run", "--genesis", "--validator"]).unwrap();
        match cli.command {
            Commands::Run(cmd) => {
                assert!(cmd.genesis);
                assert!(cmd.validator);
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_parse_normal_mode() {
        let cli = Cli::try_parse_from(["kratos-node", "run"]).unwrap();
        match cli.command {
            Commands::Run(cmd) => {
                assert!(!cmd.genesis);
            }
            _ => panic!("Expected Run command"),
        }
    }
}
