# KratOs Genesis Block Testing Guide

## Overview

This guide explains how to test the KratOs genesis block creation and initial chain state. KratOs is a native Rust blockchain implementation with custom genesis configuration.

**Source Code**: `rust/kratos-core/src/genesis/`

---

## Genesis Configuration

### Chain Configuration

**File**: `src/genesis/config.rs`

KratOs uses a **single unified configuration** - there are no separate network modes. The protocol operates identically regardless of deployment context.

| Property | Value | Description |
|----------|-------|-------------|
| Chain Name | KratOs | Single network name |
| Chain ID | 0 | Unified chain identifier |
| Configuration | `ChainConfig::default()` | One configuration for all deployments |

**Note**: For testing, duplicate the project rather than using mode flags. See [KRATOS_SYNTHESIS.md](KRATOS_SYNTHESIS.md) for unified architecture details.

### Configuration Structure

```rust
pub struct ChainConfig {
    pub chain_name: String,
    pub chain_id: u32,
    pub consensus: ConsensusConfig,
    pub network: NetworkConfig,
    pub tokenomics: TokenomicsConfig,
}

pub struct ConsensusConfig {
    pub epoch_duration: u64,      // 600 blocks (1 hour)
    pub slot_duration: u64,       // 6 seconds
    pub min_validators: usize,    // 10 (mainnet)
    pub max_validators: usize,    // 1000
}

pub struct TokenomicsConfig {
    pub initial_supply: u128,          // 1 billion KRAT
    pub initial_emission_rate: u32,    // 500 bps (5%)
    pub initial_burn_rate: u32,        // 100 bps (1%)
}
```

---

## Genesis Specification

### Initial State

**File**: `src/genesis/spec.rs`

The genesis block defines:
- Genesis block (block 0)
- Initial account balances
- Genesis validators (dev mode only)
- System parameters

### Development Mode Genesis

In `--dev` mode, genesis includes:

| Account | Balance | Role |
|---------|---------|------|
| Genesis Validator | 1,000,000 KRAT | Block producer |
| Treasury | 0 KRAT | Fee recipient |

The genesis validator is automatically created from a deterministic seed for reproducible testing.

---

## Testing Steps

### Step 1: Build the Node

```bash
cd rust/kratos-core

# Debug build (faster compilation)
cargo build

# Release build (optimized)
cargo build --release

# Run tests
cargo test
```

### Step 2: Start Development Node

```bash
# Start with dev mode
./target/debug/kratos-node run --dev

# With custom ports
./target/debug/kratos-node run --dev --port 30334 --rpc-port 9945

# With validator flag
./target/debug/kratos-node run --dev --validator
```

### Step 3: Verify Node Startup

Expected output:

```
KratOs Node v0.1.0
Chain: KratOs Dev
Role: Validator
P2P listening on /ip4/0.0.0.0/tcp/30333
RPC listening on 0.0.0.0:9944
Local peer ID: 12D3KooW...
Block #0 (genesis)
Block #1 | +12.366 KRAT
Block #2 | +12.366 KRAT
```

### Step 4: Query Genesis Block via RPC

```bash
# Get genesis block hash
curl -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getBlockHash","params":[0],"id":1}'

# Get genesis block details
curl -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getBlock","params":[0],"id":1}'

# Get chain info
curl -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getInfo","params":[],"id":1}'
```

---

## Genesis Validation Checklist

### Block Properties

- [ ] Block number is 0
- [ ] Parent hash is `0x0000...0000` (32 zero bytes)
- [ ] Block hash is deterministic (same every restart)
- [ ] State root is correctly computed
- [ ] Timestamp is genesis time

### State Properties

- [ ] Initial supply is 1,000,000,000 KRAT
- [ ] Genesis validator has correct stake
- [ ] Treasury account exists
- [ ] No unexpected accounts

### Chain Properties

- [ ] Chain can produce block #1
- [ ] Block rewards are ~12.37 KRAT (bootstrap)
- [ ] Epoch 0 is active
- [ ] Bootstrap mode is enabled

---

## Common Genesis Tests

### Test 1: Genesis Block Hash Consistency

The genesis block hash should be identical across restarts:

```bash
# First run
./target/debug/kratos-node run --dev &
sleep 5
HASH1=$(curl -s http://localhost:9944 -d '{"jsonrpc":"2.0","method":"chain_getBlockHash","params":[0],"id":1}' | jq -r '.result')
pkill kratos-node

# Purge and restart
./target/debug/kratos-node purge --base-path ~/.local/share/kratos/chains/dev
./target/debug/kratos-node run --dev &
sleep 5
HASH2=$(curl -s http://localhost:9944 -d '{"jsonrpc":"2.0","method":"chain_getBlockHash","params":[0],"id":1}' | jq -r '.result')
pkill kratos-node

# Compare
echo "Hash 1: $HASH1"
echo "Hash 2: $HASH2"
[ "$HASH1" = "$HASH2" ] && echo "SUCCESS: Genesis hashes match" || echo "FAIL: Genesis hashes differ"
```

### Test 2: Initial Supply Verification

```bash
curl -X POST http://localhost:9944 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getInfo","params":[],"id":1}' | jq '.result.total_supply'

# Expected: 1000000000000000000000 (1B KRAT in base units)
```

### Test 3: Block Production

Verify blocks are produced with correct rewards:

```bash
# Start node
./target/debug/kratos-node run --dev --validator &

# Wait for blocks
sleep 30

# Check block height
curl -s http://localhost:9944 \
  -d '{"jsonrpc":"2.0","method":"chain_getLatestBlock","params":[],"id":1}' | jq '.result.header.number'

# Should be > 0
```

