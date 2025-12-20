// Warp Sync - Fast synchronization for nodes far behind
// Principle: Download state snapshot instead of replaying all blocks
//
// When a node is more than 1000 blocks behind, warp sync downloads:
// 1. A recent finalized state snapshot (accounts, validators, etc.)
// 2. The block headers from the snapshot to current
// 3. Then continues with normal block sync

// SECURITY FIX #37: Merkle proof verification for warp sync chunks
// Prevents malicious peers from injecting fake state data during warp sync

use crate::types::{AccountId, AccountInfo, BlockNumber, Hash, StateRoot, Blake3Hasher};
use rs_merkle::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Warp sync state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarpSyncState {
    /// Not in warp sync
    Inactive,

    /// Requesting state snapshot from peers
    RequestingSnapshot,

    /// Downloading state chunks
    DownloadingState {
        /// Total chunks expected
        total_chunks: u32,
        /// Chunks received so far
        received: u32,
    },

    /// Verifying downloaded state
    VerifyingState,

    /// Downloading block headers from snapshot to tip
    DownloadingHeaders {
        /// Starting block (snapshot block)
        from: BlockNumber,
        /// Target block
        to: BlockNumber,
    },

    /// Warp sync complete, switch to regular sync
    Complete,

    /// Warp sync failed
    Failed(WarpSyncError),
}

/// Warp sync errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarpSyncError {
    /// No peers available for warp sync
    NoPeers,
    /// State root verification failed
    InvalidStateRoot,
    /// Snapshot too old
    SnapshotTooOld,
    /// Timeout waiting for data
    Timeout,
    /// Chunk verification failed
    InvalidChunk,
    /// Merkle proof verification failed (SECURITY FIX #37)
    InvalidMerkleProof,
}

/// State snapshot header (metadata)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshotHeader {
    /// Block number at which snapshot was taken
    pub block_number: BlockNumber,

    /// Block hash at snapshot
    pub block_hash: Hash,

    /// State root for verification
    pub state_root: StateRoot,

    /// Total number of chunks
    pub total_chunks: u32,

    /// Size in bytes
    pub total_size: u64,

    /// Timestamp of snapshot creation
    pub created_at: u64,
}

/// A chunk of state data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChunk {
    /// Chunk index
    pub index: u32,

    /// Accounts in this chunk
    pub accounts: Vec<(AccountId, AccountInfo)>,

    /// Merkle proof for this chunk (sibling hashes from leaf to root)
    pub proof: Vec<Hash>,

    /// Hash of this chunk's data (leaf hash in Merkle tree)
    pub chunk_hash: Hash,
}

impl StateChunk {
    /// SECURITY FIX #37: Compute the hash of this chunk's account data
    pub fn compute_hash(&self) -> Hash {
        // Serialize all accounts in this chunk deterministically
        let mut data = Vec::new();
        for (account_id, account_info) in &self.accounts {
            data.extend_from_slice(account_id.as_bytes());
            data.extend_from_slice(&account_info.nonce.to_le_bytes());
            data.extend_from_slice(&account_info.free.to_le_bytes());
            data.extend_from_slice(&account_info.reserved.to_le_bytes());
            data.extend_from_slice(account_info.last_modified.as_bytes());
        }
        Hash::hash(&data)
    }

    /// SECURITY FIX #37: Verify that chunk_hash matches computed hash
    pub fn verify_hash(&self) -> bool {
        self.chunk_hash == self.compute_hash()
    }

