# SPEC 7: Network Roles & Contributor System

**Status**: Normative
**Version**: 1.2
**Last Updated**: 2025-12-19

---

## 1. Overview

This specification defines the unified on-chain role system for KratOs, including:
- **Consensus roles** (Validator, Juror) - managed by staking/consensus
- **Contributor roles** - treasury-funded programs managed by governance

All roles are unified under `NetworkRole` for consistent identity management.

### 1.1 Design Principles

1. **Pseudonymity Preserved**: Only AccountId (Ed25519 public key) is stored on-chain
2. **Unified Role Model**: All network roles (consensus + contributor) in single enum
3. **On-Chain Verifiable**: Role status is publicly verifiable
4. **Non-Transferable**: Roles cannot be sold or transferred
5. **Role-Specific Registration**: Different roles have different registration methods

### 1.2 Constitutional Compliance

Per Article VI of the Genesis Constitution:
- Identity is OPTIONAL for all roles
- Participants can operate with only a wallet address
- No KYC or personal data required

---

## 2. Network Roles (Unified)

### 2.1 NetworkRole Enum

```rust
pub enum NetworkRole {
    // Consensus Roles
    Validator,                    // Block production, consensus
    Juror,                        // Arbitration participation

    // Contributor Roles (treasury-funded)
    Contributor(ContributorRole),
}
```

### 2.2 Registration Methods

| Role | Registration Method | Requirements |
|------|---------------------|--------------|
| **Validator** | `RegisterValidator` transaction | Stake (50,000+ KRAT) |
| **Juror** | Automatic | Validator + sufficient VC |
| **Contributor** | Governance proposal | 10 KRAT application stake |

### 2.3 Role Categories

| Category | Roles | Managed By |
|----------|-------|------------|
| **Consensus** | Validator, Juror | Staking system, automatic |
| **Contributor** | 9 treasury-funded roles | Governance voting |

### 2.4 Unified Role Registry

All network roles are tracked in a single `NetworkRoleRegistry` integrated within the `ValidatorSet`:

```rust
pub struct ValidatorSet {
    pub validators: BTreeMap<AccountId, ValidatorInfo>,
    pub total_stake: Balance,
    pub role_registry: NetworkRoleRegistry,  // Unified role tracking
}
```

**Automatic Synchronization:**
- `add_validator()` → registers in `NetworkRoleRegistry`
- `remove_validator()` → unregisters from `NetworkRoleRegistry`
- `update_validator_stake()` → syncs stake to registry

This ensures a single source of truth for all network roles.

---

## 3. Treasury Programs

### 3.1 Program Definitions

The treasury (20% of block emissions) funds the following programs:

| Program | Budget % | Description | Max Payment/Contribution |
|---------|----------|-------------|-------------------------|
| **Bug Bounty** | 20% | Security vulnerability rewards | 100,000 KRAT |
| **Security Audit** | 15% | Formal security reviews | 50,000 KRAT |
| **Core Development** | 25% | Protocol development | 25,000 KRAT |
| **Content Creation** | 10% | Documentation, tutorials, marketing | 5,000 KRAT |
| **Ambassador** | 8% | Community outreach | 2,000 KRAT |
| **Research Grant** | 10% | Academic/industry research | 50,000 KRAT |
| **Infrastructure** | 5% | Node operators, tooling | 10,000 KRAT |
| **Translation** | 4% | Internationalization | 1,000 KRAT |
| **Education** | 3% | Training, workshops | 5,000 KRAT |

**Total**: 100% of treasury allocation

### 3.2 Budget Allocation Formula

```
program_budget = treasury_emission * program_allocation_percent / 100
treasury_emission = block_emission * 20 / 100
```

---

## 4. Contributor Roles

### 4.1 Role Types

| Role | Associated Program | Description |
|------|-------------------|-------------|
| `BugHunter` | Bug Bounty | Can submit vulnerability reports |
| `SecurityAuditor` | Security Audit | Can submit audit reports |
| `CoreDeveloper` | Core Development | Can receive dev payments |
| `ContentCreator` | Content Creation | Can receive content payments |
| `Ambassador` | Ambassador | Can receive outreach payments |
| `Researcher` | Research Grant | Can receive research grants |
| `InfrastructureProvider` | Infrastructure | Node operators |
| `Translator` | Translation | Localization work |
| `Educator` | Education | Training and education |

