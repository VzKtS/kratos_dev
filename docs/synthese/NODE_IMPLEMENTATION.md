# KratOs Node Implementation - Complete Guide

## Overview

KratOs is a **native Rust blockchain implementation** built from scratch. It does NOT use Substrate or any blockchain framework. All layers (consensus, networking, storage, RPC) are custom-built using standard Rust libraries.

**Source Code**: `rust/kratos-core/`

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         kratos-node                                  │
│                                                                      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐              │
│  │     CLI      │  │   Service    │  │     RPC      │              │
│  │   (clap)     │  │   (tokio)    │  │   (warp)     │              │
│  └──────────────┘  └──────────────┘  └──────────────┘              │
│         │                  │                  │                      │
│  ┌──────▼──────────────────▼──────────────────▼───────┐            │
│  │                    Node Service                     │            │
│  │  ┌─────────────┐ ┌─────────────┐ ┌──────────────┐  │            │
│  │  │  Producer   │ │   Mempool   │ │   Network    │  │            │
│  │  │(VRF-based)  │ │(Tx Pool)    │ │  (libp2p)    │  │            │
│  │  └─────────────┘ └─────────────┘ └──────────────┘  │            │
│  └────────────────────────────────────────────────────┘            │
│                            │                                         │
│  ┌─────────────────────────▼─────────────────────────┐             │
│  │                 Consensus Layer                    │             │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐          │             │
│  │  │   PoS    │ │   VRF    │ │ Validator│          │             │
│  │  │ (credits)│ │Selection │ │ Credits  │          │             │
│  │  └──────────┘ └──────────┘ └──────────┘          │             │
│  └───────────────────────────────────────────────────┘             │
│                            │                                         │
│  ┌─────────────────────────▼─────────────────────────┐             │
│  │                  Storage Layer                     │             │
│  │  ┌──────────────┐     ┌──────────────┐            │             │
│  │  │   RocksDB    │     │ State Trie   │            │             │
│  │  │   (blocks)   │     │  (accounts)  │            │             │
│  │  └──────────────┘     └──────────────┘            │             │
│  └───────────────────────────────────────────────────┘             │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Core Components

### 1. Main Entry Point

**Location**: `src/main.rs`

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run { dev, validator, ... } => {
            runner::run_node(config).await?;
        }
        Commands::Key { subcommand } => { ... }
        Commands::Info { ... } => { ... }
        Commands::Purge { ... } => { ... }
        Commands::Export { ... } => { ... }
    }
}
```

**CLI Commands**:
- `run` - Start the node (with `--genesis` to create new network, without to join existing)
- `key generate` - Generate new keypair
- `key inspect` - Inspect existing key
- `info` - Display node information
- `purge` - Delete chain database
- `export` - Export blockchain data

**Starting Modes**:

| Mode | Command | Description |
|------|---------|-------------|
| **Genesis** | `kratos-node run --genesis --validator` | Create a new network (genesis node) |
| **Join** | `kratos-node run` | Join existing network via DNS Seeds / bootnodes |
| **Join (explicit)** | `kratos-node run --bootnode /ip4/.../p2p/...` | Join with specific bootnode |

**Startup Sequence - Genesis Exchange Protocol**:

When a node joins without `--genesis`, it follows this critical sequence:

```
1. Check for existing genesis in local database
   │
   ├─[Found] → Use stored genesis, start normally
   │
   └─[Not Found] → Genesis Exchange Protocol:
       │
       ├─ Connect to network (DNS Seeds / Bootnodes)
       ├─ Wait for peer connection
       ├─ Send GenesisRequest to first connected peer
       ├─ Wait for GenesisResponse (30s timeout)
       ├─ Validate received genesis block
       └─ Initialize chain with received genesis
```

This ensures all nodes share the **same genesis hash** - nodes with different genesis hashes cannot sync.

### 2. CLI Configuration

**Location**: `src/cli/mod.rs`

```rust
#[derive(Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Run {
        /// Genesis mode - create new network (no peer discovery)
        #[arg(long)]
        genesis: bool,

        #[arg(long)]
        validator: bool,

        #[arg(long, default_value = "30333")]
        port: u16,

        #[arg(long, default_value = "9933")]
        rpc_port: u16,

        #[arg(long)]
        base_path: Option<PathBuf>,

        #[arg(long)]
        bootnodes: Vec<String>,
    },
    // ...
}
```

### 3. Node Service

**Location**: `src/node/service.rs`

The node service orchestrates all components:

```rust
pub struct KratOsNode {
    /// Chain storage
    chain: Arc<RwLock<Chain>>,

