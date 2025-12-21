// State - Blockchain state machine
use super::db::{Database, DatabaseError, WriteOp};
use crate::consensus::clock_health::ValidatorClockRecord;
use crate::consensus::validator_credits::ValidatorCreditsRecord;
use crate::types::{AccountId, AccountInfo, Balance, Block, BlockNumber, ChainId, Hash, StateRoot, StateMerkleTree, EpochNumber};
use std::collections::HashMap;

/// Storage key prefixes
const PREFIX_ACCOUNT: &[u8] = b"account:";
const PREFIX_VC: &[u8] = b"vc:";
const PREFIX_UNBONDING: &[u8] = b"unbonding:";
const PREFIX_BLOCK_HASH: &[u8] = b"block_hash:";
const PREFIX_BLOCK_BY_HASH: &[u8] = b"block_by_hash:";
const PREFIX_BLOCK_BY_NUMBER: &[u8] = b"block_by_num:";
const PREFIX_STATE_ROOT: &[u8] = b"state_root:";
const PREFIX_CLOCK_RECORD: &[u8] = b"clock_rec:";
const KEY_BEST_BLOCK: &[u8] = b"best_block";
const KEY_GENESIS_HASH: &[u8] = b"genesis_hash";
const KEY_DRIFT_TRACKER: &[u8] = b"drift_tracker";

// =============================================================================
// DRIFT TRACKER - SECURITY FIX #35: Timestamp manipulation prevention
// =============================================================================

/// Maximum cumulative INCREMENTAL drift allowed per epoch (in seconds)
/// This tracks drift BETWEEN consecutive blocks, not absolute drift from genesis.
/// 600 slots/epoch × 2s tolerance = 20 minutes max incremental drift per epoch.
/// This allows network gaps while still detecting gradual manipulation.
pub const MAX_CUMULATIVE_DRIFT_PER_EPOCH: i64 = 1200;

/// Maximum drift per single block relative to parent (in seconds)
/// This is the expected_interval vs actual_interval tolerance.
/// With 6s slots: allows 6s ± 5s = 1s to 11s between blocks.
pub const MAX_SINGLE_BLOCK_DRIFT: i64 = 5;

/// Grace period for first block after node restart (in seconds)
/// When a node restarts, it may produce blocks with timestamps far from
/// expected slot time. We allow a large grace window for the FIRST block
/// only, then enforce normal drift limits.
pub const RESTART_GRACE_DRIFT: i64 = 3600; // 1 hour

/// Drift tracker state - persisted in consensus state
///
/// SECURITY FIX #35: Tracks cumulative timestamp drift to prevent manipulation.
///
/// Uses INCREMENTAL drift (relative to parent block) rather than absolute drift
/// from genesis. This allows the network to recover from gaps (node restarts,
/// network partitions) while still detecting gradual timestamp manipulation.
///
/// The key insight: We only care about the RATE of drift, not absolute offset.
/// A validator who consistently adds 1s extra per block is manipulating time.
/// A validator who catches up after a restart is not.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DriftTracker {
    /// Cumulative INCREMENTAL drift in current epoch (seconds, signed)
    /// This is the sum of (actual_interval - expected_interval) for each block.
    /// Positive = blocks are consistently slower than expected
    /// Negative = blocks are consistently faster than expected
    pub epoch_drift: i64,

    /// Current epoch being tracked
    pub current_epoch: u64,

    /// Genesis timestamp - used for epoch/slot calculation
    pub genesis_timestamp: u64,

    /// Last validated block number
    pub last_block: BlockNumber,

    /// Last validated block timestamp
    pub last_timestamp: u64,

    /// Last validated block slot
    pub last_slot: u64,

    /// Whether we're in a "restart grace period"
    /// Set to true when a large time gap is detected, reset after one block
    pub restart_grace_active: bool,
}

impl DriftTracker {
    /// Create a new drift tracker for genesis
    pub fn new(genesis_timestamp: u64) -> Self {
        Self {
            epoch_drift: 0,
            current_epoch: 0,
            genesis_timestamp,
            last_block: 0,
            last_timestamp: genesis_timestamp,
            last_slot: 0,
            restart_grace_active: false,
        }
    }

