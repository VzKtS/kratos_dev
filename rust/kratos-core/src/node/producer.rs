// Block Producer - Production and validation of blocks
//
// Features:
// - VRF-based slot leader selection
// - Transaction execution with state updates
// - Double-signing protection (persistent)
// - Block validation before import
// - Finality tracking
// - Integration with mempool
// - Dynamic block rewards based on network metrics
// - Fee distribution: 60% validator, 30% burn, 10% treasury
// - VC bonus for block producers

use crate::consensus::economics::{FeeDistribution, InflationCalculator, InflationConfig, NetworkMetrics, BootstrapConfig, get_bootstrap_config};
use crate::consensus::epoch::SLOT_DURATION_SECS;
use crate::consensus::validator::{ValidatorSet, UNBONDING_PERIOD};
use crate::consensus::vrf_selection::VRFSelector;
use crate::node::mempool::TransactionPool;
use crate::storage::state::StateBackend;
use crate::storage::Database;
use crate::types::*;
use crate::types::primitives::KRAT;
// SECURITY FIX #24: Import domain separation constants
use crate::types::signature::{domain_separate, DOMAIN_BLOCK_HEADER};
use ed25519_dalek::{Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn, error};

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Base block reward per block (in base units) - used as fallback
/// Dynamic rewards are calculated from network metrics when available
pub const BLOCK_REWARD: Balance = 10 * KRAT; // 10 KRAT per block (fallback)

/// Treasury account for fee distribution (10%)
/// This is a well-known address that can be updated via governance
pub const TREASURY_ACCOUNT: [u8; 32] = [
    0xFE, 0xED, 0xC0, 0xDE, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x54, 0x52, 0x45, 0x41, // "TREA"
];

/// Slots per epoch (for dynamic reward calculation)
/// 1 epoch = 1 hour = 600 blocks at 6s/slot
const SLOTS_PER_EPOCH: u64 = 600;

/// Epochs per year (for annual emission to per-block conversion)
/// 365 days × 24 hours = 8,760 epochs/year
const EPOCHS_PER_YEAR: u64 = 8_760;

/// Block production configuration
#[derive(Debug, Clone)]
pub struct ProducerConfig {
    /// Maximum transactions per block
    pub max_transactions_per_block: usize,

    /// Maximum block size in bytes
    pub max_block_size: usize,

    /// Enable transaction execution
    pub execute_transactions: bool,

    /// Minimum fee for transaction inclusion
    pub min_inclusion_fee: Balance,

    /// Block reward for producer (fallback when dynamic rewards unavailable)
    pub block_reward: Balance,

    /// Enable dynamic block rewards based on network metrics
    pub use_dynamic_rewards: bool,

    /// Fee distribution configuration (60/30/10)
    pub fee_distribution: FeeDistribution,

    /// Enable VC bonus for block producers
    pub enable_vc_bonus: bool,

    /// Treasury account for fee distribution
    pub treasury_account: AccountId,
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            max_transactions_per_block: 1000,
            max_block_size: 5 * 1024 * 1024, // 5 MB
            execute_transactions: true,
            min_inclusion_fee: 1_000,
            block_reward: BLOCK_REWARD,
            use_dynamic_rewards: true,
            fee_distribution: FeeDistribution::default_distribution(),
            enable_vc_bonus: true,
            treasury_account: AccountId::from_bytes(TREASURY_ACCOUNT),
        }
    }
}

impl ProducerConfig {
    /// Create config with dynamic rewards disabled (for testing)
    pub fn with_fixed_rewards() -> Self {
        Self {
            use_dynamic_rewards: false,
            enable_vc_bonus: false,
            ..Default::default()
        }
    }
}

// =============================================================================
// TRANSACTION EXECUTION
// =============================================================================

/// Transaction execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Transaction hash
    pub tx_hash: Hash,
    /// Success or failure
    pub success: bool,
    /// Fee paid
    pub fee_paid: Balance,
    /// Error message if failed
    pub error: Option<String>,
}

/// Transaction executor
pub struct TransactionExecutor;

impl TransactionExecutor {
    /// Execute a single transaction against state
    /// current_block is required for unbonding period tracking
    pub fn execute(
        state: &mut StateBackend,
        tx: &SignedTransaction,
        current_block: BlockNumber,
    ) -> ExecutionResult {
        let tx_hash = tx.hash();
        let fee = tx.transaction.call.base_fee();
        let sender = tx.transaction.sender;

        // Verify signature (defense in depth - mempool/validator also verify)
        if !tx.verify() {
            return ExecutionResult {
                tx_hash,
                success: false,
                fee_paid: 0,
                error: Some("Invalid transaction signature".to_string()),
            };
        }

        // Get sender account
        let mut sender_account = match state.get_account(&sender) {
            Ok(Some(acc)) => acc,
            Ok(None) => {
                return ExecutionResult {
                    tx_hash,
                    success: false,
                    fee_paid: 0,
                    error: Some("Sender account not found".to_string()),
                };
            }
            Err(e) => {
                return ExecutionResult {
                    tx_hash,
                    success: false,
                    fee_paid: 0,
                    error: Some(format!("State error: {:?}", e)),
                };
            }
        };

        // Check nonce
        if tx.transaction.nonce != sender_account.nonce {
            return ExecutionResult {
                tx_hash,
                success: false,
                fee_paid: 0,
                error: Some(format!(
                    "Invalid nonce: expected {}, got {}",
                    sender_account.nonce, tx.transaction.nonce
                )),
            };
        }

        // Check balance for fee
        if sender_account.free < fee {
            return ExecutionResult {
                tx_hash,
                success: false,
                fee_paid: 0,
                error: Some(format!(
                    "Insufficient balance for fee: need {}, have {}",
                    fee, sender_account.free
                )),
            };
        }

        // Execute based on transaction type
        let exec_result = match &tx.transaction.call {
            TransactionCall::Transfer { to, amount } => {
                Self::execute_transfer(state, &sender, *to, *amount, &mut sender_account)
            }
            TransactionCall::Stake { amount } => {
                Self::execute_stake(state, &sender, *amount, &mut sender_account)
            }
            TransactionCall::Unstake { amount } => {
                Self::execute_unstake(state, &sender, *amount, &mut sender_account, current_block)
            }
            TransactionCall::WithdrawUnbonded => {
                Self::execute_withdraw_unbonded(state, &sender, &mut sender_account, current_block)
            }
            TransactionCall::RegisterValidator { stake } => {
                // Simplified: just reserve the stake
                Self::execute_stake(state, &sender, *stake, &mut sender_account)
            }
            TransactionCall::UnregisterValidator => {
                // Simplified: start unbonding
                Ok(())
            }
            TransactionCall::CreateSidechain { deposit, .. } => {
                Self::execute_reserve(state, &sender, *deposit, &mut sender_account)
            }
            TransactionCall::ExitSidechain { .. } => {
                // Simplified: just succeed
                Ok(())
            }
            TransactionCall::SignalFork { .. } => {
                // Just deduct fee, signal is recorded elsewhere
                Ok(())
            }
            // Early validator voting - execution handled in node service
            // where ValidatorSet is available. Here we just validate basic checks.
            TransactionCall::ProposeEarlyValidator { .. } => {
                // Actual validation done in node service with access to ValidatorSet
                // Fee will be deducted if transaction succeeds
                Ok(())
            }
            TransactionCall::VoteEarlyValidator { .. } => {
                // Actual validation done in node service with access to ValidatorSet
                // Fee will be deducted if transaction succeeds
                Ok(())
            }
        };

        match exec_result {
            Ok(()) => {
                // Deduct fee and increment nonce
                sender_account.free = sender_account.free.saturating_sub(fee);
                sender_account.nonce += 1;
                sender_account.last_modified = tx_hash;

                // Save updated sender account
                if let Err(e) = state.set_account(sender, sender_account) {
                    return ExecutionResult {
                        tx_hash,
                        success: false,
                        fee_paid: 0,
                        error: Some(format!("Failed to save sender: {:?}", e)),
                    };
                }

                ExecutionResult {
                    tx_hash,
                    success: true,
                    fee_paid: fee,
                    error: None,
                }
            }
            Err(e) => ExecutionResult {
                tx_hash,
                success: false,
                fee_paid: 0,
                error: Some(e),
            },
        }
    }

