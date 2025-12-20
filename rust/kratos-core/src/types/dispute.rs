// Dispute - SPEC v3.1 Phase 6: Cross-Chain Arbitration
// Dispute types, jury structures, and arbitration workflow

use super::primitives::{BlockNumber, ChainId};
use super::account::AccountId;
use super::fraud::FraudProof;
use super::merkle::MerkleProof;
use serde::{Deserialize, Serialize};

/// Unique identifier for a dispute
pub type DisputeId = u64;

/// Cross-chain dispute requiring arbitration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dispute {
    /// Unique dispute identifier
    pub id: DisputeId,

    /// Chain where the dispute occurred
    pub chain_id: ChainId,

    /// Type of dispute
    pub dispute_type: DisputeType,

    /// Who raised this dispute
    pub raised_by: AccountId,

    /// When the dispute was raised
    pub raised_at: BlockNumber,

    /// Evidence supporting the dispute
    pub evidence: Vec<Evidence>,

    /// Current status of the dispute
    pub status: DisputeStatus,

    /// Where this dispute should be resolved
    pub jurisdiction: Jurisdiction,

    /// Selected jury members (VRF-selected)
    pub jury_members: Vec<AccountId>,

    /// Jury votes (collected during deliberation)
    pub jury_votes: Vec<JuryVote>,

    /// Final decision (if resolved)
    pub decision: Option<JuryDecision>,

    /// Evidence submission deadline
    pub evidence_deadline: BlockNumber,

    /// Deliberation deadline
    pub deliberation_deadline: Option<BlockNumber>,
}

impl Dispute {
    pub fn new(
        id: DisputeId,
        chain_id: ChainId,
        dispute_type: DisputeType,
        raised_by: AccountId,
        raised_at: BlockNumber,
        jurisdiction: Jurisdiction,
    ) -> Self {
        // Evidence submission window: 7 days (100,800 blocks @ 6s/block)
        const EVIDENCE_WINDOW: BlockNumber = 100_800;

        Self {
            id,
            chain_id,
            dispute_type,
            raised_by,
            raised_at,
            evidence: Vec::new(),
            status: DisputeStatus::Open,
            jurisdiction,
            jury_members: Vec::new(),
            jury_votes: Vec::new(),
            decision: None,
            evidence_deadline: raised_at + EVIDENCE_WINDOW,
            deliberation_deadline: None,
        }
    }

    /// Check if evidence submission period is still open
    pub fn can_submit_evidence(&self, current_block: BlockNumber) -> bool {
        current_block <= self.evidence_deadline && self.status == DisputeStatus::Open
    }

    /// Check if jury deliberation period is active
    pub fn can_vote(&self, current_block: BlockNumber) -> bool {
        matches!(self.status, DisputeStatus::Deliberating)
            && self.deliberation_deadline.map_or(false, |deadline| current_block <= deadline)
    }

    /// Get the accused party from the dispute
    pub fn accused(&self) -> Option<AccountId> {
        self.evidence.iter().find_map(|e| match e {
            Evidence::FraudProof(proof) => Some(proof.accused_validator()),
            _ => None,
        })
    }

    /// Check if the dispute has exceeded its maximum duration
    /// This prevents disputes from indefinitely blocking exits
    pub fn is_expired(&self, current_block: BlockNumber) -> bool {
        // Already in a terminal state
        if matches!(self.status, DisputeStatus::Resolved | DisputeStatus::Dismissed | DisputeStatus::Expired) {
            return false;
        }

        current_block > self.raised_at + MAX_DISPUTE_DURATION
    }

    /// Get the absolute deadline for this dispute
    pub fn absolute_deadline(&self) -> BlockNumber {
        self.raised_at + MAX_DISPUTE_DURATION
    }

