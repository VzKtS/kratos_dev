// Mempool - Production-grade transaction pool with priority ordering
//
// Features:
// - Priority fee sorting (highest fee first)
// - Per-account nonce tracking with gap detection
// - Replace-by-fee (RBF) support
// - Eviction policies for full pool
// - Rate limiting per account
// - Transaction validation before acceptance

use crate::storage::state::StateBackend;
use crate::types::{AccountId, AccountInfo, Balance, Hash, SignedTransaction, TransactionCall};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Maximum allowed nonce gap to prevent resource exhaustion attacks
/// SECURITY FIX #14: Reduced from 10 to 4 to limit pending transactions queue growth
/// SECURITY FIX #22: Further reduced from 4 to 2 for tighter DoS protection
const MAX_NONCE_GAP: u64 = 2;

/// SECURITY FIX #22: Maximum total pending transactions across all accounts
/// Prevents memory exhaustion attacks (100 accounts × 2 gap = 200 pending max)
const MAX_TOTAL_PENDING: usize = 500;

/// SECURITY FIX #34: Maximum absolute nonce value
/// Prevents attackers from submitting transactions with extremely high nonces
/// that would be impossible to fill (e.g., nonce 1_000_000_000)
/// This limits to ~15 years of activity at 1 tx/second per account
const MAX_ABSOLUTE_NONCE: u64 = 500_000_000;

/// Mempool configuration
#[derive(Debug, Clone)]
pub struct MempoolConfig {
    /// Maximum number of transactions in pool
    pub max_size: usize,

    /// Maximum transactions per account
    pub max_per_account: usize,

    /// Minimum fee to accept (in smallest unit)
    pub min_fee: Balance,

    /// Replace-by-fee minimum increase (percentage)
    pub rbf_min_increase_pct: u8,

    /// Transaction expiration time
    pub tx_expiration: Duration,

    /// Rate limit: max submissions per account per window
    pub rate_limit_per_account: usize,

    /// Rate limit window duration
    pub rate_limit_window: Duration,

    /// Enable signature verification
    /// SECURITY FIX #21: In production builds, signatures are ALWAYS verified.
    /// This flag only has effect in #[cfg(test)] builds for testing purposes.
    pub verify_signatures: bool,

    /// SECURITY FIX #14: Maximum allowed nonce gap
    pub max_nonce_gap: u64,

    /// SECURITY FIX #22: Maximum total pending transactions
    pub max_total_pending: usize,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_size: 10_000,
            max_per_account: 100,
            min_fee: 1_000, // Minimum base fee
            rbf_min_increase_pct: 10, // 10% fee increase for replacement
            tx_expiration: Duration::from_secs(3600), // 1 hour
            rate_limit_per_account: 50,
            rate_limit_window: Duration::from_secs(60),
            verify_signatures: true,
            max_nonce_gap: MAX_NONCE_GAP, // SECURITY FIX #14 & #22
            max_total_pending: MAX_TOTAL_PENDING, // SECURITY FIX #22
        }
    }
}

// =============================================================================
// PRIORITY WRAPPER
// =============================================================================

/// Transaction with priority ordering
#[derive(Debug, Clone)]
struct PrioritizedTx {
    /// Transaction hash
    hash: Hash,
    /// Effective fee (for ordering)
    fee: Balance,
    /// Timestamp when added
    added_at: Instant,
    /// Sender account
    sender: AccountId,
    /// Nonce
    nonce: u64,
}

impl PrioritizedTx {
    fn new(tx: &SignedTransaction, fee: Balance) -> Self {
        Self {
            hash: tx.hash(),
            fee,
            added_at: Instant::now(),
            sender: tx.transaction.sender,
            nonce: tx.transaction.nonce,
        }
    }
}

// Higher fee = higher priority (max heap)
impl PartialEq for PrioritizedTx {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
    }
}

impl Eq for PrioritizedTx {}

impl PartialOrd for PrioritizedTx {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedTx {
    fn cmp(&self, other: &Self) -> Ordering {
        // Primary: higher fee first
        match self.fee.cmp(&other.fee) {
            Ordering::Equal => {
                // Secondary: earlier timestamp first (FIFO for same fee)
                other.added_at.cmp(&self.added_at)
            }
            other_cmp => other_cmp,
        }
    }
}

// =============================================================================
// RATE LIMITER
// =============================================================================

/// Per-account rate limiting
#[derive(Debug, Default)]
struct AccountRateLimiter {
    /// Submission count per account in current window
    submissions: HashMap<AccountId, (usize, Instant)>,
}

impl AccountRateLimiter {
    fn check(&mut self, account: &AccountId, limit: usize, window: Duration) -> bool {
        let now = Instant::now();

        if let Some((count, window_start)) = self.submissions.get_mut(account) {
            if now.duration_since(*window_start) > window {
                // Window expired, reset
                *count = 1;
                *window_start = now;
                true
            } else if *count >= limit {
                false
            } else {
                *count += 1;
                true
            }
        } else {
            self.submissions.insert(*account, (1, now));
            true
        }
    }