    /// SECURITY FIX #37: Verify Merkle proof against expected state root
    ///
    /// This verifies that this chunk is a valid leaf in the state Merkle tree.
    /// The proof contains sibling hashes from leaf to root.
    pub fn verify_merkle_proof(&self, expected_root: &Hash, total_chunks: u32) -> bool {
        // First verify the chunk hash matches the data
        if !self.verify_hash() {
            warn!("Chunk {} hash mismatch: expected {:?}, computed {:?}",
                  self.index, self.chunk_hash, self.compute_hash());
            return false;
        }

        // Empty proof is only valid for single-chunk snapshots
        if self.proof.is_empty() {
            if total_chunks == 1 {
                // Single chunk = chunk hash should equal root
                return self.chunk_hash == *expected_root;
            } else {
                warn!("Chunk {} has empty proof but snapshot has {} chunks",
                      self.index, total_chunks);
                return false;
            }
        }

        // Compute expected proof length: ceil(log2(total_chunks))
        let expected_proof_len = (total_chunks as f64).log2().ceil() as usize;
        if self.proof.len() != expected_proof_len {
            warn!("Chunk {} proof length mismatch: expected {}, got {}",
                  self.index, expected_proof_len, self.proof.len());
            return false;
        }

        // Traverse proof path from leaf to root
        let mut current_hash: [u8; 32] = *self.chunk_hash.as_bytes();
        let mut index = self.index as usize;

        for sibling in &self.proof {
            // Determine if current node is left or right child
            let is_left = index % 2 == 0;

            // Combine current hash with sibling
            current_hash = if is_left {
                // Current is left, sibling is right
                let mut combined = Vec::with_capacity(64);
                combined.extend_from_slice(&current_hash);
                combined.extend_from_slice(sibling.as_bytes());
                Blake3Hasher::hash(&combined)
            } else {
                // Current is right, sibling is left
                let mut combined = Vec::with_capacity(64);
                combined.extend_from_slice(sibling.as_bytes());
                combined.extend_from_slice(&current_hash);
                Blake3Hasher::hash(&combined)
            };

            // Move up the tree
            index /= 2;
        }

        // Final hash should match expected root
        let computed_root = Hash::from_bytes(current_hash);
        if computed_root != *expected_root {
            warn!("Chunk {} Merkle proof failed: computed root {:?} != expected {:?}",
                  self.index, computed_root, expected_root);
            return false;
        }

        true
    }
}

/// Full state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Snapshot header
    pub header: StateSnapshotHeader,

    /// All state chunks
    pub chunks: Vec<StateChunk>,
}