    /// State backend
    state: Arc<RwLock<StateBackend>>,

    /// Transaction mempool
    mempool: Arc<TransactionPool>,

    /// Validator set
    validators: Arc<RwLock<ValidatorSet>>,

    /// Network service
    network: Arc<NetworkService>,

    /// Database
    db: Arc<Database>,
}
```

### 4. Block Producer

**Location**: `src/node/producer.rs`

VRF-based block production with dynamic rewards:

```rust
/// Block production configuration
pub struct ProducerConfig {
    pub max_transactions_per_block: usize,  // 1000
    pub max_block_size: usize,              // 5 MB
    pub block_reward: Balance,              // 10 KRAT (fallback)
    pub use_dynamic_rewards: bool,          // true
    pub fee_distribution: FeeDistribution,  // 60/30/10
}

/// Calculate dynamic block reward
pub fn calculate_block_reward(
    current_epoch: EpochNumber,
    total_supply: Balance,
    bootstrap_config: &BootstrapConfig,
) -> Balance {
    let inflation = if bootstrap_config.is_bootstrap(current_epoch) {
        bootstrap_config.target_inflation  // 6.5%
    } else {
        calculate_post_bootstrap_inflation(current_epoch)
    };

    let annual_emission = (total_supply as f64 * inflation) as Balance;
    annual_emission / BLOCKS_PER_YEAR  // ~12.37 KRAT during bootstrap
}
```

### 5. Mempool

**Location**: `src/node/mempool.rs`

Transaction pool management:

```rust
pub struct TransactionPool {
    pending: BTreeMap<AccountId, BTreeMap<Nonce, SignedTransaction>>,
    queued: VecDeque<SignedTransaction>,
    max_pool_size: usize,
}

impl TransactionPool {
    pub fn add(&mut self, tx: SignedTransaction) -> Result<(), MempoolError>;
    pub fn get_pending(&self, limit: usize) -> Vec<SignedTransaction>;
    pub fn remove(&mut self, tx_hash: &Hash);
}
```

---

## Consensus Layer

### Proof of Stake with VRF Selection

**Location**: `src/consensus/`

KratOs uses a custom PoS system with VRF (Verifiable Random Function) for fair leader selection.

#### VRF Slot Selection

**File**: `src/consensus/vrf_selection.rs`

```rust
pub struct VRFSelector {
    validators: Vec<ValidatorInfo>,
}

impl VRFSelector {
    /// Select block producer for a slot
    pub fn select_producer(
        &self,
        slot: SlotNumber,
        epoch_randomness: &[u8; 32],
    ) -> Option<AccountId> {
        // VRF output determines selection
        let vrf_input = [epoch_randomness, &slot.to_le_bytes()].concat();
        let selection_value = blake3::hash(&vrf_input);

        // Weighted selection based on stake + VC
        self.select_by_weight(selection_value)
    }
}
```

#### Validator Credits System

**File**: `src/consensus/validator_credits.rs`

```rust
pub struct ValidatorCreditsRecord {
    pub validator_id: AccountId,
    pub total_credits: u64,
    pub uptime_score: f64,
    pub arbitration_score: f64,
    pub governance_score: f64,
}
```

Credits are earned through:
- Block production (+10 VC)
- Voting participation (+5 VC)
- Arbitration service (+20 VC)
- Uptime bonuses (+1-5 VC/epoch)

### Epoch Management

**File**: `src/consensus/epoch.rs`

```rust
/// 1 epoch = 600 blocks = 1 hour (at 6s/block)
pub const EPOCH_DURATION_BLOCKS: BlockNumber = 600;

/// Slot duration in seconds
pub const SLOT_DURATION_SECS: u64 = 6;

