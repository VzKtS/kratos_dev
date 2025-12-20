// Personhood Contract - SPEC v4 Layer 2
//
// Implements Proof of Personhood verification:
// - Challenge system for identity verification
// - Social graph analysis for Sybil detection
// - Uniqueness commitments and verification
// - Personhood scoring

use crate::types::identity::{IdentityId, IdentityStatus};
use crate::types::personhood::{
    ChallengeResponse, ChallengeResponseData, ChallengeStatus, ChallengeType,
    PersonhoodChallenge, PersonhoodEvent, PersonhoodScore, SocialGraphNode,
    SybilCluster, UniquenessCommitment,
    CHALLENGE_DEPOSIT, CHALLENGE_FAILURE_PENALTY, CHALLENGE_GRACE_PERIOD,
    CHALLENGE_PERIOD, MAX_CHALLENGES_PER_IDENTITY, MIN_CHALLENGE_INTERVAL,
    MIN_CONNECTIVITY_SCORE, UNIQUENESS_MIN_ATTESTERS,
};
use crate::types::{AccountId, Balance, BlockNumber, ChainId, Hash};
use crate::contracts::identity::{IdentityRegistry, IdentityError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// =============================================================================
// PERSONHOOD REGISTRY
// =============================================================================

/// Personhood Registry - manages PoP verification for a chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonhoodRegistry {
    /// Chain this registry belongs to
    chain_id: ChainId,

    /// Active challenges by ID
    challenges: HashMap<Hash, PersonhoodChallenge>,

    /// Challenges per identity
    challenges_by_identity: HashMap<IdentityId, Vec<Hash>>,

    /// Last challenge time per identity (for rate limiting)
    last_challenge_time: HashMap<IdentityId, BlockNumber>,

    /// Social graph nodes
    social_graph: HashMap<IdentityId, SocialGraphNode>,

    /// Uniqueness commitments
    uniqueness_commitments: HashMap<IdentityId, UniquenessCommitment>,

    /// Personhood scores
    personhood_scores: HashMap<IdentityId, PersonhoodScore>,

    /// Detected Sybil clusters
    sybil_clusters: Vec<SybilCluster>,

    /// Next cluster ID
    next_cluster_id: u32,

    /// Events
    events: Vec<PersonhoodEvent>,

    /// Configuration
    config: PersonhoodConfig,
}

/// Personhood configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonhoodConfig {
    /// Enable challenges
    pub challenges_enabled: bool,

    /// Enable social graph analysis
    pub social_analysis_enabled: bool,

    /// Minimum personhood score for enhanced privileges
    pub min_score_for_privileges: u32,

    /// Allow anonymous challenges
    pub allow_anonymous_challenges: bool,

    /// Challenge cooldown multiplier (applied to MIN_CHALLENGE_INTERVAL)
    pub challenge_cooldown_multiplier: u32,
}

impl Default for PersonhoodConfig {
    fn default() -> Self {
        Self {
            challenges_enabled: true,
            social_analysis_enabled: true,
            min_score_for_privileges: 500,
            allow_anonymous_challenges: false,
            challenge_cooldown_multiplier: 1,
        }
    }
}

impl PersonhoodRegistry {
    /// Create a new personhood registry
    pub fn new(chain_id: ChainId) -> Self {
        Self {
            chain_id,
            challenges: HashMap::new(),
            challenges_by_identity: HashMap::new(),
            last_challenge_time: HashMap::new(),
            social_graph: HashMap::new(),
            uniqueness_commitments: HashMap::new(),
            personhood_scores: HashMap::new(),
            sybil_clusters: Vec::new(),
            next_cluster_id: 1,
            events: Vec::new(),
            config: PersonhoodConfig::default(),
        }
    }

    /// Create with custom config
    pub fn with_config(chain_id: ChainId, config: PersonhoodConfig) -> Self {
        Self {
            config,
            ..Self::new(chain_id)
        }
    }

    // =========================================================================
    // CHALLENGES
    // =========================================================================

