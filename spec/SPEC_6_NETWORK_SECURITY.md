# SPEC 6: Network Security States

**Version:** 1.2
**Status:** Normative
**Last Updated:** 2025-12-21

### Changelog
| Version | Date | Changes |
|---------|------|---------|
| 1.2 | 2025-12-21 | Added §16 RPC Channel Architecture |
| 1.1 | 2025-12-21 | Added §12-15 (DNS Seeds, Identity, Sync, Genesis) |
| 1.0 | 2025-12-19 | Initial specification |

---

## 1. Overview

KratOs implements a multi-level security state system that responds to validator population changes, ensuring graceful degradation when the network is under-secured and providing clear recovery paths.

**Design Principles:**
- **Automatic transitions:** State changes based on validator count
- **Graceful degradation:** Reduced functionality before emergency
- **Recovery incentives:** Bootstrap economics during recovery
- **Exit guarantee:** Users can always exit regardless of state

---

## 2. Validator Thresholds

### 2.1 Canonical Values

| Threshold | Value | Constant |
|-----------|-------|----------|
| Emergency | 25 | EMERGENCY_VALIDATORS |
| Post-Bootstrap Min | 50 | POST_BOOTSTRAP_MIN_VALIDATORS |
| Safe | 75 | SAFE_VALIDATORS |
| Optimal | 101 | OPTIMAL_VALIDATORS |

### 2.2 Threshold Interpretation

```
0-24:   Emergency (critical failure)
25-49:  Restricted (governance frozen)
50-74:  Degraded (reduced functionality)
75-100: Normal (full operation)
101:    Optimal (maximum security)
```

---

## 3. Security States

### 3.1 State Machine

```
Bootstrap → Normal → Degraded → Restricted → Emergency
    ↑                   ↓           ↓
    ←── BootstrapRecoveryMode ←←←←←─
```

### 3.2 State Definitions

| State | Validator Range | Entry Condition |
|-------|-----------------|-----------------|
| Bootstrap | Any | epoch < 1440 |
| Normal | >= 75 | V >= SafeValidators |
| Degraded | 50-74 | V < SafeValidators |
| Restricted | 25-49 | V < PostBootstrapMin |
| Emergency | < 25 | V < EmergencyValidators |
| BootstrapRecovery | < 50 | 10 epochs below min |

---

## 4. State Effects

### 4.1 Bootstrap (First 60 Days)

| Parameter | Effect |
|-----------|--------|
| Inflation | Fixed 6.5% |
| VC Multipliers | 2x for votes/uptime |
| Governance | Normal |
| Exit | Exit requires bootstrap complete |

### 4.2 Normal

| Parameter | Effect |
|-----------|--------|
| Inflation | Adaptive (0.5-10%) |
| Governance | Normal |
| All features | Enabled |

### 4.3 Degraded (DegradedSecurityMode)

| Parameter | Effect |
|-----------|--------|
| Inflation | +1% boost |
| Governance timelocks | × 2 |
| New sidechains | Paused |
| Validator incentives | Boosted |

### 4.4 Restricted (SafetyHaltMode)

| Parameter | Effect |
|-----------|--------|
| Governance | Frozen |
| Validator incentives | Maximum boost |
| Emergency triggers | Armed |
| New transactions | Limited |

### 4.5 Emergency (TerminalMode)

| Parameter | Effect |
|-----------|--------|
| Emergency powers | Auto-activated |
| Fork declaration | Allowed |
| Asset exit | Always permitted |
| Slashing escalation | Disabled |
| Identity freeze | Disabled |

---

## 5. Transitions

### 5.1 Downward Transitions (Immediate)

| From | To | Trigger |
|------|-----|---------|
| Normal | Degraded | V < 75 |
| Degraded | Restricted | V < 50 |
| Restricted | Emergency | V < 25 |

### 5.2 Upward Transitions (Requires Stability)

| From | To | Requirement |
|------|-----|-------------|
| Emergency | Restricted | V >= 25 |
| Restricted | Degraded | V >= 50 |
| Degraded | Normal | V >= 75 for 100 epochs |

### 5.3 Bootstrap Exit

Requires ALL conditions:

1. Epoch >= 1440 (60 days)
2. Validators >= 50
3. Average participation >= 90% (last 100 epochs)

---

## 6. Recovery Mechanisms

### 6.1 Normal Recovery

| Requirement | Value |
|-------------|-------|
| Validator count | >= 75 |
| Duration | 100 consecutive epochs |

### 6.2 Collapse Detection

| Trigger | 10 epochs below 50 validators |
|---------|-------------------------------|
| Action | Enter BootstrapRecoveryMode |
| Effect | Bootstrap economics re-enabled |

### 6.3 Bootstrap Recovery Mode