    /// SECURITY FIX #19: Validate status transition
    /// Returns true if the transition from current status to new status is valid
    /// This prevents invalid state transitions that could corrupt dispute handling
    pub fn is_valid_transition(&self, new_status: DisputeStatus) -> bool {
        match (&self.status, &new_status) {
            // From Open: can go to EvidenceComplete, Deliberating (skip), Dismissed, or Expired
            (DisputeStatus::Open, DisputeStatus::EvidenceComplete) => true,
            (DisputeStatus::Open, DisputeStatus::Deliberating) => true, // Direct if evidence already exists
            (DisputeStatus::Open, DisputeStatus::Dismissed) => true,
            (DisputeStatus::Open, DisputeStatus::Expired) => true,

            // From EvidenceComplete: can go to Deliberating, Dismissed, or Expired
            (DisputeStatus::EvidenceComplete, DisputeStatus::Deliberating) => true,
            (DisputeStatus::EvidenceComplete, DisputeStatus::Dismissed) => true,
            (DisputeStatus::EvidenceComplete, DisputeStatus::Expired) => true,

            // From Deliberating: can go to Resolved, Dismissed, or Expired
            (DisputeStatus::Deliberating, DisputeStatus::Resolved) => true,
            (DisputeStatus::Deliberating, DisputeStatus::Dismissed) => true,
            (DisputeStatus::Deliberating, DisputeStatus::Expired) => true,

            // From Resolved: can only go to Appealed
            (DisputeStatus::Resolved, DisputeStatus::Appealed) => true,

            // Terminal states: cannot transition from these
            (DisputeStatus::Appealed, _) => false,
            (DisputeStatus::Dismissed, _) => false,
            (DisputeStatus::Expired, _) => false,

            // Same state: no-op is allowed
            (current, new) if current == new => true,

            // All other transitions are invalid
            _ => false,
        }
    }

    /// SECURITY FIX #19: Attempt to transition to a new status with validation
    /// Returns Ok(()) if transition is valid, Err otherwise
    pub fn try_transition(&mut self, new_status: DisputeStatus) -> Result<(), ArbitrationError> {
        if !self.is_valid_transition(new_status) {
            return Err(ArbitrationError::InvalidState {
                expected: format!("valid transition from {:?}", self.status),
                actual: format!("attempted transition to {:?}", new_status),
            });
        }
        self.status = new_status;
        Ok(())
    }

