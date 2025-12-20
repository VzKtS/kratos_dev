# KratOs RPC API Reference

## Overview

KratOs exposes a **JSON-RPC 2.0** API over HTTP for client interaction. This API is used by wallets, explorers, and other applications to interact with the blockchain.

**Implementation**: `rust/kratos-core/src/rpc/`

---

## Connection

### Default Endpoint

| Environment | URL | Port |
|-------------|-----|------|
| **Development** | `http://localhost:9933` | 9933 |
| **Custom** | `http://<host>:<rpc-port>` | Configurable |

### Starting the Node

```bash
# Default RPC port (9933)
./target/debug/kratos-node run --dev --validator

# Custom RPC port
./target/debug/kratos-node run --dev --rpc-port 9944 --validator
```

### CORS Policy

By default, CORS is restricted to localhost only:
- `http://localhost`
- `http://127.0.0.1`
- `http://localhost:3000`
- `http://127.0.0.1:3000`

For production, configure allowed origins explicitly.

---

## Request Format

All requests use HTTP POST with JSON body:

```json
{
  "jsonrpc": "2.0",
  "method": "method_name",
  "params": [...],
  "id": 1
}
```

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `jsonrpc` | string | Yes | Must be `"2.0"` |
| `method` | string | Yes | Method name (e.g., `chain_getInfo`) |
| `params` | array | No | Method parameters |
| `id` | number/string | Yes | Request identifier |

---

## Response Format

### Success Response

```json
{
  "jsonrpc": "2.0",
  "result": { ... },
  "id": 1
}
```

### Error Response

```json
{
  "jsonrpc": "2.0",
  "error": {
    "code": -32601,
    "message": "Method not found: unknown_method",
    "data": null
  },
  "id": 1
}
```

### Error Codes

| Code | Name | Description |
|------|------|-------------|
| -32700 | Parse Error | Invalid JSON |
| -32600 | Invalid Request | Invalid JSON-RPC structure |
| -32601 | Method Not Found | Unknown method |
| -32602 | Invalid Params | Invalid parameters |
| -32603 | Internal Error | Server error |
| -32001 | Block Not Found | Requested block doesn't exist |
| -32002 | Transaction Not Found | Requested tx doesn't exist |
| -32003 | Account Not Found | Requested account doesn't exist |
| -32010 | Transaction Rejected | Transaction validation failed |
| -32029 | Rate Limited | Too many requests |

---

## Methods Reference

### Chain Methods

#### `chain_getInfo`

Get current chain information.

**Parameters**: None

**Response**:
```json
{
  "chainName": "KratOs",
  "height": 12345,
  "bestHash": "0x1234...abcd",
  "genesisHash": "0xabcd...1234",
  "currentEpoch": 20,
  "currentSlot": 123,
  "isSynced": true,
  "syncGap": 0
}
```

**Example**:
```bash
curl -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getInfo","params":[],"id":1}'
```

---

#### `chain_getBlock`

Get block by number, hash, or "latest".

**Parameters**:
- `[number]` - Block number (u64)
- `["latest"]` - Latest block
- `["0x..."]` - Block hash

**Response**: `BlockWithTransactions` object

**Example**:
```bash
# By number
curl -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getBlock","params":[100],"id":1}'

# Latest
curl -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getBlock","params":["latest"],"id":1}'
```

---

#### `chain_getBlockByNumber`

Get block by specific number.

**Parameters**: `[number: u64]`

**Response**:
```json
{
  "number": 100,
  "hash": "0x...",
  "parentHash": "0x...",
  "timestamp": 1702987654,
  "author": "0x...",
  "epoch": 0,
  "slot": 100,
  "txCount": 5,
  "stateRoot": "0x...",
  "transactionsRoot": "0x...",
  "transactions": [...]
}
```

---

#### `chain_getBlockByHash`

Get block by hash.

**Parameters**: `[hash: string]` - 0x-prefixed 64 hex characters

**Response**: Same as `chain_getBlockByNumber`

---

#### `chain_getLatestBlock`

Get the most recent block.

**Parameters**: None

**Response**: Same as `chain_getBlockByNumber`

---

#### `chain_getHeader`

Get block header only (without transactions).

**Parameters**: `[number?: u64]` - Optional, defaults to latest

**Response**:
```json
{
  "number": 100,
  "hash": "0x...",
  "parentHash": "0x...",
  "timestamp": 1702987654,
  "author": "0x...",
  "epoch": 0,
  "slot": 100,
  "txCount": 5,
  "stateRoot": "0x...",
  "transactionsRoot": "0x..."
}
```

---

### State Methods

#### `state_getAccount`

Get full account information.

**Parameters**: `[address: string]` - 0x-prefixed AccountId (64 hex chars)

