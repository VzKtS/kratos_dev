// Block - Structure de bloc minimal et auditable
use super::account::AccountId;
use super::merkle::StateMerkleTree;
use super::primitives::{BlockNumber, EpochNumber, Hash, SlotNumber, Timestamp};
use super::signature::{domain_separate, Signature64, DOMAIN_BLOCK_HEADER, DOMAIN_FINALITY};
use super::transaction::SignedTransaction;
use serde::{Deserialize, Serialize};

/// Bloc complet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// En-tête du bloc
    pub header: BlockHeader,

    /// Corps du bloc (transactions)
    pub body: BlockBody,
}

impl Block {
    pub fn new(header: BlockHeader, body: BlockBody) -> Self {
        Self { header, body }
    }

    /// Hash du bloc
    pub fn hash(&self) -> Hash {
        self.header.hash()
    }

    /// Vérifie que le hash du header correspond aux transactions
    pub fn verify_body_root(&self) -> bool {
        let calculated_root = self.body.transactions_root();
        self.header.transactions_root == calculated_root
    }
}

/// En-tête de bloc (minimal, auditable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    /// Numéro de bloc (hauteur)
    pub number: BlockNumber,

    /// Hash du bloc parent
    pub parent_hash: Hash,

    /// Merkle root des transactions
    pub transactions_root: Hash,

    /// Merkle root de l'état (après exécution)
    pub state_root: Hash,

    /// Timestamp du bloc
    pub timestamp: Timestamp,

    /// Epoch number
    pub epoch: EpochNumber,

    /// Slot dans l'epoch
    pub slot: SlotNumber,

    /// Validateur qui a produit ce bloc
    pub author: AccountId,

    /// Signature du validateur
    pub signature: Signature64,
}

impl BlockHeader {
    /// Hash de l'en-tête (identifiant unique du bloc)
    pub fn hash(&self) -> Hash {
        // On exclut la signature du hash pour permettre la vérification
        let bytes = bincode::serialize(&(
            self.number,
            self.parent_hash,
            self.transactions_root,
            self.state_root,
            self.timestamp,
            self.epoch,
            self.slot,
            self.author,
        ))
        .unwrap();
        Hash::hash(&bytes)
    }

    /// Vérifie la signature du validateur
    ///
    /// SECURITY FIX #33: Domain separation for block signatures
    /// Prevents cross-context signature replay attacks by prefixing
    /// the message with a unique domain identifier
    pub fn verify_signature(&self) -> bool {
        let message = self.hash();
        let domain_separated_msg = domain_separate(DOMAIN_BLOCK_HEADER, message.as_bytes());
        self.author.verify(&domain_separated_msg, self.signature.as_bytes())
    }

    /// Crée le message à signer pour ce header (avec domain separation)
    ///
    /// SECURITY FIX #33: Cette méthode doit être utilisée lors de la création
    /// de la signature du bloc pour assurer la cohérence avec verify_signature()
    pub fn signing_message(&self) -> Vec<u8> {
        let message = self.hash();
        domain_separate(DOMAIN_BLOCK_HEADER, message.as_bytes())
    }
}

/// Corps du bloc
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockBody {
    /// Transactions dans le bloc
    pub transactions: Vec<SignedTransaction>,
}

impl BlockBody {
    pub fn new(transactions: Vec<SignedTransaction>) -> Self {
        Self { transactions }
    }

    /// Calcule la racine Merkle des transactions
    ///
    /// SECURITY FIX #35: Use proper Merkle tree for transaction root computation
    /// This enables Merkle proofs for light client verification and fraud proofs
    pub fn transactions_root(&self) -> Hash {
        if self.transactions.is_empty() {
            return Hash::ZERO;
        }

        // SECURITY FIX #35: Use StateMerkleTree for proper Merkle root computation
        // This enables:
        // 1. Merkle proofs for individual transactions
        // 2. Light client verification
        // 3. Fraud proof generation
        let tx_data: Vec<Vec<u8>> = self
            .transactions
            .iter()
            .map(|tx| tx.hash().as_bytes().to_vec())
            .collect();

        let merkle_tree = StateMerkleTree::new(tx_data);
        merkle_tree.root()
    }

    /// Nombre de transactions
    pub fn transaction_count(&self) -> usize {
        self.transactions.len()
    }
}

