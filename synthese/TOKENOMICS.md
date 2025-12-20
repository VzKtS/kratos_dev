# KratOs Tokenomics - Complete Economic Model

## Overview

KratOs implements a sophisticated tokenomics system designed to balance early-stage network security with long-term deflationary economics. The model combines predictable issuance decay with growing burn mechanisms to create sustainable value accrual.

**Implementation**: Native Rust (not Substrate-based)
**Source Code**: `rust/kratos-core/src/contracts/krat.rs`, `rust/kratos-core/src/consensus/economics.rs`

---

## Core Token Specifications

| Property | Value |
|----------|-------|
| **Token Symbol** | KRAT |
| **Decimals** | 12 |
| **Base Unit** | 1 KRAT = 10^12 base units |
| **Initial Supply** | 1,000,000,000 KRAT (1 billion) |
| **Block Time** | 6 seconds |
| **Existential Deposit** | 1 milliKRAT (anti-spam) |

---

## Bootstrap Era (First 60 Days)

The network begins in a **Bootstrap Era** with special economics to incentivize early validators.

### Bootstrap Configuration (Mainnet)

| Parameter | Value | Description |
|-----------|-------|-------------|
| **Duration** | 1,440 epochs (60 days) | 1 epoch = 1 hour = 600 blocks |
| **Target Inflation** | 6.5% annual | Fixed rate during bootstrap |
| **Min Validators to Exit** | 50 | Constitutional minimum |
| **Min Total Stake to Exit** | 25,000,000 KRAT | Alternative exit condition |
| **Min Stake (Bootstrap)** | 50,000 KRAT | Floor during bootstrap |
| **Min Stake (Post-Bootstrap)** | 25,000 KRAT | Floor after bootstrap |

### Bootstrap Exit Conditions

Bootstrap exits when **ALL** of these are met:
1. Epoch >= 1,440 (60 days elapsed)
2. Active validators >= 50
3. Average participation >= 90% (last 100 epochs)

**Safety Constraint**: Network CANNOT exit bootstrap if validators < 50 (constitutional minimum).

### Block Rewards During Bootstrap

```
BlockReward = (TotalSupply × InflationRate) / BlocksPerYear
           = (1,000,000,000 KRAT × 0.065) / 5,256,000
           = ~12.37 KRAT per block
```

Where: `BlocksPerYear = 600 blocks/epoch × 8,760 epochs/year = 5,256,000`

### Early Validator Incentives (VC Multipliers)

Early validators who join during the Bootstrap Era receive **enhanced Validator Credits (VC)** to reward their contribution to network security during the critical launch phase.

**Source**: `rust/kratos-core/src/consensus/economics.rs` - `BootstrapConfig`

#### VC Multipliers During Bootstrap

| Activity | Bootstrap Multiplier | Post-Bootstrap |
|----------|---------------------|----------------|
| **Vote Credits** | 2x | 1x |
| **Uptime Credits** | 2x | 1x |
| **Arbitration Credits** | 1x | 1x |

```rust
// From BootstrapConfig
pub vc_vote_multiplier: u32,      // 2 during bootstrap
pub vc_uptime_multiplier: u32,    // 2 during bootstrap
pub vc_arbitration_multiplier: u32, // 1 (no boost)
```

#### How VC Multipliers Work

1. **During Bootstrap (epochs 0-1439)**:
   - Every vote earns 2 VC instead of 1
   - Every uptime checkpoint earns 2 VC instead of 1
   - Validators accumulate VC faster

2. **After Bootstrap (epoch >= 1440)**:
   - Multipliers reset to 1x
   - VC accumulation returns to normal rate

#### Economic Impact

Early validators benefit from:

| Benefit | Description |
|---------|-------------|
| **Faster VC Accumulation** | 2x VC means faster stake reduction eligibility |
| **Higher Block Rewards** | Fixed 6.5% inflation (vs adaptive 0.5-10% post-bootstrap) |
| **Lower Effective Stake** | VC-based stake reduction kicks in earlier |
| **First-Mover Advantage** | Accumulated VC persists after bootstrap ends |

#### Example: Early Validator Advantage

**Validator joining at epoch 0 (day 1)**:
- 1440 epochs of 2x VC multiplier
- Potential VC at bootstrap end: ~2,880 VC (with perfect participation)
- Stake reduction: up to 99% during bootstrap

**Validator joining at epoch 1440 (day 61)**:
- 0 epochs of 2x multiplier (bootstrap ended)
- Starting VC: 0
- Must earn VC at 1x rate

---

## Post-Bootstrap Economics

### Adaptive Inflation System (InflationCalculator)

After bootstrap, KratOs uses a **dynamic adaptive inflation** system that adjusts based on real-time network metrics. This is fundamentally different from fixed-rate inflation models.

