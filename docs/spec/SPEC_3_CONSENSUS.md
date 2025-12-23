# SPEC 3: Consensus Mechanism

**Version:** 2.1
**Status:** Normative
**Last Updated:** 2025-12-22

### Changelog
| Version | Date | Changes |
|---------|------|---------|
| 2.2 | 2025-12-22 | Added §6.10 Node Integration, §6.11 Fee Distribution to Finality Voters |
| 2.1 | 2025-12-22 | Added §5.3 State-Aware Transaction Selection (mempool nonce fix) |
| 2.0 | 2025-12-22 | Added §12.5 Sync Rate-Limiting (cross-ref SPEC 6 §18) |
| 1.9 | 2025-12-21 | Fixed deadlock in import_block() - VC initialization for early validators now uses outer storage lock |
| 1.8 | 2025-12-21 | Added §18 Genesis State Verification (state root verification + idempotent import) |
| 1.7 | 2025-12-21 | Fixed joining node VC initialization in apply_received_genesis_state() |
| 1.6 | 2025-12-21 | Fixed genesis validator VC initialization (100 VC on creation for VRF eligibility) |
| 1.5 | 2025-12-21 | Added §16 VRF Slot Selection, §17 Bootstrap VC Initialization |
| 1.4 | 2025-12-21 | Added §15 Timestamp Validation - Incremental Drift Model |
| 1.3 | 2025-12-21 | Added §13.4 Transaction Execution Flow |
| 1.2 | 2025-12-21 | Added §12 Block Synchronization |
| 1.0 | 2025-12-21 | Initial specification |

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

1. Gather pending transactions from mempool (state-aware selection)
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

### 5.3 State-Aware Transaction Selection

When selecting transactions from the mempool for block inclusion, the producer MUST use state-aware nonce tracking to correctly handle accounts that have already executed transactions.

**The Problem:**

The basic `select_transactions()` function initializes expected nonce to 0 for each account. This causes transactions to be skipped when an account's on-chain nonce is > 0:

```rust
// BUG: Starts at nonce 0, ignoring on-chain state
let expected_nonce = account_nonces.get(&sender).copied().unwrap_or(0);
if entry.nonce > expected_nonce {
    continue;  // Skips valid transactions with nonce > 0!
}
```

**The Solution:**

Use `select_transactions_with_state()` which queries the actual account nonce from chain state:

```rust
// CORRECT: Gets real nonce from state
let expected_nonce = state.get_account(&sender)
    .map(|a| a.nonce)
    .unwrap_or(0);
```

**Implementation in produce_block():**

```rust
let transactions = {
    let mempool_guard = mempool.read().await;
    let mut state_guard = state.write().await;
    mempool_guard.select_transactions_with_state(
        self.config.max_transactions_per_block,
        &mut state_guard,
    )
};
```

**Source:** `node/producer.rs:1083-1093`, `node/mempool.rs:775-825`

---

## 6. Finality

### 6.1 GRANDPA-Style Finality

Blocks are finalized with supermajority agreement using a two-phase voting protocol:

| Parameter | Value |
|-----------|-------|
| Threshold | >= 2/3 validators (66%) |
| Round Timeout | 6 seconds (1 block time) |
| Data | FinalityJustification |

**Implementation:** `consensus/finality/mod.rs`

### 6.2 Finality Protocol

The finality process uses GRANDPA-style rounds:

1. **Prevote Phase:** Validators broadcast preferred block to finalize
2. **Precommit Phase:** After 2/3 prevotes, validators precommit
3. **Finalization:** When 2/3 precommits collected, block is finalized

```rust
pub enum VoteType {
    Prevote,    // Phase 1: Intent to finalize
    Precommit,  // Phase 2: Commit after supermajority prevotes
}

pub struct FinalityVote {
    pub vote_type: VoteType,
    pub target_number: BlockNumber,
    pub target_hash: Hash,
    pub round: u32,
    pub epoch: EpochNumber,
    pub voter: AccountId,
    pub signature: Signature64,
}
```

**Source:** `consensus/finality/types.rs`

### 6.3 Justification Structure

