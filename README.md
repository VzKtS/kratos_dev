# KratOs - Decentralized Governance Blockchain

---

## Vision Report

> *"The creator/founder (Vzcrow) will launch the blockchain's development without any financial or technical advantage and will be subject to community votes and consensus mechanisms, like all other participants. Their status as a validator (guarantor of network security) can be revoked, resulting in the loss of the associated KRAT (the network's native token) revenue."*

### A Decentralized Infrastructure for Human Coordination

#### 1. Introduction

KratOs is not a blockchain designed to optimize transactions, speculation, or short-term efficiency. It is an attempt to address a much more difficult question:

**How can large groups of humans coordinate, decide, and coexist over the long term without central authority, without collapsing under their own complexity, and without being captured by wealth, ideology, or technology?**

KratOs is a long-term human governance protocol, designed on the premise that:
- humans are imperfect,
- power corrupts,
- communities diverge,
- no social model remains stable forever.

#### 2. The Fundamental Problem KratOs Seeks to Solve

Modern governance systems fail for structural reasons:
- Centralization accumulates power faster than accountability.
- Democracy degrades when it exceeds local scale.
- Economic capital becomes political capital.
- Digital systems optimize for speed, not legitimacy.
- A central failure propagates throughout the entire system.

Blockchains have attempted to provide an answer, but most have reproduced the same flaws:
- stake becomes power,
- early adopters dominate permanently,
- governance becomes speculative,
- systems collapse under social pressure rather than technical failure.

**KratOs starts from a different premise:**
*Governance must be slow, local, revocable, and allowed to fail in isolation.*

#### 3. Founding Philosophy

KratOs rests on five philosophical axioms:

**3.1 Imperfection Is Inevitable**
The system does not seek to eliminate conflicts or bad decisions. It seeks to contain them.

**3.2 Power Must Accumulate Slowly**
Any form of influence—economic, political, or technical—must require time, not just capital.

**3.3 Exit Is More Important Than Voice**
The ability to leave, fork, or dissolve is more stabilizing than endless debate.

**3.4 All Politics Is Local**
Global consensus is fragile. Local governance is resilient.

**3.5 No Layer Should Be Mandatory**
Identity, ideology, federation, and governance models must remain optional.

#### 4. What KratOs Is (and What It Is Not)

**KratOs is:**
- a minimal root chain providing security and finality,
- an infrastructure for sovereign communities (sidechains),
- a framework for voluntary federation,
- a long-term experiment in human coordination.

**KratOs is not:**
- a replacement for states,
- a global democracy,
- a universal identity system,
- a financial product.

#### 5. Architectural Vision

**5.1 A Minimal and Durable Core**

The root chain (L0) is intentionally limited to:
- consensus,
- staking,
- monetary issuance,
- sidechain registry,
- fork mechanisms.

It does not contain:
- social rules,
- ideology,
- imposed identity,
- definitions of human rights.

*This minimalism is deliberate: the core must survive political, cultural, and technological upheavals.*

**5.2 Sidechains as Living Communities**

Each community exists as a sidechain:
- with its own rules,
- its own local governance,
- its own lifecycle.

Communities can:
- be freely created,
- grow,
- split,
- federate,
- or disappear.

*Failure is not catastrophic—it is expected.*

**5.3 Federation Without Domination**

Host chains allow communities to:
- pool resources,
- coordinate policies,
- share infrastructure.

But federation is always:
- voluntary,
- revocable,
- limited.

*There is no level of "global government."*

#### 6. Power, Merit, and Time

**6.1 Breaking the Wealth = Power Equation**

KratOs explicitly separates:
- economic capital (security),
- reputation (Validator Credits),
- governance rights (local and contextualized).

*No single variable allows domination of the system.*

**6.2 Time as a Fundamental Resource**

In KratOs, influence requires:
- long-term participation,
- repeated honest behavior,
- visible contribution.

Power thus becomes:
- difficult to buy,
- slow to accumulate,
- easy to lose through inactivity or abuse.

#### 7. Economic Vision

The KRAT token is not a promise of profit. It is:
- a coordination cost,
- a security guarantee,
- an anti-spam and anti-capture mechanism.

Inflation is:
- higher when the system is fragile,
- lower when it is stable,
- adaptive to actual usage.

Over the long term, the system tends toward:
- low inflation,
- sustainability through fees,
- lasting equilibrium.

#### 8. Resilience by Design

KratOs is designed to survive:
- validator cartels,
- ideological splits,
- mass abandonment,
- state capture attempts,
- regulatory pressure,
- social conflicts,
- technological obsolescence.

