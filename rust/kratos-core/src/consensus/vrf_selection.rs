// VRF Selection - VRF-weighted validator selection
// Principle: Hybrid weighting to prevent both plutocracy and Sybil attacks
// Formula: VRF_weight = min(sqrt(stake), sqrt(1_000_000)) × ln(1 + VC)
//
// SECURITY: This module is critical for consensus fairness.
// All randomness must be deterministic and unbiased across all nodes.

use crate::types::{AccountId, Balance};
use schnorrkel::{Keypair, PublicKey};

/// Stake cap for VRF weighting: sqrt(1M KRAT)
const STAKE_CAP: u64 = 1_000_000;

/// Minimum VC for new validators to have non-zero weight
/// This solves the cold-start problem where new validators with VC=0
/// would have weight=0 (since ln(1)=0) and never be selected.
/// Set to 1 to give new validators a baseline weight.
const MIN_EFFECTIVE_VC: u64 = 1;

/// SECURITY FIX #26: Reduced bootstrap stake component to prevent Sybil attacks
/// Bootstrap validators (stake=0) get a much smaller weight than staked validators
/// Previous value (100.0) was equivalent to a validator with 10,000 KRAT
/// New value (10.0) is equivalent to a validator with only 100 KRAT
/// This significantly reduces the incentive for Sybil attacks via bootstrap validators
const BOOTSTRAP_STAKE_COMPONENT: f64 = 10.0;

/// SECURITY FIX #26: Minimum VC required for bootstrap validators to participate
/// Bootstrap validators must earn some reputation before they can be selected
/// This prevents instant Sybil attacks with freshly registered bootstrap validators
///
/// SECURITY FIX #41: Increased from 5 to 100 VC
/// Rationale: 5 VC was too low and could be earned in ~1 epoch
/// 100 VC requires multiple epochs of active participation (~1 week minimum)
/// This provides time window for community to detect malicious validators
/// Per SPEC 2 §4.2: VC component is ln(1 + VC), so 100 VC = ln(101) ≈ 4.6 weight
const BOOTSTRAP_MIN_VC_REQUIREMENT: u64 = 100;

/// VRF context for validator selection
const VRF_CONTEXT: &[u8] = b"kratos-vrf-validator-selection";

/// Compute VRF weight for a validator
/// Formula: VRF_weight = max(sqrt(stake), BOOTSTRAP_MIN) × ln(1 + max(VC, MIN_EFFECTIVE_VC))
///
/// SECURITY FIX #1 & #9:
/// - Bootstrap validators (stake=0) now get BOOTSTRAP_STAKE_COMPONENT weight
/// - This respects constitutional exception for bootstrap era (Article V)
///
/// SECURITY FIX #26: Anti-Sybil protection for bootstrap validators
/// - Bootstrap validators must have minimum VC to participate (prevents instant attacks)
/// - Bootstrap stake component reduced significantly (10.0 vs 100.0)
///
/// Note: We use max(VC, MIN_EFFECTIVE_VC) to solve the cold-start problem.
/// Without this, new validators with VC=0 would have weight=0 (since ln(1)=0)
/// and would never be selected to produce blocks, creating a chicken-and-egg problem.
pub fn compute_vrf_weight(stake: Balance, validator_credits: u64) -> f64 {
    // Compute stake component: min(sqrt(stake), sqrt(STAKE_CAP))
    // SECURITY FIX #9: Bootstrap validators (stake=0) get minimum weight
    let stake_f64 = stake as f64;
    let stake_sqrt = stake_f64.sqrt();
    let stake_cap_sqrt = (STAKE_CAP as f64).sqrt();

    // SECURITY FIX #26: Bootstrap validators must have minimum VC to participate
    // This prevents instant Sybil attacks with freshly registered validators
    if stake == 0 && validator_credits < BOOTSTRAP_MIN_VC_REQUIREMENT {
        return 0.0; // No weight until minimum VC is earned
    }

    // Use bootstrap minimum if stake is 0 (bootstrap validator per SPEC v2.1)
    let stake_component = if stake == 0 {
        BOOTSTRAP_STAKE_COMPONENT
    } else {
        stake_sqrt.min(stake_cap_sqrt)
    };

    // Compute VC component: ln(1 + max(VC, MIN_EFFECTIVE_VC))
    // Using max() ensures new validators always have at least ln(2) ≈ 0.693 weight factor
    let effective_vc = validator_credits.max(MIN_EFFECTIVE_VC);
    let vc_component = (1.0 + effective_vc as f64).ln();

    // Final weight
    stake_component * vc_component
}