### 4.2 Role Properties

```rust
pub struct RoleRegistration {
    pub registration_id: Hash,        // Unique ID
    pub account: AccountId,           // Pseudonymous identity
    pub role: ContributorRole,        // Role type
    pub scope: Option<ChainId>,       // Chain scope (None = Root Chain)
    pub status: RoleStatus,           // Current status
    pub granted_at: BlockNumber,      // When granted
    pub expires_at: BlockNumber,      // Expiration block
    pub granted_by_proposal: Hash,    // Governance proposal ID
    pub total_payments: Balance,      // Cumulative payments
    pub contribution_count: u32,      // Number of contributions
    pub alias: Option<String>,        // Optional public alias
}
```

### 4.3 Role Status Lifecycle

```
Application → Pending → Active → Expired
                ↓          ↓
            Rejected   Suspended
                          ↓
                       Revoked
```

| Status | Description | Can Receive Payment |
|--------|-------------|---------------------|
| `Pending` | Awaiting governance vote | No |
| `Active` | Role is valid | Yes |
| `Expired` | Past expiration (renewable) | No |
| `Suspended` | Temporarily disabled | No |
| `Revoked` | Permanently removed | No |

---

## 5. Role Application Process

### 5.1 Application Requirements

| Parameter | Value | Description |
|-----------|-------|-------------|
| Application Stake | 10 KRAT | Refunded on approval, slashed on spam |
| Role Duration | 180 days (~2,592,000 blocks) | Default validity period |
| Grace Period | 14 days (~201,600 blocks) | Renewal window after expiry |
| Max Roles per Account | 5 | Anti-abuse limit |

### 5.2 Application Flow

```
1. Applicant submits RoleApplication with:
   - Role type requested
   - Justification hash (off-chain document)
   - Stake deposit (10 KRAT)
   - Optional alias

2. Governance proposal auto-created

3. Community votes during voting period (7 days)

4. If approved (threshold met):
   - RoleRegistration created
   - Stake refunded
   - Role active for 180 days

5. If rejected:
   - Stake refunded (unless marked as spam)
   - Application marked rejected
```

### 5.3 Approval Thresholds

| Program Type | Approval Threshold |
|--------------|-------------------|
| Security Audit | 67% (supermajority) |
| Core Development | 67% (supermajority) |
| All others | 51% (simple majority) |

Higher thresholds for security-critical roles ensure stronger community vetting.

---

## 6. Contribution Claims

### 6.1 Claim Structure

```rust
pub struct ContributionClaim {
    pub claim_id: Hash,
    pub contributor: AccountId,
    pub registration_id: Hash,        // Must have active role
    pub program: TreasuryProgram,
    pub amount: Balance,              // Requested payment
    pub evidence_hash: Hash,          // Off-chain proof
    pub severity: Option<BugSeverity>,// For bug bounty only
    pub status: ClaimStatus,
}
```

### 6.2 Claim Status Flow

```
Submitted → UnderReview → Approved → Paid
                ↓            ↓
            Rejected     Disputed
```

### 6.3 Bug Severity Levels

For Bug Bounty program only:

| Severity | Reward % of Max | Example |
|----------|-----------------|---------|
| Low | 5% (5,000 KRAT) | Minor UI issues |
| Medium | 20% (20,000 KRAT) | Logic errors |
| High | 50% (50,000 KRAT) | Significant exploits |
| Critical | 100% (100,000 KRAT) | Consensus-breaking bugs |

---

## 7. Governance Integration

### 7.1 Proposal Types

Role management uses the existing governance system with new proposal types:

```rust
pub enum ProposalType {
    // ... existing types ...

    /// Grant a contributor role
    GrantContributorRole {
        applicant: AccountId,
        role: ContributorRole,
        application_id: Hash,
    },

    /// Revoke a contributor role
    RevokeContributorRole {
        registration_id: Hash,
        reason: String,
    },

    /// Approve a contribution claim
    ApproveContributionClaim {
        claim_id: Hash,
        amount: Balance,
    },
}
```

### 7.2 Voting Rules

| Action | Threshold | Timelock |
|--------|-----------|----------|
| Grant Role (standard) | 51% | 12 days |
| Grant Role (security) | 67% | 12 days |
| Revoke Role | 51% | 2 days |
| Approve Claim | 51% | 2 days |