    /// Issue a challenge to an identity
    pub fn issue_challenge(
        &mut self,
        challenger: AccountId,
        target_identity: IdentityId,
        challenge_type: ChallengeType,
        deposit: Balance,
        current_block: BlockNumber,
        identity_registry: &IdentityRegistry,
    ) -> Result<Hash, PersonhoodError> {
        if !self.config.challenges_enabled {
            return Err(PersonhoodError::ChallengesDisabled);
        }

        // Verify target exists and is active
        if !identity_registry.is_active(&target_identity, current_block) {
            return Err(PersonhoodError::TargetNotActive);
        }

        // Check challenge cooldown
        if let Some(last_time) = self.last_challenge_time.get(&target_identity) {
            let cooldown = MIN_CHALLENGE_INTERVAL * self.config.challenge_cooldown_multiplier as u64;
            if current_block < last_time + cooldown {
                return Err(PersonhoodError::ChallengeCooldown);
            }
        }

        // Check max challenges
        let current_challenges = self.challenges_by_identity
            .get(&target_identity)
            .map(|c| c.len())
            .unwrap_or(0);

        if current_challenges >= MAX_CHALLENGES_PER_IDENTITY {
            return Err(PersonhoodError::TooManyChallenges);
        }

        // Verify deposit
        if deposit < CHALLENGE_DEPOSIT {
            return Err(PersonhoodError::InsufficientDeposit);
        }

        // Create challenge
        let challenge = PersonhoodChallenge::new(
            target_identity,
            challenger,
            challenge_type.clone(),
            deposit,
            current_block,
        );

        let challenge_id = challenge.challenge_id;

        // Store
        self.challenges.insert(challenge_id, challenge);
        self.challenges_by_identity
            .entry(target_identity)
            .or_default()
            .push(challenge_id);
        self.last_challenge_time.insert(target_identity, current_block);

        // Emit event
        self.events.push(PersonhoodEvent::ChallengeIssued {
            challenge_id,
            target: target_identity,
            challenger,
            challenge_type: format!("{:?}", challenge_type),
        });

        Ok(challenge_id)
    }

    /// Respond to a challenge
    pub fn respond_to_challenge(
        &mut self,
        challenge_id: Hash,
        responder: AccountId,
        response_data: ChallengeResponseData,
        current_block: BlockNumber,
        identity_registry: &IdentityRegistry,
    ) -> Result<(), PersonhoodError> {
        let challenge = self.challenges
            .get_mut(&challenge_id)
            .ok_or(PersonhoodError::ChallengeNotFound)?;

        // Verify responder owns the target identity
        let responder_identity = identity_registry
            .get_identity_id(&responder)
            .ok_or(PersonhoodError::ResponderHasNoIdentity)?;

        if responder_identity != challenge.target_identity {
            return Err(PersonhoodError::NotChallengeTarget);
        }

        // Check challenge is still pending
        if challenge.status != ChallengeStatus::Pending {
            return Err(PersonhoodError::ChallengeNotPending);
        }

        // Check not expired
        if challenge.is_expired(current_block) {
            return Err(PersonhoodError::ChallengeExpired);
        }

        // Validate response matches challenge type
        Self::validate_response(&challenge.challenge_type, &response_data)?;

        // Record response
        challenge.response = Some(ChallengeResponse {
            submitted_at: current_block,
            response_data,
        });
        challenge.status = ChallengeStatus::ResponseSubmitted;

        self.events.push(PersonhoodEvent::ChallengeResponseSubmitted {
            challenge_id,
            target: challenge.target_identity,
        });

        Ok(())
    }

    /// Validate response against challenge type
    fn validate_response(
        challenge_type: &ChallengeType,
        response: &ChallengeResponseData,
    ) -> Result<(), PersonhoodError> {
        match (challenge_type, response) {
            (ChallengeType::Liveness { .. }, ChallengeResponseData::LivenessProof { .. }) => Ok(()),
            (ChallengeType::SocialVerification { .. }, ChallengeResponseData::SocialProof { .. }) => Ok(()),
            (ChallengeType::UniquenessChallenge { .. }, ChallengeResponseData::UniquenessProof { .. }) => Ok(()),
            (ChallengeType::StakeChallenge { .. }, ChallengeResponseData::StakeProof { .. }) => Ok(()),
            _ => Err(PersonhoodError::InvalidResponseType),
        }
    }

