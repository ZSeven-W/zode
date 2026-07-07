# Zode SDKs

This directory contains thin clients for zode server mode.

All SDKs talk to:

```sh
zode server --listen stdio://
```

The current server surface is intentionally limited to capabilities zode already
backs: initialization, thread metadata operations, turn registry operations,
filesystem helpers, one-shot `command/exec`, and read-only discovery for models,
config, skills, hooks, MCP server status, and plugins.

Not included in this stage: account/auth, marketplace, remote-control,
Realtime, websocket runtime, standalone process spawn, background terminals,
thread archive/fork, goals, and app connectors.

## Packages

- [Rust](rust/)
- [TypeScript](typescript/)
- [Python](python/)
- [Go](go/)
- [Kotlin/JVM](kotlin/)

Each SDK exposes a native `ProtocolMethod` enum or constant set with the current
stable JSON-RPC method names.

## Calling Methods

Call `initialize` once before any other request. After that, each SDK sends the
same JSON-RPC params and receives the same result shape.

```rust
server.request_method(ProtocolMethod::CommandExec, json!({"command": ["sh", "-c", "printf hi"]})).await?;
```

```ts
await client.request(ProtocolMethod.CommandExec, { command: ["sh", "-c", "printf hi"] });
```

```python
await client.request(ProtocolMethod.COMMAND_EXEC, {"command": ["sh", "-c", "printf hi"]})
```

```go
client.RequestMethod(ctx, zodesdk.ProtocolMethodCommandExec, map[string]any{
    "command": []string{"sh", "-c", "printf hi"},
}, &out)
```

```kotlin
client.request(ProtocolMethod.CommandExec, params)
```

Use raw string methods only when you intentionally need low-level JSON-RPC
experiments. Normal integrations should use `ProtocolMethod`.

## Method Reference

All field names are JSON field names. Empty params can be omitted by low-level
clients, but passing `{}` keeps request construction consistent across SDKs.

| Method | Params | Result | Use |
|--------|--------|--------|-----|
| `initialize` | `{"clientInfo":{"name":string,"version":string}}` | `{"serverInfo":{"name":string,"version":string},"zodeHome":string,"platformFamily":string,"platformOs":string,"capabilities":string[]}` | Start the connection and read server metadata. Must be called once before other methods. |
| `thread/start` | `{"cwd"?:string,"model"?:string}` | `{"thread":{"id":string,"name":string,"cwd":string,"model":string,"status":"notLoaded"|"loaded"|"running"}}` | Create a metadata-only thread. Defaults are `cwd: "."`, `model: "default"`, and name `"(untitled)"`. |
| `thread/list` | `{}` | `{"threads":Thread[]}` | List known server-mode threads. |
| `thread/read` | `{"threadId":string}` | `{"thread":Thread}` | Read one thread by id. |
| `thread/resume` | `{"threadId":string}` | `{"thread":Thread}` | Return the same thread metadata shape as `thread/read`; current stage does not attach a running UI session. |
| `thread/name/set` | `{"threadId":string,"name":string}` | `{}` | Rename a thread. |
| `thread/delete` | `{"threadId":string}` | `{}` | Delete a thread from the server-mode registry. |
| `turn/start` | `{"threadId":string,"input":string}` | `{"turn":{"id":string,"threadId":string,"status":"running"|"completed"|"interrupted"|"failed"}}` | Start the current registry-level turn. This stage records a running turn only; it does not stream model output yet. |
| `fs/readFile` | `{"path":string}` | `{"dataBase64":string}` | Read a file and return base64 bytes. |
| `fs/writeFile` | `{"path":string,"dataBase64":string}` | `{}` | Decode base64 bytes and write the file. |
| `fs/createDirectory` | `{"path":string,"recursive"?:boolean}` | `{}` | Create a directory. `recursive` defaults to `true`. |
| `fs/getMetadata` | `{"path":string}` | `{"isDirectory":boolean,"isFile":boolean,"isSymlink":boolean,"createdAtMs":number,"modifiedAtMs":number}` | Stat a path. Timestamps are Unix epoch milliseconds, or `0` when unavailable. |
| `fs/readDirectory` | `{"path":string}` | `{"entries":[{"fileName":string,"isDirectory":boolean,"isFile":boolean}]}` | List directory entries sorted by file name. |
| `fs/remove` | `{"path":string,"recursive"?:boolean,"force"?:boolean}` | `{}` | Remove a file or directory. Use `recursive: true` for non-empty directories; use `force: true` to ignore missing paths. |
| `fs/copy` | `{"sourcePath":string,"destinationPath":string,"recursive"?:boolean}` | `{}` | Copy a file, or copy a directory when `recursive: true`. |
| `command/exec` | `{"command":string[],"cwd"?:string}` | `{"processId":string,"stdout":string,"stderr":string,"exitCode"?:number}` | Run one command to completion and capture output. `command[0]` is the executable; the rest are args. |
| `model/list` | `{}` | `{"providers":[{"id":string,"name":string,"baseUrl"?:string,"kind":string,"models":[{"id":string,"name":string,"context"?:number,"maxOutput"?:number}]}]}` | Read configured model providers and model metadata. |
| `config/read` | `{}` | `{"config":object}` | Read the resolved zode config as JSON. |
| `config/list` | `{}` | `{"keys":string[]}` | List top-level config keys. |
| `skills/list` | `{}` | `{"skills":[{"name":string,"description":string}]}` | List available skills for the current working directory. |
| `skills/read` | `{"name":string}` | `{"name":string,"description":string,"prompt":string,"inputSchema":object}` | Read one skill definition by name. |
| `hooks/list` | `{}` | `{"hooks":[{"event":string,"tool"?:string,"script":string}]}` | List configured hooks. |
| `mcpServerStatus/list` | `{}` | `{"servers":[{"name":string,"enabled":boolean,"connected":boolean}]}` | List configured MCP servers and connection state. |
| `plugin/list` | `{}` | `{"plugins":[{"id":string,"kind":string,"name":string,"description":string,"detail":string,"enabled":boolean}]}` | List installed local plugins known to zode. |