**Response**:
```json
{
  "address": "0x0101...0101",
  "free": "1000 KRAT",
  "reserved": "500 KRAT",
  "total": "1500 KRAT",
  "freeRaw": 1000000000000000,
  "reservedRaw": 500000000000000,
  "totalRaw": 1500000000000000,
  "nonce": 42
}
```

**Example**:
```bash
curl -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"state_getAccount","params":["0x0101010101010101010101010101010101010101010101010101010101010101"],"id":1}'
```

---

#### `state_getBalance`

Get account balance.

**Parameters**: `[address: string]`

**Response**: `Balance` (u128) - Raw balance in base units

**Note**: 1 KRAT = 1,000,000,000,000 (10^12) base units

---

#### `state_getNonce`

Get account nonce for transaction signing.

**Parameters**: `[address: string]`

**Response**: `Nonce` (u64)

**Example**:
```bash
curl -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"state_getNonce","params":["0x..."],"id":1}'
```

---

### Author Methods (Transaction Submission)

#### `author_submitTransaction`

Submit a signed transaction to the mempool.

**Parameters**: `[signedTx: SignedTransaction]`

**SignedTransaction Structure**:
```json
{
  "transaction": {
    "sender": "0x...",
    "nonce": 0,
    "call": {
      "Transfer": {
        "to": "0x...",
        "amount": 1000000000000
      }
    },
    "timestamp": 1702987654
  },
  "signature": "0x..."
}
```

**Response**:
```json
{
  "hash": "0x...",
  "message": "Transaction submitted"
}
```

**Transaction Types** (`call` field):
```json
// Transfer
{ "Transfer": { "to": "0x...", "amount": 1000000000000 } }

// Stake
{ "Stake": { "amount": 50000000000000000 } }

// Unstake
{ "Unstake": { "amount": 1000000000000000 } }

// Withdraw Unbonded
"WithdrawUnbonded"

// Register Validator
{ "RegisterValidator": { "stake": 50000000000000000 } }

// Unregister Validator
"UnregisterValidator"
```

---

#### `author_pendingTransactions`

Get all pending transactions in mempool.

**Parameters**: None

**Response**: Array of `TransactionInfo`

---

#### `author_removeTransaction`

Remove a transaction from mempool by hash.

**Parameters**: `[hash: string]`

**Response**: `boolean`

---

### System Methods

#### `system_info`

Get full system information.

**Parameters**: None

**Response**:
```json
{
  "name": "KratOs Node",
  "version": "0.1.0",
  "chain": { ... },
  "network": {
    "localPeerId": "12D3KooW...",
    "listeningAddresses": ["/ip4/..."],
    "peerCount": 5,
    "networkBestHeight": 12345,
    "averagePeerScore": 100
  },
  "pendingTxs": 10
}
```

---

#### `system_health`

Health check endpoint.

**Parameters**: None

**Response**:
```json
{
  "healthy": true,
  "isSynced": true,
  "hasPeers": true,
  "blockHeight": 12345,
  "peerCount": 5
}
```

---

#### `system_peers`

Get connected peers.

**Parameters**: None

**Response**: `[peerCount: number, peerIds: string[]]`

---

#### `system_syncState`

Get synchronization status.

**Parameters**: None

**Response**:
```json
{
  "syncing": false,
  "currentBlock": 12345,
  "highestBlock": 12345,
  "blocksBehind": 0,
  "state": "Synced"
}
```

---

#### `system_version`

Get node version.

**Parameters**: None

**Response**: `"0.1.0"` (string)

---

#### `system_name`

Get node name.

**Parameters**: None

**Response**: `"KratOs Node"` (string)

---

### Mempool Methods

#### `mempool_status`

Get mempool statistics.

**Parameters**: None

**Response**:
```json
{
  "pendingCount": 15,
  "totalFees": 1500000,
  "stats": {
    "totalAdded": 1000,
    "totalRemoved": 985,
    "totalEvicted": 0,
    "totalRejected": 10,
    "totalReplaced": 5
  }
}
```

---

#### `mempool_content`

Get all transactions in mempool.

**Parameters**: None

**Response**: Array of `TransactionInfo`

---

### Clock Health Methods

#### `clock_getHealth`

Get clock synchronization health.

**Parameters**: None

**Response**: Clock health status

---

#### `clock_getValidatorRecord`

Get clock drift record for a validator.

**Parameters**: `[validatorAddress: string]`

**Response**: Validator clock record

---

## Data Types

### Balance

All balances are in base units (u128).

| Unit | Base Units |
|------|------------|
| 1 KRAT | 1,000,000,000,000 (10^12) |
| 1 milliKRAT | 1,000,000,000 (10^9) |
| 1 microKRAT | 1,000,000 (10^6) |

### AccountId / Address

- Format: `0x` + 64 hex characters (32 bytes)
- Example: `0x0101010101010101010101010101010101010101010101010101010101010101`

### Hash

- Format: `0x` + 64 hex characters (32 bytes)
- Example: `0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890`