    fn cleanup(&mut self, window: Duration) {
        let now = Instant::now();
        self.submissions
            .retain(|_, (_, start)| now.duration_since(*start) <= window);
    }
}

// =============================================================================
// ACCOUNT QUEUE
// =============================================================================

/// Per-account transaction queue with nonce ordering
#[derive(Debug, Default)]
struct AccountQueue {
    /// Transactions by nonce (ordered)
    by_nonce: BTreeMap<u64, Hash>,
    /// Total fee sum for this account
    total_fees: Balance,
    /// Last submission time
    last_submission: Option<Instant>,
}

impl AccountQueue {
    fn add(&mut self, nonce: u64, hash: Hash, fee: Balance) {
        self.by_nonce.insert(nonce, hash);
        self.total_fees = self.total_fees.saturating_add(fee);
        self.last_submission = Some(Instant::now());
    }

    fn remove(&mut self, nonce: u64, fee: Balance) -> Option<Hash> {
        let hash = self.by_nonce.remove(&nonce)?;
        self.total_fees = self.total_fees.saturating_sub(fee);
        Some(hash)
    }

    fn len(&self) -> usize {
        self.by_nonce.len()
    }

    fn is_empty(&self) -> bool {
        self.by_nonce.is_empty()
    }

    /// Get sequential ready transactions starting from expected nonce
    fn ready_hashes(&self, expected_nonce: u64) -> Vec<Hash> {
        let mut ready = Vec::new();
        let mut next_nonce = expected_nonce;

        for (&nonce, &hash) in &self.by_nonce {
            if nonce < next_nonce {
                // Old transaction, skip
                continue;
            }
            if nonce != next_nonce {
                // Gap detected
                break;
            }
            ready.push(hash);
            next_nonce += 1;
        }

        ready
    }

    /// Check for nonce gap
    fn has_gap(&self, expected_nonce: u64) -> bool {
        if let Some((&first_nonce, _)) = self.by_nonce.first_key_value() {
            first_nonce > expected_nonce
        } else {
            false
        }
    }

    /// SECURITY FIX #22: Count pending (non-ready) transactions
    /// These are transactions waiting for earlier nonces to be filled
    fn pending_count(&self) -> usize {
        if self.by_nonce.is_empty() {
            return 0;
        }
        // Count transactions after a gap
        let mut count = 0;
        let mut prev_nonce: Option<u64> = None;
        let mut found_gap = false;

        for &nonce in self.by_nonce.keys() {
            if let Some(prev) = prev_nonce {
                if nonce > prev + 1 {
                    found_gap = true;
                }
            }
            if found_gap {
                count += 1;
            }
            prev_nonce = Some(nonce);
        }
        count
    }
}

// =============================================================================
// TRANSACTION POOL
// =============================================================================

/// Production-grade transaction pool
pub struct TransactionPool {
    /// Configuration (public for test access)
    pub config: MempoolConfig,

    /// All transactions by hash
    transactions: HashMap<Hash, SignedTransaction>,

    /// Transaction fees by hash
    fees: HashMap<Hash, Balance>,

    /// Priority queue for block selection
    priority_queue: BinaryHeap<PrioritizedTx>,

    /// Per-account queues
    account_queues: HashMap<AccountId, AccountQueue>,

    /// Rate limiter
    rate_limiter: AccountRateLimiter,

    /// Pending transactions (waiting for earlier nonces)
    pending: HashSet<Hash>,

    /// Statistics
    stats: PoolStats,
}

/// Pool statistics
#[derive(Debug, Default, Clone)]
pub struct PoolStats {
    /// Total transactions added
    pub total_added: u64,
    /// Total transactions removed (included in blocks)
    pub total_removed: u64,
    /// Total evicted (pool full)
    pub total_evicted: u64,
    /// Total rejected (validation failed)
    pub total_rejected: u64,
    /// Total replaced (RBF)
    pub total_replaced: u64,
}

/// Pool errors
#[derive(Debug, thiserror::Error)]
pub enum PoolError {
    #[error("Transaction has no hash")]
    NoHash,