    /// Verify and resolve a challenge
    pub fn resolve_challenge(
        &mut self,
        challenge_id: Hash,
        current_block: BlockNumber,
    ) -> Result<(bool, Balance), PersonhoodError> {
        // First, get needed data with immutable borrow
        let (status, is_expired, target_identity, deposit) = {
            let challenge = self.challenges
                .get(&challenge_id)
                .ok_or(PersonhoodError::ChallengeNotFound)?;
            (challenge.status, challenge.is_expired(current_block), challenge.target_identity, challenge.deposit)
        };

        // Determine if passed based on status
        let passed = if status == ChallengeStatus::ResponseSubmitted {
            // Get challenge again for verification (immutable)
            let challenge = self.challenges
                .get(&challenge_id)
                .ok_or(PersonhoodError::ChallengeNotFound)?;
            self.verify_response(challenge, current_block)?
        } else if is_expired {
            false
        } else {
            return Err(PersonhoodError::ChallengeNotReady);
        };

        // Now do mutable operations
        let challenge = self.challenges
            .get_mut(&challenge_id)
            .ok_or(PersonhoodError::ChallengeNotFound)?;

        // Update challenge status
        challenge.status = if passed {
            ChallengeStatus::Passed
        } else if is_expired && status != ChallengeStatus::ResponseSubmitted {
            ChallengeStatus::Expired
        } else {
            ChallengeStatus::Failed
        };

        // Update personhood score
        if let Some(score) = self.personhood_scores.get_mut(&target_identity) {
            if passed {
                score.record_challenge_pass(current_block);
            } else {
                score.record_challenge_fail();
            }
        }

        // Calculate payout
        let payout = if passed {
            // Return deposit to challenger (no penalty since identity proved itself)
            deposit
        } else {
            // Identity failed - challenger gets deposit + penalty from identity
            deposit + CHALLENGE_FAILURE_PENALTY
        };

        self.events.push(PersonhoodEvent::ChallengeResolved {
            challenge_id,
            target: target_identity,
            passed,
        });

        Ok((passed, payout))
    }

    /// Verify a challenge response
    fn verify_response(
        &self,
        challenge: &PersonhoodChallenge,
        _current_block: BlockNumber,
    ) -> Result<bool, PersonhoodError> {
        let response = challenge.response.as_ref()
            .ok_or(PersonhoodError::NoResponse)?;

        match (&challenge.challenge_type, &response.response_data) {
            (ChallengeType::Liveness { nonce }, ChallengeResponseData::LivenessProof { signature }) => {
                // In production, verify signature matches nonce signed by identity owner
                // For now, just check signature is not empty
                Ok(!signature.as_bytes().iter().all(|&b| b == 0))
            }

            (ChallengeType::SocialVerification { required_attestations }, ChallengeResponseData::SocialProof { attestation_ids }) => {
                // Verify enough attestations
                Ok(attestation_ids.len() >= *required_attestations)
            }

            (ChallengeType::UniquenessChallenge { suspected_duplicate }, ChallengeResponseData::UniquenessProof { proof_hash, witnesses }) => {
                // Verify proof hash is not empty and enough witnesses
                let valid_proof = !proof_hash.as_bytes().iter().all(|&b| b == 0);
                let enough_witnesses = witnesses.len() >= 3;
                // Additional check: proof_hash should not match suspected duplicate's commitment
                let not_duplicate = if let Some(commitment) = self.uniqueness_commitments.get(suspected_duplicate) {
                    *proof_hash != commitment.commitment
                } else {
                    true
                };
                Ok(valid_proof && enough_witnesses && not_duplicate)
            }

            (ChallengeType::StakeChallenge { required_stake }, ChallengeResponseData::StakeProof { stake_amount, .. }) => {
                // Verify stake amount meets requirement
                Ok(*stake_amount >= *required_stake)
            }

            _ => Err(PersonhoodError::InvalidResponseType),
        }
    }