    /// Check if the dispute is stale and should be auto-dismissed
    /// A dispute is stale if:
    /// 1. It's still Open and evidence deadline passed without evidence, OR
    /// 2. It's Deliberating but deliberation deadline passed without quorum, OR
    /// 3. It's exceeded the maximum duration
    pub fn is_stale(&self, current_block: BlockNumber) -> bool {
        // Maximum duration exceeded
        if self.is_expired(current_block) {
            return true;
        }

        match self.status {
            DisputeStatus::Open => {
                // Evidence deadline passed but still Open (no evidence submitted)
                current_block > self.evidence_deadline && self.evidence.is_empty()
            }
            DisputeStatus::EvidenceComplete => {
                // Evidence complete but no jury selected within reasonable time
                // Give 7 days after evidence complete to select jury
                current_block > self.evidence_deadline + 100_800 && self.jury_members.is_empty()
            }
            DisputeStatus::Deliberating => {
                // Deliberation deadline passed without enough votes
                if let Some(deadline) = self.deliberation_deadline {
                    let quorum = (self.jury_members.len() / 2) + 1;
                    current_block > deadline && self.jury_votes.len() < quorum
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}

/// Types of disputes that can be arbitrated
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeType {
    /// Validator misbehavior (double signing, invalid blocks, etc.)
    ValidatorMisconduct,

    /// Cross-chain treaty violation (not yet implemented)
    CrossChainTreatyViolation,

    /// Fraudulent exit attempt
    FraudulentExit,

    /// Conflicting state roots between chains
    StateRootDispute,
}

/// Where the dispute should be resolved
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Jurisdiction {
    /// Resolved on the sidechain itself
    Local(ChainId),

    /// Resolved on the host chain
    Host(ChainId),

    /// Resolved on the root chain (most serious)
    Root,
}

/// Current status of a dispute
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeStatus {
    /// Awaiting evidence submission
    Open,

    /// Evidence submitted, awaiting jury selection
    EvidenceComplete,

    /// Jury selected via VRF, awaiting votes
    Deliberating,

    /// Jury has reached a verdict
    Resolved,

    /// Appealed to higher jurisdiction
    Appealed,

    /// Dismissed (insufficient evidence or frivolous)
    Dismissed,

    /// Expired due to exceeding maximum duration (auto-dismissed)
    Expired,
}

/// Evidence that can be submitted for a dispute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Evidence {
    /// Fraud proof (from Phase 5)
    FraudProof(FraudProof),

    /// Merkle proof of state (from Phase 4)
    StateProof {
        proof: MerkleProof,
        description: String,
    },

    /// Block headers as evidence
    BlockHeaders {
        headers: Vec<super::block::BlockHeader>,
        description: String,
    },

    /// General textual evidence
    TextEvidence {
        content: String,
        submitted_by: AccountId,
        submitted_at: BlockNumber,
    },
}

/// A jury member's vote on a dispute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JuryVote {
    /// The juror who voted
    pub juror: AccountId,

    /// Their verdict
    pub verdict: Verdict,

    /// Optional justification for their vote
    pub justification: Option<String>,

    /// When the vote was submitted
    pub timestamp: BlockNumber,
}

/// Verdict a juror can render
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Accused is guilty of the claimed violation
    Guilty,

    /// Accused is not guilty / evidence insufficient
    NotGuilty,

    /// Juror abstains from voting
    Abstain,
}

/// Final decision from the jury
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JuryDecision {
    /// The dispute this decision is for
    pub dispute_id: DisputeId,

    /// All jury votes
    pub votes: Vec<JuryVote>,

    /// The final verdict (majority wins)
    pub verdict: Verdict,

    /// Percentage of jury that voted guilty
    pub conviction_percentage: u8,

    /// When the decision was made
    pub decided_at: BlockNumber,

    /// Enforcement action to take
    pub enforcement: Option<Enforcement>,
}

impl JuryDecision {
    /// Calculate verdict from jury votes
    pub fn tally_votes(votes: Vec<JuryVote>, decided_at: BlockNumber, dispute_id: DisputeId) -> Self {
        let total_votes = votes.len() as u8;
        if total_votes == 0 {
            return Self {
                dispute_id,
                votes,
                verdict: Verdict::NotGuilty,
                conviction_percentage: 0,
                decided_at,
                enforcement: None,
            };
        }

        let guilty_votes = votes.iter().filter(|v| v.verdict == Verdict::Guilty).count() as u8;
        let not_guilty_votes = votes.iter().filter(|v| v.verdict == Verdict::NotGuilty).count() as u8;

        let conviction_percentage = ((guilty_votes as u16 * 100) / total_votes as u16) as u8;

        // Majority rule: more than 50% guilty votes = guilty verdict
        let verdict = if guilty_votes > not_guilty_votes {
            Verdict::Guilty
        } else {
            Verdict::NotGuilty
        };

        Self {
            dispute_id,
            votes,
            verdict,
            conviction_percentage,
            decided_at,
            enforcement: None,
        }
    }
}

/// Action to take when enforcing a jury decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Enforcement {
    /// Slash a validator's stake
    SlashValidator {
        validator: AccountId,
        amount: u128,
        severity: super::fraud::FraudSeverity,
    },

    /// Reduce validator credits
    SlashValidatorCredits {
        validator: AccountId,
        amount: u32,
    },

    /// Force a chain into purge
    ForceExit {
        chain_id: ChainId,
    },

    /// Mark a state root as invalid
    InvalidateStateRoot {
        chain_id: ChainId,
        root: super::primitives::Hash,
    },

    /// Slash the accuser (false claim)
    SlashAccuser {
        accuser: AccountId,
        amount: u128,
    },

    /// No action (not guilty verdict)
    None,
}

/// Errors that can occur during arbitration
#[derive(Debug, Clone, thiserror::Error)]
pub enum ArbitrationError {
    #[error("Dispute not found: {0}")]
    DisputeNotFound(DisputeId),

    #[error("Evidence submission period closed")]
    EvidenceWindowClosed,

    #[error("Deliberation period closed")]
    DeliberationClosed,

    #[error("Not a jury member for this dispute")]
    NotJuryMember,

    #[error("Jury already voted")]
    AlreadyVoted,

    #[error("Insufficient jury members: {0} < minimum")]
    InsufficientJury(usize),

    #[error("Invalid dispute state: expected {expected}, got {actual}")]
    InvalidState {
        expected: String,
        actual: String,
    },

    #[error("VRF selection failed: {0}")]
    VRFSelectionFailed(String),

    #[error("Dispute already resolved")]
    AlreadyResolved,

    #[error("Invalid jurisdiction for chain")]
    InvalidJurisdiction,

    #[error("Maximum evidence submissions reached")]
    EvidenceLimitReached,

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Deliberation deadline not passed")]
    DeliberationNotComplete,