It achieves this by ensuring that:
- no failure is global,
- no community is permanent,
- no decision is irreversible.

#### 9. A System Aware of Its Limits

KratOs does not claim to solve:
- human disagreements,
- moral conflicts,
- political violence,
- cultural divergences.

It provides:
- clear boundaries,
- rules of interaction,
- mechanisms for peaceful separation.

*A system that assumes harmony fails.*
*A system that assumes conflict can endure.*

#### 10. Long-Term Ambition

If it succeeds, KratOs could become:
- a foundation for digital cities,
- an infrastructure for cooperative economies,
- a neutral ground for governance experiments,
- a memory of institutional evolution.

But success is not domination. Success is:
- longevity,
- adaptability,
- peaceful fragmentation.

#### 11. Final Declaration

KratOs is not designed to win a market.
It is designed to outlast cycles.

It sacrifices speed for legitimacy,
efficiency for resilience,
simplicity for depth.

**KratOs is not the promise of a better world.**

**It is a framework allowing many worlds to coexist—and fail—without destroying each other.**

---

> **VERSION: DEVNET - FUNCTIONAL**
>
> The core blockchain is now functional with multi-node consensus and KRAT transfers working.
> Currently running on devnet with 2+ validator nodes.
>
> **Roadmap:**
> - `kratos_dev` (here) → Development & Devnet ✅
> - [`KratOs`](https://github.com/VzKtS/KratOs) → Testnet & Mainnet
>
> Once Devnet phase is complete, the project will be merged into the main repository.

---

**A from-scratch blockchain implementation in pure Rust, designed for long-term resilience and democratic governance.**

## Overview

KratOs is a minimalist, auditable blockchain platform built entirely from scratch without external blockchain frameworks. It enables decentralized, community-driven governance with hierarchical sidechains, cross-chain arbitration, and constitutional guarantees.

**Philosophy**: Minimal, auditable, durable - no magic, no hidden complexity.

## Current Status

| Metric | Value |
|--------|-------|
| **Lines of Code** | ~35,000 |
| **Tests Passing** | 560+ |
| **Spec Compliance** | v1-v8 |
| **Architecture** | From-scratch Rust |
| **Network Status** | Devnet (2+ nodes) |
| **Consensus** | VRF-based PoS |
| **Wallet** | CLI wallet functional |

### Recent Achievements

- ✅ Multi-node devnet running (VPS + local nodes)
- ✅ KRAT transfers working between nodes
- ✅ Block synchronization with peer discovery
- ✅ VRF slot leader election operational
- ✅ Transaction history RPC implemented
- ✅ CLI wallet with encrypted key storage

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
| `rpc/` | JSON-RPC server (20+ methods) |
| `storage/` | RocksDB state storage |
| `tests/` | Integration & invariant tests |

### Wallet (`kratos-wallet/`)

| Module | Purpose |
|--------|---------|
| `main.rs` | CLI interface, commands |
| `crypto.rs` | Ed25519 keys, Argon2 encryption |
| `rpc.rs` | JSON-RPC client |
| `storage.rs` | Encrypted wallet storage |
| `types.rs` | Transaction types, history |

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
# Development mode (solo validator)
./target/release/kratos-node --dev

# Bootstrap node (first node in network)
./target/release/kratos-node \
  --validator \
  --rpc-port 9933 \
  --p2p-port 30333 \
  --base-path ~/.kratos-node1

# Join existing network
./target/release/kratos-node \
  --validator \
  --rpc-port 9934 \
  --p2p-port 30334 \
  --base-path ~/.kratos-node2 \
  --bootnodes "/ip4/<IP>/tcp/30333/p2p/<PEER_ID>"
```

### Run Wallet

```bash
cd rust/kratos-wallet
cargo build --release

# Create new wallet
./target/release/kratos-wallet create

# Check balance
./target/release/kratos-wallet balance

# Send KRAT
./target/release/kratos-wallet send <ADDRESS> <AMOUNT>

# View transaction history
./target/release/kratos-wallet history
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
│   ├── kratos-core/              # Main blockchain node
│   │   └── src/
│   │       ├── main.rs           # Node entry point
│   │       ├── cli/              # CLI runner, argument parsing
│   │       ├── types/            # Core data types (15 modules)
│   │       ├── contracts/        # System contracts (13 modules)
│   │       ├── consensus/        # VRF consensus, validator selection
│   │       ├── execution/        # Transaction execution engine
│   │       ├── genesis/          # Genesis block configuration
│   │       ├── network/          # libp2p networking, sync, DNS seeds
│   │       ├── node/             # Node service, mempool, block producer
│   │       ├── rpc/              # JSON-RPC server (20+ methods)
│   │       ├── storage/          # RocksDB state storage
│   │       └── tests/            # Integration tests
│   │
│   └── kratos-wallet/            # CLI wallet
│       └── src/
│           ├── main.rs           # CLI commands (create, send, balance, history)
│           ├── crypto.rs         # Ed25519 + Argon2 encryption
│           ├── rpc.rs            # JSON-RPC client
│           ├── storage.rs        # Encrypted wallet file storage
│           └── types.rs          # Transaction types
│
├── spec/                         # Protocol specifications
│   ├── SPEC_1_TOKEN.md           # KRAT token specification
│   ├── SPEC_2_ECONOMICS.md       # Economic model
│   ├── SPEC_3_CONSENSUS.md       # VRF consensus
│   ├── SPEC_4_IDENTITY.md        # Identity system
│   ├── SPEC_5_SECURITY.md        # Security invariants
│   ├── SPEC_6_NETWORK_SECURITY.md # Network security
│   ├── SPEC_7_EMERGENCY.md       # Emergency powers
│   └── SPEC_8_WALLET.md          # Wallet specification
│
├── synthese/                     # Technical synthesis documents
│   ├── KRATOS_SYNTHESIS.md       # Main technical overview
│   └── WALLET_SYNTHESIS.md       # Wallet architecture
│
└── docs/
    └── diagrams/                 # Architecture diagrams (SVG)
        ├── block-production-flow.svg
        ├── transaction-flow.svg
        └── ...
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

## JSON-RPC API

### State Methods

| Method | Description |
|--------|-------------|
| `state_getAccount` | Get account info (balance, nonce) |
| `state_getNonce` | Get account nonce |
| `state_getTransactionHistory` | Get transaction history for address |

### Chain Methods

| Method | Description |
|--------|-------------|
| `chain_getInfo` | Get chain info (height, hash, sync status) |
| `chain_getBlock` | Get block by hash |
| `chain_getBlockByNumber` | Get block by number |

### Author Methods

| Method | Description |
|--------|-------------|
| `author_submitTransaction` | Submit signed transaction |

### Validator Methods (Bootstrap Era)

| Method | Description |
|--------|-------------|
| `validator_getEarlyVotingStatus` | Get bootstrap voting status |
| `validator_getPendingCandidates` | List validator candidates |
| `validator_getCandidateVotes` | Get votes for candidate |
| `validator_canVote` | Check if account can vote |

### System Methods

| Method | Description |
|--------|-------------|
| `system_health` | Node health check |
| `system_peers` | List connected peers |

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
- [x] P2P networking (libp2p)
- [x] Block synchronization
- [x] Multi-node devnet
- [x] CLI wallet with encryption
- [x] Transaction history RPC
- [x] KRAT transfers

### In Progress
- [ ] Early validator voting system
- [ ] Block explorer backend
- [ ] Improved peer discovery

### Planned
- [ ] Light client support
- [ ] WebSocket subscriptions
- [ ] Block explorer frontend
- [ ] Mobile wallet
- [ ] Testnet launch
- [ ] Mainnet launch

## Contributing

We welcome contributions! Key areas:

1. **Testing** - More edge case coverage
2. **Documentation** - API docs, tutorials
3. **Networking** - P2P improvements
4. **Tooling** - CLI tools, monitoring

## License

Apache License 2.0 - See LICENSE file

## Network Configuration

### Genesis Validators

The network starts with bootstrap validators who can produce blocks and vote for new validators:

| Validator | Address | Initial Balance |
|-----------|---------|-----------------|
| Bootstrap 1 | `0x0101...0101` | 10,000,000 KRAT |
| Bootstrap 2 | `0x0202...0202` | 10,000,000 KRAT |

### Block Parameters

| Parameter | Value |
|-----------|-------|
| Block Time | 6 seconds |
| Block Reward | ~12.37 KRAT |
| Max Block Size | 1 MB |
| Transaction Fee | 0.001 KRAT base |

### Network Ports

| Port | Purpose |
|------|---------|
| 9933 | JSON-RPC HTTP |
| 30333 | P2P libp2p |

## Links

- **Specifications**: `/spec/` directory
- **Technical Synthesis**: `/synthese/` directory
- **Architecture Diagrams**: `/docs/diagrams/`
- **Tests**: `/rust/kratos-core/src/tests/`

---

**KratOs** - Governance by the people, secured by cryptography.
