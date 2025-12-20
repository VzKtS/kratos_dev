// Networking Integration Tests
// Tests for P2P networking, peer management, sync, and request-response protocols

#[cfg(test)]
mod tests {
    use crate::network::peer::{PeerInfo, PeerManager, PeerState, PeerStats, INITIAL_SCORE, MIN_SCORE};
    use crate::network::request::{
        BlockRequest, BlockResponse, KratosCodec, KratosRequest, KratosResponse,
        StatusRequest, StatusResponse, SyncRequest, SyncResponse,
    };
    use crate::network::sync::{SyncManager, SyncState};
    use crate::network::rate_limit::{NetworkRateLimiter, RateLimitConfig};
    use crate::types::*;
    use libp2p::PeerId;
    use std::time::Duration;

    // =========================================================================
    // HELPER FUNCTIONS
    // =========================================================================

    fn create_peer_id() -> PeerId {
        let keypair = libp2p::identity::Keypair::generate_ed25519();
        PeerId::from(keypair.public())
    }

    fn create_test_block(number: BlockNumber) -> Block {
        use ed25519_dalek::{SigningKey, Signer};
        use crate::types::signature::{domain_separate, DOMAIN_BLOCK_HEADER};

        // Create a deterministic signing key for testing
        let signing_key = SigningKey::from_bytes(&[1u8; 32]);
        let author = AccountId::from_bytes(signing_key.verifying_key().to_bytes());

        let mut header = BlockHeader {
            number,
            parent_hash: Hash::ZERO,
            transactions_root: Hash::ZERO, // Empty transactions -> ZERO root is valid
            state_root: Hash::ZERO,
            timestamp: number * 6,
            epoch: number / 100,
            slot: number,
            author,
            signature: Signature64([0; 64]),
        };

        // Sign the header properly with domain separation (SECURITY FIX #24)
        let header_hash = header.hash();
        let message = domain_separate(DOMAIN_BLOCK_HEADER, header_hash.as_bytes());
        let signature = signing_key.sign(&message);
        header.signature = Signature64(signature.to_bytes());

        Block {
            header,
            body: BlockBody {
                transactions: vec![],
            },
        }
    }

    // =========================================================================
    // PEER MANAGER TESTS
    // =========================================================================

    mod peer_management {
        use super::*;

        #[test]
        fn test_peer_lifecycle() {
            let mut manager = PeerManager::new();
            let peer_id = create_peer_id();

            // Initially no peers
            assert_eq!(manager.connected_count(), 0);
            assert!(manager.needs_more_peers());

            // Connect peer
            manager.peer_connected(peer_id);
            assert_eq!(manager.connected_count(), 1);

            // Peer should be active
            let peer = manager.get_peer(&peer_id).unwrap();
            assert!(peer.is_active());
            assert_eq!(peer.score, INITIAL_SCORE);

            // Disconnect peer
            manager.peer_disconnected(&peer_id);
            assert_eq!(manager.connected_count(), 0);
        }

        #[test]
        fn test_peer_scoring() {
            let mut manager = PeerManager::new();
            let peer_id = create_peer_id();

            manager.peer_connected(peer_id);

            // Good block increases score
            let initial_score = manager.get_peer(&peer_id).unwrap().score;
            manager.record_good_block(&peer_id);
            assert!(manager.get_peer(&peer_id).unwrap().score > initial_score);

            // Bad block decreases score significantly
            manager.record_bad_block(&peer_id);
            assert!(manager.get_peer(&peer_id).unwrap().score < initial_score);
        }

        #[test]
        fn test_peer_ban() {
            let mut manager = PeerManager::new();
            let peer_id = create_peer_id();

            manager.peer_connected(peer_id);
            manager.ban_peer(&peer_id, "test ban");

            let peer = manager.get_peer(&peer_id).unwrap();
            assert_eq!(peer.state, PeerState::Banned);
            assert!(peer.should_disconnect());
        }

        #[test]
        fn test_best_sync_peer() {
            let mut manager = PeerManager::new();

            let peer1 = create_peer_id();
            let peer2 = create_peer_id();
            let peer3 = create_peer_id();

            manager.peer_connected(peer1);
            manager.peer_connected(peer2);
            manager.peer_connected(peer3);

            manager.update_peer_height(&peer1, 100);
            manager.update_peer_height(&peer2, 200);
            manager.update_peer_height(&peer3, 150);

            // Best sync peer should have highest height
            let best = manager.best_sync_peer().unwrap();
            assert_eq!(best.best_height, 200);
        }

