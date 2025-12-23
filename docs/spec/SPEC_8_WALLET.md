# SPEC 8: KratOs Wallet

**Version:** 1.2
**Status:** Normative
**Last Updated:** 2025-12-21

### Changelog
| Version | Date | Changes |
|---------|------|---------|
| 1.2 | 2025-12-21 | Added domain separation for transaction signing (§3.4) - KRATOS_TRANSACTION_V1 prefix |
| 1.1 | 2025-12-21 | Added §14 Node Integration Details (serialization, deserialization, hash computation, RPC architecture) |
| 1.0 | 2025-12-21 | Initial specification |

---

## 1. Overview

The KratOs Wallet is a command-line wallet application for managing KRAT tokens and participating in validator governance during the bootstrap era.

**Design Principles:**
- **Security first:** Strong encryption with AES-256-GCM and Argon2
- **Self-custody:** Keys never leave the user's device
- **Minimal dependencies:** Focused on core functionality
- **Validator integration:** Full support for early validator voting

---

## 2. Architecture

### 2.1 Module Structure

```
rust/kratos-wallet/
├── src/
│   ├── main.rs      # CLI application & menus
│   ├── crypto.rs    # Key management & encryption
│   ├── rpc.rs       # Node communication
│   ├── types.rs     # Data structures
│   ├── storage.rs   # File persistence
│   └── ui.rs        # Terminal formatting
```

### 2.2 Component Diagram

```
┌─────────────────────────────────────────────────────────┐
│                    Wallet Application                    │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐          │
│  │   CLI    │───→│  Crypto  │───→│ Storage  │          │
│  │  (main)  │    │          │    │          │          │
│  └──────────┘    └──────────┘    └──────────┘          │
│       │                               │                  │
│       ▼                               ▼                  │
│  ┌──────────┐                   ┌──────────┐            │
│  │   RPC    │──── HTTP ────────→│  Node    │            │
│  │  Client  │                   │          │            │
│  └──────────┘                   └──────────┘            │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

---

## 3. Cryptography

### 3.1 Key Generation

| Algorithm | Purpose |
|-----------|---------|
| Ed25519 | Signing keys (RFC 8032) |
| OsRng | Cryptographically secure random |

**Key Structure:**
```rust
pub struct WalletKeys {
    signing_key: SigningKey,      // 32 bytes (private)
    verifying_key: VerifyingKey,  // 32 bytes (public)
}
```

### 3.2 Key Derivation

| Parameter | Value |
|-----------|-------|
| Algorithm | Argon2id (default) |
| Salt | 16 bytes random |
| Output | 32 bytes (AES-256 key) |

### 3.3 Wallet Encryption

| Parameter | Value |
|-----------|-------|
| Algorithm | AES-256-GCM |
| Nonce | 12 bytes random |
| Authentication | Built-in (AEAD) |

**Encryption Flow:**
```
Password + Salt → Argon2 → 256-bit Key
Secret Key + Key + Nonce → AES-GCM → Encrypted Secret
```

### 3.4 Transaction Signing

**Domain Separation:**

Transactions use domain-separated signatures to prevent cross-context replay attacks:

```
DOMAIN_TRANSACTION = "KRATOS_TRANSACTION_V1:"