pub struct EpochConfig {
    pub number: EpochNumber,
    pub start_block: BlockNumber,
    pub end_block: BlockNumber,
    pub total_slots: SlotNumber,
}
```

### Validation

**File**: `src/consensus/validation.rs`

Block validation checks:
- Header hash integrity
- Parent hash exists
- Correct block height
- Valid VRF proof
- Valid proposer signature
- State root matches
- Transactions valid
- Block size limits

### GRANDPA Finality

**Location**: `src/consensus/finality/`

KratOs uses a GRANDPA-style Byzantine Fault Tolerant finality gadget:

```rust
/// Finality round state machine
pub enum RoundState {
    Prevoting,      // Collecting prevotes
    Precommitting,  // 2/3 prevotes reached, collecting precommits
    Completed,      // 2/3 precommits reached, block finalized
    Failed,         // Round timed out
}

/// Finality vote structure
pub struct FinalityVote {
    pub vote_type: VoteType,  // Prevote or Precommit
    pub target_number: BlockNumber,
    pub target_hash: Hash,
    pub round: u32,
    pub epoch: EpochNumber,
    pub voter: AccountId,
    pub signature: Signature64,
}
```

**Key Components:**

| File | Purpose |
|------|---------|
| `finality/mod.rs` | Module structure, supermajority helpers |
| `finality/types.rs` | FinalityVote, VoteType, EquivocationProof |
| `finality/votes.rs` | VoteCollector with equivocation detection |
| `finality/rounds.rs` | FinalityRound, RoundManager |
| `finality/gadget.rs` | FinalityGadget coordinator |

**Protocol Flow:**
```
Prevoting Phase          Precommitting Phase          Finalized
     │                          │                          │
     ▼                          ▼                          ▼
 Validators vote    →    Commit with 2/3    →    Block is
 on best chain           prevote target          finalized
     │                          │                          │
     └── 2/3 prevotes ──────────┘── 2/3 precommits ────────┘
```

**Supermajority Threshold:**
```rust
pub fn has_supermajority(count: usize, total: usize) -> bool {
    count * 100 >= total * 66  // 2/3 = 66%
}
```

**Configuration:**

| Parameter | Value |
|-----------|-------|
| Round timeout | 6 seconds |
| Minimum validators | 3 |
| Threshold | 66% (2/3) |
| Domain separation | `KRATOS_FINALITY_V1:` |

**See Also:** [grandpa-finality.svg](../diagrams/grandpa-finality.svg)

### Node-Level Finality Integration

**Location**: `src/node/finality_integration.rs`

The finality gadget is coordinated with the node via `FinalityIntegration`:

```rust
/// Coordinates finality gadget with node operations
pub struct FinalityIntegration<S: FinalitySigner, B: FinalityBroadcaster> {
    /// The finality gadget
    gadget: RwLock<FinalityGadget<S, B>>,

    /// Voters from last finalization (for reward distribution)
    last_finality_voters: RwLock<Vec<AccountId>>,

    /// Last finalized block number
    last_finalized: RwLock<BlockNumber>,

    /// Whether finality is active (requires MIN_VALIDATORS_FOR_FINALITY)
    is_active: RwLock<bool>,
}
```

**Key Components:**

| Component | File | Purpose |
|-----------|------|---------|
| `NodeFinalitySigner` | `finality_integration.rs` | Signs votes with validator key (closure pattern for security) |
| `NodeFinalityBroadcaster` | `finality_integration.rs` | Broadcasts votes via unbounded channel to network |
| `FinalityIntegration` | `finality_integration.rs` | Coordinates gadget lifecycle with node |

**Initialization Flow:**

```
Node Startup
     │
     ▼
initialize_finality(validator_key)
     │
     ├─ Create NodeFinalitySigner with signing closure
     ├─ Create NodeFinalityBroadcaster with channel
     ├─ Build validator set from current validators
     ├─ Create FinalityIntegration
     │
     ▼