        #[test]
        fn test_bootstrap_nodes() {
            let mut manager = PeerManager::new();

            let peer1 = create_peer_id();
            let addr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();

            manager.add_bootstrap_nodes(vec![(peer1, addr)]);

            // Bootstrap node should be tracked
            assert_eq!(manager.get_bootstrap_nodes().len(), 1);

            // Bootstrap peer should be marked as such
            let peer = manager.get_peer(&peer1).unwrap();
            assert!(peer.is_bootstrap);
        }

        #[test]
        fn test_peer_to_disconnect() {
            let mut manager = PeerManager::new();
            let peer_id = create_peer_id();

            manager.peer_connected(peer_id);

            // Record many bad blocks to lower score
            for _ in 0..10 {
                manager.record_bad_block(&peer_id);
            }

            // Peer should be in disconnect list
            let to_disconnect = manager.peers_to_disconnect();
            assert!(to_disconnect.contains(&peer_id));
        }

        #[test]
        fn test_peer_stats() {
            let mut manager = PeerManager::new();

            let peer1 = create_peer_id();
            let peer2 = create_peer_id();

            manager.peer_connected(peer1);
            manager.peer_connected(peer2);

            manager.update_peer_height(&peer1, 100);
            manager.update_peer_height(&peer2, 200);

            let stats = manager.stats();
            assert_eq!(stats.connected, 2);
            assert_eq!(stats.best_height, 200);
            assert_eq!(stats.average_score, INITIAL_SCORE);
        }
    }

    // =========================================================================
    // SYNC MANAGER TESTS
    // =========================================================================

    mod sync_management {
        use super::*;

        #[test]
        fn test_sync_state_transitions() {
            let mut sync = SyncManager::new(100);

            // Initially idle
            assert_eq!(sync.state(), SyncState::Idle);

            // Small gap - stay synced
            sync.peer_height_update(105);
            assert_eq!(sync.state(), SyncState::Synced);
            assert!(!sync.should_sync());

            // Larger gap - start downloading
            sync.peer_height_update(120);
            assert_eq!(sync.state(), SyncState::Downloading);
            assert!(sync.should_sync());

            // Very large gap - far behind
            sync.peer_height_update(2000);
            assert_eq!(sync.state(), SyncState::FarBehind);
            assert!(sync.should_sync());
        }

        #[test]
        fn test_block_download_queue() {
            let mut sync = SyncManager::new(100);
            sync.peer_height_update(200);

            // Prepare download batch
            let batch = sync.prepare_download();
            assert!(batch.is_some());

            let blocks = batch.unwrap();
            assert!(!blocks.is_empty());
            assert_eq!(blocks[0], 101);
        }

        #[test]
        fn test_block_import_order() {
            let mut sync = SyncManager::new(100);
            sync.peer_height_update(200); // Must set best_known_height for validation

            // Add blocks out of order - with validation they should all be accepted
            assert!(sync.add_downloaded_block(create_test_block(103)));
            assert!(sync.add_downloaded_block(create_test_block(101)));
            assert!(sync.add_downloaded_block(create_test_block(102)));

            // Should import in order
            let block = sync.next_block_to_import().unwrap();
            assert_eq!(block.header.number, 101);

            sync.update_local_height(101);
            let block = sync.next_block_to_import().unwrap();
            assert_eq!(block.header.number, 102);
        }

        #[test]
        fn test_sync_gap_calculation() {
            let mut sync = SyncManager::new(100);
            sync.peer_height_update(250);

            assert_eq!(sync.sync_gap(), 150);

            sync.update_local_height(200);
            assert_eq!(sync.sync_gap(), 50);
        }

        #[test]
        fn test_sync_response_handling() {
            let mut sync = SyncManager::new(100);
            sync.peer_height_update(200);

            let blocks = vec![
                create_test_block(101),
                create_test_block(102),
                create_test_block(103),
            ];

            let accepted = sync.handle_sync_response(blocks, true);

            assert_eq!(accepted, 3);
            assert_eq!(sync.pending_count(), 3);
        }
    }

    // =========================================================================
    // REQUEST-RESPONSE TESTS
    // =========================================================================

