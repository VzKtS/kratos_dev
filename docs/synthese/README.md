# KratOs Syntheses

This directory contains comprehensive synthesis documents that combine information from multiple specification sources.

## Unified Architecture

KratOs uses a **single unified configuration** - there are no separate dev/devnet/testnet/mainnet modes. All documents in this directory reflect this unified architecture.

## Document Index

| Document | Description |
|----------|-------------|
| [KRATOS_SYNTHESIS.md](KRATOS_SYNTHESIS.md) | Complete protocol synthesis - all specifications unified |
| [TOKENOMICS.md](TOKENOMICS.md) | Complete economic model and token mechanics |
| [NODE_IMPLEMENTATION.md](NODE_IMPLEMENTATION.md) | Node architecture and implementation guide |
| [BLOCKCHAIN_DATA_MODEL.md](BLOCKCHAIN_DATA_MODEL.md) | Data types, transactions, and governance structures |
| [RPC_API_REFERENCE.md](RPC_API_REFERENCE.md) | **JSON-RPC API reference for clients (Kotlin, JS, etc.)** |
| [GENESIS_TESTING.md](GENESIS_TESTING.md) | Genesis block creation and testing procedures |

## Relationship to Specifications

```
spec/
├── SPEC_1_TOKENOMICS.md      ──┐
├── SPEC_2_VALIDATOR_CREDITS.md │
├── SPEC_3_CONSENSUS.md        ├──► synthese/KRATOS_SYNTHESIS.md
├── SPEC_4_SIDECHAINS.md       │    (Complete Protocol Overview)
├── SPEC_5_GOVERNANCE.md       │
└── SPEC_6_NETWORK_SECURITY.md─┘

synthese/
├── KRATOS_SYNTHESIS.md       # Master synthesis of all SPECs
├── TOKENOMICS.md             # Detailed economic model (SPEC 1, 2)
├── NODE_IMPLEMENTATION.md    # Implementation guide (SPEC 3, 6)
├── BLOCKCHAIN_DATA_MODEL.md  # Data structures (SPEC 4, 5)
└── GENESIS_TESTING.md        # Testing procedures
```

## Version History

| Date | Change |
|------|--------|
| 2025-12-19 | **Network Event Loop Fix (v1.11)**: Main event loop now polls network - fixes mDNS/genesis responses |
| 2025-12-19 | **mDNS-Only Discovery**: Allow joining networks via mDNS without requiring bootnodes |
| 2025-12-19 | **mDNS Fix**: Auto-dial discovered peers, swarm polling via `poll_once()`, `is_connected()` helper |
| 2025-12-19 | **Genesis Exchange Protocol**: Joining nodes receive genesis from network before init |
| 2025-12-19 | **Genesis Mode**: Added `--genesis` flag - creates new network; without flag joins via DNS Seeds |
| 2025-12-19 | **DNS Seeds**: Decentralized peer discovery - nodes auto-connect without manual bootnode config |
| 2025-12-19 | **Bootstrap Node**: Added hardcoded fallback bootnode (78.240.168.225) |
| 2025-12-19 | Added KRATOS_SYNTHESIS.md with Clock Health & Drift Tracking |
| 2025-12-19 | Unified architecture - removed dev/devnet/testnet/mainnet modes |
| 2025-12-19 | Created README.md for synthese directory |

## Implementation Reference

Source code: `rust/kratos-core/src/`

All synthesis documents reference the actual implementation to ensure accuracy.