    #[error("Transaction already exists")]
    AlreadyExists,

    #[error("Pool is full")]
    PoolFull,

    #[error("Fee too low: {0} < minimum {1}")]
    FeeTooLow(Balance, Balance),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Invalid nonce: expected {expected}, got {got}")]
    InvalidNonce { expected: u64, got: u64 },

    #[error("Nonce too old: {got} < current {current}")]
    NonceTooOld { got: u64, current: u64 },

    #[error("Insufficient balance: need {need}, have {have}")]
    InsufficientBalance { need: Balance, have: Balance },

    #[error("Too many transactions for account: {count} >= {max}")]
    TooManyPerAccount { count: usize, max: usize },

    #[error("Rate limit exceeded for account")]
    RateLimitExceeded,

    /// SECURITY FIX #22: Global pending limit
    #[error("Too many pending transactions globally: {count} >= {max}")]
    TooManyPendingGlobal { count: usize, max: usize },

    /// SECURITY FIX #34: Nonce exceeds absolute maximum
    #[error("Nonce exceeds maximum: {nonce} > {max}")]
    NonceTooHigh { nonce: u64, max: u64 },

    #[error("RBF fee increase insufficient: need {need_pct}% increase")]
    RbfFeeInsufficient { need_pct: u8 },

    #[error("Transaction expired")]
    Expired,

    #[error("Validation error: {0}")]
    Validation(String),
}

impl TransactionPool {
    /// Create a new transaction pool with default config
    pub fn new(max_size: usize) -> Self {
        Self::with_config(MempoolConfig {
            max_size,
            ..Default::default()
        })
    }

    /// Create with custom configuration
    pub fn with_config(config: MempoolConfig) -> Self {
        Self {
            config,
            transactions: HashMap::new(),
            fees: HashMap::new(),
            priority_queue: BinaryHeap::new(),
            account_queues: HashMap::new(),
            rate_limiter: AccountRateLimiter::default(),
            pending: HashSet::new(),
            stats: PoolStats::default(),
        }
    }

    /// Add a transaction to the pool
    pub fn add(&mut self, tx: SignedTransaction) -> Result<(), PoolError> {
        self.add_with_validation(tx, None)
    }

    /// Add with optional state validation
    pub fn add_with_validation(
        &mut self,
        tx: SignedTransaction,
        state: Option<&mut StateBackend>,
    ) -> Result<(), PoolError> {
        let hash = tx.hash.ok_or(PoolError::NoHash)?;

        // Check if already exists
        if self.transactions.contains_key(&hash) {
            return Err(PoolError::AlreadyExists);
        }

        let sender = tx.transaction.sender;
        let nonce = tx.transaction.nonce;
        let fee = tx.transaction.call.base_fee();

        // SECURITY FIX #34: Check absolute nonce limit
        // Prevents attackers from submitting transactions with impossibly high nonces
        if nonce > MAX_ABSOLUTE_NONCE {
            self.stats.total_rejected += 1;
            return Err(PoolError::NonceTooHigh {
                nonce,
                max: MAX_ABSOLUTE_NONCE,
            });
        }

        // Validate fee minimum
        if fee < self.config.min_fee {
            self.stats.total_rejected += 1;
            return Err(PoolError::FeeTooLow(fee, self.config.min_fee));
        }

        // Rate limiting
        if !self.rate_limiter.check(
            &sender,
            self.config.rate_limit_per_account,
            self.config.rate_limit_window,
        ) {
            self.stats.total_rejected += 1;
            return Err(PoolError::RateLimitExceeded);
        }

        // SECURITY FIX #21: Signature verification is MANDATORY in production.
        // The verify_signatures flag is only respected in test builds.
        #[cfg(test)]
        let should_verify = self.config.verify_signatures;
        #[cfg(not(test))]
        let should_verify = true; // ALWAYS verify in production

        if should_verify && !tx.verify() {
            self.stats.total_rejected += 1;
            return Err(PoolError::InvalidSignature);
        }

        // Check per-account limit
        let account_queue = self.account_queues.entry(sender).or_default();
        if account_queue.len() >= self.config.max_per_account {
            // Check if this is a replacement (same nonce, higher fee)
            if let Some(&existing_hash) = account_queue.by_nonce.get(&nonce) {
                return self.try_replace(tx, existing_hash);
            }
            self.stats.total_rejected += 1;
            return Err(PoolError::TooManyPerAccount {
                count: account_queue.len(),
                max: self.config.max_per_account,
            });
        }

        // State validation (if state available)
        if let Some(backend) = state {
            self.validate_against_state(&tx, backend)?;
        }

        // Check if replacement
        if let Some(&existing_hash) = self
            .account_queues
            .get(&sender)
            .and_then(|q| q.by_nonce.get(&nonce))
        {
            return self.try_replace(tx, existing_hash);
        }

        // Pool full - try eviction
        if self.transactions.len() >= self.config.max_size {
            if !self.evict_lowest_fee(fee) {
                self.stats.total_rejected += 1;
                return Err(PoolError::PoolFull);
            }
        }

        // Add to pool
        self.insert_transaction(tx, fee);

        debug!(
            "✅ Transaction {} added to pool (fee={}, total={})",
            hash,
            fee,
            self.transactions.len()
        );

        self.stats.total_added += 1;
        Ok(())
    }

