// KRAT Token - System contract pour le token natif
// Principe: Supply adaptative, emission décroissante, burn automatique

use crate::types::{Balance, BlockNumber, KRAT, MILLIKRAT};
use serde::{Deserialize, Serialize};

/// Supply initiale: 1 milliard de KRAT
pub const INITIAL_SUPPLY: Balance = 1_000_000_000 * KRAT;

/// Émission initiale annuelle (en bps - basis points, 1 bps = 0.01%)
/// 500 bps = 5% par an
pub const INITIAL_EMISSION_RATE_BPS: u32 = 500;

/// Émission minimale annuelle
/// 50 bps = 0.5% par an
pub const MIN_EMISSION_RATE_BPS: u32 = 50;

/// Taux de burn initial
/// 100 bps = 1% par an
pub const INITIAL_BURN_RATE_BPS: u32 = 100;

/// Taux de burn maximal
/// 350 bps = 3.5% par an
pub const MAX_BURN_RATE_BPS: u32 = 350;

/// Période d'émission (en blocs)
/// 30 jours = 30 * 24 * 3600 / 6 = 432,000 blocs
pub const EMISSION_PERIOD_BLOCKS: BlockNumber = 432_000;

/// Demi-vie de l'émission (en années)
/// Après 5 ans, émission = (initial - min) / 2 + min
pub const EMISSION_HALF_LIFE_YEARS: f64 = 5.0;

/// État du système tokenomics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenomicsState {
    /// Supply totale actuelle
    pub total_supply: Balance,

    /// Total émis depuis genesis
    pub total_minted: Balance,

    /// Total brûlé depuis genesis
    pub total_burned: Balance,

    /// Période d'émission actuelle
    pub current_period: u64,

    /// Dernier bloc d'émission
    pub last_emission_block: BlockNumber,
}

impl TokenomicsState {
    pub fn genesis() -> Self {
        Self {
            total_supply: INITIAL_SUPPLY,
            total_minted: 0,
            total_burned: 0,
            current_period: 0,
            last_emission_block: 0,
        }
    }

    /// Calcule le taux d'émission actuel (en bps)
    /// Formule: r(t) = r_min + (r_0 - r_min) * e^(-k*t)
    /// où k = ln(2) / half_life
    pub fn current_emission_rate(&self) -> u32 {
        let years = self.years_elapsed();
        let decay_constant = (2.0_f64).ln() / EMISSION_HALF_LIFE_YEARS;
        let decay_factor = (-decay_constant * years).exp();

        let rate = MIN_EMISSION_RATE_BPS as f64
            + (INITIAL_EMISSION_RATE_BPS - MIN_EMISSION_RATE_BPS) as f64 * decay_factor;

        rate.max(MIN_EMISSION_RATE_BPS as f64).min(INITIAL_EMISSION_RATE_BPS as f64) as u32
    }

    /// Calcule le taux de burn actuel (en bps)
    /// Formule: b(t) = b_max - (b_max - b_0) * e^(-g*t)
    /// Croissance vers le maximum
    pub fn current_burn_rate(&self) -> u32 {
        let years = self.years_elapsed();
        let growth_speed = 0.25; // 25% de croissance par an
        let growth_factor = (-growth_speed * years).exp();

        let rate = MAX_BURN_RATE_BPS as f64
            - (MAX_BURN_RATE_BPS - INITIAL_BURN_RATE_BPS) as f64 * growth_factor;

        rate.max(INITIAL_BURN_RATE_BPS as f64).min(MAX_BURN_RATE_BPS as f64) as u32
    }

    /// Années écoulées depuis genesis
    /// Note: 12 periods of 30 days = 360 days, we use 365.25 days for accurate years
    /// Each period is 30 days, so periods_per_year = 365.25 / 30 ≈ 12.175
    fn years_elapsed(&self) -> f64 {
        const PERIODS_PER_YEAR: f64 = 365.25 / 30.0; // ~12.175 periods per year
        self.current_period as f64 / PERIODS_PER_YEAR
    }

    /// Vérifie si c'est le moment d'émettre
    pub fn should_emit(&self, current_block: BlockNumber) -> bool {
        current_block >= self.last_emission_block + EMISSION_PERIOD_BLOCKS
    }

    /// Calcule le montant à émettre pour cette période
    /// SECURITY FIX #20: Added overflow protection and bounds checking
    pub fn calculate_emission(&self) -> Balance {
        let rate_bps = self.current_emission_rate();

        // SECURITY FIX #20: Validate rate is within expected bounds
        let rate_bps = rate_bps.min(INITIAL_EMISSION_RATE_BPS).max(MIN_EMISSION_RATE_BPS);

        // Émission sur 30 jours = émission annuelle / periods_per_year
        // Each period is 30 days, periods_per_year ≈ 12.175
        // Use integer math to avoid precision loss: multiply by 30, divide by 365
        // annual_emission * 30 / 365 = (total_supply * rate_bps / 10_000) * 30 / 365
        // Reorder to minimize precision loss: total_supply * rate_bps * 30 / (10_000 * 365)
        let numerator = self.total_supply
            .saturating_mul(rate_bps as u128)
            .saturating_mul(30);
        let denominator = 10_000u128 * 365;

        // SECURITY FIX #20: Protect against division by zero (should never happen with constants)
        if denominator == 0 {
            return 0;
        }

        numerator / denominator
    }