Finality Active (if >= 3 validators)
```

**Event Loop Integration:**

```rust
// In runner.rs main loop
loop {
    tokio::select! {
        // ... other events ...

        // Finality tick (every SLOT_DURATION_SECS)
        _ = finality_tick_interval.tick() => {
            if config.validator && validator_key.is_some() {
                node.tick_finality().await;           // Handle timeouts
                node.broadcast_finality_messages().await;  // Send queued votes
            }
        }
    }
}
```

**Network Message Handling:**

```rust
// In service.rs - handle_network_event()
NetworkEvent::FinalityVoteReceived(vote) => {
    if let Some(voters) = self.process_finality_vote(vote).await {
        // Voters list available for reward distribution
        info!("Block finalized with {} voters", voters.len());
    }
}
```

**Fee Distribution Integration:**

When a block is finalized, the precommit voters receive 10% of fees:

```
Block Finalized
     │
     ▼
get_last_finality_voters() → Vec<AccountId>
     │
     ▼
distribute_fees(total_fees, producer, finality_voters)
     │
     ├─ 50% → Block Producer
     ├─ 10% → Finality Voters (shared equally)
     ├─ 30% → Burn
     └─ 10% → Treasury
```

---

## Network Layer

### libp2p Networking

**Location**: `src/network/`

Built on libp2p with custom protocols:

```rust
pub struct NetworkService {
    swarm: Swarm<KratosBehaviour>,
    local_peer_id: PeerId,
    peers: HashMap<PeerId, PeerInfo>,
}

/// Network behavior combining all protocols
pub struct KratosBehaviour {
    pub gossipsub: gossipsub::Behaviour,    // Block/tx propagation
    pub request_response: Behaviour,         // Direct peer queries
    pub kademlia: kad::Behaviour,            // Peer discovery (DHT)
}
```

### Network Protocols

| Protocol | Purpose |
|----------|---------|
| **Gossipsub** | Block and transaction propagation |
| **Request-Response** | Direct peer queries (sync, status, genesis) |
| **Kademlia DHT** | Distributed peer discovery |

### Protocol Topics

- `/kratos/blocks/1.0.0` - New block announcements
- `/kratos/transactions/1.0.0` - Transaction propagation
- `/kratos/sync/1.0.0` - Chain synchronization
- `/kratos/consensus/1.0.0` - Consensus messages
- `/kratos/finality/1.0.0` - Finality votes and justifications

### Default Ports

| Port | Service |
|------|---------|
| 30333 | P2P networking (libp2p) |
| 30334 | DNS Seed heartbeat (TCP) |
| 8080 | DNS Seed HTTP API |
| 9933 | JSON-RPC HTTP (default) |

### Peer Discovery - DNS Seeds

**Location**: `src/network/dns_seeds.rs`, `src/network/dns_seed_client.rs`

KratOs implements decentralized peer discovery via DNS Seeds. When a node starts, it automatically queries DNS seeds to find active peers without manual configuration.

**Discovery Order:**
```
1. DNS Seeds (IDpeers.json)       → Fetch signed peer lists via HTTP
2. Fallback Bootnodes             → Hardcoded peers always included
3. CLI Bootnodes                  → Use --bootnode /ip4/.../p2p/...
4. Kademlia DHT                   → Learn peers from connected nodes
```

**Official DNS Seeds:**

| Seed | IP Address | Region |
|------|------------|--------|
| seed1 | 5.189.184.205 | EU |
| seed2 | 45.8.132.252 | EU |
| seed3 | 74.208.14.99 | US |

**DNS Seed Client:**

```rust
// In network/dns_seed_client.rs

/// Fetch peer list from DNS Seed
pub async fn fetch_peers_from_dns_seed(seed_url: &str) -> Result<Vec<PeerInfo>> {
    let client = reqwest::Client::new();
    let response = client.get(format!("{}/peers", seed_url))
        .timeout(Duration::from_secs(10))
        .send()
        .await?;

    let id_peers: IdPeersFile = response.json().await?;

    // Verify Ed25519 signature before trusting peer list
    verify_idpeers_signature(&id_peers)?;

    Ok(id_peers.peers)
}
```

**IDpeers.json Format (received from DNS Seeds):**

```json
{
  "version": 1,
  "generated_at": "2025-12-23T10:00:00Z",
  "ttl_seconds": 300,
  "network_state": "Normal",
  "peers": [
    {
      "peer_id": "a1b2c3d4...(64 hex chars)",
      "libp2p_peer_id": "12D3KooWSpAybJ2D9DAt74StZYE4NA7Dbx9W9DxgyANyjT6g6GPn",
      "addresses": ["/ip4/45.8.132.252/tcp/30333"],
      "score": 950,
      "is_validator": true,
      "current_height": 12345,
      "last_seen": 1703329200
    }
  ],
  "signature": "hex-encoded-ed25519-signature",
  "dns_seed_id": "hex-encoded-public-key"
}
```

**Peer Fields:**
- `peer_id`: Ed25519 public key (32 bytes hex) for signature verification
- `libp2p_peer_id`: Base58 PeerId for multiaddr connection (used in `/ip4/.../tcp/.../p2p/<libp2p_peer_id>`)
- `addresses`: Multiaddr list where the peer is reachable

**Integration at Startup:**

```rust
// In node/service.rs - KratOsNode::new()

