// Merkle - State roots and Merkle proof infrastructure (SPEC v3.1 Phase 4)
use super::primitives::{BlockNumber, ChainId, Hash};
use rs_merkle::{algorithms::Sha256, Hasher, MerkleTree};
use serde::{Deserialize, Serialize};

/// Blake3-based hasher for Merkle trees (consistent with rest of KratOs)
#[derive(Clone)]
pub struct Blake3Hasher;

impl Hasher for Blake3Hasher {
    type Hash = [u8; 32];

    fn hash(data: &[u8]) -> Self::Hash {
        blake3::hash(data).into()
    }
}

/// State root for a blockchain state at a specific block
/// Represents the Merkle root of all account states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateRoot {
    /// The Merkle root hash
    pub root: Hash,

    /// Block number when this state root was committed
    pub block_number: BlockNumber,

    /// Chain ID (for cross-chain verification)
    pub chain_id: ChainId,
}

impl StateRoot {
    pub fn new(root: Hash, block_number: BlockNumber, chain_id: ChainId) -> Self {
        Self {
            root,
            block_number,
            chain_id,
        }
    }

    /// Zero state root (for genesis)
    pub fn zero(chain_id: ChainId) -> Self {
        Self {
            root: Hash::ZERO,
            block_number: 0,
            chain_id,
        }
    }
}

/// Merkle inclusion proof for cross-chain verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// The leaf data being proven
    pub leaf: Vec<u8>,

    /// The leaf index in the tree
    pub leaf_index: usize,

    /// Merkle proof path (sibling hashes)
    pub proof: Vec<[u8; 32]>,

    /// The root this proof verifies against
    pub root: Hash,

    /// Block number of the state root
    pub block_number: BlockNumber,

    /// Chain ID this proof is from
    pub chain_id: ChainId,
}

impl MerkleProof {
    pub fn new(
        leaf: Vec<u8>,
        leaf_index: usize,
        proof: Vec<[u8; 32]>,
        root: Hash,
        block_number: BlockNumber,
        chain_id: ChainId,
    ) -> Self {
        Self {
            leaf,
            leaf_index,
            proof,
            root,
            block_number,
            chain_id,
        }
    }

    /// Verify this Merkle proof
    pub fn verify(&self) -> bool {
        // Compute leaf hash
        let leaf_hash = Blake3Hasher::hash(&self.leaf);

        // Rebuild root from proof path
        let computed_root = self.compute_root_from_proof(leaf_hash);

        // Verify matches expected root
        computed_root == *self.root.as_bytes()
    }

    /// Compute root hash from proof path (internal helper)
    fn compute_root_from_proof(&self, mut current_hash: [u8; 32]) -> [u8; 32] {
        let mut index = self.leaf_index;

        for sibling in &self.proof {
            // Determine if current node is left or right child
            let is_left = index % 2 == 0;

            // Combine with sibling
            current_hash = if is_left {
                // Current is left, sibling is right
                let mut combined = Vec::with_capacity(64);
                combined.extend_from_slice(&current_hash);
                combined.extend_from_slice(sibling);
                Blake3Hasher::hash(&combined)
            } else {
                // Current is right, sibling is left
                let mut combined = Vec::with_capacity(64);
                combined.extend_from_slice(sibling);
                combined.extend_from_slice(&current_hash);
                Blake3Hasher::hash(&combined)
            };

            // Move up the tree
            index /= 2;
        }

        current_hash
    }
}

/// Merkle tree builder for state roots
pub struct StateMerkleTree {
    /// The underlying Merkle tree
    tree: MerkleTree<Blake3Hasher>,

    /// Leaves data (for proof generation)
    leaves: Vec<Vec<u8>>,
}

impl StateMerkleTree {
    /// Create a new Merkle tree from state data
    pub fn new(leaves: Vec<Vec<u8>>) -> Self {
        // Hash all leaves
        let leaf_hashes: Vec<[u8; 32]> = leaves
            .iter()
            .map(|leaf| Blake3Hasher::hash(leaf))
            .collect();

        // Build tree
        let tree = MerkleTree::<Blake3Hasher>::from_leaves(&leaf_hashes);

        Self { tree, leaves }
    }

    /// Get the Merkle root
    pub fn root(&self) -> Hash {
        match self.tree.root() {
            Some(root_hash) => Hash::from_bytes(root_hash),
            None => Hash::ZERO, // Empty tree
        }
    }

    /// Generate inclusion proof for a leaf at given index
    pub fn generate_proof(
        &self,
        leaf_index: usize,
        block_number: BlockNumber,
        chain_id: ChainId,
    ) -> Option<MerkleProof> {
        if leaf_index >= self.leaves.len() {
            return None;
        }

        // Get proof from tree
        let indices = vec![leaf_index];
        let proof_data = self.tree.proof(&indices);
        let proof_hashes = proof_data.proof_hashes().to_vec();

        Some(MerkleProof::new(
            self.leaves[leaf_index].clone(),
            leaf_index,
            proof_hashes,
            self.root(),
            block_number,
            chain_id,
        ))
    }

