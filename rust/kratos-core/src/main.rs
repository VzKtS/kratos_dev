// KratOs Node - Entry point
// Principle: Protocol for coexistence, not an application

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

mod cli;
mod consensus;
mod contracts;
mod execution;
mod genesis;
mod network;
mod node;
mod rpc;
mod storage;
mod types;

#[cfg(test)]
mod tests;

use clap::Parser;
use cli::{Cli, Commands, KeySubcommand};
use cli::config::NodeConfig;
use cli::runner::run_node;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Initialize logging based on verbosity
    let log_filter = if cli.verbose {
        "debug"
    } else {
        &cli.log_level
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(log_filter)),
        )
        .init();

    // Print banner
    print_banner();

    // Execute command
    match cli.command {
        Commands::Run(cmd) => {
            // Build node configuration from CLI args
            let config = NodeConfig::from_run_cmd(&cmd).map_err(|e| {
                error!("Configuration error: {}", e);
                anyhow::anyhow!("Configuration error: {}", e)
            })?;

            // Run the node
            if let Err(e) = run_node(config).await {
                error!("Node error: {}", e);
                return Err(anyhow::anyhow!("Node error: {}", e));
            }
        }

        Commands::Info(cmd) => {
            info!("Querying node at {}", cmd.rpc);
            // TODO: Implement RPC client to query node info
            println!("Node info query not yet implemented");
            println!("Try: curl -X POST {} -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"method\":\"system_info\",\"params\":[],\"id\":1}}'", cmd.rpc);
        }

        Commands::Key(cmd) => {
            match cmd.subcommand {
                KeySubcommand::Generate { scheme, output, format } => {
                    generate_key(&scheme, output.as_ref(), &format)?;
                }
                KeySubcommand::Inspect { key, scheme } => {
                    inspect_key(&key, &scheme)?;
                }
                KeySubcommand::Insert { base_path, key_type, scheme, suri } => {
                    info!("Inserting key of type {} into keystore", key_type);
                    // TODO: Implement keystore
                    warn!("Keystore not yet implemented");
                }
                KeySubcommand::List { base_path } => {
                    info!("Listing keys in keystore");
                    // TODO: Implement keystore listing
                    warn!("Keystore not yet implemented");
                }
            }
        }

        Commands::Export(cmd) => {
            info!("Exporting chain data to {}", cmd.output.display());
            // TODO: Implement chain export
            warn!("Chain export not yet implemented");
        }

        Commands::Purge(cmd) => {
            let path = cmd.get_base_path();

            if !cmd.yes {
                println!("This will delete all chain data at: {}", path.display());
                println!("Are you sure? [y/N]");

                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;

                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            if path.exists() {
                std::fs::remove_dir_all(&path)?;
                info!("Purged chain data at: {}", path.display());
            } else {
                info!("No data to purge at: {}", path.display());
            }
        }
    }

    info!("Goodbye!");
    Ok(())
}