// 1. Fetch peers from DNS Seeds (signed IDpeers.json)
for seed_ip in OFFICIAL_DNS_SEEDS {
    let seed_url = format!("http://{}:8080", seed_ip);
    if let Ok(peers) = fetch_peers_from_dns_seed(&seed_url).await {
        bootstrap_addrs.extend(peers);
    }
}

// 2. Always include fallback bootnodes
for bootnode in FALLBACK_BOOTNODES {
    bootstrap_addrs.push(parse_bootnode(bootnode)?);
}

// 3. Add CLI bootnodes
for bootnode in &config.network.bootnodes {
    bootstrap_addrs.push(parse_bootnode(bootnode)?);
}

// 4. Register all peers with network
network.add_bootstrap_nodes(bootstrap_addrs);
```

**Heartbeat Protocol (Node → DNS Seed):**

Nodes periodically send signed heartbeats to DNS Seeds to register themselves:

```rust
pub struct HeartbeatMessage {
    pub version: u32,              // Protocol version (currently 1)
    pub peer_id: [u8; 32],         // Ed25519 public key (for signature verification)
    pub libp2p_peer_id: String,    // libp2p PeerId base58 (for peer discovery)
    pub addresses: Vec<String>,    // Multiaddr list (e.g., ["/ip4/1.2.3.4/tcp/30333"])
    pub current_height: u64,       // Current block height
    pub best_hash: [u8; 32],       // Best block hash
    pub genesis_hash: [u8; 32],    // Genesis hash (for chain validation)
    pub is_validator: bool,        // Is active validator
    pub validator_count: Option<u32>,
    pub total_stake: Option<u128>,
    pub protocol_version: u32,
    pub timestamp: u64,            // Unix timestamp
    pub signature: [u8; 64],       // Ed25519 with domain separation
}
```

**Key Fields:**
- `peer_id`: Ed25519 public key (32 bytes) for **signature verification**
- `libp2p_peer_id`: Base58 PeerId (e.g., `12D3KooW...`) for **peer connection**

**Heartbeat Flow:**
```
Node                              DNS Seed (port 30334)
  │                                     │
  │─────── TCP Connect ────────────────►│
  │                                     │
  │─────── HeartbeatMessage ───────────►│
  │        (signed with node key)       │
  │                                     │
  │◄────── NetworkStateInfo ────────────│
  │        (current security state)     │
  │                                     │
  └───── Repeat every 2 minutes ────────┘
```

**Becoming a DNS Seed Operator:**
1. Deploy the `kratos-dns-seed` application
2. Configure heartbeat receiver (TCP port 30334)
3. Maintain 99.9% uptime for 30 days
4. Submit PR to add seed to official list
5. Pass community review for independence

**See Also:** [SPEC 6 §20 - DNS Seed Service](../spec/SPEC_6_NETWORK_SECURITY.md#20-dns-seed-service-kratos-dns-seed)

### Network Identity Persistence

**Location**: `src/network/service.rs`

Each node has a persistent PeerId derived from an Ed25519 keypair stored on disk:

```rust
// Network identity is saved to: <data_dir>/network/network_key

