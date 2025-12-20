# KratOs Protocol Synthesis

**Version:** 1.10
**Status:** Normative
**Last Updated:** 2025-12-19

---

## 1. Protocol Philosophy

KratOs is a minimal, auditable, and durable blockchain protocol designed for coexistence rather than dominance.

### Core Principles

| Principle | Description |
|-----------|-------------|
| **Power is slow** | Governance changes require time; no instant protocol changes |
| **Failures are local** | One chain's failure doesn't affect others |
| **Exit is always possible** | Capital is never permanently frozen |
| **Merit over wealth** | Validator Credits balance stake-based power |

### Unified Architecture

KratOs operates with a **single unified configuration**. There are no dev/devnet/testnet/mainnet mode flags within the codebase. This design:

- Eliminates mode-related bugs
- Simplifies logical interactions
- Ensures consistent behavior
- Makes auditing straightforward

### Repository Strategy

The unified architecture is maintained through a **dual-repository model**:

```
┌─────────────────────────────────────────────────────────────┐
│          github.com/VzKtS/kratos_dev (Development)           │
│                                                              │
│  Purpose: Active development and testing                     │
│  Environments: dev (local), devnet (shared testing)          │
│  Access: Full read/write for developers                      │
│                                                              │
│  ┌──────────────┐     ┌──────────────┐                      │
│  │     dev      │ ──► │    devnet    │                      │
│  │   (local)    │     │   (shared)   │                      │
│  └──────────────┘     └──────────────┘                      │
│                              │                               │
│                              ▼                               │
│                    kratosnode-release_dev                    │
│                       (validated build)                      │
└──────────────────────────────┼───────────────────────────────┘
                               │
                               ▼  push/commit
┌──────────────────────────────────────────────────────────────┐
│            github.com/VzKtS/KratOs (Production)              │
│                                                              │
│  Purpose: Stable, audited releases                           │
│  Environments: testnet (staging), mainnet (live)             │
│  Access: Commits from kratos_dev releases only               │
│                                                              │
│  ┌──────────────┐     ┌──────────────┐                      │
│  │   testnet    │ ──► │   mainnet    │                      │
│  │  (staging)   │     │    (live)    │                      │
│  └──────────────┘     └──────────────┘                      │
└──────────────────────────────────────────────────────────────┘
```

| Repository | URL | Environments | Branches |
|------------|-----|--------------|----------|
| **kratos_dev** | github.com/VzKtS/kratos_dev | dev, devnet | dev, devnet, security |
| **KratOs** | github.com/VzKtS/KratOs | testnet, mainnet | No (main only) |

**kratos_dev Branch Structure:**

| Branch | Bootstrap | Validators | Purpose |
|--------|-----------|------------|---------|
| **dev** | Short cycle (accelerated) | Local/simulated | Rapid development iteration |
| **devnet** | None (post-bootstrap) | Simulated network | Integration testing, production-like behavior |
| **security** | Normal (from KratOs) | Production params | Security fixes with fast turnaround |

```
kratos_dev (github.com/VzKtS/kratos_dev)
────────────────────────────────────────

┌─────────────────────────────────────────────────────────────────┐
│                                                                 │
│  dev branch ────────────┐                                       │
│  (short bootstrap)      │                                       │
│                         ├──► merge ──► devnet branch            │
│  feature/xxx ───────────┘              (no bootstrap,           │
│                                         simulated validators)   │
│                                              │                  │
│                                              ▼                  │
│                                kratosnode-release_dev           │
│                                              │                  │
└──────────────────────────────────────────────┼──────────────────┘
                                               │
                       ┌───────────────────────┤
                       │                       │
                       ▼                       ▼
┌──────────────────────────────┐    ┌─────────────────────────────┐
│  security branch             │    │  KratOs (Production)        │
│  (cloned from KratOs/main)   │    │  github.com/VzKtS/KratOs    │
│                              │    │                             │
│  - Production parameters     │───►│  main only (linear history) │
│  - Fast security patches     │    │                             │
│  - Normal bootstrap config   │    │  testnet ──► mainnet        │
└──────────────────────────────┘    └─────────────────────────────┘
```

**Branch Configuration Details:**

| Branch | Bootstrap Config | Validator Mode |
|--------|------------------|----------------|
| **dev** | `end_epoch: 10`, accelerated timings | Single local validator |
| **devnet** | Disabled (simulates mature network) | Multi-validator simulation |
| **security** | `end_epoch: 1440` (production) | Production parameters |

**Release Flow:**