```rust
pub struct FinalityJustification {
    pub block_number: BlockNumber,
    pub block_hash: Hash,
    pub signatures: Vec<ValidatorSignature>,
    pub epoch: EpochNumber,
}
```

**Source:** `types/block.rs:169-243`

### 6.4 Vote Collection

The `VoteCollector` aggregates votes and detects equivocation:

```rust
pub struct VoteCollector {
    validators: HashSet<AccountId>,
    prevotes: HashMap<(BlockNumber, Hash), Vec<FinalityVote>>,
    precommits: HashMap<(BlockNumber, Hash), Vec<FinalityVote>>,
    state: RoundState,  // Prevoting -> Precommitting -> Completed
}
```

**Source:** `consensus/finality/votes.rs`

### 6.5 Equivocation Detection

Validators voting for different blocks in the same round are detected:

```rust
pub struct EquivocationProof {
    pub validator: AccountId,
    pub vote1: FinalityVote,
    pub vote2: FinalityVote,
    pub round: u32,
    pub epoch: EpochNumber,
}
```

Equivocation leads to slashing as defined in SPEC v5.

### 6.6 Domain Separation

All finality signatures use domain separation:

```rust
pub const DOMAIN_FINALITY: &[u8] = b"KRATOS_FINALITY_V1:";
```

This prevents cross-context signature replay attacks.

**Source:** `types/signature.rs:68-71`

### 6.7 Finality Properties

- **Safety:** Finalized blocks cannot be reverted (with 2/3 honest validators)
- **Liveness:** Eventually all blocks get finalized
- **Determinism:** Same justification on all nodes
- **Byzantine Tolerance:** Tolerates < 1/3 malicious validators

### 6.8 Network Messages

Finality uses dedicated gossip topic `/kratos/finality/1.0.0`:

| Message | Purpose |
|---------|---------|
| `FinalityVote` | Broadcast prevote/precommit |
| `FinalityJustification` | Announce finalization |
| `FinalityVotesRequest` | Request votes for catch-up |
| `FinalityVotesResponse` | Respond with historical votes |

**Source:** `network/protocol.rs:106-128`

### 6.9 RPC Methods

| Method | Description |
|--------|-------------|
| `finality_getStatus` | Get finality state |
| `finality_getLastFinalized` | Get last finalized block |
| `finality_getJustification` | Get justification for block |
| `finality_getRoundInfo` | Get current round info |

**Source:** `rpc/methods.rs:876-980`

### 6.10 Node Integration

The finality gadget is integrated into the node service via `FinalityIntegration`:

```rust
/// Node-level finality coordinator
pub struct FinalityIntegration<S: FinalitySigner, B: FinalityBroadcaster> {
    gadget: RwLock<FinalityGadget<S, B>>,
    last_finality_voters: RwLock<Vec<AccountId>>,
    last_finalized: RwLock<BlockNumber>,
    is_active: RwLock<bool>,
}
```

**Initialization:** Finality is initialized for validators after node startup:

```rust
// In runner.rs
if let Some(ref key) = validator_key {
    node.initialize_finality(key.clone()).await;
}
```

**Block Import Integration:** After each block is imported, finality is notified:

```rust
// In service.rs - import_block() and store_produced_block()
self.notify_finality_block_imported(block.header.number, block_hash).await;
```

**Finality Tick Loop:** Periodic tick for timeout handling:

```rust
// In main event loop (runner.rs)
_ = finality_tick_interval.tick() => {
    node.tick_finality().await;
    node.broadcast_finality_messages().await;
}
```

**Source:** `node/finality_integration.rs`, `node/service.rs:1850-2000`, `cli/runner.rs:450-480`

### 6.11 Fee Distribution to Finality Voters

Validators who participate in block finalization receive 10% of transaction fees:

| Recipient | Share | Description |
|-----------|-------|-------------|
| Block Producer | 50% | Primary reward for block production |
| **Finality Voters** | **10%** | Shared equally among precommit signers |
| Burn | 30% | Deflationary mechanism |
| Treasury | 10% | Protocol development fund |