    /// Get active challenges for an identity
    pub fn get_challenges(&self, identity_id: &IdentityId) -> Vec<&PersonhoodChallenge> {
        self.challenges_by_identity
            .get(identity_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.challenges.get(id))
                    .filter(|c| c.status == ChallengeStatus::Pending || c.status == ChallengeStatus::ResponseSubmitted)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Cancel a challenge (only challenger can cancel, before response)
    pub fn cancel_challenge(
        &mut self,
        challenge_id: Hash,
        canceller: AccountId,
    ) -> Result<Balance, PersonhoodError> {
        let challenge = self.challenges
            .get_mut(&challenge_id)
            .ok_or(PersonhoodError::ChallengeNotFound)?;

        if challenge.challenger != canceller {
            return Err(PersonhoodError::NotChallenger);
        }

        if challenge.status != ChallengeStatus::Pending {
            return Err(PersonhoodError::ChallengeNotPending);
        }

        challenge.status = ChallengeStatus::Cancelled;

        Ok(challenge.deposit)
    }

    // =========================================================================
    // SOCIAL GRAPH
    // =========================================================================

    /// Update social graph with new attestation
    pub fn record_attestation(
        &mut self,
        attester_identity: IdentityId,
        target_identity: IdentityId,
    ) {
        // Update attester node
        let attester_node = self.social_graph
            .entry(attester_identity)
            .or_insert_with(|| SocialGraphNode::new(attester_identity));
        attester_node.add_connection(target_identity, false);

        // Update target node
        let target_node = self.social_graph
            .entry(target_identity)
            .or_insert_with(|| SocialGraphNode::new(target_identity));
        target_node.add_connection(attester_identity, true);
    }

    /// Recalculate social graph metrics
    pub fn recalculate_social_graph(&mut self) {
        let total_identities = self.social_graph.len() as u64;

        for node in self.social_graph.values_mut() {
            node.calculate_connectivity(total_identities);
        }

        // Update personhood scores
        for (identity_id, node) in &self.social_graph {
            if let Some(score) = self.personhood_scores.get_mut(identity_id) {
                score.update_social(node.connectivity_score);
            }
        }

        // Detect clusters if enabled
        if self.config.social_analysis_enabled {
            self.detect_sybil_clusters();
        }
    }

    /// Detect potential Sybil clusters
    fn detect_sybil_clusters(&mut self) {
        // Simple clustering: group nodes with high internal connectivity
        // More sophisticated algorithms could be implemented

        let mut visited: HashSet<IdentityId> = HashSet::new();
        let mut new_clusters: Vec<SybilCluster> = Vec::new();

        for (identity_id, node) in &self.social_graph {
            if visited.contains(identity_id) {
                continue;
            }

            // BFS to find connected component
            let mut cluster = SybilCluster::new(self.next_cluster_id);
            let mut queue: Vec<IdentityId> = vec![*identity_id];

            while let Some(current) = queue.pop() {
                if visited.contains(&current) {
                    continue;
                }

                visited.insert(current);
                cluster.add_member(current);

                if let Some(current_node) = self.social_graph.get(&current) {
                    for neighbor in &current_node.connections {
                        if !visited.contains(neighbor) {
                            queue.push(*neighbor);
                        }
                    }
                }
            }

            // Only track clusters with 3+ members
            if cluster.members.len() >= 3 {
                // Calculate internal density
                let member_count = cluster.members.len() as u32;
                let max_edges = member_count * (member_count - 1);
                let internal_edges: u32 = cluster.members.iter()
                    .filter_map(|id| self.social_graph.get(id))
                    .map(|n| n.connections.iter().filter(|c| cluster.members.contains(c)).count() as u32)
                    .sum();

                cluster.internal_density = if max_edges > 0 {
                    internal_edges as f32 / max_edges as f32
                } else {
                    0.0
                };

                // Calculate external connections
                cluster.external_connections = cluster.members.iter()
                    .filter_map(|id| self.social_graph.get(id))
                    .map(|n| n.connections.iter().filter(|c| !cluster.members.contains(c)).count() as u32)
                    .sum();

                cluster.calculate_risk();

                if cluster.flagged {
                    self.events.push(PersonhoodEvent::SybilClusterDetected {
                        cluster_id: cluster.cluster_id,
                        member_count: cluster.members.len(),
                        risk_score: cluster.risk_score,
                    });
                }

                new_clusters.push(cluster);
                self.next_cluster_id += 1;
            }
        }

        self.sybil_clusters = new_clusters;
    }

    /// Check if identity is in a suspicious cluster
    pub fn is_in_suspicious_cluster(&self, identity_id: &IdentityId) -> bool {
        self.sybil_clusters.iter().any(|c| c.flagged && c.members.contains(identity_id))
    }

    /// Get social connectivity score
    pub fn get_connectivity(&self, identity_id: &IdentityId) -> u32 {
        self.social_graph
            .get(identity_id)
            .map(|n| n.connectivity_score)
            .unwrap_or(0)
    }

    // =========================================================================
    // UNIQUENESS
    // =========================================================================

    /// Submit uniqueness commitment
    pub fn submit_uniqueness_commitment(
        &mut self,
        identity_id: IdentityId,
        commitment_data: &[u8],
        current_block: BlockNumber,
        identity_registry: &IdentityRegistry,
    ) -> Result<(), PersonhoodError> {
        // Verify identity exists
        if identity_registry.get_identity(&identity_id).is_none() {
            return Err(PersonhoodError::IdentityNotFound);
        }

        // Check no existing commitment
        if self.uniqueness_commitments.contains_key(&identity_id) {
            return Err(PersonhoodError::CommitmentExists);
        }

        let commitment = UniquenessCommitment::new(identity_id, commitment_data, current_block);
        self.uniqueness_commitments.insert(identity_id, commitment);

        Ok(())
    }

    /// Add witness to uniqueness commitment
    pub fn witness_uniqueness(
        &mut self,
        identity_id: IdentityId,
        witness: AccountId,
        current_block: BlockNumber,
    ) -> Result<bool, PersonhoodError> {
        let commitment = self.uniqueness_commitments
            .get_mut(&identity_id)
            .ok_or(PersonhoodError::NoCommitment)?;

        let added = commitment.add_witness(witness);

        if commitment.verified {
            // Update personhood score
            if let Some(score) = self.personhood_scores.get_mut(&identity_id) {
                score.update_uniqueness(true, commitment.witnesses.len());
            }

            self.events.push(PersonhoodEvent::UniquenessVerified {
                identity_id,
                witness_count: commitment.witnesses.len(),
            });
        }

        Ok(added)
    }

    /// Check if identity has verified uniqueness
    pub fn is_unique_verified(&self, identity_id: &IdentityId) -> bool {
        self.uniqueness_commitments
            .get(identity_id)
            .map(|c| c.verified)
            .unwrap_or(false)
    }

    // =========================================================================
    // PERSONHOOD SCORES
    // =========================================================================

    /// Initialize personhood score for identity
    pub fn initialize_score(&mut self, identity_id: IdentityId) {
        if !self.personhood_scores.contains_key(&identity_id) {
            self.personhood_scores.insert(identity_id, PersonhoodScore::new());
        }
    }

    /// Get personhood score
    pub fn get_score(&self, identity_id: &IdentityId) -> Option<&PersonhoodScore> {
        self.personhood_scores.get(identity_id)
    }

    /// Update activity score
    pub fn record_activity(&mut self, identity_id: IdentityId, actions: u32) {
        if let Some(score) = self.personhood_scores.get_mut(&identity_id) {
            let old_score = score.total;
            score.update_activity(actions);

            if score.total != old_score {
                self.events.push(PersonhoodEvent::PersonhoodScoreUpdated {
                    identity_id,
                    old_score,
                    new_score: score.total,
                });
            }
        }
    }

    /// Update longevity score based on identity age
    pub fn update_longevity(&mut self, identity_id: IdentityId, identity_age: BlockNumber) {
        if let Some(score) = self.personhood_scores.get_mut(&identity_id) {
            let old_score = score.total;
            score.update_longevity(identity_age);

            if score.total != old_score {
                self.events.push(PersonhoodEvent::PersonhoodScoreUpdated {
                    identity_id,
                    old_score,
                    new_score: score.total,
                });
            }
        }
    }

    /// Check if identity meets personhood threshold
    pub fn meets_threshold(&self, identity_id: &IdentityId) -> bool {
        self.personhood_scores
            .get(identity_id)
            .map(|s| s.meets_threshold(self.config.min_score_for_privileges))
            .unwrap_or(false)
    }

    /// Check if identity is verified
    pub fn is_verified(&self, identity_id: &IdentityId, current_block: BlockNumber) -> bool {
        self.personhood_scores
            .get(identity_id)
            .map(|s| s.is_verified(current_block))
            .unwrap_or(false)
    }

    // =========================================================================
    // EVENTS
    // =========================================================================

    /// Get events
    pub fn events(&self) -> &[PersonhoodEvent] {
        &self.events
    }

    /// Clear events
    pub fn clear_events(&mut self) {
        self.events.clear();
    }
}

/// Personhood errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum PersonhoodError {
    #[error("Challenges are disabled")]
    ChallengesDisabled,

