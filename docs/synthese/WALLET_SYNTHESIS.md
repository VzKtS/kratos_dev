# KratOs Wallet Synthesis

**Version:** 1.1
**Status:** Normative
**Last Updated:** 2025-12-21

---

## 1. Executive Summary

KratOs Wallet is a secure, self-custody CLI wallet for the KratOs blockchain. It provides KRAT token management and validator governance participation during the bootstrap era.

### Key Features

| Feature | Description |
|---------|-------------|
| **Key Management** | Ed25519 generation and import |
| **Encryption** | AES-256-GCM with Argon2 KDF |
| **Transfers** | Send KRAT with replay protection |
| **Validator Voting** | Propose and vote for early validators |
| **History** | Local transaction tracking |

### Security Rating: 7.5/10

**Strengths:** Strong cryptography, file permissions, nonce protection
**Weaknesses:** Unencrypted history, no password strength enforcement

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        KratOs Wallet                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│   │   main.rs   │  │  crypto.rs  │  │  storage.rs │            │
│   │   ───────   │  │  ────────   │  │  ──────────  │            │
│   │ CLI Menus   │  │ Ed25519     │  │ wallet.json │            │
│   │ User Input  │  │ AES-GCM     │  │ history.json│            │
│   │ Workflows   │  │ Argon2      │  │ Permissions │            │
│   └──────┬──────┘  └──────┬──────┘  └──────┬──────┘            │
│          │                │                │                     │
│          ▼                ▼                ▼                     │
│   ┌─────────────┐  ┌─────────────┐  ┌─────────────┐            │
│   │   rpc.rs    │  │  types.rs   │  │   ui.rs     │            │
│   │   ──────    │  │  ────────   │  │  ─────      │            │
│   │ JSON-RPC    │  │ Transaction │  │ Formatting  │            │
│   │ HTTP Client │  │ Account     │  │ Prompts     │            │
│   │ Node API    │  │ History     │  │ Spinners    │            │
│   └──────┬──────┘  └─────────────┘  └─────────────┘            │
│          │                                                       │
│          ▼                                                       │
│   ┌─────────────────────────────────────────────────────────┐  │
│   │                    KratOs Node (RPC)                     │  │
│   │   state_* │ chain_* │ author_* │ validator_*            │  │
│   └─────────────────────────────────────────────────────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. Cryptographic Stack

### 3.1 Algorithm Selection

| Layer | Algorithm | Standard |
|-------|-----------|----------|
| Signing | Ed25519 | RFC 8032 |
| Encryption | AES-256-GCM | NIST SP 800-38D |
| Key Derivation | Argon2id | RFC 9106 |
| Randomness | OsRng | Platform CSPRNG |

### 3.2 Key Generation Flow

```
OsRng (CSPRNG)
    │
    ▼
Ed25519 SigningKey (32 bytes)
    │
    ├──→ Verifying Key (public, 32 bytes)
    │         │
    │         └──→ Account ID (hex encoded)
    │
    └──→ Secret Key (private, stored encrypted)
```

### 3.3 Wallet Encryption Flow

```
User Password
    │
    ├──→ Random Salt (16 bytes)
    │
    ▼
Argon2id(password, salt)
    │
    ▼
Derived Key (32 bytes)
    │
    ├──→ Random Nonce (12 bytes)
    │
    ▼
AES-256-GCM(secret_key, derived_key, nonce)
    │
    ▼
┌────────────────────────────────────┐
│         EncryptedWallet            │
├────────────────────────────────────┤
│ encrypted_secret: Vec<u8>          │
│ salt: String (base64)              │
│ nonce: [u8; 12]                    │
│ public_key: [u8; 32]  ← verification │
│ rpc_url: String                    │
│ version: u32                       │
└────────────────────────────────────┘
    │
    ▼
wallet.json (mode 0o600)
```

### 3.4 Decryption & Verification

```
wallet.json + Password
    │
    ▼
Argon2id(password, stored_salt)
    │
    ▼
Derived Key
    │
    ▼
AES-256-GCM.decrypt(encrypted_secret, derived_key, stored_nonce)
    │
    ▼
Decrypted Secret Key
    │
    ├──→ Derive public key
    │
    ▼
Compare derived_public_key == stored_public_key
    │
    ├── Match ───→ Return keys ✓
    │
    └── Mismatch ─→ "Tampered wallet" ✗
```

---

## 4. Transaction System

### 4.1 Transaction Types

| Type | Purpose | Parameters |
|------|---------|------------|
| `Transfer` | Send KRAT | to, amount |
| `ProposeEarlyValidator` | Propose candidate | candidate |
| `VoteEarlyValidator` | Vote for candidate | candidate |

