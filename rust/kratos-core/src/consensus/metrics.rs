// Metrics - Instrumentation for VC/VRF distribution and selection
// Purpose: Track and expose metrics to detect concentration, starvation, and biases

use crate::consensus::validator_credits::ValidatorCreditsRecord;
use crate::consensus::vrf_selection::compute_vrf_weight;
use crate::types::{AccountId, Balance};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metrics snapshot for VC distribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VCDistributionMetrics {
    /// Total number of validators
    pub total_validators: usize,

    /// Total VC across all validators
    pub total_vc: u64,

    /// Average VC per validator
    pub average_vc: f64,

    /// Median VC
    pub median_vc: u64,

    /// Minimum VC
    pub min_vc: u64,

    /// Maximum VC
    pub max_vc: u64,

    /// Standard deviation of VC
    pub std_dev_vc: f64,

    /// Gini coefficient (0 = perfect equality, 1 = perfect inequality)
    pub gini_coefficient: f64,

    /// Distribution buckets: VC range -> count
    pub distribution_buckets: HashMap<String, usize>,

    /// Top 10 validators by VC
    pub top_10_validators: Vec<(AccountId, u64)>,
}

/// Metrics snapshot for VRF weight distribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VRFWeightMetrics {
    /// Total number of validators
    pub total_validators: usize,

    /// Total weight sum
    pub total_weight: f64,

    /// Average weight per validator
    pub average_weight: f64,

    /// Median weight
    pub median_weight: f64,

    /// Minimum weight
    pub min_weight: f64,

    /// Maximum weight
    pub max_weight: f64,

    /// Standard deviation of weight
    pub std_dev_weight: f64,

    /// Weight concentration ratio (top 10% / total)
    pub top_10_percent_concentration: f64,

    /// Top 10 validators by weight
    pub top_10_by_weight: Vec<(AccountId, f64)>,
}

/// Metrics for actual validator selection frequency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionFrequencyMetrics {
    /// Epoch range covered
    pub epoch_start: u64,
    pub epoch_end: u64,

    /// Total slots analyzed
    pub total_slots: u64,

    /// Selection counts per validator
    pub selection_counts: HashMap<AccountId, u64>,

    /// Expected vs actual selection ratio per validator
    pub expected_vs_actual: HashMap<AccountId, f64>,

    /// Chi-squared statistic for fairness test
    pub chi_squared: f64,

    /// Validators with zero selections (starvation)
    pub starved_validators: Vec<AccountId>,
}

/// Metrics collector
pub struct MetricsCollector {
    /// VC records snapshot
    vc_records: HashMap<AccountId, ValidatorCreditsRecord>,

    /// Stake snapshot
    stakes: HashMap<AccountId, Balance>,

    /// Selection history (validator -> count)
    selection_history: HashMap<AccountId, u64>,

    /// Total slots tracked
    total_slots: u64,

    /// Epoch range
    epoch_start: u64,
    epoch_end: u64,
}

impl MetricsCollector {
    /// Create new metrics collector
    pub fn new() -> Self {
        Self {
            vc_records: HashMap::new(),
            stakes: HashMap::new(),
            selection_history: HashMap::new(),
            total_slots: 0,
            epoch_start: 0,
            epoch_end: 0,
        }
    }

    /// Update snapshot with current validator set
    pub fn update_snapshot(
        &mut self,
        vc_records: HashMap<AccountId, ValidatorCreditsRecord>,
        stakes: HashMap<AccountId, Balance>,
    ) {
        self.vc_records = vc_records;
        self.stakes = stakes;
    }

    /// Record a validator selection
    pub fn record_selection(&mut self, validator_id: AccountId, epoch: u64, _slot: u64) {
        *self.selection_history.entry(validator_id).or_insert(0) += 1;
        self.total_slots += 1;

        if self.total_slots == 1 {
            self.epoch_start = epoch;
        }
        self.epoch_end = epoch;
    }