    mod request_response {
        use super::*;

        #[test]
        fn test_block_request_by_hash() {
            let hash = Hash::hash(b"test block");
            let request = BlockRequest::by_hash(hash);

            match request {
                KratosRequest::Block(BlockRequest::ByHash(h)) => {
                    assert_eq!(h, hash);
                }
                _ => panic!("Wrong request type"),
            }
        }

        #[test]
        fn test_block_request_by_number() {
            let request = BlockRequest::by_number(100);

            match request {
                KratosRequest::Block(BlockRequest::ByNumber(n)) => {
                    assert_eq!(n, 100);
                }
                _ => panic!("Wrong request type"),
            }
        }

        #[test]
        fn test_sync_request() {
            let request = SyncRequest::new(100, 50);

            match request {
                KratosRequest::Sync(req) => {
                    assert_eq!(req.from_block, 100);
                    assert_eq!(req.max_blocks, 50);
                    assert!(req.include_bodies);
                }
                _ => panic!("Wrong request type"),
            }
        }

        #[test]
        fn test_status_request() {
            let request = StatusRequest::new(100, Hash::ZERO, Hash::hash(b"genesis"));

            match request {
                KratosRequest::Status(req) => {
                    assert_eq!(req.best_block, 100);
                    assert_eq!(req.genesis_hash, Hash::hash(b"genesis"));
                    assert_eq!(req.protocol_version, 1);
                }
                _ => panic!("Wrong request type"),
            }
        }

        #[test]
        fn test_block_response_found() {
            let block = create_test_block(100);
            let response = BlockResponse::found(block.clone());

            match response {
                KratosResponse::Block(BlockResponse::Block(b)) => {
                    assert_eq!(b.header.number, 100);
                }
                _ => panic!("Wrong response type"),
            }
        }

        #[test]
        fn test_block_response_not_found() {
            let response = BlockResponse::not_found();

            match response {
                KratosResponse::Block(BlockResponse::NotFound) => {}
                _ => panic!("Wrong response type"),
            }
        }

        #[test]
        fn test_sync_response() {
            let blocks = vec![create_test_block(100), create_test_block(101)];
            let response = SyncResponse::new(blocks.clone(), true, 200);

            match response {
                KratosResponse::Sync(res) => {
                    assert_eq!(res.blocks.len(), 2);
                    assert!(res.has_more);
                    assert_eq!(res.best_height, 200);
                }
                _ => panic!("Wrong response type"),
            }
        }

        #[test]
        fn test_status_response() {
            let response = StatusResponse::new(100, Hash::ZERO, Hash::hash(b"genesis"), 5);

            match response {
                KratosResponse::Status(res) => {
                    assert_eq!(res.best_block, 100);
                    assert_eq!(res.peer_count, 5);
                }
                _ => panic!("Wrong response type"),
            }
        }

        #[test]
        fn test_request_serialization_roundtrip() {
            let request = SyncRequest::new(100, 50);

            let serialized = bincode::serialize(&request).unwrap();
            let deserialized: KratosRequest = bincode::deserialize(&serialized).unwrap();

            match deserialized {
                KratosRequest::Sync(req) => {
                    assert_eq!(req.from_block, 100);
                    assert_eq!(req.max_blocks, 50);
                }
                _ => panic!("Wrong type after deserialization"),
            }
        }

        #[test]
        fn test_response_serialization_roundtrip() {
            let block = create_test_block(100);
            let response = BlockResponse::found(block);

            let serialized = bincode::serialize(&response).unwrap();
            let deserialized: KratosResponse = bincode::deserialize(&serialized).unwrap();

            match deserialized {
                KratosResponse::Block(BlockResponse::Block(b)) => {
                    assert_eq!(b.header.number, 100);
                }
                _ => panic!("Wrong type after deserialization"),
            }
        }
    }

    // =========================================================================
    // RATE LIMITING TESTS
    // =========================================================================

    mod rate_limiting {
        use super::*;

        #[test]
        fn test_rate_limit_allows_normal_traffic() {
            let mut limiter = NetworkRateLimiter::new(RateLimitConfig::default());
            let peer_id = create_peer_id();

            // Should allow reasonable traffic
            for _ in 0..10 {
                assert!(limiter.check_message(&peer_id, 1000).is_ok());
            }
        }