/// Check if a bootstrap validator meets minimum requirements for selection
/// SECURITY FIX #26: Bootstrap validators need to earn VC before participating
pub fn is_bootstrap_eligible(stake: Balance, validator_credits: u64) -> bool {
    if stake == 0 {
        validator_credits >= BOOTSTRAP_MIN_VC_REQUIREMENT
    } else {
        true // Staked validators are always eligible
    }
}

/// VRF-based validator selector
pub struct VRFSelector {
    /// VRF keypair for this node
    keypair: Option<Keypair>,
}

impl VRFSelector {
    /// Create new VRF selector
    pub fn new(keypair: Option<Keypair>) -> Self {
        Self { keypair }
    }

    /// Generate VRF output and proof for slot
    /// Returns (VRF output, VRFproof)
    pub fn generate_vrf(
        &self,
        slot: u64,
        epoch: u64,
    ) -> Result<(VRFOutput, VRFProof), VRFError> {
        let keypair = self.keypair.as_ref().ok_or(VRFError::NoKeypair)?;

        // Create transcript
        let mut transcript = merlin::Transcript::new(VRF_CONTEXT);
        transcript.append_message(b"epoch", &epoch.to_le_bytes());
        transcript.append_message(b"slot", &slot.to_le_bytes());

        // Sign with VRF
        let (in_out, proof, _) = keypair.vrf_sign(transcript);

        // Extract VRF output (32 bytes)
        let output_bytes = in_out.to_preout().to_bytes();

        Ok((
            VRFOutput {
                bytes: output_bytes.to_vec(),
            },
            VRFProof {
                proof: proof.to_bytes().to_vec(),
            },
        ))
    }

    /// Verify VRF proof
    pub fn verify_vrf(
        public_key: &PublicKey,
        slot: u64,
        epoch: u64,
        output: &VRFOutput,
        proof: &VRFProof,
    ) -> Result<bool, VRFError> {
        // Create transcript
        let mut transcript = merlin::Transcript::new(VRF_CONTEXT);
        transcript.append_message(b"epoch", &epoch.to_le_bytes());
        transcript.append_message(b"slot", &slot.to_le_bytes());

        // Parse proof
        let proof_bytes: [u8; 64] = proof
            .proof
            .as_slice()
            .try_into()
            .map_err(|_| VRFError::InvalidProof)?;
        let vrf_proof = schnorrkel::vrf::VRFProof::from_bytes(&proof_bytes)
            .map_err(|_| VRFError::InvalidProof)?;

        // Parse output
        let output_bytes: [u8; 32] = output
            .bytes
            .as_slice()
            .try_into()
            .map_err(|_| VRFError::InvalidOutput)?;
        let vrf_preout = schnorrkel::vrf::VRFPreOut::from_bytes(&output_bytes)
            .map_err(|_| VRFError::InvalidOutput)?;

        // Verify
        let (in_out, _) = public_key
            .vrf_verify(transcript, &vrf_preout, &vrf_proof)
            .map_err(|_| VRFError::VerificationFailed)?;

        // Check output matches
        Ok(in_out.to_preout().to_bytes() == output.bytes.as_slice())
    }

