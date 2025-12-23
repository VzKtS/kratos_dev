# SPEC 5: Governance

**Version:** 1.0
**Status:** Normative
**Last Updated:** 2025-12-19

---

## 1. Overview

KratOs implements on-chain governance for validators to make collective decisions about chain parameters, validator management, and voluntary exits.

**Principles:**
- **Stake-weighted voting:** Voting power proportional to stake
- **Quorum requirements:** Minimum participation for validity
- **Timelocks:** Delay between approval and execution
- **Supermajority:** Higher thresholds for critical decisions

---

## 2. Proposal Types

### 2.1 Standard Proposals (51% threshold)

| Type | Parameters | Description |
|------|------------|-------------|
| ParameterChange | parameter, old_value, new_value | Modify chain parameter |
| AddValidator | validator | Add validator to chain |
| RemoveValidator | validator | Remove validator from chain |
| LeaveHost | - | Leave current hostchain |
| RequestAffiliation | host_chain | Request hostchain affiliation |
| TreasurySpend | recipient, amount, reason | Spend from treasury |
| Custom | title, description, data | Custom proposal |

### 2.2 Exit Proposals (66% supermajority)

Per Genesis Constitution Article III: "2/3 supermajority" = floor(66.67%) = 66%

| Type | Parameters | Description |
|------|------------|-------------|
| ExitDissolve | - | Dissolve sidechain completely |
| ExitMerge | target_chain | Merge into another chain |
| ExitReattachRoot | - | Reattach to root chain |
| ExitJoinHost | host_chain | Join a hostchain |

---

## 3. Voting Thresholds

| Category | Threshold | Use Case |
|----------|-----------|----------|
| Standard | 51% | Regular proposals (true majority, not tie) |
| Supermajority | 66% | Exit proposals (2/3 per Constitution) |
| Quorum | 30% | Minimum participation |

### 3.1 Approval Calculation

```
approval = yes_votes / (yes_votes + no_votes) × 100
```

Abstain votes count toward quorum but not approval.

---

## 4. Timing Parameters

| Parameter | Duration | Blocks (@6s) |
|-----------|----------|--------------|
| Voting Period | 7 days | 100,800 |
| Standard Timelock | 12 days | 172,800 |
| Exit Timelock | 30 days | 432,000 |
| Grace Period | 2 days | 28,800 |

---

## 5. Proposal Lifecycle

### 5.1 Status Flow

```
Created → Active → Passed → ReadyToExecute → Executed
                      ↓              ↓
                  Rejected       Expired
                      ↓
                  Cancelled
```

### 5.2 Status Definitions

| Status | Description |
|--------|-------------|
| Active | Voting is ongoing |
| Passed | Voting passed, in timelock |
| Rejected | Failed to reach threshold |
| ReadyToExecute | Timelock complete |
| Executed | Proposal executed |
| Cancelled | Proposal cancelled |
| Expired | Not executed in grace period |

---

## 6. Creating Proposals

### 6.1 Requirements

- Proposer must be a validator
- Deposit required: 100 KRAT
- Only one active exit proposal per chain

### 6.2 Proposal Structure

```rust
pub struct Proposal {
    id: ProposalId,
    chain_id: ChainId,
    proposer: AccountId,
    proposal_type: ProposalType,
    description: Option<String>,
    status: ProposalStatus,
    created_at: BlockNumber,
    voting_ends_at: BlockNumber,
    timelock_ends_at: Option<BlockNumber>,
    votes: Vec<VoteRecord>,
    yes_votes: Balance,
    no_votes: Balance,
    abstain_votes: Balance,
    deposit: Balance,
}
```

---

## 7. Voting

### 7.1 Vote Options

| Vote | Effect |
|------|--------|
| Yes | Support the proposal |
| No | Oppose the proposal |
| Abstain | No position (counts for quorum) |

### 7.2 Voting Power

- Based on validator stake
- One vote per validator per proposal
- Cannot change vote after casting

### 7.3 Vote Record

```rust
pub struct VoteRecord {
    voter: AccountId,
    vote: Vote,
    weight: Balance,
    timestamp: BlockNumber,
}
```

---

## 8. Execution

### 8.1 Execution Requirements

1. Status must be `ReadyToExecute`
2. Current block > timelock_ends_at
3. Current block < timelock_ends_at + GRACE_PERIOD

### 8.2 Post-Execution

- Status changes to `Executed`
- Deposit returned to proposer
- Proposal effects applied

---

## 9. Deposit Handling

| Outcome | Deposit |
|---------|---------|
| Passed & Executed | Returned |
| Rejected | Returned |
| Expired | Burned |
| Cancelled by proposer | Returned |

---

## 10. Exit Proposal Specifics

### 10.1 Constraints

- Only one active exit proposal per chain
- Cannot cancel once voting ends
- 30-day timelock for asset withdrawal

### 10.2 Exit Preparation Window

During exit timelock:
- All assets freely withdrawable
- No new transactions accepted
- Governance frozen

### 10.3 Exit Types

| Type | Effect |
|------|--------|
| Dissolve | Chain dissolved, assets returned |
| Merge | State merged into target chain |
| ReattachRoot | Becomes direct child of root |
| JoinHost | Joins hostchain with shared security |

---

## 11. Governance in Security States

Governance behavior changes with network security state:

| State | Governance Status |
|-------|-------------------|
| Bootstrap | Normal |
| Normal | Normal |
| Degraded | Timelocks × 2 |
| Restricted | Frozen |
| Emergency | Frozen |

---

## 12. Implementation

### 12.1 Constants

```rust
pub const SUPERMAJORITY_THRESHOLD: u8 = 66;
pub const STANDARD_THRESHOLD: u8 = 50;
pub const MIN_QUORUM_PERCENT: u8 = 30;
pub const EXIT_TIMELOCK: BlockNumber = 432_000;
pub const STANDARD_TIMELOCK: BlockNumber = 172_800;
pub const VOTING_PERIOD: BlockNumber = 100_800;
pub const GRACE_PERIOD: BlockNumber = 28_800;
pub const PROPOSAL_DEPOSIT: Balance = 100;
```

### 12.2 Source Files

| File | Contents |
|------|----------|
| `contracts/governance.rs` | Governance contract |
| `types/transaction.rs` | Governance transactions |

---

## 13. Vote Credits

Validators earn Vote Credits (VC) for governance participation:

- +1 VC per vote cast
- Daily limit: 3 votes
- Monthly limit: 50 votes

See SPEC 2 (Validator Credits) for details.

---

## 14. Related Specifications

- **SPEC 1:** Tokenomics - Treasury spending
- **SPEC 2:** Validator Credits - Vote credits
- **SPEC 4:** Sidechains - Exit mechanisms
- **SPEC 6:** Network Security - Governance freeze