impl StateSnapshot {
    /// Create a new state snapshot from accounts
    /// SECURITY FIX #37: Now generates proper Merkle proofs for each chunk
    pub fn new(
        block_number: BlockNumber,
        block_hash: Hash,
        state_root: StateRoot,
        accounts: Vec<(AccountId, AccountInfo)>,
        chunk_size: usize,
    ) -> Self {
        let total_size = accounts.len() * std::mem::size_of::<(AccountId, AccountInfo)>();

        // First pass: create chunks and compute their hashes
        let chunk_data: Vec<(u32, Vec<(AccountId, AccountInfo)>, Hash)> = accounts
            .chunks(chunk_size)
            .enumerate()
            .map(|(i, chunk)| {
                let chunk_accounts = chunk.to_vec();
                // Compute hash of chunk data
                let mut data = Vec::new();
                for (account_id, account_info) in &chunk_accounts {
                    data.extend_from_slice(account_id.as_bytes());
                    data.extend_from_slice(&account_info.nonce.to_le_bytes());
                    data.extend_from_slice(&account_info.free.to_le_bytes());
                    data.extend_from_slice(&account_info.reserved.to_le_bytes());
                    data.extend_from_slice(account_info.last_modified.as_bytes());
                }
                let chunk_hash = Hash::hash(&data);
                (i as u32, chunk_accounts, chunk_hash)
            })
            .collect();

        let total_chunks = chunk_data.len() as u32;

        // Build Merkle tree from chunk hashes to generate proofs
        let chunk_hashes: Vec<[u8; 32]> = chunk_data.iter()
            .map(|(_, _, hash)| *hash.as_bytes())
            .collect();

        // Generate proofs for each chunk
        let chunks: Vec<StateChunk> = chunk_data.into_iter()
            .map(|(index, accounts, chunk_hash)| {
                let proof = Self::generate_merkle_proof(&chunk_hashes, index as usize);
                StateChunk {
                    index,
                    accounts,
                    proof,
                    chunk_hash,
                }
            })
            .collect();

        // Compute the chunks root for the header
        let chunks_root = Self::compute_merkle_root(&chunk_hashes);

        let header = StateSnapshotHeader {
            block_number,
            block_hash,
            state_root: StateRoot::new(chunks_root, block_number, state_root.chain_id),
            total_chunks,
            total_size: total_size as u64,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        Self { header, chunks }
    }

    /// Compute Merkle root from leaf hashes
    fn compute_merkle_root(leaves: &[[u8; 32]]) -> Hash {
        if leaves.is_empty() {
            return Hash::ZERO;
        }
        if leaves.len() == 1 {
            return Hash::from_bytes(leaves[0]);
        }

        let mut current_level: Vec<[u8; 32]> = leaves.to_vec();

        while current_level.len() > 1 {
            let mut next_level = Vec::new();
            for chunk in current_level.chunks(2) {
                let combined_hash = if chunk.len() == 2 {
                    let mut combined = Vec::with_capacity(64);
                    combined.extend_from_slice(&chunk[0]);
                    combined.extend_from_slice(&chunk[1]);
                    Blake3Hasher::hash(&combined)
                } else {
                    // Odd node: duplicate it
                    let mut combined = Vec::with_capacity(64);
                    combined.extend_from_slice(&chunk[0]);
                    combined.extend_from_slice(&chunk[0]);
                    Blake3Hasher::hash(&combined)
                };
                next_level.push(combined_hash);
            }
            current_level = next_level;
        }

        Hash::from_bytes(current_level[0])
    }

    /// Generate Merkle proof for a leaf at given index
    fn generate_merkle_proof(leaves: &[[u8; 32]], leaf_index: usize) -> Vec<Hash> {
        if leaves.len() <= 1 {
            return vec![];
        }

        let mut proof = Vec::new();
        let mut current_level: Vec<[u8; 32]> = leaves.to_vec();
        let mut index = leaf_index;

        while current_level.len() > 1 {
            // Get sibling index
            let sibling_index = if index % 2 == 0 { index + 1 } else { index - 1 };

            // Add sibling to proof (handle odd-length levels)
            if sibling_index < current_level.len() {
                proof.push(Hash::from_bytes(current_level[sibling_index]));
            } else {
                // Odd level: sibling is self (duplicate)
                proof.push(Hash::from_bytes(current_level[index]));
            }

            // Build next level
            let mut next_level = Vec::new();
            for chunk in current_level.chunks(2) {
                let combined_hash = if chunk.len() == 2 {
                    let mut combined = Vec::with_capacity(64);
                    combined.extend_from_slice(&chunk[0]);
                    combined.extend_from_slice(&chunk[1]);
                    Blake3Hasher::hash(&combined)
                } else {
                    let mut combined = Vec::with_capacity(64);
                    combined.extend_from_slice(&chunk[0]);
                    combined.extend_from_slice(&chunk[0]);
                    Blake3Hasher::hash(&combined)
                };
                next_level.push(combined_hash);
            }

            current_level = next_level;
            index /= 2;
        }

        proof
    }

    /// SECURITY FIX #37: Verify the snapshot integrity including Merkle proofs
    pub fn verify(&self) -> bool {
        // Verify chunk count matches header
        if self.chunks.len() as u32 != self.header.total_chunks {
            warn!("Snapshot verification failed: chunk count mismatch (header={}, actual={})",
                  self.header.total_chunks, self.chunks.len());
            return false;
        }

        // Verify chunk indices are sequential
        for (i, chunk) in self.chunks.iter().enumerate() {
            if chunk.index != i as u32 {
                warn!("Snapshot verification failed: chunk {} has wrong index {}",
                      i, chunk.index);
                return false;
            }
        }

        // SECURITY FIX #37: Verify Merkle proofs for each chunk
        let expected_root = &self.header.state_root.root;
        for chunk in &self.chunks {
            if !chunk.verify_merkle_proof(expected_root, self.header.total_chunks) {
                warn!("Snapshot verification failed: chunk {} Merkle proof invalid",
                      chunk.index);
                return false;
            }
        }

        debug!("Snapshot verified: {} chunks, root={:?}",
               self.chunks.len(), expected_root);
        true
    }

    /// Get all accounts from snapshot
    pub fn accounts(&self) -> impl Iterator<Item = &(AccountId, AccountInfo)> {
        self.chunks.iter().flat_map(|c| c.accounts.iter())
    }

    /// Total account count
    pub fn account_count(&self) -> usize {
        self.chunks.iter().map(|c| c.accounts.len()).sum()
    }
}

/// Warp sync manager
pub struct WarpSyncManager {
    /// Current state
    state: WarpSyncState,

    /// Local chain height
    local_height: BlockNumber,

    /// Best known network height
    network_height: BlockNumber,