    fn execute_transfer(
        state: &mut StateBackend,
        sender: &AccountId,
        to: AccountId,
        amount: Balance,
        sender_account: &mut AccountInfo,
    ) -> Result<(), String> {
        // Check balance
        let fee = TransactionCall::Transfer { to, amount }.base_fee();
        let total_needed = amount.saturating_add(fee);

        if sender_account.free < total_needed {
            return Err(format!(
                "Insufficient balance: need {}, have {}",
                total_needed, sender_account.free
            ));
        }

        // Deduct from sender
        sender_account.free = sender_account.free.saturating_sub(amount);

        // Credit to recipient
        let mut recipient = state
            .get_account(&to)
            .map_err(|e| format!("State error: {:?}", e))?
            .unwrap_or(AccountInfo {
                nonce: 0,
                free: 0,
                reserved: 0,
                last_modified: Hash::ZERO,
            });

        recipient.free = recipient.free.saturating_add(amount);

        state
            .set_account(to, recipient)
            .map_err(|e| format!("Failed to save recipient: {:?}", e))?;

        Ok(())
    }

    fn execute_stake(
        _state: &mut StateBackend,
        _sender: &AccountId,
        amount: Balance,
        sender_account: &mut AccountInfo,
    ) -> Result<(), String> {
        let fee = TransactionCall::Stake { amount }.base_fee();
        let total_needed = amount.saturating_add(fee);

        if sender_account.free < total_needed {
            return Err(format!(
                "Insufficient balance for stake: need {}, have {}",
                total_needed, sender_account.free
            ));
        }

        // Move from free to reserved
        sender_account.free = sender_account.free.saturating_sub(amount);
        sender_account.reserved = sender_account.reserved.saturating_add(amount);

        Ok(())
    }

    fn execute_unstake(
        state: &mut StateBackend,
        sender: &AccountId,
        amount: Balance,
        sender_account: &mut AccountInfo,
        current_block: BlockNumber,
    ) -> Result<(), String> {
        if sender_account.reserved < amount {
            return Err(format!(
                "Insufficient staked balance: need {}, have {}",
                amount, sender_account.reserved
            ));
        }

        // Move from reserved to unbonding state (not free!)
        // The funds will be locked for UNBONDING_PERIOD blocks
        sender_account.reserved = sender_account.reserved.saturating_sub(amount);

        // Record the unbonding request with release time
        state.add_unbonding_request(*sender, amount, current_block, UNBONDING_PERIOD)
            .map_err(|e| format!("Failed to record unbonding: {:?}", e))?;

        Ok(())
    }

    fn execute_withdraw_unbonded(
        state: &mut StateBackend,
        sender: &AccountId,
        sender_account: &mut AccountInfo,
        current_block: BlockNumber,
    ) -> Result<(), String> {
        // Only withdraw funds that have completed the unbonding period
        let withdrawn = state.withdraw_matured_unbonding(sender, current_block)
            .map_err(|e| format!("Failed to withdraw unbonded: {:?}", e))?;

        if withdrawn == 0 {
            return Err("No matured unbonding funds to withdraw".to_string());
        }

        // Add withdrawn amount to free balance
        sender_account.free = sender_account.free.saturating_add(withdrawn);

        Ok(())
    }

    fn execute_reserve(
        _state: &mut StateBackend,
        _sender: &AccountId,
        amount: Balance,
        sender_account: &mut AccountInfo,
    ) -> Result<(), String> {
        let fee = TransactionCall::CreateSidechain {
            metadata: SidechainMetadata {
                name: None,
                description: None,
                parent_chain: None,
            },
            deposit: amount,
        }
        .base_fee();

        let total_needed = amount.saturating_add(fee);

        if sender_account.free < total_needed {
            return Err(format!(
                "Insufficient balance for deposit: need {}, have {}",
                total_needed, sender_account.free
            ));
        }

        // Reserve the deposit
        sender_account.free = sender_account.free.saturating_sub(amount);
        sender_account.reserved = sender_account.reserved.saturating_add(amount);

        Ok(())
    }
}

// =============================================================================
// BLOCK VALIDATION
// =============================================================================

/// Maximum allowed clock drift into the future (in seconds)
/// Blocks with timestamps more than this far ahead of current time will be rejected.
/// SECURITY FIX #23: Reduced from 30 to 10 seconds to limit timestamp manipulation.
/// Over 100 blocks, 30s drift allows ~50 minutes of cumulative manipulation.
/// With 10s, this is reduced to ~17 minutes.
pub const MAX_FUTURE_DRIFT_SECS: u64 = 10;

/// Minimum time between consecutive blocks (in seconds)
/// SECURITY FIX #23: Increased from SLOT_DURATION_SECS/2 to SLOT_DURATION_SECS - 1
/// This tightens the window for timestamp manipulation while still allowing
/// minor clock variations. With 6s slots, this means minimum 5s between blocks.
pub const MIN_BLOCK_INTERVAL_SECS: u64 = SLOT_DURATION_SECS.saturating_sub(1);

/// Block validator
pub struct BlockValidator;

impl BlockValidator {
    /// Validate a block before import
    pub fn validate(
        block: &Block,
        parent: &Block,
        validator_set: &ValidatorSet,
    ) -> Result<(), ValidationError> {
        // 1. Check block number is sequential
        if block.header.number != parent.header.number + 1 {
            return Err(ValidationError::InvalidBlockNumber {
                expected: parent.header.number + 1,
                got: block.header.number,
            });
        }

        // 2. Check parent hash
        if block.header.parent_hash != parent.hash() {
            return Err(ValidationError::InvalidParentHash);
        }

        // 3. Validate timestamp (comprehensive checks)
        Self::validate_timestamp(block, parent)?;

        // 4. Check slot is after parent slot (within same epoch or next)
        if block.header.epoch < parent.header.epoch {
            return Err(ValidationError::InvalidEpoch);
        }
        if block.header.epoch == parent.header.epoch && block.header.slot <= parent.header.slot {
            return Err(ValidationError::InvalidSlot);
        }

        // 5. Verify author is a valid validator
        if !validator_set.is_active(&block.header.author) {
            return Err(ValidationError::InvalidAuthor);
        }

        // 6. Verify block signature
        Self::verify_signature(block)?;

        // 7. Verify transactions root
        let computed_root = Self::compute_transactions_root(&block.body.transactions);
        if block.header.transactions_root != computed_root {
            return Err(ValidationError::InvalidTransactionsRoot);
        }

        // 8. Check all transaction signatures
        for (i, tx) in block.body.transactions.iter().enumerate() {
            if !tx.verify() {
                return Err(ValidationError::InvalidTransactionSignature(i));
            }
        }

        Ok(())
    }