```
Development path:
dev branch ──► devnet branch ──► kratosnode-release_dev ──► KratOs/main
                                                                 │
                                                       ┌─────────┴─────────┐
                                                       ▼                   ▼
                                                   testnet              mainnet

Security path:
KratOs/main ──► security branch ──► fix ──► kratosnode-release_dev ──► KratOs/main
                (fast turnaround)
```

**Key Rules:**

1. **No direct commits to KratOs** - All changes must originate from kratos_dev
2. **No branches on KratOs** - Linear history, main only
3. **Release gate** - Only `kratosnode-release_dev` builds are pushed to KratOs
4. **Security branch** - Cloned from KratOs/main for urgent fixes with production params
5. **devnet = Production simulation** - No bootstrap, validators simulated as if network is live
6. **Single codebase** - All branches share identical code structure, only config differs

**Post-Mainnet Governance:**

Once mainnet is launched, all protocol upgrades require **Root Chain consensus**:

```
┌─────────────────────────────────────────────────────────────────┐
│                     Pre-Mainnet (current)                       │
│                                                                 │
│  kratos_dev ──► KratOs ──► testnet ──► mainnet launch          │
│  (developer discretion)                                         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼ mainnet launch
┌─────────────────────────────────────────────────────────────────┐
│                     Post-Mainnet                                │
│                                                                 │
│  kratos_dev ──► proposal ──► Root Chain Vote ──► KratOs/main   │
│                     │              │                            │
│                     │         51% standard                      │
│                     │         67% critical                      │
│                     ▼                                           │
│              testnet staging ──► mainnet deploy                 │
└─────────────────────────────────────────────────────────────────┘
```

| Phase | Upgrade Authority | Process |
|-------|-------------------|---------|
| **Pre-Mainnet** | Developers | Direct push to KratOs |
| **Post-Mainnet** | Root Chain Validators | Governance proposal required |

**Post-Mainnet Upgrade Types:**

| Upgrade Type | Threshold | Timelock | Example |
|--------------|-----------|----------|---------|
| Parameter change | 51% | 12 days | Fee adjustment |
| Protocol upgrade | 51% | 12 days | New features |
| Critical/Security | 67% | 30 days | Consensus changes |
| Emergency fix | 67% | Fast-track | Active exploit response |

**INVARIANT:** After mainnet launch, no code reaches production without Root Chain validator consensus.

---

## 2. Token (KRAT)

### Base Properties

| Property | Value |
|----------|-------|
| Symbol | KRAT |
| Decimals | 12 |
| Initial Supply | 1,000,000,000 KRAT |
| Existential Deposit | 0.001 KRAT |

### Economic Lifecycle

```
Year 0-1:  Bootstrap Era (fixed 6.5% inflation)
Year 1-5:  Adaptive inflation (0.5-10%)
Year 5+:   Deflationary (burn > emission)
Year 20:   ~824M KRAT supply
```

### Fee Distribution (60/30/10)

| Recipient | Share |
|-----------|-------|
| Validators | 60% |
| Burn | 30% |
| Treasury | 10% |

---

## 3. Time & Epochs

### Block Parameters

| Parameter | Value |
|-----------|-------|
| Block time | 6 seconds |
| Blocks per epoch | 600 |
| Epoch duration | 1 hour |
| Bootstrap duration | 1,440 epochs (60 days) |

### Logical Time Units

| Unit | Epochs | Purpose |
|------|--------|---------|
| Day | 1 epoch | Vote credit limit |
| Month | 4 epochs | Vote monthly limit, seniority |
| Year | 52 epochs | Arbitration limit |

---

## 4. Validator System

### Selection Formula (VRF-Weighted)

```
VRF_weight = min(sqrt(stake), sqrt(STAKE_CAP)) × ln(1 + VC)
```

**Properties:**
- Square-root stake provides diminishing returns
- Stake capped at 1M KRAT prevents whale dominance
- VC = 0 means weight = 0 (cannot be selected)
- Logarithmic VC growth rewards consistency over grinding

### Validator Thresholds

| Threshold | Count | Effect |
|-----------|-------|--------|
| Emergency | < 25 | Terminal mode |
| Restricted | 25-49 | Governance frozen |
| Degraded | 50-74 | Reduced functionality |
| Normal | 75-100 | Full operation |
| Optimal | 101 | Maximum security |

### Validator Limits

| Chain Type | Maximum |
|------------|---------|
| Root Chain | 101 |
| Sidechain | 100 |
| Hostchain | 200 (aggregate) |
| Network | 1,000 |

---

## 5. Validator Credits (VC)

### Credit Types