### 4.2 Transaction Structure

```rust
Transaction {
    sender: [u8; 32],      // Public key (account ID)
    nonce: u64,            // Incremental counter
    call: TransactionCall, // Operation type
    timestamp: u64,        // Unix seconds
}
```

### 4.3 Signing Process

**Domain Separation:**

All transaction signatures use domain separation to prevent cross-context replay attacks:

```
DOMAIN_TRANSACTION = "KRATOS_TRANSACTION_V1:"
```

This prefix is prepended to the serialized transaction before signing, ensuring signatures cannot be reused in other contexts (block headers, governance votes, etc.).

**Signing Flow:**

```
Transaction
    │
    ▼
bincode::serialize()
    │
    ▼
Raw bytes
    │
    ▼
Prepend DOMAIN_TRANSACTION
    │
    ▼
domain_separated_message = "KRATOS_TRANSACTION_V1:" || raw_bytes
    │
    ▼
Ed25519.sign(secret_key, domain_separated_message)
    │
    ▼
SignedTransaction {
    transaction,
    signature: [u8; 64]
}
```

**Implementation (crypto.rs):**

```rust
const DOMAIN_TRANSACTION: &[u8] = b"KRATOS_TRANSACTION_V1:";

fn domain_separate(domain: &[u8], message: &[u8]) -> Vec<u8> {
    let mut separated = Vec::with_capacity(domain.len() + message.len());
    separated.extend_from_slice(domain);
    separated.extend_from_slice(message);
    separated
}

// In create_transfer(), create_propose_early_validator(), etc:
let tx_bytes = bincode::serialize(&transaction).unwrap();
let message = domain_separate(DOMAIN_TRANSACTION, &tx_bytes);
let signature = self.sign(&message);
```

**Source:** `kratos-wallet/src/crypto.rs:17-27, 84-87`

### 4.4 Submission Format (JSON-RPC)

```json
{
  "transaction": {
    "sender": "0x...",
    "nonce": 5,
    "call": {
      "Transfer": {
        "to": "0x...",
        "amount": 1000000000000
      }
    },
    "timestamp": 1734789600
  },
  "signature": "0x..."
}
```

---

## 5. RPC Integration

### 5.1 RPC Client Architecture

```rust
pub struct RpcClient {
    url: String,                 // http://127.0.0.1:9933
    client: reqwest::Client,     // HTTP client
    request_id: AtomicU64,       // Monotonic counter
}
```

### 5.2 Available Methods

#### Account Operations
| Method | Purpose |
|--------|---------|
| `state_getAccount` | Get balance and nonce |
| `state_getNonce` | Get current nonce |
| `state_getTransactionHistory` | Query tx history |

#### Chain Operations
| Method | Purpose |
|--------|---------|
| `chain_getInfo` | Block height, chain name |

#### Transaction Operations
| Method | Purpose |
|--------|---------|
| `author_submitTransaction` | Submit signed tx |

#### Validator Operations (Bootstrap)
| Method | Purpose |
|--------|---------|
| `validator_getEarlyVotingStatus` | Bootstrap era info |
| `validator_getPendingCandidates` | List candidates |
| `validator_getCandidateVotes` | Candidate details |
| `validator_canVote` | Check voting eligibility |

### 5.3 Error Handling

```
RPC Call
    │
    ├── Network Error ──→ "Network error: {}"
    │
    ├── HTTP Error ────→ "HTTP error: {status}"
    │
    ├── Parse Error ───→ "Parse error: {}"
    │
    ├── RPC Error ─────→ error.message
    │
    └── Success ───────→ result
```

---

## 6. Storage System

### 6.1 Storage Locations

| File | Path | Content |
|------|------|---------|
| Wallet | `~/.local/share/kratos-wallet/wallet.json` | Encrypted keys |
| History | `~/.local/share/kratos-wallet/history.json` | Transaction log |

### 6.2 Wallet Storage

```rust
impl WalletStorage {
    // Check existence
    pub fn wallet_exists(&self) -> bool;

    // Create/update
    pub fn save_wallet(&self, keys, password, rpc_url) -> Result<()>;

    // Read
    pub fn load_wallet(&self, password) -> Result<(WalletKeys, String)>;

    // Info without decryption
    pub fn get_wallet_info(&self) -> Result<(public_key, rpc_url)>;
}
```

### 6.3 History Storage