    /// Try to replace an existing transaction (RBF)
    fn try_replace(&mut self, new_tx: SignedTransaction, existing_hash: Hash) -> Result<(), PoolError> {
        let existing_fee = self.fees.get(&existing_hash).copied().unwrap_or(0);
        let new_fee = new_tx.transaction.call.base_fee();

        // Calculate required fee increase (use saturating ops to prevent overflow)
        let min_increase = existing_fee.saturating_mul(self.config.rbf_min_increase_pct as u128) / 100;
        let required_fee = existing_fee.saturating_add(min_increase);

        if new_fee < required_fee {
            self.stats.total_rejected += 1;
            return Err(PoolError::RbfFeeInsufficient {
                need_pct: self.config.rbf_min_increase_pct,
            });
        }

        // Remove old transaction
        self.remove_internal(&existing_hash);

        // Add new transaction
        self.insert_transaction(new_tx.clone(), new_fee);

        info!(
            "🔄 Transaction replaced via RBF: {} -> {} (fee {} -> {})",
            existing_hash,
            new_tx.hash(),
            existing_fee,
            new_fee
        );

        self.stats.total_replaced += 1;
        Ok(())
    }

    /// Validate transaction against chain state
    fn validate_against_state(&self, tx: &SignedTransaction, state: &mut StateBackend) -> Result<(), PoolError> {
        let sender = tx.transaction.sender;
        let nonce = tx.transaction.nonce;
        let fee = tx.transaction.call.base_fee();

        // Get account info
        let account = state.get_account(&sender).ok().flatten().unwrap_or(AccountInfo {
            nonce: 0,
            free: 0,
            reserved: 0,
            last_modified: Hash::ZERO,
        });

        // Check nonce
        let expected_nonce = account.nonce;
        let pool_nonce = self
            .account_queues
            .get(&sender)
            .and_then(|q| q.by_nonce.keys().max().copied())
            .map(|n| n + 1)
            .unwrap_or(expected_nonce);

        let effective_expected = pool_nonce.max(expected_nonce);

        if nonce < expected_nonce {
            return Err(PoolError::NonceTooOld {
                got: nonce,
                current: expected_nonce,
            });
        }

        // SECURITY FIX #14: Limit nonce gap to prevent resource exhaustion
        // Attackers could flood the pending queue with high-nonce transactions
        if nonce > effective_expected + self.config.max_nonce_gap {
            return Err(PoolError::InvalidNonce {
                expected: effective_expected,
                got: nonce,
            });
        }

        // SECURITY FIX #22: Check global pending transaction limit
        // This prevents DoS attacks using many accounts with gap transactions
        let total_pending: usize = self
            .account_queues
            .values()
            .map(|q| q.pending_count())
            .sum();
        if total_pending >= self.config.max_total_pending && nonce > effective_expected {
            return Err(PoolError::TooManyPendingGlobal {
                count: total_pending,
                max: self.config.max_total_pending,
            });
        }

        // Check balance for fee + value
        let value = match &tx.transaction.call {
            TransactionCall::Transfer { amount, .. } => *amount,
            TransactionCall::Stake { amount } => *amount,
            TransactionCall::CreateSidechain { deposit, .. } => *deposit,
            TransactionCall::RegisterValidator { stake } => *stake,
            _ => 0,
        };

        let total_cost = fee.saturating_add(value);
        if account.free < total_cost {
            return Err(PoolError::InsufficientBalance {
                need: total_cost,
                have: account.free,
            });
        }

        Ok(())
    }