fn load_or_generate_keypair(data_dir: Option<&PathBuf>) -> Result<Keypair, Error> {
    if let Some(dir) = data_dir {
        let key_path = dir.join("network").join("network_key");

        if key_path.exists() {
            // Load existing - PeerId stays the same
            Keypair::ed25519_from_bytes(std::fs::read(&key_path)?)
        } else {
            // Generate new and save
            let keypair = Keypair::generate_ed25519();
            std::fs::write(&key_path, keypair.secret())?;
            Ok(keypair)
        }
    } else {
        Ok(Keypair::generate_ed25519())  // Ephemeral
    }
}
```

**Key Points:**
- First startup: generates new keypair, saves to `<data_dir>/network/network_key`
- Subsequent startups: loads existing keypair, PeerId remains stable
- File permissions: 0600 (Unix) for security
- No data directory: ephemeral mode (PeerId changes each restart)

### Block Synchronization

**Location**: `src/network/sync.rs`, `src/node/service.rs`

When a node joins the network, it syncs historical blocks from peers:

```
Joining Node                         Genesis Node
     │                                     │
     │ ──── GenesisRequest ──────────────► │
     │ ◄─── GenesisResponse ───────────────│
     │      (block, validators, balances)  │
     │                                     │
     │ ──── SyncRequest(from=1) ─────────► │
     │ ◄─── SyncResponse(blocks 1-N) ──────│
     │                                     │
     │    [Import each block with rewards] │
     │                                     │
```

**Block Import Process** (for synced blocks):

```rust
// In node/service.rs - import_block()

async fn import_block(&self, block: Block) -> Result<(), NodeError> {
    // 1. Validate block header and signature
    BlockValidator::validate(&block, parent, &validators)?;

    // 2. Execute all transactions
    let mut total_fees = 0;
    for tx in &block.body.transactions {
        let result = TransactionExecutor::execute(&mut storage, tx, block_number);
        total_fees += result.fee_paid;
    }

    // 3. CRITICAL: Apply block rewards (same as during production)
    apply_block_rewards_for_import(
        &mut storage,
        block.header.author,  // Block producer
        block.header.epoch,   // For inflation calculation
        total_fees,           // Fee distribution
    )?;

    // 4. Verify state root matches
    let computed = storage.compute_state_root(block_number, chain_id);
    if computed.root != block.header.state_root {
        return Err(NodeError::StateRootMismatch);
    }

    // 5. Store block and update chain state
    storage.store_block(&block)?;
}
```

**Key Points:**
- Block rewards MUST be applied during import (not just during production)
- Reward calculation uses the same logic: `BlockReward + 50% fees` to producer (SPEC v3.2)
- 10% of fees go to finality voters (divided equally among participants)
- 30% of fees are burned, 10% go to treasury
- State root is computed AFTER applying rewards
- Genesis state includes validators and balances from the genesis node

---

## Storage Layer

### RocksDB Database

**Location**: `src/storage/db.rs`

```rust
pub struct Database {
    inner: rocksdb::DB,
}

impl Database {
    pub fn get<K: AsRef<[u8]>, V: DeserializeOwned>(&self, key: K) -> Option<V>;
    pub fn put<K: AsRef<[u8]>, V: Serialize>(&self, key: K, value: &V);
    pub fn delete<K: AsRef<[u8]>>(&self, key: K);
    pub fn iter_prefix(&self, prefix: &[u8]) -> impl Iterator<Item = (Vec<u8>, Vec<u8>)>;
}
```

### State Backend

**Location**: `src/storage/state.rs`

```rust
pub struct StateBackend {
    accounts: HashMap<AccountId, AccountState>,
    validators: HashMap<AccountId, ValidatorState>,
    storage_root: Hash,
}

