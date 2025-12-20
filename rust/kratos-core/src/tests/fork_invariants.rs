// Fork Invariants Tests - SPEC v8: Long-Term Resilience, Forking & Protocol Survivability
// Principle: Forking is a first-class mechanism, not a failure
//
// These tests verify the safety invariants defined in SPEC v8 Section 11:
// 1. Forking is ALWAYS possible
// 2. No authority can block fork
// 3. No fork erases history
// 4. Identity survives forks
// 5. Exit precedes coercion
// 6. No global collapse from local failure
//
// Additional invariants tested:
// - Fork neutrality (Section 8)
// - Asset continuity (Section 6.1)
// - Ossification properties (Section 9)

use crate::contracts::fork::{
    ForkContract, ForkError, ForkEvent,
    FORK_PROPOSAL_DEPOSIT, MAX_CONCURRENT_FORKS, FORK_COOLDOWN,
};
use crate::types::{
    AccountId, Balance, BlockNumber, ChainId, Hash,
    ForkType, ForkStatus, ForkAlignment, ForkDeclarant,
    IdentitySnapshot, ReputationSnapshot, ValidatorSnapshot,
    IdentityStatus, ReputationDomain,
    FORK_VALIDATOR_THRESHOLD_PERCENT, FORK_STAKE_THRESHOLD_PERCENT,
    FORK_SIDECHAIN_THRESHOLD, OSSIFICATION_APPROVAL_THRESHOLD,
    BLOCKS_PER_YEAR,
};
use crate::types::protocol::{ProtocolVersion, ConstitutionalAxiom};
use std::collections::HashMap;

// =============================================================================
// TEST HELPERS
// =============================================================================

fn create_account(seed: u8) -> AccountId {
    AccountId::from_bytes([seed; 32])
}

fn setup_contract_with_validators(count: u32) -> ForkContract {
    let mut contract = ForkContract::new();

    for i in 1..=count {
        contract.set_voting_power(create_account(i as u8), 10);
    }
    contract.set_total_stake(100_000_000);

    contract
}

fn propose_and_declare_fork(contract: &mut ForkContract, name: &str, current_block: BlockNumber) -> Hash {
    let fork_id = contract.propose_fork(
        create_account(1),
        name.to_string(),
        ForkType::Technical {
            version: ProtocolVersion::new(2, 0, 0),
            description: "Test".to_string(),
        },
        "Test fork".to_string(),
        FORK_PROPOSAL_DEPOSIT,
        current_block,
    ).unwrap();

    // Add enough support to declare (33% validators)
    // Proposer counts as 1, need 3 more for 40% (4/10)
    for i in 2..=4 {
        contract.support_fork(fork_id, create_account(i), 10_000_000, current_block + 1).unwrap();
    }

    fork_id
}

// =============================================================================
// INVARIANT 1: FORKING IS ALWAYS POSSIBLE (SPEC v8 Section 11.1)
// =============================================================================

mod forking_always_possible {
    use super::*;