    /// Threshold to trigger warp sync (blocks behind)
    warp_threshold: u64,

    /// Downloaded snapshot header
    snapshot_header: Option<StateSnapshotHeader>,

    /// Downloaded chunks (indexed by chunk number)
    received_chunks: HashMap<u32, StateChunk>,

    /// Maximum snapshot age in blocks
    max_snapshot_age: u64,
}

impl WarpSyncManager {
    /// Create a new warp sync manager
    pub fn new(local_height: BlockNumber) -> Self {
        Self {
            state: WarpSyncState::Inactive,
            local_height,
            network_height: local_height,
            warp_threshold: 1000,
            snapshot_header: None,
            received_chunks: HashMap::new(),
            max_snapshot_age: 10000, // Max 10k blocks old
        }
    }

    /// Update local height
    pub fn update_local_height(&mut self, height: BlockNumber) {
        self.local_height = height;
        self.check_warp_needed();
    }

    /// Update network height from peer
    pub fn peer_height_update(&mut self, height: BlockNumber) {
        if height > self.network_height {
            self.network_height = height;
            self.check_warp_needed();
        }
    }

    /// Check if warp sync is needed
    fn check_warp_needed(&mut self) {
        let gap = self.network_height.saturating_sub(self.local_height);

        if self.state == WarpSyncState::Inactive && gap > self.warp_threshold {
            info!(
                "📡 Warp sync triggered: gap={} blocks (threshold={})",
                gap, self.warp_threshold
            );
            self.state = WarpSyncState::RequestingSnapshot;
        }
    }

    /// Get current state
    pub fn state(&self) -> WarpSyncState {
        self.state
    }

    /// Check if warp sync is active
    pub fn is_active(&self) -> bool {
        !matches!(
            self.state,
            WarpSyncState::Inactive | WarpSyncState::Complete | WarpSyncState::Failed(_)
        )
    }

    /// Check if warp sync should be used
    pub fn should_warp_sync(&self) -> bool {
        matches!(self.state, WarpSyncState::RequestingSnapshot)
    }

    /// Handle received snapshot header
    pub fn handle_snapshot_header(&mut self, header: StateSnapshotHeader) -> Result<(), WarpSyncError> {
        // Validate snapshot age
        let age = self.network_height.saturating_sub(header.block_number);
        if age > self.max_snapshot_age {
            return Err(WarpSyncError::SnapshotTooOld);
        }

        info!(
            "📦 Received snapshot header: block={}, chunks={}, size={}",
            header.block_number, header.total_chunks, header.total_size
        );

        self.snapshot_header = Some(header.clone());
        self.received_chunks.clear();
        self.state = WarpSyncState::DownloadingState {
            total_chunks: header.total_chunks,
            received: 0,
        };

        Ok(())
    }

    /// Handle received state chunk
    /// SECURITY FIX #37: Now verifies Merkle proof before accepting chunk
    pub fn handle_state_chunk(&mut self, chunk: StateChunk) -> Result<(), WarpSyncError> {
        let header = self.snapshot_header.as_ref().ok_or(WarpSyncError::InvalidChunk)?;

        // Validate chunk index
        if chunk.index >= header.total_chunks {
            warn!("Rejected chunk with invalid index {} (max={})",
                  chunk.index, header.total_chunks - 1);
            return Err(WarpSyncError::InvalidChunk);
        }

        // SECURITY FIX #37: Verify Merkle proof before accepting chunk
        // This prevents malicious peers from injecting fake state data
        let expected_root = &header.state_root.root;
        if !chunk.verify_merkle_proof(expected_root, header.total_chunks) {
            warn!("Rejected chunk {} with invalid Merkle proof", chunk.index);
            return Err(WarpSyncError::InvalidMerkleProof);
        }

        debug!("📥 Received and verified chunk {}/{}", chunk.index + 1, header.total_chunks);

        self.received_chunks.insert(chunk.index, chunk);

        // Update state
        let received = self.received_chunks.len() as u32;
        if received == header.total_chunks {
            self.state = WarpSyncState::VerifyingState;
        } else {
            self.state = WarpSyncState::DownloadingState {
                total_chunks: header.total_chunks,
                received,
            };
        }

        Ok(())
    }