pub struct AccountState {
    pub balance: Balance,
    pub nonce: Nonce,
    pub validator_stake: Option<Balance>,
}
```

### Data Directory Structure

```
~/.local/share/kratos/chains/<chain>/
├── db/              # RocksDB database
│   ├── blocks/      # Block headers and bodies
│   ├── state/       # State trie
│   └── tx/          # Transaction index
├── keystore/        # Encrypted validator keys
└── network/         # Peer identity
```

---

## RPC API

> **Full API Reference**: See [RPC_API_REFERENCE.md](RPC_API_REFERENCE.md) for complete documentation.

### JSON-RPC Server

**Location**: `src/rpc/`
**Default Port**: `9933`
**Protocol**: JSON-RPC 2.0 over HTTP

Built with warp, featuring:
- Rate limiting (DoS protection)
- CORS security (localhost-only by default)
- Request validation

### Available Methods

| Category | Methods |
|----------|---------|
| **Chain** | `chain_getInfo`, `chain_getBlock`, `chain_getBlockByNumber`, `chain_getBlockByHash`, `chain_getLatestBlock`, `chain_getHeader` |
| **State** | `state_getAccount`, `state_getBalance`, `state_getNonce` |
| **Author** | `author_submitTransaction`, `author_pendingTransactions`, `author_removeTransaction` |
| **System** | `system_info`, `system_health`, `system_peers`, `system_syncState`, `system_version`, `system_name` |
| **Mempool** | `mempool_status`, `mempool_content` |
| **Clock** | `clock_getHealth`, `clock_getValidatorRecord` |
| **Finality** | `finality_getStatus`, `finality_getLastFinalized`, `finality_getJustification`, `finality_getRoundInfo` |

### Quick Examples

```bash
# Get chain info
curl -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getInfo","params":[],"id":1}'

# Get account balance
curl -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"state_getBalance","params":["0x0101010101010101010101010101010101010101010101010101010101010101"],"id":1}'

# Submit transaction
curl -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"author_submitTransaction","params":[...],"id":1}'
```

> See [RPC_API_REFERENCE.md](RPC_API_REFERENCE.md) for detailed request/response formats and client examples.

---

## Cryptography

### Signature Schemes

**Ed25519** (primary):
- Block signing
- Transaction signing
- Validator identity

**SR25519** (optional):
- VRF generation
- Key derivation

### Hashing

| Algorithm | Use |
|-----------|-----|
| **BLAKE3** | Block hashes, state roots, fast hashing |
| **BLAKE2b** | Account addresses, legacy compatibility |

### Libraries Used

- `ed25519-dalek` - Ed25519 signatures
- `schnorrkel` - SR25519/VRF
- `blake3` - Fast hashing
- `rs_merkle` - Merkle tree proofs

---

## Build Instructions

### Prerequisites

```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable

# System dependencies (Ubuntu/Debian)
sudo apt-get install -y build-essential clang libclang-dev librocksdb-dev
```

### Build Commands

```bash
cd rust/kratos-core

# Debug build (faster compilation)
cargo build

# Release build (optimized)
cargo build --release

# Run tests
cargo test

# Binary locations:
# Debug: target/debug/kratos-node
# Release: target/release/kratos-node
```

---

## Running the Node

### Development Mode (Single Node)

```bash
./target/debug/kratos-node run --dev

# With custom ports
./target/debug/kratos-node run --dev --port 30334 --rpc-port 9945

# With validator mode
./target/debug/kratos-node run --dev --validator
```

### Custom Data Directory

```bash
./target/debug/kratos-node run \
  --dev \
  --base-path /data/kratos
```

### Multi-Node Network

**Node 1**:
```bash
./target/debug/kratos-node run \
  --port 30333 \
  --rpc-port 9944 \
  --validator
```

**Node 2**:
```bash
./target/debug/kratos-node run \
  --port 30334 \
  --rpc-port 9945 \
  --bootnodes /ip4/127.0.0.1/tcp/30333/p2p/<PEER_ID>
```

### Expected Output

```
2025-01-15 10:00:00 KratOs Node v0.1.0
2025-01-15 10:00:00 Chain: KratOs Dev
2025-01-15 10:00:00 Role: Validator
2025-01-15 10:00:00 P2P listening on /ip4/0.0.0.0/tcp/30333
2025-01-15 10:00:00 RPC listening on 0.0.0.0:9944
2025-01-15 10:00:00 Local peer ID: 12D3KooW...
2025-01-15 10:00:06 Block #1 | +12.366 KRAT
2025-01-15 10:00:12 Block #2 | +12.366 KRAT
```

---

## Key Management

### Generate New Key

```bash
./target/debug/kratos-node key generate