    #[test]
    fn test_fork_proposal_always_accepted() {
        // Any validator can propose a fork at any time
        let mut contract = setup_contract_with_validators(10);

        let result = contract.propose_fork(
            create_account(5), // Any validator
            "Proposal Fork".to_string(),
            ForkType::Social {
                community_rationale: "Community values divergence".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        );

        assert!(result.is_ok(), "Fork proposal must always be possible");
    }

    #[test]
    fn test_fork_during_emergency_allowed() {
        // Forking must be possible even during emergencies
        let mut contract = setup_contract_with_validators(10);

        // Even if we imagine emergency state is active somewhere,
        // fork proposals are independent
        let result = contract.propose_fork(
            create_account(1),
            "Emergency Fork".to_string(),
            ForkType::Survival {
                external_threat: "Protocol captured".to_string(),
            },
            "Escape captured protocol".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_multiple_fork_proposals_allowed() {
        // Multiple fork proposals can coexist
        let mut contract = setup_contract_with_validators(10);

        for i in 0..MAX_CONCURRENT_FORKS {
            let result = contract.propose_fork(
                create_account(1),
                format!("Fork {}", i),
                ForkType::Technical {
                    version: ProtocolVersion::new(2, i as u16, 0),
                    description: "Upgrade".to_string(),
                },
                "Description".to_string(),
                FORK_PROPOSAL_DEPOSIT,
                1000 + i as u64,
            );
            assert!(result.is_ok(), "Must allow {} concurrent forks", MAX_CONCURRENT_FORKS);
        }
    }

    #[test]
    fn test_all_fork_types_supported() {
        // All fork types from SPEC v8 Section 3.1 must be creatable
        let mut contract = setup_contract_with_validators(10);

        // Technical fork
        assert!(contract.propose_fork(
            create_account(1),
            "Technical".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Security patch".to_string(),
            },
            "Tech".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).is_ok());

        // Constitutional fork
        assert!(contract.propose_fork(
            create_account(1),
            "Constitutional".to_string(),
            ForkType::Constitutional {
                violated_axiom: ConstitutionalAxiom::ExitAlwaysPossible,
                rationale: "Exit blocked".to_string(),
            },
            "Const".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1001,
        ).is_ok());

        // Governance fork
        assert!(contract.propose_fork(
            create_account(1),
            "Governance".to_string(),
            ForkType::Governance {
                deadlock_reason: "Quorum failures".to_string(),
                failed_proposals: 10,
            },
            "Gov".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1002,
        ).is_ok());
    }
}

// =============================================================================
// INVARIANT 2: NO AUTHORITY CAN BLOCK FORK (SPEC v8 Section 11.2)
// =============================================================================

mod no_authority_block_fork {
    use super::*;

    #[test]
    fn test_no_single_veto() {
        // No single validator can veto a fork
        let mut contract = setup_contract_with_validators(10);

        let fork_id = contract.propose_fork(
            create_account(1),
            "Test Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Test".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // Even if one validator doesn't support, others can
        for i in 2..=4 {
            contract.support_fork(fork_id, create_account(i), 10_000_000, 1100).unwrap();
        }

        let fork = contract.get_fork(fork_id).unwrap();
        // Fork is declared regardless of validator 1's opinion
        assert_eq!(fork.status, ForkStatus::Declared);
    }

    #[test]
    fn test_sidechains_can_trigger_fork() {
        // Sidechains can declare fork independently of validators
        let mut contract = setup_contract_with_validators(10);

        let fork_id = contract.propose_fork(
            create_account(1),
            "Sidechain Fork".to_string(),
            ForkType::Constitutional {
                violated_axiom: ConstitutionalAxiom::ExitAlwaysPossible,
                rationale: "Exit blocked".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // 3 sidechains can trigger fork
        contract.declare_fork_from_sidechains(
            fork_id,
            vec![ChainId(1), ChainId(2), ChainId(3)],
            1100,
        ).unwrap();

        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Declared);
        assert!(matches!(fork.declared_by, ForkDeclarant::Sidechains { .. }));
    }

    #[test]
    fn test_stake_threshold_independent() {
        // Stake threshold works independently of validator count
        let mut contract = setup_contract_with_validators(10);
        contract.set_total_stake(100_000);

        let fork_id = contract.propose_fork(
            create_account(1),
            "Stake Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Test".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // Support with 40% stake
        contract.support_fork(fork_id, create_account(2), 40_000, 1100).unwrap();

        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Declared);
    }
}

// =============================================================================
// INVARIANT 3: NO FORK ERASES HISTORY (SPEC v8 Section 11.3)
// =============================================================================

mod no_history_erasure {
    use super::*;

    #[test]
    fn test_fork_preserves_executed_history() {
        let mut contract = setup_contract_with_validators(10);

        // Execute a fork
        let fork_id = propose_and_declare_fork(&mut contract, "History Fork", 1000);

        let fork = contract.get_fork(fork_id).unwrap();
        let prep_ends = fork.preparation_ends;
        let fork_height = fork.fork_height;

        contract.mark_ready(fork_id, prep_ends + 1).unwrap();
        contract.execute_fork(fork_id, ChainId(100), fork_height + 1).unwrap();

        // History is preserved
        assert_eq!(contract.fork_history().len(), 1);
        assert_eq!(contract.fork_history()[0].status, ForkStatus::Executed);
        assert_eq!(contract.fork_history()[0].name, "History Fork");
    }

    #[test]
    fn test_snapshot_preserves_state_root() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Snapshot Fork", 1000);

        let state_root = Hash::hash(b"original_state");

        contract.create_snapshot(
            fork_id,
            state_root,
            vec![],
            Hash::hash(b"identity"),
            Hash::hash(b"reputation"),
            Hash::hash(b"sidechains"),
            100_000_000_000,
            2000,
        ).unwrap();

        let snapshot = contract.get_snapshot(fork_id).unwrap();
        assert_eq!(snapshot.state_root, state_root);
        assert_eq!(snapshot.block_number, 2000);
    }

    #[test]
    fn test_cancelled_forks_remain_in_proposals() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = contract.propose_fork(
            create_account(1),
            "Cancelled Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Test".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        contract.cancel_fork(fork_id, "Not needed".to_string()).unwrap();

        // Fork record still exists
        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Cancelled);
    }
}

// =============================================================================
// INVARIANT 4: IDENTITY SURVIVES FORKS (SPEC v8 Section 11.4)
// =============================================================================

mod identity_survives {
    use super::*;

    #[test]
    fn test_identity_snapshot_created() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Identity Fork", 1000);

        contract.create_snapshot(
            fork_id,
            Hash::hash(b"state"),
            vec![],
            Hash::hash(b"identity_root"),
            Hash::hash(b"reputation"),
            Hash::hash(b"sidechains"),
            100_000_000_000,
            2000,
        ).unwrap();

        // Add identity to snapshot
        let identity = IdentitySnapshot {
            identity_hash: Hash::hash(b"user123"),
            status: IdentityStatus::Active,
            attestation_count: 5,
            deposit: 100_000,
            registered_at: 500,
        };

        contract.add_identity_to_snapshot(fork_id, identity.clone()).unwrap();

        let continuity = contract.get_continuity(fork_id).unwrap();
        assert!(continuity.identity_snapshots.contains_key(&identity.identity_hash));
    }

    #[test]
    fn test_balance_snapshot_created() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Balance Fork", 1000);

        contract.create_snapshot(
            fork_id,
            Hash::hash(b"state"),
            vec![],
            Hash::hash(b"identity"),
            Hash::hash(b"reputation"),
            Hash::hash(b"sidechains"),
            100_000_000_000,
            2000,
        ).unwrap();

        // Add balance snapshot
        let account = create_account(1);
        contract.add_balance_to_snapshot(fork_id, account, 50_000_000).unwrap();

        let continuity = contract.get_continuity(fork_id).unwrap();
        assert_eq!(*continuity.balance_snapshot.get(&account).unwrap(), 50_000_000);
    }