signing_message = DOMAIN_TRANSACTION || bincode::serialize(transaction)
signature = Ed25519.sign(secret_key, signing_message)
```

This ensures a transaction signature cannot be replayed as a block header signature or any other context.

**Signing Flow:**

```
Transaction → bincode serialize → prepend domain → Ed25519 Sign → 64-byte Signature
```

**Signed Transaction:**
```rust
pub struct SignedTransaction {
    pub transaction: Transaction,
    pub signature: [u8; 64],
}
```

---

## 4. Transaction Types

### 4.1 Supported Operations

| Transaction | Description | Fee Estimate |
|-------------|-------------|--------------|
| `Transfer` | Send KRAT to address | ~0.00001 KRAT |
| `ProposeEarlyValidator` | Propose validator candidate | ~0.00005 KRAT |
| `VoteEarlyValidator` | Vote for candidate | ~0.00001 KRAT |

### 4.2 Transaction Structure

```rust
pub struct Transaction {
    pub sender: [u8; 32],       // Public key
    pub nonce: u64,             // Replay protection
    pub call: TransactionCall,  // Operation type
    pub timestamp: u64,         // Unix timestamp
}
```

### 4.3 TransactionCall Variants

```rust
pub enum TransactionCall {
    Transfer {
        to: [u8; 32],
        amount: u128,
    },
    ProposeEarlyValidator {
        candidate: [u8; 32],
    },
    VoteEarlyValidator {
        candidate: [u8; 32],
    },
}
```

---

## 5. Storage

### 5.1 File Locations

| File | Content | Encrypted |
|------|---------|-----------|
| `wallet.json` | Keys + RPC URL | Yes |
| `history.json` | Transaction history | No |

**Default Directory:** `~/.local/share/kratos-wallet/`

### 5.2 Wallet File Format

```rust
pub struct EncryptedWallet {
    pub encrypted_secret: Vec<u8>,  // AES-GCM ciphertext
    pub salt: String,               // Argon2 salt (base64)
    pub nonce: [u8; 12],           // AES-GCM nonce
    pub public_key: [u8; 32],      // For verification
    pub rpc_url: String,           // Node endpoint
    pub version: u32,              // Format version
}
```

### 5.3 File Permissions

| Platform | Permission |
|----------|------------|
| Unix/Linux | 0o600 (owner read/write) |
| Windows | Default ACL |

---

## 6. RPC Integration

### 6.1 JSON-RPC Protocol

| Parameter | Value |
|-----------|-------|
| Version | 2.0 |
| Transport | HTTP POST |
| Format | JSON |

### 6.2 Account Methods

| Method | Parameters | Returns |
|--------|------------|---------|
| `state_getAccount` | address | AccountInfo |
| `state_getNonce` | address | u64 |
| `state_getTransactionHistory` | address, limit, offset | TransactionHistoryResponse |

### 6.3 Chain Methods

| Method | Parameters | Returns |
|--------|------------|---------|
| `chain_getInfo` | - | ChainInfo |

### 6.4 Transaction Methods

| Method | Parameters | Returns |
|--------|------------|---------|
| `author_submitTransaction` | SignedTransaction (JSON) | TransactionSubmitResult |

### 6.5 Validator Methods (Bootstrap Era)

| Method | Parameters | Returns |
|--------|------------|---------|
| `validator_getEarlyVotingStatus` | - | EarlyVotingStatus |
| `validator_getPendingCandidates` | - | PendingCandidatesResponse |
| `validator_getCandidateVotes` | candidate | CandidateVotesResponse |
| `validator_canVote` | account | CanVoteResponse |

---

## 7. User Workflows

### 7.1 Wallet Creation

```
Start → No wallet found
    ↓
Choose: [Import] or [Generate]
    ↓
Import: Enter secret key (64 hex chars)
Generate: Display new keys, require backup confirmation
    ↓
Enter RPC endpoint
    ↓
Set password (with confirmation)
    ↓
Save encrypted wallet
```

### 7.2 Send Transaction

```
Enter recipient address (64 hex chars)
    ↓
Enter amount in KRAT
    ↓
Show summary + confirm
    ↓
Fetch nonce from node
    ↓
Create + sign transaction
    ↓
Submit to node
    ↓
Record in local history
```

### 7.3 Early Validator Voting

```
Check if user is validator (RPC: validator_canVote)
    ↓
If validator: Show voting menu
    ↓
Options:
├── View pending candidates
├── Propose new validator
├── Vote for candidate
└── Check candidate status
```

---

## 8. Security Considerations

### 8.1 Strengths

| Feature | Implementation |
|---------|----------------|
| Key encryption | AES-256-GCM with Argon2 KDF |
| Signing | Ed25519 (NIST-approved) |
| File protection | Unix 0o600 permissions |
| Replay protection | Nonce-based |
| Tamper detection | Public key verification on decrypt |

### 8.2 Known Limitations

| Issue | Risk | Mitigation |
|-------|------|------------|
| Unencrypted history | Privacy leak | Future: encrypt history |
| No password strength check | Weak passwords | User education |
| Memory residue | Key exposure | Future: use zeroize crate |

### 8.3 Best Practices

1. **Backup secret key** before using wallet
2. **Use strong password** (12+ characters, mixed case, symbols)
3. **Verify recipient address** before confirming
4. **Keep wallet updated** for security patches

---

## 9. Data Structures

### 9.1 Account Information

```rust
pub struct AccountInfo {
    pub address: String,
    pub free: u128,       // Available balance
    pub reserved: u128,   // Locked balance
    pub total: u128,      // free + reserved
    pub nonce: u64,       // Transaction count
}
```

### 9.2 Transaction Record

```rust
pub struct TransactionRecord {
    pub hash: String,
    pub direction: TransactionDirection,  // Sent | Received
    pub status: TransactionStatus,        // Pending | Confirmed | Failed
    pub counterparty: String,
    pub amount: u128,
    pub timestamp: u64,
    pub block_number: Option<u64>,
    pub nonce: u64,
    pub note: Option<String>,
}
```

### 9.3 Early Voting Types

```rust
pub struct EarlyVotingStatus {
    pub is_bootstrap_era: bool,
    pub current_block: u64,
    pub bootstrap_end_block: u64,
    pub blocks_until_end: u64,
    pub votes_required: usize,
    pub validator_count: usize,
    pub max_validators: usize,
    pub pending_candidates: usize,
    pub can_add_validators: bool,
}