| Type | Rate | Limit |
|------|------|-------|
| Vote | +1/vote | 3/epoch, 50/month |
| Uptime | +1/epoch | >= 95% participation |
| Arbitration | +5/case | 5/year |
| Seniority | +5/month | Automatic |

### Maximum Accumulation

- Perfect participation: ~298 VC/year
- Realistic: ~200-250 VC/year

### Stake Reduction

```
VC_norm = min(TotalVC / 5000, 1.0)
StakeReduction = MaxReduction × VC_norm
RequiredStake = max(NominalStake × (1 - StakeReduction), StakeFloor)
```

| Phase | Floor | Max Reduction |
|-------|-------|---------------|
| Bootstrap | 50,000 KRAT | 99% |
| Post-Bootstrap | 25,000 KRAT | 95% |

---

## 6. Bootstrap Era

### Duration & Conditions

| Parameter | Value |
|-----------|-------|
| Duration | >= 1,440 epochs (60 days) |
| Exit requires | >= 50 validators |
| Exit requires | >= 90% avg participation (100 epochs) |

### Bootstrap Incentives

| Activity | Multiplier |
|----------|------------|
| Vote Credits | 2x |
| Uptime Credits | 2x |
| Arbitration | 1x |

**INVARIANT:** Network CANNOT exit bootstrap with < 50 validators.

---

## 7. Security States

### State Machine

```
Bootstrap → Normal → Degraded → Restricted → Emergency
    ↑                   ↓           ↓
    ←── BootstrapRecovery ←←←←←←←←←←
```

### State Effects

| State | Inflation | Governance | Special |
|-------|-----------|------------|---------|
| Bootstrap | 6.5% fixed | Normal | VC 2x multipliers |
| Normal | 0.5-10% adaptive | Normal | Full features |
| Degraded | +1% boost | Timelocks × 2 | Sidechains paused |
| Restricted | Max boost | Frozen | Emergency armed |
| Emergency | - | Frozen | Exit always allowed |

### Recovery Requirements

| Transition | Requirement |
|------------|-------------|
| → Normal | >= 75 validators for 100 epochs |
| Collapse detection | < 50 validators for 10 epochs |
| Bootstrap recovery | Re-enables bootstrap economics |

---

## 8. Sidechains

### Security Modes

| Mode | Validator Source | Deposit |
|------|-----------------|---------|
| Inherited | Copy from parent | 1,000 KRAT |
| Shared | Hostchain pool | Variable |
| Sovereign | Self-managed | 10,000 KRAT |

### Chain Limits

| Parameter | Value |
|-----------|-------|
| Max depth | 3 levels |
| Max affiliates/host | 50 |
| Min validators/chain | 3 |
| Inactivity threshold | 90 days |

### Exit Types

| Type | Timelock |
|------|----------|
| Dissolve | 30 days |
| Merge | 30 days |
| Reattach to Root | 30 days |
| Join Host | 7 days |
| Leave Host | 7 days |

---

## 9. Governance

### Voting Thresholds

| Type | Threshold |
|------|-----------|
| Standard proposals | 51% |
| Exit proposals | 66% (2/3 supermajority per Constitution) |
| Quorum | 30% |

### Timing

| Parameter | Duration |
|-----------|----------|
| Voting period | 7 days |
| Standard timelock | 12 days |
| Exit timelock | 30 days |
| Grace period | 2 days |

### Proposal Types (Standard)

- ParameterChange
- AddValidator / RemoveValidator
- TreasurySpend
- RequestAffiliation / LeaveHost
- Custom

### Proposal Types (Supermajority)

- ExitDissolve
- ExitMerge
- ExitReattachRoot
- ExitJoinHost

---

## 10. Slashing

### Severity Levels

| Severity | VC Slash | Stake Slash | Cooldown |
|----------|----------|-------------|----------|
| Critical | 50% | 5-20% | 52 epochs |
| High | 25% | 1-5% | 12 epochs |
| Medium | 10% | 0-1% | None |
| Low | 5% | 0% | None |

### Events

| Event | Severity |
|-------|----------|
| Double Signing | Critical |
| Equivocation | Critical |
| Arbitration Misconduct | High |
| Extended Downtime (>= 12 epochs) | Medium |
| Short Downtime (< 12 epochs) | Low |

---

## 11. Clock Health & Drift Tracking

### Incremental Drift Model

KratOs uses an **incremental drift model** that measures time drift between consecutive blocks rather than absolute drift from genesis. This design:

- Allows nodes to restart without immediate drift violations
- Detects time manipulation between consecutive blocks
- Maintains network synchronization over time

### Drift Constants

