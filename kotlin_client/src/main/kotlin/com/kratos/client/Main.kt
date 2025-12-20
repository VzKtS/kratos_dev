package com.kratos.client

import io.ktor.client.*
import io.ktor.client.engine.cio.*
import io.ktor.client.plugins.websocket.*
import io.ktor.client.request.*
import io.ktor.client.statement.*
import io.ktor.http.*
import io.ktor.websocket.*
import kotlinx.coroutines.*
import kotlinx.serialization.*
import kotlinx.serialization.json.*
import java.util.concurrent.atomic.AtomicLong

/**
 * Client léger KratOs pour interagir avec la blockchain
 */
class KratOsClient(
    private val nodeUrl: String,
    private val wsUrl: String = nodeUrl.replace("http", "ws")
) {
    private val client = HttpClient(CIO) {
        install(WebSockets)
    }
    
    private val requestIdCounter = AtomicLong(0)
    
    suspend fun jsonRpcCall(
        method: String,
        params: List<Any> = emptyList()
    ): JsonElement {
        val requestId = requestIdCounter.incrementAndGet()
        
        val request = buildJsonObject {
            put("jsonrpc", "2.0")
            put("method", method)
            put("params", Json.encodeToJsonElement(params))
            put("id", requestId)
        }
        
        val response: HttpResponse = client.post(nodeUrl) {
            contentType(ContentType.Application.Json)
            setBody(request.toString())
        }
        
        return Json.parseToJsonElement(response.bodyAsText())
    }
    
    suspend fun connectToSidechain(
        sidechainId: Long,
        onMessage: suspend (String) -> Unit
    ) {
        client.webSocket(
            host = wsUrl.substringAfter("://").substringBefore(":"),
            port = wsUrl.substringAfter(":").substringAfter(":").toIntOrNull() ?: 9944,
            path = "/sidechain/$sidechainId"
        ) {
            for (frame in incoming) {
                when (frame) {
                    is Frame.Text -> {
                        onMessage(frame.readText())
                    }
                    else -> {}
                }
            }
        }
    }
    
    fun close() {
        client.close()
    }
}

class SidechainAPI(private val client: KratOsClient) {
    
    suspend fun getSidechainMetadata(sidechainId: Long): SidechainMetadata? {
        return try {
            val result = client.jsonRpcCall(
                "sidechain_getMetadata",
                listOf(sidechainId)
            )
            
            Json.decodeFromJsonElement<SidechainMetadata>(
                result.jsonObject["result"] ?: return null
            )
        } catch (e: Exception) {
            println("Erreur lors de la récupération des métadonnées: ${e.message}")
            null
        }
    }
    
    suspend fun createSidechain(
        accountId: String,
        parentId: Long? = null
    ): Long? {
        return try {
            val params = mutableListOf<Any>(accountId)
            parentId?.let { params.add(it) }
            
            val result = client.jsonRpcCall(
                "sidechain_create",
                params
            )
            
            result.jsonObject["result"]?.jsonPrimitive?.longOrNull
        } catch (e: Exception) {
            println("Erreur lors de la création de la sidechain: ${e.message}")
            null
        }
    }
    
    suspend fun recordActivity(sidechainId: Long): Boolean {
        return try {
            val result = client.jsonRpcCall(
                "sidechain_recordActivity",
                listOf(sidechainId)
            )
            
            result.jsonObject["result"]?.jsonPrimitive?.boolean ?: false
        } catch (e: Exception) {
            println("Erreur lors de l'enregistrement de l'activité: ${e.message}")
            false
        }
    }
    
    suspend fun listSidechainsByOwner(accountId: String): List<Long> {
        return try {
            val result = client.jsonRpcCall(
                "sidechain_listByOwner",
                listOf(accountId)
            )
            
            Json.decodeFromJsonElement<List<Long>>(
                result.jsonObject["result"] ?: return emptyList()
            )
        } catch (e: Exception) {
            println("Erreur lors de la récupération des sidechains: ${e.message}")
            emptyList()
        }
    }
}

class HostChainAPI(private val client: KratOsClient) {
    
    suspend fun createHostChain(
        accountId: String,
        initialSidechains: List<Long>
    ): Long? {
        return try {
            val result = client.jsonRpcCall(
                "hostchain_create",
                listOf(accountId, initialSidechains)
            )
            
            result.jsonObject["result"]?.jsonPrimitive?.longOrNull
        } catch (e: Exception) {
            println("Erreur lors de la création de la host chain: ${e.message}")
            null
        }
    }
    
    suspend fun requestAffiliation(
        sidechainId: Long,
        hostId: Long
    ): Boolean {
        return try {
            val result = client.jsonRpcCall(
                "hostchain_requestAffiliation",
                listOf(sidechainId, hostId)
            )
            
            result.jsonObject["result"]?.jsonPrimitive?.boolean ?: false
        } catch (e: Exception) {
            println("Erreur lors de la demande d'affiliation: ${e.message}")
            false
        }
    }
    