    /// Insert transaction into all indexes
    fn insert_transaction(&mut self, tx: SignedTransaction, fee: Balance) {
        let hash = tx.hash();
        let sender = tx.transaction.sender;
        let nonce = tx.transaction.nonce;

        // Add to main storage
        self.transactions.insert(hash, tx.clone());
        self.fees.insert(hash, fee);

        // Add to priority queue
        self.priority_queue.push(PrioritizedTx::new(&tx, fee));

        // Add to account queue
        self.account_queues
            .entry(sender)
            .or_default()
            .add(nonce, hash, fee);
    }

    /// Remove a transaction from the pool
    pub fn remove(&mut self, hash: &Hash) -> Option<SignedTransaction> {
        let tx = self.remove_internal(hash)?;
        self.stats.total_removed += 1;
        debug!(
            "Transaction {} removed from pool (remaining: {})",
            hash,
            self.transactions.len()
        );
        Some(tx)
    }

    /// Internal remove without stats update
    fn remove_internal(&mut self, hash: &Hash) -> Option<SignedTransaction> {
        let tx = self.transactions.remove(hash)?;
        let fee = self.fees.remove(hash).unwrap_or(0);
        let sender = tx.transaction.sender;
        let nonce = tx.transaction.nonce;

        // Remove from account queue
        if let Some(queue) = self.account_queues.get_mut(&sender) {
            queue.remove(nonce, fee);
            if queue.is_empty() {
                self.account_queues.remove(&sender);
            }
        }

        // Remove from pending set
        self.pending.remove(hash);

        // Note: We don't remove from priority_queue (lazy cleanup)
        // Invalid entries are filtered during selection

        Some(tx)
    }

    /// Evict lowest fee transaction to make room
    fn evict_lowest_fee(&mut self, new_fee: Balance) -> bool {
        // Find lowest fee transaction
        let mut lowest_hash = None;
        let mut lowest_fee = new_fee;

        for (hash, &fee) in &self.fees {
            if fee < lowest_fee {
                lowest_fee = fee;
                lowest_hash = Some(*hash);
            }
        }

        if let Some(hash) = lowest_hash {
            self.remove_internal(&hash);
            self.stats.total_evicted += 1;
            info!("Evicted transaction {} (fee={})", hash, lowest_fee);
            true
        } else {
            false
        }
    }

    /// Get a transaction by hash
    pub fn get(&self, hash: &Hash) -> Option<&SignedTransaction> {
        self.transactions.get(hash)
    }

    /// Check if transaction exists
    pub fn contains(&self, hash: &Hash) -> bool {
        self.transactions.contains_key(hash)
    }

    /// Get ready transactions for an account (sequential nonces)
    pub fn ready_transactions(&self, account: &AccountId, current_nonce: u64) -> Vec<SignedTransaction> {
        let queue = match self.account_queues.get(account) {
            Some(q) => q,
            None => return vec![],
        };

        queue
            .ready_hashes(current_nonce)
            .iter()
            .filter_map(|h| self.transactions.get(h).cloned())
            .collect()
    }

    /// Select best transactions for block production
    pub fn select_transactions(&self, max_count: usize) -> Vec<SignedTransaction> {
        let mut selected = Vec::with_capacity(max_count);
        let mut seen_hashes = HashSet::new();
        let mut account_nonces: HashMap<AccountId, u64> = HashMap::new();

        // Clone priority queue for iteration
        let mut heap = self.priority_queue.clone();

        while selected.len() < max_count {
            let entry = match heap.pop() {
                Some(e) => e,
                None => break,
            };

            // Skip if already processed or removed
            if seen_hashes.contains(&entry.hash) {
                continue;
            }
            if !self.transactions.contains_key(&entry.hash) {
                continue;
            }

            let tx = match self.transactions.get(&entry.hash) {
                Some(t) => t,
                None => continue,
            };

            // Check nonce ordering
            let expected_nonce = account_nonces
                .get(&entry.sender)
                .copied()
                .unwrap_or(0);

            if entry.nonce < expected_nonce {
                // Old nonce, skip
                continue;
            }
            if entry.nonce > expected_nonce {
                // Gap in nonces, skip for now (might need earlier tx first)
                continue;
            }

            seen_hashes.insert(entry.hash);
            account_nonces.insert(entry.sender, entry.nonce + 1);
            selected.push(tx.clone());
        }

        selected
    }