| Parameter | Value | Description |
|-----------|-------|-------------|
| MAX_SINGLE_BLOCK_DRIFT | 5 seconds | Maximum drift per block |
| MAX_CUMULATIVE_DRIFT_PER_EPOCH | 1,200 seconds | Total drift allowed per epoch |
| RESTART_GRACE_DRIFT | 3,600 seconds | Grace period for node restarts |

### Drift Calculation

```
slots_elapsed = block_slot - last_slot
expected_interval = slots_elapsed × slot_duration (6s)
actual_interval = block_timestamp - last_timestamp
incremental_drift = actual_interval - expected_interval
```

### Clock Health States

| State | Priority Modifier | Condition |
|-------|------------------|-----------|
| Healthy | 1.0 | Drift within tolerance |
| Degraded | 0.5 | Minor timing issues |
| Excluded | 0.0 | Severe drift, cannot produce blocks |
| Recovering | 0.0 | Returning from excluded state |

### State Transitions

```
Healthy → Degraded (minor drift detected)
Degraded → Healthy (3 consecutive good blocks)
Degraded → Excluded (continued drift)
Excluded → Recovering (drift resolved)
Recovering → Healthy (5 consecutive good blocks)
```

### Validator Priority

Clock health affects validator selection:

```
effective_weight = VRF_weight × priority_modifier
```

- **Healthy validators** are prioritized for block production
- **Degraded validators** have reduced selection probability
- **Excluded/Recovering validators** cannot produce blocks

### Restart Grace Period

When a node restarts after being offline:

1. First block after large time gap triggers grace mode
2. Grace period allows sync without drift penalty
3. Drift tracking resumes with next consecutive block

**INVARIANT:** A validator with clock issues can still participate once their clock synchronizes.

---

## 12. Contributor Roles & Treasury Programs

### Overview

Contributors are pseudonymous participants who receive compensation from the treasury for specific contributions. Only AccountId (Ed25519 public key) is stored on-chain - no personal data required.

### Treasury Programs

The treasury (20% of block emissions) funds 9 programs:

| Program | Budget % | Max Payment | Approval |
|---------|----------|-------------|----------|
| **Bug Bounty** | 20% | 100,000 KRAT | 51% |
| **Security Audit** | 15% | 50,000 KRAT | 67% |
| **Core Development** | 25% | 25,000 KRAT | 67% |
| **Content Creation** | 10% | 5,000 KRAT | 51% |
| **Ambassador** | 8% | 2,000 KRAT | 51% |
| **Research Grant** | 10% | 50,000 KRAT | 51% |
| **Infrastructure** | 5% | 10,000 KRAT | 51% |
| **Translation** | 4% | 1,000 KRAT | 51% |
| **Education** | 3% | 5,000 KRAT | 51% |

### Contributor Roles

| Role | Program | Description |
|------|---------|-------------|
| `BugHunter` | Bug Bounty | Security vulnerability reports |
| `SecurityAuditor` | Security Audit | Formal security reviews |
| `CoreDeveloper` | Core Development | Protocol development |
| `ContentCreator` | Content Creation | Docs, tutorials, marketing |
| `Ambassador` | Ambassador | Community outreach |
| `Researcher` | Research Grant | Academic/industry research |
| `InfrastructureProvider` | Infrastructure | Node operators, tooling |
| `Translator` | Translation | Localization |
| `Educator` | Education | Training, workshops |

### Role Lifecycle

```
Application → Pending → Active → Expired
                ↓          ↓
            Rejected   Suspended → Revoked
```

| Status | Can Receive Payment |
|--------|---------------------|
| **Pending** | No |
| **Active** | Yes |
| **Expired** | No (renewable) |
| **Suspended** | No |
| **Revoked** | No (permanent) |

### Application Process

| Parameter | Value |
|-----------|-------|
| Application Stake | 10 KRAT |
| Role Duration | 180 days |
| Grace Period | 14 days |
| Max Roles per Account | 5 |

### Bug Severity Levels

| Severity | Reward % | Max Payout |
|----------|----------|------------|
| Low | 5% | 5,000 KRAT |
| Medium | 20% | 20,000 KRAT |
| High | 50% | 50,000 KRAT |
| Critical | 100% | 100,000 KRAT |

### Anti-Abuse Mechanisms

1. **Stake requirement**: 10 KRAT deposit for applications
2. **Role limits**: Max 5 roles per account
3. **Cooldown**: 7 days between same-role applications
4. **Monthly cap**: 2x max payment per contributor per program

### Unified Role Registry

All network roles (Validator, Juror, Contributor) are tracked in a single `NetworkRoleRegistry` integrated within `ValidatorSet`:

```rust
pub struct ValidatorSet {
    pub validators: BTreeMap<AccountId, ValidatorInfo>,
    pub total_stake: Balance,
    pub role_registry: NetworkRoleRegistry,  // Unified tracking
}
```

**Automatic synchronization:**
- Validator registration → `role_registry.register_validator()`
- Validator removal → `role_registry.unregister_validator()`
- Stake updates → `role_registry.update_validator_stake()`

This provides a single source of truth for all network role assignments.

**INVARIANT:** Only accounts with active roles can receive treasury payments.

**INVARIANT:** Every validator in `ValidatorSet.validators` has a corresponding entry in `ValidatorSet.role_registry`.

---

## 13. Economic Projections

### 20-Year Supply

| Year | Supply (KRAT) | Net Change |
|------|---------------|------------|
| 0 | 1,000,000,000 | Initial |
| 1 | ~1,048,000,000 | +4.8% |
| 5 | ~1,082,000,000 | Peak |
| 10 | ~1,025,000,000 | -1.8%/year |
| 20 | ~824,000,000 | -2.6%/year |

### Burn Rate Growth

```
b(t) = b_max - (b_max - b_0) × e^(-g × t)
```

| Year | Burn Rate |
|------|-----------|
| 1 | ~1.3% |
| 5 | ~2.8% (crossover) |
| 10 | ~3.4% |
| 20 | ~3.5% (max) |

---

## 14. Key Invariants

### Security Invariants

1. Bootstrap exit requires >= 50 validators
2. Emergency triggers at < 25 validators
3. Exit is always possible regardless of state
4. Fork participation is never punished
5. VC = 0 means no block production rights

### Economic Invariants

1. Fee distribution: 60% validators, 30% burn, 10% treasury
2. Emission distribution: 70% validators, 20% treasury, 10% reserve
3. Minimum inflation: 0.5%
4. Maximum inflation: 10%
5. Maximum burn rate: 3.5%

### Governance Invariants

1. Exit proposals require 66% supermajority (2/3 per Constitution)
2. Only one active exit proposal per chain
3. Governance frozen in Restricted/Emergency states
4. Timelocks doubled in Degraded state

---

## 15. Peer Discovery - DNS Seeds

### Overview

KratOs implements decentralized peer discovery via **DNS Seeds**. When a node starts, it automatically discovers peers without manual configuration.

### Discovery Hierarchy

```
Node Startup
    │
    ▼
1. DNS Seeds (Primary)
    │ seed1.kratos.network → [IP list]
    │ seed2.kratos.community → [IP list]
    ▼
2. Hardcoded Bootnodes (Fallback)
    │ /ip4/X.X.X.X/tcp/30333/p2p/...
    ▼
3. mDNS (Local Network)
    │ Discover peers on same LAN
    │ Auto-dial discovered peers
    ▼
4. Kademlia DHT (Propagation)
    │ Learn peers from connected peers
    ▼
Connected to Network
```

### DNS Seed Requirements

| Requirement | Value |
|-------------|-------|
| Min independent operators | 3 |
| Geographic regions | >= 2 |
| Uptime SLA | 99.9% |
| Update frequency | <= 60 seconds |
| Max stale entries | 10% |

### Fallback Bootstrap Nodes

| Node | IP | Peer ID | Operator |
|------|-----|---------|----------|
| Foundation Node 1 | 78.240.168.225 | 12D3KooWEko82Ro... | KratOs Foundation |

### Configuration Constants

```rust
pub const DEFAULT_P2P_PORT: u16 = 30333;
pub const DNS_TIMEOUT_SECS: u64 = 10;
pub const MAX_DNS_PEERS: usize = 25;
```

### Security Considerations

| Threat | Mitigation |
|--------|------------|
| Poisoned seeds | Multiple independent sources |
| DNS hijacking | Fallback to hardcoded bootnodes |
| Eclipse attack | DHT propagation diversifies peers |
| Sybil seeds | Community vetting of operators |

### mDNS Local Discovery

mDNS enables automatic peer discovery on local networks without any configuration.

**Key Implementation Details:**

1. **Discovery**: mDNS broadcasts on local network to find KratOs peers
2. **Auto-dial**: Discovered peers are automatically dialed to establish connection
3. **Network Event Loop**: Main event loop polls network every 100ms to process mDNS events (v1.11+)
4. **No Bootnodes Required**: Nodes can join networks using mDNS alone (v1.10+)

**Critical: Network Polling (v1.11)**

The main event loop MUST poll the network regularly. Without polling:
- mDNS discovery/announcements are never processed
- Incoming connections are never accepted
- Genesis requests are never responded to
- Gossipsub messages are never propagated