    /// Comprehensive timestamp validation
    ///
    /// Validates:
    /// 1. Timestamp is strictly after parent (no time travel)
    /// 2. Timestamp is not too far in the future (prevents future block attacks)
    /// 3. Minimum time between blocks is respected (prevents rapid block spam)
    /// 4. Timestamp is consistent with the declared slot
    fn validate_timestamp(block: &Block, parent: &Block) -> Result<(), ValidationError> {
        let block_ts = block.header.timestamp;
        let parent_ts = parent.header.timestamp;

        // 1. Timestamp must be strictly after parent
        if block_ts <= parent_ts {
            return Err(ValidationError::TimestampNotAfterParent {
                block_ts,
                parent_ts,
            });
        }

        // 2. Timestamp must not be too far in the future
        // This prevents attackers from creating blocks with timestamps far ahead,
        // which could be used to manipulate time-dependent logic
        let current_ts = chrono::Utc::now().timestamp() as u64;
        let max_allowed_ts = current_ts.saturating_add(MAX_FUTURE_DRIFT_SECS);

        if block_ts > max_allowed_ts {
            return Err(ValidationError::TimestampTooFarInFuture {
                block_ts,
                current_ts,
                max_drift: MAX_FUTURE_DRIFT_SECS,
            });
        }

        // 3. Minimum time between blocks
        // Prevents rapid block production that could overwhelm the network
        let interval = block_ts.saturating_sub(parent_ts);
        if interval < MIN_BLOCK_INTERVAL_SECS {
            return Err(ValidationError::TimestampTooCloseToParent {
                interval,
                min_required: MIN_BLOCK_INTERVAL_SECS,
            });
        }

        // 4. Timestamp-slot consistency using INCREMENTAL DRIFT model
        //
        // FIX: Use interval-based validation aligned with DriftTracker.
        // The slot field contains ABSOLUTE slots since genesis, not relative to epoch.
        // We validate that the time INTERVAL matches the slot INTERVAL.
        //
        // INCREMENTAL MODEL:
        //   slots_elapsed = block.slot - parent.slot (absolute slots)
        //   expected_interval = slots_elapsed × SLOT_DURATION_SECS
        //   actual_interval = block_ts - parent_ts
        //   drift = actual_interval - expected_interval
        //
        // We allow drift within ±SLOT_DURATION_SECS for clock skew.
        let slots_elapsed = block.header.slot.saturating_sub(parent.header.slot);
        let expected_interval = slots_elapsed.saturating_mul(SLOT_DURATION_SECS);
        let actual_interval = block_ts.saturating_sub(parent_ts);

        // Calculate drift (signed)
        let drift = (actual_interval as i64).saturating_sub(expected_interval as i64);

        // Allow drift within ±SLOT_DURATION_SECS
        if drift.abs() > SLOT_DURATION_SECS as i64 {
            return Err(ValidationError::TimestampSlotMismatch {
                expected_ts: parent_ts.saturating_add(expected_interval),
                actual_ts: block_ts,
                slot: block.header.slot,
            });
        }

        Ok(())
    }

    /// Validate block without parent (for genesis or partial validation)
    pub fn validate_standalone(block: &Block) -> Result<(), ValidationError> {
        // Verify signature
        Self::verify_signature(block)?;

        // Verify transactions root
        let computed_root = Self::compute_transactions_root(&block.body.transactions);
        if block.header.transactions_root != computed_root {
            return Err(ValidationError::InvalidTransactionsRoot);
        }

        Ok(())
    }

    fn verify_signature(block: &Block) -> Result<(), ValidationError> {
        let author_bytes = block.header.author.as_bytes();

        // Try to create verifying key from author
        let verifying_key = VerifyingKey::from_bytes(author_bytes)
            .map_err(|_| ValidationError::InvalidAuthorKey)?;

        // Get header hash (without signature)
        let header_hash = block.header.hash();

        // SECURITY FIX #24: Apply domain separation for verification
        // This must match the domain used when signing in produce_block()
        let message = domain_separate(DOMAIN_BLOCK_HEADER, header_hash.as_bytes());

        // Create signature from bytes
        let signature = ed25519_dalek::Signature::from_bytes(&block.header.signature.0);

        // Verify with domain-separated message
        verifying_key
            .verify(&message, &signature)
            .map_err(|_| ValidationError::InvalidBlockSignature)?;

        Ok(())
    }

    fn compute_transactions_root(transactions: &[SignedTransaction]) -> Hash {
        if transactions.is_empty() {
            return Hash::ZERO;
        }

        let mut data = Vec::new();
        for tx in transactions {
            let hash = tx.hash();
            data.extend_from_slice(hash.as_bytes());
        }

        Hash::hash(&data)
    }
}

/// Block validation errors
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Invalid block number: expected {expected}, got {got}")]
    InvalidBlockNumber { expected: BlockNumber, got: BlockNumber },

    #[error("Invalid parent hash")]
    InvalidParentHash,

    #[error("Timestamp not after parent: block={block_ts}, parent={parent_ts}")]
    TimestampNotAfterParent { block_ts: u64, parent_ts: u64 },

    #[error("Timestamp too far in future: block={block_ts}, now={current_ts}, max_drift={max_drift}s")]
    TimestampTooFarInFuture { block_ts: u64, current_ts: u64, max_drift: u64 },

    #[error("Timestamp too close to parent: interval={interval}s, min_required={min_required}s")]
    TimestampTooCloseToParent { interval: u64, min_required: u64 },

    #[error("Timestamp inconsistent with slot: expected ~{expected_ts} for slot {slot}, got {actual_ts}")]
    TimestampSlotMismatch { expected_ts: u64, actual_ts: u64, slot: SlotNumber },

    #[error("Invalid epoch")]
    InvalidEpoch,

    #[error("Invalid slot")]
    InvalidSlot,

    #[error("Invalid author: not an active validator")]
    InvalidAuthor,

    #[error("Invalid author public key")]
    InvalidAuthorKey,

    #[error("Invalid block signature")]
    InvalidBlockSignature,

    #[error("Invalid transactions root")]
    InvalidTransactionsRoot,

    #[error("Invalid transaction signature at index {0}")]
    InvalidTransactionSignature(usize),

    #[error("State error: {0}")]
    StateError(String),
}

// =============================================================================
// FINALITY TRACKER
// =============================================================================

/// Tracks block finality using BTreeMap for O(log n) insertion instead of O(n log n) sort
#[derive(Debug, Clone)]
pub struct FinalityTracker {
    /// Last finalized block number
    finalized_block: BlockNumber,

    /// Last finalized block hash
    finalized_hash: Hash,

    /// Blocks waiting for finality (BTreeMap maintains sorted order automatically)
    pending_finality: BTreeMap<BlockNumber, Hash>,

    /// Number of confirmations required
    confirmations_required: u32,
}

impl FinalityTracker {
    pub fn new(genesis_hash: Hash, confirmations: u32) -> Self {
        Self {
            finalized_block: 0,
            finalized_hash: genesis_hash,
            pending_finality: BTreeMap::new(),
            confirmations_required: confirmations,
        }
    }

    /// Add a new block to track - O(log n) insertion
    pub fn add_block(&mut self, number: BlockNumber, hash: Hash) {
        self.pending_finality.insert(number, hash);
        self.update_finality();
    }

    fn update_finality(&mut self) {
        if self.pending_finality.is_empty() {
            return;
        }

        // Get the latest block number (last key in BTreeMap is the largest)
        let latest = match self.pending_finality.keys().next_back() {
            Some(&n) => n,
            None => return,
        };

        // Calculate the threshold for finality
        let finality_threshold = latest.saturating_sub(self.confirmations_required as u64);

        // Collect blocks that can be finalized (block number <= threshold)
        let to_finalize: Vec<_> = self
            .pending_finality
            .range(..=finality_threshold)
            .map(|(&n, &h)| (n, h))
            .collect();

        // Update finalized state
        for (number, hash) in &to_finalize {
            if *number > self.finalized_block {
                self.finalized_block = *number;
                self.finalized_hash = *hash;
                info!("🔒 Block #{} finalized (hash: {})", number, hash);
            }
        }

        // Remove finalized blocks from pending
        for (number, _) in to_finalize {
            self.pending_finality.remove(&number);
        }
    }

    /// Get last finalized block
    pub fn finalized(&self) -> (BlockNumber, Hash) {
        (self.finalized_block, self.finalized_hash)
    }

    /// Check if a block is finalized
    pub fn is_finalized(&self, number: BlockNumber) -> bool {
        number <= self.finalized_block
    }

    /// Get pending blocks count
    pub fn pending_count(&self) -> usize {
        self.pending_finality.len()
    }
}

impl Default for FinalityTracker {
    fn default() -> Self {
        Self::new(Hash::ZERO, 6) // 6 confirmations default
    }
}

// =============================================================================
// BLOCK PRODUCER
// =============================================================================