    /// Validate a block's timestamp and update drift tracking
    /// Returns Ok(drift) on success, Err on validation failure
    ///
    /// INCREMENTAL DRIFT MODEL:
    /// Instead of measuring drift from genesis, we measure the drift of the
    /// INTERVAL between blocks:
    ///   expected_interval = (current_slot - last_slot) × slot_duration
    ///   actual_interval = current_timestamp - last_timestamp
    ///   incremental_drift = actual_interval - expected_interval
    ///
    /// This allows catching up after restarts while detecting manipulation.
    pub fn validate_and_update(
        &mut self,
        block_number: BlockNumber,
        block_timestamp: u64,
        block_epoch: u64,
        block_slot: u64,
        slot_duration_secs: u64,
    ) -> Result<i64, DriftError> {
        // Special case: genesis block (block 0)
        if block_number == 0 {
            self.last_block = 0;
            self.last_timestamp = block_timestamp;
            self.last_slot = block_slot;
            self.current_epoch = block_epoch;
            return Ok(0);
        }

        // Check: Timestamp must be after parent
        if block_timestamp <= self.last_timestamp {
            return Err(DriftError::TimestampNotAfterParent {
                block_timestamp,
                parent_timestamp: self.last_timestamp,
            });
        }

        // Calculate expected interval (slots elapsed × slot_duration)
        let slots_elapsed = block_slot.saturating_sub(self.last_slot);
        let expected_interval = slots_elapsed.saturating_mul(slot_duration_secs);

        // Calculate actual interval
        let actual_interval = block_timestamp.saturating_sub(self.last_timestamp);

        // Calculate incremental drift
        let incremental_drift = (actual_interval as i64).saturating_sub(expected_interval as i64);

        // Detect restart/network gap: if time gap is huge, enter grace mode
        if actual_interval > expected_interval.saturating_add(RESTART_GRACE_DRIFT as u64) {
            // Large gap detected - likely restart or network partition
            // Allow this block but mark grace as active
            tracing::info!(
                "Large time gap detected: {}s vs expected {}s - grace period",
                actual_interval, expected_interval
            );
            self.restart_grace_active = true;
            // Don't count this huge drift - it would blow up the cumulative
            // Just update state and return 0 drift for this block
            self.last_block = block_number;
            self.last_timestamp = block_timestamp;
            self.last_slot = block_slot;
            if block_epoch > self.current_epoch {
                self.epoch_drift = 0;
                self.current_epoch = block_epoch;
            }
            return Ok(0);
        }

        // If we were in grace mode, exit it now (one block grace)
        if self.restart_grace_active {
            self.restart_grace_active = false;
        }

        // Check 1: Single block incremental drift limit
        if incremental_drift.abs() > MAX_SINGLE_BLOCK_DRIFT {
            return Err(DriftError::SingleBlockDriftExceeded {
                drift: incremental_drift,
                max_allowed: MAX_SINGLE_BLOCK_DRIFT,
                block_number,
            });
        }

        // Reset epoch drift on epoch boundary
        if block_epoch > self.current_epoch {
            self.epoch_drift = 0;
            self.current_epoch = block_epoch;
        }

        // Accumulate incremental drift
        self.epoch_drift = self.epoch_drift.saturating_add(incremental_drift);

        // Check 2: Cumulative incremental drift limit
        if self.epoch_drift.abs() > MAX_CUMULATIVE_DRIFT_PER_EPOCH {
            return Err(DriftError::CumulativeDriftExceeded {
                cumulative_drift: self.epoch_drift,
                max_allowed: MAX_CUMULATIVE_DRIFT_PER_EPOCH,
                epoch: block_epoch,
            });
        }

        // Update tracker state
        self.last_block = block_number;
        self.last_timestamp = block_timestamp;
        self.last_slot = block_slot;

        Ok(incremental_drift)
    }

    /// Get current cumulative drift for the epoch
    pub fn current_drift(&self) -> i64 {
        self.epoch_drift
    }

    /// Check if drift is within safe bounds (for monitoring)
    pub fn is_healthy(&self) -> bool {
        self.epoch_drift.abs() < MAX_CUMULATIVE_DRIFT_PER_EPOCH / 2
    }
}

/// Drift validation errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum DriftError {
    #[error("Single block drift exceeded: {drift}s (max ±{max_allowed}s) at block {block_number}")]
    SingleBlockDriftExceeded {
        drift: i64,
        max_allowed: i64,
        block_number: BlockNumber,
    },

    #[error("Cumulative epoch drift exceeded: {cumulative_drift}s (max ±{max_allowed}s) in epoch {epoch}")]
    CumulativeDriftExceeded {
        cumulative_drift: i64,
        max_allowed: i64,
        epoch: u64,
    },

    #[error("Timestamp not after parent: block={block_timestamp}, parent={parent_timestamp}")]
    TimestampNotAfterParent {
        block_timestamp: u64,
        parent_timestamp: u64,
    },
}

/// Unbonding request - tracks stake being unbonded with release block
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UnbondingRequest {
    /// Amount being unbonded
    pub amount: Balance,
    /// Block number when unbonding was initiated
    pub unbonding_started: BlockNumber,
    /// Block number when funds can be withdrawn (started + UNBONDING_PERIOD)
    pub release_block: BlockNumber,
}

/// All unbonding requests for an account
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct UnbondingInfo {
    /// List of pending unbonding requests
    pub requests: Vec<UnbondingRequest>,
}