# Output:
# Secret seed: "your secret seed phrase..."
# Public key: 5GrwvaEF...
# SS58 Address: 5GrwvaEF...
```

### Generate with Specific Scheme

```bash
# Ed25519
./target/debug/kratos-node key generate --scheme ed25519

# SR25519
./target/debug/kratos-node key generate --scheme sr25519
```

### Inspect Key

```bash
./target/debug/kratos-node key inspect "your seed phrase"
```

---

## Troubleshooting

### Port Already in Use

```bash
# Check what's using the port
lsof -i :30333

# Use different ports
./target/debug/kratos-node run --dev --port 30334 --rpc-port 9945
```

### Database Corruption

```bash
# Purge and restart
./target/debug/kratos-node purge --base-path ~/.kratos
./target/debug/kratos-node run --dev
```

### Cannot Connect to Peers

```bash
# Check firewall
sudo ufw allow 30333/tcp

# Verify bootnode address is correct
ping <bootnode-ip>
```

### Compilation Errors

```bash
# Update Rust
rustup update stable

# Clean and rebuild
cargo clean
cargo build --release

# Check dependencies
sudo apt-get install -y librocksdb-dev clang
```

---

## Configuration Files

### Genesis Configuration

**File**: `src/genesis/config.rs`

KratOs uses a **single unified configuration** - there are no separate network modes.

```rust
pub struct ChainConfig {
    pub chain_name: String,
    pub chain_id: u32,
    pub consensus: ConsensusConfig,
    pub network: NetworkConfig,
    pub tokenomics: TokenomicsConfig,
}

// Single unified configuration:
// - ChainConfig::default()   - One configuration for all deployments
```

See [KRATOS_SYNTHESIS.md](KRATOS_SYNTHESIS.md) for complete protocol overview.

### Genesis Specification

**File**: `src/genesis/spec.rs`

Defines initial state:
- Initial balances
- Genesis validators
- System parameters

---

## Technology Stack

| Component | Technology |
|-----------|------------|
| **Language** | Rust |
| **Async Runtime** | tokio |
| **CLI** | clap |
| **Networking** | libp2p |
| **Database** | RocksDB |
| **RPC** | warp (JSON-RPC 2.0) |
| **Cryptography** | ed25519-dalek, schnorrkel, blake3 |
| **Serialization** | serde, bincode |
| **Merkle Trees** | rs_merkle |

---

## Source Files Reference

### kratos-core (Node Implementation)

| Directory | Contents |
|-----------|----------|
| `src/cli/` | Command-line interface |
| `src/node/` | Node service, producer, mempool |
| `src/consensus/` | PoS, VRF, validation, slashing |
| `src/consensus/finality/` | GRANDPA-style BFT finality gadget |
| `src/network/` | libp2p networking |
| `src/network/dns_seeds.rs` | DNS seed registry and resolution |
| `src/network/dns_seed_client.rs` | DNS seed client (fetch IDpeers.json) |
| `src/storage/` | RocksDB, state management |
| `src/rpc/` | JSON-RPC server and methods |
| `src/types/` | Core types (Block, Transaction, etc.) |
| `src/contracts/` | System contracts (KRAT, staking) |
| `src/genesis/` | Genesis configuration |

### kratos-dns-seed (DNS Seed Service)

| Directory | Contents |
|-----------|----------|
| `src/main.rs` | Application entry point |
| `src/heartbeat/` | Heartbeat receiver (TCP:30334) |
| `src/registry/` | Peer registry and scoring (RocksDB) |
| `src/network_state/` | Network state aggregator |
| `src/api/` | HTTP API (:8080) |
| `src/dns/` | DNS server (UDP:53) |
| `src/distribution/` | IDpeers.json generation |
| `src/crypto/` | Ed25519 signatures and verification |
| `src/types/` | Shared types (HeartbeatMessage, etc.) |

---

**Implementation Status**: Complete
**Last Updated**: 2025-12-23
**Runtime**: tokio async
**Framework**: Native Rust (no Substrate)
**Specification Version**: Unified (see [KRATOS_SYNTHESIS.md](KRATOS_SYNTHESIS.md))
**DNS Seed Service**: See [SPEC 6 §20](../spec/SPEC_6_NETWORK_SECURITY.md#20-dns-seed-service-kratos-dns-seed)