/// Block producer with transaction execution
pub struct BlockProducer {
    /// Configuration
    config: ProducerConfig,

    /// Validator signing key
    validator_key: Option<ed25519_dalek::SigningKey>,

    /// Database for double-signing protection
    db: Arc<Database>,

    /// Finality tracker
    finality: FinalityTracker,

    /// Inflation calculator for dynamic rewards
    inflation_calculator: InflationCalculator,

    /// Bootstrap configuration for era-specific rewards
    bootstrap_config: BootstrapConfig,

    /// SECURITY FIX #26: Track detected double-signing evidence for slashing
    /// Maps (epoch, slot) -> Vec<(block_hash, author)> to detect conflicting blocks
    double_sign_evidence: std::sync::RwLock<BTreeMap<(EpochNumber, SlotNumber), Vec<(Hash, AccountId)>>>,
}

impl BlockProducer {
    /// Create a new block producer
    pub fn new(validator_key: Option<ed25519_dalek::SigningKey>, db: Arc<Database>) -> Self {
        Self {
            config: ProducerConfig::default(),
            validator_key,
            db,
            finality: FinalityTracker::default(),
            inflation_calculator: InflationCalculator::new(InflationConfig::default()),
            bootstrap_config: get_bootstrap_config(),
            double_sign_evidence: std::sync::RwLock::new(BTreeMap::new()),
        }
    }

    /// Create with custom config
    pub fn with_config(
        config: ProducerConfig,
        validator_key: Option<ed25519_dalek::SigningKey>,
        db: Arc<Database>,
    ) -> Self {
        Self {
            config,
            validator_key,
            db,
            finality: FinalityTracker::default(),
            inflation_calculator: InflationCalculator::new(InflationConfig::default()),
            bootstrap_config: get_bootstrap_config(),
            double_sign_evidence: std::sync::RwLock::new(BTreeMap::new()),
        }
    }

    /// SECURITY FIX #26: Record a block for double-signing detection
    /// Returns Some(SlashableEvent) if double-signing is detected
    pub fn record_block_for_double_sign_detection(
        &self,
        block: &Block,
    ) -> Option<crate::consensus::slashing::SlashableEvent> {
        use crate::consensus::slashing::SlashableEvent;

        let epoch = block.header.epoch;
        let slot = block.header.slot;
        let block_hash = block.hash();
        let author = block.header.author;
        let key = (epoch, slot);

        let mut evidence = self.double_sign_evidence.write().ok()?;
        let blocks_at_slot = evidence.entry(key).or_insert_with(Vec::new);

        // Check if this author already has a different block at this slot
        for (existing_hash, existing_author) in blocks_at_slot.iter() {
            if *existing_author == author && *existing_hash != block_hash {
                // Double-signing detected!
                warn!(
                    "🚨 DOUBLE-SIGNING DETECTED: Validator {:?} produced blocks {} and {} at epoch {}, slot {}",
                    author, existing_hash, block_hash, epoch, slot
                );

                return Some(SlashableEvent::DoubleSigning {
                    slot,
                    epoch,
                    block_hash_1: *existing_hash.as_bytes(),
                    block_hash_2: *block_hash.as_bytes(),
                });
            }
        }

        // Record this block
        blocks_at_slot.push((block_hash, author));

        None
    }

    /// SECURITY FIX #26: Get all detected double-signing evidence
    /// Call this at epoch boundaries to process slashing events
    pub fn get_and_clear_double_sign_evidence(&self) -> Vec<(AccountId, crate::consensus::slashing::SlashableEvent)> {
        // For now, evidence is stored per-block. In a full implementation,
        // this would return validated evidence for slashing.
        // The double-signing is detected in record_block_for_double_sign_detection
        vec![]
    }

    /// Calculate dynamic block reward based on network metrics and era
    ///
    /// During bootstrap era (SPEC v2): Uses 6.5% inflation rate
    /// After bootstrap: Uses adaptive inflation (0.5% - 10%)
    ///
    /// Formula: BlockReward = AnnualEmission / BlocksPerYear
    /// Where: AnnualEmission = TotalSupply × InflationRate
    ///
    /// Falls back to config.block_reward if dynamic rewards disabled
    fn calculate_block_reward(&self, metrics: &NetworkMetrics, current_epoch: EpochNumber) -> Balance {
        if !self.config.use_dynamic_rewards {
            return self.config.block_reward;
        }

        // Check if we're in bootstrap era
        let is_bootstrap = self.bootstrap_config.is_bootstrap(current_epoch);

        let annual_emission = if is_bootstrap {
            // Bootstrap era: Use fixed 6.5% inflation (SPEC v2)
            let bootstrap_inflation = self.bootstrap_config.target_inflation;
            (metrics.total_supply as f64 * bootstrap_inflation) as Balance
        } else {
            // Post-bootstrap: Use adaptive inflation from calculator
            self.inflation_calculator.calculate_annual_emission(metrics)
        };

        // Convert to per-block reward
        // AnnualEmission / EpochsPerYear / SlotsPerEpoch
        let blocks_per_year = EPOCHS_PER_YEAR * SLOTS_PER_EPOCH;
        let block_reward = annual_emission / (blocks_per_year as u128);

        debug!(
            "Block reward calculation: total_supply={}, inflation={}, annual_emission={}, blocks_per_year={}, block_reward={} ({} KRAT)",
            metrics.total_supply,
            if is_bootstrap { self.bootstrap_config.target_inflation } else { 0.0 },
            annual_emission,
            blocks_per_year,
            block_reward,
            block_reward / KRAT
        );

        // Ensure minimum viable reward
        block_reward.max(1 * KRAT)
    }

    /// Apply VC bonus to block reward
    ///
    /// Formula: FinalReward = BaseReward × (1 + ln(1 + VC) / 10)
    ///
    /// This provides diminishing returns for high VC:
    /// - VC = 0: multiplier = 1.0 (no bonus)
    /// - VC = 100: multiplier ≈ 1.46
    /// - VC = 1000: multiplier ≈ 1.69
    /// - VC = 5000: multiplier ≈ 1.85
    /// - VC = 10000: multiplier ≈ 1.92
    fn apply_vc_bonus(&self, base_reward: Balance, validator_vc: u64) -> Balance {
        if !self.config.enable_vc_bonus || validator_vc == 0 {
            return base_reward;
        }

        // Calculate: 1 + ln(1 + VC) / 10
        let vc_factor = 1.0 + (1.0 + validator_vc as f64).ln() / 10.0;

        // Apply multiplier
        let bonus_reward = (base_reward as f64 * vc_factor) as Balance;

        bonus_reward
    }

    /// Distribute fees and rewards according to config
    ///
    /// Distribution:
    /// - 60% to block producer (validator_share)
    /// - 30% burned (removed from circulation)
    /// - 10% to treasury
    ///
    /// Returns: (validator_amount, burn_amount, treasury_amount)
    fn distribute_rewards(&self, total_amount: Balance) -> (Balance, Balance, Balance) {
        self.config.fee_distribution.distribute(total_amount)
    }

    /// Get default network metrics when state is not available
    /// Used as fallback for reward calculation
    fn get_default_metrics(&self) -> NetworkMetrics {
        NetworkMetrics {
            total_supply: 1_000_000_000 * KRAT, // 1B KRAT
            total_staked: 100_000_000 * KRAT,   // 100M KRAT (10%)
            active_validators: 100,
            active_users: 10_000,
            transactions_count: 100_000,
        }
    }

    /// Set finality tracker
    pub fn set_finality(&mut self, finality: FinalityTracker) {
        self.finality = finality;
    }

    /// Get finality tracker
    pub fn finality(&self) -> &FinalityTracker {
        &self.finality
    }