    /// Select transactions with state-aware nonce tracking
    pub fn select_transactions_with_state(
        &self,
        max_count: usize,
        state: &mut StateBackend,
    ) -> Vec<SignedTransaction> {
        let mut selected = Vec::with_capacity(max_count);
        let mut account_nonces: HashMap<AccountId, u64> = HashMap::new();
        let mut seen_hashes = HashSet::new();

        let mut heap = self.priority_queue.clone();

        while selected.len() < max_count {
            let entry = match heap.pop() {
                Some(e) => e,
                None => break,
            };

            if seen_hashes.contains(&entry.hash) || !self.transactions.contains_key(&entry.hash) {
                continue;
            }

            let tx = match self.transactions.get(&entry.hash) {
                Some(t) => t,
                None => continue,
            };

            // Get expected nonce from cache or state
            let expected_nonce = if let Some(&n) = account_nonces.get(&entry.sender) {
                n
            } else {
                let account_nonce = state
                    .get_account(&entry.sender)
                    .ok()
                    .flatten()
                    .map(|a| a.nonce)
                    .unwrap_or(0);
                account_nonces.insert(entry.sender, account_nonce);
                account_nonce
            };

            if entry.nonce != expected_nonce {
                continue;
            }

            seen_hashes.insert(entry.hash);
            account_nonces.insert(entry.sender, entry.nonce + 1);
            selected.push(tx.clone());
        }

        selected
    }

    /// Remove all transactions included in a block
    pub fn remove_included(&mut self, block_txs: &[SignedTransaction]) {
        for tx in block_txs {
            if let Some(hash) = tx.hash {
                self.remove(&hash);
            }
        }
    }

    /// Remove stale transactions for an account (nonce < current)
    pub fn remove_stale(&mut self, account: &AccountId, current_nonce: u64) {
        let stale_hashes: Vec<Hash> = self
            .account_queues
            .get(account)
            .map(|q| {
                q.by_nonce
                    .iter()
                    .filter(|(&nonce, _)| nonce < current_nonce)
                    .map(|(_, &hash)| hash)
                    .collect()
            })
            .unwrap_or_default();

        for hash in stale_hashes {
            self.remove_internal(&hash);
            self.stats.total_evicted += 1;
        }
    }

    /// Cleanup expired transactions
    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();
        let expired: Vec<Hash> = self
            .priority_queue
            .iter()
            .filter(|e| now.duration_since(e.added_at) > self.config.tx_expiration)
            .map(|e| e.hash)
            .collect();

        for hash in expired {
            if self.remove_internal(&hash).is_some() {
                self.stats.total_evicted += 1;
                debug!("Expired transaction {} removed", hash);
            }
        }

        // Cleanup rate limiter
        self.rate_limiter.cleanup(self.config.rate_limit_window);
    }

    /// Prune old transactions
    pub fn prune(&mut self, current_block: u64) {
        // Cleanup expired
        self.cleanup_expired();

        // Rebuild priority queue (remove stale entries)
        let valid_hashes: HashSet<_> = self.transactions.keys().copied().collect();
        let mut new_heap = BinaryHeap::new();

        for entry in self.priority_queue.drain() {
            if valid_hashes.contains(&entry.hash) {
                new_heap.push(entry);
            }
        }

        self.priority_queue = new_heap;

        info!(
            "Mempool pruned at block {}: {} transactions remaining",
            current_block,
            self.transactions.len()
        );
    }

    /// Number of transactions
    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    /// Get pool statistics
    pub fn stats(&self) -> &PoolStats {
        &self.stats
    }

    /// Get pending count for an account
    pub fn pending_count(&self, account: &AccountId) -> usize {
        self.account_queues
            .get(account)
            .map(|q| q.len())
            .unwrap_or(0)
    }

    /// Get total pending fees
    pub fn total_fees(&self) -> Balance {
        self.fees.values().sum()
    }

    /// Remove all transactions for an account
    pub fn remove_account_transactions(&mut self, account: &AccountId) -> Vec<SignedTransaction> {
        let hashes: Vec<Hash> = self
            .account_queues
            .get(account)
            .map(|q| q.by_nonce.values().copied().collect())
            .unwrap_or_default();

        hashes
            .iter()
            .filter_map(|h| self.remove(h))
            .collect()
    }

    /// Get configuration
    pub fn config(&self) -> &MempoolConfig {
        &self.config
    }
}