pub struct EarlyValidatorCandidate {
    pub candidate: String,
    pub proposer: String,
    pub vote_count: usize,
    pub votes_required: usize,
    pub has_quorum: bool,
    pub created_at: u64,
    pub voters: Vec<String>,
}
```

---

## 10. Constants

### 10.1 Token Units

| Constant | Value |
|----------|-------|
| KRAT | 10^12 units |
| Decimals | 12 |

### 10.2 Display Formatting

| Format | Example |
|--------|---------|
| Balance | `1,234.567890123456 KRAT` |
| Address (short) | `0x1234abcd...5678efgh` |
| Timestamp | `2 hours ago` / `Dec 21, 2025` |

---

## 11. Error Handling

### 11.1 RPC Errors

| Error | Meaning |
|-------|---------|
| Network error | Node unreachable |
| HTTP error | Node returned error status |
| Parse error | Invalid response format |
| Empty response | No result in response |

### 11.2 Crypto Errors

| Error | Meaning |
|-------|---------|
| Invalid password | Wrong password or corrupted wallet |
| Cipher error | AES-GCM initialization failed |
| Key verification failed | Tampered wallet file |

### 11.3 Storage Errors

| Error | Meaning |
|-------|---------|
| File not found | Wallet doesn't exist |
| Permission denied | Cannot read/write file |
| Parse error | Corrupted JSON |

---

## 12. CLI Commands

### 12.1 Main Menu

| Option | Description |
|--------|-------------|
| Check Balance | Query account balance from node |
| Send KRAT | Create and submit transfer |
| Transaction History | View past transactions |
| Early Validator Voting | Validator-only voting menu |
| Settings | Account, RPC, password settings |
| Exit | Close wallet |

### 12.2 Validator Menu (Bootstrap Era)

| Option | Description |
|--------|-------------|
| View Pending Candidates | List all candidates with vote counts |
| Propose New Validator | Submit candidacy proposal |
| Vote for Candidate | Cast vote for pending candidate |
| Check Candidate Status | Query specific candidate |

---

## 13. Dependencies

### 13.1 Cryptography

| Crate | Purpose |
|-------|---------|
| ed25519-dalek | Ed25519 signing |
| aes-gcm | Symmetric encryption |
| argon2 | Password key derivation |
| sha2 | Hashing |
| rand | Random generation |

### 13.2 Serialization

| Crate | Purpose |
|-------|---------|
| serde | Serialization framework |
| serde_json | JSON format |
| bincode | Binary format (signing) |
| hex | Hex encoding |

### 13.3 Networking

| Crate | Purpose |
|-------|---------|
| reqwest | HTTP client |
| tokio | Async runtime |

### 13.4 CLI/UI

| Crate | Purpose |
|-------|---------|
| dialoguer | Interactive prompts |
| console | Terminal styling |
| indicatif | Progress indicators |

---

## 14. Node Integration Details

### 14.1 Transaction Serialization (Wallet → Node)

The wallet serializes transactions with hex strings for JSON transport:

```json
{
  "transaction": {
    "sender": "0x1234abcd...",
    "nonce": 0,
    "call": {
      "ProposeEarlyValidator": {
        "candidate": "0x5678efgh..."
      }
    },
    "timestamp": 1703170800
  },
  "signature": "0x084bf5feed0e0ec608f03bf925027c84..."
}
```

### 14.2 Node Deserialization

The node uses custom `Deserialize` implementations to accept both formats:

| Type | Formats Accepted |
|------|------------------|
| `AccountId` | Hex string `"0x..."` or byte array `[u8; 32]` |
| `Signature64` | Hex string `"0x..."` or byte array `[u8; 64]` |

**Implementation:** Custom serde Visitor pattern in `types/signature.rs` and `types/account.rs`

### 14.3 Transaction Hash Computation

Wallet transactions are submitted without a hash (the hash field is `#[serde(skip)]` in bincode).

**Node behavior:**
1. Receives transaction via `author_submitTransaction`
2. Checks if `tx.hash` is `None`
3. If missing, auto-computes: `tx.hash = Some(tx.transaction.hash())`
4. Proceeds with mempool insertion

**Source:** `node/service.rs::submit_transaction()`

### 14.4 RPC Channel Architecture

The node uses a channel-based RPC pattern because libp2p's Swarm is not `Sync`:

```
HTTP Request
     ↓
route_request() → Creates RpcCall variant
     ↓
mpsc::channel → Sends to runner
     ↓
handle_rpc_call() → Processes request
     ↓
oneshot response → Returns to HTTP handler
```

**Key files:**
- `rpc/server.rs` - RpcCall enum, route_request(), handlers
- `cli/runner.rs` - handle_rpc_call() match arms

---

## 15. Related Specifications

- **SPEC 1:** Tokenomics - KRAT token properties
- **SPEC 3:** Consensus - Transaction format and validation
- **SPEC 6:** Network Security - Early validator voting rules
- **Synthesis §20:** Early Validator Voting System
- **Synthesis §24-27:** RPC Architecture and Integration Details