    /// FIX: New error type for duplicate dispute detection
    #[error("Duplicate dispute ID detected: {0} (possible state corruption)")]
    DuplicateDispute(DisputeId),

    /// SECURITY FIX #4: Unauthorized action error
    #[error("Unauthorized to {action}: requires {required}")]
    Unauthorized {
        action: String,
        required: String,
    },

    /// SECURITY FIX #30: Maximum number of disputes reached (overflow protection)
    #[error("Maximum number of disputes reached (u64::MAX)")]
    MaxDisputesReached,
}

/// Jury size configuration
pub const MIN_JURY_SIZE: usize = 7;
pub const MAX_JURY_SIZE: usize = 21;
pub const DEFAULT_JURY_SIZE: usize = 13;

/// Maximum evidence submissions per dispute (prevents DoS attacks)
pub const MAX_EVIDENCE_COUNT: usize = 50;

/// Timing configuration (in blocks @ 6s/block)
pub const EVIDENCE_SUBMISSION_PERIOD: BlockNumber = 100_800;  // 7 days
pub const DELIBERATION_PERIOD: BlockNumber = 201_600;         // 14 days
pub const APPEAL_WINDOW: BlockNumber = 432_000;               // 30 days
/// Maximum total dispute duration (prevents indefinite exit blocking)
/// 7 days (evidence) + 14 days (deliberation) + 30 days (appeal) + 7 days (buffer) = 58 days
pub const MAX_DISPUTE_DURATION: BlockNumber = 835_200;        // 58 days