    suspend fun getHostChainMetadata(hostId: Long): HostChainMetadata? {
        return try {
            val result = client.jsonRpcCall(
                "hostchain_getMetadata",
                listOf(hostId)
            )
            
            Json.decodeFromJsonElement<HostChainMetadata>(
                result.jsonObject["result"] ?: return null
            )
        } catch (e: Exception) {
            println("Erreur lors de la récupération des métadonnées: ${e.message}")
            null
        }
    }
}

object ScaleCodec {
    
    fun encode(value: Any): ByteArray {
        return when (value) {
            is Int -> encodeCompactInt(value)
            is Long -> encodeCompactLong(value)
            is String -> encodeString(value)
            is List<*> -> encodeList(value)
            else -> throw IllegalArgumentException("Type non supporté: ${value::class}")
        }
    }
    
    private fun encodeCompactInt(value: Int): ByteArray {
        return when {
            value < 64 -> byteArrayOf((value shl 2).toByte())
            value < 16384 -> {
                val v = (value shl 2) or 1
                byteArrayOf((v and 0xFF).toByte(), ((v shr 8) and 0xFF).toByte())
            }
            else -> {
                val bytes = mutableListOf<Byte>(2)
                var temp = value
                while (temp > 0) {
                    bytes.add((temp and 0xFF).toByte())
                    temp = temp shr 8
                }
                bytes.toByteArray()
            }
        }
    }
    
    private fun encodeCompactLong(value: Long): ByteArray {
        return encodeCompactInt(value.toInt())
    }
    
    private fun encodeString(value: String): ByteArray {
        val bytes = value.toByteArray(Charsets.UTF_8)
        return encodeCompactInt(bytes.size) + bytes
    }
    
    private fun encodeList(value: List<*>): ByteArray {
        val result = mutableListOf<Byte>()
        result.addAll(encodeCompactInt(value.size).toList())
        
        value.forEach { item ->
            if (item != null) {
                result.addAll(encode(item).toList())
            }
        }
        
        return result.toByteArray()
    }
    
    fun decode(bytes: ByteArray, type: String): Any {
        return when (type) {
            "u32", "u64" -> decodeCompactInt(bytes)
            "String" -> decodeString(bytes)
            "Vec" -> decodeList(bytes)
            else -> throw IllegalArgumentException("Type non supporté: $type")
        }
    }
    
    private fun decodeCompactInt(bytes: ByteArray): Int {
        val firstByte = bytes[0].toInt() and 0xFF
        return when (firstByte and 0x03) {
            0 -> firstByte shr 2
            1 -> ((firstByte shr 2) or ((bytes[1].toInt() and 0xFF) shl 6))
            else -> {
                val length = (firstByte shr 2) + 4
                var result = 0
                for (i in 1..length) {
                    result = result or ((bytes[i].toInt() and 0xFF) shl ((i - 1) * 8))
                }
                result
            }
        }
    }
    
    private fun decodeString(bytes: ByteArray): String {
        val length = decodeCompactInt(bytes)
        val lengthBytes = if (length < 64) 1 else 2
        return bytes.sliceArray(lengthBytes until lengthBytes + length)
            .toString(Charsets.UTF_8)
    }
    
    private fun decodeList(bytes: ByteArray): List<Any> {
        val length = decodeCompactInt(bytes)
        return emptyList()
    }
}

@Serializable
data class SidechainMetadata(
    val id: Long,
    val parentId: Long? = null,
    val owner: String,
    val validators: List<String> = emptyList(),
    val status: String,
    val createdAt: Long,
    val lastActivity: Long
)

@Serializable
data class HostChainMetadata(
    val id: Long,
    val creator: String,
    val memberSidechains: List<Long>,
    val validatorPool: List<String>,
    val createdAt: Long
)

suspend fun main() = coroutineScope {
    val client = KratOsClient("http://localhost:9933")
    val sidechainAPI = SidechainAPI(client)
    val hostChainAPI = HostChainAPI(client)
    
    try {
        println("=== KratOs Client Demo ===\n")
        
        println("Récupération des métadonnées de la sidechain 1...")
        val metadata = sidechainAPI.getSidechainMetadata(1)
        metadata?.let {
            println("Sidechain ID: ${it.id}")
            println("Owner: ${it.owner}")
            println("Status: ${it.status}")
            println("Validators: ${it.validators.size}\n")
        }
        
        println("Listing des sidechains du compte...")
        val sidechains = sidechainAPI.listSidechainsByOwner("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY")
        println("Sidechains trouvées: $sidechains\n")
        
        println("Connexion à la sidechain 1 via WebSocket...")
        launch {
            sidechainAPI.recordActivity(1)
        }
        
        println("\nClient KratOs initialisé avec succès!")
        
    } catch (e: Exception) {
        println("Erreur: ${e.message}")
        e.printStackTrace()
    } finally {
        client.close()
    }
}
