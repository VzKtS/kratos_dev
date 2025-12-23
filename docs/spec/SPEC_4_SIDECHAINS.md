# SPEC 4: Sidechains and Hostchains

**Version:** 1.0
**Status:** Normative
**Last Updated:** 2025-12-19

---

## 1. Overview

KratOs implements a hierarchical sidechain architecture allowing parallel chains with varying security models, enabling scalability while maintaining root chain security guarantees.

**Key Concepts:**
- **Sidechain:** Parallel chain attached to root or another chain
- **Hostchain:** Chain providing shared validator pool to multiple sidechains
- **Federation:** Hierarchy of affiliated chains with shared security

---

## 2. Chain Hierarchy

### 2.1 Chain Types

| Type | Description | Maximum Validators |
|------|-------------|-------------------|
| Root Chain | Primary KratOs chain | 101 |
| Sidechain | Parallel chain attached to parent | 100 |
| Hostchain | Chain with shared validator pool | 200 (aggregate) |
| Child Chain | Sidechain of a sidechain | 100 |

### 2.2 Hierarchy Structure

```
Root Chain (L1)
├── Sidechain A (L2)
│   ├── Child A1 (L3)
│   └── Child A2 (L3)
├── Sidechain B (L2)
└── Hostchain H (L2)
    ├── Affiliate S1
    └── Affiliate S2
```

---

## 3. Security Modes

### 3.1 Available Modes

| Mode | Validator Source | Deposit | Use Case |
|------|-----------------|---------|----------|
| Inherited | Copy from parent | BASE_DEPOSIT | Child chains |
| Shared | From hostchain pool | Variable | Affiliated chains |
| Sovereign | Self-managed | SOVEREIGN_DEPOSIT | Independent chains |

### 3.2 Deposit Requirements

| Mode | Base Deposit | Calculation |
|------|--------------|-------------|
| Inherited | 1,000 KRAT | Fixed |
| Shared | Variable | Based on hostchain size |
| Sovereign | 10,000 KRAT | Fixed |

```rust
pub fn calculate_deposit(mode: SecurityMode, host_members: usize) -> Balance {
    match mode {
        Inherited => BASE_DEPOSIT,
        Shared => BASE_DEPOSIT + (host_members as Balance * MEMBER_DEPOSIT_INCREMENT),
        Sovereign => SOVEREIGN_DEPOSIT,
    }
}
```

---

## 4. Sidechain Lifecycle

### 4.1 Status States

| Status | Description |
|--------|-------------|
| Active | Normal operation |
| Frozen | Governance/emergency freeze |
| Purged | Inactive, awaiting cleanup |

### 4.2 Creation

```
CreateSidechain {
    owner: AccountId,
    name: Option<String>,
    description: Option<String>,
    parent: Option<ChainId>,
    security_mode: SecurityMode,
    host_id: Option<ChainId>,  // For Shared mode
    deposit: Balance,
}
```

### 4.3 Activity Tracking

- Last activity recorded on each block production
- Inactivity threshold: 90 days (1,296,000 blocks)
- Purge check interval: 6 hours (3,600 blocks)

### 4.4 Auto-Purge

Inactive chains are automatically purged:

```
if current_block - last_activity >= INACTIVITY_THRESHOLD {
    chain.status = Purged;
    deposit → owner;
}
```

---

## 5. Hostchain Operations

### 5.1 Creation

```
CreateHostchain {
    creator: AccountId,
    initial_validators: Vec<AccountId>,
}
```

### 5.2 Validator Pool

Hostchains maintain a shared validator pool:

- Validators added/removed via governance
- Pool distributed to affiliated sidechains
- Maximum aggregate: 200 validators

### 5.3 Affiliation

Sidechains can affiliate with hostchains:

```
affiliate_sidechain(sidechain_id, host_id) {
    // Add sidechain to host's member list
    // Assign host's validators to sidechain
    // Update sidechain_to_host mapping
}
```

---

## 6. Validator Assignment

### 6.1 By Security Mode

| Mode | Validator Source | Update Trigger |
|------|-----------------|----------------|
| Inherited | Parent chain | Parent validator changes |
| Shared | Hostchain pool | Pool changes |
| Sovereign | Self-managed | Governance only |

