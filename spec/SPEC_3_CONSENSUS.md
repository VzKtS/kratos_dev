# SPEC 3: Consensus Mechanism

**Version:** 1.0
**Status:** Normative
**Last Updated:** 2025-12-19

---

## 1. Overview

KratOs uses a Proof-of-Stake (PoS) consensus mechanism with VRF-based validator selection and GRANDPA-style finality.

**Design Principles:**
- **Weighted randomness:** Selection proportional to stake and reputation (VC)
- **Finality:** Blocks finalized with 2/3+ validator signatures
- **Predictability:** Deterministic selection from randomness source

---

## 2. Time Parameters

### 2.1 Block Time

| Parameter | Value |
|-----------|-------|
| Block time | 6 seconds |
| Blocks per epoch | 600 |
| Epoch duration | 1 hour |
| Epochs per day | 24 |
| Epochs per year | 8,760 |

### 2.2 Slot Structure

Each epoch contains 600 slots:

```
Epoch N: [Slot 0, Slot 1, ... Slot 599]
         ↓
         Block production window
```

---

## 3. Block Structure

### 3.1 Block Components

```
Block
├── BlockHeader
│   ├── number          (block height)
│   ├── parent_hash     (previous block hash)
│   ├── transactions_root (Merkle root)
│   ├── state_root      (post-execution state)
│   ├── timestamp       (Unix timestamp)
│   ├── epoch           (epoch number)
│   ├── slot            (slot within epoch)
│   ├── author          (validator AccountId)
│   └── signature       (Ed25519 signature)
└── BlockBody
    └── transactions: Vec<SignedTransaction>
```

### 3.2 Block Hash

Block hash excludes signature for verification:

```
hash = H(number || parent_hash || transactions_root ||
         state_root || timestamp || epoch || slot || author)
```

---

## 4. Validator Selection

### 4.1 VRF-Weighted Selection

Validators are selected using VRF with stake and VC weighting:

```
VRF_weight = min(sqrt(stake), sqrt(STAKE_CAP)) × ln(1 + VC)
```

### 4.2 Selection Process

1. Compute slot randomness: `seed = H(epoch_randomness || slot_number)`
2. For each validator, compute VRF output
3. Weight VRF output by `VRF_weight`
4. Select validator with highest weighted score

### 4.3 Randomness Source

Epoch randomness derived from previous epoch's first block:

```
epoch_randomness(N) = hash(block_0_of_epoch(N-1))
epoch_randomness(0) = ZERO  // Genesis epoch
```

---

## 5. Block Production

### 5.1 Producer Responsibilities

Selected validator must:

1. Gather pending transactions from mempool
2. Execute transactions and compute state root
3. Build block header with correct fields
4. Sign block with validator key
5. Broadcast block to network

### 5.2 Block Validation

Nodes validate received blocks:

| Check | Validation |
|-------|------------|
| Parent | parent_hash matches chain head |
| Height | number = parent.number + 1 |
| Author | validator was selected for slot |
| Signature | Ed25519 signature valid |
| Transactions | All transactions valid |
| State root | Matches post-execution state |
| Timestamp | Within acceptable drift |

---

## 6. Finality

### 6.1 GRANDPA-Style Finality

Blocks are finalized with supermajority agreement:

| Parameter | Value |
|-----------|-------|
| Threshold | >= 2/3 validators |
| Data | FinalityJustification |

### 6.2 Justification Structure

```rust
pub struct FinalityJustification {
    pub block_number: BlockNumber,
    pub block_hash: Hash,
    pub signatures: Vec<ValidatorSignature>,
    pub epoch: EpochNumber,
}
```

### 6.3 Finality Properties

- **Safety:** Finalized blocks cannot be reverted
- **Liveness:** Eventually all blocks get finalized
- **Determinism:** Same justification on all nodes

---

## 7. Fork Choice

### 7.1 Longest Chain Rule

Before finality, nodes follow longest valid chain.

### 7.2 Finality Override

Finalized blocks take precedence:
- If finality justification exists, that chain wins
- Reorgs cannot cross finalized blocks

---

## 8. Block Rewards

### 8.1 Reward Calculation

Block producer receives:

```
reward = (total_supply × inflation_rate) / blocks_per_year
```

### 8.2 Distribution

See SPEC 1 (Tokenomics) for detailed reward distribution.

---

## 9. Missed Slots

### 9.1 Handling Missing Blocks

If selected validator fails to produce:

1. Slot remains empty
2. Next slot continues normally
3. Validator uptime credits affected

### 9.2 Uptime Impact

| Participation | Effect |
|---------------|--------|
| >= 95% | +1 VC per epoch |
| < 95% | No VC for epoch |
| Extended absence | Potential slashing |

---

## 10. Security Properties

### 10.1 Byzantine Fault Tolerance

- Tolerates < 1/3 malicious validators
- Finality requires 2/3+ honest
- Selection unpredictable until slot

### 10.2 Attack Resistance

| Attack | Mitigation |
|--------|------------|
| Grinding | VRF randomness non-malleable |
| Long-range | Finality checkpoints |
| Nothing-at-stake | Slashing for equivocation |

---

## 11. Implementation

### 11.1 Source Files

| File | Contents |
|------|----------|
| `consensus/pos.rs` | Validator selection |
| `consensus/epoch.rs` | Epoch management |
| `consensus/vrf_selection.rs` | VRF integration |
| `types/block.rs` | Block structure |
| `node/producer.rs` | Block production |

### 11.2 Key Structures

```rust
pub struct ValidatorSelector {
    validator_set: ValidatorSet,
    epoch_config: EpochConfig,
}

pub struct RandomnessProvider {
    block_hashes: HashMap<BlockNumber, Hash>,
}
```

---

## 12. Related Specifications

- **SPEC 1:** Tokenomics - Block rewards
- **SPEC 2:** Validator Credits - Selection weighting
- **SPEC 6:** Network Security - Validator thresholds
