# SPEC 1: KratOs Tokenomics

**Version:** 1.0
**Status:** Normative
**Last Updated:** 2025-12-19

---

## 1. Overview

This specification defines the economic model of the KratOs blockchain, including token supply, inflation, burn mechanisms, fee distribution, and staking economics.

---

## 2. Token Specifications

### 2.1 Base Token

| Property | Value |
|----------|-------|
| Symbol | KRAT |
| Decimals | 12 |
| Base Unit | 1 KRAT = 10^12 base units |
| Initial Supply | 1,000,000,000 KRAT (1 billion) |
| Existential Deposit | 1 milliKRAT (0.001 KRAT) |

### 2.2 Block Parameters

| Parameter | Value |
|-----------|-------|
| Block Time | 6 seconds |
| Blocks per Epoch | 600 (1 hour) |
| Epochs per Year | 8,760 |
| Blocks per Year | 5,256,000 |

---

## 3. Bootstrap Era

### 3.1 Duration and Parameters

The network begins in a Bootstrap Era with fixed economics.

| Parameter | Value |
|-----------|-------|
| Duration | 1,440 epochs (60 days) |
| Target Inflation | 6.5% annual (fixed) |
| Min Stake | 50,000 KRAT |

### 3.2 Bootstrap Exit Conditions

Bootstrap exits when ALL conditions are met:

1. **Epoch >= 1,440** (60 days elapsed)
2. **Active validators >= 50** (POST_BOOTSTRAP_MIN_VALIDATORS)
3. **Average participation >= 90%** (last 100 epochs)

**INVARIANT:** Network CANNOT exit bootstrap if validators < 50.

### 3.3 Block Rewards During Bootstrap

```
BlockReward = (TotalSupply × InflationRate) / BlocksPerYear
           = (1,000,000,000 × 0.065) / 5,256,000
           ≈ 12.37 KRAT per block
```

### 3.4 Early Validator Incentives (VC Multipliers)

| Activity | Bootstrap Multiplier | Post-Bootstrap |
|----------|---------------------|----------------|
| Vote Credits | 2x | 1x |
| Uptime Credits | 2x | 1x |
| Arbitration Credits | 1x | 1x |

---

## 4. Post-Bootstrap Economics

### 4.1 Adaptive Inflation System

After bootstrap, inflation is calculated dynamically:

```
AnnualEmission = BaseSecurityBudget × SecurityGapFactor × ActivityFactor
```

**Bounds:** min_inflation (0.5%) ≤ EffectiveRate ≤ max_inflation (10%)

### 4.2 Inflation Configuration

| Parameter | Value |
|-----------|-------|
| target_validators | 100 |
| avg_validator_cost | 10,000 KRAT/year |
| target_stake_ratio | 0.30 (30%) |
| target_active_users | 10,000 |
| min_inflation | 0.5% |
| max_inflation | 10% |

### 4.3 Base Security Budget

```
BaseSecurityBudget = target_validators × avg_validator_cost
                   = 100 × 10,000 KRAT
                   = 1,000,000 KRAT/year
```

### 4.4 Security Gap Factor

Adjusts emission based on staking ratio:

```
SecurityGapFactor = target_stake_ratio / actual_stake_ratio
```

| Staking Ratio | Factor | Bounds |
|---------------|--------|--------|
| < target | > 1.0 | Capped at 1.5 |
| = target (30%) | 1.0 | Baseline |
| > target | < 1.0 | Floored at 0.3 |

### 4.5 Activity Factor

Adjusts emission based on network usage:

```
ActivityFactor = sqrt(active_users / target_active_users)
```

| Active Users | Factor | Bounds |
|--------------|--------|--------|
| Low (< 10k) | < 1.0 | Floored at 0.5 |
| Target (10k) | 1.0 | Baseline |
| High (> 10k) | > 1.0 | Capped at 1.2 |

---

## 5. Burn Mechanism

### 5.1 Burn Rate Growth

Burn rate grows asymptotically with network maturity:

```
b(t) = b_max - (b_max - b_0) × e^(-g × t)
```

| Parameter | Value |
|-----------|-------|
| b_0 (initial) | 1.0% |
| b_max (maximum) | 3.5% |
| g (growth speed) | 0.25 |

### 5.2 Long-Term Projection

| Period | Burn Rate |
|--------|-----------|
| Year 1 | ~1.3% |
| Year 5 | ~2.8% (crossover: burn > issuance) |
| Year 10 | ~3.4% |
| Year 20 | ~3.5% (maximum) |

---

## 6. Fee Distribution