```rust
// Main event loop (cli/runner.rs)
loop {
    tokio::select! {
        // Network polling every 100ms - CRITICAL for mDNS/genesis
        _ = network_poll_interval.tick() => {
            node.poll_network().await;           // Process swarm events
            while let Some(event) = node.next_network_event().await {
                node.process_network_event(event).await;
            }
        }
        // ... other event handlers
    }
}
```

```rust
// mDNS discovery handler (network/service.rs)
MdnsEvent::Discovered(list) => {
    for (peer_id, multiaddr) in list {
        // Add address to DHT
        swarm.behaviour_mut().add_address(peer_id, multiaddr.clone());

        // CRITICAL: Dial the peer to establish connection
        if !peer_manager.is_connected(&peer_id) {
            swarm.dial(multiaddr);
        }
    }
}
```

**Source:** `network/service.rs:472-486`, `network/peer.rs:291-294`, `cli/runner.rs:146-160`, `node/service.rs:1014-1036`

### Becoming a Seed Operator

1. Run a DNS seed server (see implementation)
2. Maintain 99.9% uptime for 30 days
3. Submit PR to add seed to official list
4. Pass community review for independence

**Source:** `network/dns_seeds.rs`

---

## 16. Genesis Exchange Protocol

### Overview

When a new node joins the KratOs network, it must receive the **canonical genesis block** from existing peers before initializing its chain state. This ensures all nodes on the network share the same genesis hash.

### Problem Solved

Without genesis exchange:
- Each node would generate its own genesis block locally
- Different genesis hashes → incompatible chains
- Nodes cannot sync because they see each other as "different networks"

### Protocol Messages

| Message | Direction | Purpose |
|---------|-----------|---------|
| `GenesisRequest` | Joining → Existing | Request genesis info from peer |
| `GenesisResponse` | Existing → Joining | Send genesis block and chain info |

**Request:**
```rust
pub struct GenesisRequest {
    pub protocol_version: u32,  // For compatibility check
}
```

**Response:**
```rust
pub struct GenesisResponse {
    pub genesis_hash: Hash,        // Canonical chain identifier
    pub genesis_block: Block,      // Full genesis block for validation
    pub chain_name: String,        // Chain name for verification
    pub protocol_version: u32,     // Protocol version
}
```

### Startup Sequence