    /// Verify a proof against this tree
    pub fn verify_proof(&self, proof: &MerkleProof) -> bool {
        proof.verify() && proof.root == self.root()
    }
}

/// State commitment - what gets stored per block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateCommitment {
    /// The state root
    pub state_root: StateRoot,

    /// Number of accounts in this state
    pub account_count: u64,

    /// Total supply at this block
    pub total_supply: u128,
}

impl StateCommitment {
    pub fn new(state_root: StateRoot, account_count: u64, total_supply: u128) -> Self {
        Self {
            state_root,
            account_count,
            total_supply,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merkle_tree_creation() {
        let leaves = vec![
            b"account1".to_vec(),
            b"account2".to_vec(),
            b"account3".to_vec(),
            b"account4".to_vec(),
        ];

        let tree = StateMerkleTree::new(leaves);
        let root = tree.root();

        assert_ne!(root, Hash::ZERO);
    }

    #[test]
    fn test_merkle_proof_generation_and_verification() {
        let leaves = vec![
            b"account1".to_vec(),
            b"account2".to_vec(),
            b"account3".to_vec(),
            b"account4".to_vec(),
        ];

        let tree = StateMerkleTree::new(leaves);
        let chain_id = ChainId(1);

        // Generate proof for second leaf
        let proof = tree.generate_proof(1, 100, chain_id).unwrap();

        // Verify proof
        assert!(proof.verify());
        assert!(tree.verify_proof(&proof));
        assert_eq!(proof.chain_id, chain_id);
        assert_eq!(proof.block_number, 100);
    }

    #[test]
    fn test_merkle_proof_invalid_modification() {
        let leaves = vec![
            b"account1".to_vec(),
            b"account2".to_vec(),
            b"account3".to_vec(),
            b"account4".to_vec(),
        ];

        let tree = StateMerkleTree::new(leaves);
        let mut proof = tree.generate_proof(1, 100, ChainId(1)).unwrap();

        // Tamper with leaf data
        proof.leaf = b"account2_modified".to_vec();

        // Proof should fail
        assert!(!proof.verify());
    }

    #[test]
    fn test_merkle_proof_wrong_root() {
        let leaves1 = vec![b"account1".to_vec(), b"account2".to_vec()];
        let leaves2 = vec![b"account3".to_vec(), b"account4".to_vec()];

        let tree1 = StateMerkleTree::new(leaves1);
        let tree2 = StateMerkleTree::new(leaves2);

        let proof = tree1.generate_proof(0, 100, ChainId(1)).unwrap();

        // Proof from tree1 should not verify against tree2
        assert!(!tree2.verify_proof(&proof));
    }

    #[test]
    fn test_empty_tree() {
        let tree = StateMerkleTree::new(vec![]);
        assert_eq!(tree.root(), Hash::ZERO);
    }

    #[test]
    fn test_single_leaf_tree() {
        let leaves = vec![b"single_account".to_vec()];
        let tree = StateMerkleTree::new(leaves);

        let proof = tree.generate_proof(0, 0, ChainId(0)).unwrap();
        assert!(proof.verify());
        assert!(tree.verify_proof(&proof));
    }

    #[test]
    fn test_state_root_creation() {
        let root_hash = Hash::from_bytes([1; 32]);
        let state_root = StateRoot::new(root_hash, 100, ChainId(1));

        assert_eq!(state_root.root, root_hash);
        assert_eq!(state_root.block_number, 100);
        assert_eq!(state_root.chain_id, ChainId(1));
    }

    #[test]
    fn test_state_root_zero() {
        let state_root = StateRoot::zero(ChainId(1));
        assert_eq!(state_root.root, Hash::ZERO);
        assert_eq!(state_root.block_number, 0);
    }

    #[test]
    fn test_state_commitment_creation() {
        let root_hash = Hash::from_bytes([2; 32]);
        let state_root = StateRoot::new(root_hash, 200, ChainId(2));
        let commitment = StateCommitment::new(state_root, 1000, 5_000_000);

        assert_eq!(commitment.account_count, 1000);
        assert_eq!(commitment.total_supply, 5_000_000);
        assert_eq!(commitment.state_root.block_number, 200);
    }

    #[test]
    fn test_merkle_proof_deterministic() {
        let leaves = vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()];

        let tree1 = StateMerkleTree::new(leaves.clone());
        let tree2 = StateMerkleTree::new(leaves);

        // Same inputs should produce same root
        assert_eq!(tree1.root(), tree2.root());

        let proof1 = tree1.generate_proof(0, 0, ChainId(0)).unwrap();
        let proof2 = tree2.generate_proof(0, 0, ChainId(0)).unwrap();

        // Same proofs should have same structure
        assert_eq!(proof1.proof.len(), proof2.proof.len());
    }
}