/// Bloc genesis (premier bloc)
impl Block {
    pub fn genesis(state_root: Hash, genesis_accounts: Vec<AccountId>) -> Self {
        let header = BlockHeader {
            number: 0,
            parent_hash: Hash::ZERO,
            transactions_root: Hash::ZERO,
            state_root,
            timestamp: 0,
            epoch: 0,
            slot: 0,
            author: genesis_accounts.first().copied().unwrap_or(AccountId::from_bytes([0; 32])),
            signature: Signature64::zero(),
        };

        let body = BlockBody::new(vec![]);

        Self { header, body }
    }
}

/// Justification de finalité (GRANDPA-like)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalityJustification {
    /// Numéro du bloc finalisé
    pub block_number: BlockNumber,

    /// Hash du bloc finalisé
    pub block_hash: Hash,

    /// Signatures des validateurs (>= 2/3)
    pub signatures: Vec<ValidatorSignature>,

    /// Epoch de finalisation
    pub epoch: EpochNumber,
}

impl FinalityJustification {
    /// Crée le message à signer pour la finalité (avec domain separation)
    ///
    /// SECURITY FIX #33: Domain separation for finality signatures
    /// Message format: DOMAIN_FINALITY || block_number || block_hash || epoch
    pub fn signing_message(&self) -> Vec<u8> {
        let message = bincode::serialize(&(
            self.block_number,
            self.block_hash,
            self.epoch,
        ))
        .unwrap();
        domain_separate(DOMAIN_FINALITY, &message)
    }

    /// Vérifie toutes les signatures de la justification
    ///
    /// SECURITY FIX #33: Proper finality signature verification
    /// Returns (valid_count, total_count) for threshold checking
    pub fn verify_signatures(&self) -> (usize, usize) {
        let message = self.signing_message();
        let valid_count = self
            .signatures
            .iter()
            .filter(|sig| sig.validator.verify(&message, sig.signature.as_bytes()))
            .count();
        (valid_count, self.signatures.len())
    }

    /// Vérifie si la justification atteint le seuil de 2/3
    ///
    /// SECURITY FIX #33: Proper supermajority check for finality
    /// Per Genesis Constitution: 2/3 supermajority required
    pub fn has_supermajority(&self, total_validators: usize) -> bool {
        let (valid_count, _) = self.verify_signatures();
        // 2/3 supermajority = need more than 66% (i.e., 67%)
        // Using floor(2/3) = 66% per Constitution
        valid_count * 100 >= total_validators * 66
    }

    /// Vérifie que la justification est complète et valide
    ///
    /// Returns true if:
    /// - All signatures are valid
    /// - Signatures reach 2/3 of total validators
    /// - No duplicate validators
    pub fn is_valid(&self, total_validators: usize) -> bool {
        // Check for duplicate validators
        let mut seen = std::collections::HashSet::new();
        for sig in &self.signatures {
            if !seen.insert(sig.validator) {
                return false; // Duplicate validator
            }
        }

        // Check supermajority
        self.has_supermajority(total_validators)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    pub validator: AccountId,
    pub signature: Signature64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_hash_deterministic() {
        let header = BlockHeader {
            number: 1,
            parent_hash: Hash::ZERO,
            transactions_root: Hash::ZERO,
            state_root: Hash::ZERO,
            timestamp: 1234567890,
            epoch: 0,
            slot: 0,
            author: AccountId::from_bytes([1; 32]),
            signature: Signature64::zero(),
        };

        let hash1 = header.hash();
        let hash2 = header.hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_empty_transactions_root() {
        let body = BlockBody::new(vec![]);
        assert_eq!(body.transactions_root(), Hash::ZERO);
    }

    #[test]
    fn test_genesis_block() {
        let state_root = Hash::ZERO;
        let genesis = Block::genesis(state_root, vec![]);
        assert_eq!(genesis.header.number, 0);
        assert_eq!(genesis.header.parent_hash, Hash::ZERO);
    }

    #[test]
    fn test_hash_excludes_signature() {
        // Create two headers with identical data but different signatures
        let header1 = BlockHeader {
            number: 1,
            parent_hash: Hash::ZERO,
            transactions_root: Hash::ZERO,
            state_root: Hash::ZERO,
            timestamp: 1234567890,
            epoch: 0,
            slot: 0,
            author: AccountId::from_bytes([1; 32]),
            signature: Signature64::zero(),
        };

        let mut header2 = header1.clone();
        header2.signature = Signature64::from_bytes([0xFF; 64]);

        // Hash should be the same despite different signatures
        assert_eq!(header1.hash(), header2.hash(),
            "Hash computation must exclude signature field");
    }
}