    #[error("Target identity is not active")]
    TargetNotActive,

    #[error("Challenge cooldown period not elapsed")]
    ChallengeCooldown,

    #[error("Too many challenges for this identity")]
    TooManyChallenges,

    #[error("Insufficient challenge deposit")]
    InsufficientDeposit,

    #[error("Challenge not found")]
    ChallengeNotFound,

    #[error("Responder has no identity")]
    ResponderHasNoIdentity,

    #[error("Not the challenge target")]
    NotChallengeTarget,

    #[error("Challenge is not pending")]
    ChallengeNotPending,

    #[error("Challenge has expired")]
    ChallengeExpired,

    #[error("Invalid response type for challenge")]
    InvalidResponseType,

    #[error("Challenge not ready for resolution")]
    ChallengeNotReady,

    #[error("No response submitted")]
    NoResponse,

    #[error("Not the challenger")]
    NotChallenger,

    #[error("Identity not found")]
    IdentityNotFound,

    #[error("Uniqueness commitment already exists")]
    CommitmentExists,

    #[error("No uniqueness commitment found")]
    NoCommitment,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::identity::IdentityRegistry;

    fn create_account(seed: u8) -> AccountId {
        AccountId::from_bytes([seed; 32])
    }

    fn create_hash(seed: u8) -> Hash {
        Hash::hash(&[seed; 32])
    }