impl Default for TransactionPool {
    fn default() -> Self {
        Self::with_config(MempoolConfig::default())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn create_test_tx(sender: [u8; 32], nonce: u64) -> SignedTransaction {
        create_test_tx_with_fee(sender, nonce, 1000)
    }

    fn create_test_tx_with_fee(sender: [u8; 32], nonce: u64, amount: Balance) -> SignedTransaction {
        let tx = Transaction {
            sender: AccountId::from_bytes(sender),
            nonce,
            call: TransactionCall::Transfer {
                to: AccountId::from_bytes([2; 32]),
                amount, // Fee is based on call type, but we test with different amounts
            },
            timestamp: 0,
        };

        let hash_input = [&sender[..], &nonce.to_le_bytes()[..], &amount.to_le_bytes()[..]].concat();
        SignedTransaction {
            transaction: tx,
            signature: Signature64([0; 64]),
            hash: Some(Hash::hash(&hash_input)),
        }
    }

    fn create_stake_tx(sender: [u8; 32], nonce: u64, amount: Balance) -> SignedTransaction {
        let tx = Transaction {
            sender: AccountId::from_bytes(sender),
            nonce,
            call: TransactionCall::Stake { amount },
            timestamp: 0,
        };

        let hash_input = [&sender[..], &nonce.to_le_bytes()[..], &amount.to_le_bytes()[..]].concat();
        SignedTransaction {
            transaction: tx,
            signature: Signature64([0; 64]),
            hash: Some(Hash::hash(&hash_input)),
        }
    }

    #[test]
    fn test_add_transaction() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        let tx = create_test_tx([1; 32], 0);
        let result = pool.add(tx);
        assert!(result.is_ok());
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_duplicate_transaction() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        let tx = create_test_tx([1; 32], 0);

        pool.add(tx.clone()).unwrap();
        let result = pool.add(tx);
        assert!(matches!(result, Err(PoolError::AlreadyExists)));
    }

    #[test]
    fn test_remove_transaction() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        let tx = create_test_tx([1; 32], 0);
        let hash = tx.hash.unwrap();

        pool.add(tx).unwrap();
        assert_eq!(pool.len(), 1);

        let removed = pool.remove(&hash);
        assert!(removed.is_some());
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn test_ready_transactions() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        let sender = [1; 32];

        // Add 3 transactions with sequential nonces
        for i in 0..3 {
            pool.add(create_test_tx(sender, i)).unwrap();
        }