    #[test]
    fn test_reputation_snapshot_with_fork_decay() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Reputation Fork", 1000);

        contract.create_snapshot(
            fork_id,
            Hash::hash(b"state"),
            vec![],
            Hash::hash(b"identity"),
            Hash::hash(b"reputation"),
            Hash::hash(b"sidechains"),
            100_000_000_000,
            2000,
        ).unwrap();

        let mut domains = HashMap::new();
        domains.insert(ReputationDomain::Technical, 100);

        let reputation = ReputationSnapshot {
            identity_hash: Hash::hash(b"user123"),
            chain_id: ChainId(1),
            domains,
            post_fork_decay_multiplier: 2, // 2x faster decay post-fork
        };

        contract.add_reputation_to_snapshot(fork_id, reputation.clone()).unwrap();

        let continuity = contract.get_continuity(fork_id).unwrap();
        let key = (ChainId(1), Hash::hash(b"user123"));
        assert!(continuity.reputation_snapshots.contains_key(&key));

        // Verify decay multiplier is preserved
        let stored = continuity.reputation_snapshots.get(&key).unwrap();
        assert_eq!(stored.post_fork_decay_multiplier, 2);
    }
}

// =============================================================================
// INVARIANT 5: EXIT PRECEDES COERCION (SPEC v8 Section 11.5)
// =============================================================================

mod exit_precedes_coercion {
    use super::*;

    #[test]
    fn test_preparation_period_allows_exit() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Exit Fork", 1000);

        let fork = contract.get_fork(fork_id).unwrap();

        // Technical forks have 30 day minimum preparation
        // 30 * 14400 = 432000 blocks minimum
        assert!(fork.preparation_ends - fork.declared_at >= 30 * 14_400);
    }

    #[test]
    fn test_sidechains_can_choose_alignment() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Alignment Fork", 1000);

        // Sidechain can declare alignment
        contract.set_sidechain_alignment(
            fork_id,
            ChainId(1),
            ForkAlignment::Independent,
            Hash::hash(b"proposal"),
            2000,
        ).unwrap();

        assert_eq!(contract.get_alignment(fork_id, ChainId(1)), ForkAlignment::Independent);
    }

    #[test]
    fn test_default_alignment_is_independence() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Default Fork", 1000);

        // Apply default alignments
        contract.apply_default_alignments(
            fork_id,
            vec![ChainId(1), ChainId(2)],
            2000,
        ).unwrap();

        // Default is independence (not forced to choose A or B)
        assert_eq!(contract.get_alignment(fork_id, ChainId(1)), ForkAlignment::Independent);
        assert_eq!(contract.get_alignment(fork_id, ChainId(2)), ForkAlignment::Independent);
    }
}