Transaction fees follow the **60/30/10 rule**:

| Recipient | Share |
|-----------|-------|
| Validators | 60% |
| Burn | 30% |
| Treasury | 10% |

---

## 7. Emission Distribution

Newly minted tokens each emission period:

| Recipient | Share |
|-----------|-------|
| Validators | 70% |
| Treasury | 20% |
| Reserve | 10% |

**Emission Period:** 432,000 blocks (30 days)

---

## 8. Staking Economics

### 8.1 Stake Requirements

Based on Validator Credits (VC) system (see SPEC 2):

```
VC_norm = min(TotalVC / 5000, 1.0)
StakeReduction = MaxReduction × VC_norm
RequiredStake = max(NominalStake × (1 - StakeReduction), StakeFloor)
```

| Parameter | Bootstrap | Post-Bootstrap |
|-----------|-----------|----------------|
| Nominal Stake | 500,000 KRAT | 500,000 KRAT |
| VC Target | 5,000 | 5,000 |
| Max Reduction | 99% | 95% |
| Stake Floor | 50,000 KRAT | 25,000 KRAT |

### 8.2 Unbonding Period

| Parameter | Value |
|-----------|-------|
| Duration | 28 days |
| Status | Funds locked, no rewards |

---

## 9. Validator Limits

Constitutional bounds on validator counts:

| Chain Type | Maximum | Constant |
|------------|---------|----------|
| Root Chain | 101 | MAX_VALIDATORS |
| Sidechain | 100 | MAX_VALIDATORS_PER_CHAIN |
| Host Chain | 200 | MAX_VALIDATORS_PER_HOST |
| Network | 1,000 | max_validators (config) |

---

## 10. Slashing Penalties

### 10.1 Severity Levels

| Severity | VC Slash | Stake Slash | Cooldown |
|----------|----------|-------------|----------|
| Critical | 50% | 5-20% | 52 epochs |
| High | 25% | 1-5% | 12 epochs |
| Medium | 10% | 0-1% | None |
| Low | 5% | 0% | None |

### 10.2 Slashable Events

| Event | Severity |
|-------|----------|
| Double Signing | Critical |
| Equivocation | Critical |
| Arbitration Misconduct | High |
| Invalid Governance Execution | High |
| Extended Downtime (>= 12 epochs) | Medium |
| Low Participation (< 50%) | Medium |
| Short Downtime (< 12 epochs) | Low |

---

## 11. 20-Year Economic Projection

| Year | Supply (KRAT) | Net Change |
|------|---------------|------------|
| 0 | 1,000,000,000 | Initial |
| 1 | ~1,048,000,000 | +4.8% |
| 5 | ~1,082,000,000 | Peak (crossover) |
| 10 | ~1,025,000,000 | -1.8%/year |
| 20 | ~824,000,000 | -2.6%/year |

**Long-term:** Deflationary after Year 5 (burn exceeds issuance).

---

## 12. Implementation Constants

### 12.1 Token Constants (krat.rs)

```rust
pub const INITIAL_SUPPLY: Balance = 1_000_000_000 * KRAT;
pub const INITIAL_EMISSION_RATE_BPS: u32 = 500;    // 5%
pub const MIN_EMISSION_RATE_BPS: u32 = 50;         // 0.5%
pub const INITIAL_BURN_RATE_BPS: u32 = 100;        // 1%
pub const MAX_BURN_RATE_BPS: u32 = 350;            // 3.5%
pub const EMISSION_PERIOD_BLOCKS: BlockNumber = 432_000;
pub const EMISSION_HALF_LIFE_YEARS: f64 = 5.0;
```

### 12.2 Validator Thresholds (economics.rs)

```rust
pub const BOOTSTRAP_TARGET_INFLATION: f64 = 0.065;
pub const EMERGENCY_VALIDATORS: u32 = 25;
pub const POST_BOOTSTRAP_MIN_VALIDATORS: u32 = 50;
pub const SAFE_VALIDATORS: u32 = 75;
pub const OPTIMAL_VALIDATORS: u32 = 101;
```

---

## 13. Source Files

| File | Contents |
|------|----------|
| `contracts/krat.rs` | Token constants, emission calculation |
| `consensus/economics.rs` | Bootstrap config, adaptive inflation |
| `consensus/slashing.rs` | Slashing penalties |
| `consensus/validator.rs` | Staking mechanics |

---

## 14. Related Specifications

- **SPEC 2:** Validator Credits - VC accumulation and stake reduction
- **SPEC 3:** Consensus - Block production and rewards
- **SPEC 6:** Network Security - Security state transitions