    /// Compute VC distribution metrics
    pub fn compute_vc_distribution(&self) -> VCDistributionMetrics {
        let total_validators = self.vc_records.len();

        if total_validators == 0 {
            return VCDistributionMetrics {
                total_validators: 0,
                total_vc: 0,
                average_vc: 0.0,
                median_vc: 0,
                min_vc: 0,
                max_vc: 0,
                std_dev_vc: 0.0,
                gini_coefficient: 0.0,
                distribution_buckets: HashMap::new(),
                top_10_validators: Vec::new(),
            };
        }

        // Collect all VC values
        let mut vc_values: Vec<(AccountId, u64)> = self
            .vc_records
            .iter()
            .map(|(id, record)| (*id, record.total_vc()))
            .collect();

        vc_values.sort_by(|a, b| b.1.cmp(&a.1)); // Sort descending

        let total_vc: u64 = vc_values.iter().map(|(_, vc)| vc).sum();
        let average_vc = total_vc as f64 / total_validators as f64;

        // Min, max, median
        let min_vc = vc_values.last().map(|(_, vc)| *vc).unwrap_or(0);
        let max_vc = vc_values.first().map(|(_, vc)| *vc).unwrap_or(0);
        let median_vc = if total_validators > 0 {
            vc_values[total_validators / 2].1
        } else {
            0
        };

        // Standard deviation
        let variance: f64 = vc_values
            .iter()
            .map(|(_, vc)| {
                let diff = *vc as f64 - average_vc;
                diff * diff
            })
            .sum::<f64>()
            / total_validators as f64;
        let std_dev_vc = variance.sqrt();

        // Gini coefficient
        let gini_coefficient = Self::compute_gini(&vc_values.iter().map(|(_, vc)| *vc).collect::<Vec<u64>>());

        // Distribution buckets
        let mut distribution_buckets = HashMap::new();
        let buckets = vec![
            ("0-10", 0, 10),
            ("10-50", 10, 50),
            ("50-100", 50, 100),
            ("100-500", 100, 500),
            ("500-1000", 500, 1000),
            ("1000+", 1000, u64::MAX),
        ];

        for (label, min, max) in buckets {
            let count = vc_values
                .iter()
                .filter(|(_, vc)| *vc >= min && *vc < max)
                .count();
            distribution_buckets.insert(label.to_string(), count);
        }

        // Top 10
        let top_10_validators = vc_values.iter().take(10).copied().collect();

        VCDistributionMetrics {
            total_validators,
            total_vc,
            average_vc,
            median_vc,
            min_vc,
            max_vc,
            std_dev_vc,
            gini_coefficient,
            distribution_buckets,
            top_10_validators,
        }
    }

    /// Compute VRF weight distribution metrics
    pub fn compute_vrf_weight_distribution(&self) -> VRFWeightMetrics {
        let total_validators = self.vc_records.len();

        if total_validators == 0 {
            return VRFWeightMetrics {
                total_validators: 0,
                total_weight: 0.0,
                average_weight: 0.0,
                median_weight: 0.0,
                min_weight: 0.0,
                max_weight: 0.0,
                std_dev_weight: 0.0,
                top_10_percent_concentration: 0.0,
                top_10_by_weight: Vec::new(),
            };
        }

        // Compute weights for all validators
        let mut weights: Vec<(AccountId, f64)> = self
            .vc_records
            .iter()
            .map(|(id, record)| {
                let stake = *self.stakes.get(id).unwrap_or(&0);
                let vc = record.total_vc();
                let weight = compute_vrf_weight(stake, vc);
                (*id, weight)
            })
            .collect();

        weights.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap()); // Sort descending

        let total_weight: f64 = weights.iter().map(|(_, w)| w).sum();
        let average_weight = total_weight / total_validators as f64;

        // Min, max, median
        let min_weight = weights.last().map(|(_, w)| *w).unwrap_or(0.0);
        let max_weight = weights.first().map(|(_, w)| *w).unwrap_or(0.0);
        let median_weight = if total_validators > 0 {
            weights[total_validators / 2].1
        } else {
            0.0
        };

        // Standard deviation
        let variance: f64 = weights
            .iter()
            .map(|(_, w)| {
                let diff = w - average_weight;
                diff * diff
            })
            .sum::<f64>()
            / total_validators as f64;
        let std_dev_weight = variance.sqrt();