```rust
pub struct TransactionHistory {
    pub transactions: Vec<TransactionRecord>,
    pub last_synced_block: Option<u64>,
}

impl TransactionHistory {
    pub fn add(&mut self, record: TransactionRecord);  // Deduplicates
    pub fn get_page(&self, page: usize, per_page: usize) -> Vec<&TransactionRecord>;
}
```

### 6.4 File Security

| Platform | Implementation |
|----------|----------------|
| Unix | `OpenOptions::mode(0o600)` |
| Windows | Default ACL |
| Write | Truncate + atomic write |

---

## 7. User Interface

### 7.1 Main Menu Structure

```
┌─────────────────────────────────────────┐
│           KratOs Wallet v1.0             │
├─────────────────────────────────────────┤
│                                          │
│  Account: 0x1234abcd...5678efgh         │
│                                          │
│  What would you like to do?              │
│                                          │
│  > 💰 Check Balance                      │
│    📤 Send KRAT                          │
│    📜 Transaction History                │
│    🗳️  Early Validator Voting  [if val] │
│    ⚙️  Settings                          │
│    🚪 Exit                               │
│                                          │
└─────────────────────────────────────────┘
```

### 7.2 Validator Menu (Bootstrap Era)

```
┌─────────────────────────────────────────┐
│        🗳️  Early Validator Voting        │
├─────────────────────────────────────────┤
│                                          │
│  Bootstrap Era Status                    │
│  ──────────────────────────────────────  │
│  Status: ACTIVE                          │
│  Progress: 150,000 / 864,000 blocks      │
│  Validators: 5 / 21                      │
│  Threshold: 3 votes needed               │
│  Candidates: 2 pending                   │
│                                          │
│  > 📋 View Pending Candidates            │
│    ➕ Propose New Validator              │
│    ✅ Vote for Candidate                 │
│    🔍 Check Candidate Status             │
│    ⬅️  Back to Main Menu                 │
│                                          │
└─────────────────────────────────────────┘
```

### 7.3 UI Components

| Component | Crate | Usage |
|-----------|-------|-------|
| Select menu | dialoguer | Menu navigation |
| Input | dialoguer | Text entry |
| Confirm | dialoguer | Yes/No prompts |
| Spinner | indicatif | Loading indicators |
| Style | console | Colors and formatting |

### 7.4 Display Formatting

| Type | Format | Example |
|------|--------|---------|
| Balance | Comma separated | `1,234.567890 KRAT` |
| Address | Shortened | `0x1234...5678` |
| Time (recent) | Relative | `2 hours ago` |
| Time (old) | Absolute | `Dec 21, 2025` |
| Amount sent | Red | `-100.00 KRAT` |
| Amount received | Green | `+100.00 KRAT` |

---

## 8. Workflow Details

### 8.1 Wallet Setup

```
┌──────────────────────────────────────────────────────────────┐
│                    First Time Setup                           │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
               ┌──────────────────────────┐
               │  Import or Generate?      │
               └──────────────────────────┘
                    │              │
            [Import]│              │[Generate]
                    ▼              ▼
        ┌─────────────────┐  ┌─────────────────┐
        │ Enter secret    │  │ Generate random │
        │ (64 hex chars)  │  │ Ed25519 keypair │
        └────────┬────────┘  └────────┬────────┘
                 │                     │
                 │                     ▼
                 │           ┌─────────────────┐
                 │           │ Display keys    │
                 │           │ (BACKUP NOW!)   │
                 │           └────────┬────────┘
                 │                     │
                 │                     ▼
                 │           ┌─────────────────┐
                 │           │ Confirm backup  │
                 │           └────────┬────────┘
                 │                     │
                 └──────────┬──────────┘
                            ▼
               ┌──────────────────────────┐
               │  Enter RPC endpoint       │
               │  (http://127.0.0.1:9933) │
               └──────────────────────────┘
                            │
                            ▼
               ┌──────────────────────────┐
               │  Set password             │
               │  (with confirmation)      │
               └──────────────────────────┘
                            │
                            ▼
               ┌──────────────────────────┐
               │  Encrypt & save wallet   │
               └──────────────────────────┘
                            │
                            ▼
               ┌──────────────────────────┐
               │  Ready to use ✓          │
               └──────────────────────────┘
```

### 8.2 Send Transaction