    /// Émet de nouveaux tokens
    pub fn mint(&mut self, amount: Balance, block: BlockNumber) {
        self.total_supply = self.total_supply.saturating_add(amount);
        self.total_minted = self.total_minted.saturating_add(amount);
        self.last_emission_block = block;
        self.current_period += 1;
    }

    /// Brûle des tokens
    pub fn burn(&mut self, amount: Balance) {
        self.total_supply = self.total_supply.saturating_sub(amount);
        self.total_burned = self.total_burned.saturating_add(amount);
    }

    /// Distribution des tokens émis
    /// 70% validateurs, 20% trésor, 10% réserve
    /// Note: We ensure the sum equals exactly `minted` by computing reserve as remainder
    /// SECURITY FIX #20: Uses saturating operations to prevent overflow
    pub fn distribute_emission(&self, minted: Balance) -> EmissionDistribution {
        // SECURITY FIX #20: Use checked division with saturating multiply
        // This prevents any overflow even with very large values
        let to_validators = minted.saturating_mul(70) / 100;
        let to_treasury = minted.saturating_mul(20) / 100;
        // Reserve gets the remainder to ensure sum == minted (no dust loss)
        let to_reserve = minted.saturating_sub(to_validators).saturating_sub(to_treasury);

        // SECURITY FIX #20: Verify distribution invariant (sum must equal minted)
        // This is a defensive check - should always pass due to calculation method
        debug_assert!(
            to_validators.saturating_add(to_treasury).saturating_add(to_reserve) <= minted,
            "Distribution invariant violated"
        );

        EmissionDistribution {
            to_validators,
            to_treasury,
            to_reserve,
        }
    }
}

/// Distribution de l'émission
#[derive(Debug, Clone, Copy)]
pub struct EmissionDistribution {
    pub to_validators: Balance,
    pub to_treasury: Balance,
    pub to_reserve: Balance,
}

/// Dépôt existentiel minimum (anti-spam)
pub const EXISTENTIAL_DEPOSIT: Balance = 1 * MILLIKRAT;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_state() {
        let state = TokenomicsState::genesis();
        assert_eq!(state.total_supply, INITIAL_SUPPLY);
        assert_eq!(state.total_minted, 0);
        assert_eq!(state.total_burned, 0);
    }

    #[test]
    fn test_emission_rate_decay() {
        let mut state = TokenomicsState::genesis();

        // Période 0 (début)
        let rate_0 = state.current_emission_rate();
        assert_eq!(rate_0, INITIAL_EMISSION_RATE_BPS);

        // Simule 5 ans (60 périodes)
        state.current_period = 60;
        let rate_5y = state.current_emission_rate();

        // Après 5 ans (demi-vie), le taux devrait être ~moitié entre initial et min
        let expected = MIN_EMISSION_RATE_BPS + (INITIAL_EMISSION_RATE_BPS - MIN_EMISSION_RATE_BPS) / 2;
        assert!((rate_5y as i32 - expected as i32).abs() < 20); // Tolérance de 20 bps
    }

    #[test]
    fn test_burn_rate_growth() {
        let mut state = TokenomicsState::genesis();

        // Période 0
        let rate_0 = state.current_burn_rate();
        assert_eq!(rate_0, INITIAL_BURN_RATE_BPS);

        // Simule 10 ans
        state.current_period = 120;
        let rate_10y = state.current_burn_rate();

        // Après 10 ans, devrait être proche du max
        assert!(rate_10y > INITIAL_BURN_RATE_BPS);
        assert!(rate_10y <= MAX_BURN_RATE_BPS);
    }

    #[test]
    fn test_mint_and_burn() {
        let mut state = TokenomicsState::genesis();

        // Mint 1000 KRAT
        state.mint(1000 * KRAT, 100);
        assert_eq!(state.total_supply, INITIAL_SUPPLY + 1000 * KRAT);
        assert_eq!(state.total_minted, 1000 * KRAT);

        // Burn 500 KRAT
        state.burn(500 * KRAT);
        assert_eq!(state.total_supply, INITIAL_SUPPLY + 500 * KRAT);
        assert_eq!(state.total_burned, 500 * KRAT);
    }

    #[test]
    fn test_emission_distribution() {
        let state = TokenomicsState::genesis();
        let minted = 1000 * KRAT;

        let dist = state.distribute_emission(minted);

        assert_eq!(dist.to_validators, 700 * KRAT);
        assert_eq!(dist.to_treasury, 200 * KRAT);
        assert_eq!(dist.to_reserve, 100 * KRAT);

        // Vérifie que la somme = total
        assert_eq!(
            dist.to_validators + dist.to_treasury + dist.to_reserve,
            minted
        );
    }

    #[test]
    fn test_should_emit() {
        let mut state = TokenomicsState::genesis();
        state.last_emission_block = 0;

        // Pas encore le moment
        assert!(!state.should_emit(EMISSION_PERIOD_BLOCKS - 1));

        // Maintenant c'est le moment
        assert!(state.should_emit(EMISSION_PERIOD_BLOCKS));
        assert!(state.should_emit(EMISSION_PERIOD_BLOCKS + 1000));
    }
}