| Parameter | Effect |
|-----------|--------|
| Inflation | 100% to validators |
| VC multipliers | 2x (bootstrap rates) |
| Stake requirements | Reduced |

---

## 7. Safety Invariants

### 7.1 Security Invariants

1. **No insecure bootstrap exit:** Cannot exit bootstrap with < 50 validators
2. **No silent collapse:** Transition to degraded states is logged and observable
3. **Automatic degradation:** State changes require no governance action
4. **Emergency without capture:** Emergency powers cannot be blocked
5. **Exit without permission:** Users can always withdraw assets
6. **Fork without punishment:** Fork participation never punished

### 7.2 Tested Invariants

```rust
assert!(bootstrap_exit_requires_50_validators);
assert!(emergency_triggers_at_25_validators);
assert!(recovery_requires_100_epochs_at_75);
assert!(exit_always_possible);
```

---

## 8. Configuration

### 8.1 DegradedSecurityConfig

```rust
pub struct DegradedSecurityConfig {
    // Degraded State (50-74 validators)
    degraded_inflation_boost_percent: u32,        // 1
    degraded_governance_timelock_multiplier: u32, // 2
    degraded_sidechains_paused: bool,             // true

    // Restricted State (25-49 validators)
    restricted_governance_frozen: bool,            // true
    restricted_validator_incentives_boosted: bool, // true
    restricted_emergency_armed: bool,              // true

    // Emergency State (< 25 validators)
    emergency_auto_trigger: bool,          // true
    emergency_fork_allowed: bool,          // true
    emergency_exit_always_allowed: bool,   // true
    emergency_no_slashing_escalation: bool, // true

    // Recovery
    normal_recovery_epochs: EpochNumber,    // 100
    collapse_detection_epochs: EpochNumber, // 10
}
```

---

## 9. Monitoring

### 9.1 Key Metrics

| Metric | Description |
|--------|-------------|
| validator_count | Current active validators |
| security_state | Current state name |
| epochs_in_state | Duration in current state |
| recovery_progress | Epochs toward recovery |

### 9.2 Alerting Thresholds

| Level | Validator Count |
|-------|-----------------|
| Warning | < 80 |
| Critical | < 60 |
| Emergency | < 30 |

---

## 10. Implementation

### 10.1 Constants

```rust
pub const BOOTSTRAP_MIN_VALIDATORS: u32 = 1;
pub const POST_BOOTSTRAP_MIN_VALIDATORS: u32 = 50;
pub const SAFE_VALIDATORS: u32 = 75;
pub const OPTIMAL_VALIDATORS: u32 = 101;
pub const EMERGENCY_VALIDATORS: u32 = 25;
pub const BOOTSTRAP_EPOCHS_MIN: EpochNumber = 1440;
pub const MIN_PARTICIPATION_PERCENT: u32 = 90;
pub const PARTICIPATION_WINDOW: EpochNumber = 100;
pub const NORMAL_RECOVERY_EPOCHS: EpochNumber = 100;
pub const COLLAPSE_DETECTION_EPOCHS: EpochNumber = 10;
```

### 10.2 Source Files

| File | Contents |
|------|----------|
| `consensus/economics.rs` | Security state machine |
| `contracts/emergency.rs` | Emergency powers |
| `node/service.rs` | State monitoring |

---

## 11. State Data Structures

### 11.1 NetworkSecurityState Enum

```rust
pub enum NetworkSecurityState {
    Bootstrap,
    Normal,
    DegradedSecurityMode {
        entered_at: EpochNumber,
        epochs_in_dsm: EpochNumber,
        current_validators: u32,
        validators_needed: u32,
        consecutive_epochs_above_safe: EpochNumber,
    },
    SafetyHaltMode {
        entered_at: EpochNumber,
        epochs_in_shm: EpochNumber,
        current_validators: u32,
        epochs_without_finality: EpochNumber,
    },
    TerminalMode {
        entered_at: EpochNumber,
        epochs_in_terminal: EpochNumber,
        current_validators: u32,
        terminal_state_root: Option<[u8; 32]>,
    },
    BootstrapRecoveryMode {
        entered_at: EpochNumber,
        epochs_in_recovery: EpochNumber,
        current_validators: u32,
        validators_needed: u32,
    },
}
```

---

## 12. Peer Discovery - DNS Seeds

### 12.1 Overview

KratOs implements decentralized peer discovery via **DNS Seeds** - specialized DNS servers that return IP addresses of active network nodes. This enables nodes to join the network automatically without manual bootnode configuration.

**Design Principles:**
- **Decentralized:** Multiple independent seed operators
- **Resilient:** Fallback mechanisms if DNS fails
- **Permissionless:** Anyone can operate a seed
- **Censorship-resistant:** Geographic and organizational diversity