**Distribution Logic:**

```rust
// In economics.rs
pub fn distribute_fees(
    total_fees: Balance,
    producer: AccountId,
    finality_voters: &[AccountId],
) -> FeeDistributionResult {
    let producer_share = total_fees * 50 / 100;
    let finality_share = total_fees * 10 / 100;
    let burn_share = total_fees * 30 / 100;
    let treasury_share = total_fees * 10 / 100;

    // Finality reward divided equally among voters
    let per_voter = if !finality_voters.is_empty() {
        finality_share / finality_voters.len() as u128
    } else {
        0
    };

    // ...
}
```

**Source:** `consensus/economics.rs:180-250`

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

## 12. Block Synchronization

### 12.1 Sync Race Condition Handling

During initial synchronization, blocks may arrive out of order due to network latency. The protocol handles this gracefully:

| Condition | Action |
|-----------|--------|
| Block already imported | Ignore (duplicate) |
| Block is next sequential | Import immediately |
| Block ahead of local height | Buffer for later |
| Block behind local height | Ignore (stale) |

### 12.2 Block Buffering

Out-of-order blocks are stored in the `SyncManager` and imported when sequential:

```
Receive Block #5 (local height: 2)
    → Buffer block #5

Receive Block #3 (local height: 2)
    → Import block #3
    → Check buffer: no #4, stop

Receive Block #4 (local height: 3)
    → Import block #4
    → Check buffer: found #5, import
    → Check buffer: no #6, stop
```

### 12.3 Selective Peer Banning

Peers are only banned for **validation failures**, not sequencing issues:

| Error Type | Action |
|------------|--------|
| Invalid signature | Ban peer |
| Invalid parent hash | Ban peer |
| Invalid transactions root | Ban peer |
| Block number mismatch | Log only |
| Duplicate block | Ignore |

### 12.4 Genesis Timestamp

All slot calculations use the **genesis block timestamp** as the canonical time reference:

```
slot = ((block_timestamp - genesis_timestamp) / SLOT_DURATION) % SLOTS_PER_EPOCH
```

This ensures all nodes on the network compute the same expected slot for any given block.

### 12.5 Sync Rate-Limiting

To prevent sync request storms during high gossip activity, synchronization is rate-limited:

| Parameter | Value |
|-----------|-------|
| Minimum interval | 500ms between requests |
| Max pending requests | 3 concurrent |
| Batch size | 50 blocks |

See **SPEC 6 §18** for full implementation details.

---

## 13. Early Validator Onboarding

### 13.1 Bootstrap Voting

During the bootstrap era (first 60 days), new validators can be added through voting:

| Parameter | Value |
|-----------|-------|
| Votes required | 3 |
| Who can vote | Active validators |
| When | Bootstrap era only |

### 13.2 Transaction Types

| Transaction | Description |
|-------------|-------------|
| `ProposeEarlyValidator` | Nominate a candidate |
| `VoteEarlyValidator` | Vote for a pending candidate |

### 13.3 Approval Process

1. Active validator proposes candidate (counts as 1st vote)
2. Other active validators vote for candidate
3. At 3 votes, candidate becomes active validator
4. All pending candidates are rejected when bootstrap ends

### 13.4 Transaction Execution Flow

Early validator transactions follow a two-phase execution model:

| Phase | Location | Action |
|-------|----------|--------|
| 1. Fee & Validation | `producer.rs` | Deduct fees, validate sender |
| 2. State Change | `service.rs` | Execute on ValidatorSet |

**Implementation:**

```rust
// Phase 1: In producer.rs - TransactionExecutor::execute()
TransactionCall::ProposeEarlyValidator { .. } => Ok(())  // Just pass validation
TransactionCall::VoteEarlyValidator { .. } => Ok(())     // Fee deducted after

// Phase 2: In service.rs - import_block() and store_produced_block()
for tx in block.body.transactions.iter() {
    match &tx.transaction.call {
        TransactionCall::ProposeEarlyValidator { candidate } => {
            validators.propose_early_validator(*candidate, tx.sender, block_number);
        }
        TransactionCall::VoteEarlyValidator { candidate } => {
            validators.vote_early_validator(*candidate, tx.sender, block_number);
        }
        _ => {}
    }
}
```