**Source**: `rust/kratos-core/src/consensus/economics.rs` - `InflationCalculator`

#### Core Formula

```
AnnualEmission = BaseSecurityBudget × SecurityGapFactor × ActivityFactor
```

Bounded by: `min_inflation (0.5%) ≤ EffectiveRate ≤ max_inflation (10%)`

#### Inflation Configuration

```rust
pub struct InflationConfig {
    pub target_validators: u32,        // 100 validators
    pub avg_validator_cost: Balance,   // 10,000 KRAT/year per validator
    pub target_stake_ratio: f64,       // 0.30 (30% of supply staked)
    pub target_active_users: u64,      // 10,000 users
    pub min_inflation: f64,            // 0.005 (0.5%)
    pub max_inflation: f64,            // 0.10 (10%)
}
```

#### Base Security Budget

The foundational emission target based on validator economics:

```
BaseSecurityBudget = target_validators × avg_validator_cost
                   = 100 × 10,000 KRAT
                   = 1,000,000 KRAT/year
```

#### Security Gap Factor

Adjusts inflation based on network security (staking ratio):

```
SecurityGapFactor = target_stake_ratio / actual_stake_ratio
```

| Staking Ratio | Factor | Effect |
|---------------|--------|--------|
| 10% (under-secured) | 1.5 (capped) | +50% emission to attract stakers |
| 30% (target) | 1.0 | Baseline emission |
| 50% (over-secured) | 0.6 | -40% emission (sufficient security) |
| 100% (max) | 0.3 (floor) | Minimum emission |

**Bounds**: Clamped to [0.3, 1.5]

#### Activity Factor

Adjusts inflation based on network usage:

```
ActivityFactor = sqrt(active_users / target_active_users)
```

| Active Users | Factor | Effect |
|--------------|--------|--------|
| 1,000 (low) | 0.5 (floor) | Reduced emission |
| 10,000 (target) | 1.0 | Baseline |
| 40,000 (high) | 1.2 (cap) | +20% to support growth |

**Bounds**: Clamped to [0.5, 1.2]

### Network Metrics Input

The `InflationCalculator` uses real-time `NetworkMetrics`:

```rust
pub struct NetworkMetrics {
    pub total_supply: Balance,       // Current circulating supply
    pub total_staked: Balance,       // Total KRAT staked
    pub active_validators: u32,      // Current validator count
    pub active_users: u64,           // Recent active addresses
    pub transactions_count: u64,     // Recent transaction count
}
```

### Example Calculations

**Scenario 1: Healthy Network**
- Supply: 1B KRAT, Staked: 300M (30%), Users: 10,000
- SecurityGapFactor = 0.30/0.30 = 1.0
- ActivityFactor = sqrt(10000/10000) = 1.0
- Emission = 1M × 1.0 × 1.0 = 1M KRAT/year (0.1%)

**Scenario 2: Under-Secured Network**
- Supply: 1B KRAT, Staked: 100M (10%), Users: 5,000
- SecurityGapFactor = 0.30/0.10 = 3.0 → capped to 1.5
- ActivityFactor = sqrt(5000/10000) = 0.707
- Emission = 1M × 1.5 × 0.707 = 1.06M KRAT/year

**Scenario 3: Over-Secured, High Activity**
- Supply: 1B KRAT, Staked: 500M (50%), Users: 50,000
- SecurityGapFactor = 0.30/0.50 = 0.6
- ActivityFactor = sqrt(50000/10000) = 2.24 → capped to 1.2
- Emission = 1M × 0.6 × 1.2 = 720K KRAT/year

### Static Emission Constants (krat.rs)

For emission period calculations and long-term projections:

```rust
pub const INITIAL_EMISSION_RATE_BPS: u32 = 500;   // 5% initial
pub const MIN_EMISSION_RATE_BPS: u32 = 50;        // 0.5% floor
pub const EMISSION_PERIOD_BLOCKS: BlockNumber = 432_000; // 30 days
pub const EMISSION_HALF_LIFE_YEARS: f64 = 5.0;
```

### Burn Schedule (Growing Toward Maximum)

Annual burn rate grows with network activity:

```
b(t) = b_max - (b_max - b_0) × e^(-g×t)
```

Where:
- `b_0` = 1.0% (initial burn rate)
- `b_max` = 3.5% (maximum burn rate)
- `g` = 0.25 (growth speed)

### Emission Period

Tokens are emitted in 30-day periods:
- **Emission Period**: 432,000 blocks (30 days at 6s/block)
- **Per-Period Emission**: Calculated from `InflationCalculator.calculate_epoch_emission()`

---

## Fee Distribution

Transaction fees follow the **60/30/10 rule** (SPEC v3.1):

| Recipient | Share | Description |
|-----------|-------|-------------|
| **Validators** | 60% | Block producer rewards |
| **Burn** | 30% | Permanently removed from circulation |
| **Treasury** | 10% | Community-governed development fund |

