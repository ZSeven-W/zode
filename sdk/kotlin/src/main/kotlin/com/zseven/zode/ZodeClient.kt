package com.zseven.zode

import java.io.BufferedReader
import java.io.Closeable
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.io.Writer
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutionException
import java.util.concurrent.atomic.AtomicLong
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.encodeToJsonElement
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put

/**
 * A JSON-RPC 2.0 client over the "zode server" stdio transport.
 *
 * A single background reader thread owns the child's stdout: it resolves
 * pending request futures by id (supporting out-of-order responses), dispatches
 * notifications to [onNotification], and answers server->client
 * `approval/request` frames via [onApprovalRequest]. Each approval is answered
 * on its own thread so the reader never blocks. All stdin writes are serialized
 * behind a lock.
 */
class ZodeClient(
    val binary: String = "zode",
    private val serverArgs: List<String> = listOf("server"),
    private val env: Map<String, String>? = null,
) : Closeable {
    // encodeDefaults so the jsonrpc default is written; explicitNulls=false so
    // null params / approvalPolicy are omitted from the wire.
    private val json =
        Json {
            ignoreUnknownKeys = true
            encodeDefaults = true
            explicitNulls = false
        }

    private val lifecycleLock = Any()
    private val writeLock = Any()

    private var process: Process? = null
    private var writer: Writer? = null
    private var reader: Thread? = null

    private val nextId = AtomicLong(1)
    private val pending = ConcurrentHashMap<Long, CompletableFuture<JsonElement>>()

    @Volatile
    private var notificationHandler: ((method: String, params: JsonElement?) -> Unit)? = null

    @Volatile
    private var approvalHandler: ((ApprovalRequestParams) -> ApprovalDecision)? = null

    /** Registers the notification handler, replacing any previous one. */
    fun onNotification(handler: ((method: String, params: JsonElement?) -> Unit)?) {
        notificationHandler = handler
    }

    /**
     * Registers the approval handler, replacing any previous one. An
     * unregistered handler (null) denies.
     */
    fun onApprovalRequest(handler: ((ApprovalRequestParams) -> ApprovalDecision)?) {
        approvalHandler = handler
    }

    /** Spawns the server process (idempotent) and launches the reader loop. */
    fun start() {
        synchronized(lifecycleLock) {
            if (process != null) return
            val builder =
                ProcessBuilder(listOf(binary) + serverArgs)
                    .redirectError(ProcessBuilder.Redirect.INHERIT)
            env?.let {
                builder.environment().clear()
                builder.environment().putAll(it)
            }
            val child = builder.start()
            process = child
            writer = OutputStreamWriter(child.outputStream, Charsets.UTF_8)
            val stdout = BufferedReader(InputStreamReader(child.inputStream, Charsets.UTF_8))
            val thread =
                Thread({ readLoop(stdout) }, "zode-reader").apply {
                    isDaemon = true
                    start()
                }
            reader = thread
        }
    }

    /**
     * Sends the initialize handshake. [approvalPolicy] is optional: null omits
     * the field from the wire so the server default applies.
     */
    fun initialize(
        name: String = "zode-sdk-kotlin",
        version: String = "0.0.0",
        approvalPolicy: String? = null,
    ): JsonElement {
        val params = InitializeParams(ClientInfo(name, version), approvalPolicy)
        return request(ProtocolMethod.Initialize.wireName, json.encodeToJsonElement(params))
    }

    /**
     * Sends a client->server request and blocks until the matching response
     * (by id) arrives or the connection closes. Throws [RpcException] on a
     * JSON-RPC error result.
     */
    fun request(method: String, params: JsonElement? = null): JsonElement {
        start()
        val id = nextId.getAndIncrement()
        val future = CompletableFuture<JsonElement>()
        pending[id] = future

        val frame = JsonRpcRequest(id = JsonPrimitive(id), method = method, params = params)
        try {
            write(json.encodeToString(JsonRpcRequest.serializer(), frame))
        } catch (e: Exception) {
            pending.remove(id)
            throw e
        }

        try {
            return future.get()
        } catch (e: ExecutionException) {
            throw e.cause ?: e
        }
    }

    fun request(method: ProtocolMethod, params: JsonElement? = null): JsonElement =
        request(method.wireName, params)

    /** Sends a client->server notification (no id, no response awaited). */
    fun notify(method: String, params: JsonElement? = null) {
        start()
        val frame = JsonRpcNotification(method = method, params = params)
        write(json.encodeToString(JsonRpcNotification.serializer(), frame))
    }

    fun notify(method: ProtocolMethod, params: JsonElement? = null) = notify(method.wireName, params)

    override fun close() {
        val (proc, w) =
            synchronized(lifecycleLock) {
                val p = process
                val out = writer
                process = null
                writer = null
                reader = null
                p to out
            }
        try {
            w?.close()
        } catch (_: Exception) {
        }
        proc?.destroy()
        rejectPending("zode client closed")
    }

    /** Serializes value + newline onto stdin under [writeLock]. */
    private fun write(payload: String) {
        val w = synchronized(lifecycleLock) { writer } ?: error("zode client is not started")
        synchronized(writeLock) {
            w.write(payload)
            w.write("\n")
            w.flush()
        }
    }

    /** Owns the child's stdout, routing each newline-delimited frame until EOF. */
    private fun readLoop(stdout: BufferedReader) {
        try {
            stdout.use { r ->
                while (true) {
                    val line = r.readLine() ?: break
                    if (line.isNotEmpty()) route(line)
                }
            }
        } catch (_: Exception) {
            // Reader stream closed underneath us during shutdown.
        } finally {
            rejectPending("zode server closed the connection")
        }
    }

    /** Classifies one frame and dispatches it; malformed frames are dropped. */
    private fun route(line: String) {
        val frame =
            try {
                classifyIncomingFrame(line, json)
            } catch (_: InvalidFrameException) {
                return
            }
        when (frame.kind) {
            FrameKind.Response, FrameKind.Error -> {
                val id = frame.id?.jsonPrimitive?.longOrNull ?: return
                val future = pending.remove(id) ?: return
                if (frame.kind == FrameKind.Error) {
                    future.completeExceptionally(parseError(frame.fields["error"]))
                } else {
                    future.complete(frame.fields["result"] ?: JsonNull)
                }
            }
            FrameKind.Notification -> dispatchNotification(frame)
            FrameKind.ServerRequest ->
                // Answer on its own thread so a slow approval handler never
                // stalls the reader.
                Thread({
                    try {
                        answerServerRequest(frame)
                    } catch (_: Exception) {
                        // Writing the answer can race a close; ignore.
                    }
                }, "zode-approval").apply {
                    isDaemon = true
                    start()
                }
        }
    }

    private fun parseError(errorField: JsonElement?): RpcException {
        val obj = errorField as? JsonObject
        val code = obj?.get("code")?.jsonPrimitive?.longOrNull?.toInt() ?: 0
        val message = obj?.get("message")?.jsonPrimitive?.content ?: "RPC error"
        return RpcException(code = code, message = message, data = obj?.get("data"))
    }

    /** Invokes the notification handler, swallowing any handler exception. */
    private fun dispatchNotification(frame: ClassifiedFrame) {
        val handler = notificationHandler ?: return
        val method = frame.method ?: return
        try {
            handler(method, frame.params)
        } catch (_: Throwable) {
            // A throwing handler must not kill the reader.
        }
    }

    /**
     * Handles a server->client request. Only approval/request is supported;
     * anything else gets a method-not-found error. An unregistered or throwing
     * approval handler denies.
     */
    private fun answerServerRequest(frame: ClassifiedFrame) {
        val idRaw = frame.id ?: return
        if (frame.method != "approval/request") {
            write(
                json.encodeToString(
                    JsonRpcErrorFrame.serializer(),
                    JsonRpcErrorFrame(
                        id = idRaw,
                        error = RpcErrorObject(code = -32601, message = "method not found"),
                    ),
                ),
            )
            return
        }
        val decision = resolveDecision(frame.params)
        val result = buildJsonObject { put("decision", decision.wire) }
        write(
            json.encodeToString(
                JsonRpcResultFrame.serializer(),
                JsonRpcResultFrame(id = idRaw, result = result),
            ),
        )
    }

    /** Runs the approval handler, denying when unregistered or throwing. */
    private fun resolveDecision(rawParams: JsonElement?): ApprovalDecision {
        val handler = approvalHandler ?: return ApprovalDecision.Deny
        val params =
            try {
                if (rawParams != null) {
                    json.decodeFromJsonElement(ApprovalRequestParams.serializer(), rawParams)
                } else {
                    ApprovalRequestParams()
                }
            } catch (_: Exception) {
                ApprovalRequestParams()
            }
        return try {
            handler(params)
        } catch (_: Throwable) {
            ApprovalDecision.Deny
        }
    }

    /** Fails every waiting caller. Idempotent — already-completed futures ignore it. */
    private fun rejectPending(reason: String) {
        val iterator = pending.entries.iterator()
        while (iterator.hasNext()) {
            val entry = iterator.next()
            iterator.remove()
            entry.value.completeExceptionally(IllegalStateException(reason))
        }
    }
}