### 12.2 Discovery Hierarchy

```
Node Startup
    │
    ▼
1. DNS Seeds + Fallback Bootnodes (Primary)
    │ DNS: seed1.kratos.network → [IP list]
    │ Bootnodes: /ip4/X.X.X.X/tcp/30333/p2p/...
    ▼
2. CLI Bootnodes (Manual)
    │ --bootnode /ip4/X.X.X.X/tcp/30333/p2p/...
    ▼
3. Kademlia DHT (Propagation)
    │ Learn peers from connected nodes
    ▼
Connected to Network
```

### 12.3 DNS Seed Requirements

| Requirement | Value |
|-------------|-------|
| Min independent operators | 3 |
| Geographic regions | >= 2 |
| Uptime SLA | 99.9% |
| Update frequency | <= 60 seconds |
| Max stale entries | 10% |

### 12.4 Seed Server Architecture

```
┌─────────────────────────────────────────┐
│            DNS Seed Server              │
├─────────────────────────────────────────┤
│ Crawler    │ Scans network continuously │
│ Database   │ Stores active peer list    │
│ DNS Server │ Responds to A/AAAA queries │
│ Monitor    │ Health checks on peers     │
└─────────────────────────────────────────┘
```

### 12.5 Becoming a Seed Operator

1. Run a DNS seed server (open source implementation)
2. Maintain 99.9% uptime for 30 days
3. Submit PR to add seed to official list
4. Pass community review for independence

### 12.6 Security Considerations

| Threat | Mitigation |
|--------|------------|
| Poisoned seeds | Multiple independent sources |
| DNS hijacking | Fallback bootnodes always included |
| Eclipse attack | DHT propagation diversifies peers |
| Sybil seeds | Community vetting of operators |

### 12.7 Implementation

| File | Contents |
|------|----------|
| `network/dns_seeds.rs` | DNS resolver, seed registry |
| `node/service.rs` | Integration at startup |

### 12.8 Configuration

```rust
// Official DNS Seeds (populated at mainnet)
pub const OFFICIAL_DNS_SEEDS: &[&str] = &[
    "45.8.132.252",  // KratOs Dev VPS
];

// Fallback hardcoded bootnodes (always included in resolution)
pub const FALLBACK_BOOTNODES: &[&str] = &[
    "/ip4/45.8.132.252/tcp/30333/p2p/12D3KooWEeXJRXqC1XANbsKAzibKisFNitZhbi2RArciXuEHJGcx",
];

// Default P2P port
pub const DEFAULT_P2P_PORT: u16 = 30333;
```

### 12.9 Current Bootstrap Nodes

| Node | IP | Peer ID | Operator |
|------|-----|---------|----------|
| Genesis Node | 45.8.132.252 | 12D3KooWEeXJRXqC... | KratOs Dev |

**Note:** Additional bootstrap nodes will be added as the network grows.

---

## 13. Network Identity Persistence

### 13.1 Overview

Each KratOs node has a unique **PeerId** derived from an Ed25519 keypair. This identity is persisted to disk to ensure stable network topology across restarts.

### 13.2 Identity Storage

| Item | Location |
|------|----------|
| Network key file | `<data_dir>/network/network_key` |
| Key format | Ed25519 secret key (32 bytes) |
| Permissions | 0600 (Unix) |

### 13.3 Behavior

| Scenario | Action |
|----------|--------|
| First startup | Generate new keypair, save to disk |
| Subsequent startups | Load existing keypair from disk |
| No data directory | Use ephemeral keypair (warning logged) |

### 13.4 Implementation

```rust
// In network/service.rs

const NETWORK_KEY_FILENAME: &str = "network_key";

fn load_or_generate_keypair(data_dir: Option<&PathBuf>) -> Result<Keypair, Error> {
    if let Some(dir) = data_dir {
        let key_path = dir.join("network").join(NETWORK_KEY_FILENAME);

        if key_path.exists() {
            // Load existing keypair
            let key_bytes = std::fs::read(&key_path)?;
            Keypair::ed25519_from_bytes(key_bytes)
        } else {
            // Generate and save new keypair
            let keypair = Keypair::generate_ed25519();
            std::fs::write(&key_path, keypair.secret())?;
            Ok(keypair)
        }
    } else {
        // Ephemeral mode
        Ok(Keypair::generate_ed25519())
    }
}
```

### 13.5 Security Considerations

| Consideration | Implementation |
|---------------|----------------|
| Key protection | File permissions set to 0600 |
| Key backup | Users should backup `network_key` file |
| Key rotation | Delete file to generate new identity |

---

## 14. Sync Protocol Security

### 14.1 Block Buffering

Nodes buffer out-of-order blocks during synchronization to prevent false-positive peer banning:

