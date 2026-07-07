package com.zseven.zode

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

enum class ProtocolMethod(val wireName: String) {
    Initialize("initialize"),
    ThreadStart("thread/start"),
    ThreadResume("thread/resume"),
    ThreadList("thread/list"),
    ThreadRead("thread/read"),
    ThreadDelete("thread/delete"),
    ThreadNameSet("thread/name/set"),
    TurnStart("turn/start"),
    FsReadFile("fs/readFile"),
    FsWriteFile("fs/writeFile"),
    FsCreateDirectory("fs/createDirectory"),
    FsGetMetadata("fs/getMetadata"),
    FsReadDirectory("fs/readDirectory"),
    FsRemove("fs/remove"),
    FsCopy("fs/copy"),
    CommandExec("command/exec"),
    ModelList("model/list"),
    ConfigRead("config/read"),
    ConfigList("config/list"),
    SkillsList("skills/list"),
    SkillsRead("skills/read"),
    HooksList("hooks/list"),
    McpServerStatusList("mcpServerStatus/list"),
    PluginList("plugin/list"),
}

@Serializable
data class JsonRpcRequest(
    val id: JsonElement,
    val method: String,
    val params: JsonElement? = null,
)

@Serializable
data class JsonRpcNotification(
    val method: String,
    val params: JsonElement? = null,
)

class RpcException(
    val code: Int,
    override val message: String,
    val data: JsonElement? = null,
) : RuntimeException(message)