/// State backend - Gère l'état de la blockchain
///
/// SECURITY FIX #20: Thread-safety documentation and atomic operations.
///
/// This implementation uses a write-through cache with the following guarantees:
/// 1. Always writing to DB before updating cache (write-through)
/// 2. Clearing cache on batch commits (invalidation)
/// 3. Cache generation tracking for staleness detection
///
/// THREAD SAFETY: This struct is NOT thread-safe by itself.
/// For concurrent access, wrap in Arc<RwLock<StateBackend>> and ensure:
/// - All read-modify-write operations hold the write lock for the entire operation
/// - Cache is invalidated after external state changes
/// - Use `execute_atomic` for operations that must be atomic
///
/// INVARIANT: cache_generation monotonically increases on each invalidation.
/// This allows detecting stale cached data in multi-threaded scenarios.
pub struct StateBackend {
    db: Database,
    /// Cache en mémoire pour optimisation
    /// SECURITY: Write-through cache - DB is always authoritative
    account_cache: HashMap<AccountId, AccountInfo>,
    /// Generation counter for cache staleness detection
    /// Increments on every invalidation, wraps around after u64::MAX
    cache_generation: u64,
    /// Tracks if we're in an atomic operation (for debugging)
    #[cfg(debug_assertions)]
    in_atomic_operation: bool,
}