```
┌──────────────────────────────────────────────────────────────┐
│                    Send KRAT Flow                             │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
               ┌──────────────────────────┐
               │  Enter recipient address  │
               │  (validate: 64 hex chars) │
               └──────────────────────────┘
                              │
                              ▼
               ┌──────────────────────────┐
               │  Enter amount in KRAT     │
               │  (validate: positive num) │
               └──────────────────────────┘
                              │
                              ▼
               ┌──────────────────────────┐
               │  Show summary:            │
               │  - Recipient (short)      │
               │  - Amount                 │
               │  - Estimated fee          │
               └──────────────────────────┘
                              │
                              ▼
               ┌──────────────────────────┐
               │  Confirm? [y/N]           │
               └──────────────────────────┘
                    │              │
              [Yes] │              │ [No]
                    ▼              ▼
        ┌─────────────────┐  ┌─────────────┐
        │ Fetch nonce     │  │ Cancelled   │
        │ from node       │  └─────────────┘
        └────────┬────────┘
                 │
                 ▼
        ┌─────────────────┐
        │ Create tx with  │
        │ nonce + timestamp│
        └────────┬────────┘
                 │
                 ▼
        ┌─────────────────┐
        │ Sign with       │
        │ Ed25519 key     │
        └────────┬────────┘
                 │
                 ▼
        ┌─────────────────┐
        │ Submit via RPC  │
        │ author_submit   │
        └────────┬────────┘
                 │
                 ▼
        ┌─────────────────┐
        │ Record in local │
        │ history.json    │
        └────────┬────────┘
                 │
                 ▼
        ┌─────────────────┐
        │ Display hash    │
        │ and status ✓    │
        └─────────────────┘
```

### 8.3 Early Validator Voting

```
┌──────────────────────────────────────────────────────────────┐
│                Vote for Candidate Flow                        │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
               ┌──────────────────────────┐
               │  Load pending candidates  │
               │  via RPC                  │
               └──────────────────────────┘
                              │
                              ▼
               ┌──────────────────────────┐
               │  Display candidate list   │
               │  with vote progress       │
               │                           │
               │  1. 0x1234... (2/3 votes) │
               │  2. 0x5678... (1/3 votes) │
               │  Cancel                   │
               └──────────────────────────┘
                              │
                              ▼
               ┌──────────────────────────┐
               │  Select candidate         │
               └──────────────────────────┘
                              │
                              ▼
               ┌──────────────────────────┐
               │  Check if already voted   │
               └──────────────────────────┘
                    │              │
           [Already]│              │[Not yet]
                    ▼              ▼
        ┌─────────────────┐  ┌─────────────────┐
        │ "Already voted" │  │ Show vote       │
        │ message         │  │ summary         │
        └─────────────────┘  └────────┬────────┘
                                      │
                                      ▼
                          ┌──────────────────────────┐
                          │  Confirm vote? [y/N]      │
                          └──────────────────────────┘
                                      │
                                [Yes] ▼
                          ┌──────────────────────────┐
                          │  Create VoteEarlyValidator│
                          │  transaction              │
                          └──────────────────────────┘
                                      │
                                      ▼
                          ┌──────────────────────────┐
                          │  Submit via RPC           │
                          └──────────────────────────┘
                                      │
                                      ▼
                          ┌──────────────────────────┐
                          │  Check if deciding vote   │
                          │  (vote_count+1 >= required)│
                          └──────────────────────────┘
                                 │           │
                          [Yes]  │           │ [No]
                                 ▼           ▼
                    ┌─────────────────┐ ┌──────────────┐
                    │ "Deciding vote!"│ │ "Vote       │
                    │ "Candidate will │ │  submitted" │
                    │  be approved"   │ └──────────────┘
                    └─────────────────┘
```

---

## 9. Security Analysis

### 9.1 Threat Model

| Threat | Mitigation | Status |
|--------|------------|--------|
| Key theft (file) | AES-256-GCM encryption | ✓ |
| Key theft (memory) | Rust ownership | Partial |
| Weak password | Argon2 (slow hash) | Partial |
| Replay attack | Nonce-based | ✓ |
| MITM (RPC) | User verifies endpoint | Manual |
| Tampered wallet | Public key verification | ✓ |
| Transaction analysis | History unencrypted | ✗ |

### 9.2 Security Invariants

1. **Secret key never leaves device** unencrypted
2. **Every transaction requires signature** with valid key
3. **Nonce prevents replay** of old transactions
4. **Decryption verifies integrity** via public key match
5. **File permissions restrict** unauthorized access

### 9.3 Recommendations

| Priority | Issue | Recommendation |
|----------|-------|----------------|
| High | Unencrypted history | Encrypt history.json |
| Medium | No password policy | Enforce minimum strength |
| Medium | Memory residue | Use `zeroize` crate |
| Low | RPC URL exposure | Encrypt with wallet |

---