/// Print the KratOs banner
fn print_banner() {
    println!(r#"
    ╔═══════════════════════════════════════════════════════════╗
    ║                                                           ║
    ║   ██╗  ██╗██████╗  █████╗ ████████╗ ██████╗ ███████╗     ║
    ║   ██║ ██╔╝██╔══██╗██╔══██╗╚══██╔══╝██╔═══██╗██╔════╝     ║
    ║   █████╔╝ ██████╔╝███████║   ██║   ██║   ██║███████╗     ║
    ║   ██╔═██╗ ██╔══██╗██╔══██║   ██║   ██║   ██║╚════██║     ║
    ║   ██║  ██╗██║  ██║██║  ██║   ██║   ╚██████╔╝███████║     ║
    ║   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝    ╚═════╝ ╚══════╝     ║
    ║                                                           ║
    ║              Minimal • Auditable • Durable                ║
    ║                 Protocol for Coexistence                  ║
    ║                                                           ║
    ╚═══════════════════════════════════════════════════════════╝
    "#);
    println!("    Version: {}", env!("CARGO_PKG_VERSION"));
    println!();
}

/// Generate a new keypair
fn generate_key(
    scheme: &str,
    output: Option<&std::path::PathBuf>,
    format: &str,
) -> anyhow::Result<()> {
    use ed25519_dalek::{SigningKey, VerifyingKey};
    use rand::rngs::OsRng;

    info!("Generating {} keypair", scheme);

    match scheme {
        "ed25519" => {
            let signing_key = SigningKey::generate(&mut OsRng);
            let verifying_key: VerifyingKey = (&signing_key).into();

            let secret_hex = hex::encode(signing_key.to_bytes());
            let public_hex = hex::encode(verifying_key.to_bytes());
            let account_id = format!("0x{}", public_hex);

            match format {
                "json" => {
                    let json = serde_json::json!({
                        "scheme": scheme,
                        "secretKey": format!("0x{}", secret_hex),
                        "publicKey": format!("0x{}", public_hex),
                        "accountId": account_id,
                    });

                    let output_str = serde_json::to_string_pretty(&json)?;

                    if let Some(path) = output {
                        std::fs::write(path, &output_str)?;
                        info!("Key saved to: {}", path.display());
                    } else {
                        println!("{}", output_str);
                    }
                }
                "hex" => {
                    println!("Secret Key: 0x{}", secret_hex);
                    println!("Public Key: 0x{}", public_hex);
                    println!("Account ID: {}", account_id);
                }
                _ => {
                    return Err(anyhow::anyhow!("Unknown format: {}", format));
                }
            }
        }
        "sr25519" => {
            // Use schnorrkel for sr25519
            use schnorrkel::{Keypair, MiniSecretKey};

            let mini_secret = MiniSecretKey::generate();
            let keypair: Keypair = mini_secret.expand_to_keypair(schnorrkel::ExpansionMode::Ed25519);

            let secret_hex = hex::encode(mini_secret.as_bytes());
            let public_hex = hex::encode(keypair.public.to_bytes());
            let account_id = format!("0x{}", public_hex);

            match format {
                "json" => {
                    let json = serde_json::json!({
                        "scheme": scheme,
                        "secretSeed": format!("0x{}", secret_hex),
                        "publicKey": format!("0x{}", public_hex),
                        "accountId": account_id,
                    });

                    let output_str = serde_json::to_string_pretty(&json)?;

                    if let Some(path) = output {
                        std::fs::write(path, &output_str)?;
                        info!("Key saved to: {}", path.display());
                    } else {
                        println!("{}", output_str);
                    }
                }
                "hex" => {
                    println!("Secret Seed: 0x{}", secret_hex);
                    println!("Public Key: 0x{}", public_hex);
                    println!("Account ID: {}", account_id);
                }
                _ => {
                    return Err(anyhow::anyhow!("Unknown format: {}", format));
                }
            }
        }
        _ => {
            return Err(anyhow::anyhow!("Unknown key scheme: {}", scheme));
        }
    }

    Ok(())
}

/// Inspect a key
fn inspect_key(key: &str, scheme: &str) -> anyhow::Result<()> {
    use ed25519_dalek::{SigningKey, VerifyingKey};

    info!("Inspecting {} key", scheme);

    // Remove 0x prefix if present
    let key_hex = key.strip_prefix("0x").unwrap_or(key);

    match scheme {
        "ed25519" => {
            let key_bytes = hex::decode(key_hex)?;

            if key_bytes.len() == 32 {
                // Could be secret or public key
                // Try as secret key first
                if let Ok(secret_bytes) = key_bytes.clone().try_into() {
                    let signing_key = SigningKey::from_bytes(&secret_bytes);
                    let verifying_key: VerifyingKey = (&signing_key).into();

                    println!("Type: Secret Key");
                    println!("Public Key: 0x{}", hex::encode(verifying_key.to_bytes()));
                    println!("Account ID: 0x{}", hex::encode(verifying_key.to_bytes()));
                } else {
                    println!("Type: Public Key");
                    println!("Account ID: 0x{}", key_hex);
                }
            } else if key_bytes.len() == 64 {
                // Full keypair
                println!("Type: Full Keypair (64 bytes)");
                println!("Secret: 0x{}", hex::encode(&key_bytes[..32]));
                println!("Public: 0x{}", hex::encode(&key_bytes[32..]));
            } else {
                return Err(anyhow::anyhow!(
                    "Invalid key length: {} bytes (expected 32 or 64)",
                    key_bytes.len()
                ));
            }
        }
        "sr25519" => {
            let key_bytes = hex::decode(key_hex)?;

            if key_bytes.len() == 32 {
                // Mini secret key
                use schnorrkel::{Keypair, MiniSecretKey};

                let mini_bytes: [u8; 32] = key_bytes.try_into()
                    .map_err(|_| anyhow::anyhow!("Invalid key length"))?;
                let mini_secret = MiniSecretKey::from_bytes(&mini_bytes)
                    .map_err(|e| anyhow::anyhow!("Invalid sr25519 key: {:?}", e))?;
                let keypair: Keypair =
                    mini_secret.expand_to_keypair(schnorrkel::ExpansionMode::Ed25519);

                println!("Type: Secret Seed");
                println!("Public Key: 0x{}", hex::encode(keypair.public.to_bytes()));
                println!(
                    "Account ID: 0x{}",
                    hex::encode(keypair.public.to_bytes())
                );
            } else {
                return Err(anyhow::anyhow!(
                    "Invalid key length: {} bytes (expected 32)",
                    key_bytes.len()
                ));
            }
        }
        _ => {
            return Err(anyhow::anyhow!("Unknown key scheme: {}", scheme));
        }
    }

    Ok(())
}