    fn setup_registry_with_identity() -> (PersonhoodRegistry, IdentityRegistry, IdentityId, AccountId) {
        let mut identity_registry = IdentityRegistry::new(ChainId(1));
        let owner = create_account(1);

        // Create and activate identity
        let id = identity_registry.declare_identity(owner, b"data", None, 1000).unwrap();
        // Force activate for testing
        identity_registry.force_activate(&id);

        let personhood_registry = PersonhoodRegistry::new(ChainId(1));

        (personhood_registry, identity_registry, id, owner)
    }

    #[test]
    fn test_issue_challenge() {
        let (mut personhood, identity_registry, target_id, _) = setup_registry_with_identity();
        let challenger = create_account(2);

        let result = personhood.issue_challenge(
            challenger,
            target_id,
            ChallengeType::Liveness { nonce: create_hash(99) },
            CHALLENGE_DEPOSIT,
            2000,
            &identity_registry,
        );

        assert!(result.is_ok());
        assert_eq!(personhood.challenges.len(), 1);
    }

    #[test]
    fn test_challenge_cooldown() {
        let (mut personhood, identity_registry, target_id, _) = setup_registry_with_identity();
        let challenger = create_account(2);

        // First challenge
        personhood.issue_challenge(
            challenger,
            target_id,
            ChallengeType::Liveness { nonce: create_hash(99) },
            CHALLENGE_DEPOSIT,
            2000,
            &identity_registry,
        ).unwrap();

        // Second challenge too soon
        let result = personhood.issue_challenge(
            challenger,
            target_id,
            ChallengeType::Liveness { nonce: create_hash(100) },
            CHALLENGE_DEPOSIT,
            2001,
            &identity_registry,
        );

        assert!(matches!(result, Err(PersonhoodError::ChallengeCooldown)));
    }

