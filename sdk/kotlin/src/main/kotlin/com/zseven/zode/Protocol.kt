package com.zseven.zode

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive

/**
 * The exact version string every frame must carry. The zode app-server is
 * strict: outgoing frames must include it and incoming frames without it are
 * rejected by [classifyIncomingFrame].
 */
const val JSONRPC_VERSION: String = "2.0"

/**
 * Every client->server method in the canonical wire order defined by
 * `sdk/fixtures/jsonrpc/protocol.schema.json`. Enum declaration order is the
 * wire order; [entries] therefore mirrors the schema `methods` array exactly
 * (27 entries).
 */
enum class ProtocolMethod(val wireName: String) {
    Initialize("initialize"),
    ThreadStart("thread/start"),
    ThreadResume("thread/resume"),
    ThreadList("thread/list"),
    ThreadRead("thread/read"),
    ThreadDelete("thread/delete"),
    ThreadNameSet("thread/name/set"),
    TurnStart("turn/start"),
    TurnInterrupt("turn/interrupt"),
    FsReadFile("fs/readFile"),
    FsWriteFile("fs/writeFile"),
    FsCreateDirectory("fs/createDirectory"),
    FsGetMetadata("fs/getMetadata"),
    FsReadDirectory("fs/readDirectory"),
    FsRemove("fs/remove"),
    FsCopy("fs/copy"),
    CommandExec("command/exec"),
    ModelList("model/list"),
    ModelSet("model/set"),
    ConfigRead("config/read"),
    ConfigList("config/list"),
    ConfigWrite("config/write"),
    SkillsList("skills/list"),
    SkillsRead("skills/read"),
    HooksList("hooks/list"),
    McpServerStatusList("mcpServerStatus/list"),
    PluginList("plugin/list"),
}

/**
 * A client->server request. [jsonrpc] always defaults to [JSONRPC_VERSION] so
 * the strict server accepts the frame. [id] is a [JsonElement] because the
 * JSON-RPC spec allows a number or a string; the client uses numbers, fixtures
 * use strings.
 */
@Serializable
data class JsonRpcRequest(
    val jsonrpc: String = JSONRPC_VERSION,
    val id: JsonElement,
    val method: String,
    val params: JsonElement? = null,
)

/** A client->server notification (no id, no response awaited). */
@Serializable
data class JsonRpcNotification(
    val jsonrpc: String = JSONRPC_VERSION,
    val method: String,
    val params: JsonElement? = null,
)

/** A server->client response answer frame (used to answer approval requests). */
@Serializable
data class JsonRpcResultFrame(
    val jsonrpc: String = JSONRPC_VERSION,
    val id: JsonElement,
    val result: JsonElement,
)

/** A server->client error answer frame (used for unsupported server requests). */
@Serializable
data class JsonRpcErrorFrame(
    val jsonrpc: String = JSONRPC_VERSION,
    val id: JsonElement,
    val error: RpcErrorObject,
)

@Serializable
data class RpcErrorObject(
    val code: Int,
    val message: String,
    val data: JsonElement? = null,
)

/** Identifies the SDK client during the initialize handshake. */
@Serializable
data class ClientInfo(
    val name: String,
    val version: String,
)

/**
 * Params for the initialize request. [approvalPolicy] is omitted from the wire
 * when null so the server applies its own default (serialization runs with
 * `explicitNulls = false`).
 */
@Serializable
data class InitializeParams(
    val clientInfo: ClientInfo,
    val approvalPolicy: String? = null,
)

/** The answer to a server->client approval/request. */
enum class ApprovalDecision(val wire: String) {
    Allow("allow"),
    AllowAlways("allowAlways"),
    Deny("deny"),
}

/** Params of a server->client approval/request. */
@Serializable
data class ApprovalRequestParams(
    @SerialName("approvalId") val approvalId: String = "",
    val kind: String = "",
    val summary: String = "",
    val threadId: String? = null,
    val turnId: String? = null,
    val tool: String? = null,
    val input: JsonElement? = null,
)

/** The kind of an incoming, JSON-RPC-2.0-validated frame. */
enum class FrameKind {
    Response,
    Error,
    Notification,
    ServerRequest,
}

/**
 * An incoming frame tagged with its JSON-RPC kind. [fields] holds the raw
 * top-level members so the dispatch loop can pull id, result, error, method,
 * and params without re-parsing.
 */
data class ClassifiedFrame(
    val kind: FrameKind,
    val fields: JsonObject,
) {
    val method: String?
        get() = fields["method"]?.jsonPrimitive?.contentOrNull

    val id: JsonElement?
        get() = fields["id"]

    val params: JsonElement?
        get() = fields["params"]
}

/** Thrown by [classifyIncomingFrame] for anything that is not a strict frame. */
class InvalidFrameException(message: String) : RuntimeException(message)

/**
 * Decodes and classifies a single incoming frame, rejecting anything that is
 * not a strict JSON-RPC 2.0 object. Any frame missing the `"jsonrpc":"2.0"`
 * marker (or otherwise malformed) throws [InvalidFrameException].
 */
fun classifyIncomingFrame(raw: String, json: kotlinx.serialization.json.Json): ClassifiedFrame {
    val element =
        try {
            json.parseToJsonElement(raw)
        } catch (_: Exception) {
            throw InvalidFrameException("frame is not valid JSON")
        }
    val obj = element as? JsonObject ?: throw InvalidFrameException("frame is not a JSON object")

    val version = (obj["jsonrpc"] as? JsonPrimitive)?.contentOrNull
    if (version != JSONRPC_VERSION) {
        throw InvalidFrameException("frame missing or invalid jsonrpc marker")
    }

    val hasMethod = obj.containsKey("method")
    if (hasMethod) {
        // method must be a string
        (obj["method"] as? JsonPrimitive)?.contentOrNull
            ?: throw InvalidFrameException("method is not a string")
        return if (obj.containsKey("id")) {
            ClassifiedFrame(FrameKind.ServerRequest, obj)
        } else {
            ClassifiedFrame(FrameKind.Notification, obj)
        }
    }

    if (!obj.containsKey("id")) {
        throw InvalidFrameException("frame has neither method nor id")
    }
    return when {
        obj.containsKey("error") -> ClassifiedFrame(FrameKind.Error, obj)
        obj.containsKey("result") -> ClassifiedFrame(FrameKind.Response, obj)
        else -> throw InvalidFrameException("id frame has neither result nor error")
    }
}

/** Error raised when a request receives a JSON-RPC error response. */
class RpcException(
    val code: Int,
    override val message: String,
    val data: JsonElement? = null,
) : RuntimeException(message)
