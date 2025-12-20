// Integration Tests - SPEC v3.1 Phase 9
// End-to-end tests for complete sidechain lifecycle and cross-chain operations

#[cfg(test)]
mod lifecycle_tests {
    use crate::contracts::governance::{
        GovernanceContract, ProposalType, ProposalStatus, Vote,
        VOTING_PERIOD, EXIT_TIMELOCK,
    };
    use crate::contracts::messaging::{
        MessagingContract, MessageType, MessageStatus, GovernanceNotificationType,
        BASE_MESSAGE_FEE,
    };
    use crate::contracts::sidechains::ChainRegistry;
    use crate::types::{
        AccountId, Balance, BlockNumber, ChainId, ChainStatus, SecurityMode,
        PurgeTrigger, INACTIVITY_THRESHOLD_V3_1, PURGE_WARNING_PERIOD,
        WITHDRAWAL_WINDOW_DURATION, Hash,
    };
    use crate::types::merkle::StateMerkleTree;

    // ===== HELPER FUNCTIONS =====

    fn create_accounts() -> (AccountId, AccountId, AccountId, AccountId) {
        (
            AccountId::from_bytes([1; 32]),
            AccountId::from_bytes([2; 32]),
            AccountId::from_bytes([3; 32]),
            AccountId::from_bytes([4; 32]),
        )
    }

    fn setup_governance_with_voters(chain_id: ChainId) -> GovernanceContract {
        let mut gov = GovernanceContract::new();
        let (alice, bob, charlie, dave) = create_accounts();

        gov.set_voting_power(chain_id, alice, 1000);
        gov.set_voting_power(chain_id, bob, 1000);
        gov.set_voting_power(chain_id, charlie, 1000);
        gov.set_voting_power(chain_id, dave, 1000);

        gov
    }

    fn create_valid_merkle_proof(block_number: BlockNumber, chain_id: ChainId) -> crate::types::merkle::MerkleProof {
        let leaves = vec![
            b"leaf0".to_vec(),
            b"leaf1".to_vec(),
            b"leaf2".to_vec(),
            b"leaf3".to_vec(),
        ];
        let tree = StateMerkleTree::new(leaves);
        tree.generate_proof(0, block_number, chain_id).unwrap()
    }

    // ===== TEST 1: COMPLETE SIDECHAIN LIFECYCLE =====
    // Create → Active → PendingPurge → Frozen → Snapshot → WithdrawalWindow → Purged