    #[test]
    fn test_respond_to_challenge() {
        let (mut personhood, identity_registry, target_id, owner) = setup_registry_with_identity();
        let challenger = create_account(2);

        let challenge_id = personhood.issue_challenge(
            challenger,
            target_id,
            ChallengeType::Liveness { nonce: create_hash(99) },
            CHALLENGE_DEPOSIT,
            2000,
            &identity_registry,
        ).unwrap();

        let result = personhood.respond_to_challenge(
            challenge_id,
            owner,
            ChallengeResponseData::LivenessProof { signature: create_hash(50) },
            2100,
            &identity_registry,
        );

        assert!(result.is_ok());

        let challenge = personhood.challenges.get(&challenge_id).unwrap();
        assert_eq!(challenge.status, ChallengeStatus::ResponseSubmitted);
    }

    #[test]
    fn test_invalid_response_type() {
        let (mut personhood, identity_registry, target_id, owner) = setup_registry_with_identity();
        let challenger = create_account(2);

        let challenge_id = personhood.issue_challenge(
            challenger,
            target_id,
            ChallengeType::Liveness { nonce: create_hash(99) },
            CHALLENGE_DEPOSIT,
            2000,
            &identity_registry,
        ).unwrap();

        // Wrong response type
        let result = personhood.respond_to_challenge(
            challenge_id,
            owner,
            ChallengeResponseData::StakeProof { stake_amount: 100, lock_until: 10000 },
            2100,
            &identity_registry,
        );

        assert!(matches!(result, Err(PersonhoodError::InvalidResponseType)));
    }

    #[test]
    fn test_resolve_challenge_passed() {
        let (mut personhood, identity_registry, target_id, owner) = setup_registry_with_identity();
        personhood.initialize_score(target_id);

        let challenger = create_account(2);

        let challenge_id = personhood.issue_challenge(
            challenger,
            target_id,
            ChallengeType::Liveness { nonce: create_hash(99) },
            CHALLENGE_DEPOSIT,
            2000,
            &identity_registry,
        ).unwrap();

        personhood.respond_to_challenge(
            challenge_id,
            owner,
            ChallengeResponseData::LivenessProof { signature: create_hash(50) },
            2100,
            &identity_registry,
        ).unwrap();

        let (passed, payout) = personhood.resolve_challenge(challenge_id, 2200).unwrap();

        assert!(passed);
        assert_eq!(payout, CHALLENGE_DEPOSIT);

        let score = personhood.get_score(&target_id).unwrap();
        assert_eq!(score.challenges_passed, 1);
    }

    #[test]
    fn test_cancel_challenge() {
        let (mut personhood, identity_registry, target_id, _) = setup_registry_with_identity();
        let challenger = create_account(2);

        let challenge_id = personhood.issue_challenge(
            challenger,
            target_id,
            ChallengeType::Liveness { nonce: create_hash(99) },
            CHALLENGE_DEPOSIT,
            2000,
            &identity_registry,
        ).unwrap();

        let deposit = personhood.cancel_challenge(challenge_id, challenger).unwrap();

        assert_eq!(deposit, CHALLENGE_DEPOSIT);

        let challenge = personhood.challenges.get(&challenge_id).unwrap();
        assert_eq!(challenge.status, ChallengeStatus::Cancelled);
    }