### Signature

- Ed25519 signature: 64 bytes
- Format: Hex-encoded or base64

### TransactionInfo

```json
{
  "hash": "0x...",
  "from": "0x...",
  "nonce": 0,
  "txType": "transfer",
  "details": { "to": "0x...", "amount": 1000000000000 },
  "timestamp": 1702987654,
  "fee": 1000
}
```

### BlockInfo

```json
{
  "number": 100,
  "hash": "0x...",
  "parentHash": "0x...",
  "timestamp": 1702987654,
  "author": "0x...",
  "epoch": 0,
  "slot": 100,
  "txCount": 5,
  "stateRoot": "0x...",
  "transactionsRoot": "0x..."
}
```

---

## Client Examples

### Kotlin (Ktor)

```kotlin
import io.ktor.client.*
import io.ktor.client.request.*
import io.ktor.client.statement.*
import kotlinx.serialization.*

@Serializable
data class JsonRpcRequest(
    val jsonrpc: String = "2.0",
    val method: String,
    val params: List<String> = emptyList(),
    val id: Int
)

@Serializable
data class JsonRpcResponse<T>(
    val jsonrpc: String,
    val result: T? = null,
    val error: JsonRpcError? = null,
    val id: Int
)

@Serializable
data class JsonRpcError(
    val code: Int,
    val message: String,
    val data: String? = null
)

class KratOsRpcClient(private val endpoint: String = "http://localhost:9933") {
    private val client = HttpClient()

    suspend fun getBalance(address: String): Long {
        val request = JsonRpcRequest(
            method = "state_getBalance",
            params = listOf(address),
            id = 1
        )
        val response: JsonRpcResponse<Long> = client.post(endpoint) {
            setBody(request)
        }.body()
        return response.result ?: throw Exception(response.error?.message)
    }

    suspend fun getNonce(address: String): Long {
        val request = JsonRpcRequest(
            method = "state_getNonce",
            params = listOf(address),
            id = 1
        )
        val response: JsonRpcResponse<Long> = client.post(endpoint) {
            setBody(request)
        }.body()
        return response.result ?: 0
    }
}
```

### JavaScript/TypeScript

```typescript
interface JsonRpcRequest {
  jsonrpc: "2.0";
  method: string;
  params: any[];
  id: number;
}

interface JsonRpcResponse<T> {
  jsonrpc: "2.0";
  result?: T;
  error?: { code: number; message: string };
  id: number;
}

class KratOsClient {
  constructor(private endpoint: string = "http://localhost:9933") {}

  private async call<T>(method: string, params: any[] = []): Promise<T> {
    const response = await fetch(this.endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        method,
        params,
        id: Date.now()
      })
    });
    const json: JsonRpcResponse<T> = await response.json();
    if (json.error) throw new Error(json.error.message);
    return json.result!;
  }

  async getChainInfo() {
    return this.call<ChainInfo>("chain_getInfo");
  }

  async getBalance(address: string): Promise<bigint> {
    return BigInt(await this.call<string>("state_getBalance", [address]));
  }

  async getNonce(address: string): Promise<number> {
    return this.call<number>("state_getNonce", [address]);
  }
}
```

### cURL

```bash
# Get chain info
curl -s -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getInfo","params":[],"id":1}' | jq

# Get account balance
curl -s -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"state_getBalance","params":["0x0101..."],"id":1}' | jq

# Get latest block
curl -s -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"chain_getLatestBlock","params":[],"id":1}' | jq

# Health check
curl -s -X POST http://localhost:9933 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"system_health","params":[],"id":1}' | jq
```

---

## Security Considerations

### Rate Limiting

The RPC server implements rate limiting to prevent DoS attacks:
- Error code `-32029` indicates rate limit exceeded
- Response includes `retryAfter` field in seconds

### Transaction Signing

Transactions must be signed using **Ed25519** with **domain separation**:

```
message = DOMAIN_TRANSACTION || serialize(transaction)
signature = ed25519_sign(private_key, message)
```

Where `DOMAIN_TRANSACTION = "KRATOS_TRANSACTION_V1:"`

### Input Validation

- All hex strings must be properly formatted (`0x` prefix)
- AccountIds and hashes must be exactly 32 bytes (64 hex chars)
- Nonces must match account state
- Balances use `u128` (max ~3.4 × 10^38)

---

## Source Files

| File | Description |
|------|-------------|
| [server.rs](../rust/kratos-core/src/rpc/server.rs) | HTTP server, CORS, rate limiting |
| [methods.rs](../rust/kratos-core/src/rpc/methods.rs) | RPC method implementations |
| [types.rs](../rust/kratos-core/src/rpc/types.rs) | Request/response types |
| [rate_limit.rs](../rust/kratos-core/src/rpc/rate_limit.rs) | Rate limiting logic |

---

**Last Updated**: 2025-12-19
**Version**: 1.0.0