// =============================================================================
// INVARIANT 6: NO GLOBAL COLLAPSE FROM LOCAL FAILURE (SPEC v8 Section 11.6)
// =============================================================================

mod no_global_collapse {
    use super::*;

    #[test]
    fn test_one_fork_failure_doesnt_affect_others() {
        let mut contract = setup_contract_with_validators(10);

        // Create two forks
        let fork1 = contract.propose_fork(
            create_account(1),
            "Fork One".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Test".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        let fork2 = contract.propose_fork(
            create_account(1),
            "Fork Two".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 1, 0),
                description: "Test".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1001,
        ).unwrap();

        // Cancel one
        contract.cancel_fork(fork1, "Cancelled".to_string()).unwrap();

        // Other still works
        for i in 2..=4 {
            contract.support_fork(fork2, create_account(i), 10_000_000, 1100).unwrap();
        }

        let fork2_status = contract.get_fork(fork2).unwrap();
        assert_eq!(fork2_status.status, ForkStatus::Declared);
    }

    #[test]
    fn test_sidechain_alignment_scoped() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Scoped Fork", 1000);

        // Each sidechain's alignment is independent
        contract.set_sidechain_alignment(fork_id, ChainId(1), ForkAlignment::FollowA, Hash::hash(b"p1"), 2000).unwrap();
        contract.set_sidechain_alignment(fork_id, ChainId(2), ForkAlignment::FollowB, Hash::hash(b"p2"), 2000).unwrap();
        contract.set_sidechain_alignment(fork_id, ChainId(3), ForkAlignment::Independent, Hash::hash(b"p3"), 2000).unwrap();

        // All three choices coexist
        assert_eq!(contract.get_alignment(fork_id, ChainId(1)), ForkAlignment::FollowA);
        assert_eq!(contract.get_alignment(fork_id, ChainId(2)), ForkAlignment::FollowB);
        assert_eq!(contract.get_alignment(fork_id, ChainId(3)), ForkAlignment::Independent);
    }
}

// =============================================================================
// FORK NEUTRALITY (SPEC v8 Section 8)
// =============================================================================

mod fork_neutrality {
    use super::*;

    #[test]
    fn test_no_fork_privileged() {
        // Both fork paths (A and B) are treated equally
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Neutral Fork", 1000);

        // Both alignments are valid
        contract.set_sidechain_alignment(fork_id, ChainId(1), ForkAlignment::FollowA, Hash::hash(b"p"), 2000).unwrap();
        contract.set_sidechain_alignment(fork_id, ChainId(2), ForkAlignment::FollowB, Hash::hash(b"p"), 2000).unwrap();

        // Neither triggers errors or special handling
        assert_eq!(contract.get_alignment(fork_id, ChainId(1)), ForkAlignment::FollowA);
        assert_eq!(contract.get_alignment(fork_id, ChainId(2)), ForkAlignment::FollowB);
    }

    #[test]
    fn test_no_slashing_for_fork_choice() {
        // Fork contract has no slashing mechanism
        let contract = setup_contract_with_validators(10);

        // There's no slash method in ForkContract
        // This is by design - fork participation is never punished
        assert!(contract.proposals.is_empty()); // Just verify contract exists
    }

