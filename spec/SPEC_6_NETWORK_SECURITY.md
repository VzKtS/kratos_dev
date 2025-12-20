# SPEC 6: Network Security States

**Version:** 1.0
**Status:** Normative
**Last Updated:** 2025-12-19

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
    "/ip4/45.8.132.252/tcp/30333/p2p/12D3KooWEko82RoEwFb1tr6KkmgNhCdGKUdoTjrMcex5WQnvaKSY",
];

// Default P2P port
pub const DEFAULT_P2P_PORT: u16 = 30333;
```

### 12.9 Current Bootstrap Nodes

| Node | IP | Peer ID | Operator |
|------|-----|---------|----------|
| Dev VPS | 45.8.132.252 | 12D3KooWEko82Ro... | KratOs Dev |

**Note:** Additional bootstrap nodes will be added as the network grows.

---

## 13. Related Specifications

- **SPEC 1:** Tokenomics - Inflation adjustments per state
- **SPEC 2:** Validator Credits - VC multipliers during bootstrap
- **SPEC 5:** Governance - Governance freeze in restricted