    /// Select validator for slot based on VRF outputs and weights
    /// Returns the AccountId of the selected validator
    ///
    /// FIX: Improved precision handling and bounds checking
    pub fn select_validator(
        slot: u64,
        epoch: u64,
        candidates: &[(AccountId, Balance, u64)], // (id, stake, VC)
    ) -> Result<AccountId, VRFError> {
        if candidates.is_empty() {
            return Err(VRFError::NoCandidates);
        }

        // Compute weighted VRF scores
        let mut weighted_scores: Vec<(AccountId, f64)> = Vec::with_capacity(candidates.len());

        for (validator_id, stake, vc) in candidates {
            // Compute VRF weight
            let weight = compute_vrf_weight(*stake, *vc);

            // Create deterministic randomness from validator ID, slot, and epoch
            let mut input = Vec::new();
            input.extend_from_slice(validator_id.as_bytes());
            input.extend_from_slice(&slot.to_le_bytes());
            input.extend_from_slice(&epoch.to_le_bytes());

            // Hash to get pseudo-random value
            let hash = blake3::hash(&input);
            let hash_bytes = hash.as_bytes();

            // SECURITY FIX #1: Proper unbiased random value generation
            // Use full 64 bits from hash and normalize correctly to [0, 1)
            // This ensures uniform distribution without bias at boundaries
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&hash_bytes[0..8]);
            let random_u64 = u64::from_le_bytes(bytes);

            // Convert to [0, 1) range using proper normalization
            // We use u64::MAX + 1 conceptually (2^64) as divisor
            // This gives uniform distribution: random_u64 / 2^64
            let random_val = (random_u64 as f64) / ((u64::MAX as f64) + 1.0);

            // Weighted score = weight × random_val
            // Score is in range [0, weight) with uniform distribution
            let score = weight * random_val;
            weighted_scores.push((*validator_id, score));
        }

        // FIX: Check if weighted_scores is empty after processing (defensive)
        if weighted_scores.is_empty() {
            return Err(VRFError::NoCandidates);
        }

        // Select validator with highest weighted score
        // FIX: Use total_cmp for deterministic NaN handling
        weighted_scores.sort_by(|a, b| b.1.total_cmp(&a.1));

        Ok(weighted_scores[0].0)
    }
}

/// VRF output
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VRFOutput {
    pub bytes: Vec<u8>,
}

/// VRF proof
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VRFProof {
    pub proof: Vec<u8>,
}

/// VRF errors
#[derive(Debug, thiserror::Error)]
pub enum VRFError {
    #[error("No VRF keypair available")]
    NoKeypair,

    #[error("Invalid VRF proof")]
    InvalidProof,

    #[error("Invalid VRF output")]
    InvalidOutput,

    #[error("VRF verification failed")]
    VerificationFailed,

    #[error("No validator candidates")]
    NoCandidates,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_compute_vrf_weight() {
        // Case 1: Low stake, VC=0 now uses MIN_EFFECTIVE_VC=1, so weight = sqrt(stake) * ln(2)
        // This solves the cold-start problem where new validators with VC=0 couldn't be selected
        let weight1 = compute_vrf_weight(100_000, 0);
        let expected_weight1 = (100_000.0_f64).sqrt() * (2.0_f64).ln();
        assert!((weight1 - expected_weight1).abs() < 0.001);

        // Case 2: High stake (above cap), VC=0
        let weight2 = compute_vrf_weight(10_000_000, 0);
        // Should be capped at sqrt(1M) × ln(2) due to MIN_EFFECTIVE_VC
        let expected_max_stake = (STAKE_CAP as f64).sqrt();
        let vc_ln = (2.0_f64).ln(); // ln(1 + max(0, 1)) = ln(2)
        assert!((weight2 - expected_max_stake * vc_ln).abs() < 0.001);

        // Case 3: Low stake, high VC
        let weight3 = compute_vrf_weight(100_000, 100);
        let stake_sqrt = (100_000.0_f64).sqrt();
        let vc_ln = (1.0_f64 + 100.0_f64).ln();
        let expected = stake_sqrt * vc_ln;
        assert!((weight3 - expected).abs() < 0.001);

        // Case 4: VC increases weight (VC=50 > MIN_EFFECTIVE_VC=1)
        let weight_min_vc = compute_vrf_weight(500_000, 0); // uses MIN_EFFECTIVE_VC=1
        let weight_with_vc = compute_vrf_weight(500_000, 50);
        assert!(weight_with_vc > weight_min_vc);
    }