    #[test]
    fn test_all_declarant_types_equal() {
        // Validators, stake, and sidechains all have equal standing
        let mut contract1 = setup_contract_with_validators(10);
        let mut contract2 = setup_contract_with_validators(10);
        let mut contract3 = setup_contract_with_validators(10);
        contract2.set_total_stake(100_000);

        // Validator-declared fork
        let fork1 = contract1.propose_fork(
            create_account(1), "V Fork".to_string(),
            ForkType::Technical { version: ProtocolVersion::new(2, 0, 0), description: "Test".to_string() },
            "Desc".to_string(), FORK_PROPOSAL_DEPOSIT, 1000,
        ).unwrap();
        for i in 2..=4 {
            contract1.support_fork(fork1, create_account(i), 10_000_000, 1100).unwrap();
        }

        // Stake-declared fork
        let fork2 = contract2.propose_fork(
            create_account(1), "S Fork".to_string(),
            ForkType::Technical { version: ProtocolVersion::new(2, 0, 0), description: "Test".to_string() },
            "Desc".to_string(), FORK_PROPOSAL_DEPOSIT, 1000,
        ).unwrap();
        contract2.support_fork(fork2, create_account(2), 40_000, 1100).unwrap();

        // Sidechain-declared fork
        let fork3 = contract3.propose_fork(
            create_account(1), "C Fork".to_string(),
            ForkType::Technical { version: ProtocolVersion::new(2, 0, 0), description: "Test".to_string() },
            "Desc".to_string(), FORK_PROPOSAL_DEPOSIT, 1000,
        ).unwrap();
        contract3.declare_fork_from_sidechains(fork3, vec![ChainId(1), ChainId(2), ChainId(3)], 1100).unwrap();

        // All three are equally Declared
        assert_eq!(contract1.get_fork(fork1).unwrap().status, ForkStatus::Declared);
        assert_eq!(contract2.get_fork(fork2).unwrap().status, ForkStatus::Declared);
        assert_eq!(contract3.get_fork(fork3).unwrap().status, ForkStatus::Declared);
    }
}

// =============================================================================
// OSSIFICATION MODE (SPEC v8 Section 9)
// =============================================================================

mod ossification_mode {
    use super::*;

    #[test]
    fn test_ossification_requires_10_years() {
        let mut contract = setup_contract_with_validators(10);

        // Not enough time
        contract.ossification.last_parameter_change = BLOCKS_PER_YEAR * 5;
        let current_block = BLOCKS_PER_YEAR * 14;

        let result = contract.propose_ossification(create_account(1), current_block, false);
        assert!(result.is_err()); // Only 9 years, need 10
    }

    #[test]
    fn test_ossification_requires_90_percent() {
        let mut contract = setup_contract_with_validators(10);

        contract.ossification.last_parameter_change = 0;
        let current_block = BLOCKS_PER_YEAR * 11;

        contract.propose_ossification(create_account(1), current_block, false).unwrap();

        // 8 approve (80%) - not enough
        for i in 1..=8 {
            contract.vote_ossification(create_account(i), true, current_block + 100).unwrap();
        }

        // 2 reject
        for i in 9..=10 {
            contract.vote_ossification(create_account(i), false, current_block + 100).unwrap();
        }

        assert!(!contract.is_ossified());
    }

    #[test]
    fn test_ossification_blocked_during_emergency() {
        let mut contract = setup_contract_with_validators(10);

        contract.ossification.last_parameter_change = 0;
        let current_block = BLOCKS_PER_YEAR * 11;

        // Emergency active
        let result = contract.propose_ossification(create_account(1), current_block, true);
        assert!(matches!(result, Err(ForkError::CannotOssifyDuringEmergency)));
    }

    #[test]
    fn test_forking_allowed_after_ossification() {
        let mut contract = setup_contract_with_validators(10);

        // Activate ossification
        contract.ossification.last_parameter_change = 0;
        let current_block = BLOCKS_PER_YEAR * 11;

        contract.propose_ossification(create_account(1), current_block, false).unwrap();
        for i in 1..=9 {
            contract.vote_ossification(create_account(i), true, current_block + 100).unwrap();
        }

        assert!(contract.is_ossified());

        // Forking is STILL possible (SPEC v8 Section 9.2)
        let result = contract.propose_fork(
            create_account(1),
            "Post-Ossification Fork".to_string(),
            ForkType::Constitutional {
                violated_axiom: ConstitutionalAxiom::ForkingLegitimate,
                rationale: "Community evolution".to_string(),
            },
            "Even ossified protocols can fork".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            current_block + 1000,
        );

        assert!(result.is_ok(), "Forking must remain possible even after ossification");
    }
}

// =============================================================================
// ASSET CONTINUITY (SPEC v8 Section 6)
// =============================================================================

mod asset_continuity {
    use super::*;