```
┌─────────────────────────────────────────────┐
│              SyncManager                     │
├─────────────────────────────────────────────┤
│ pending_blocks: HashMap<BlockNumber, Block> │
│ download_queue: VecDeque<BlockNumber>       │
│ batch_size: 50                              │
└─────────────────────────────────────────────┘
```

### 14.2 Buffering Rules

| Block Number | Local Height | Action |
|--------------|--------------|--------|
| <= local | Any | Ignore (duplicate/stale) |
| local + 1 | Any | Import immediately |
| > local + 1 | <= best_known + 100 | Buffer |
| > best_known + 100 | Any | Reject (too far ahead) |

### 14.3 Buffer Drain

After each successful import, buffered blocks are processed:

```rust
loop {
    let next = buffer.get(local_height + 1);
    match next {
        Some(block) => import(block),
        None => break,
    }
}
```

### 14.4 Peer Trust Levels

| Behavior | Trust Impact |
|----------|--------------|
| Valid blocks | Increase |
| Out-of-order blocks | Neutral |
| Invalid signature | Ban immediately |
| Invalid parent hash | Ban immediately |
| Timeout | Decrease |

---

## 15. Genesis Timestamp Security

### 15.1 Canonical Time Reference

The genesis block timestamp serves as the canonical time reference for all slot calculations:

```
genesis_timestamp = genesis_block.header.timestamp
slot_for_block = ((block.timestamp - genesis_timestamp) / 6) % 600
```

### 15.2 Why This Matters

Without a canonical genesis timestamp:
- Nodes using wall-clock time would reject valid blocks
- Time drift between nodes would cause chain splits
- Syncing nodes would fail with "TimestampSlotMismatch"

### 15.3 Genesis Time Propagation

```
Genesis Node
    │ creates genesis block with timestamp T
    ▼
GenesisResponse
    │ includes genesis block
    ▼
Joining Node
    │ extracts T from genesis.header.timestamp
    │ stores in GenesisConfig.genesis_timestamp
    ▼
All Slot Calculations
    │ use T as reference
```

---

## 16. RPC Channel Architecture

### 16.1 Design Rationale

The KratOs node uses a **channel-based RPC pattern** because libp2p's `Swarm` is not `Sync`. The HTTP server cannot directly access network/state components that require the swarm.

### 16.2 Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                       HTTP Server                             │
│  (Receives JSON-RPC requests)                                │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  route_request() ─────────────► RpcCall Variant              │
│       │                              │                        │
│       │                              ▼                        │
│       │                    mpsc::Sender<RpcCall>              │
│       │                              │                        │
│       │                              │                        │
│       ▼                              │                        │
│  oneshot::Receiver ◄─────────────────┘                       │
│       │                                                       │
│       ▼                                                       │
│  JsonRpcResponse                                              │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│                    CLI Runner Event Loop                      │
│  (Owns Swarm and Node)                                       │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  mpsc::Receiver<RpcCall> ─────► handle_rpc_call()            │
│                                       │                       │
│                                       ▼                       │
│                             Match on RpcCall variant         │
│                                       │                       │
│                                       ▼                       │
│                              Execute node operation          │
│                                       │                       │
│                                       ▼                       │
│                             oneshot::Sender::send(result)    │
└──────────────────────────────────────────────────────────────┘
```

### 16.3 RpcCall Enum

| Variant | Description |
|---------|-------------|
| `StateGetAccount` | Query account balance/nonce |
| `StateGetNonce` | Query account nonce only |
| `ChainGetInfo` | Get chain height/epoch info |
| `AuthorSubmitTransaction` | Submit signed transaction |
| `ValidatorGetEarlyVotingStatus` | Get bootstrap voting status |
| `ValidatorGetPendingCandidates` | List validator candidates |
| `ValidatorGetCandidateVotes` | Get votes for candidate |
| `ValidatorCanVote` | Check if account can vote |

### 16.4 Adding New RPC Methods

To add a new RPC method:

1. **Add variant to `RpcCall` enum** (`rpc/server.rs`)
2. **Add route in `route_request()`** (`rpc/server.rs`)
3. **Add handler function** (`rpc/server.rs`)
4. **Add match arm in `handle_rpc_call()`** (`cli/runner.rs`)

### 16.5 Source Files

| File | Contents |
|------|----------|
| `rpc/server.rs` | RpcCall enum, route_request(), handlers |
| `cli/runner.rs` | handle_rpc_call() match arms |

---

## 17. Related Specifications

- **SPEC 1:** Tokenomics - Inflation adjustments per state
- **SPEC 2:** Validator Credits - VC multipliers during bootstrap
- **SPEC 3:** Consensus - Block synchronization and validation
- **SPEC 5:** Governance - Governance freeze in restricted
- **SPEC 8:** Wallet - Client-side RPC usage