```
┌─────────────────────────────────────────────────────────────────┐
│                     Node Startup Flow                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   ┌─────────────────┐                                           │
│   │   CLI Parsed    │                                           │
│   └────────┬────────┘                                           │
│            │                                                     │
│            ▼                                                     │
│   ┌────────────────────┐                                        │
│   │  --genesis flag?   │                                        │
│   └────────┬───────────┘                                        │
│            │                                                     │
│     ┌──────┴──────┐                                             │
│     │             │                                              │
│     ▼             ▼                                              │
│   [YES]         [NO]                                            │
│     │             │                                              │
│     ▼             ▼                                              │
│ ┌────────┐   ┌──────────────┐                                   │
│ │ Create │   │ Check DB for │                                   │
│ │ Genesis│   │ existing     │                                   │
│ │ Locally│   │ genesis      │                                   │
│ └───┬────┘   └──────┬───────┘                                   │
│     │               │                                            │
│     │        ┌──────┴──────┐                                    │
│     │        │             │                                     │
│     │        ▼             ▼                                     │
│     │      [Found]     [Not Found]                              │
│     │        │             │                                     │
│     │        ▼             ▼                                     │
│     │   ┌─────────┐   ┌──────────────┐                          │
│     │   │ Use     │   │ Connect to   │                          │
│     │   │ stored  │   │ network FIRST│                          │
│     │   │ genesis │   └──────┬───────┘                          │
│     │   └────┬────┘          │                                  │
│     │        │               ▼                                   │
│     │        │        ┌──────────────┐                          │
│     │        │        │ Send Genesis │                          │
│     │        │        │ Request      │                          │
│     │        │        └──────┬───────┘                          │
│     │        │               │                                   │
│     │        │               ▼                                   │
│     │        │        ┌──────────────┐                          │
│     │        │        │ Wait for     │                          │
│     │        │        │ GenesisResp  │                          │
│     │        │        │ (30s timeout)│                          │
│     │        │        └──────┬───────┘                          │
│     │        │               │                                   │
│     │        │               ▼                                   │
│     │        │        ┌──────────────┐                          │
│     │        │        │ Initialize   │                          │
│     │        │        │ with received│                          │
│     │        │        │ genesis      │                          │
│     │        │        └──────┬───────┘                          │
│     │        │               │                                   │
│     ▼        ▼               ▼                                   │
│ ┌────────────────────────────────────┐                          │
│ │       Node Running                 │                          │
│ │  (Serve genesis to new peers)      │                          │
│ └────────────────────────────────────┘                          │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Mode Behavior

| Mode | Genesis Source | Serves Genesis | Connects First |
|------|---------------|----------------|----------------|
| `--genesis` | Created locally | Yes | No |
| Join (existing DB) | Loaded from DB | Yes | Yes |
| Join (fresh) | Received from peer | Yes (after init) | Yes |

### Security Considerations

| Threat | Mitigation |
|--------|------------|
| False genesis | Validate genesis block structure and rules |
| Genesis mismatch | Hash comparison with multiple peers |
| Request flood | Rate limiting on genesis requests |
| Man-in-middle | Multiple peer validation recommended |

### Protocol Identifier

```rust
pub const GENESIS_PROTOCOL: &str = "/kratos/genesis/1.0.0";
```

### Implementation Files

| File | Purpose |
|------|---------|
| `network/request.rs` | GenesisRequest/GenesisResponse types |
| `network/protocol.rs` | NetworkMessage::GenesisRequest/Response |
| `network/service.rs` | Genesis serving and requesting logic |
| `node/service.rs` | Startup sequence orchestration |

---

## 17. Implementation Reference

### Source Structure

```
rust/kratos-core/src/
├── consensus/
│   ├── clock_health.rs   # Clock status & drift tracking
│   ├── economics.rs      # Bootstrap, security states
│   ├── epoch.rs          # Time management
│   ├── pos.rs            # Validator selection
│   ├── slashing.rs       # Slashing logic
│   ├── validator.rs      # Validator management
│   ├── validator_credits.rs  # VC accumulation
│   └── vrf_selection.rs  # VRF weighting
├── contracts/
│   ├── krat.rs           # Token economics
│   ├── staking.rs        # Stake management
│   ├── governance.rs     # Proposals & voting
│   └── sidechains.rs     # Chain registry
├── network/
│   ├── dns_seeds.rs      # DNS seed resolver & registry
│   ├── service.rs        # P2P networking
│   └── sync.rs           # Chain synchronization
├── types/
│   ├── block.rs          # Block structure
│   ├── transaction.rs    # Transaction types
│   ├── chain.rs          # Chain types
│   └── contributor.rs    # Contributor roles & treasury
├── node/
│   ├── producer.rs       # Block production
│   └── service.rs        # Node lifecycle
└── cli/
    ├── mod.rs            # CLI commands
    └── config.rs         # Node configuration
```

### Running a Node

```bash
# Build
cargo build --release

# Run as validator
./target/release/kratos-node run --validator --validator-key <path>

# Run as full node
./target/release/kratos-node run