    #[test]
    fn test_complete_sidechain_lifecycle() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Step 1: Create sidechain
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("LifecycleTest".to_string()),
                Some("Testing full lifecycle".to_string()),
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::Active);

        // Step 2: Record some activity
        registry.record_activity(chain_id, 1000).unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.last_activity, 1000);

        // Step 3: Simulate 90 days of inactivity - should trigger purge
        let after_90_days = 1000 + INACTIVITY_THRESHOLD_V3_1 + 1;
        let trigger = registry.check_purge_triggers(chain_id, after_90_days);
        assert!(matches!(trigger, Some(PurgeTrigger::Inactivity)));

        // Step 4: Trigger purge
        registry.trigger_purge(chain_id, PurgeTrigger::Inactivity, after_90_days).unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::PendingPurge);

        // Step 5: Wait 30 days warning period → Frozen
        let after_warning = after_90_days + PURGE_WARNING_PERIOD;
        registry.advance_purge_state(chain_id, after_warning).unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::Frozen);

        // Step 6: Frozen → Snapshot (immediate)
        registry.advance_purge_state(chain_id, after_warning + 1).unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::Snapshot);

        // Step 7: Snapshot → WithdrawalWindow (immediate)
        registry.advance_purge_state(chain_id, after_warning + 2).unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::WithdrawalWindow);

        // Step 8: Withdraw during window
        let withdrawal = registry.withdraw_from_purged_chain(chain_id, owner).unwrap();
        assert_eq!(withdrawal.amount, crate::types::SOVEREIGN_DEPOSIT);

        // Step 9: Wait 30 days → Purged
        let window_start = registry.get_sidechain(&chain_id).unwrap().withdrawal_window_start.unwrap();
        let after_withdrawal_window = window_start + WITHDRAWAL_WINDOW_DURATION;
        registry.advance_purge_state(chain_id, after_withdrawal_window).unwrap();
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::Purged);
    }

    // ===== TEST 2: VOLUNTARY EXIT WITH GOVERNANCE VOTE =====

    #[test]
    fn test_voluntary_exit_with_vote() {
        let chain_id = ChainId(1);
        let mut gov = setup_governance_with_voters(chain_id);
        let (alice, bob, charlie, dave) = create_accounts();

        // Step 1: Create exit proposal
        let proposal_id = gov.create_proposal(
            chain_id,
            alice,
            ProposalType::ExitDissolve,
            Some("Community decided to dissolve".to_string()),
            1000,
        ).unwrap();

        // Step 2: Vote - need 66% supermajority
        gov.vote(proposal_id, alice, Vote::Yes, 1001).unwrap();
        gov.vote(proposal_id, bob, Vote::Yes, 1002).unwrap();
        gov.vote(proposal_id, charlie, Vote::Yes, 1003).unwrap();
        gov.vote(proposal_id, dave, Vote::No, 1004).unwrap();

        // 75% voted yes > 66% threshold
        let proposal = gov.get_proposal(proposal_id).unwrap();
        assert_eq!(proposal.approval_percentage(), 75);

        // Step 3: Finalize after voting period
        let after_voting = 1000 + VOTING_PERIOD + 1;
        let status = gov.finalize_voting(proposal_id, after_voting).unwrap();
        assert_eq!(status, ProposalStatus::Passed);

        // Step 4: Wait for timelock (30 days for exit)
        let after_timelock = after_voting + EXIT_TIMELOCK + 1;
        let ready = gov.check_execution_ready(proposal_id, after_timelock).unwrap();
        assert!(ready);

        // Step 5: Mark as executed
        gov.mark_executed(proposal_id, after_timelock).unwrap();
        let proposal = gov.get_proposal(proposal_id).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);
    }

    // ===== TEST 3: EXIT BLOCKED BY DISPUTE =====

    #[test]
    fn test_exit_blocked_by_dispute() {
        let chain_id = ChainId(1);
        let mut gov = setup_governance_with_voters(chain_id);
        let alice = AccountId::from_bytes([1; 32]);

        // Add dispute to chain
        gov.add_dispute(chain_id);
        assert!(gov.has_dispute(chain_id));

        // Try to create exit proposal - should fail
        let result = gov.create_proposal(
            chain_id,
            alice,
            ProposalType::ExitDissolve,
            None,
            1000,
        );

        assert!(result.is_err());

        // Remove dispute
        gov.remove_dispute(chain_id);
        assert!(!gov.has_dispute(chain_id));

        // Now exit proposal should work
        let result = gov.create_proposal(
            chain_id,
            alice,
            ProposalType::ExitDissolve,
            None,
            1000,
        );

        assert!(result.is_ok());
    }

    // ===== TEST 4: CROSS-CHAIN GOVERNANCE NOTIFICATION =====

    #[test]
    fn test_cross_chain_governance_notification() {
        let mut messaging = MessagingContract::new();
        let sender = AccountId::from_bytes([1; 32]);
        let source = ChainId(1);
        let target = ChainId(0); // Notify root chain

        // Send exit notification to root chain
        let message_id = messaging.send_message(
            source,
            target,
            sender,
            None,
            MessageType::GovernanceNotification {
                notification_type: GovernanceNotificationType::ExitInitiated,
            },
            vec![],
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE,
        ).unwrap();

        // Verify message created
        let message = messaging.get_message(message_id).unwrap();
        assert_eq!(message.source_chain, source);
        assert_eq!(message.target_chain, target);
        assert!(matches!(
            message.message_type,
            MessageType::GovernanceNotification { notification_type: GovernanceNotificationType::ExitInitiated }
        ));
    }

    // ===== TEST 5: FRAUD PROOF TRIGGERS PURGE =====

    #[test]
    fn test_fraud_triggers_purge() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);
        let val3 = AccountId::from_bytes([12; 32]);

        // Create chain with validators
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("FraudTest".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        registry.add_validator_to_chain(chain_id, val1).unwrap();
        registry.add_validator_to_chain(chain_id, val2).unwrap();
        registry.add_validator_to_chain(chain_id, val3).unwrap();

        // Slash 1 validator (33% of 3)
        registry.slash_validator(chain_id, &val1).unwrap();

        // Check fraud trigger
        let trigger = registry.check_purge_triggers(chain_id, 1000);
        assert!(matches!(trigger, Some(PurgeTrigger::ValidatorFraud)));

        // Auto-purge should detect and trigger
        registry.trigger_purge(chain_id, PurgeTrigger::ValidatorFraud, 1000).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::PendingPurge);
        assert!(matches!(sidechain.purge_trigger, Some(PurgeTrigger::ValidatorFraud)));
    }

    // ===== TEST 6: STATE DIVERGENCE DETECTION =====

    #[test]
    fn test_state_divergence_purge() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("DivergenceTest".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Report state divergence (owner is authorized to report)
        registry.report_state_divergence(chain_id, 1000, owner).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.state_divergence_detected_at, Some(1000));

        // Check trigger
        let trigger = registry.check_purge_triggers(chain_id, 1001);
        assert!(matches!(trigger, Some(PurgeTrigger::StateDivergence)));
    }

    // ===== TEST 7: CROSS-CHAIN MESSAGE FLOW =====

    #[test]
    fn test_complete_message_flow() {
        let mut messaging = MessagingContract::new();
        let sender = AccountId::from_bytes([1; 32]);
        let recipient = AccountId::from_bytes([2; 32]);
        let source = ChainId(1);
        let target = ChainId(2);

        // Step 1: Send message
        let message_id = messaging.send_message(
            source,
            target,
            sender,
            Some(recipient),
            MessageType::AssetTransfer { amount: 1000, asset_id: None },
            b"transfer data".to_vec(),
            Hash::from_bytes([0; 32]),
            1000,
            BASE_MESSAGE_FEE * 2,
        ).unwrap();

        let message = messaging.get_message(message_id).unwrap();
        assert_eq!(message.status, MessageStatus::Pending);

        // Step 2: Relay message with proof
        let proof = create_valid_merkle_proof(1000, source);
        messaging.relay_message(message_id, proof).unwrap();

        let message = messaging.get_message(message_id).unwrap();
        assert_eq!(message.status, MessageStatus::Relayed);

        // Step 3: Verify and deliver
        let verification = messaging.verify_and_deliver(message_id, 2000).unwrap();
        assert!(verification.is_valid);

        let message = messaging.get_message(message_id).unwrap();
        assert_eq!(message.status, MessageStatus::Delivered);
    }

    // ===== TEST 8: HOSTCHAIN AFFILIATION FLOW =====

    #[test]
    fn test_hostchain_affiliation_with_governance() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);
        let val2 = AccountId::from_bytes([11; 32]);

        // Create hostchain with validators
        let host_id = registry.create_hostchain(owner, 0);
        registry.add_validator_to_hostchain(host_id, val1).unwrap();
        registry.add_validator_to_hostchain(host_id, val2).unwrap();

        // Create sidechain with Shared mode
        let sidechain_id = registry
            .create_sidechain(
                owner,
                Some("SharedChain".to_string()),
                None,
                None,
                SecurityMode::Shared,
                Some(host_id),
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Verify auto-affiliation and validator assignment
        let host = registry.get_hostchain(&host_id).unwrap();
        assert!(host.member_chains.contains(&sidechain_id));

        let sidechain = registry.get_sidechain(&sidechain_id).unwrap();
        assert_eq!(sidechain.validators.len(), 2);
        assert!(sidechain.validators.contains(&val1));
        assert!(sidechain.validators.contains(&val2));

        // Leave host
        registry.leave_host(sidechain_id).unwrap();

        let host = registry.get_hostchain(&host_id).unwrap();
        assert!(!host.member_chains.contains(&sidechain_id));
    }

    // ===== TEST 9: GOVERNANCE FAILURE TRACKING =====

    #[test]
    fn test_governance_failure_purge() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("GovFailTest".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Record 3 consecutive governance failures
        registry.record_governance_failure(chain_id).unwrap();
        registry.record_governance_failure(chain_id).unwrap();
        registry.record_governance_failure(chain_id).unwrap();

        // Check trigger
        let trigger = registry.check_purge_triggers(chain_id, 1000);
        assert!(matches!(trigger, Some(PurgeTrigger::GovernanceFailure)));

        // Reset on success
        registry.reset_governance_failures(chain_id).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.governance_failures, 0);

        // No trigger now
        let trigger = registry.check_purge_triggers(chain_id, 1000);
        assert!(trigger.is_none());
    }

    // ===== TEST 10: AUTO-PURGE SYSTEM =====

    #[test]
    fn test_auto_purge_detection_and_advancement() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("AutoPurgeTest".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Initial state
        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::Active);

        // Run auto-purge after 90 days of inactivity
        let after_90_days = INACTIVITY_THRESHOLD_V3_1 + crate::contracts::sidechains::PURGE_CHECK_INTERVAL + 1;
        let triggered = registry.auto_purge_v3_1(after_90_days);

        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].0, chain_id);
        assert!(matches!(triggered[0].1, PurgeTrigger::Inactivity));

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::PendingPurge);

        // Run auto-purge after warning period - should advance through states
        let after_warning = after_90_days + PURGE_WARNING_PERIOD + crate::contracts::sidechains::PURGE_CHECK_INTERVAL;
        registry.auto_purge_v3_1(after_warning);

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::WithdrawalWindow);
    }

    // ===== TEST 11: MULTI-PROPOSAL VOTING =====

    #[test]
    fn test_multiple_proposals_same_chain() {
        let chain_id = ChainId(1);
        let mut gov = setup_governance_with_voters(chain_id);
        let (alice, bob, charlie, _dave) = create_accounts();

        // Create multiple non-exit proposals
        let proposal1 = gov.create_proposal(
            chain_id,
            alice,
            ProposalType::AddValidator { validator: AccountId::from_bytes([10; 32]) },
            None,
            1000,
        ).unwrap();

        let proposal2 = gov.create_proposal(
            chain_id,
            bob,
            ProposalType::RemoveValidator { validator: AccountId::from_bytes([11; 32]) },
            None,
            1001,
        ).unwrap();

        // Vote on both
        gov.vote(proposal1, alice, Vote::Yes, 1002).unwrap();
        gov.vote(proposal1, bob, Vote::Yes, 1003).unwrap();
        gov.vote(proposal1, charlie, Vote::No, 1004).unwrap();

        gov.vote(proposal2, alice, Vote::No, 1005).unwrap();
        gov.vote(proposal2, bob, Vote::Yes, 1006).unwrap();
        gov.vote(proposal2, charlie, Vote::Yes, 1007).unwrap();

        // Finalize both
        let after_voting = 1000 + VOTING_PERIOD + 1;
        let status1 = gov.finalize_voting(proposal1, after_voting).unwrap();
        let status2 = gov.finalize_voting(proposal2, after_voting).unwrap();

        // Both should pass (66% approval)
        assert_eq!(status1, ProposalStatus::Passed);
        assert_eq!(status2, ProposalStatus::Passed);
    }

    // ===== TEST 12: MESSAGE EXPIRY =====

    #[test]
    fn test_message_expiry_handling() {
        let mut messaging = MessagingContract::new();
        let sender = AccountId::from_bytes([1; 32]);

        // Send messages at different times
        let msg1 = messaging.send_message(
            ChainId(1), ChainId(2), sender, None,
            MessageType::DataTransfer, vec![], Hash::from_bytes([0; 32]),
            1000, BASE_MESSAGE_FEE,
        ).unwrap();

        let msg2 = messaging.send_message(
            ChainId(1), ChainId(2), sender, None,
            MessageType::DataTransfer, vec![], Hash::from_bytes([0; 32]),
            2000, BASE_MESSAGE_FEE,
        ).unwrap();

        // After MESSAGE_EXPIRY from first message
        let expired = messaging.expire_old_messages(1000 + crate::contracts::messaging::MESSAGE_EXPIRY + 1);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], msg1);

        let message1 = messaging.get_message(msg1).unwrap();
        assert_eq!(message1.status, MessageStatus::Expired);

        let message2 = messaging.get_message(msg2).unwrap();
        assert_eq!(message2.status, MessageStatus::Pending); // Still valid
    }

    // ===== TEST 13: INHERITED SECURITY CHAIN DELETION =====

    #[test]
    fn test_inherited_chain_follows_parent() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);
        let val1 = AccountId::from_bytes([10; 32]);

        // Create parent
        let parent_id = registry
            .create_sidechain(
                owner,
                Some("Parent".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        registry.add_validator_to_chain(parent_id, val1).unwrap();

        // Create child with inherited security
        let child_id = registry
            .create_sidechain(
                owner,
                Some("Child".to_string()),
                None,
                Some(parent_id),
                SecurityMode::Inherited,
                None,
                crate::types::BASE_DEPOSIT,
                0,
            )
            .unwrap();

        // Child should have parent's validators
        let child = registry.get_sidechain(&child_id).unwrap();
        assert_eq!(child.validators.len(), 1);
        assert!(child.validators.contains(&val1));

        // Add new validator to parent
        let val2 = AccountId::from_bytes([11; 32]);
        registry.add_validator_to_chain(parent_id, val2).unwrap();

        // Re-assign child validators
        registry.assign_validators(child_id).unwrap();

        let child = registry.get_sidechain(&child_id).unwrap();
        assert_eq!(child.validators.len(), 2);
        assert!(child.validators.contains(&val1));
        assert!(child.validators.contains(&val2));
    }

    // ===== TEST 14: SNAPSHOT STATE ROOT =====

    #[test]
    fn test_snapshot_state_root_commitment() {
        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        let chain_id = registry
            .create_sidechain(
                owner,
                Some("SnapshotTest".to_string()),
                None,
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        // Update verified state root
        let state_root = Hash::hash(b"final_state");
        registry.update_verified_state_root(chain_id, state_root).unwrap();

        // Advance to Snapshot state
        registry.trigger_purge(chain_id, PurgeTrigger::Inactivity, 0).unwrap();
        registry.advance_purge_state(chain_id, PURGE_WARNING_PERIOD).unwrap();
        registry.advance_purge_state(chain_id, PURGE_WARNING_PERIOD + 1).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::Snapshot);

        // Set snapshot state root
        let snapshot_root = Hash::hash(b"snapshot_state");
        registry.set_snapshot_state_root(chain_id, snapshot_root).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.snapshot_state_root, Some(snapshot_root));
    }

    // ===== TEST 15: COMPLETE INTEGRATION SCENARIO =====

    #[test]
    fn test_full_integration_scenario() {
        // This test simulates a complete real-world scenario:
        // 1. Create a sidechain
        // 2. Set up governance
        // 3. Vote to join a host chain
        // 4. Send cross-chain message
        // 5. Eventually decide to exit
        // 6. Complete exit process

        let mut registry = ChainRegistry::new();
        let owner = AccountId::from_bytes([1; 32]);

        // Step 1: Create hostchain
        let host_id = registry.create_hostchain(owner, 0);
        let val1 = AccountId::from_bytes([10; 32]);
        registry.add_validator_to_hostchain(host_id, val1).unwrap();

        // Step 2: Create sidechain as Sovereign first
        let chain_id = registry
            .create_sidechain(
                owner,
                Some("IntegrationChain".to_string()),
                Some("Full integration test".to_string()),
                None,
                SecurityMode::Sovereign,
                None,
                crate::types::SOVEREIGN_DEPOSIT,
                0,
            )
            .unwrap();

        registry.add_validator_to_chain(chain_id, val1).unwrap();

        // Step 3: Set up governance
        let mut gov = setup_governance_with_voters(chain_id);
        let (alice, bob, charlie, dave) = create_accounts();

        // Step 4: Create proposal to join host (via affiliation)
        let affiliation_proposal = gov.create_proposal(
            chain_id,
            alice,
            ProposalType::RequestAffiliation { host_chain: host_id },
            Some("Join the host chain for shared security".to_string()),
            1000,
        ).unwrap();

        // Vote passes
        gov.vote(affiliation_proposal, alice, Vote::Yes, 1001).unwrap();
        gov.vote(affiliation_proposal, bob, Vote::Yes, 1002).unwrap();
        gov.vote(affiliation_proposal, charlie, Vote::Yes, 1003).unwrap();
        gov.vote(affiliation_proposal, dave, Vote::No, 1004).unwrap();

        let after_voting = 1000 + VOTING_PERIOD + 1;
        let status = gov.finalize_voting(affiliation_proposal, after_voting).unwrap();
        assert_eq!(status, ProposalStatus::Passed);

        // Step 5: Set up cross-chain messaging
        let mut messaging = MessagingContract::new();

        // Send state root commitment to root chain
        let message_id = messaging.send_message(
            chain_id,
            ChainId(0),
            val1,
            None,
            MessageType::StateRootCommitment {
                block_number: 5000,
                state_root: Hash::hash(b"chain_state"),
            },
            vec![],
            Hash::from_bytes([0; 32]),
            5000,
            BASE_MESSAGE_FEE,
        ).unwrap();

        assert_eq!(messaging.get_message(message_id).unwrap().status, MessageStatus::Pending);

        // Step 6: Eventually decide to exit
        let exit_proposal = gov.create_proposal(
            chain_id,
            alice,
            ProposalType::ExitDissolve,
            Some("Community decided to wind down".to_string()),
            10000,
        ).unwrap();

        // Supermajority votes yes
        gov.vote(exit_proposal, alice, Vote::Yes, 10001).unwrap();
        gov.vote(exit_proposal, bob, Vote::Yes, 10002).unwrap();
        gov.vote(exit_proposal, charlie, Vote::Yes, 10003).unwrap();
        gov.vote(exit_proposal, dave, Vote::No, 10004).unwrap();

        let after_exit_voting = 10000 + VOTING_PERIOD + 1;
        let status = gov.finalize_voting(exit_proposal, after_exit_voting).unwrap();
        assert_eq!(status, ProposalStatus::Passed);

        // Wait for timelock
        let after_timelock = after_exit_voting + EXIT_TIMELOCK + 1;
        gov.check_execution_ready(exit_proposal, after_timelock).unwrap();
        gov.mark_executed(exit_proposal, after_timelock).unwrap();

        // Step 7: Execute exit in registry
        registry.trigger_purge(chain_id, PurgeTrigger::Inactivity, after_timelock).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::PendingPurge);

        // Send exit notification
        let _exit_msg = messaging.send_message(
            chain_id,
            ChainId(0),
            val1,
            None,
            MessageType::GovernanceNotification {
                notification_type: GovernanceNotificationType::ExitInitiated,
            },
            vec![],
            Hash::from_bytes([0; 32]),
            after_timelock,
            BASE_MESSAGE_FEE,
        ).unwrap();

        // Complete purge cycle
        let after_warning = after_timelock + PURGE_WARNING_PERIOD;
        registry.advance_purge_state(chain_id, after_warning).unwrap();
        registry.advance_purge_state(chain_id, after_warning + 1).unwrap();
        registry.advance_purge_state(chain_id, after_warning + 2).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::WithdrawalWindow);

        // Withdraw
        let withdrawal = registry.withdraw_from_purged_chain(chain_id, owner).unwrap();
        assert_eq!(withdrawal.amount, crate::types::SOVEREIGN_DEPOSIT);

        // Final purge
        let window_start = registry.get_sidechain(&chain_id).unwrap().withdrawal_window_start.unwrap();
        registry.advance_purge_state(chain_id, window_start + WITHDRAWAL_WINDOW_DURATION).unwrap();

        let sidechain = registry.get_sidechain(&chain_id).unwrap();
        assert_eq!(sidechain.status, ChainStatus::Purged);

        // Final message
        let _purge_msg = messaging.send_message(
            chain_id,
            ChainId(0),
            val1,
            None,
            MessageType::GovernanceNotification {
                notification_type: GovernanceNotificationType::ChainPurged,
            },
            vec![],
            Hash::from_bytes([0; 32]),
            window_start + WITHDRAWAL_WINDOW_DURATION + 1,
            BASE_MESSAGE_FEE,
        ).unwrap();

        // Verify final state
        let stats = messaging.get_stats(chain_id);
        assert!(stats.sent >= 3); // At least 3 messages sent during lifecycle
    }
}