        let ready = pool.ready_transactions(&AccountId::from_bytes(sender), 0);
        assert_eq!(ready.len(), 3);
    }

    #[test]
    fn test_ready_transactions_with_gap() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        let sender = [1; 32];

        // Add transactions with gap (nonce 0, 1, 3)
        pool.add(create_test_tx(sender, 0)).unwrap();
        pool.add(create_test_tx(sender, 1)).unwrap();
        pool.add(create_test_tx(sender, 3)).unwrap(); // Gap

        let ready = pool.ready_transactions(&AccountId::from_bytes(sender), 0);
        assert_eq!(ready.len(), 2); // Only 0 and 1
    }

    #[test]
    fn test_pool_full_eviction() {
        let mut pool = TransactionPool::new(2);
        pool.config.verify_signatures = false;

        // Add low fee tx first
        pool.add(create_test_tx([1; 32], 0)).unwrap(); // Transfer: 1000 fee
        pool.add(create_test_tx([2; 32], 0)).unwrap(); // Transfer: 1000 fee

        // Pool is full, try adding higher fee tx
        let high_fee_tx = create_stake_tx([3; 32], 0, 10000); // Stake: 5000 fee
        let result = pool.add(high_fee_tx);

        assert!(result.is_ok());
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.stats.total_evicted, 1);
    }

    #[test]
    fn test_select_transactions() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        for i in 0..5 {
            pool.add(create_test_tx([1; 32], i)).unwrap();
        }

        let selected = pool.select_transactions(3);
        assert_eq!(selected.len(), 3);

        // Verify nonce ordering
        assert_eq!(selected[0].transaction.nonce, 0);
        assert_eq!(selected[1].transaction.nonce, 1);
        assert_eq!(selected[2].transaction.nonce, 2);
    }

    #[test]
    fn test_priority_ordering() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        // Different senders with different fee transactions
        pool.add(create_test_tx([1; 32], 0)).unwrap(); // Transfer: 1000
        pool.add(create_stake_tx([2; 32], 0, 10000)).unwrap(); // Stake: 5000
        pool.add(create_test_tx([3; 32], 0)).unwrap(); // Transfer: 1000

        let selected = pool.select_transactions(3);

        // Stake tx should be first (highest fee)
        assert_eq!(selected[0].transaction.sender, AccountId::from_bytes([2; 32]));
    }

    #[test]
    fn test_rate_limiting() {
        let mut pool = TransactionPool::with_config(MempoolConfig {
            rate_limit_per_account: 2,
            rate_limit_window: Duration::from_secs(60),
            verify_signatures: false,
            ..Default::default()
        });

        let sender = [1; 32];

        pool.add(create_test_tx(sender, 0)).unwrap();
        pool.add(create_test_tx(sender, 1)).unwrap();

        // Third should be rate limited
        let result = pool.add(create_test_tx(sender, 2));
        assert!(matches!(result, Err(PoolError::RateLimitExceeded)));
    }

    #[test]
    fn test_per_account_limit() {
        let mut pool = TransactionPool::with_config(MempoolConfig {
            max_per_account: 3,
            verify_signatures: false,
            rate_limit_per_account: 100, // Disable rate limit for this test
            ..Default::default()
        });

        let sender = [1; 32];

        for i in 0..3 {
            pool.add(create_test_tx(sender, i)).unwrap();
        }

        // Fourth should fail
        let result = pool.add(create_test_tx(sender, 3));
        assert!(matches!(result, Err(PoolError::TooManyPerAccount { .. })));
    }

    #[test]
    fn test_replace_by_fee() {
        let mut pool = TransactionPool::with_config(MempoolConfig {
            max_per_account: 3,
            rbf_min_increase_pct: 10,
            verify_signatures: false,
            rate_limit_per_account: 100,
            ..Default::default()
        });

        let sender = [1; 32];

        // Fill up account limit
        for i in 0..3 {
            pool.add(create_test_tx(sender, i)).unwrap();
        }

        // Try to replace nonce 0 with higher fee
        let replacement = create_stake_tx(sender, 0, 10000); // Higher fee
        let result = pool.add(replacement);
        assert!(result.is_ok());
        assert_eq!(pool.stats.total_replaced, 1);
    }

    #[test]
    fn test_remove_stale() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        let sender = [1; 32];
        let account = AccountId::from_bytes(sender);

        // Add transactions with nonces 0, 1, 2
        for i in 0..3 {
            pool.add(create_test_tx(sender, i)).unwrap();
        }

        assert_eq!(pool.len(), 3);

        // Remove stale (nonces < 2)
        pool.remove_stale(&account, 2);

        assert_eq!(pool.len(), 1);
        let remaining = pool.ready_transactions(&account, 2);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].transaction.nonce, 2);
    }

    #[test]
    fn test_stats() {
        let mut pool = TransactionPool::with_config(MempoolConfig {
            min_fee: 10_000, // High minimum to test rejection
            verify_signatures: false,
            ..Default::default()
        });

        let tx = create_stake_tx([1; 32], 0, 10000); // Stake has 5000 fee, meets min
        let hash = tx.hash.unwrap();

        pool.config.min_fee = 1_000; // Lower min for this tx
        pool.add(tx).unwrap();
        assert_eq!(pool.stats().total_added, 1);

        pool.remove(&hash);
        assert_eq!(pool.stats().total_removed, 1);

        // Try low fee tx (Transfer has 1000 fee, below high minimum)
        pool.config.min_fee = 10_000; // Set high minimum
        let low_fee_tx = create_test_tx([2; 32], 0); // Transfer: 1000 fee
        let result = pool.add(low_fee_tx);
        assert!(matches!(result, Err(PoolError::FeeTooLow(_, _))));
        assert_eq!(pool.stats().total_rejected, 1);
    }

    #[test]
    fn test_remove_included() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        let txs: Vec<_> = (0..5)
            .map(|i| create_test_tx([1; 32], i))
            .collect();

        for tx in &txs {
            pool.add(tx.clone()).unwrap();
        }

        assert_eq!(pool.len(), 5);

        // Simulate block including first 3
        pool.remove_included(&txs[0..3]);

        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn test_total_fees() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        pool.add(create_test_tx([1; 32], 0)).unwrap(); // 1000
        pool.add(create_stake_tx([2; 32], 0, 10000)).unwrap(); // 5000

        assert_eq!(pool.total_fees(), 6000);
    }

    #[test]
    fn test_pending_count() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        let sender = AccountId::from_bytes([1; 32]);

        for i in 0..3 {
            pool.add(create_test_tx([1; 32], i)).unwrap();
        }

        assert_eq!(pool.pending_count(&sender), 3);
    }

    #[test]
    fn test_contains() {
        let mut pool = TransactionPool::new(100);
        pool.config.verify_signatures = false;

        let tx = create_test_tx([1; 32], 0);
        let hash = tx.hash.unwrap();

        assert!(!pool.contains(&hash));
        pool.add(tx).unwrap();
        assert!(pool.contains(&hash));
    }
}