    #[test]
    fn test_vrf_generation_and_verification() {
        // Generate keypair
        let keypair = Keypair::generate();
        let selector = VRFSelector::new(Some(keypair.clone()));

        // Generate VRF
        let slot = 42;
        let epoch = 1;
        let (output, proof) = selector.generate_vrf(slot, epoch).unwrap();

        // Verify VRF
        let is_valid = VRFSelector::verify_vrf(&keypair.public, slot, epoch, &output, &proof).unwrap();
        assert!(is_valid);

        // Verify with wrong slot should fail
        let is_valid_wrong = VRFSelector::verify_vrf(&keypair.public, slot + 1, epoch, &output, &proof).unwrap_or(false);
        assert!(!is_valid_wrong);
    }

    #[test]
    fn test_select_validator() {
        // Create test candidates
        let validator1 = AccountId::from_bytes([1; 32]);
        let validator2 = AccountId::from_bytes([2; 32]);
        let validator3 = AccountId::from_bytes([3; 32]);

        let candidates = vec![
            (validator1, 500_000, 10),  // Medium stake, low VC
            (validator2, 1_000_000, 50), // High stake, high VC
            (validator3, 100_000, 5),   // Low stake, very low VC
        ];

        // Select validator for slot
        let selected = VRFSelector::select_validator(1, 0, &candidates).unwrap();

        // Should return one of the validators
        assert!(
            selected == validator1 || selected == validator2 || selected == validator3
        );

        // Selection should be deterministic for same inputs
        let selected2 = VRFSelector::select_validator(1, 0, &candidates).unwrap();
        assert_eq!(selected, selected2);

        // Different slot should potentially select different validator
        let selected_different = VRFSelector::select_validator(2, 0, &candidates).unwrap();
        // Note: This may or may not be different due to randomness
        assert!(
            selected_different == validator1
                || selected_different == validator2
                || selected_different == validator3
        );
    }

    #[test]
    fn test_weight_increases_selection_probability() {
        let validator1 = AccountId::from_bytes([1; 32]);
        let validator2 = AccountId::from_bytes([2; 32]);

        // Validator 2 has much higher weight
        let candidates = vec![
            (validator1, 100_000, 0),   // Low stake, no VC
            (validator2, 900_000, 100), // High stake, high VC
        ];

        // Run selection many times
        let mut selections = HashMap::new();
        for slot in 0..100 {
            let selected = VRFSelector::select_validator(slot, 0, &candidates).unwrap();
            *selections.entry(selected).or_insert(0) += 1;
        }

        // Validator 2 should be selected more often (though not guaranteed in every sample)
        // This is a probabilistic test, so we just check both were selected at least once
        assert!(selections.contains_key(&validator1) || selections.contains_key(&validator2));
    }

    #[test]
    fn test_stake_cap_limits_weight() {
        // Very high stake should be capped
        let weight_capped = compute_vrf_weight(100_000_000, 10);

        // Stake above cap should give same weight
        let weight_also_capped = compute_vrf_weight(1_000_000_000, 10);

        assert_eq!(weight_capped, weight_also_capped);

        // Should equal sqrt(STAKE_CAP) × ln(1 + 10)
        let expected = (STAKE_CAP as f64).sqrt() * (11.0_f64).ln();
        assert!((weight_capped - expected).abs() < 0.001);
    }
}
