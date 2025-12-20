# KratOs Specifications

This directory contains the normative specifications for the KratOs blockchain protocol.

## Unified Architecture

KratOs uses a **single unified configuration** - there are no separate dev/devnet/testnet/mainnet modes. The protocol operates with one set of parameters regardless of deployment context. This design choice:

- Simplifies logical interactions
- Reduces configuration-related bugs
- Ensures consistent behavior across all deployments
- Makes the codebase more auditable

For testing purposes, duplicate the project rather than using mode flags.

## Specification Index

| SPEC | Title | Description |
|------|-------|-------------|
| [SPEC 1](SPEC_1_TOKENOMICS.md) | Tokenomics | Token supply, inflation, burn, fee distribution |
| [SPEC 2](SPEC_2_VALIDATOR_CREDITS.md) | Validator Credits | VC accumulation, VRF weighting, stake reduction |
| [SPEC 3](SPEC_3_CONSENSUS.md) | Consensus | PoS mechanism, block production, finality |
| [SPEC 4](SPEC_4_SIDECHAINS.md) | Sidechains | Chain hierarchy, security modes, exits |
| [SPEC 5](SPEC_5_GOVERNANCE.md) | Governance | Proposals, voting, timelocks |
| [SPEC 6](SPEC_6_NETWORK_SECURITY.md) | Network Security | Security states, thresholds, recovery |
| [SPEC 7](SPEC_7_CONTRIBUTOR_ROLES.md) | Contributor Roles | Treasury programs, pseudonymous contributors |
| [SYNTHESIS](../synthese/KRATOS_SYNTHESIS.md) | Protocol Synthesis | Complete protocol overview and integration |

## Specification Status

All specifications are **Normative** and reflect the current implementation.

## Cross-References

```
SPEC 1 (Tokenomics)
├── SPEC 2: VC-based stake reduction
├── SPEC 3: Block rewards
└── SPEC 6: Inflation per security state

SPEC 2 (Validator Credits)
├── SPEC 1: Staking economics
├── SPEC 3: VRF selection weights
└── SPEC 5: Vote credits

SPEC 3 (Consensus)
├── SPEC 1: Block rewards
├── SPEC 2: Selection weighting
└── SPEC 6: Validator thresholds

SPEC 4 (Sidechains)
├── SPEC 1: Validator limits
├── SPEC 5: Exit proposals
└── SPEC 6: Chain security states

SPEC 5 (Governance)
├── SPEC 1: Treasury spending
├── SPEC 2: Vote credits
├── SPEC 4: Exit mechanisms
├── SPEC 6: Governance freeze
└── SPEC 7: Role approvals

SPEC 6 (Network Security)
├── SPEC 1: Inflation adjustments
├── SPEC 2: Bootstrap VC multipliers
└── SPEC 5: Governance freeze

SPEC 7 (Contributor Roles)
├── SPEC 1: Treasury funding
└── SPEC 5: Role governance
```

## Version History

| Date | Change |
|------|--------|
| 2025-12-19 | Added SPEC 7 - Contributor Roles & Treasury Programs |
| 2025-12-19 | Unified architecture - removed dev/devnet/testnet/mainnet modes |
| 2025-12-19 | Reorganized SPECs into unified structure (1-6) |
| 2025-12-19 | Added KRATOS_SYNTHESIS.md - complete protocol overview |
| 2025-12-15 | Legacy SPEC_V1 through V8 replaced |

## Implementation Reference

Source code: `rust/kratos-core/src/`

| Module | Related SPECs |
|--------|---------------|
| `consensus/` | SPEC 1, 2, 3, 6 |
| `contracts/` | SPEC 1, 4, 5 |
| `types/` | All SPECs |
| `node/` | SPEC 3 |
