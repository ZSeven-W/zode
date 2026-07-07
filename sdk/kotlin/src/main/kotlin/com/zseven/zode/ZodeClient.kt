package com.zseven.zode

import java.io.BufferedReader
import java.io.Closeable
import java.io.InputStreamReader
import java.io.PrintWriter
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject

class ZodeClient(
    val binary: String = "zode",
) : Closeable {
    private val json = Json { ignoreUnknownKeys = true }
    private var process: Process? = null
    private var stdin: PrintWriter? = null
    private var stdout: BufferedReader? = null
    private var nextId: Long = 1

    @Synchronized
    fun start() {
        if (process != null) return
        val child = ProcessBuilder(binary, "server")
            .redirectError(ProcessBuilder.Redirect.INHERIT)
            .start()
        process = child
        stdin = PrintWriter(child.outputStream, true)
        stdout = BufferedReader(InputStreamReader(child.inputStream))
    }

    @Synchronized
    fun initialize(
        name: String = "zode-sdk-kotlin",
        version: String = "0.0.0",
    ): JsonElement {
        val params = buildJsonObject {
            putJsonObject("clientInfo") {
                put("name", name)
                put("version", version)
            }
        }
        return request("initialize", params)
    }

    @Synchronized
    fun request(method: String, params: JsonElement? = null): JsonElement {
        start()
        val requestId = nextId++
        val payload = buildJsonObject {
            put("id", requestId)
            put("method", method)
            if (params != null) put("params", params)
        }
        stdin?.println(payload.toString()) ?: error("zode client is not started")

        while (true) {
            val line = stdout?.readLine() ?: error("zode server closed the connection")
            val message = json.parseToJsonElement(line).jsonObject
            val id = message["id"]?.jsonPrimitive?.longOrNull
            if (id != requestId) continue
            val error = message["error"]
            if (error != null) {
                val obj = error.jsonObject
                throw RpcException(
                    code = obj["code"]?.jsonPrimitive?.longOrNull?.toInt() ?: 0,
                    message = obj["message"]?.jsonPrimitive?.content ?: "RPC error",
                    data = obj["data"],
                )
            }
            return message["result"] ?: JsonNull
        }
    }

    @Synchronized
    fun request(method: ProtocolMethod, params: JsonElement? = null): JsonElement =
        request(method.wireName, params)

    @Synchronized
    fun notify(method: String, params: JsonElement? = null) {
        start()
        val payload = buildJsonObject {
            put("method", method)
            if (params != null) put("params", params)
        }
        stdin?.println(payload.toString()) ?: error("zode client is not started")
    }

    @Synchronized
    fun notify(method: ProtocolMethod, params: JsonElement? = null) =
        notify(method.wireName, params)

    override fun close() {
        stdin?.close()
        stdout?.close()
        process?.destroy()
        stdin = null
        stdout = null
        process = null
    }
}