# Generate key
./target/release/kratos-node key generate --output validator.json
```

---

## 18. Specification Cross-Reference

| Topic | Primary SPEC | Related SPECs |
|-------|-------------|---------------|
| Token supply, inflation | SPEC 1 | 2, 3, 6, 7 |
| Validator reputation | SPEC 2 | 1, 3, 5 |
| Block production | SPEC 3 | 1, 2, 6 |
| Chain hierarchy | SPEC 4 | 1, 5, 6 |
| Proposals & voting | SPEC 5 | 1, 2, 4, 6, 7 |
| Security states | SPEC 6 | 1, 2, 5 |
| Contributor roles | SPEC 7 | 1, 5 |
| Clock health & drift | Synthesis §11 | 3, 6 |
| Peer discovery | SPEC 6 §12 | Synthesis §15 |
| Genesis exchange | Synthesis §16 | Synthesis §15, 17 |

---

## 19. Additional Implementation Details

The following mechanics are implemented in code and documented here for completeness:

### Validator Reputation System

Validators have a reputation score (0-100) affecting participation eligibility:

| Action | Reputation Change |
|--------|-------------------|
| Block produced | +1 (max 100) |
| Block missed | -1 |
| Slash event | -20 |

**Participation requires:** `reputation > 0`

**Source:** `consensus/validator.rs:51-52, 188-204`

### Transaction Mechanics

#### Replace-by-Fee (RBF)

Mempool supports transaction replacement with minimum 10% fee increase:
```
new_fee >= old_fee × 1.10
```

**Source:** `node/mempool.rs:511-542`

#### Nonce Gap Detection

Maximum allowed nonce gap: 2 transactions ahead of expected nonce.

**Source:** `node/mempool.rs:25`

### VRF Selection Details

#### Cold-Start Fix

New validators with VC=0 are assigned `MIN_EFFECTIVE_VC = 1` to ensure non-zero selection weight:
```
effective_vc = validator_credits.max(1)
vc_component = ln(1 + effective_vc)
```

**Source:** `consensus/vrf_selection.rs:18, 70-72`

#### Bootstrap Validator Requirements

| Parameter | Value |
|-----------|-------|
| BOOTSTRAP_MIN_VC_REQUIREMENT | 5 |
| BOOTSTRAP_STAKE_COMPONENT | 10.0 |

**Source:** `consensus/vrf_selection.rs:25-30`

### Slashing Mechanics

#### Critical Event Decay

Critical violation count decays over time:
- Decay interval: 26 epochs (~4.3 days)
- After 26 epochs without new critical event: count -= 1

**Source:** `consensus/slashing.rs:389-416`

### Sidechain Purge Timing

| Parameter | Value |
|-----------|-------|
| Purge check interval | 3,600 blocks (6 hours) |
| Fraud proof validity | 432,000 blocks (30 days) |

**Source:** `contracts/sidechains.rs:164-167, 810-814`

### Dispute Resolution Timing

| Parameter | Value |
|-----------|-------|
| Max dispute duration | 835,200 blocks (58 days) |
| Evidence window | 100,800 blocks (7 days) |
| Deliberation period | 201,600 blocks (14 days) |
| Appeal window | 432,000 blocks (30 days) |

**Undocumented in SPEC 6:** 58-day maximum enforced automatically.

**Source:** `types/dispute.rs:489`

### Domain Separation

Signature domains prevent cross-context signature reuse:

| Domain | Prefix | Usage |
|--------|--------|-------|
| DOMAIN_TRANSACTION | `KRATOS_TX_` | Transaction signatures |
| DOMAIN_BLOCK_HEADER | `KRATOS_BLOCK_` | Block header signatures |

**Source:** `types/signature.rs:15-18`

---

## 19. Known Issues & Technical Debt

The following issues were identified during security audit (2025-12-19) and have been fixed:

### Security Fixes Applied

| Issue | Location | Status |
|-------|----------|--------|
| Governance threshold 50% vs 51% | governance.rs:14 | **FIXED** (51%) |
| Supermajority threshold 66% per Constitution | governance.rs:13 | **FIXED** (66%) |
| MIN_VALIDATOR_STAKE 10k vs 50k | validator.rs:8 | **FIXED** (50k) |
| Bootstrap MIN_VC_REQUIREMENT too low | vrf_selection.rs:30 | **FIXED** (100 VC) |
| Missing block domain separation | block.rs:86-88 | **FIXED** |
| Missing finality justification verification | block.rs | **FIXED** |
| Missing VC slashing | slashing.rs | **PENDING** |
| Missing nonce validation in blocks | validation.rs:291 | **PENDING** |

### Security Audit Reference

Full audit report: [SECURITY_AUDIT_REPORT.md](../SECURITY_AUDIT_REPORT.md)

---

## 20. Document History

| Date | Version | Change |
|------|---------|--------|
| 2025-12-19 | 1.11 | **Network Event Loop Fix**: Main event loop (`run_event_loop`) now polls network every 100ms - fixes genesis node not responding to mDNS/genesis requests |
| 2025-12-19 | 1.10 | **mDNS-Only Discovery**: Allow joining networks via mDNS alone without requiring bootnodes - removed early return that blocked mDNS discovery |
| 2025-12-19 | 1.9 | **mDNS Fix**: Auto-dial discovered peers, swarm polling during genesis exchange, added `poll_once()` for non-blocking network processing |
| 2025-12-19 | 1.8 | Added §16 Genesis Exchange Protocol: joining nodes receive genesis from network before initialization |
| 2025-12-19 | 1.7 | Added §15 Peer Discovery - DNS Seeds: decentralized peer discovery, bootstrap nodes, security mitigations |
| 2025-12-19 | 1.6 | Security fixes: governance thresholds (51%/66%), MIN_VALIDATOR_STAKE (50k), domain separation, finality verification, bootstrap VC (100) |
| 2025-12-19 | 1.5 | Added §18 (Additional Implementation Details), §19 (Known Issues) |
| 2025-12-19 | 1.4 | Unified Role Registry: ValidatorSet now integrates NetworkRoleRegistry |
| 2025-12-19 | 1.3 | Added Contributor Roles & Treasury Programs (§12) |
| 2025-12-19 | 1.2 | Added Repository Strategy (dual-repo model) |
| 2025-12-19 | 1.1 | Added Clock Health & Drift Tracking section |
| 2025-12-19 | 1.0 | Initial synthesis - unified architecture |
