# KratOs Node Usage Guide

## Overview

KratOs is a **custom Rust blockchain implementation** with its own consensus, networking, and storage layers. The node is built from scratch using standard Rust libraries rather than a framework like Substrate.

---

## Quick Start

### Prerequisites

```bash
# Rust toolchain (stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable

# System dependencies (Ubuntu/Debian)
sudo apt-get update
sudo apt-get install -y build-essential clang libclang-dev librocksdb-dev
```

### Build the Node

```bash
cd rust/kratos-core

# Debug build (faster compilation)
cargo build

# Release build (optimized)
cargo build --release
```

### Run in Development Mode

```bash
# Debug build
./target/debug/kratos-node run --dev

# Release build
./target/release/kratos-node run --dev
```

---

## CLI Commands

The `kratos-node` binary provides the following commands:

### `run` - Run the Node

```bash
kratos-node run [OPTIONS]

Options:
  --dev              Run in development mode (single-node testnet)
  --base-path <DIR>  Specify data directory (default: ~/.kratos)
  --port <PORT>      P2P port (default: 30333)
  --rpc-port <PORT>  JSON-RPC port (default: 9944)
  --bootnodes <ADDR> Comma-separated list of bootnode addresses
  --validator        Run as a validator node
```

**Examples:**

```bash
# Development mode (for testing)
kratos-node run --dev

# Custom data directory
kratos-node run --base-path /data/kratos --dev

# Custom ports
kratos-node run --port 30334 --rpc-port 9945 --dev

# Validator node
kratos-node run --validator --base-path ~/.kratos-validator
```

### `info` - Show Node Information

```bash
kratos-node info [OPTIONS]

Options:
  --base-path <DIR>  Data directory to inspect
```

Displays information about the node including chain state, block height, and peer count.

### `key` - Key Management

```bash
kratos-node key <SUBCOMMAND>

Subcommands:
  generate   Generate a new keypair
  inspect    Inspect an existing key
```

**Examples:**

```bash
# Generate a new key
kratos-node key generate

# Generate with specific scheme
kratos-node key generate --scheme ed25519
kratos-node key generate --scheme sr25519

# Inspect a key from seed
kratos-node key inspect "my secret seed phrase"
```

### `export` - Export Chain Data

```bash
kratos-node export [OPTIONS]

Options:
  --base-path <DIR>  Data directory
  --output <FILE>    Output file path
```

Export blockchain data for backup or migration.

### `purge` - Purge Chain Data

```bash
kratos-node purge [OPTIONS]

Options:
  --base-path <DIR>  Data directory to purge
```

**Warning:** This permanently deletes all chain data.

---

## Architecture

### Core Components

| Component | Technology | Description |
|-----------|------------|-------------|
| **Storage** | RocksDB | Persistent key-value storage for blocks, state, and transactions |
| **Networking** | libp2p | P2P networking with gossipsub, kademlia DHT, and mDNS discovery |
| **Cryptography** | ed25519-dalek, schnorrkel | Ed25519 and SR25519 signature schemes |
| **Hashing** | blake3, rs_merkle | Fast hashing and Merkle tree proofs |
| **RPC** | warp | JSON-RPC server for external interactions |
| **Async Runtime** | tokio | Async runtime for concurrent operations |

### Key Features

- **Validator Credits System**: Reputation-based validator selection
- **VRF-based Selection**: Verifiable random function for fair block producer selection
- **Cross-chain Arbitration**: Dispute resolution with jury selection
- **Sidechain Support**: Host chain and sidechain lifecycle management
- **Economic Model**: KRAT token with emission and slashing mechanisms

---

## Network Configuration

### Default Ports

| Service | Port | Description |
|---------|------|-------------|
| P2P | 30333 | Peer-to-peer networking |
| JSON-RPC | 9944 | HTTP/WebSocket RPC |

### Connecting to Peers

```bash
# Connect to specific bootnodes
kratos-node run --bootnodes /ip4/192.168.1.100/tcp/30333/p2p/12D3KooW...

# Multiple bootnodes
kratos-node run --bootnodes /ip4/node1.example.com/tcp/30333/p2p/12D3KooW...,/ip4/node2.example.com/tcp/30333/p2p/12D3KooW...
```