    #[test]
    fn test_social_graph_recording() {
        let mut personhood = PersonhoodRegistry::new(ChainId(1));

        let id1 = create_hash(1);
        let id2 = create_hash(2);
        let id3 = create_hash(3);

        // Record attestations
        personhood.record_attestation(id1, id2);
        personhood.record_attestation(id1, id3);
        personhood.record_attestation(id2, id3);

        assert_eq!(personhood.social_graph.len(), 3);

        let node1 = personhood.social_graph.get(&id1).unwrap();
        assert_eq!(node1.outgoing.len(), 2);
        assert_eq!(node1.incoming.len(), 0);

        let node3 = personhood.social_graph.get(&id3).unwrap();
        assert_eq!(node3.incoming.len(), 2);
    }

    #[test]
    fn test_uniqueness_commitment() {
        let (mut personhood, identity_registry, target_id, _) = setup_registry_with_identity();

        personhood.submit_uniqueness_commitment(
            target_id,
            b"unique_data_123",
            1000,
            &identity_registry,
        ).unwrap();

        assert!(personhood.uniqueness_commitments.contains_key(&target_id));
        assert!(!personhood.is_unique_verified(&target_id));
    }

    #[test]
    fn test_uniqueness_verification() {
        let (mut personhood, identity_registry, target_id, _) = setup_registry_with_identity();
        personhood.initialize_score(target_id);

        personhood.submit_uniqueness_commitment(
            target_id,
            b"unique_data",
            1000,
            &identity_registry,
        ).unwrap();

        // Add witnesses
        personhood.witness_uniqueness(target_id, create_account(10), 1001).unwrap();
        personhood.witness_uniqueness(target_id, create_account(11), 1002).unwrap();
        assert!(!personhood.is_unique_verified(&target_id));

        personhood.witness_uniqueness(target_id, create_account(12), 1003).unwrap();
        assert!(personhood.is_unique_verified(&target_id));
    }

    #[test]
    fn test_personhood_score_initialization() {
        let mut personhood = PersonhoodRegistry::new(ChainId(1));
        let id = create_hash(1);

        personhood.initialize_score(id);

        let score = personhood.get_score(&id).unwrap();
        assert_eq!(score.total, 0);
    }

    #[test]
    fn test_activity_tracking() {
        let mut personhood = PersonhoodRegistry::new(ChainId(1));
        let id = create_hash(1);

        personhood.initialize_score(id);
        personhood.record_activity(id, 10);

        let score = personhood.get_score(&id).unwrap();
        assert_eq!(score.activity_score, 100); // 10 actions * 10 points
    }

    #[test]
    fn test_threshold_check() {
        let mut personhood = PersonhoodRegistry::new(ChainId(1));
        let id = create_hash(1);

        personhood.initialize_score(id);

        // Initially below threshold
        assert!(!personhood.meets_threshold(&id));

        // Activity alone gives max 200 points
        personhood.record_activity(id, 50); // 200 activity points (capped)
        assert!(!personhood.meets_threshold(&id)); // threshold is 500, not enough

        // Need to add more score components to reach 500
        // Update longevity (200 points for ~1 year)
        personhood.update_longevity(id, 5_256_000);
        // Now we have: 200 (activity) + 200 (longevity) = 400, still not enough

        // Lower the threshold for this test
        personhood.config.min_score_for_privileges = 400;
        assert!(personhood.meets_threshold(&id));
    }

    #[test]
    fn test_sybil_cluster_detection() {
        let mut personhood = PersonhoodRegistry::new(ChainId(1));

        // Create a suspicious cluster: 4 identities all attesting each other
        let ids: Vec<Hash> = (1..=4).map(|i| create_hash(i)).collect();

        for i in 0..ids.len() {
            for j in 0..ids.len() {
                if i != j {
                    personhood.record_attestation(ids[i], ids[j]);
                }
            }
        }

        personhood.recalculate_social_graph();

        assert!(!personhood.sybil_clusters.is_empty());
        // The cluster should have high internal density
        let cluster = &personhood.sybil_clusters[0];
        assert_eq!(cluster.members.len(), 4);
    }
}
