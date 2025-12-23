# KratOs Blockchain Data Model

## Overview

This document describes all data types that can be stored on the KratOs blockchain, including transactions, governance proposals, disputes, and state structures.

**Implementation**: Native Rust (not Substrate-based)
**Source Code**: `rust/kratos-core/src/types/`, `rust/kratos-core/src/contracts/`

---

## Block Structure

Each block contains a header and body:

```
Block
├── BlockHeader
│   ├── number          (block height)
│   ├── parent_hash     (previous block hash)
│   ├── transactions_root (Merkle root of transactions)
│   ├── state_root      (Merkle root of state after execution)
│   ├── timestamp       (Unix timestamp)
│   ├── epoch           (epoch number)
│   ├── slot            (slot within epoch)
│   ├── author          (validator who produced the block)
│   └── signature       (Ed25519 signature)
└── BlockBody
    └── transactions: Vec<SignedTransaction>
```

**Source**: `rust/kratos-core/src/types/block.rs`

### Finality

Blocks are finalized with `FinalityJustification` containing >= 2/3 validator signatures.

---

## Transactions (9 Types)

All user actions are submitted as signed transactions.

**Source**: `rust/kratos-core/src/types/transaction.rs`

### TransactionCall Enum

| Type | Parameters | Description | Base Fee |
|------|------------|-------------|----------|
| **Transfer** | `to: AccountId, amount: Balance` | Simple KRAT transfer | 0.000001 KRAT |
| **Stake** | `amount: Balance` | Lock tokens for staking | 0.000005 KRAT |
| **Unstake** | `amount: Balance` | Begin unbonding (28-day period) | 0.000005 KRAT |
| **WithdrawUnbonded** | - | Claim tokens after unbonding | 0.000002 KRAT |
| **RegisterValidator** | `stake: Balance` | Register as validator | 0.0001 KRAT |
| **UnregisterValidator** | - | Deregister as validator | 0.00005 KRAT |
| **CreateSidechain** | `metadata, deposit` | Create new sidechain | 0.001 KRAT |
| **ExitSidechain** | `chain_id: ChainId` | Exit from a sidechain | 0.0005 KRAT |
| **SignalFork** | `name, description` | Signal fork for migration | 0.01 KRAT |

### Transaction Structure

```rust
pub struct SignedTransaction {
    pub transaction: Transaction,
    pub signature: Signature64,   // Ed25519 (64 bytes)
    pub hash: Option<Hash>,
}

pub struct Transaction {
    pub sender: AccountId,
    pub nonce: Nonce,             // Anti-replay
    pub call: TransactionCall,
    pub timestamp: u64,
}
```

---

## Governance Proposals (11 Types)

Validators can submit governance proposals for chain management.

**Source**: `rust/kratos-core/src/contracts/governance.rs`

### ProposalType Enum

| Type | Parameters | Description | Threshold |
|------|------------|-------------|-----------|
| **ParameterChange** | `parameter, old_value, new_value` | Modify chain parameter | 51% |
| **AddValidator** | `validator: AccountId` | Add validator to chain | 51% |
| **RemoveValidator** | `validator: AccountId` | Remove validator | 51% |
| **ExitDissolve** | - | Dissolve sidechain completely | 66% (2/3) |
| **ExitMerge** | `target_chain: ChainId` | Merge into another sidechain | 66% (2/3) |
| **ExitReattachRoot** | - | Reattach to root chain | 66% (2/3) |
| **ExitJoinHost** | `host_chain: ChainId` | Join a host chain | 66% (2/3) |
| **LeaveHost** | - | Leave current host chain | 51% |
| **RequestAffiliation** | `host_chain: ChainId` | Request affiliation | 51% |
| **TreasurySpend** | `recipient, amount, reason` | Spend from treasury | 51% |
| **Custom** | `title, description, data` | Custom proposal | 51% |

### Proposal Status Flow

```
Active → Passed → ReadyToExecute → Executed
   ↓        ↓                          ↓
Rejected  Cancelled                  Expired
```

| Status | Description |
|--------|-------------|
| **Active** | Voting is ongoing |
| **Passed** | Voting passed, in timelock |
| **Rejected** | Failed to reach threshold |
| **ReadyToExecute** | Timelock complete, awaiting execution |
| **Executed** | Proposal executed |
| **Cancelled** | Proposal cancelled |
| **Expired** | Not executed within grace period |

### Vote Options

| Vote | Description |
|------|-------------|
| **Yes** | Support the proposal |
| **No** | Oppose the proposal |
| **Abstain** | No position |

### Timing Constants

| Parameter | Value | Description |
|-----------|-------|-------------|
| Voting Period | 7 days (100,800 blocks) | Duration for voting |
| Standard Timelock | 2 days (28,800 blocks) | Regular proposals |
| Exit Timelock | 30 days | Exit proposals |
| Grace Period | 2 days (28,800 blocks) | Execution window |

