// Sync - Protocole de synchronisation de la chaîne
use crate::network::protocol::NetworkMessage;
use crate::node::producer::BlockValidator;
use crate::types::*;
use std::collections::{HashMap, VecDeque};
use tracing::{debug, info, warn};

/// État de synchronisation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// En sync avec le réseau
    Synced,

    /// En train de télécharger des blocs
    Downloading,

    /// Très en retard, besoin de warp sync
    FarBehind,

    /// Pas de peers
    Idle,
}

/// Gestionnaire de synchronisation
pub struct SyncManager {
    /// État actuel
    state: SyncState,

    /// Hauteur locale de la chaîne
    local_height: BlockNumber,

    /// Meilleure hauteur connue du réseau
    best_known_height: BlockNumber,

    /// Blocs téléchargés en attente d'import
    pending_blocks: HashMap<BlockNumber, Block>,

    /// File d'attente de blocs à télécharger
    download_queue: VecDeque<BlockNumber>,

    /// Nombre de blocs à télécharger par requête
    batch_size: u32,

    /// Seuil pour déclencher le sync
    sync_threshold: u64,
}

impl SyncManager {
    pub fn new(local_height: BlockNumber) -> Self {
        Self {
            state: SyncState::Idle,
            local_height,
            best_known_height: local_height,
            pending_blocks: HashMap::new(),
            download_queue: VecDeque::new(),
            batch_size: 50,
            sync_threshold: 10,
        }
    }

    /// Met à jour la hauteur locale
    pub fn update_local_height(&mut self, height: BlockNumber) {
        self.local_height = height;
        self.update_state();
    }

    /// Notifie d'une hauteur de chaîne d'un peer
    pub fn peer_height_update(&mut self, peer_height: BlockNumber) {
        if peer_height > self.best_known_height {
            info!("📡 Best known height updated: {}", peer_height);
            self.best_known_height = peer_height;
        }
        self.update_state();
    }

    /// Met à jour l'état de synchronisation
    fn update_state(&mut self) {
        let gap = self.best_known_height.saturating_sub(self.local_height);

        self.state = if gap == 0 {
            SyncState::Synced
        } else if gap > 1000 {
            SyncState::FarBehind
        } else if gap > self.sync_threshold {
            SyncState::Downloading
        } else {
            SyncState::Synced
        };

        debug!("Sync state: {:?}, gap: {}", self.state, gap);
    }

    /// Vérifie si on doit synchroniser
    pub fn should_sync(&self) -> bool {
        matches!(self.state, SyncState::Downloading | SyncState::FarBehind)
    }

    /// Retourne l'état actuel
    pub fn state(&self) -> SyncState {
        self.state
    }

    /// Prépare les blocs à télécharger
    pub fn prepare_download(&mut self) -> Option<Vec<BlockNumber>> {
        if !self.should_sync() {
            return None;
        }

        // Rempli la queue si vide
        if self.download_queue.is_empty() {
            let start = self.local_height + 1;
            let end = (start + self.batch_size as u64).min(self.best_known_height + 1);

            for block_num in start..end {
                if !self.pending_blocks.contains_key(&block_num) {
                    self.download_queue.push_back(block_num);
                }
            }
        }

        if self.download_queue.is_empty() {
            return None;
        }

        // Prend un batch de la queue
        let mut batch = Vec::new();
        for _ in 0..self.batch_size {
            if let Some(block_num) = self.download_queue.pop_front() {
                batch.push(block_num);
            } else {
                break;
            }
        }

        Some(batch)
    }

    /// Ajoute un bloc téléchargé (avec validation de base)
    /// Returns true if the block was accepted, false if validation failed
    pub fn add_downloaded_block(&mut self, block: Block) -> bool {
        let block_num = block.header.number;

        // Validate block signature and transactions root
        // Full validation with parent will happen during import
        if let Err(e) = BlockValidator::validate_standalone(&block) {
            warn!(
                "❌ Block #{} failed standalone validation: {}",
                block_num, e
            );
            return false;
        }

        // Basic sanity checks
        if block_num <= self.local_height {
            warn!(
                "❌ Block #{} is not ahead of local height {}",
                block_num, self.local_height
            );
            return false;
        }

        // Check if block number matches expected range
        if block_num > self.best_known_height + 100 {
            warn!(
                "❌ Block #{} is too far ahead of best known height {}",
                block_num, self.best_known_height
            );
            return false;
        }

        debug!("📥 Block #{} downloaded and validated", block_num);
        self.pending_blocks.insert(block_num, block);
        true
    }

    /// Récupère le prochain bloc séquentiel à importer
    pub fn next_block_to_import(&mut self) -> Option<Block> {
        let next_height = self.local_height + 1;

        self.pending_blocks.remove(&next_height)
    }

    /// Nombre de blocs en attente
    pub fn pending_count(&self) -> usize {
        self.pending_blocks.len()
    }

    /// Gap avec le réseau
    pub fn sync_gap(&self) -> u64 {
        self.best_known_height.saturating_sub(self.local_height)
    }

    /// Crée une requête de sync
    pub fn create_sync_request(&mut self) -> Option<NetworkMessage> {
        let blocks_to_download = self.prepare_download()?;

        if blocks_to_download.is_empty() {
            return None;
        }

        let from_block = *blocks_to_download.first()?;

        Some(NetworkMessage::SyncRequest {
            from_block,
            max_blocks: self.batch_size,
        })
    }