/// Arbitration rewards (in validator credits)
pub const ARBITRATION_VC_REWARD: u32 = 5;
pub const MAX_ARBITRATIONS_PER_YEAR: u32 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispute_creation() {
        let dispute = Dispute::new(
            1,
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            1000,
            Jurisdiction::Root,
        );

        assert_eq!(dispute.id, 1);
        assert_eq!(dispute.chain_id, ChainId(1));
        assert_eq!(dispute.status, DisputeStatus::Open);
        assert_eq!(dispute.evidence_deadline, 1000 + EVIDENCE_SUBMISSION_PERIOD);
    }

    #[test]
    fn test_evidence_submission_window() {
        let dispute = Dispute::new(
            1,
            ChainId(1),
            DisputeType::FraudulentExit,
            AccountId::from_bytes([1; 32]),
            1000,
            Jurisdiction::Host(ChainId(0)),
        );

        // Within window
        assert!(dispute.can_submit_evidence(1000));
        assert!(dispute.can_submit_evidence(50_000));

        // Outside window
        assert!(!dispute.can_submit_evidence(102_000));
    }

    #[test]
    fn test_jury_decision_tally_guilty() {
        let votes = vec![
            JuryVote {
                juror: AccountId::from_bytes([1; 32]),
                verdict: Verdict::Guilty,
                justification: None,
                timestamp: 1000,
            },
            JuryVote {
                juror: AccountId::from_bytes([2; 32]),
                verdict: Verdict::Guilty,
                justification: None,
                timestamp: 1001,
            },
            JuryVote {
                juror: AccountId::from_bytes([3; 32]),
                verdict: Verdict::NotGuilty,
                justification: None,
                timestamp: 1002,
            },
        ];

        let decision = JuryDecision::tally_votes(votes, 2000, 1);

        assert_eq!(decision.verdict, Verdict::Guilty);
        assert_eq!(decision.conviction_percentage, 66); // 2/3 = 66%
    }

    #[test]
    fn test_jury_decision_tally_not_guilty() {
        let votes = vec![
            JuryVote {
                juror: AccountId::from_bytes([1; 32]),
                verdict: Verdict::Guilty,
                justification: None,
                timestamp: 1000,
            },
            JuryVote {
                juror: AccountId::from_bytes([2; 32]),
                verdict: Verdict::NotGuilty,
                justification: None,
                timestamp: 1001,
            },
            JuryVote {
                juror: AccountId::from_bytes([3; 32]),
                verdict: Verdict::NotGuilty,
                justification: None,
                timestamp: 1002,
            },
        ];

        let decision = JuryDecision::tally_votes(votes, 2000, 1);

        assert_eq!(decision.verdict, Verdict::NotGuilty);
        assert_eq!(decision.conviction_percentage, 33); // 1/3 = 33%
    }

    #[test]
    fn test_jury_decision_with_abstentions() {
        let votes = vec![
            JuryVote {
                juror: AccountId::from_bytes([1; 32]),
                verdict: Verdict::Guilty,
                justification: None,
                timestamp: 1000,
            },
            JuryVote {
                juror: AccountId::from_bytes([2; 32]),
                verdict: Verdict::Abstain,
                justification: Some("Conflict of interest".to_string()),
                timestamp: 1001,
            },
            JuryVote {
                juror: AccountId::from_bytes([3; 32]),
                verdict: Verdict::NotGuilty,
                justification: None,
                timestamp: 1002,
            },
        ];

        let decision = JuryDecision::tally_votes(votes, 2000, 1);

        // With abstention, guilty=1, not_guilty=1, so not_guilty wins (tie goes to defendant)
        assert_eq!(decision.verdict, Verdict::NotGuilty);
        assert_eq!(decision.conviction_percentage, 33); // 1/3 = 33%
    }

    #[test]
    fn test_empty_jury_votes() {
        let decision = JuryDecision::tally_votes(vec![], 2000, 1);

        assert_eq!(decision.verdict, Verdict::NotGuilty);
        assert_eq!(decision.conviction_percentage, 0);
    }

    #[test]
    fn test_dispute_status_transitions() {
        let mut dispute = Dispute::new(
            1,
            ChainId(1),
            DisputeType::StateRootDispute,
            AccountId::from_bytes([1; 32]),
            1000,
            Jurisdiction::Local(ChainId(1)),
        );

        assert_eq!(dispute.status, DisputeStatus::Open);

        dispute.status = DisputeStatus::EvidenceComplete;
        assert_eq!(dispute.status, DisputeStatus::EvidenceComplete);

        dispute.status = DisputeStatus::Deliberating;
        dispute.deliberation_deadline = Some(1000 + DELIBERATION_PERIOD);
        assert!(dispute.can_vote(1000));
        assert!(!dispute.can_vote(203_000)); // Past deadline
    }

    #[test]
    fn test_dispute_expiration() {
        let dispute = Dispute::new(
            1,
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            1000,
            Jurisdiction::Root,
        );

        // Not expired within max duration
        assert!(!dispute.is_expired(1000));
        assert!(!dispute.is_expired(1000 + MAX_DISPUTE_DURATION - 1));

        // Expired after max duration
        assert!(dispute.is_expired(1000 + MAX_DISPUTE_DURATION + 1));
        assert!(dispute.is_expired(1000 + MAX_DISPUTE_DURATION + 100_000));
    }

    #[test]
    fn test_dispute_expiration_terminal_states() {
        let mut dispute = Dispute::new(
            1,
            ChainId(1),
            DisputeType::ValidatorMisconduct,
            AccountId::from_bytes([1; 32]),
            1000,
            Jurisdiction::Root,
        );

        let far_future = 1000 + MAX_DISPUTE_DURATION + 1_000_000;

        // Resolved disputes are never "expired" (already handled)
        dispute.status = DisputeStatus::Resolved;
        assert!(!dispute.is_expired(far_future));

        // Dismissed disputes are never "expired"
        dispute.status = DisputeStatus::Dismissed;
        assert!(!dispute.is_expired(far_future));

        // Already expired disputes are never "expired" again
        dispute.status = DisputeStatus::Expired;
        assert!(!dispute.is_expired(far_future));
    }

    #[test]
    fn test_dispute_is_stale_no_evidence() {
        let dispute = Dispute::new(
            1,
            ChainId(1),
            DisputeType::FraudulentExit,
            AccountId::from_bytes([1; 32]),
            1000,
            Jurisdiction::Host(ChainId(0)),
        );

        // Not stale before evidence deadline
        assert!(!dispute.is_stale(1000));
        assert!(!dispute.is_stale(50_000));

        // Stale after evidence deadline with no evidence
        assert!(dispute.is_stale(102_000));
    }

    #[test]
    fn test_dispute_absolute_deadline() {
        let dispute = Dispute::new(
            1,
            ChainId(1),
            DisputeType::StateRootDispute,
            AccountId::from_bytes([1; 32]),
            5000,
            Jurisdiction::Root,
        );

        assert_eq!(dispute.absolute_deadline(), 5000 + MAX_DISPUTE_DURATION);
    }
}