---

## Disputes & Arbitration (4 Types)

Cross-chain arbitration system for resolving conflicts.

**Source**: `rust/kratos-core/src/types/dispute.rs`

### DisputeType Enum

| Type | Description | Jurisdiction |
|------|-------------|--------------|
| **ValidatorMisconduct** | Double signing, invalid blocks | Root Chain |
| **CrossChainTreatyViolation** | Violation of inter-chain treaties | Host Chain |
| **FraudulentExit** | Fraudulent exit attempt | Host/Root Chain |
| **StateRootDispute** | Conflicting state roots | Depends on chains |

### Dispute Status Flow

```
Open → EvidenceComplete → Deliberating → Resolved
  ↓          ↓                 ↓            ↓
Expired   Dismissed        Expired      Appealed
```

| Status | Description |
|--------|-------------|
| **Open** | Awaiting evidence submission |
| **EvidenceComplete** | Evidence submitted, awaiting jury |
| **Deliberating** | Jury selected, voting in progress |
| **Resolved** | Jury reached verdict |
| **Appealed** | Appealed to higher jurisdiction |
| **Dismissed** | Insufficient evidence |
| **Expired** | Exceeded maximum duration |

### Evidence Types

| Type | Description |
|------|-------------|
| **FraudProof** | Cryptographic proof of fraud |
| **StateProof** | Merkle proof of state |
| **BlockHeaders** | Block headers as evidence |
| **TextEvidence** | General textual evidence |

### Jury Verdicts

| Verdict | Description |
|---------|-------------|
| **Guilty** | Accused is guilty |
| **NotGuilty** | Accused is not guilty |
| **Abstain** | Juror abstains |

### Enforcement Actions

| Action | Description |
|--------|-------------|
| **SlashValidator** | Slash validator stake |
| **SlashValidatorCredits** | Reduce VC |
| **ForceExit** | Force chain into purge |
| **InvalidateStateRoot** | Mark state root invalid |
| **SlashAccuser** | Slash false accuser |
| **None** | No action (not guilty) |

### Timing Constants

| Parameter | Value | Description |
|-----------|-------|-------------|
| Evidence Submission | 7 days (100,800 blocks) | Window for evidence |
| Deliberation Period | 14 days (201,600 blocks) | Jury voting period |
| Appeal Window | 30 days (432,000 blocks) | Window to appeal |
| Max Dispute Duration | 58 days (835,200 blocks) | Absolute deadline |

### Jury Configuration

| Parameter | Value |
|-----------|-------|
| Minimum Jury Size | 7 |
| Maximum Jury Size | 21 |
| Default Jury Size | 13 |
| Max Evidence Count | 50 |

---

## State Storage

### Account State

| Field | Type | Description |
|-------|------|-------------|
| `balance` | Balance | Available KRAT |
| `nonce` | u64 | Transaction counter |
| `staked` | Balance | Locked for staking |
| `unbonding` | Balance | In unbonding period |

### Validator State

| Field | Type | Description |
|-------|------|-------------|
| `public_key` | AccountId | Validator identity |
| `stake` | Balance | Total staked |
| `validator_credits` | u32 | VC accumulated |
| `status` | ValidatorStatus | Active/Inactive/Jailed |
| `uptime_records` | Vec | Participation history |

### Chain State (Sidechains)

| Field | Type | Description |
|-------|------|-------------|
| `chain_id` | ChainId | Unique identifier |
| `name` | String | Chain name |
| `parent_chain` | Option<ChainId> | Parent chain ID |
| `validators` | Vec<AccountId> | Validator set |
| `state_root` | Hash | Current state root |

---

## Network Roles

All on-chain roles are unified under the `NetworkRole` enum and tracked in a single `NetworkRoleRegistry`.

**Source**: `rust/kratos-core/src/types/contributor.rs`, `rust/kratos-core/src/consensus/validator.rs`

### NetworkRole Enum

| Role | Description | Registration Method |
|------|-------------|---------------------|
| **Validator** | Block production, consensus | `RegisterValidator` transaction |
| **Juror** | Arbitration participation | Automatic (VC-based) |
| **Contributor(role)** | Treasury-funded programs | Governance proposal |

```rust
pub enum NetworkRole {
    Validator,           // Consensus role
    Juror,              // Arbitration role
    Contributor(ContributorRole),  // Treasury-funded
}
```

### Unified Role Registry

The `NetworkRoleRegistry` is integrated within `ValidatorSet` for unified role management:

```rust
pub struct ValidatorSet {
    pub validators: BTreeMap<AccountId, ValidatorInfo>,
    pub total_stake: Balance,
    pub role_registry: NetworkRoleRegistry,  // Unified tracking
}
```