```rust
// From economics.rs
pub fn default_distribution() -> FeeDistribution {
    FeeDistribution {
        validators_share: 0.60,
        burn_share: 0.30,
        treasury_share: 0.10,
    }
}
```

---

## Emission Distribution

Newly minted tokens each emission period are distributed:

| Recipient | Allocation | Description |
|-----------|------------|-------------|
| **Validators** | 70% | Block production and consensus rewards |
| **Treasury** | 20% | Ecosystem development fund |
| **Reserve** | 10% | Protocol emergency fund |

---

## Validator Staking Economics

### Stake Requirements

Based on Validator Credits (VC) system:

```
VC_norm = min(TotalVC / 5000, 1.0)
StakeReduction = MaxReduction × VC_norm
RequiredStake = max(NominalStake × (1 − StakeReduction), StakeFloor)
```

| Parameter | Bootstrap | Post-Bootstrap |
|-----------|-----------|----------------|
| **Nominal Stake** | 500,000 KRAT | 500,000 KRAT |
| **VC Target** | 5,000 | 5,000 |
| **Max Reduction** | 99% | 95% |
| **Stake Floor** | 50,000 KRAT | 25,000 KRAT |

### Unbonding Period

- **Duration**: 28 days (UNBONDING_PERIOD constant)
- **Funds locked** during unbonding, no rewards

---

## Slashing Penalties

Validators face reputation (VC) and economic (stake) penalties for misbehavior:

### Severity Levels

| Severity | VC Slash | Stake Slash | Cooldown |
|----------|----------|-------------|----------|
| **Critical** | 50% | 5-20% | 52 epochs (~1 year) |
| **High** | 25% | 1-5% | 12 epochs (~3 months) |
| **Medium** | 10% | 0-1% | None |
| **Low** | 5% | 0% | None |

### Slashable Events

| Event | Severity | Description |
|-------|----------|-------------|
| **Double Signing** | Critical | Two blocks for same slot |
| **Equivocation** | Critical | Conflicting votes |
| **Arbitration Misconduct** | High | Bad faith arbitration |
| **Invalid Governance Execution** | High | Improper proposal execution |
| **Extended Downtime** | Medium/Low | >= 12 epochs: Medium, < 12: Low |
| **Low Participation** | Medium/Low | < 50%: Medium, >= 50%: Low |

---

## Network Security States

The network transitions through security states based on validator count (SPEC v7.1):

| State | Validator Range | Effects |
|-------|-----------------|---------|
| **Bootstrap** | Any (epoch < 1440) | Fixed 6.5% inflation, building validator set |
| **Normal** | >= 75 | Full functionality, normal inflation |
| **Degraded** | 50-74 | Inflation +1%, governance timelocks x2 |
| **Restricted** | 25-49 | Governance frozen, incentives boosted |
| **Emergency** | < 25 | Automatic emergency, fork allowed |

### Recovery Conditions

- **Normal Recovery**: 100 consecutive epochs at >= 75 validators
- **Collapse Detection**: 10 consecutive epochs below 50 validators triggers Bootstrap Recovery Mode

### Validator Limits (Constitutional Bounds)

The network enforces hard caps on validator counts per chain type:

| Chain Type | Maximum Validators | Constant | Rationale |
|------------|-------------------|----------|-----------|
| **Root Chain** | 101 | `MAX_VALIDATORS` | Constitutional limit for consensus performance |
| **Sidechain** | 100 | `MAX_VALIDATORS_PER_CHAIN` | Per-sidechain limit |
| **Host Chain** | 200 | `MAX_VALIDATORS_PER_HOST` | Aggregate limit for host + sidechains |
| **Network Global** | 1,000 | `max_validators` (config) | Total across all chains |

**Source**: `rust/kratos-core/src/types/protocol.rs`, `rust/kratos-core/src/types/chain.rs`

#### Why 101 Maximum on Root Chain?

1. **Consensus Performance**: More validators increase block finality latency
2. **Sufficient Decentralization**: 101 validators provides robust Byzantine fault tolerance (BFT requires < 1/3 malicious)
3. **Economic Sustainability**: Block rewards split among max 101 ensures viable validator economics
4. **Constitutional Bound**: Cannot be changed without constitutional amendment (Article III)

#### What Happens at 101 Validators?

- Network reaches **optimal state** (highest security level)
- **New validator registrations are rejected** on root chain
- Validators can join **sidechains** instead (up to 100 per sidechain)
- Existing validators can be replaced via **governance** if underperforming

#### Overflow to Sidechains

When root chain is full (101 validators):
```
New Validator → Root Chain Full? → Join Sidechain (max 100 per chain)
                     ↓
              Wait for slot or replace underperformer via governance
```

---