---

## 8. Anti-Abuse Mechanisms

### 8.1 Spam Prevention

- **Application Stake**: 10 KRAT deposit
- **Stake Slashing**: Spam applications lose stake
- **Role Limits**: Max 5 roles per account
- **Cooldown**: 7 days between applications for same role

### 8.2 Payment Limits

- **Per-Contribution Cap**: Program-specific maximum
- **Monthly Cap**: 2x max payment per contributor per program
- **Annual Budget**: Each program has fixed annual allocation

### 8.3 Abuse Detection

- **Duplicate Claims**: Same evidence_hash rejected
- **Self-Dealing**: Claims reviewed by non-contributor validators
- **Velocity Limits**: Max claims per time period

---

## 9. Role Renewal

### 9.1 Renewal Process

1. Within grace period (14 days after expiry)
2. Submit renewal request (no stake required)
3. Automatic approval if:
   - No misconduct during term
   - At least 1 contribution made
   - Original proposal not revoked
4. Otherwise requires new governance vote

### 9.2 Automatic Renewal Criteria

```rust
fn can_auto_renew(registration: &RoleRegistration) -> bool {
    registration.contribution_count > 0 &&
    registration.status != RoleStatus::Suspended &&
    !has_misconduct_record(registration.account)
}
```

---

## 10. Events

The contributor system emits the following on-chain events:

| Event | Description |
|-------|-------------|
| `ApplicationSubmitted` | New role application |
| `RoleGranted` | Role approved and activated |
| `RoleRenewed` | Role renewed for new term |
| `RoleSuspended` | Role temporarily disabled |
| `RoleRevoked` | Role permanently removed |
| `ClaimSubmitted` | New contribution claim |
| `ClaimApproved` | Claim approved for payment |
| `ClaimPaid` | Payment executed |
| `ClaimRejected` | Claim rejected |

---

## 11. Implementation Reference

### 11.1 Source Files

| File | Description |
|------|-------------|
| [contributor.rs](../../rust/kratos-core/src/types/contributor.rs) | Core types: NetworkRole, NetworkRoleRegistry, ContributorRole |
| [validator.rs](../../rust/kratos-core/src/consensus/validator.rs) | ValidatorSet with integrated NetworkRoleRegistry |
| [governance.rs](../../rust/kratos-core/src/contracts/governance.rs) | Proposal integration |
| [krat.rs](../../rust/kratos-core/src/contracts/krat.rs) | Treasury distribution |

### 11.2 Constants

```rust
// Role timing
pub const DEFAULT_ROLE_DURATION: BlockNumber = 2_592_000;  // 180 days
pub const ROLE_GRACE_PERIOD: BlockNumber = 201_600;         // 14 days

// Anti-spam
pub const ROLE_APPLICATION_STAKE: Balance = 10_000_000_000_000; // 10 KRAT
pub const MAX_ROLES_PER_ACCOUNT: usize = 5;
```

---

## 12. Future Extensions

### 12.1 Application Layer Integration

The base protocol provides the foundation. Application layers can extend with:

- **Reputation Scoring**: Track contributor quality over time
- **Skill Badges**: Certifications for specific skills
- **DAO Integration**: Program-specific governance DAOs
- **Automated Verification**: On-chain proof verification for claims

### 12.2 Cross-Chain Portability

Contributors with good standing can port their reputation to sidechains:

```
Root Chain Role → Attestation → Sidechain Recognition
```

---

## 13. Security Considerations

### 13.1 Threat Model

| Threat | Mitigation |
|--------|------------|
| Sybil Attack | Stake requirement, role limits |
| Collusion | Supermajority for security roles |
| Self-Dealing | Claim review by non-contributors |
| Spam | Stake slashing, cooldowns |

### 13.2 Invariants

1. **No Payment Without Role**: Claims require active role
2. **No Role Without Vote**: All roles require governance approval
3. **Budget Limit**: Cannot exceed program allocation
4. **Pseudonymity**: Only AccountId stored on-chain

---

**Implementation Status**: Complete
**Source Code**: `rust/kratos-core/src/types/contributor.rs`
**Related Specs**: [SPEC 1](SPEC_1_TOKENOMICS.md) (Treasury), [SPEC 5](SPEC_5_GOVERNANCE.md) (Voting)