impl StateBackend {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            account_cache: HashMap::new(),
            cache_generation: 0,
            #[cfg(debug_assertions)]
            in_atomic_operation: false,
        }
    }

    /// SECURITY FIX #20: Execute a closure atomically with proper cache management.
    /// This ensures read-modify-write operations are consistent.
    ///
    /// The closure receives a mutable reference to self and should perform
    /// all related state modifications. Cache is invalidated after the operation.
    ///
    /// Example:
    /// ```ignore
    /// state.execute_atomic(|s| {
    ///     let account = s.get_account(&id)?;
    ///     // modify account
    ///     s.set_account(id, modified_account)?;
    ///     Ok(())
    /// })?;
    /// ```
    pub fn execute_atomic<F, R>(&mut self, f: F) -> Result<R, StateError>
    where
        F: FnOnce(&mut Self) -> Result<R, StateError>,
    {
        #[cfg(debug_assertions)]
        {
            assert!(!self.in_atomic_operation, "Nested atomic operations not allowed");
            self.in_atomic_operation = true;
        }

        let result = f(self);

        #[cfg(debug_assertions)]
        {
            self.in_atomic_operation = false;
        }

        // Always invalidate cache after atomic operation to ensure consistency
        self.invalidate_cache();

        result
    }

    /// FIX: Invalidate the entire cache - call this when external state changes are possible
    pub fn invalidate_cache(&mut self) {
        self.account_cache.clear();
        self.cache_generation = self.cache_generation.wrapping_add(1);
    }

    /// FIX: Get current cache generation (useful for debugging/testing)
    pub fn cache_generation(&self) -> u64 {
        self.cache_generation
    }

    /// Récupère un compte
    pub fn get_account(&mut self, id: &AccountId) -> Result<Option<AccountInfo>, StateError> {
        // Vérifie le cache d'abord
        if let Some(info) = self.account_cache.get(id) {
            return Ok(Some(info.clone()));
        }

        // Sinon, lit depuis la DB
        let key = Self::account_key(id);
        if let Some(data) = self.db.get(&key)? {
            let info: AccountInfo =
                bincode::deserialize(&data).map_err(|e| StateError::DeserializationFailed(e.to_string()))?;

            // Met à jour le cache
            self.account_cache.insert(*id, info.clone());

            Ok(Some(info))
        } else {
            Ok(None)
        }
    }

    /// Met à jour un compte
    /// SECURITY FIX #6: Write-through cache - write to DB first, then update cache
    /// This ensures cache is never ahead of DB state in case of failures
    pub fn set_account(&mut self, id: AccountId, info: AccountInfo) -> Result<(), StateError> {
        let key = Self::account_key(&id);
        let value = bincode::serialize(&info).map_err(|e| StateError::SerializationFailed(e.to_string()))?;

        // SECURITY FIX #6: Write to DB FIRST, then update cache
        // If DB write fails, cache remains unchanged (safe)
        // If we crash after DB write but before cache update, cache will
        // be repopulated from DB on next read (safe)
        self.db.put(&key, &value)?;

        // Only update cache after successful DB write
        self.account_cache.insert(id, info);

        Ok(())
    }

    /// Supprime un compte
    /// SECURITY FIX #6: Write to DB first, then update cache
    pub fn delete_account(&mut self, id: &AccountId) -> Result<(), StateError> {
        let key = Self::account_key(id);

        // SECURITY FIX #6: Delete from DB FIRST, then remove from cache
        self.db.delete(&key)?;

        // Only remove from cache after successful DB delete
        self.account_cache.remove(id);

        Ok(())
    }

    /// Transfert de balance
    ///
    /// SECURITY FIX #34: Atomic transfer to prevent race conditions
    /// Uses execute_atomic to ensure all reads and writes are performed
    /// within a single atomic operation, preventing double-spend attacks
    /// in concurrent environments.
    pub fn transfer(&mut self, from: AccountId, to: AccountId, amount: Balance) -> Result<(), StateError> {
        self.execute_atomic(|state| {
            // Récupère les comptes (atomic read)
            let mut from_account = state
                .get_account(&from)?
                .ok_or(StateError::AccountNotFound(from))?;
            let mut to_account = state.get_account(&to)?.unwrap_or_else(AccountInfo::new);

            // Vérifie la balance
            if from_account.free < amount {
                return Err(StateError::InsufficientBalance {
                    account: from,
                    available: from_account.free,
                    required: amount,
                });
            }

            // Effectue le transfert
            from_account.free = from_account.free.saturating_sub(amount);
            to_account.free = to_account.free.saturating_add(amount);

            // Sauvegarde (atomic write)
            state.set_account(from, from_account)?;
            state.set_account(to, to_account)?;

            Ok(())
        })
    }

    /// Sauvegarde le hash d'un bloc
    pub fn set_block_hash(&self, number: BlockNumber, hash: Hash) -> Result<(), StateError> {
        let key = Self::block_hash_key(number);
        self.db.put(&key, hash.as_bytes())?;
        Ok(())
    }

    /// Récupère le hash d'un bloc
    pub fn get_block_hash(&self, number: BlockNumber) -> Result<Option<Hash>, StateError> {
        let key = Self::block_hash_key(number);
        if let Some(data) = self.db.get(&key)? {
            if data.len() == 32 {
                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&data);
                return Ok(Some(Hash::from(bytes)));
            }
        }
        Ok(None)
    }

    /// Définit le meilleur bloc (hauteur de la chaîne)
    pub fn set_best_block(&self, number: BlockNumber) -> Result<(), StateError> {
        let value = bincode::serialize(&number).map_err(|e| StateError::SerializationFailed(e.to_string()))?;
        self.db.put(KEY_BEST_BLOCK, &value)?;
        Ok(())
    }

    /// Get best block number
    pub fn get_best_block(&self) -> Result<Option<BlockNumber>, StateError> {
        if let Some(data) = self.db.get(KEY_BEST_BLOCK)? {
            let number: BlockNumber =
                bincode::deserialize(&data).map_err(|e| StateError::DeserializationFailed(e.to_string()))?;
            Ok(Some(number))
        } else {
            Ok(None)
        }
    }

    // ===== Block Storage =====

    /// Store a full block (by hash and by number)
    pub fn store_block(&self, block: &Block) -> Result<(), StateError> {
        let block_hash = block.hash();
        let block_number = block.header.number;

        let data = bincode::serialize(block)
            .map_err(|e| StateError::SerializationFailed(e.to_string()))?;

        // Store by hash
        let key_by_hash = Self::block_by_hash_key(&block_hash);
        self.db.put(&key_by_hash, &data)?;

        // Store by number
        let key_by_number = Self::block_by_number_key(block_number);
        self.db.put(&key_by_number, &data)?;

        // Also store hash -> number mapping
        self.set_block_hash(block_number, block_hash)?;

        Ok(())
    }

    /// Get block by hash
    pub fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, StateError> {
        let key = Self::block_by_hash_key(hash);
        if let Some(data) = self.db.get(&key)? {
            let block: Block = bincode::deserialize(&data)
                .map_err(|e| StateError::DeserializationFailed(e.to_string()))?;
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }

    /// Get block by number
    pub fn get_block_by_number(&self, number: BlockNumber) -> Result<Option<Block>, StateError> {
        let key = Self::block_by_number_key(number);
        if let Some(data) = self.db.get(&key)? {
            let block: Block = bincode::deserialize(&data)
                .map_err(|e| StateError::DeserializationFailed(e.to_string()))?;
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }

    /// Get blocks in a range (for sync)
    /// Returns up to `max_count` blocks starting from `from`
    pub fn get_blocks_range(&self, from: BlockNumber, max_count: u32) -> Result<Vec<Block>, StateError> {
        let mut blocks = Vec::new();
        let best = self.get_best_block()?.unwrap_or(0);

        // from..(from + max_count) gives exactly max_count blocks
        let end = (from + max_count as u64).min(best + 1);
        for number in from..end {
            if let Some(block) = self.get_block_by_number(number)? {
                blocks.push(block);
            } else {
                // Gap in blocks, stop here
                break;
            }
        }

        Ok(blocks)
    }

    /// Set genesis hash
    pub fn set_genesis_hash(&self, hash: Hash) -> Result<(), StateError> {
        self.db.put(KEY_GENESIS_HASH, hash.as_bytes())?;
        Ok(())
    }

    /// Get genesis hash
    pub fn get_genesis_hash(&self) -> Result<Option<Hash>, StateError> {
        if let Some(data) = self.db.get(KEY_GENESIS_HASH)? {
            if data.len() == 32 {
                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&data);
                return Ok(Some(Hash::from(bytes)));
            }
        }
        Ok(None)
    }

    // ===== Drift Tracker (SECURITY FIX #35) =====

    /// Store drift tracker state
    /// This is persisted to ensure drift tracking survives restarts and is inherited by forks
    pub fn set_drift_tracker(&self, tracker: &DriftTracker) -> Result<(), StateError> {
        let value = bincode::serialize(tracker)
            .map_err(|e| StateError::SerializationFailed(e.to_string()))?;
        self.db.put(KEY_DRIFT_TRACKER, &value)?;
        Ok(())
    }

    /// Get drift tracker state
    /// Returns None if not initialized (genesis case)
    pub fn get_drift_tracker(&self) -> Result<Option<DriftTracker>, StateError> {
        if let Some(data) = self.db.get(KEY_DRIFT_TRACKER)? {
            let tracker: DriftTracker = bincode::deserialize(&data)
                .map_err(|e| StateError::DeserializationFailed(e.to_string()))?;
            Ok(Some(tracker))
        } else {
            Ok(None)
        }
    }

    /// Initialize drift tracker with genesis timestamp
    /// Called once at genesis block creation
    pub fn init_drift_tracker(&self, genesis_timestamp: u64) -> Result<DriftTracker, StateError> {
        let tracker = DriftTracker::new(genesis_timestamp);
        self.set_drift_tracker(&tracker)?;
        Ok(tracker)
    }

    /// Validate block timestamp and update drift tracker atomically
    /// Returns the drift for this block on success
    pub fn validate_block_drift(
        &self,
        block: &Block,
        slot_duration_secs: u64,
    ) -> Result<i64, StateError> {
        // Get or create drift tracker
        let mut tracker = self.get_drift_tracker()?
            .ok_or_else(|| StateError::DriftTrackerNotInitialized)?;

        // Validate and update
        let drift = tracker.validate_and_update(
            block.header.number,
            block.header.timestamp,
            block.header.epoch,
            block.header.slot,
            slot_duration_secs,
        ).map_err(|e| StateError::DriftValidationFailed(e.to_string()))?;

        // Persist updated tracker
        self.set_drift_tracker(&tracker)?;

        Ok(drift)
    }

    /// Update drift tracker for synced historical blocks without strict validation
    /// Used during initial sync when we trust the network consensus
    /// This just updates the tracker state to track the chain's progress
    pub fn update_drift_tracker_for_sync(&self, block: &Block) -> Result<(), StateError> {
        let mut tracker = self.get_drift_tracker()?
            .ok_or_else(|| StateError::DriftTrackerNotInitialized)?;

        // Update tracker state without validation
        // For historical blocks, we trust they were validated when produced
        tracker.last_block = block.header.number;
        tracker.last_timestamp = block.header.timestamp;
        tracker.last_slot = block.header.slot;
        tracker.current_epoch = block.header.epoch;

        // Persist updated tracker
        self.set_drift_tracker(&tracker)?;

        Ok(())
    }

    /// Compute state root (Merkle root of all accounts) - SPEC v3.1 Phase 4
    ///
    /// SECURITY FIX #5 & #25: State root must be deterministic across all nodes.
    /// We use a BTreeMap to collect accounts, which guarantees sorted order
    /// by key (lexicographic byte order). This ensures the same state always
    /// produces the same root, regardless of database iteration order.
    ///
    /// INVARIANT: This function MUST produce identical output for identical state,
    /// regardless of the order in which accounts were added to the database.
    pub fn compute_state_root(&self, block_number: BlockNumber, chain_id: ChainId) -> StateRoot {
        // SECURITY FIX #25: Use BTreeMap instead of Vec + sort for guaranteed ordering
        // BTreeMap maintains keys in sorted order by default, which is more robust
        // than relying on sort_by after collection, especially if keys have
        // variable-length encoding that could affect comparison.
        use std::collections::BTreeMap;

        let mut account_entries: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

        for (key, value) in self.db.prefix_iterator(PREFIX_ACCOUNT) {
            account_entries.insert(key, value);
        }

        // Si aucun compte, retourne un state root vide
        if account_entries.is_empty() {
            return StateRoot::zero(chain_id);
        }

        // BTreeMap iterator is already sorted by key, so we just extract values
        // in the deterministic order guaranteed by BTreeMap
        let account_leaves: Vec<Vec<u8>> = account_entries
            .into_iter()
            .map(|(_, value)| value)
            .collect();

        // Construit le Merkle tree et calcule le root
        let tree = StateMerkleTree::new(account_leaves);
        let root_hash = tree.root();

        StateRoot::new(root_hash, block_number, chain_id)
    }

    /// Sauvegarde un state root pour un bloc donné - SPEC v3.1 Phase 4
    pub fn store_state_root(&self, block_number: BlockNumber, state_root: StateRoot) -> Result<(), StateError> {
        let key = Self::state_root_key(block_number);
        let value = bincode::serialize(&state_root)
            .map_err(|e| StateError::SerializationFailed(e.to_string()))?;
        self.db.put(&key, &value)?;
        Ok(())
    }

    /// Récupère le state root pour un bloc donné - SPEC v3.1 Phase 4
    pub fn get_state_root(&self, block_number: BlockNumber) -> Result<Option<StateRoot>, StateError> {
        let key = Self::state_root_key(block_number);
        if let Some(data) = self.db.get(&key)? {
            let state_root: StateRoot = bincode::deserialize(&data)
                .map_err(|e| StateError::DeserializationFailed(e.to_string()))?;
            Ok(Some(state_root))
        } else {
            Ok(None)
        }
    }

    /// Commit un batch de changements
    pub fn commit_batch(&mut self, ops: Vec<WriteOp>) -> Result<(), StateError> {
        self.db.batch_write(ops)?;
        // FIX: Increment generation and clear cache after commit
        // This ensures any cached data is invalidated
        self.invalidate_cache();
        Ok(())
    }

    // ===== Validator Credits Storage =====

    /// Get Validator Credits record
    pub fn get_vc_record(&self, validator_id: &AccountId) -> Result<Option<ValidatorCreditsRecord>, StateError> {
        let key = Self::vc_key(validator_id);
        if let Some(data) = self.db.get(&key)? {
            let record: ValidatorCreditsRecord = bincode::deserialize(&data)
                .map_err(|e| StateError::DeserializationFailed(e.to_string()))?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    /// Set Validator Credits record
    pub fn set_vc_record(&mut self, validator_id: AccountId, record: ValidatorCreditsRecord) -> Result<(), StateError> {
        let key = Self::vc_key(&validator_id);
        let value = bincode::serialize(&record)
            .map_err(|e| StateError::SerializationFailed(e.to_string()))?;
        self.db.put(&key, &value)?;
        Ok(())
    }

    /// Delete Validator Credits record
    pub fn delete_vc_record(&mut self, validator_id: &AccountId) -> Result<(), StateError> {
        let key = Self::vc_key(validator_id);
        self.db.delete(&key)?;
        Ok(())
    }

    /// Get total VC for a validator
    pub fn get_total_vc(&self, validator_id: &AccountId) -> Result<u64, StateError> {
        if let Some(record) = self.get_vc_record(validator_id)? {
            Ok(record.total_vc())
        } else {
            Ok(0)
        }
    }

    /// Initialize VC for a bootstrap validator with minimum required VC
    /// Bootstrap validators need BOOTSTRAP_MIN_VC_REQUIREMENT (100) to be eligible for VRF selection
    /// This is called when an early validator is approved during bootstrap era
    pub fn initialize_bootstrap_vc(
        &mut self,
        validator_id: AccountId,
        block_number: BlockNumber,
        current_epoch: EpochNumber,
    ) -> Result<(), StateError> {
        // Create a new VC record with bootstrap credits
        let mut record = ValidatorCreditsRecord::new(block_number, current_epoch);
        // Grant bootstrap VC (100) as uptime credits so they can be selected via VRF
        // This matches BOOTSTRAP_MIN_VC_REQUIREMENT in vrf_selection.rs
        record.uptime_credits = 100;
        self.set_vc_record(validator_id, record)
    }

    // ===== Unbonding Storage =====

    /// Get unbonding info for an account
    pub fn get_unbonding_info(&self, account_id: &AccountId) -> Result<Option<UnbondingInfo>, StateError> {
        let key = Self::unbonding_key(account_id);
        if let Some(data) = self.db.get(&key)? {
            let info: UnbondingInfo = bincode::deserialize(&data)
                .map_err(|e| StateError::DeserializationFailed(e.to_string()))?;
            Ok(Some(info))
        } else {
            Ok(None)
        }
    }

    /// Set unbonding info for an account
    pub fn set_unbonding_info(&mut self, account_id: AccountId, info: UnbondingInfo) -> Result<(), StateError> {
        let key = Self::unbonding_key(&account_id);
        let value = bincode::serialize(&info)
            .map_err(|e| StateError::SerializationFailed(e.to_string()))?;
        self.db.put(&key, &value)?;
        Ok(())
    }

    /// Add an unbonding request for an account
    pub fn add_unbonding_request(
        &mut self,
        account_id: AccountId,
        amount: Balance,
        current_block: BlockNumber,
        unbonding_period: BlockNumber,
    ) -> Result<(), StateError> {
        let mut info = self.get_unbonding_info(&account_id)?.unwrap_or_default();

        info.requests.push(UnbondingRequest {
            amount,
            unbonding_started: current_block,
            release_block: current_block.saturating_add(unbonding_period),
        });

        self.set_unbonding_info(account_id, info)
    }

    /// Get total amount currently unbonding (not yet withdrawable)
    pub fn get_total_unbonding(&self, account_id: &AccountId) -> Result<Balance, StateError> {
        if let Some(info) = self.get_unbonding_info(account_id)? {
            Ok(info.requests.iter().map(|r| r.amount).sum())
        } else {
            Ok(0)
        }
    }

    /// Withdraw all matured unbonding requests, returns total withdrawn amount
    pub fn withdraw_matured_unbonding(
        &mut self,
        account_id: &AccountId,
        current_block: BlockNumber,
    ) -> Result<Balance, StateError> {
        let mut info = match self.get_unbonding_info(account_id)? {
            Some(info) => info,
            None => return Ok(0),
        };

        let mut withdrawn: Balance = 0;

        // Separate matured from pending requests
        let (matured, pending): (Vec<_>, Vec<_>) = info.requests
            .into_iter()
            .partition(|r| current_block >= r.release_block);

        // Sum up matured amounts
        for request in matured {
            withdrawn = withdrawn.saturating_add(request.amount);
        }

        // Update with remaining pending requests
        info.requests = pending;

        if info.requests.is_empty() {
            // Delete the record if no more pending requests
            let key = Self::unbonding_key(account_id);
            self.db.delete(&key)?;
        } else {
            self.set_unbonding_info(*account_id, info)?;
        }

        Ok(withdrawn)
    }

    // ===== Clock Health Storage (SECURITY FIX #36) =====

    /// Get clock health record for a validator
    /// Returns None if no record exists (never excluded)
    pub fn get_clock_record(&self, validator_id: &AccountId) -> Result<Option<ValidatorClockRecord>, StateError> {
        let key = Self::clock_record_key(validator_id);
        if let Some(data) = self.db.get(&key)? {
            let record: ValidatorClockRecord = bincode::deserialize(&data)
                .map_err(|e| StateError::DeserializationFailed(e.to_string()))?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    /// Set clock health record for a validator
    pub fn set_clock_record(&self, validator_id: &AccountId, record: &ValidatorClockRecord) -> Result<(), StateError> {
        let key = Self::clock_record_key(validator_id);
        let value = bincode::serialize(record)
            .map_err(|e| StateError::SerializationFailed(e.to_string()))?;
        self.db.put(&key, &value)?;
        Ok(())
    }

    /// Record a clock sync failure for a validator
    /// This increments the failure counter and updates last_exclusion_epoch
    pub fn record_clock_failure(&self, validator_id: &AccountId, epoch: EpochNumber) -> Result<(), StateError> {
        let mut record = self.get_clock_record(validator_id)?
            .unwrap_or_else(ValidatorClockRecord::new);

        record.record_failure(epoch);
        self.set_clock_record(validator_id, &record)
    }

    /// Record a missed slot due to clock exclusion
    pub fn record_clock_missed_slot(&self, validator_id: &AccountId) -> Result<(), StateError> {
        let mut record = self.get_clock_record(validator_id)?
            .unwrap_or_else(ValidatorClockRecord::new);

        record.record_missed_slot();
        self.set_clock_record(validator_id, &record)
    }

    /// Get total clock sync failures for a validator
    pub fn get_clock_failures(&self, validator_id: &AccountId) -> Result<u32, StateError> {
        Ok(self.get_clock_record(validator_id)?
            .map(|r| r.clock_sync_failures)
            .unwrap_or(0))
    }

    /// Get VC penalty from clock failures
    pub fn get_clock_vc_penalty(&self, validator_id: &AccountId) -> Result<u64, StateError> {
        Ok(self.get_clock_record(validator_id)?
            .map(|r| r.vc_penalty())
            .unwrap_or(0))
    }

    // Fonctions utilitaires pour les clés
    fn account_key(id: &AccountId) -> Vec<u8> {
        let mut key = PREFIX_ACCOUNT.to_vec();
        key.extend_from_slice(id.as_bytes());
        key
    }

    fn block_hash_key(number: BlockNumber) -> Vec<u8> {
        let mut key = PREFIX_BLOCK_HASH.to_vec();
        key.extend_from_slice(&number.to_le_bytes());
        key
    }

    fn state_root_key(number: BlockNumber) -> Vec<u8> {
        let mut key = PREFIX_STATE_ROOT.to_vec();
        key.extend_from_slice(&number.to_le_bytes());
        key
    }

    fn vc_key(id: &AccountId) -> Vec<u8> {
        let mut key = PREFIX_VC.to_vec();
        key.extend_from_slice(id.as_bytes());
        key
    }

    fn unbonding_key(id: &AccountId) -> Vec<u8> {
        let mut key = PREFIX_UNBONDING.to_vec();
        key.extend_from_slice(id.as_bytes());
        key
    }

    fn clock_record_key(id: &AccountId) -> Vec<u8> {
        let mut key = PREFIX_CLOCK_RECORD.to_vec();
        key.extend_from_slice(id.as_bytes());
        key
    }

    fn block_by_hash_key(hash: &Hash) -> Vec<u8> {
        let mut key = PREFIX_BLOCK_BY_HASH.to_vec();
        key.extend_from_slice(hash.as_bytes());
        key
    }

    fn block_by_number_key(number: BlockNumber) -> Vec<u8> {
        let mut key = PREFIX_BLOCK_BY_NUMBER.to_vec();
        key.extend_from_slice(&number.to_le_bytes());
        key
    }
}

/// Erreurs d'état
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("Compte non trouvé: {0:?}")]
    AccountNotFound(AccountId),

    #[error("Balance insuffisante pour {account:?}: disponible={available}, requis={required}")]
    InsufficientBalance {
        account: AccountId,
        available: Balance,
        required: Balance,
    },

    #[error("Erreur de base de données: {0}")]
    DatabaseError(#[from] DatabaseError),

    #[error("Échec de sérialisation: {0}")]
    SerializationFailed(String),

    #[error("Échec de désérialisation: {0}")]
    DeserializationFailed(String),

    #[error("Drift tracker not initialized - call init_drift_tracker at genesis")]
    DriftTrackerNotInitialized,

    #[error("Drift validation failed: {0}")]
    DriftValidationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use tempfile::TempDir;

    #[test]
    fn test_account_operations() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();
        let mut state = StateBackend::new(db);

        let account_id = AccountId::from_bytes([1; 32]);
        let mut account_info = AccountInfo::new();
        account_info.free = 1000;

        // Set account
        state.set_account(account_id, account_info.clone()).unwrap();

        // Get account
        let retrieved = state.get_account(&account_id).unwrap();
        assert_eq!(retrieved.unwrap().free, 1000);
    }

    #[test]
    fn test_transfer() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();
        let mut state = StateBackend::new(db);

        let alice = AccountId::from_bytes([1; 32]);
        let bob = AccountId::from_bytes([2; 32]);

        // Alice a 1000 tokens
        let mut alice_info = AccountInfo::new();
        alice_info.free = 1000;
        state.set_account(alice, alice_info).unwrap();

        // Transfert de 300 tokens à Bob
        state.transfer(alice, bob, 300).unwrap();

        // Vérifications
        let alice_after = state.get_account(&alice).unwrap().unwrap();
        let bob_after = state.get_account(&bob).unwrap().unwrap();

        assert_eq!(alice_after.free, 700);
        assert_eq!(bob_after.free, 300);
    }

    #[test]
    fn test_block_tracking() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();
        let state = StateBackend::new(db);

        let hash = Hash::hash(b"block1");

        state.set_block_hash(1, hash).unwrap();
        state.set_best_block(1).unwrap();

        let retrieved_hash = state.get_block_hash(1).unwrap();
        let best_block = state.get_best_block().unwrap();

        assert_eq!(retrieved_hash, Some(hash));
        assert_eq!(best_block, Some(1));
    }

    #[test]
    fn test_state_root_computation_empty() {
        use crate::types::ChainId;

        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();
        let state = StateBackend::new(db);

        // Empty state should produce zero state root
        let state_root = state.compute_state_root(0, ChainId(1));
        assert_eq!(state_root.root, Hash::ZERO);
        assert_eq!(state_root.block_number, 0);
        assert_eq!(state_root.chain_id, ChainId(1));
    }

    #[test]
    fn test_state_root_computation_with_accounts() {
        use crate::types::ChainId;

        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();
        let mut state = StateBackend::new(db);

        // Add some accounts
        let alice = AccountId::from_bytes([1; 32]);
        let bob = AccountId::from_bytes([2; 32]);

        let mut alice_info = AccountInfo::new();
        alice_info.free = 1000;
        state.set_account(alice, alice_info).unwrap();

        let mut bob_info = AccountInfo::new();
        bob_info.free = 500;
        state.set_account(bob, bob_info).unwrap();

        // Compute state root
        let state_root = state.compute_state_root(100, ChainId(1));

        // Should not be zero (we have accounts)
        assert_ne!(state_root.root, Hash::ZERO);
        assert_eq!(state_root.block_number, 100);
        assert_eq!(state_root.chain_id, ChainId(1));
    }

    #[test]
    fn test_state_root_deterministic() {
        use crate::types::ChainId;

        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();
        let db1 = Database::open(temp_dir1.path()).unwrap();
        let db2 = Database::open(temp_dir2.path()).unwrap();
        let mut state1 = StateBackend::new(db1);
        let mut state2 = StateBackend::new(db2);

        // Add same accounts to both states
        let alice = AccountId::from_bytes([1; 32]);
        let mut alice_info = AccountInfo::new();
        alice_info.free = 1000;

        state1.set_account(alice, alice_info.clone()).unwrap();
        state2.set_account(alice, alice_info).unwrap();

        // Compute state roots
        let root1 = state1.compute_state_root(1, ChainId(1));
        let root2 = state2.compute_state_root(1, ChainId(1));

        // Should be identical
        assert_eq!(root1.root, root2.root);
    }

    #[test]
    fn test_state_root_storage_and_retrieval() {
        use crate::types::ChainId;

        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();
        let mut state = StateBackend::new(db);

        // Add an account and compute state root
        let alice = AccountId::from_bytes([1; 32]);
        let mut alice_info = AccountInfo::new();
        alice_info.free = 1000;
        state.set_account(alice, alice_info).unwrap();

        let state_root = state.compute_state_root(100, ChainId(1));

        // Store it
        state.store_state_root(100, state_root).unwrap();

        // Retrieve it
        let retrieved = state.get_state_root(100).unwrap();
        assert_eq!(retrieved, Some(state_root));

        // Non-existent block should return None
        let non_existent = state.get_state_root(999).unwrap();
        assert_eq!(non_existent, None);
    }

    #[test]
    fn test_state_root_changes_with_state() {
        use crate::types::ChainId;

        let temp_dir = TempDir::new().unwrap();
        let db = Database::open(temp_dir.path()).unwrap();
        let mut state = StateBackend::new(db);

        let alice = AccountId::from_bytes([1; 32]);

        // Initial state root (empty)
        let root1 = state.compute_state_root(1, ChainId(1));

        // Add account
        let mut alice_info = AccountInfo::new();
        alice_info.free = 1000;
        state.set_account(alice, alice_info.clone()).unwrap();

        let root2 = state.compute_state_root(2, ChainId(1));

        // Modify account
        alice_info.free = 2000;
        state.set_account(alice, alice_info).unwrap();

        let root3 = state.compute_state_root(3, ChainId(1));

        // All roots should be different
        assert_ne!(root1.root, root2.root);
        assert_ne!(root2.root, root3.root);
        assert_ne!(root1.root, root3.root);
    }
}