        #[test]
        fn test_rate_limit_blocks_excessive_traffic() {
            let config = RateLimitConfig {
                max_messages_per_window: 5,
                window_duration: Duration::from_secs(10),
                max_message_size: 10_000,
                max_connections_per_peer: 3,
                max_bandwidth_per_peer: 10_000,
            };
            let mut limiter = NetworkRateLimiter::new(config);
            let peer_id = create_peer_id();

            // Rapidly send many messages
            let mut blocked = false;
            for _ in 0..20 {
                if limiter.check_message(&peer_id, 100).is_err() {
                    blocked = true;
                    break;
                }
            }

            assert!(blocked, "Rate limiter should block excessive traffic");
        }

        #[test]
        fn test_rate_limit_blocks_large_messages() {
            let config = RateLimitConfig {
                max_messages_per_window: 100,
                window_duration: Duration::from_secs(10),
                max_message_size: 1000,
                max_connections_per_peer: 3,
                max_bandwidth_per_peer: 1_000_000,
            };
            let mut limiter = NetworkRateLimiter::new(config);
            let peer_id = create_peer_id();

            // Try to send oversized message
            let result = limiter.check_message(&peer_id, 2000);
            assert!(result.is_err());
        }

        #[test]
        fn test_manual_ban() {
            let mut limiter = NetworkRateLimiter::new(RateLimitConfig::default());
            let peer_id = create_peer_id();

            limiter.ban_peer(peer_id);
            assert!(limiter.banned_peers().contains(&peer_id));

            // Banned peer should fail rate check
            let result = limiter.check_message(&peer_id, 100);
            assert!(result.is_err());
        }
    }

    // =========================================================================
    // INTEGRATION SCENARIOS
    // =========================================================================

    mod integration {
        use super::*;

        #[test]
        fn test_full_sync_scenario() {
            let mut peer_manager = PeerManager::new();
            let mut sync_manager = SyncManager::new(0);

            // Simulate connecting to peers
            let peer1 = create_peer_id();
            let peer2 = create_peer_id();

            peer_manager.peer_connected(peer1);
            peer_manager.peer_connected(peer2);

            // Peers report their heights
            peer_manager.update_peer_height(&peer1, 100);
            peer_manager.update_peer_height(&peer2, 150);

            sync_manager.peer_height_update(100);
            sync_manager.peer_height_update(150);

            // Should need sync
            assert!(sync_manager.should_sync());
            assert_eq!(sync_manager.state(), SyncState::Downloading);

            // Get best peer for sync
            let best = peer_manager.best_sync_peer().unwrap();
            assert_eq!(best.best_height, 150);

            // Simulate receiving blocks (with validation enabled)
            for i in 1..=50 {
                assert!(sync_manager.add_downloaded_block(create_test_block(i)),
                    "Block {} should be accepted", i);
            }

            // Import blocks in order
            for i in 1..=50 {
                let block = sync_manager.next_block_to_import().unwrap();
                assert_eq!(block.header.number, i);
                sync_manager.update_local_height(i);
            }

            assert_eq!(sync_manager.sync_gap(), 100);
        }

        #[test]
        fn test_peer_reputation_tracking() {
            let mut manager = PeerManager::new();
            let good_peer = create_peer_id();
            let bad_peer = create_peer_id();

            manager.peer_connected(good_peer);
            manager.peer_connected(bad_peer);

            // Good peer sends valid blocks
            for _ in 0..10 {
                manager.record_good_block(&good_peer);
            }

            // Bad peer sends invalid data
            for _ in 0..3 {
                manager.record_bad_block(&bad_peer);
            }

            // Good peer should have higher score
            let good_score = manager.get_peer(&good_peer).unwrap().score;
            let bad_score = manager.get_peer(&bad_peer).unwrap().score;

            assert!(good_score > bad_score);
            assert!(good_score > INITIAL_SCORE);
            assert!(bad_score < INITIAL_SCORE);
        }

        #[test]
        fn test_genesis_mismatch_handling() {
            let our_genesis = Hash::hash(b"genesis_chain_a");
            let their_genesis = Hash::hash(b"genesis_chain_b");

            // Peers with different genesis should be rejected
            assert_ne!(our_genesis, their_genesis);

            // This would trigger a ban in the actual network service
            // Here we just verify the hashes are different
        }

