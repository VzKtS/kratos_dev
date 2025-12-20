# SPEC 2: Validator Credits (VC)

**Version:** 1.1
**Status:** Normative
**Last Updated:** 2025-12-19

---

## 1. Overview

Validator Credits (VC) is a merit-based reputation system enabling consensus participation through time, engagement, and contribution rather than wealth alone.

**Core Principles:**
- **Anti-plutocracy:** Stake alone should not grant absolute power
- **Anti-Sybil:** Merit accumulation requires real time and participation
- **Gradual onboarding:** New validators build reputation before full participation
- **Long-term alignment:** Seniority rewards sustained commitment

---

## 2. Credit Types

### 2.1 Vote Credits (Governance)

| Parameter | Value |
|-----------|-------|
| Credits per vote | +1 VC |
| Daily limit | 3 votes/epoch |
| Monthly limit | 50 votes/4 epochs |

### 2.2 Uptime Credits (Consensus)

| Parameter | Value |
|-----------|-------|
| Credits per epoch | +1 VC |
| Threshold | >= 95% participation |

### 2.3 Arbitration Credits (Dispute Resolution)

| Parameter | Value |
|-----------|-------|
| Credits per arbitration | +5 VC |
| Yearly limit | 5 arbitrations/52 epochs |

### 2.4 Seniority Credits (Long-term Commitment)

| Parameter | Value |
|-----------|-------|
| Credits per period | +5 VC |
| Period | 4 epochs (~1 month) |
| Condition | Validator remains active |

---

## 3. Total VC Calculation

```
Total_VC = Vote_Credits + Uptime_Credits + Arbitration_Credits + Seniority_Credits
```

**Properties:**
- Non-transferable (bound to validator identity)
- Monotonically increasing (except via slashing)
- Epoch-based (blockchain time, not wall-clock)

---

## 4. VRF-Weighted Selection

### 4.1 Selection Formula

```
VRF_weight = min(sqrt(stake), sqrt(STAKE_CAP)) × ln(1 + VC)
```

| Parameter | Value |
|-----------|-------|
| STAKE_CAP | 1,000,000 KRAT |
| sqrt(STAKE_CAP) | 1,000 |

### 4.2 Component Effects

**Stake Component:** `min(sqrt(stake), sqrt(STAKE_CAP))`
- Square root provides diminishing returns
- Cap at 1M KRAT prevents whale domination

**VC Component:** `ln(1 + VC)`
- Logarithmic growth prevents exponential power
- VC=0 results in weight=0 (no selection possible)
- Rewards early accumulation more than late

### 4.3 Implications

| Validator Type | Effect |
|----------------|--------|
| VC=0 | Cannot be selected (weight=0) |
| High stake, low VC | Reduced influence |
| Low stake, high VC | Merit can overcome limited wealth |

---

## 5. Epoch-Based Time Windows

All time windows use blockchain epochs for fork-safety:

| Unit | Definition |
|------|------------|
| Epoch | 600 blocks (~1 hour) |
| Day (logical) | 1 epoch |
| Month (logical) | 4 epochs |
| Year (logical) | 52 epochs |

**Rationale:** Epoch numbers are deterministic and canonical across forks.

---

## 6. Anti-Spam Limits

| Credit Type | Limit | Window |
|-------------|-------|--------|
| Vote | 3 | 1 epoch |
| Vote | 50 | 4 epochs |
| Arbitration | 5 | 52 epochs |
| Uptime | 1 | 1 epoch (automatic) |
| Seniority | 5 | 4 epochs (automatic) |

---

## 7. Maximum VC Accumulation

Theoretical maximum per year (perfect participation):

| Type | Calculation | VC/Year |
|------|-------------|---------|
| Vote | 52 × 3 | 156 |
| Uptime | 52 × 1 | 52 |
| Arbitration | 5 × 5 | 25 |
| Seniority | 13 × 5 | 65 |
| **Total** | | **298** |

**Realistic:** ~200-250 VC/year for active validators.

---

## 8. Stake-Based VC Reduction

VC allows validators to reduce their stake requirements:

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

---

## 9. Bootstrap VC Multipliers

During bootstrap (first 60 days), VC accumulation is accelerated:

| Activity | Bootstrap | Post-Bootstrap |
|----------|-----------|----------------|
| Vote Credits | 2x | 1x |
| Uptime Credits | 2x | 1x |
| Arbitration Credits | 1x | 1x |

---

## 10. Security Invariants

1. **Non-negativity:** All VC components >= 0
2. **Monotonicity:** Total VC only increases (absent slashing)
3. **Bounded growth:** Anti-spam limits prevent unbounded accumulation
4. **Determinism:** Same inputs produce same VRF selection
5. **Stake cap enforcement:** Stakes above cap don't increase weight
6. **Fairness:** Identical validators have identical selection probability

---

## 11. Attack Resistance

### 11.1 Sybil Attack
- VC requires real time (epochs pass at fixed rate)
- Cannot spam due to limits
- **Bootstrap validators require minimum 100 VC** before they can participate in block production
- **Result:** Splitting stake dilutes both stake and VC

### 11.2 Plutocracy
- Stake capped at 1M KRAT
- VC required for selection
- **Result:** Wealth alone insufficient

### 11.3 Vote Spam
- 3 votes/epoch, 50 votes/month limits
- Epoch-based resets prevent gaming
- **Result:** Only meaningful participation counts

### 11.4 VC Farming
- Logarithmic component provides diminishing returns
- Seniority requires actual time passage
- **Result:** Cannot rush accumulation

---

## 12. Slashing (Future)

Planned slashing penalties:

| Offense | VC Slash |
|---------|----------|
| Double-signing | -50% |
| Extended downtime | -10%/month offline |
| Invalid blocks | -25% |

---

## 13. Implementation

### 13.1 Storage Structure

```rust
pub struct ValidatorCreditsRecord {
    pub vote_credits: u32,
    pub uptime_credits: u32,
    pub arbitration_credits: u32,
    pub seniority_credits: u32,
    pub last_daily_reset_epoch: EpochNumber,
    pub last_monthly_reset_epoch: EpochNumber,
    pub last_yearly_reset_epoch: EpochNumber,
    pub last_seniority_credit_epoch: EpochNumber,
}
```

### 13.2 Source Files

| File | Contents |
|------|----------|
| `consensus/validator_credits.rs` | VC record management |
| `consensus/vrf_selection.rs` | VRF-weighted selection |
| `consensus/epoch.rs` | Epoch configuration |

---

## 14. Related Specifications

- **SPEC 1:** Tokenomics - Staking economics and slashing
- **SPEC 3:** Consensus - Block production using VRF weights
- **SPEC 5:** Governance - Vote credit accumulation