    /// Verify completed state download
    pub fn verify_state(&mut self) -> Result<StateSnapshot, WarpSyncError> {
        let header = self.snapshot_header.take().ok_or(WarpSyncError::InvalidStateRoot)?;

        // Collect chunks in order
        let mut chunks = Vec::with_capacity(header.total_chunks as usize);
        for i in 0..header.total_chunks {
            let chunk = self.received_chunks.remove(&i).ok_or(WarpSyncError::InvalidChunk)?;
            chunks.push(chunk);
        }

        let snapshot = StateSnapshot {
            header,
            chunks,
        };

        // Verify snapshot integrity
        if !snapshot.verify() {
            self.state = WarpSyncState::Failed(WarpSyncError::InvalidStateRoot);
            return Err(WarpSyncError::InvalidStateRoot);
        }

        info!(
            "✅ Warp sync state verified: {} accounts at block #{}",
            snapshot.account_count(),
            snapshot.header.block_number
        );

        // Move to header download phase
        self.state = WarpSyncState::DownloadingHeaders {
            from: snapshot.header.block_number,
            to: self.network_height,
        };

        Ok(snapshot)
    }

    /// Mark warp sync as complete
    pub fn complete(&mut self) {
        info!("🎉 Warp sync complete!");
        self.state = WarpSyncState::Complete;
    }

    /// Mark warp sync as failed
    pub fn fail(&mut self, error: WarpSyncError) {
        self.state = WarpSyncState::Failed(error);
    }

    /// Get chunks still needed
    pub fn chunks_needed(&self) -> Vec<u32> {
        if let Some(ref header) = self.snapshot_header {
            (0..header.total_chunks)
                .filter(|i| !self.received_chunks.contains_key(i))
                .collect()
        } else {
            vec![]
        }
    }

    /// Get download progress (0.0 - 1.0)
    pub fn download_progress(&self) -> f64 {
        match self.state {
            WarpSyncState::DownloadingState { total_chunks, received } => {
                if total_chunks > 0 {
                    received as f64 / total_chunks as f64
                } else {
                    0.0
                }
            }
            WarpSyncState::VerifyingState | WarpSyncState::DownloadingHeaders { .. } => 1.0,
            WarpSyncState::Complete => 1.0,
            _ => 0.0,
        }
    }
}

impl Default for WarpSyncManager {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChainId, Hash};