        // Top 10% concentration
        let top_10_percent_count = (total_validators as f64 * 0.1).ceil() as usize;
        let top_10_percent_weight: f64 = weights.iter().take(top_10_percent_count).map(|(_, w)| w).sum();
        let top_10_percent_concentration = if total_weight > 0.0 {
            top_10_percent_weight / total_weight
        } else {
            0.0
        };

        // Top 10 by weight
        let top_10_by_weight = weights.iter().take(10).copied().collect();

        VRFWeightMetrics {
            total_validators,
            total_weight,
            average_weight,
            median_weight,
            min_weight,
            max_weight,
            std_dev_weight,
            top_10_percent_concentration,
            top_10_by_weight,
        }
    }

    /// Compute selection frequency metrics
    pub fn compute_selection_frequency(&self) -> SelectionFrequencyMetrics {
        if self.total_slots == 0 {
            return SelectionFrequencyMetrics {
                epoch_start: self.epoch_start,
                epoch_end: self.epoch_end,
                total_slots: 0,
                selection_counts: HashMap::new(),
                expected_vs_actual: HashMap::new(),
                chi_squared: 0.0,
                starved_validators: Vec::new(),
            };
        }

        // Compute expected selection probabilities based on VRF weights
        let weights: HashMap<AccountId, f64> = self
            .vc_records
            .iter()
            .map(|(id, record)| {
                let stake = *self.stakes.get(id).unwrap_or(&0);
                let vc = record.total_vc();
                (*id, compute_vrf_weight(stake, vc))
            })
            .collect();

        let total_weight: f64 = weights.values().sum();

        // Expected vs actual
        let mut expected_vs_actual = HashMap::new();
        let mut chi_squared = 0.0;

        for (validator_id, weight) in &weights {
            let expected_selections = (weight / total_weight) * self.total_slots as f64;
            let actual_selections = *self.selection_history.get(validator_id).unwrap_or(&0) as f64;

            let ratio = if expected_selections > 0.0 {
                actual_selections / expected_selections
            } else {
                0.0
            };

            expected_vs_actual.insert(*validator_id, ratio);

            // Chi-squared contribution
            if expected_selections > 0.0 {
                let diff = actual_selections - expected_selections;
                chi_squared += (diff * diff) / expected_selections;
            }
        }

        // Find starved validators (zero selections despite non-zero weight)
        let starved_validators: Vec<AccountId> = weights
            .iter()
            .filter(|(id, weight)| **weight > 0.0 && !self.selection_history.contains_key(id))
            .map(|(id, _)| *id)
            .collect();

        SelectionFrequencyMetrics {
            epoch_start: self.epoch_start,
            epoch_end: self.epoch_end,
            total_slots: self.total_slots,
            selection_counts: self.selection_history.clone(),
            expected_vs_actual,
            chi_squared,
            starved_validators,
        }
    }

    /// Reset selection tracking
    pub fn reset_selection_tracking(&mut self) {
        self.selection_history.clear();
        self.total_slots = 0;
        self.epoch_start = 0;
        self.epoch_end = 0;
    }

    /// Compute Gini coefficient for inequality measurement
    /// 0 = perfect equality, 1 = perfect inequality
    fn compute_gini(values: &[u64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }

        let mut sorted = values.to_vec();
        sorted.sort();

        let n = sorted.len() as f64;
        let sum: u64 = sorted.iter().sum();

        if sum == 0 {
            return 0.0;
        }

        let mut numerator = 0.0;
        for (i, value) in sorted.iter().enumerate() {
            numerator += (2.0 * (i as f64 + 1.0) - n - 1.0) * (*value as f64);
        }

        numerator / (n * sum as f64)
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Format metrics for debug output
impl VCDistributionMetrics {
    pub fn format_debug(&self) -> String {
        format!(
            "VC Distribution:\n\
             - Total validators: {}\n\
             - Total VC: {}\n\
             - Average VC: {:.2}\n\
             - Median VC: {}\n\
             - Range: {} - {}\n\
             - Std Dev: {:.2}\n\
             - Gini coefficient: {:.4} (0=equal, 1=unequal)\n\
             - Distribution: {:?}\n\
             - Top 10: {:?}",
            self.total_validators,
            self.total_vc,
            self.average_vc,
            self.median_vc,
            self.min_vc,
            self.max_vc,
            self.std_dev_vc,
            self.gini_coefficient,
            self.distribution_buckets,
            self.top_10_validators
        )
    }
}

impl VRFWeightMetrics {
    pub fn format_debug(&self) -> String {
        format!(
            "VRF Weight Distribution:\n\
             - Total validators: {}\n\
             - Total weight: {:.2}\n\
             - Average weight: {:.4}\n\
             - Median weight: {:.4}\n\
             - Range: {:.4} - {:.4}\n\
             - Std Dev: {:.4}\n\
             - Top 10% concentration: {:.2}%\n\
             - Top 10: {:?}",
            self.total_validators,
            self.total_weight,
            self.average_weight,
            self.median_weight,
            self.min_weight,
            self.max_weight,
            self.std_dev_weight,
            self.top_10_percent_concentration * 100.0,
            self.top_10_by_weight
        )
    }
}

impl SelectionFrequencyMetrics {
    pub fn format_debug(&self) -> String {
        format!(
            "Selection Frequency (epochs {}-{}):\n\
             - Total slots: {}\n\
             - Unique validators selected: {}\n\
             - Starved validators: {}\n\
             - Chi-squared statistic: {:.2}",
            self.epoch_start,
            self.epoch_end,
            self.total_slots,
            self.selection_counts.len(),
            self.starved_validators.len(),
            self.chi_squared
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gini_coefficient() {
        // Perfect equality
        let equal = vec![100, 100, 100, 100];
        assert!((MetricsCollector::compute_gini(&equal) - 0.0).abs() < 0.01);

        // Perfect inequality
        let unequal = vec![0, 0, 0, 1000];
        let gini = MetricsCollector::compute_gini(&unequal);
        assert!(gini > 0.7); // Should be close to 1.0

        // Moderate inequality
        let moderate = vec![10, 20, 30, 40];
        let gini_mod = MetricsCollector::compute_gini(&moderate);
        assert!(gini_mod > 0.0 && gini_mod < 0.5);
    }

    #[test]
    fn test_metrics_collector_empty() {
        let collector = MetricsCollector::new();
        let vc_metrics = collector.compute_vc_distribution();
        assert_eq!(vc_metrics.total_validators, 0);

        let vrf_metrics = collector.compute_vrf_weight_distribution();
        assert_eq!(vrf_metrics.total_validators, 0);

        let freq_metrics = collector.compute_selection_frequency();
        assert_eq!(freq_metrics.total_slots, 0);
    }

    #[test]
    fn test_metrics_collector_with_data() {
        let mut collector = MetricsCollector::new();

        // Create test data
        let mut vc_records = HashMap::new();
        let mut stakes = HashMap::new();

        for i in 0..10 {
            let validator = AccountId::from_bytes([i; 32]);
            let mut record = ValidatorCreditsRecord::new(0, 0);

            // Give varying VC amounts
            for _ in 0..(i * 10) {
                let _ = record.add_vote_credit(1, 0);
            }

            vc_records.insert(validator, record);
            stakes.insert(validator, (i as u128 + 1) * 100_000);
        }

        collector.update_snapshot(vc_records, stakes);

        // Test VC distribution
        let vc_metrics = collector.compute_vc_distribution();
        assert_eq!(vc_metrics.total_validators, 10);
        assert!(vc_metrics.total_vc > 0);
        assert!(vc_metrics.gini_coefficient > 0.0);

        // Test VRF weight distribution
        let vrf_metrics = collector.compute_vrf_weight_distribution();
        assert_eq!(vrf_metrics.total_validators, 10);
        assert!(vrf_metrics.total_weight > 0.0);

        // Record some selections
        for i in 0..5 {
            let validator = AccountId::from_bytes([i; 32]);
            collector.record_selection(validator, 0, i as u64);
        }

        let freq_metrics = collector.compute_selection_frequency();
        assert_eq!(freq_metrics.total_slots, 5);
        assert_eq!(freq_metrics.selection_counts.len(), 5);
    }
}