**Automatic synchronization:**
- `add_validator()` → `role_registry.register_validator()`
- `remove_validator()` → `role_registry.unregister_validator()`
- `update_validator_stake()` → `role_registry.update_validator_stake()`

---

## Contributor Roles & Treasury Programs

On-chain contributor status for treasury-funded programs.

### Treasury Programs

| Program | Budget % | Max Payment | Approval |
|---------|----------|-------------|----------|
| **BugBounty** | 20% | 100,000 KRAT | 51% |
| **SecurityAudit** | 15% | 50,000 KRAT | 67% |
| **CoreDevelopment** | 25% | 25,000 KRAT | 67% |
| **ContentCreation** | 10% | 5,000 KRAT | 51% |
| **Ambassador** | 8% | 2,000 KRAT | 51% |
| **ResearchGrant** | 10% | 50,000 KRAT | 51% |
| **Infrastructure** | 5% | 10,000 KRAT | 51% |
| **Translation** | 4% | 1,000 KRAT | 51% |
| **Education** | 3% | 5,000 KRAT | 51% |

### Contributor Roles

| Role | Program | Description |
|------|---------|-------------|
| `BugHunter` | BugBounty | Security vulnerability reports |
| `SecurityAuditor` | SecurityAudit | Formal security reviews |
| `CoreDeveloper` | CoreDevelopment | Protocol development |
| `ContentCreator` | ContentCreation | Docs, tutorials, marketing |
| `Ambassador` | Ambassador | Community outreach |
| `Researcher` | ResearchGrant | Academic/industry research |
| `InfrastructureProvider` | Infrastructure | Node operators, tooling |
| `Translator` | Translation | Localization |
| `Educator` | Education | Training, workshops |

### Role Status Flow

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

### Role Registration

```rust
pub struct RoleRegistration {
    pub registration_id: Hash,
    pub account: AccountId,        // Pseudonymous
    pub role: ContributorRole,
    pub status: RoleStatus,
    pub granted_at: BlockNumber,
    pub expires_at: BlockNumber,   // 180 days default
    pub total_payments: Balance,
    pub contribution_count: u32,
    pub alias: Option<String>,     // Optional public name
}
```

### Contribution Claims

```rust
pub struct ContributionClaim {
    pub claim_id: Hash,
    pub contributor: AccountId,
    pub program: TreasuryProgram,
    pub amount: Balance,
    pub evidence_hash: Hash,       // Off-chain proof
    pub severity: Option<BugSeverity>, // For bug bounty
    pub status: ClaimStatus,
}
```

### Bug Severity Levels

| Severity | Reward % | Max Payout |
|----------|----------|------------|
| **Low** | 5% | 5,000 KRAT |
| **Medium** | 20% | 20,000 KRAT |
| **High** | 50% | 50,000 KRAT |
| **Critical** | 100% | 100,000 KRAT |

### Timing Constants

| Parameter | Value | Description |
|-----------|-------|-------------|
| Role Duration | 180 days (~2,592,000 blocks) | Default validity |
| Grace Period | 14 days (~201,600 blocks) | Renewal window |
| Application Stake | 10 KRAT | Anti-spam deposit |
| Max Roles | 5 | Per account limit |

---

## Network Security States

The network operates in different modes based on validator count:

| State | Validator Range | Effects |
|-------|-----------------|---------|
| **Bootstrap** | Any (epoch < 1440) | Fixed 6.5% inflation |
| **Normal** | >= 75 | Full functionality |
| **Degraded** | 50-74 | Inflation +1%, timelocks x2 |
| **Restricted** | 25-49 | Governance frozen |
| **Emergency** | < 25 | Fork allowed |

---

## Source Files Reference

| File | Contents |
|------|----------|
| [transaction.rs](../../rust/kratos-core/src/types/transaction.rs) | Transaction types |
| [block.rs](../../rust/kratos-core/src/types/block.rs) | Block structure |
| [governance.rs](../../rust/kratos-core/src/contracts/governance.rs) | Governance proposals |
| [dispute.rs](../../rust/kratos-core/src/types/dispute.rs) | Dispute arbitration |
| [chain.rs](../../rust/kratos-core/src/types/chain.rs) | Sidechain types |
| [account.rs](../../rust/kratos-core/src/types/account.rs) | Account types |
| [contributor.rs](../../rust/kratos-core/src/types/contributor.rs) | NetworkRole, NetworkRoleRegistry, ContributorRole |
| [validator.rs](../../rust/kratos-core/src/consensus/validator.rs) | ValidatorSet with integrated NetworkRoleRegistry |
| [identity.rs](../../rust/kratos-core/src/types/identity.rs) | Identity & pseudonymity system |

---

**Implementation Status**: Complete
**Last Updated**: 2025-12-19
**Specification Version**: Unified (see [KRATOS_SYNTHESIS.md](KRATOS_SYNTHESIS.md))