        #[test]
        fn test_concurrent_downloads() {
            let mut sync = SyncManager::new(0);
            sync.peer_height_update(200);

            // Get first batch
            let batch1 = sync.prepare_download().unwrap();
            assert!(!batch1.is_empty());

            // Simulate receiving blocks from multiple peers
            // (In reality these would come from different peers concurrently)
            for i in 1..=50 {
                assert!(sync.add_downloaded_block(create_test_block(i)),
                    "Block {} should be accepted", i);
            }

            assert_eq!(sync.pending_count(), 50);

            // Import sequentially
            for i in 1..=50 {
                let block = sync.next_block_to_import().unwrap();
                assert_eq!(block.header.number, i);
                sync.update_local_height(i);
            }

            assert_eq!(sync.pending_count(), 0);
        }

        #[test]
        fn test_network_recovery() {
            let mut manager = PeerManager::new();

            // Add several peers
            let peers: Vec<_> = (0..5).map(|_| create_peer_id()).collect();

            for peer in &peers {
                manager.peer_connected(*peer);
                manager.update_peer_height(peer, 100);
            }

            assert_eq!(manager.connected_count(), 5);

            // Simulate network issues - disconnect all
            for peer in &peers {
                manager.peer_disconnected(peer);
            }

            assert_eq!(manager.connected_count(), 0);
            assert!(manager.needs_more_peers());

            // Reconnect some peers
            for peer in peers.iter().take(3) {
                manager.peer_connected(*peer);
            }

            assert_eq!(manager.connected_count(), 3);
            assert!(!manager.needs_more_peers()); // MIN_PEERS is 3
        }
    }

    // =========================================================================
    // PROTOCOL INVARIANTS
    // =========================================================================

    mod invariants {
        use super::*;

        #[test]
        fn invariant_blocks_imported_sequentially() {
            let mut sync = SyncManager::new(100);
            sync.peer_height_update(200); // Must set best_known_height for validation

            // Add blocks out of order
            assert!(sync.add_downloaded_block(create_test_block(105)));
            assert!(sync.add_downloaded_block(create_test_block(103)));
            assert!(sync.add_downloaded_block(create_test_block(101)));
            assert!(sync.add_downloaded_block(create_test_block(104)));
            assert!(sync.add_downloaded_block(create_test_block(102)));

            // Must import in strict order
            for expected in 101..=105 {
                let block = sync.next_block_to_import().unwrap();
                assert_eq!(block.header.number, expected);
                sync.update_local_height(expected);
            }
        }

        #[test]
        fn invariant_banned_peers_stay_banned() {
            let mut manager = PeerManager::new();
            let peer_id = create_peer_id();

            manager.peer_connected(peer_id);
            manager.ban_peer(&peer_id, "malicious");

            // Even after tick/maintenance, peer stays banned
            manager.tick();

            let peer = manager.get_peer(&peer_id).unwrap();
            assert_eq!(peer.state, PeerState::Banned);
        }

        #[test]
        fn invariant_peer_scores_bounded() {
            let mut manager = PeerManager::new();
            let peer_id = create_peer_id();

            manager.peer_connected(peer_id);

            // Many good blocks shouldn't overflow
            for _ in 0..10000 {
                manager.record_good_block(&peer_id);
            }

            let score = manager.get_peer(&peer_id).unwrap().score;
            assert!(score > 0);
            assert!(score < i32::MAX);

            // Many bad blocks - score should eventually trigger disconnect
            // but shouldn't underflow i32
            for _ in 0..10000 {
                manager.record_bad_block(&peer_id);
            }

            let score = manager.get_peer(&peer_id).unwrap().score;
            // Score can go below MIN_SCORE (which triggers disconnect)
            // but should never underflow to positive due to wrapping
            assert!(score < i32::MAX / 2); // Not wrapped around
            assert!(score <= MIN_SCORE); // Should be at or below disconnect threshold
        }

        #[test]
        fn invariant_sync_gap_non_negative() {
            let mut sync = SyncManager::new(100);

            // Even if local is ahead, gap should be 0
            sync.peer_height_update(50);
            assert_eq!(sync.sync_gap(), 0);

            sync.update_local_height(200);
            assert_eq!(sync.sync_gap(), 0);
        }
    }