## 20-Year Economic Simulation

### Key Milestones

**Year 1** (Post-Bootstrap):
- Supply: ~1,048,000,000 KRAT (4.8% net growth)
- Issuance: ~48,000,000 KRAT
- Burn: ~13,000,000 KRAT

**Year 5** (Crossover Point):
- Supply: ~1,082,000,000 KRAT (peak)
- First deflationary year (burn > issuance)

**Year 10**:
- Supply: ~1,025,000,000 KRAT
- Net deflation: ~1.8%/year

**Year 20**:
- Supply: ~824,000,000 KRAT
- 17.6% below initial supply
- Net deflation: ~2.6%/year

---

## Implementation Constants

From `rust/kratos-core/src/contracts/krat.rs`:

```rust
/// Initial supply: 1 billion KRAT
pub const INITIAL_SUPPLY: Balance = 1_000_000_000 * KRAT;

/// Initial emission rate: 5% per year (500 basis points)
pub const INITIAL_EMISSION_RATE_BPS: u32 = 500;

/// Minimum emission rate: 0.5% per year (50 basis points)
pub const MIN_EMISSION_RATE_BPS: u32 = 50;

/// Initial burn rate: 1% per year (100 basis points)
pub const INITIAL_BURN_RATE_BPS: u32 = 100;

/// Maximum burn rate: 3.5% per year (350 basis points)
pub const MAX_BURN_RATE_BPS: u32 = 350;

/// Emission period: 30 days (432,000 blocks at 6s/block)
pub const EMISSION_PERIOD_BLOCKS: BlockNumber = 432_000;

/// Emission half-life: 5 years
pub const EMISSION_HALF_LIFE_YEARS: f64 = 5.0;
```

From `rust/kratos-core/src/consensus/economics.rs`:

```rust
/// Bootstrap inflation rate
pub const BOOTSTRAP_TARGET_INFLATION: f64 = 0.065; // 6.5%

/// Validator thresholds (SPEC v7.1)
pub const EMERGENCY_VALIDATORS: u32 = 25;
pub const POST_BOOTSTRAP_MIN_VALIDATORS: u32 = 50;
pub const SAFE_VALIDATORS: u32 = 75;
pub const OPTIMAL_VALIDATORS: u32 = 101;

/// Adaptive Inflation Defaults (InflationConfig)
// target_validators: 100
// avg_validator_cost: 10,000 KRAT/year
// target_stake_ratio: 0.30 (30%)
// target_active_users: 10,000
// min_inflation: 0.005 (0.5%)
// max_inflation: 0.10 (10%)
```

---

## Comparison to Other Blockchains

| Feature | KratOs | Bitcoin | Ethereum | Polkadot |
|---------|--------|---------|----------|----------|
| **Initial Supply** | 1B KRAT | 21M BTC | Unlimited | 1B DOT |
| **Issuance** | Adaptive (0.5% - 10%) | Halving | Fixed ~0.5% | Fixed ~10% |
| **Issuance Model** | Security + Activity based | Time-based halving | Fixed rate | Fixed rate |
| **Burn** | Growing (1% → 3.5%) | None | EIP-1559 | Treasury only |
| **Long-term** | Deflationary | Deflationary | Variable | Inflationary |
| **Minimum Issuance** | 0.5%/year perpetual | Zero (after 2140) | None | None |

---

## Economic Rationale

1. **Early Security Funding**: 6.5% bootstrap inflation funds initial validator rewards
2. **Adaptive Security**: Inflation increases when network is under-secured (low stake ratio)
3. **Activity-Responsive**: Emission adjusts based on actual network usage
4. **Activity-Driven Burn**: Burn grows with network usage (1% → 3.5%)
5. **Long-Term Deflation**: Eventually burn > issuance
6. **Perpetual Minimum**: 0.5% floor ensures validator funding forever
7. **VC-Based Reduction**: Reputation lowers stake requirements up to 95%
8. **Self-Balancing**: Over-secured networks automatically reduce inflation

---

## Source Files Reference

| File | Contents |
|------|----------|
| [krat.rs](../rust/kratos-core/src/contracts/krat.rs) | Token constants, emission calculation, TokenomicsState |
| [economics.rs](../rust/kratos-core/src/consensus/economics.rs) | Bootstrap config, fee distribution, network states |
| [slashing.rs](../rust/kratos-core/src/consensus/slashing.rs) | Slashing penalties and severity levels |
| [validator.rs](../rust/kratos-core/src/consensus/validator.rs) | Validator staking and unbonding |
| [producer.rs](../rust/kratos-core/src/node/producer.rs) | Block rewards calculation |

---

**Implementation Status**: Complete
**Last Updated**: 2025-12-19
**Specification Version**: Unified (see [KRATOS_SYNTHESIS.md](KRATOS_SYNTHESIS.md))