### Test 4: Bootstrap Mode Active

During the first 1440 epochs (60 days), bootstrap mode should be active with 6.5% inflation:

```bash
# Check block rewards in logs
RUST_LOG=kratos_node::node::producer=debug ./target/debug/kratos-node run --dev 2>&1 | grep "Block reward"

# Expected: ~12,366,818,873,668 base units (~12.37 KRAT)
```

---

## Automated Tests

### Unit Tests

Run genesis-related unit tests:

```bash
cd rust/kratos-core

# Run all tests
cargo test

# Run specific genesis tests
cargo test genesis::

# Run config tests
cargo test config::
```

### Integration Tests

```bash
# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_genesis_config -- --nocapture
```

### Example Test Cases

```rust
#[test]
fn test_genesis_block_properties() {
    let genesis = create_genesis_block();

    assert_eq!(genesis.header.number, 0);
    assert_eq!(genesis.header.parent_hash, Hash::zero());
    assert!(!genesis.header.state_root.is_zero());
}

#[test]
fn test_genesis_initial_supply() {
    let state = StateBackend::genesis();

    assert_eq!(state.total_supply(), INITIAL_SUPPLY);
    assert_eq!(state.total_supply(), 1_000_000_000 * KRAT);
}

#[test]
fn test_bootstrap_config() {
    let config = get_bootstrap_config();

    // Unified configuration values
    assert_eq!(config.end_epoch, 1440);
    assert_eq!(config.min_validators_exit, 50);
    assert_eq!(config.target_inflation, 0.065);
}
```

---

## Troubleshooting

### Issue: Genesis Hash Mismatch

**Symptom**: Genesis hash differs between restarts

**Causes**:
- Timestamp in genesis block
- Non-deterministic state initialization

**Solution**: Ensure genesis timestamp is fixed (not using current time)

### Issue: No Blocks After Genesis

**Symptom**: Node starts but block #1 never appears

**Causes**:
- Validator not enabled
- No validator key configured
- VRF selection not matching

**Solution**:
```bash
# Ensure validator mode
./target/debug/kratos-node run --dev --validator
```

### Issue: Wrong Block Rewards

**Symptom**: Block rewards are 10 KRAT instead of ~12.37 KRAT

**Causes**:
- Dev mode not enabled
- Epoch calculation using time instead of blocks

**Solution**: Ensure `enable_dev_mode()` is called for `--dev` flag

### Issue: Database Locked

**Symptom**: "Database lock file exists"

**Solution**:
```bash
rm -f ~/.local/share/kratos/chains/dev/LOCK
```

---

## Multi-Node Testing

### Two-Node Local Network

**Terminal 1 (Node 1)**:
```bash
./target/debug/kratos-node run \
  --port 30333 \
  --rpc-port 9944 \
  --validator
```

**Terminal 2 (Node 2)**:
```bash
./target/debug/kratos-node run \
  --port 30334 \
  --rpc-port 9945 \
  --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/<PEER_ID>
```

### Verify Synchronization

Both nodes should have the same:
- Genesis hash
- Current block height (after sync)
- State root

```bash
# Node 1
curl -s localhost:9944 -d '{"jsonrpc":"2.0","method":"chain_getBlockHash","params":[0],"id":1}' | jq

# Node 2
curl -s localhost:9945 -d '{"jsonrpc":"2.0","method":"chain_getBlockHash","params":[0],"id":1}' | jq
```

---

## Genesis Parameters Reference

### Tokenomics

| Parameter | Value | Constant |
|-----------|-------|----------|
| Initial Supply | 1,000,000,000 KRAT | `INITIAL_SUPPLY` |
| Initial Emission | 5.0%/year | `INITIAL_EMISSION_RATE_BPS = 500` |
| Min Emission | 0.5%/year | `MIN_EMISSION_RATE_BPS = 50` |
| Initial Burn | 1.0%/year | `INITIAL_BURN_RATE_BPS = 100` |
| Max Burn | 3.5%/year | `MAX_BURN_RATE_BPS = 350` |

### Consensus

| Parameter | Value | Constant |
|-----------|-------|----------|
| Epoch Duration | 600 blocks | `EPOCH_DURATION_BLOCKS` |
| Slot Duration | 6 seconds | `SLOT_DURATION_SECS` |
| Bootstrap Duration | 1440 epochs | `end_epoch` |
| Bootstrap Inflation | 6.5% | `target_inflation` |

### Network

| Parameter | Value |
|-----------|-------|
| P2P Port | 30333 |
| RPC Port | 9944 |
| Protocol Name | /kratos/1.0.0 |

---

## Quick Commands

```bash
# Build and test
cargo build && cargo test

# Start dev node
./target/debug/kratos-node run --dev

# Start with debug logging
RUST_LOG=debug ./target/debug/kratos-node run --dev

# Purge chain data
./target/debug/kratos-node purge --base-path ~/.local/share/kratos/chains/dev

# Check genesis
curl localhost:9944 -d '{"jsonrpc":"2.0","method":"chain_getBlockHash","params":[0],"id":1}'

# Check current height
curl localhost:9944 -d '{"jsonrpc":"2.0","method":"chain_getLatestBlock","params":[],"id":1}' | jq '.result.header.number'
```

---

**Implementation Status**: Complete
**Last Updated**: 2025-12-19
**Framework**: Native Rust (no Substrate)
**Specification Version**: Unified (see [KRATOS_SYNTHESIS.md](KRATOS_SYNTHESIS.md))