## 10. API Reference

### 10.1 WalletKeys API

```rust
impl WalletKeys {
    // Creation
    pub fn generate() -> Self;
    pub fn from_secret(secret: [u8; 32]) -> Self;

    // Accessors
    pub fn account_id_hex(&self) -> String;
    pub fn account_id_bytes(&self) -> [u8; 32];
    pub fn secret_key_hex(&self) -> String;      // Use with caution
    pub fn secret_key_bytes(&self) -> [u8; 32];  // Use with caution

    // Signing
    pub fn sign(&self, message: &[u8]) -> [u8; 64];

    // Transaction creation
    pub fn create_transfer(&self, to: [u8; 32], amount: u128, nonce: u64) -> SignedTransaction;
    pub fn create_propose_early_validator(&self, candidate: [u8; 32], nonce: u64) -> SignedTransaction;
    pub fn create_vote_early_validator(&self, candidate: [u8; 32], nonce: u64) -> SignedTransaction;
}
```

### 10.2 RpcClient API

```rust
impl RpcClient {
    // Creation
    pub fn new(url: &str) -> Self;

    // Account
    pub fn get_account(&self, address: &str) -> Result<AccountInfo, String>;
    pub fn get_nonce(&self, address: &str) -> Result<u64, String>;

    // Chain
    pub fn get_block_height(&self) -> Result<u64, String>;

    // Transactions
    pub fn submit_transaction(&self, tx: &SignedTransaction) -> Result<TransactionSubmitResult, String>;
    pub fn get_transaction_history(&self, address: &str, limit: u32, offset: u32) -> Result<TransactionHistoryResponse, String>;

    // Validator (bootstrap)
    pub fn get_early_voting_status(&self) -> Result<EarlyVotingStatus, String>;
    pub fn get_pending_candidates(&self) -> Result<PendingCandidatesResponse, String>;
    pub fn get_candidate_votes(&self, candidate: &str) -> Result<CandidateVotesResponse, String>;
    pub fn can_vote(&self, account: &str) -> Result<CanVoteResponse, String>;
    pub fn submit_propose_early_validator(&self, tx: &SignedTransaction) -> Result<TransactionSubmitResult, String>;
    pub fn submit_vote_early_validator(&self, tx: &SignedTransaction) -> Result<TransactionSubmitResult, String>;
}
```

### 10.3 Storage API

```rust
impl WalletStorage {
    pub fn new(wallet_dir: &Path) -> Self;
    pub fn wallet_exists(&self) -> bool;
    pub fn save_wallet(&self, keys: &WalletKeys, password: &str, rpc_url: &str) -> Result<(), String>;
    pub fn load_wallet(&self, password: &str) -> Result<(WalletKeys, String), String>;
    pub fn delete_wallet(&self) -> Result<(), String>;
    pub fn get_wallet_info(&self) -> Result<(String, String), String>;

    pub fn load_history(&self) -> TransactionHistory;
    pub fn save_history(&self, history: &TransactionHistory) -> Result<(), String>;
    pub fn add_transaction(&self, record: TransactionRecord) -> Result<(), String>;
    pub fn clear_history(&self) -> Result<(), String>;
}
```

---

## 11. Testing

### 11.1 Unit Tests

| Module | Coverage |
|--------|----------|
| crypto.rs | Key generation, signing, encrypt/decrypt |
| types.rs | Serialization |
| storage.rs | Save/load, deduplication |
| rpc.rs | Client creation, ID increment |
| ui.rs | Formatting functions |

### 11.2 Test Commands

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific module
cargo test crypto::tests
```

---

## 12. Building & Installation

### 12.1 Build Requirements

| Requirement | Version |
|-------------|---------|
| Rust | 1.70+ |
| Cargo | 1.70+ |

### 12.2 Build Commands

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Binary location
./target/release/kratos-wallet
```

### 12.3 First Run

```bash
# Run wallet
./kratos-wallet

# Will prompt for:
# 1. Import or generate keys
# 2. RPC endpoint
# 3. Password
```

---

## 13. Document History

| Date | Version | Change |
|------|---------|--------|
| 2025-12-21 | 1.1 | Added domain separation for transaction signing (§4.3) - KRATOS_TRANSACTION_V1 prefix implementation |
| 2025-12-21 | 1.0 | Initial wallet synthesis document |

---

## 14. Related Documents

- **SPEC 8:** Wallet - Technical specification
- **SPEC 1:** Tokenomics - KRAT token properties
- **SPEC 3:** Consensus - Transaction validation
- **Synthesis §20:** Early Validator Voting System