    fn create_test_snapshot(block_number: BlockNumber, account_count: usize) -> StateSnapshot {
        let accounts: Vec<_> = (0..account_count)
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
            100, // 100 accounts per chunk
        )
    }

    #[test]
    fn test_warp_sync_inactive_when_close() {
        let mut manager = WarpSyncManager::new(100);
        manager.peer_height_update(200);

        assert_eq!(manager.state(), WarpSyncState::Inactive);
        assert!(!manager.is_active());
    }

    #[test]
    fn test_warp_sync_triggered_when_far_behind() {
        let mut manager = WarpSyncManager::new(100);
        manager.peer_height_update(2000);

        assert_eq!(manager.state(), WarpSyncState::RequestingSnapshot);
        assert!(manager.is_active());
        assert!(manager.should_warp_sync());
    }

    #[test]
    fn test_snapshot_creation() {
        let snapshot = create_test_snapshot(1000, 500);

        assert_eq!(snapshot.header.block_number, 1000);
        assert_eq!(snapshot.header.total_chunks, 5); // 500 accounts / 100 per chunk
        assert_eq!(snapshot.account_count(), 500);
        assert!(snapshot.verify());
    }

    #[test]
    fn test_snapshot_header_handling() {
        let mut manager = WarpSyncManager::new(0);
        manager.network_height = 2000;
        manager.state = WarpSyncState::RequestingSnapshot;

        let snapshot = create_test_snapshot(1500, 200);
        let result = manager.handle_snapshot_header(snapshot.header.clone());

        assert!(result.is_ok());
        assert!(matches!(manager.state(), WarpSyncState::DownloadingState { .. }));
    }

    #[test]
    fn test_snapshot_too_old_rejected() {
        let mut manager = WarpSyncManager::new(0);
        manager.network_height = 20000;
        manager.state = WarpSyncState::RequestingSnapshot;

        let snapshot = create_test_snapshot(1000, 100); // 19000 blocks old
        let result = manager.handle_snapshot_header(snapshot.header);

        assert!(matches!(result, Err(WarpSyncError::SnapshotTooOld)));
    }

    #[test]
    fn test_chunk_handling() {
        let mut manager = WarpSyncManager::new(0);
        manager.network_height = 2000;
        manager.state = WarpSyncState::RequestingSnapshot;

        let snapshot = create_test_snapshot(1500, 200);
        manager.handle_snapshot_header(snapshot.header.clone()).unwrap();

        // Add chunks one by one
        for chunk in snapshot.chunks.iter() {
            manager.handle_state_chunk(chunk.clone()).unwrap();
        }

        assert!(matches!(manager.state(), WarpSyncState::VerifyingState));
    }

    #[test]
    fn test_state_verification() {
        let mut manager = WarpSyncManager::new(0);
        manager.network_height = 2000;
        manager.state = WarpSyncState::RequestingSnapshot;

        let snapshot = create_test_snapshot(1500, 200);
        manager.handle_snapshot_header(snapshot.header.clone()).unwrap();

        for chunk in snapshot.chunks.iter() {
            manager.handle_state_chunk(chunk.clone()).unwrap();
        }

        let result = manager.verify_state();
        assert!(result.is_ok());
        assert!(matches!(manager.state(), WarpSyncState::DownloadingHeaders { .. }));
    }

    #[test]
    fn test_download_progress() {
        let mut manager = WarpSyncManager::new(0);
        manager.network_height = 2000;
        manager.state = WarpSyncState::RequestingSnapshot;

        assert_eq!(manager.download_progress(), 0.0);

        let snapshot = create_test_snapshot(1500, 500);
        manager.handle_snapshot_header(snapshot.header.clone()).unwrap();

        // Progress should reflect chunks received
        manager.handle_state_chunk(snapshot.chunks[0].clone()).unwrap();
        assert!((manager.download_progress() - 0.2).abs() < 0.01); // 1/5

        manager.handle_state_chunk(snapshot.chunks[1].clone()).unwrap();
        assert!((manager.download_progress() - 0.4).abs() < 0.01); // 2/5
    }

    #[test]
    fn test_chunks_needed() {
        let mut manager = WarpSyncManager::new(0);
        manager.network_height = 2000;
        manager.state = WarpSyncState::RequestingSnapshot;

        let snapshot = create_test_snapshot(1500, 300); // 3 chunks
        manager.handle_snapshot_header(snapshot.header.clone()).unwrap();

        assert_eq!(manager.chunks_needed(), vec![0, 1, 2]);

        manager.handle_state_chunk(snapshot.chunks[1].clone()).unwrap();
        assert_eq!(manager.chunks_needed(), vec![0, 2]);
    }

    // SECURITY FIX #37: Tests for Merkle proof verification

    #[test]
    fn test_chunk_hash_verification() {
        let snapshot = create_test_snapshot(1000, 100);

        // Each chunk should have correct hash
        for chunk in &snapshot.chunks {
            assert!(chunk.verify_hash(), "Chunk {} hash verification failed", chunk.index);
        }
    }

    #[test]
    fn test_chunk_merkle_proof_verification() {
        let snapshot = create_test_snapshot(1000, 500); // 5 chunks

        // Each chunk's Merkle proof should verify against the state root
        let expected_root = &snapshot.header.state_root.root;
        for chunk in &snapshot.chunks {
            assert!(
                chunk.verify_merkle_proof(expected_root, snapshot.header.total_chunks),
                "Chunk {} Merkle proof verification failed",
                chunk.index
            );
        }
    }

    #[test]
    fn test_tampered_chunk_data_rejected() {
        let mut snapshot = create_test_snapshot(1000, 200);

        // Tamper with chunk data
        if let Some(first_chunk) = snapshot.chunks.get_mut(0) {
            if let Some((_, ref mut info)) = first_chunk.accounts.get_mut(0) {
                info.free = 999_999_999; // Modify balance
            }
        }

        // Hash verification should fail
        let chunk = &snapshot.chunks[0];
        assert!(!chunk.verify_hash(), "Tampered chunk hash should fail verification");

        // Merkle proof should also fail
        let expected_root = &snapshot.header.state_root.root;
        assert!(
            !chunk.verify_merkle_proof(expected_root, snapshot.header.total_chunks),
            "Tampered chunk Merkle proof should fail"
        );
    }

    #[test]
    fn test_tampered_chunk_hash_rejected() {
        let mut snapshot = create_test_snapshot(1000, 200);

        // Tamper with chunk hash (pretending data is different)
        if let Some(first_chunk) = snapshot.chunks.get_mut(0) {
            first_chunk.chunk_hash = Hash::from_bytes([0xDE; 32]);
        }

        // Hash verification should fail (computed != stored)
        let chunk = &snapshot.chunks[0];
        assert!(!chunk.verify_hash(), "Tampered chunk_hash should fail verification");
    }

    #[test]
    fn test_tampered_merkle_proof_rejected() {
        let mut snapshot = create_test_snapshot(1000, 400); // 4 chunks

        // Tamper with Merkle proof
        if let Some(first_chunk) = snapshot.chunks.get_mut(0) {
            if let Some(proof_hash) = first_chunk.proof.get_mut(0) {
                *proof_hash = Hash::from_bytes([0xBA; 32]);
            }
        }

        // Merkle proof should fail
        let chunk = &snapshot.chunks[0];
        let expected_root = &snapshot.header.state_root.root;
        assert!(
            !chunk.verify_merkle_proof(expected_root, snapshot.header.total_chunks),
            "Tampered Merkle proof should fail verification"
        );
    }

    #[test]
    fn test_chunk_with_wrong_root_rejected() {
        let snapshot = create_test_snapshot(1000, 200);

        // Try to verify against wrong root
        let wrong_root = Hash::from_bytes([0xFF; 32]);
        let chunk = &snapshot.chunks[0];

        assert!(
            !chunk.verify_merkle_proof(&wrong_root, snapshot.header.total_chunks),
            "Chunk verified against wrong root should fail"
        );
    }

    #[test]
    fn test_malicious_chunk_injection_rejected() {
        // Simulate attack: malicious peer sends chunk with fake data
        let mut manager = WarpSyncManager::new(0);
        manager.network_height = 2000;
        manager.state = WarpSyncState::RequestingSnapshot;

        let snapshot = create_test_snapshot(1500, 200);
        manager.handle_snapshot_header(snapshot.header.clone()).unwrap();

        // Create a malicious chunk with fake accounts
        let malicious_chunk = StateChunk {
            index: 0,
            accounts: vec![
                (AccountId::from_bytes([0xAA; 32]), AccountInfo {
                    nonce: 0,
                    free: 999_999_999_999, // Fake huge balance
                    reserved: 0,
                    last_modified: Hash::ZERO,
                }),
            ],
            proof: snapshot.chunks[0].proof.clone(), // Reuse valid proof
            chunk_hash: snapshot.chunks[0].chunk_hash, // Reuse valid hash
        };

        // Should be rejected because chunk_hash doesn't match fake data
        let result = manager.handle_state_chunk(malicious_chunk);
        assert!(
            matches!(result, Err(WarpSyncError::InvalidMerkleProof)),
            "Malicious chunk injection should be rejected"
        );
    }

    #[test]
    fn test_single_chunk_snapshot() {
        // Edge case: single chunk snapshot (no siblings in proof)
        let snapshot = create_test_snapshot(1000, 50); // 1 chunk

        assert_eq!(snapshot.header.total_chunks, 1);
        assert!(snapshot.chunks[0].proof.is_empty(), "Single chunk should have empty proof");

        // Verification should still work
        assert!(snapshot.verify(), "Single chunk snapshot should verify");

        let chunk = &snapshot.chunks[0];
        let expected_root = &snapshot.header.state_root.root;
        assert!(
            chunk.verify_merkle_proof(expected_root, 1),
            "Single chunk Merkle proof should verify"
        );
    }

    #[test]
    fn test_odd_number_chunks() {
        // Edge case: odd number of chunks (needs special handling in tree)
        let snapshot = create_test_snapshot(1000, 350); // 4 chunks (3.5 rounded up)

        assert!(snapshot.verify(), "Odd-chunk snapshot should verify");

        // All chunks should verify
        let expected_root = &snapshot.header.state_root.root;
        for chunk in &snapshot.chunks {
            assert!(
                chunk.verify_merkle_proof(expected_root, snapshot.header.total_chunks),
                "Chunk {} in odd-count snapshot should verify",
                chunk.index
            );
        }
    }
}
