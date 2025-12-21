// Transaction - Types de transactions L0 (minimales)
use super::account::AccountId;
use super::primitives::{Balance, ChainId, Hash, Nonce};
use super::signature::{Signature64, domain_separate, DOMAIN_TRANSACTION};
use serde::{Deserialize, Serialize};

/// Transaction signée
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTransaction {
    /// Transaction non signée
    pub transaction: Transaction,

    /// Signature Ed25519 (64 bytes)
    pub signature: Signature64,

    /// Hash de la transaction (pour indexation)
    #[serde(skip)]
    pub hash: Option<Hash>,
}

impl SignedTransaction {
    pub fn new(transaction: Transaction, signature: [u8; 64]) -> Self {
        let hash = transaction.hash();
        Self {
            transaction,
            signature: Signature64::from(signature),
            hash: Some(hash),
        }
    }

    /// Vérifie la signature
    /// SECURITY FIX #27: Uses domain separation to prevent signature replay attacks
    pub fn verify(&self) -> bool {
        // SECURITY FIX #27: Serialize transaction and apply domain separation
        let tx_bytes = match bincode::serialize(&self.transaction) {
            Ok(bytes) => bytes,
            Err(_) => return false, // Invalid transaction cannot be verified
        };

        // Apply domain separation (same as DOMAIN_BLOCK_HEADER pattern)
        let message = domain_separate(DOMAIN_TRANSACTION, &tx_bytes);

        self.transaction
            .sender
            .verify(&message, self.signature.as_bytes())
    }

    /// Create signing message for a transaction (with domain separation)
    /// SECURITY FIX #27: Use this method when signing transactions
    pub fn signing_message(transaction: &Transaction) -> Option<Vec<u8>> {
        let tx_bytes = bincode::serialize(transaction).ok()?;
        Some(domain_separate(DOMAIN_TRANSACTION, &tx_bytes))
    }

    /// Hash de la transaction
    pub fn hash(&self) -> Hash {
        self.hash.unwrap_or_else(|| self.transaction.hash())
    }
}

/// Transaction non signée (Inner)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Émetteur
    pub sender: AccountId,

    /// Nonce (anti-replay)
    pub nonce: Nonce,

    /// Type de transaction
    pub call: TransactionCall,

    /// Timestamp de création (optionnel, pour tri)
    pub timestamp: u64,
}

impl Transaction {
    pub fn new(sender: AccountId, nonce: Nonce, call: TransactionCall) -> Self {
        Self {
            sender,
            nonce,
            call,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Hash de la transaction
    /// SECURITY FIX #28: Safe serialization without panic
    pub fn hash(&self) -> Hash {
        // Use hash of all fields combined as fallback if serialization fails
        match bincode::serialize(self) {
            Ok(bytes) => Hash::hash(&bytes),
            Err(_) => {
                // Fallback: hash key fields manually
                // This should never happen with valid transactions, but we handle it gracefully
                let mut data = Vec::new();
                data.extend_from_slice(self.sender.as_bytes());
                data.extend_from_slice(&self.nonce.to_le_bytes());
                data.extend_from_slice(&self.timestamp.to_le_bytes());
                Hash::hash(&data)
            }
        }
    }
}

/// Types de transactions L0 (MINIMAL - pas de Turing-complet)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionCall {
    /// Transfert simple
    Transfer {
        to: AccountId,
        amount: Balance,
    },

    /// Staking - Bond tokens
    Stake {
        amount: Balance,
    },

    /// Unstake - Unbond tokens (lent, ex: 28 jours)
    Unstake {
        amount: Balance,
    },

    /// Withdraw unbonded (après période d'attente)
    WithdrawUnbonded,

    /// Enregistrement validateur
    RegisterValidator {
        /// Stake initial
        stake: Balance,
    },

    /// Désenregistrement validateur
    UnregisterValidator,

    /// Création de sidechain (permissionless)
    CreateSidechain {
        /// Métadonnées minimales
        metadata: SidechainMetadata,
        /// Dépôt initial
        deposit: Balance,
    },

    /// Exit d'une sidechain
    ExitSidechain {
        chain_id: ChainId,
    },

    /// Signal de fork (pour migration)
    SignalFork {
        /// Nom du fork
        name: String,
        /// Description
        description: String,
    },

    // =========================================================================
    // EARLY VALIDATOR VOTING (Bootstrap Era Only)
    // Constitutional: Progressive decentralization through voting
    // =========================================================================

    /// Propose a new early validator candidate
    /// Can only be submitted by existing validators during bootstrap era
    ProposeEarlyValidator {
        /// Candidate account to propose
        candidate: AccountId,
    },

    /// Vote for an early validator candidate
    /// Can only be submitted by existing validators during bootstrap era
    VoteEarlyValidator {
        /// Candidate to vote for
        candidate: AccountId,
    },
}

impl TransactionCall {
    /// Estimation du coût (simple, pas de gas complexe)
    pub fn base_fee(&self) -> Balance {
        match self {
            TransactionCall::Transfer { .. } => 1_000, // 0.000001 KRAT
            TransactionCall::Stake { .. } => 5_000,
            TransactionCall::Unstake { .. } => 5_000,
            TransactionCall::WithdrawUnbonded => 2_000,
            TransactionCall::RegisterValidator { .. } => 100_000,
            TransactionCall::UnregisterValidator => 50_000,
            TransactionCall::CreateSidechain { .. } => 1_000_000, // 0.001 KRAT
            TransactionCall::ExitSidechain { .. } => 500_000,
            TransactionCall::SignalFork { .. } => 10_000_000, // 0.01 KRAT (décourage spam)
            // Early validator voting (bootstrap only)
            // Low fees to encourage participation in decentralization
            TransactionCall::ProposeEarlyValidator { .. } => 50_000, // 0.00005 KRAT
            TransactionCall::VoteEarlyValidator { .. } => 10_000,    // 0.00001 KRAT
        }
    }
}

/// Métadonnées minimales d'une sidechain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidechainMetadata {
    /// Nom (optionnel)
    pub name: Option<String>,

    /// Description (optionnel)
    pub description: Option<String>,

    /// Parent chain (optionnel, pour hiérarchie)
    pub parent_chain: Option<ChainId>,
}

/// Résultat d'exécution d'une transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionResult {
    Success {
        /// Hash de la transaction
        tx_hash: Hash,
        /// Frais payés
        fee_paid: Balance,
    },
    Failure {
        /// Hash de la transaction
        tx_hash: Hash,
        /// Raison de l'échec
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_hash_deterministic() {
        let sender = AccountId::from_bytes([1u8; 32]);
        let tx1 = Transaction::new(
            sender,
            0,
            TransactionCall::Transfer {
                to: AccountId::from_bytes([2u8; 32]),
                amount: 1000,
            },
        );

        let hash1 = tx1.hash();
        let hash2 = tx1.hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_base_fees() {
        assert_eq!(
            TransactionCall::Transfer {
                to: AccountId::from_bytes([0; 32]),
                amount: 100
            }
            .base_fee(),
            1_000
        );

        assert_eq!(
            TransactionCall::RegisterValidator { stake: 10000 }.base_fee(),
            100_000
        );
    }
}