## ProtocolMethod Names

| Method | Rust | TypeScript | Python | Go | Kotlin |
|--------|------|------------|--------|----|--------|
| `initialize` | `ProtocolMethod::Initialize` | `ProtocolMethod.Initialize` | `ProtocolMethod.INITIALIZE` | `ProtocolMethodInitialize` | `ProtocolMethod.Initialize` |
| `thread/start` | `ProtocolMethod::ThreadStart` | `ProtocolMethod.ThreadStart` | `ProtocolMethod.THREAD_START` | `ProtocolMethodThreadStart` | `ProtocolMethod.ThreadStart` |
| `thread/resume` | `ProtocolMethod::ThreadResume` | `ProtocolMethod.ThreadResume` | `ProtocolMethod.THREAD_RESUME` | `ProtocolMethodThreadResume` | `ProtocolMethod.ThreadResume` |
| `thread/list` | `ProtocolMethod::ThreadList` | `ProtocolMethod.ThreadList` | `ProtocolMethod.THREAD_LIST` | `ProtocolMethodThreadList` | `ProtocolMethod.ThreadList` |
| `thread/read` | `ProtocolMethod::ThreadRead` | `ProtocolMethod.ThreadRead` | `ProtocolMethod.THREAD_READ` | `ProtocolMethodThreadRead` | `ProtocolMethod.ThreadRead` |
| `thread/delete` | `ProtocolMethod::ThreadDelete` | `ProtocolMethod.ThreadDelete` | `ProtocolMethod.THREAD_DELETE` | `ProtocolMethodThreadDelete` | `ProtocolMethod.ThreadDelete` |
| `thread/name/set` | `ProtocolMethod::ThreadNameSet` | `ProtocolMethod.ThreadNameSet` | `ProtocolMethod.THREAD_NAME_SET` | `ProtocolMethodThreadNameSet` | `ProtocolMethod.ThreadNameSet` |
| `turn/start` | `ProtocolMethod::TurnStart` | `ProtocolMethod.TurnStart` | `ProtocolMethod.TURN_START` | `ProtocolMethodTurnStart` | `ProtocolMethod.TurnStart` |
| `fs/readFile` | `ProtocolMethod::FsReadFile` | `ProtocolMethod.FsReadFile` | `ProtocolMethod.FS_READ_FILE` | `ProtocolMethodFsReadFile` | `ProtocolMethod.FsReadFile` |
| `fs/writeFile` | `ProtocolMethod::FsWriteFile` | `ProtocolMethod.FsWriteFile` | `ProtocolMethod.FS_WRITE_FILE` | `ProtocolMethodFsWriteFile` | `ProtocolMethod.FsWriteFile` |
| `fs/createDirectory` | `ProtocolMethod::FsCreateDirectory` | `ProtocolMethod.FsCreateDirectory` | `ProtocolMethod.FS_CREATE_DIRECTORY` | `ProtocolMethodFsCreateDirectory` | `ProtocolMethod.FsCreateDirectory` |
| `fs/getMetadata` | `ProtocolMethod::FsGetMetadata` | `ProtocolMethod.FsGetMetadata` | `ProtocolMethod.FS_GET_METADATA` | `ProtocolMethodFsGetMetadata` | `ProtocolMethod.FsGetMetadata` |
| `fs/readDirectory` | `ProtocolMethod::FsReadDirectory` | `ProtocolMethod.FsReadDirectory` | `ProtocolMethod.FS_READ_DIRECTORY` | `ProtocolMethodFsReadDirectory` | `ProtocolMethod.FsReadDirectory` |
| `fs/remove` | `ProtocolMethod::FsRemove` | `ProtocolMethod.FsRemove` | `ProtocolMethod.FS_REMOVE` | `ProtocolMethodFsRemove` | `ProtocolMethod.FsRemove` |
| `fs/copy` | `ProtocolMethod::FsCopy` | `ProtocolMethod.FsCopy` | `ProtocolMethod.FS_COPY` | `ProtocolMethodFsCopy` | `ProtocolMethod.FsCopy` |
| `command/exec` | `ProtocolMethod::CommandExec` | `ProtocolMethod.CommandExec` | `ProtocolMethod.COMMAND_EXEC` | `ProtocolMethodCommandExec` | `ProtocolMethod.CommandExec` |
| `model/list` | `ProtocolMethod::ModelList` | `ProtocolMethod.ModelList` | `ProtocolMethod.MODEL_LIST` | `ProtocolMethodModelList` | `ProtocolMethod.ModelList` |
| `config/read` | `ProtocolMethod::ConfigRead` | `ProtocolMethod.ConfigRead` | `ProtocolMethod.CONFIG_READ` | `ProtocolMethodConfigRead` | `ProtocolMethod.ConfigRead` |
| `config/list` | `ProtocolMethod::ConfigList` | `ProtocolMethod.ConfigList` | `ProtocolMethod.CONFIG_LIST` | `ProtocolMethodConfigList` | `ProtocolMethod.ConfigList` |
| `skills/list` | `ProtocolMethod::SkillsList` | `ProtocolMethod.SkillsList` | `ProtocolMethod.SKILLS_LIST` | `ProtocolMethodSkillsList` | `ProtocolMethod.SkillsList` |
| `skills/read` | `ProtocolMethod::SkillsRead` | `ProtocolMethod.SkillsRead` | `ProtocolMethod.SKILLS_READ` | `ProtocolMethodSkillsRead` | `ProtocolMethod.SkillsRead` |
| `hooks/list` | `ProtocolMethod::HooksList` | `ProtocolMethod.HooksList` | `ProtocolMethod.HOOKS_LIST` | `ProtocolMethodHooksList` | `ProtocolMethod.HooksList` |
| `mcpServerStatus/list` | `ProtocolMethod::McpServerStatusList` | `ProtocolMethod.McpServerStatusList` | `ProtocolMethod.MCP_SERVER_STATUS_LIST` | `ProtocolMethodMcpServerStatusList` | `ProtocolMethod.McpServerStatusList` |
| `plugin/list` | `ProtocolMethod::PluginList` | `ProtocolMethod.PluginList` | `ProtocolMethod.PLUGIN_LIST` | `ProtocolMethodPluginList` | `ProtocolMethod.PluginList` |

Shared contract and fixture material:

- [Contract](contract/)
- [JSON-RPC fixtures](fixtures/)

Run the SDK checks that are available locally:

```sh
../scripts/test-sdks.sh
```

From the repository root:

```sh
scripts/test-sdks.sh
```