This two-phase approach is necessary because:
- `TransactionExecutor` operates on `StateBackend` (storage)
- `ValidatorSet` is a separate in-memory structure
- Early validator logic requires access to the full validator set

**Source:** `node/producer.rs:239-248`, `node/service.rs:851-900`, `node/service.rs:999-1046`

---

## 14. Block Production vs Import

### 14.1 The State Root Problem

When a validator produces a block:
1. Transactions are executed → state modified
2. Block rewards applied → state modified
3. State root computed and stored in block header

If the same block is then passed through `import_block()`, the state would be modified again, causing a state root mismatch.

### 14.2 Separate Code Paths

| Operation | Function | Description |
|-----------|----------|-------------|
| Produce block | `produce_block()` | Execute txs, apply rewards, compute state root |
| Store local block | `store_produced_block()` | Store only, no re-execution |
| Import remote block | `import_block()` | Re-execute all, validate state root |

### 14.3 store_produced_block()

For blocks produced locally, this function:
- Stores block to database
- Updates chain height
- Removes txs from mempool
- Broadcasts to peers
- Does NOT re-execute transactions or re-apply rewards

### 14.4 Invariant

**State modifications happen exactly once per block.** This ensures:
- Produced blocks: state root matches what was computed during production
- Imported blocks: state root matches after independent re-execution

---

## 15. Timestamp Validation

### 15.1 Incremental Drift Model

Timestamp validation uses an **incremental drift model** that compares time intervals rather than absolute timestamps. This approach:

- Works correctly during synchronization (no genesis timestamp required)
- Aligns with the DriftTracker in state management
- Allows nodes to sync historical chains without false failures

### 15.2 Validation Algorithm

```rust
// Calculate interval-based drift
slots_elapsed = block.slot - parent.slot          // Absolute slots
expected_interval = slots_elapsed × SLOT_DURATION // Expected time delta
actual_interval = block_ts - parent_ts            // Actual time delta
drift = actual_interval - expected_interval       // Signed drift
```

### 15.3 Validation Rules

| Check | Condition | Error |
|-------|-----------|-------|
| Timestamp order | block_ts > parent_ts | TimestampNotAfterParent |
| Future limit | block_ts <= now + 15s | TimestampTooFarInFuture |
| Minimum interval | interval >= 5s | TimestampTooCloseToParent |
| Slot consistency | \|drift\| <= 6s | TimestampSlotMismatch |

### 15.4 Key Insight

The slot field contains **absolute slot numbers** since genesis (e.g., slot 5107589), not relative slots within an epoch (0-599). The incremental model handles this correctly by computing slot differences.

**Source:** `node/producer.rs:544-571`

---

## 16. VRF Slot Selection

### 16.1 Slot Leader Selection

Only the VRF-selected validator produces a block for each slot. This prevents multiple validators from producing competing blocks.

**Selection Check in try_produce_block():**

```rust
// Before producing, check if we are the slot leader via VRF
let producer = BlockProducer::new(None, producer_db.clone());
match producer.is_slot_leader(slot, epoch, &validators, &validator_id, &storage) {
    Ok(true) => {
        // We are the slot leader, proceed with block production
    }
    Ok(false) => {
        // Not our turn to produce
        return Ok(None);
    }
    Err(e) => {
        // VRF check failed, skip production
        return Ok(None);
    }
}
```

### 16.2 is_slot_leader() Implementation

The slot leader check uses VRF selection with stake and VC weighting:

1. Build candidate list from active validators
2. Compute VRF weight for each: `min(sqrt(stake), sqrt(STAKE_CAP)) × ln(1 + VC)`
3. Use `VRFSelector::select_validator(slot, epoch, &candidates)` to determine leader
4. Compare selected validator with local validator ID

### 16.3 Key Invariant

**Exactly one validator produces per slot.** This ensures:
- No competing blocks at the same height
- Deterministic leader selection across all nodes
- Network consensus on which validator should produce

