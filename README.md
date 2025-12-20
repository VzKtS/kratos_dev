# KratOs - Decentralized Governance Blockchain

> **VERSION: DEVELOPMENT - NON FUNCTIONAL**
>
> This is an active development branch. The code is not ready for production use.
> Pull requests and contributions are welcome.
>
> **Roadmap:**
> - `kratos_dev` (here) → Development & Devnet
> - [`KratOs`](https://github.com/VzKtS/KratOs) → Testnet & Mainnet
>
> Once Dev and Devnet phases are complete, the project will be merged into the main repository.

---

**A from-scratch blockchain implementation in pure Rust, designed for long-term resilience and democratic governance.**

## Overview

KratOs is a minimalist, auditable blockchain platform built entirely from scratch without external blockchain frameworks. It enables decentralized, community-driven governance with hierarchical sidechains, cross-chain arbitration, and constitutional guarantees.

**Philosophy**: Minimal, auditable, durable - no magic, no hidden complexity.

## Current Status

| Metric | Value |
|--------|-------|
| **Lines of Code** | ~33,000 |
| **Tests Passing** | 560 |
| **Spec Compliance** | v1-v8 |
| **Architecture** | From-scratch Rust |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                       ROOT CHAIN                            │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐       │
│  │ KRAT    │  │ Staking │  │ Identity│  │ Meta-   │       │
│  │ Token   │  │         │  │         │  │ Gov     │       │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘       │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐       │
│  │Emergency│  │  Fork   │  │Arbitrate│  │ Side-   │       │
│  │ Powers  │  │ Manager │  │         │  │ chains  │       │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘       │
└─────────────────────────────────────────────────────────────┘
          │                    │                    │
    ┌─────┴─────┐        ┌─────┴─────┐        ┌─────┴─────┐
    │ HOST CHAIN│        │ HOST CHAIN│        │ HOST CHAIN│
    │  (Region) │        │ (Industry)│        │ (Interest)│
    └─────┬─────┘        └─────┬─────┘        └─────┬─────┘
          │                    │                    │
    ┌─────┴─────┐        ┌─────┴─────┐        ┌─────┴─────┐
    │ SIDECHAIN │        │ SIDECHAIN │        │ SIDECHAIN │
    │(Community)│        │(Community)│        │(Community)│
    └───────────┘        └───────────┘        └───────────┘
```

## Core Modules

### Types (`src/types/`) - 15 modules

| Module | Purpose | Lines |
|--------|---------|-------|
| `primitives` | Hash, AccountId, Balance, BlockNumber | ~100 |
| `signature` | Ed25519 signatures, verification | ~150 |
| `account` | Account state, nonces, balances | ~100 |
| `transaction` | 25 transaction types, fees, validation | ~400 |
| `block` | Block structure, headers, validation | ~200 |
| `chain` | ChainId, ChainStatus, federation | ~150 |
| `merkle` | Merkle trees, proofs, verification | ~300 |
| `fraud` | Fraud proofs, state transition disputes | ~400 |
| `dispute` | Dispute types, arbitration data | ~500 |
| `identity` | L1 identity, attestations, deposits | ~300 |
| `personhood` | Proof-of-personhood, uniqueness | ~250 |
| `reputation` | Multi-domain reputation, decay | ~350 |
| `protocol` | Constitutional axioms, parameters | ~400 |
| `emergency` | Emergency states, circuit breakers | ~500 |
| `fork` | Fork types, declarations, ossification | ~560 |

### Contracts (`src/contracts/`) - 13 modules

| Contract | Purpose | Tests |
|----------|---------|-------|
| `krat` | Native token, transfers, minting | 15 |
| `staking` | Validator staking, delegation | 18 |
| `governance` | Proposals, voting, execution | 22 |
| `sidechains` | Registration, federation, purging | 20 |
| `identity` | Identity registration, attestation | 16 |
| `personhood` | Uniqueness verification | 12 |
| `reputation` | Reputation management, endorsements | 18 |
| `messaging` | Cross-chain message passing | 14 |
| `arbitration` | VRF jury selection, verdicts | 11 |
| `meta_governance` | Constitutional amendments | 15 |
| `emergency` | Emergency powers, recovery | 21 |
| `fork` | Fork lifecycle, snapshots | 22 |

### Other Modules

| Module | Purpose |
|--------|---------|
| `consensus/` | VRF leader selection, block production |
| `execution/` | Transaction execution engine |
| `genesis/` | Genesis block configuration |
| `network/` | P2P networking, sync |
| `node/` | Node service, mempool, producer |
| `rpc/` | JSON-RPC server |
| `storage/` | RocksDB state storage |
| `tests/` | Integration & invariant tests |

## Implemented Specifications

### SPEC v1: Validator Credits
- Dual-token system (KRAT economic, VC governance)
- VC earned through validator work
- 1 person = 1 vote via VC weighting

### SPEC v2: Economics
- Decaying emission (5% → 0.5% over 20 years)
- Growing burn mechanism (1% → 3.5%)
- 70% validators, 20% treasury, 10% reserve

### SPEC v3.1: Hierarchical Sidechains
- L1 (Root) → L2 (Host) → L3 (Community) hierarchy
- Opt-in federation with exit guarantees
- Merkle proofs for cross-chain verification
- Fraud proof system with challenge periods

### SPEC v4: Identity & Reputation
- L1 identities with attestation network
- Proof-of-personhood integration
- Multi-domain reputation (technical, social, governance)
- Cross-chain reputation portability

### SPEC v5: Security Invariants
- 18 cryptographic security guarantees
- Slashing conditions defined
- Constitutional axiom enforcement

### SPEC v6: Meta-Governance
- 5 constitutional axioms (immutable)
- 3 governance parameters (amendable)
- Sunset clauses for emergency powers

### SPEC v7: Emergency Powers
- Emergency state machine (75% threshold)
- 5 circuit breakers (finality, participation, state, slashing, governance)
- 6-step recovery process
- Auto-expiration (7 days max)

### SPEC v8: Long-Term Resilience
- 5 fork types (technical, constitutional, governance, social, survival)
- Multiple declaration paths (33% validators, 40% stake, 3 sidechains)
- Fork neutrality (no privileged fork)
- Ossification mode (10 years, 90% consensus)
- State/identity/reputation snapshots

## Constitutional Axioms

These are **immutable** - they cannot be changed by governance:

1. **ExitAlwaysPossible** - Any user can exit with assets within 30 days
2. **NoRetroactivePunishment** - Cannot punish for past legal actions
3. **IdentitySovereignty** - Identity cannot be erased without consent
4. **TransparencyRequired** - All governance decisions must be auditable
5. **ForkingLegitimate** - Forking is always a valid option

## Safety Invariants

Tested and enforced (see `tests/`):

- Emergency powers cannot become permanent
- Constitution cannot be bypassed even in emergency
- Exit is always possible (30-day guarantee)
- Failures stay local (no global collapse)
- Recovery is deterministic
- Forking is always possible

## Building & Running

### Prerequisites

```bash
# Install Rust (nightly required)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default nightly
```

### Build

```bash
cd rust/kratos-core

# Check compilation
cargo check

# Build
cargo build --release

# Run tests
cargo test
```

### Run Node

```bash
# Development mode
./target/release/kratos-node --dev

# With custom port
./target/release/kratos-node --rpc-port 9933
```

## Test Coverage

```
560 tests passing

Breakdown:
- types/: 180+ tests
- contracts/: 200+ tests
- integration/: 50+ tests
- security_invariants/: 40+ tests
- emergency_invariants/: 29 tests
- fork_invariants/: 31 tests
```

## Project Structure

```
KratOs/
├── rust/
│   └── kratos-core/
│       └── src/
│           ├── main.rs           # Node entry point
│           ├── types/            # Core data types (15 modules)
│           ├── contracts/        # System contracts (13 modules)
│           ├── consensus/        # VRF consensus
│           ├── execution/        # Transaction execution
│           ├── genesis/          # Genesis configuration
│           ├── network/          # P2P networking
│           ├── node/             # Node service
│           ├── rpc/              # JSON-RPC API
│           ├── storage/          # RocksDB storage
│           └── tests/            # Integration tests
└── spec/
    ├── SPEC_V1_VALIDATOR_CREDITS.md
    ├── SPEC_V2_ECONOMICS.md
    ├── SPEC_V3.1_RECONCILIATION.md
    ├── SPEC_V4_RECONCILIATION.md
    ├── SPEC_V5_RECONCILIATION.md
    ├── SPEC_V6_RECONCILIATION.md
    ├── SPEC_V7_RECONCILIATION.md
    ├── SPEC_V8_RECONCILIATION.md
    └── IMPLEMENTATION_LOG_*.md
```

## Key Design Decisions

### Why From-Scratch?

1. **Auditability** - Every line of code is visible and understandable
2. **Minimalism** - No framework bloat, only what's needed
3. **Durability** - No dependency on external SDK roadmaps
4. **Educational** - Clear architecture for contributors

### Why Rust?

1. **Memory safety** - No null pointers, no data races
2. **Performance** - Zero-cost abstractions
3. **Ecosystem** - Excellent crypto libraries (ed25519-dalek, sha3)
4. **Reliability** - Strong type system prevents bugs

### Why Hierarchical Sidechains?

1. **Scalability** - Horizontal scaling through sidechains
2. **Sovereignty** - Communities control their own chains
3. **Flexibility** - Different chains, different rules
4. **Exit rights** - Always possible to leave

## Roadmap

### Completed
- [x] Core type system
- [x] All system contracts
- [x] Consensus mechanism (VRF)
- [x] Transaction execution
- [x] Storage layer (RocksDB)
- [x] Emergency powers system
- [x] Fork management system
- [x] 560 tests passing

### In Progress
- [ ] P2P networking finalization
- [ ] Block synchronization
- [ ] Multi-node testnet

### Planned
- [ ] Light client support
- [ ] WebSocket subscriptions
- [ ] Block explorer
- [ ] Wallet integration
- [ ] Mainnet launch

## Contributing

We welcome contributions! Key areas:

1. **Testing** - More edge case coverage
2. **Documentation** - API docs, tutorials
3. **Networking** - P2P improvements
4. **Tooling** - CLI tools, monitoring

## License

Apache License 2.0 - See LICENSE file

## Links

- **Specifications**: `/spec/` directory
- **Implementation Logs**: `/spec/IMPLEMENTATION_LOG_*.md`
- **Tests**: `/rust/kratos-core/src/tests/`

---

**KratOs** - Governance by the people, secured by cryptography.