    /// Checks if an epoch/slot has already been signed
    fn has_signed_slot(&self, epoch: EpochNumber, slot: SlotNumber) -> Result<bool, ProductionError> {
        let key = format!("signed_slot:{}:{}", epoch, slot);
        match self.db.get(key.as_bytes()) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(ProductionError::DatabaseError(e.to_string())),
        }
    }

    /// Records that an epoch/slot has been signed
    fn mark_slot_as_signed(
        &self,
        epoch: EpochNumber,
        slot: SlotNumber,
        block_hash: Hash,
    ) -> Result<(), ProductionError> {
        let key = format!("signed_slot:{}:{}", epoch, slot);
        self.db
            .put(key.as_bytes(), block_hash.as_bytes())
            .map_err(|e| ProductionError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    /// Checks if this node is the slot leader using VRF
    ///
    /// Returns:
    /// - Ok(true) if this validator is selected as slot leader
    /// - Ok(false) if another validator is selected or no candidates
    /// - Err if state read fails (prevents silent inconsistencies that could cause forks)
    pub fn is_slot_leader(
        &self,
        slot: SlotNumber,
        epoch: EpochNumber,
        validator_set: &ValidatorSet,
        validator_id: &AccountId,
        state: &StateBackend,
    ) -> Result<bool, ProductionError> {
        if validator_set.validators.is_empty() {
            return Ok(false);
        }

        // Build candidate list with stake and VC
        // CRITICAL: Propagate errors instead of using defaults to prevent
        // different nodes from computing different leaders due to state inconsistencies
        let mut candidates = Vec::new();
        for validator in validator_set.validators.values() {
            let vc = state.get_total_vc(&validator.id)
                .map_err(|e| ProductionError::StateError(format!(
                    "Failed to get VC for validator {}: {}",
                    validator.id, e
                )))?;
            candidates.push((validator.id, validator.stake, vc));
        }

        // Use VRF selection
        match VRFSelector::select_validator(slot, epoch, &candidates) {
            Ok(selected_id) => Ok(selected_id == *validator_id),
            Err(e) => {
                // NoCandidates is not an error - just means we're not the leader
                if matches!(e, crate::consensus::vrf_selection::VRFError::NoCandidates) {
                    Ok(false)
                } else {
                    Err(ProductionError::StateError(format!("VRF selection error: {:?}", e)))
                }
            }
        }
    }

    /// Produce a new block
    pub async fn produce_block(
        &mut self,
        parent_block: &Block,
        mempool: Arc<RwLock<TransactionPool>>,
        state: Arc<RwLock<StateBackend>>,
        validator_id: AccountId,
        epoch: EpochNumber,
        slot: SlotNumber,
    ) -> Result<Block, ProductionError> {
        let block_number = parent_block.header.number + 1;
        debug!(
            "Producing block #{} (epoch {}, slot {})",
            block_number, epoch, slot
        );

        // Double-signing protection
        if self.has_signed_slot(epoch, slot)? {
            warn!(
                "⚠️  Double-signing attempt detected for epoch {}, slot {}",
                epoch, slot
            );
            return Err(ProductionError::AlreadySignedThisSlot);
        }

        // Verify we have a validator key
        let signing_key = self
            .validator_key
            .as_ref()
            .ok_or(ProductionError::NoValidatorKey)?;

        // Select transactions from mempool
        let transactions = {
            let mempool_guard = mempool.read().await;
            mempool_guard.select_transactions(self.config.max_transactions_per_block)
        };

        debug!("Selected {} transactions for block", transactions.len());

        // Execute transactions and collect results
        let (executed_txs, execution_results, state_root_computed) = {
            let mut state_guard = state.write().await;

            let mut executed = Vec::new();
            let mut results = Vec::new();
            let mut failed_count = 0;

            if self.config.execute_transactions {
                for tx in &transactions {
                    let result = TransactionExecutor::execute(&mut state_guard, tx, block_number);

                    if result.success {
                        executed.push(tx.clone());
                        results.push(result);
                    } else {
                        failed_count += 1;
                        debug!("Transaction {} failed: {:?}", tx.hash(), result.error);
                        results.push(result);
                    }
                }

                if failed_count > 0 {
                    info!("⚠️  {} transactions failed execution", failed_count);
                }
            } else {
                // No execution, include all
                executed = transactions;
            }

            // =================================================================
            // BLOCK REWARDS & FEE DISTRIBUTION
            // =================================================================
            //
            // Block reward: 100% to validator (no burn on emission)
            // Transaction fees: 60% validator, 30% burn, 10% treasury
            //
            // This creates:
            // - Full block reward incentive for validators
            // - Deflationary pressure only when network is used (fee burns)
            // - Sustainable treasury funding from fees

            // Get network metrics for dynamic reward calculation
            // TODO: Fetch real metrics from state when available
            let metrics = self.get_default_metrics();

            // Calculate base block reward dynamically (uses bootstrap rate if in bootstrap era)
            let base_block_reward = self.calculate_block_reward(&metrics, epoch);

            // Get validator's VC for bonus calculation
            let validator_vc = state_guard
                .get_total_vc(&validator_id)
                .unwrap_or(0);

            // Apply VC bonus to block reward
            let block_reward_with_bonus = self.apply_vc_bonus(base_block_reward, validator_vc);

            // Collect total fees from executed transactions
            let total_fees: Balance = results.iter().map(|r| r.fee_paid).sum();

            // Distribute fees: 60% validator, 30% burn, 10% treasury
            let (fee_to_validator, fee_burn, fee_to_treasury) = self.distribute_rewards(total_fees);

            // Total validator reward = 100% block reward + 60% fees
            let total_validator_reward = block_reward_with_bonus.saturating_add(fee_to_validator);

            if total_validator_reward > 0 || fee_to_treasury > 0 {
                // Pay validator: full block reward + their share of fees
                let mut validator_account = state_guard
                    .get_account(&validator_id)
                    .map_err(|e| ProductionError::StateError(format!("Get validator account: {:?}", e)))?
                    .unwrap_or(AccountInfo::new());

                validator_account.free = validator_account.free.saturating_add(total_validator_reward);

                state_guard
                    .set_account(validator_id, validator_account)
                    .map_err(|e| ProductionError::StateError(format!("Set validator account: {:?}", e)))?;

                // Pay treasury (10% of fees only)
                if fee_to_treasury > 0 {
                    let mut treasury_account = state_guard
                        .get_account(&self.config.treasury_account)
                        .map_err(|e| ProductionError::StateError(format!("Get treasury account: {:?}", e)))?
                        .unwrap_or(AccountInfo::new());

                    treasury_account.free = treasury_account.free.saturating_add(fee_to_treasury);

                    state_guard
                        .set_account(self.config.treasury_account, treasury_account)
                        .map_err(|e| ProductionError::StateError(format!("Set treasury account: {:?}", e)))?;
                }

                // Fee burn (30% of fees) - simply not credited to anyone
                // Creates deflationary pressure when network is actively used

                // Log reward with validator address for debugging
                let reward_krat = total_validator_reward / KRAT;
                let reward_frac = (total_validator_reward % KRAT) / 1_000_000_000; // 3 decimals
                info!(
                    "💰 Reward: +{}.{:03} KRAT → 0x{}",
                    reward_krat, reward_frac, hex::encode(validator_id.as_bytes())
                );

                debug!(
                    "Rewards distributed: block_reward={} (VC bonus from {} VC), fee_validator={}, fee_burn={}, fee_treasury={}",
                    block_reward_with_bonus, validator_vc, fee_to_validator, fee_burn, fee_to_treasury
                );
            }

            // Compute state root
            let chain_id = ChainId(0); // TODO: Configure
            let state_root = state_guard.compute_state_root(block_number, chain_id);

            // Store state root
            state_guard
                .store_state_root(block_number, state_root)
                .map_err(|e| ProductionError::StateError(e.to_string()))?;

            (executed, results, state_root)
        };

        // Build block
        let transactions_root = Self::compute_transactions_root(&executed_txs);

        // SECURITY FIX #35: Use canonical slot timestamp to avoid cumulative drift
        // The slot timestamp is deterministic: genesis_timestamp + slot * SLOT_DURATION
        // This ensures consistent timestamps across all validators and prevents drift accumulation
        // Get genesis_timestamp from state's drift tracker
        let genesis_timestamp = {
            let state_read = state.read().await;
            state_read.get_drift_tracker()
                .map_err(|e| ProductionError::StateError(format!("Failed to get drift tracker: {:?}", e)))?
                .map(|t| t.genesis_timestamp)
                .unwrap_or_else(|| chrono::Utc::now().timestamp() as u64)
        };
        let timestamp = genesis_timestamp.saturating_add(slot.saturating_mul(SLOT_DURATION_SECS));

        let mut header = BlockHeader {
            number: block_number,
            parent_hash: parent_block.hash(),
            transactions_root,
            state_root: state_root_computed.root,
            timestamp,
            epoch,
            slot,
            author: validator_id,
            signature: Signature64([0; 64]),
        };

        // Sign header with domain separation (SECURITY FIX #24)
        // Domain separation prevents block signatures from being replayed as transaction signatures
        let header_hash = header.hash();
        let message = domain_separate(DOMAIN_BLOCK_HEADER, header_hash.as_bytes());
        let signature = signing_key.sign(&message);
        header.signature = Signature64(signature.to_bytes());

        let block = Block {
            header,
            body: BlockBody {
                transactions: executed_txs,
            },
        };

        // Record signed slot (double-signing protection)
        let block_hash = block.hash();
        self.mark_slot_as_signed(epoch, slot, block_hash)?;

        // Update finality tracker
        self.finality.add_block(block_number, block_hash);

        // Remove included transactions from mempool
        {
            let mut mempool_guard = mempool.write().await;
            mempool_guard.remove_included(&block.body.transactions);
        }

        let tx_count = block.body.transactions.len();
        let total_fees: Balance = execution_results.iter().map(|r| r.fee_paid).sum();

        // Calculate rewards for logging (must match actual reward calculation)
        // Block reward: 100% to validator with VC bonus
        // Fees: 60% validator, 30% burn, 10% treasury
        let metrics = self.get_default_metrics();
        let base_block_reward = self.calculate_block_reward(&metrics, epoch);

        // Get validator's VC for bonus calculation
        let validator_vc = {
            let state_guard = state.read().await;
            state_guard.get_total_vc(&validator_id).unwrap_or(0)
        };
        let block_reward = self.apply_vc_bonus(base_block_reward, validator_vc);

        let (fee_to_validator, _, _) = self.distribute_rewards(total_fees);
        let total_validator_reward = block_reward.saturating_add(fee_to_validator);

        // Format validator reward in KRAT (divide by 10^12)
        let reward_krat = total_validator_reward / 1_000_000_000_000;
        let reward_remainder = (total_validator_reward % 1_000_000_000_000) / 1_000_000_000; // 3 decimal places

        if tx_count > 0 {
            let fees_krat = total_fees / 1_000_000_000_000;
            let fees_remainder = (total_fees % 1_000_000_000_000) / 1_000_000_000;
            info!(
                "⛏️  Block #{} | {} txs | +{}.{:03} KRAT (reward) | fees: {}.{:03} KRAT",
                block.header.number, tx_count, reward_krat, reward_remainder, fees_krat, fees_remainder
            );
        } else {
            info!(
                "⛏️  Block #{} | +{}.{:03} KRAT",
                block.header.number, reward_krat, reward_remainder
            );
        }

        Ok(block)
    }

    /// Validate and import a block from network
    pub async fn validate_and_import(
        &mut self,
        block: Block,
        parent: &Block,
        validator_set: &ValidatorSet,
        state: Arc<RwLock<StateBackend>>,
        mempool: Arc<RwLock<TransactionPool>>,
    ) -> Result<(), ProductionError> {
        // Validate block
        BlockValidator::validate(&block, parent, validator_set)
            .map_err(|e| ProductionError::ValidationError(e.to_string()))?;

        // Execute transactions to verify state root
        if self.config.execute_transactions {
            let computed_root = {
                let mut state_guard = state.write().await;

                // Execute all transactions
                for tx in &block.body.transactions {
                    let result = TransactionExecutor::execute(&mut state_guard, tx, block.header.number);
                    if !result.success {
                        return Err(ProductionError::ExecutionError(format!(
                            "Transaction {} failed: {:?}",
                            tx.hash(),
                            result.error
                        )));
                    }
                }

                // Compute state root
                let chain_id = ChainId(0);
                let root = state_guard.compute_state_root(block.header.number, chain_id);

                // Store state root
                state_guard
                    .store_state_root(block.header.number, root)
                    .map_err(|e| ProductionError::StateError(e.to_string()))?;

                root.root
            };

            // Verify state root matches
            if computed_root != block.header.state_root {
                return Err(ProductionError::StateRootMismatch {
                    expected: block.header.state_root,
                    computed: computed_root,
                });
            }
        }

        // Update finality tracker
        self.finality.add_block(block.header.number, block.hash());

        // Remove included transactions from mempool
        {
            let mut mempool_guard = mempool.write().await;
            mempool_guard.remove_included(&block.body.transactions);
        }

        info!(
            "📥 Block #{} imported and validated ({} txs)",
            block.header.number,
            block.body.transactions.len()
        );

        Ok(())
    }

    /// Compute transactions root (Merkle root)
    fn compute_transactions_root(transactions: &[SignedTransaction]) -> Hash {
        if transactions.is_empty() {
            return Hash::ZERO;
        }

        let mut data = Vec::new();
        for tx in transactions {
            let hash = tx.hash();
            data.extend_from_slice(hash.as_bytes());
        }

        Hash::hash(&data)
    }

    /// Compute current slot from timestamp
    pub fn current_slot(genesis_timestamp: u64) -> SlotNumber {
        let now = chrono::Utc::now().timestamp() as u64;
        if now < genesis_timestamp {
            return 0;
        }
        (now - genesis_timestamp) / SLOT_DURATION_SECS
    }

    /// Compute current epoch from slot
    pub fn current_epoch(slot: SlotNumber, slots_per_epoch: u64) -> EpochNumber {
        slot / slots_per_epoch
    }

    /// Wait for the next slot
    pub async fn wait_next_slot(current_slot: SlotNumber, genesis_timestamp: u64) {
        let next_slot_time = genesis_timestamp + (current_slot + 1) * SLOT_DURATION_SECS;
        let now = chrono::Utc::now().timestamp() as u64;

        if next_slot_time > now {
            let wait_duration = std::time::Duration::from_secs(next_slot_time - now);
            debug!("⏳ Waiting {:?} for next slot", wait_duration);
            tokio::time::sleep(wait_duration).await;
        }
    }

    /// Get configuration
    pub fn config(&self) -> &ProducerConfig {
        &self.config
    }
}

// =============================================================================
// BLOCK REWARD APPLICATION (for sync/import)
// =============================================================================

/// Apply block rewards during block import (for syncing nodes)
/// This replicates the reward logic from produce_block for imported blocks
///
/// Arguments:
/// - state: mutable state backend to credit rewards
/// - author: block producer account
/// - epoch: block epoch (for bootstrap/post-bootstrap inflation rate)
/// - total_fees: sum of fees from all transactions in block
///
/// Returns: total reward credited to validator (block reward + fees share)
pub fn apply_block_rewards_for_import(
    state: &mut StateBackend,
    author: AccountId,
    epoch: EpochNumber,
    total_fees: Balance,
) -> Result<Balance, String> {
    // Get bootstrap config to check if we're in bootstrap era
    let bootstrap_config = get_bootstrap_config();
    let is_bootstrap = bootstrap_config.is_bootstrap(epoch);

    // Get default metrics for reward calculation
    let metrics = NetworkMetrics {
        total_supply: 1_000_000_000 * KRAT, // 1B KRAT
        total_staked: 100_000_000 * KRAT,   // 100M KRAT (10%)
        active_validators: 100,
        active_users: 10_000,
        transactions_count: 100_000,
    };

    // Calculate base block reward (same logic as BlockProducer::calculate_block_reward)
    let annual_emission = if is_bootstrap {
        // Bootstrap era: Use fixed 6.5% inflation (SPEC v2)
        let bootstrap_inflation = bootstrap_config.target_inflation;
        (metrics.total_supply as f64 * bootstrap_inflation) as Balance
    } else {
        // Post-bootstrap: Use adaptive inflation (simplified for import)
        let inflation_config = InflationConfig::default();
        let calculator = InflationCalculator::new(inflation_config);
        calculator.calculate_annual_emission(&metrics)
    };

    // Convert to per-block reward
    let blocks_per_year = EPOCHS_PER_YEAR * SLOTS_PER_EPOCH;
    let block_reward = (annual_emission / (blocks_per_year as u128)).max(1 * KRAT);

    // Get validator's VC for bonus calculation
    let validator_vc = state.get_total_vc(&author).unwrap_or(0);

    // Apply VC bonus: FinalReward = BaseReward × (1 + ln(1 + VC) / 10)
    let block_reward_with_bonus = if validator_vc > 0 {
        let multiplier = 1.0 + (1.0 + validator_vc as f64).ln() / 10.0;
        (block_reward as f64 * multiplier) as Balance
    } else {
        block_reward
    };

    // Distribute fees: 60% validator, 30% burn, 10% treasury
    let fee_distribution = FeeDistribution::default_distribution();
    let (fee_to_validator, _fee_burn, fee_to_treasury) = fee_distribution.distribute(total_fees);

    // Total validator reward = 100% block reward + 60% fees
    let total_validator_reward = block_reward_with_bonus.saturating_add(fee_to_validator);

    // Credit validator
    if total_validator_reward > 0 {
        let mut validator_account = state
            .get_account(&author)
            .map_err(|e| format!("Get validator account: {:?}", e))?
            .unwrap_or(AccountInfo::new());

        validator_account.free = validator_account.free.saturating_add(total_validator_reward);

        state
            .set_account(author, validator_account)
            .map_err(|e| format!("Set validator account: {:?}", e))?;
    }

    // Credit treasury (10% of fees only)
    if fee_to_treasury > 0 {
        let treasury_account_id = AccountId::from_bytes(TREASURY_ACCOUNT);
        let mut treasury_account = state
            .get_account(&treasury_account_id)
            .map_err(|e| format!("Get treasury account: {:?}", e))?
            .unwrap_or(AccountInfo::new());

        treasury_account.free = treasury_account.free.saturating_add(fee_to_treasury);

        state
            .set_account(treasury_account_id, treasury_account)
            .map_err(|e| format!("Set treasury account: {:?}", e))?;
    }

    // Log reward reconstruction (not new rewards, just rebuilding state to match producer)
    let reward_krat = total_validator_reward / KRAT;
    let reward_frac = (total_validator_reward % KRAT) / 1_000_000_000; // 3 decimals
    debug!(
        "📥 State rebuild: credited +{}.{:03} KRAT to author 0x{} (matching producer's state)",
        reward_krat, reward_frac, hex::encode(author.as_bytes())
    );

    Ok(total_validator_reward)
}

// =============================================================================
// ERRORS
// =============================================================================

/// Block production errors
#[derive(Debug, thiserror::Error)]
pub enum ProductionError {
    #[error("No validator key configured")]
    NoValidatorKey,

    #[error("Not the slot leader")]
    NotSlotLeader,

    #[error("Already signed a block for this slot")]
    AlreadySignedThisSlot,

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("State error: {0}")]
    StateError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("State root mismatch: expected {expected}, computed {computed}")]
    StateRootMismatch { expected: Hash, computed: Hash },
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::Database;
    use crate::types::primitives::KRAT;
    use ed25519_dalek::Signer as _;
    use tempfile::tempdir;

    fn create_test_account(state: &mut StateBackend, id: AccountId, balance: Balance) {
        let account = AccountInfo {
            nonce: 0,
            free: balance,
            reserved: 0,
            last_modified: Hash::ZERO,
        };
        state.set_account(id, account).unwrap();
    }

    /// Create a test transaction with dummy signature (for tests with verify_signatures = false)
    fn create_test_tx(sender: AccountId, to: AccountId, amount: Balance, nonce: u64) -> SignedTransaction {
        let tx = Transaction {
            sender,
            nonce,
            call: TransactionCall::Transfer { to, amount },
            timestamp: chrono::Utc::now().timestamp() as u64,
        };
        SignedTransaction {
            transaction: tx,
            signature: Signature64([0; 64]),
            hash: None,
        }
    }

    /// Create a properly signed test transaction
    /// SECURITY FIX #27: Uses domain separation via SignedTransaction::signing_message()
    fn create_signed_tx(
        signing_key: &ed25519_dalek::SigningKey,
        to: AccountId,
        amount: Balance,
        nonce: u64,
    ) -> SignedTransaction {
        let sender = AccountId::from_bytes(signing_key.verifying_key().to_bytes());
        let tx = Transaction {
            sender,
            nonce,
            call: TransactionCall::Transfer { to, amount },
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        // SECURITY FIX #27: Use domain-separated signing message
        let message = SignedTransaction::signing_message(&tx).unwrap();
        let signature = signing_key.sign(&message);
        let tx_hash = tx.hash();

        SignedTransaction {
            transaction: tx,
            signature: Signature64(signature.to_bytes()),
            hash: Some(tx_hash),
        }
    }

    /// Create a properly signed stake transaction
    /// SECURITY FIX #27: Uses domain separation via SignedTransaction::signing_message()
    fn create_signed_stake_tx(
        signing_key: &ed25519_dalek::SigningKey,
        amount: Balance,
        nonce: u64,
    ) -> SignedTransaction {
        let sender = AccountId::from_bytes(signing_key.verifying_key().to_bytes());
        let tx = Transaction {
            sender,
            nonce,
            call: TransactionCall::Stake { amount },
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        // SECURITY FIX #27: Use domain-separated signing message
        let message = SignedTransaction::signing_message(&tx).unwrap();
        let signature = signing_key.sign(&message);
        let tx_hash = tx.hash();

        SignedTransaction {
            transaction: tx,
            signature: Signature64(signature.to_bytes()),
            hash: Some(tx_hash),
        }
    }

    #[test]
    fn test_compute_transactions_root_empty() {
        let transactions = vec![];
        let root = BlockProducer::compute_transactions_root(&transactions);
        assert_eq!(root, Hash::ZERO);
    }

    #[test]
    fn test_compute_transactions_root_deterministic() {
        // For merkle root computation, signature validity doesn't matter
        let sender_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let to = AccountId::from_bytes([2; 32]);

        let txs = vec![
            create_signed_tx(&sender_key, to, 1000, 0),
            create_signed_tx(&sender_key, to, 2000, 1),
        ];

        let root1 = BlockProducer::compute_transactions_root(&txs);
        let root2 = BlockProducer::compute_transactions_root(&txs);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_current_slot() {
        let genesis = chrono::Utc::now().timestamp() as u64 - 60;
        let slot = BlockProducer::current_slot(genesis);
        assert_eq!(slot, 60 / SLOT_DURATION_SECS);
    }

    #[test]
    fn test_transaction_executor_transfer() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().to_str().unwrap()).unwrap();
        let mut state = StateBackend::new(db);

        // Generate a real keypair for the sender
        let sender_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let sender = AccountId::from_bytes(sender_key.verifying_key().to_bytes());
        let receiver = AccountId::from_bytes([2; 32]);

        // Create sender with balance
        create_test_account(&mut state, sender, 100 * KRAT);

        // Create properly signed transfer transaction
        let tx = create_signed_tx(&sender_key, receiver, 10 * KRAT, 0);

        // Execute (use block 1 for testing)
        let result = TransactionExecutor::execute(&mut state, &tx, 1);

        assert!(result.success, "Execution failed: {:?}", result.error);
        assert_eq!(result.fee_paid, 1_000); // Transfer fee

        // Check balances
        let sender_acc = state.get_account(&sender).unwrap().unwrap();
        let receiver_acc = state.get_account(&receiver).unwrap().unwrap();

        // Sender: 100 KRAT - 10 KRAT transfer - 1000 fee
        assert_eq!(sender_acc.free, 100 * KRAT - 10 * KRAT - 1_000);
        assert_eq!(sender_acc.nonce, 1);

        // Receiver: 10 KRAT
        assert_eq!(receiver_acc.free, 10 * KRAT);
    }

    #[test]
    fn test_transaction_executor_insufficient_balance() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().to_str().unwrap()).unwrap();
        let mut state = StateBackend::new(db);

        // Generate a real keypair for the sender
        let sender_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let sender = AccountId::from_bytes(sender_key.verifying_key().to_bytes());
        let receiver = AccountId::from_bytes([2; 32]);

        // Create sender with small balance
        create_test_account(&mut state, sender, 1000);

        // Try to transfer more than available (properly signed)
        let tx = create_signed_tx(&sender_key, receiver, 10 * KRAT, 0);

        let result = TransactionExecutor::execute(&mut state, &tx, 1);

        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_transaction_executor_invalid_nonce() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().to_str().unwrap()).unwrap();
        let mut state = StateBackend::new(db);

        // Generate a real keypair for the sender
        let sender_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let sender = AccountId::from_bytes(sender_key.verifying_key().to_bytes());
        let receiver = AccountId::from_bytes([2; 32]);

        create_test_account(&mut state, sender, 100 * KRAT);

        // Wrong nonce (expecting 0, giving 5) - properly signed
        let tx = create_signed_tx(&sender_key, receiver, 1000, 5);

        let result = TransactionExecutor::execute(&mut state, &tx, 1);

        assert!(!result.success);
        assert!(result.error.unwrap().contains("nonce"));
    }

    #[test]
    fn test_transaction_executor_stake() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().to_str().unwrap()).unwrap();
        let mut state = StateBackend::new(db);

        // Generate a real keypair for the sender
        let sender_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let sender = AccountId::from_bytes(sender_key.verifying_key().to_bytes());
        create_test_account(&mut state, sender, 100 * KRAT);

        // Create properly signed stake transaction
        let tx = create_signed_stake_tx(&sender_key, 50 * KRAT, 0);

        let result = TransactionExecutor::execute(&mut state, &tx, 1);

        assert!(result.success, "Stake failed: {:?}", result.error);

        let sender_acc = state.get_account(&sender).unwrap().unwrap();
        // Free: 100 KRAT - 50 KRAT staked - 5000 fee
        assert_eq!(sender_acc.free, 100 * KRAT - 50 * KRAT - 5_000);
        assert_eq!(sender_acc.reserved, 50 * KRAT);
    }

    #[test]
    fn test_finality_tracker() {
        let mut tracker = FinalityTracker::new(Hash::ZERO, 3);

        // Add blocks
        for i in 1u64..=10 {
            let hash = Hash::hash(&i.to_le_bytes());
            tracker.add_block(i, hash);
        }

        // With 10 blocks and 3 confirmations, blocks 1-7 should be finalized
        let (finalized_num, _) = tracker.finalized();
        assert_eq!(finalized_num, 7);

        assert!(tracker.is_finalized(5));
        assert!(tracker.is_finalized(7));
        assert!(!tracker.is_finalized(8));
    }

    #[tokio::test]
    async fn test_produce_block() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        let db = Arc::new(Database::open(dir1.path().to_str().unwrap()).unwrap());
        let state_db = Database::open(dir2.path().to_str().unwrap()).unwrap();
        let mut state = StateBackend::new(state_db);

        // Create validator key
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let validator_id = AccountId::from_bytes(signing_key.verifying_key().to_bytes());

        // Create validator account with balance for testing
        create_test_account(&mut state, validator_id, 1000 * KRAT);

        let mut producer = BlockProducer::new(Some(signing_key), db);
        producer.config.execute_transactions = false; // Skip execution for this test

        // Create genesis block
        let genesis = Block {
            header: BlockHeader {
                number: 0,
                parent_hash: Hash::ZERO,
                transactions_root: Hash::ZERO,
                state_root: Hash::ZERO,
                timestamp: 0,
                epoch: 0,
                slot: 0,
                author: AccountId::from_bytes([0; 32]),
                signature: Signature64([0; 64]),
            },
            body: BlockBody {
                transactions: vec![],
            },
        };

        let mempool = Arc::new(RwLock::new(TransactionPool::default()));
        let state_arc = Arc::new(RwLock::new(state));

        let result = producer
            .produce_block(&genesis, mempool, state_arc, validator_id, 0, 1)
            .await;

        assert!(result.is_ok(), "Block production failed: {:?}", result.err());

        let block = result.unwrap();
        assert_eq!(block.header.number, 1);
        assert_eq!(block.header.parent_hash, genesis.hash());
        assert_eq!(block.header.author, validator_id);
        assert_eq!(block.header.epoch, 0);
        assert_eq!(block.header.slot, 1);
    }

    #[tokio::test]
    async fn test_produce_block_with_transactions() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        let db = Arc::new(Database::open(dir1.path().to_str().unwrap()).unwrap());
        let state_db = Database::open(dir2.path().to_str().unwrap()).unwrap();
        let mut state = StateBackend::new(state_db);

        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let validator_id = AccountId::from_bytes(signing_key.verifying_key().to_bytes());

        // Create sender keypair for properly signed transactions
        let sender_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let sender = AccountId::from_bytes(sender_key.verifying_key().to_bytes());
        let receiver = AccountId::from_bytes([2; 32]);

        create_test_account(&mut state, validator_id, 1000 * KRAT);
        create_test_account(&mut state, sender, 100 * KRAT);

        let state_arc = Arc::new(RwLock::new(state));

        // Add properly signed transactions to mempool
        let mempool = Arc::new(RwLock::new(TransactionPool::default()));
        {
            let mut mp = mempool.write().await;
            mp.add(create_signed_tx(&sender_key, receiver, 10 * KRAT, 0)).unwrap();
            mp.add(create_signed_tx(&sender_key, receiver, 5 * KRAT, 1)).unwrap();
        }

        let mut producer = BlockProducer::new(Some(signing_key), db);

        let genesis = Block {
            header: BlockHeader {
                number: 0,
                parent_hash: Hash::ZERO,
                transactions_root: Hash::ZERO,
                state_root: Hash::ZERO,
                timestamp: 0,
                epoch: 0,
                slot: 0,
                author: AccountId::from_bytes([0; 32]),
                signature: Signature64([0; 64]),
            },
            body: BlockBody {
                transactions: vec![],
            },
        };

        let result = producer
            .produce_block(&genesis, mempool.clone(), state_arc.clone(), validator_id, 0, 1)
            .await;

        assert!(result.is_ok(), "Block production failed: {:?}", result.err());

        let block = result.unwrap();
        assert_eq!(block.body.transactions.len(), 2);

        // Mempool should be empty after block production
        let mp = mempool.read().await;
        assert_eq!(mp.len(), 0);

        // Verify state updated
        let mut state = state_arc.write().await;
        let sender_acc = state.get_account(&sender).unwrap().unwrap();
        // Executed both transfers
        assert_eq!(sender_acc.nonce, 2);
    }

    #[test]
    fn test_double_signing_protection() {
        let dir = tempdir().unwrap();
        let db = Arc::new(Database::open(dir.path().to_str().unwrap()).unwrap());

        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::thread_rng());
        let producer = BlockProducer::new(Some(signing_key), db);

        // First sign should succeed
        assert!(!producer.has_signed_slot(0, 1).unwrap());

        producer
            .mark_slot_as_signed(0, 1, Hash::hash(&[1, 2, 3]))
            .unwrap();

        // Second check should show already signed
        assert!(producer.has_signed_slot(0, 1).unwrap());

        // Different slot should be fine
        assert!(!producer.has_signed_slot(0, 2).unwrap());
    }
}