### 6.2 Inherited Mode

```
parent_validators = get_chain(parent_id).validators;
child.validators = parent_validators.clone();
```

### 6.3 Shared Mode

```
host_pool = get_host(host_id).validator_pool;
sidechain.validators = host_pool.clone();
```

### 6.4 Sovereign Mode

```
// Only governance can modify validators
add_validator_to_chain(chain_id, validator);
remove_validator_from_chain(chain_id, validator);
```

---

## 7. Exit Mechanisms

### 7.1 Exit Types

| Type | Description | Timelock |
|------|-------------|----------|
| ExitDissolve | Dissolve chain completely | 30 days |
| ExitMerge | Merge into another chain | 30 days |
| ExitReattachRoot | Reattach to root chain | 30 days |
| ExitJoinHost | Join a hostchain | 7 days |
| LeaveHost | Leave current hostchain | 7 days |

### 7.2 Exit Process

1. **Proposal:** Governance proposal submitted
2. **Voting:** 67% threshold for exit proposals
3. **Timelock:** 30-day preparation period
4. **Withdrawal Window:** Users withdraw assets
5. **Execution:** Chain state transition

### 7.3 Asset Withdrawal

During exit preparation:
- All assets freely withdrawable
- No new deposits accepted
- Governance frozen

---

## 8. Cross-Chain Communication

### 8.1 State Root Anchoring

Sidechains anchor state roots to parent chain:

```rust
pub struct StateRootAnchor {
    sidechain_id: ChainId,
    block_number: BlockNumber,
    state_root: Hash,
    signatures: Vec<ValidatorSignature>,
}
```

### 8.2 Anchoring Frequency

| Mode | Anchoring Interval |
|------|-------------------|
| Inherited | Every epoch |
| Shared | Every 10 epochs |
| Sovereign | On-demand |

---

## 9. Limits and Constraints

### 9.1 Validator Limits

| Chain Type | Maximum | Constant |
|------------|---------|----------|
| Root Chain | 101 | MAX_VALIDATORS |
| Sidechain | 100 | MAX_VALIDATORS_PER_CHAIN |
| Hostchain | 200 | MAX_VALIDATORS_PER_HOST |
| Network | 1,000 | max_validators |

### 9.2 Chain Limits

| Parameter | Value |
|-----------|-------|
| Max depth | 3 levels (L1→L2→L3) |
| Max affiliates per host | 50 |
| Min validators per chain | 3 |

---

## 10. Implementation

### 10.1 Source Files

| File | Contents |
|------|----------|
| `contracts/sidechains.rs` | Chain registry |
| `types/chain.rs` | Chain types |
| `contracts/governance.rs` | Exit proposals |

### 10.2 Key Structures

```rust
pub struct ChainRegistry {
    sidechains: HashMap<ChainId, SidechainInfo>,
    hostchains: HashMap<ChainId, HostChainInfo>,
    sidechain_to_host: HashMap<ChainId, ChainId>,
    next_chain_id: u32,
}

pub struct SidechainInfo {
    id: ChainId,
    parent: Option<ChainId>,
    owner: AccountId,
    status: ChainStatus,
    security_mode: SecurityMode,
    validators: HashSet<AccountId>,
    deposit: Balance,
    last_activity: BlockNumber,
}
```

---

## 11. Constants

```rust
// Inactivity threshold: 90 days
pub const INACTIVITY_THRESHOLD: BlockNumber = 1_296_000;

// Purge check interval: 6 hours
pub const PURGE_CHECK_INTERVAL: BlockNumber = 3_600;

// Deposits
pub const BASE_DEPOSIT: Balance = 1_000 * KRAT;
pub const SOVEREIGN_DEPOSIT: Balance = 10_000 * KRAT;
pub const MEMBER_DEPOSIT_INCREMENT: Balance = 100 * KRAT;
```

---

## 12. Related Specifications

- **SPEC 1:** Tokenomics - Validator limits, staking
- **SPEC 5:** Governance - Exit proposals
- **SPEC 6:** Network Security - Chain security states