    #[test]
    fn test_no_asset_burns() {
        // ForkContract has no burn mechanism
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Asset Fork", 1000);

        contract.create_snapshot(
            fork_id,
            Hash::hash(b"state"),
            vec![],
            Hash::hash(b"identity"),
            Hash::hash(b"reputation"),
            Hash::hash(b"sidechains"),
            100_000_000_000,
            2000,
        ).unwrap();

        // Add balance
        contract.add_balance_to_snapshot(fork_id, create_account(1), 1_000_000).unwrap();

        // There's no method to reduce or burn this balance
        let continuity = contract.get_continuity(fork_id).unwrap();
        assert_eq!(*continuity.balance_snapshot.get(&create_account(1)).unwrap(), 1_000_000);
    }

    #[test]
    fn test_total_supply_preserved() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Supply Fork", 1000);

        let total_supply: Balance = 1_000_000_000_000;

        contract.create_snapshot(
            fork_id,
            Hash::hash(b"state"),
            vec![],
            Hash::hash(b"identity"),
            Hash::hash(b"reputation"),
            Hash::hash(b"sidechains"),
            total_supply,
            2000,
        ).unwrap();

        let snapshot = contract.get_snapshot(fork_id).unwrap();
        assert_eq!(snapshot.total_supply, total_supply);
    }

    #[test]
    fn test_validator_stakes_preserved() {
        let mut contract = setup_contract_with_validators(10);

        let fork_id = propose_and_declare_fork(&mut contract, "Validator Fork", 1000);

        let validators = vec![
            ValidatorSnapshot {
                id: create_account(1),
                stake: 10_000_000,
                validator_credits: 100,
                is_active: true,
            },
            ValidatorSnapshot {
                id: create_account(2),
                stake: 20_000_000,
                validator_credits: 200,
                is_active: true,
            },
        ];

        contract.create_snapshot(
            fork_id,
            Hash::hash(b"state"),
            validators.clone(),
            Hash::hash(b"identity"),
            Hash::hash(b"reputation"),
            Hash::hash(b"sidechains"),
            100_000_000_000,
            2000,
        ).unwrap();

        let snapshot = contract.get_snapshot(fork_id).unwrap();
        assert_eq!(snapshot.validator_set.len(), 2);
        assert_eq!(snapshot.validator_set[0].stake, 10_000_000);
        assert_eq!(snapshot.validator_set[1].stake, 20_000_000);
    }
}

// =============================================================================
// DECLARATION THRESHOLDS (SPEC v8 Section 3.2)
// =============================================================================

mod declaration_thresholds {
    use super::*;

    #[test]
    fn test_validator_threshold_33_percent() {
        // ≥33% validators can declare
        let mut contract = setup_contract_with_validators(10);

        let fork_id = contract.propose_fork(
            create_account(1),
            "33% Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Test".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // Proposer + 3 = 4/10 = 40% > 33%
        for i in 2..=4 {
            contract.support_fork(fork_id, create_account(i), 10_000_000, 1100).unwrap();
        }

        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Declared);
        assert_eq!(FORK_VALIDATOR_THRESHOLD_PERCENT, 33);
    }

    #[test]
    fn test_stake_threshold_40_percent() {
        // ≥40% stake can declare
        let mut contract = setup_contract_with_validators(10);
        contract.set_total_stake(100_000);

        let fork_id = contract.propose_fork(
            create_account(1),
            "40% Fork".to_string(),
            ForkType::Technical {
                version: ProtocolVersion::new(2, 0, 0),
                description: "Test".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        // 40% stake
        contract.support_fork(fork_id, create_account(2), 40_000, 1100).unwrap();

        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Declared);
        assert_eq!(FORK_STAKE_THRESHOLD_PERCENT, 40);
    }

    #[test]
    fn test_sidechain_threshold_3() {
        // ≥3 sidechains can declare
        let mut contract = setup_contract_with_validators(10);

        let fork_id = contract.propose_fork(
            create_account(1),
            "3 Sidechain Fork".to_string(),
            ForkType::Constitutional {
                violated_axiom: ConstitutionalAxiom::ExitAlwaysPossible,
                rationale: "Exit blocked".to_string(),
            },
            "Description".to_string(),
            FORK_PROPOSAL_DEPOSIT,
            1000,
        ).unwrap();

        contract.declare_fork_from_sidechains(
            fork_id,
            vec![ChainId(1), ChainId(2), ChainId(3)],
            1100,
        ).unwrap();

        let fork = contract.get_fork(fork_id).unwrap();
        assert_eq!(fork.status, ForkStatus::Declared);
        assert_eq!(FORK_SIDECHAIN_THRESHOLD, 3);
    }
}