**Source:** `node/service.rs:1459-1500`, `node/producer.rs:1013-1050`

---

## 17. Bootstrap VC Initialization

### 17.1 The Problem

Bootstrap validators (stake=0) require minimum VC to participate in VRF selection:

```rust
// In vrf_selection.rs
const BOOTSTRAP_MIN_VC_REQUIREMENT: u64 = 100;

if stake == 0 && validator_credits < BOOTSTRAP_MIN_VC_REQUIREMENT {
    return 0.0; // No VRF weight - cannot be selected
}
```

New early validators start with 0 VC, so they have zero VRF weight and are never selected.

### 17.2 Solution

When a bootstrap validator is created or approved, initialize their VC record with 100 uptime credits:

```rust
// In storage/state.rs
pub fn initialize_bootstrap_vc(
    &mut self,
    validator_id: AccountId,
    block_number: BlockNumber,
    current_epoch: EpochNumber,
) -> Result<(), StateError> {
    let mut record = ValidatorCreditsRecord::new(block_number, current_epoch);
    record.uptime_credits = 100; // BOOTSTRAP_MIN_VC_REQUIREMENT
    self.set_vc_record(validator_id, record)
}
```

### 17.3 When VC is Initialized

VC initialization happens in multiple code paths:

| Location | Function | When |
|----------|----------|------|
| `genesis/spec.rs` | `GenesisBuilder::build()` | Genesis node: validator creation |
| `genesis/spec.rs` | `apply_to_state()` | Joining node: genesis spec initialization |
| `service.rs` | `apply_received_genesis_state()` | Joining node: network genesis initialization |
| `service.rs` | `import_block()` | After approve_early_validator() succeeds |
| `service.rs` | `store_produced_block()` | After approve_early_validator() succeeds |

**CRITICAL:** All code paths that initialize bootstrap validators MUST call `initialize_bootstrap_vc()` to ensure consistent state roots. Missing this call causes state root mismatch during block sync.

### 17.4 Lock Ordering

When initializing VC in `import_block()`, the code must reuse the already-held storage lock from the outer scope. Attempting to acquire a new `storage.write().await` while holding one causes a **deadlock** (tokio RwLock is not reentrant):

```rust
// WRONG - causes deadlock
let mut storage = self.storage.write().await;  // Outer lock
// ... later in early validator processing ...
drop(validators);
let mut storage = self.storage.write().await;  // DEADLOCK!

// CORRECT - reuse outer lock
let mut storage = self.storage.write().await;  // Outer lock
// ... later in early validator processing ...
storage.initialize_bootstrap_vc(...);  // Uses existing lock
```

### 17.5 Result

With 100 VC initialized:
- Bootstrap validator has non-zero VRF weight
- Can be selected as slot leader
- Participates fairly in block production rotation

**Source:** `storage/state.rs:746-761`, `node/service.rs:873-882`, `node/service.rs:1072-1081`

---

## 18. Genesis State Verification

### 18.1 State Root Verification for Joining Nodes

When a node joins an existing network, it receives genesis data from a peer and MUST verify that the locally computed state root matches the genesis block header's state_root:

```rust
// In apply_received_genesis_state()
let computed_root = state.compute_state_root(0, chain_id);
if computed_root.root != expected_state_root {
    return Err("Genesis state root mismatch");
}
```

### 18.2 Idempotent Block Import

The `import_block()` function includes an idempotency check to prevent cumulative state corruption if block import is retried:

```rust
// Check if block already exists in storage
if let Ok(Some(existing)) = storage.get_block_by_number(block_number) {
    if existing.hash() == block_hash {
        return Ok(()); // Already imported, skip
    }
}
```

This ensures that:
- A block is never processed twice
- Failed imports don't leave corrupted state for retry
- Network race conditions don't cause duplicate processing

**Source:** `node/service.rs:766-776`, `node/service.rs:1697-1709`

---

## 19. Related Specifications

- **SPEC 1:** Tokenomics - Block rewards
- **SPEC 2:** Validator Credits - Selection weighting
- **SPEC 6:** Network Security - Validator thresholds