    /// Traite une réponse de sync
    /// Returns the number of blocks that were accepted
    pub fn handle_sync_response(&mut self, blocks: Vec<Block>, has_more: bool) -> usize {
        let total = blocks.len();
        info!("📦 Received {} blocks from sync", total);

        let mut accepted = 0;
        for block in blocks {
            if self.add_downloaded_block(block) {
                accepted += 1;
            }
        }

        if accepted < total {
            warn!(
                "⚠️ Rejected {} blocks during sync",
                total - accepted
            );
        }

        if !has_more {
            debug!("Sync complet pour ce batch");
        }

        accepted
    }
}

impl Default for SyncManager {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_state_synced() {
        let mut sync = SyncManager::new(100);
        sync.peer_height_update(100);

        assert_eq!(sync.state(), SyncState::Synced);
        assert!(!sync.should_sync());
    }

    #[test]
    fn test_sync_state_downloading() {
        let mut sync = SyncManager::new(100);
        sync.peer_height_update(120);

        assert_eq!(sync.state(), SyncState::Downloading);
        assert!(sync.should_sync());
    }

    #[test]
    fn test_sync_state_far_behind() {
        let mut sync = SyncManager::new(100);
        sync.peer_height_update(2000);

        assert_eq!(sync.state(), SyncState::FarBehind);
        assert!(sync.should_sync());
    }

    #[test]
    fn test_prepare_download() {
        let mut sync = SyncManager::new(100);
        sync.peer_height_update(200);

        let batch = sync.prepare_download();
        assert!(batch.is_some());

        let blocks = batch.unwrap();
        assert_eq!(blocks.len(), 50); // batch_size
        assert_eq!(blocks[0], 101);
        assert_eq!(blocks[49], 150);
    }

    #[test]
    fn test_add_and_import_block() {
        use ed25519_dalek::{SigningKey, Signer};

        let mut sync = SyncManager::new(100);
        sync.peer_height_update(200); // Set best known height so block is in valid range

        // Create a properly signed block for testing
        use crate::types::signature::{domain_separate, DOMAIN_BLOCK_HEADER};

        let signing_key = SigningKey::from_bytes(&[1u8; 32]);
        let author = AccountId::from_bytes(signing_key.verifying_key().to_bytes());

        let mut header = BlockHeader {
            number: 101,
            parent_hash: Hash::ZERO,
            transactions_root: Hash::ZERO, // Empty transactions -> ZERO root
            state_root: Hash::ZERO,
            timestamp: 0,
            epoch: 0,
            slot: 0,
            author,
            signature: Signature64([0; 64]),
        };

        // Sign the header with domain separation (SECURITY FIX #24)
        let header_hash = header.hash();
        let message = domain_separate(DOMAIN_BLOCK_HEADER, header_hash.as_bytes());
        let signature = signing_key.sign(&message);
        header.signature = Signature64(signature.to_bytes());

        let block = Block {
            header,
            body: BlockBody {
                transactions: vec![],
            },
        };

        // Block should be accepted
        assert!(sync.add_downloaded_block(block.clone()));
        assert_eq!(sync.pending_count(), 1);

        let imported = sync.next_block_to_import();
        assert!(imported.is_some());
        assert_eq!(imported.unwrap().header.number, 101);
        assert_eq!(sync.pending_count(), 0);
    }

    #[test]
    fn test_reject_invalid_signature_block() {
        use ed25519_dalek::{SigningKey, Signer};

        let mut sync = SyncManager::new(100);
        sync.peer_height_update(200);

        // Create a valid keypair but sign with wrong data
        let signing_key = SigningKey::from_bytes(&[1u8; 32]);
        let author = AccountId::from_bytes(signing_key.verifying_key().to_bytes());

        // Sign with wrong message (not the header hash)
        let wrong_signature = signing_key.sign(b"wrong message");

        // Block with mismatched signature should be rejected
        let block = Block {
            header: BlockHeader {
                number: 101,
                parent_hash: Hash::ZERO,
                transactions_root: Hash::ZERO,
                state_root: Hash::ZERO,
                timestamp: 0,
                epoch: 0,
                slot: 0,
                author,
                signature: Signature64(wrong_signature.to_bytes()), // Wrong signature
            },
            body: BlockBody {
                transactions: vec![],
            },
        };

        // Block should be rejected due to invalid signature
        assert!(!sync.add_downloaded_block(block));
        assert_eq!(sync.pending_count(), 0);
    }

    #[test]
    fn test_reject_block_behind_local_height() {
        use ed25519_dalek::{SigningKey, Signer};
        use crate::types::signature::{domain_separate, DOMAIN_BLOCK_HEADER};

        let mut sync = SyncManager::new(100);
        sync.peer_height_update(200);

        // Create a properly signed block but with number <= local_height
        let signing_key = SigningKey::from_bytes(&[1u8; 32]);
        let author = AccountId::from_bytes(signing_key.verifying_key().to_bytes());

        let mut header = BlockHeader {
            number: 50, // Behind local height of 100
            parent_hash: Hash::ZERO,
            transactions_root: Hash::ZERO,
            state_root: Hash::ZERO,
            timestamp: 0,
            epoch: 0,
            slot: 0,
            author,
            signature: Signature64([0; 64]),
        };

        // Sign with domain separation (SECURITY FIX #24)
        let header_hash = header.hash();
        let message = domain_separate(DOMAIN_BLOCK_HEADER, header_hash.as_bytes());
        let signature = signing_key.sign(&message);
        header.signature = Signature64(signature.to_bytes());

        let block = Block {
            header,
            body: BlockBody {
                transactions: vec![],
            },
        };

        // Block should be rejected - behind local height
        assert!(!sync.add_downloaded_block(block));
        assert_eq!(sync.pending_count(), 0);
    }

    #[test]
    fn test_sync_gap() {
        let mut sync = SyncManager::new(100);
        sync.peer_height_update(250);

        assert_eq!(sync.sync_gap(), 150);
    }
}