### Local Discovery

In development mode, nodes automatically discover each other via mDNS on the local network.

---

## JSON-RPC API

The node exposes a JSON-RPC API on the configured RPC port.

### Example Requests

```bash
# Get chain info
curl -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getInfo","params":[],"id":1}'

# Get block by number
curl -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getBlock","params":[100],"id":1}'

# Get account balance
curl -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"state_getBalance","params":["5GrwvaEF..."],"id":1}'

# Submit transaction
curl -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"author_submitExtrinsic","params":["0x..."],"id":1}'
```

---

## Running a Validator

### 1. Generate Validator Keys

```bash
# Generate session keys
kratos-node key generate --scheme sr25519

# Save the output:
# Secret seed: "your secret seed phrase"
# Public key: 5GrwvaEF...
# SS58 Address: 5GrwvaEF...
```

### 2. Start Validator Node

```bash
kratos-node run \
  --validator \
  --base-path ~/.kratos-validator \
  --port 30333 \
  --rpc-port 9944
```

### 3. Register as Validator

Submit a validator registration transaction with the required bond amount (minimum stake).

---

## Data Directory Structure

```
~/.kratos/
├── chains/
│   └── kratos/
│       ├── db/              # RocksDB database
│       │   ├── blocks/      # Block storage
│       │   ├── state/       # State trie
│       │   └── tx/          # Transaction pool
│       ├── keystore/        # Encrypted keys
│       └── network/         # Peer identity
└── config.toml              # Optional configuration
```

---

## Development & Testing

### Run Tests

```bash
cd rust/kratos-core

# Run all tests
cargo test

# Run specific test module
cargo test consensus::
cargo test contracts::

# Run with output
cargo test -- --nocapture
```

### Debug Logging

```bash
# Enable debug logging
RUST_LOG=debug kratos-node run --dev

# Specific module logging
RUST_LOG=kratos_core::consensus=debug,kratos_core::network=info kratos-node run --dev
```

---

## Troubleshooting

### Node Won't Start

**Problem:** Port already in use

```bash
# Check what's using the port
lsof -i :30333
lsof -i :9944

# Use different ports
kratos-node run --port 30334 --rpc-port 9945 --dev
```

### Database Corruption

**Problem:** Node fails to start with database errors

```bash
# Purge and restart
kratos-node purge --base-path ~/.kratos
kratos-node run --dev
```

### Peer Connection Issues

**Problem:** Node can't find peers

```bash
# Check firewall
sudo ufw allow 30333/tcp

# Verify bootnode addresses are correct
# Ensure network connectivity
ping node1.example.com
```

### Build Errors

**Problem:** Compilation fails

```bash
# Update Rust
rustup update stable

# Clean build
cargo clean
cargo build --release

# Check dependencies
sudo apt-get install -y librocksdb-dev clang libclang-dev
```

---

## Token Specifications

| Property | Value |
|----------|-------|
| **Symbol** | KRAT |
| **Decimals** | 12 |
| **Initial Supply** | 100,000,000 KRAT |
| **Block Time** | ~6 seconds |
| **Emission** | Deflationary with scheduled halving |

---

## Documentation

- [OVERVIEW.md](OVERVIEW.md) - Project overview and synthesis
- [README.md](README.md) - General documentation
- [spec/SPEC_V1_VALIDATOR_CREDITS.md](spec/SPEC_V1_VALIDATOR_CREDITS.md) - Validator Credits system
- [spec/SPEC_V2_ECONOMICS.md](spec/SPEC_V2_ECONOMICS.md) - Economic model
- [spec/SPEC_V3_SIDECHAINS.md](spec/SPEC_V3_SIDECHAINS.md) - Sidechain lifecycle
- [spec/SPEC_V6_NETWORK_SAFETY.md](spec/SPEC_V6_NETWORK_SAFETY.md) - Network safety mechanisms

---

## Support

For issues and feature requests, see the project repository.

---

**KratOs Node - Custom Rust Blockchain Implementation**