    // =========================================================================
    // BLOCK STORAGE SYNC TESTS
    // =========================================================================

    mod storage_sync {
        use super::*;
        use crate::storage::{Database, StateBackend};
        use tempfile::TempDir;

        fn create_test_storage() -> (TempDir, StateBackend) {
            let dir = TempDir::new().unwrap();
            let db = Database::open(dir.path().to_str().unwrap()).unwrap();
            let backend = StateBackend::new(db);
            (dir, backend)
        }

        #[test]
        fn test_store_and_retrieve_block() {
            let (_dir, backend) = create_test_storage();

            let block = create_test_block(100);
            let block_hash = block.hash();

            // Store block
            backend.store_block(&block).unwrap();

            // Retrieve by hash
            let retrieved = backend.get_block_by_hash(&block_hash).unwrap();
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().header.number, 100);

            // Retrieve by number
            let retrieved = backend.get_block_by_number(100).unwrap();
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().header.number, 100);
        }

        #[test]
        fn test_block_range_retrieval() {
            let (_dir, backend) = create_test_storage();

            // Store multiple blocks
            for i in 1..=10 {
                let block = create_test_block(i);
                backend.store_block(&block).unwrap();
            }

            // Set best block
            backend.set_best_block(10).unwrap();

            // Get range
            let blocks = backend.get_blocks_range(1, 5).unwrap();
            assert_eq!(blocks.len(), 5);
            assert_eq!(blocks[0].header.number, 1);
            assert_eq!(blocks[4].header.number, 5);
        }

        #[test]
        fn test_block_range_partial() {
            let (_dir, backend) = create_test_storage();

            // Store blocks 1-5
            for i in 1..=5 {
                let block = create_test_block(i);
                backend.store_block(&block).unwrap();
            }
            backend.set_best_block(5).unwrap();

            // Request more than available
            let blocks = backend.get_blocks_range(1, 10).unwrap();
            assert_eq!(blocks.len(), 5);
        }

        #[test]
        fn test_genesis_hash_storage() {
            let (_dir, backend) = create_test_storage();

            let genesis_hash = Hash::hash(b"genesis");
            backend.set_genesis_hash(genesis_hash).unwrap();

            let retrieved = backend.get_genesis_hash().unwrap();
            assert_eq!(retrieved, Some(genesis_hash));
        }

        #[test]
        fn test_sync_with_storage() {
            let (_dir, backend) = create_test_storage();
            let mut sync = SyncManager::new(0);

            // Simulate a full node with blocks
            for i in 1..=100 {
                let block = create_test_block(i);
                backend.store_block(&block).unwrap();
            }
            backend.set_best_block(100).unwrap();

            // New node wants to sync
            sync.peer_height_update(100);
            assert!(sync.should_sync());

            // Get blocks from storage (simulating peer response)
            let batch = sync.prepare_download().unwrap();
            let start = batch[0];
            let count = batch.len() as u32;

            let blocks = backend.get_blocks_range(start, count).unwrap();
            assert!(!blocks.is_empty());

            // Process sync response - now returns count of accepted blocks
            let accepted = sync.handle_sync_response(blocks.clone(), false);
            assert_eq!(accepted, blocks.len());
            assert_eq!(sync.pending_count(), blocks.len());

            // Import blocks
            for expected in batch.iter() {
                let block = sync.next_block_to_import().unwrap();
                assert_eq!(block.header.number, *expected);
                sync.update_local_height(*expected);
            }
        }

        #[test]
        fn test_block_persistence_after_import() {
            let (_dir, backend) = create_test_storage();
            let mut sync = SyncManager::new(0);
            sync.peer_height_update(100); // Must set best_known_height for validation

            // Receive blocks via sync
            let blocks: Vec<_> = (1..=5).map(|i| create_test_block(i)).collect();
            let accepted = sync.handle_sync_response(blocks.clone(), false);
            assert_eq!(accepted, 5);

            // Import and persist each block
            for i in 1..=5 {
                let block = sync.next_block_to_import().unwrap();

                // Persist to storage
                backend.store_block(&block).unwrap();
                backend.set_best_block(i).unwrap();

                sync.update_local_height(i);
            }

            // Verify all blocks are persisted
            for i in 1..=5 {
                let block = backend.get_block_by_number(i).unwrap();
                assert!(block.is_some(), "Block {} should be persisted", i);
            }

            // Best block should be updated
            let best = backend.get_best_block().unwrap();
            assert_eq!(best, Some(5));
        }
    }

    // =========================================================================
    // WARP SYNC INTEGRATION TESTS
    // =========================================================================

    mod warp_sync {
        use super::*;
        use crate::network::warp_sync::{WarpSyncManager, WarpSyncState, StateSnapshot};
        use crate::types::{ChainId, StateRoot};

        fn create_test_snapshot(block_number: BlockNumber) -> StateSnapshot {
            let accounts: Vec<_> = (0..100)
                .map(|i| {
                    let mut bytes = [0u8; 32];
                    bytes[0..8].copy_from_slice(&(i as u64).to_le_bytes());
                    (
                        AccountId::from_bytes(bytes),
                        AccountInfo {
                            nonce: i as u64,
                            free: i as u128 * 1000,
                            reserved: 0,
                            last_modified: Hash::ZERO,
                        },
                    )
                })
                .collect();

            StateSnapshot::new(
                block_number,
                Hash::hash(&block_number.to_le_bytes()),
                StateRoot::zero(ChainId::ROOT),
                accounts,
                50,
            )
        }

        #[test]
        fn test_warp_sync_full_flow() {
            let mut warp = WarpSyncManager::new(0);

            // Far behind - triggers warp sync
            warp.peer_height_update(5000);
            assert!(warp.is_active());
            assert_eq!(warp.state(), WarpSyncState::RequestingSnapshot);

            // Receive snapshot header
            let snapshot = create_test_snapshot(4000);
            warp.handle_snapshot_header(snapshot.header.clone()).unwrap();
            assert!(matches!(warp.state(), WarpSyncState::DownloadingState { .. }));

            // Receive all chunks
            for chunk in &snapshot.chunks {
                warp.handle_state_chunk(chunk.clone()).unwrap();
            }
            assert_eq!(warp.state(), WarpSyncState::VerifyingState);

            // Verify state
            let verified = warp.verify_state().unwrap();
            assert_eq!(verified.account_count(), 100);
            assert!(matches!(warp.state(), WarpSyncState::DownloadingHeaders { .. }));

            // Complete warp sync
            warp.complete();
            assert_eq!(warp.state(), WarpSyncState::Complete);
        }

        #[test]
        fn test_warp_sync_combined_with_regular_sync() {
            let mut warp = WarpSyncManager::new(0);
            let mut sync = SyncManager::new(0);

            // Node is far behind
            warp.peer_height_update(5000);
            sync.peer_height_update(5000);

            // Warp sync should be triggered, regular sync sees FarBehind
            assert!(warp.is_active());
            assert_eq!(sync.state(), SyncState::FarBehind);

            // After warp sync completes at block 4000
            let snapshot = create_test_snapshot(4000);
            warp.handle_snapshot_header(snapshot.header.clone()).unwrap();
            for chunk in &snapshot.chunks {
                warp.handle_state_chunk(chunk.clone()).unwrap();
            }
            warp.verify_state().unwrap();

            // Update local height to snapshot block
            sync.update_local_height(4000);

            // Now regular sync should continue from 4000
            // Gap of 1000 is Downloading (>1000 is FarBehind)
            assert_eq!(sync.sync_gap(), 1000);
            assert_eq!(sync.state(), SyncState::Downloading);

            // Prepare download should start from 4001
            let batch = sync.prepare_download();
            assert!(batch.is_some());
            let blocks = batch.unwrap();
            assert_eq!(blocks[0], 4001);
        }

        #[test]
        fn test_warp_sync_progress_tracking() {
            let mut warp = WarpSyncManager::new(0);
            warp.peer_height_update(5000);

            let snapshot = create_test_snapshot(4000);
            warp.handle_snapshot_header(snapshot.header.clone()).unwrap();

            // Progress should increase with each chunk
            let total_chunks = snapshot.chunks.len();
            for (i, chunk) in snapshot.chunks.iter().enumerate() {
                warp.handle_state_chunk(chunk.clone()).unwrap();

                let expected_progress = (i + 1) as f64 / total_chunks as f64;
                let actual_progress = warp.download_progress();
                assert!((actual_progress - expected_progress).abs() < 0.01);
            }

            assert_eq!(warp.download_progress(), 1.0);
        }
    }
}
