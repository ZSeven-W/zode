# Zode SDKs

This directory contains thin clients for zode server mode.

## Transport

Clients spawn a plain `zode server` child process and talk newline-delimited
JSON-RPC 2.0 over its stdin/stdout — **stdio is the default**, so the SDKs do
not pass any `--listen` flag:

```sh
zode server              # stdio (default) — what every SDK spawns
zode server --listen stdio://   # the same thing, spelled out
zode server --listen off        # start nothing and exit 0
```

### WebSocket transport

For a long-lived, out-of-process server, launch a loopback WebSocket listener.
The host must be exactly `127.0.0.1` or `[::1]`; port `0` picks a free port:

```sh
zode server --listen ws://127.0.0.1:0
```

On startup the server writes a private credentials file, `server.json`, into the
zode config directory (`<config-dir>/server.json`, i.e. `~/.zode/server.json`
unless `$ZODE_CONFIG_DIR` overrides it), mode `0600`:

```json
{ "port": 9876, "pid": 12345, "token": "…64 hex chars…" }
```

Clients read `port` + `token` from that file and open
`ws://127.0.0.1:<port>` with an `Authorization: Bearer <token>` header. The
listener stays bound to loopback and rejects any upgrade whose bearer token does
not match. The file is removed on clean shutdown by the owning pid.

## Surface

The server exposes: initialization + capability discovery, thread metadata
lifecycle, **streaming turns** (model output and tool calls surface as
notifications — see [Notifications](#notifications)), interactive **approvals**,
filesystem helpers, one-shot `command/exec`, model list/set, config
read/list/write, and read-only discovery for skills, hooks, MCP server status,
and plugins.

Still out of scope: account/auth, marketplace, remote-control, Realtime,
standalone process spawn, background terminals, thread archive/fork, goals, and
app connectors.

## Packages

| SDK | Release channel |
|-----|-----------------|
| [Rust](rust/) | Tagged Git dependency plus a standalone source bundle on the GitHub Release |
| [TypeScript](typescript/) | GitHub npm Packages: `@zseven-w/zode-sdk` |
| [Python](python/) | Wheel and source distribution on the GitHub Release |
| [Go](go/) | Go module tag: `sdk/go/v0.1.0-beta.9` |
| [Kotlin/JVM](kotlin/) | GitHub Maven Packages: `com.zseven.zode:zode-sdk` |

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

## Approval policy

`initialize` accepts an optional `approvalPolicy` that governs whether
side-effecting work (tool calls, `command/exec`, filesystem writes) runs, is
blocked, or is confirmed with the client. The value is echoed back in the
`initialize` result's `approvalPolicy` field so a client can confirm what the
server accepted.

| Value | Meaning |
|-------|---------|
| `readOnly` | **Default.** Side-effecting operations are denied; read-only work runs freely. Omitting `approvalPolicy` selects this. |
| `auto` | Side-effecting operations run without asking. Use this for headless automation that must write files / run commands. |
| `prompt` | The server asks the client to approve each side-effecting operation via a server→client `approval/request` (see [Server → client requests](#server--client-requests)). |

> **Breaking change (this release):** the default is now `readOnly`. A client
> that previously relied on unrestricted `command/exec` / `fs/writeFile` must now
> pass `"auto"` (or handle `"prompt"`).

When `approvalPolicy: "prompt"` is in effect, the server sends an
`approval/request` and waits; the client answers with
`{ "decision": "allow" | "allowAlways" | "deny" }`. `allowAlways` grants the
current operation and suppresses further prompts for equivalent operations in
the session.

## Method Reference

All field names are JSON field names. Empty params can be omitted by low-level
clients, but passing `{}` keeps request construction consistent across SDKs.

| Method | Params | Result | Use |
|--------|--------|--------|-----|
| `initialize` | `{"clientInfo":{"name":string,"version":string},"approvalPolicy"?:"readOnly"|"auto"|"prompt"}` | `{"serverInfo":{"name":string,"version":string},"zodeHome":string,"platformFamily":string,"platformOs":string,"capabilities":string[],"approvalPolicy":"readOnly"|"auto"|"prompt"}` | Start the connection and read server metadata. Must be called once before other methods. `approvalPolicy` defaults to `readOnly`; the result echoes the accepted policy. |
| `thread/start` | `{"cwd"?:string,"model"?:string}` | `{"thread":{"id":string,"name":string,"cwd":string,"model":string,"status":"notLoaded"|"loaded"|"running"}}` | Create a metadata-only thread. Defaults are `cwd: "."`, `model: "default"`, and name `"(untitled)"`. |
| `thread/list` | `{}` | `{"threads":Thread[]}` | List known server-mode threads. |
| `thread/read` | `{"threadId":string}` | `{"thread":Thread}` | Read one thread by id. |
| `thread/resume` | `{"threadId":string}` | `{"thread":Thread}` | Return the same thread metadata shape as `thread/read`; current stage does not attach a running UI session. |
| `thread/name/set` | `{"threadId":string,"name":string}` | `{}` | Rename a thread. |
| `thread/delete` | `{"threadId":string}` | `{}` | Delete a thread from the server-mode registry. |
| `turn/start` | `{"threadId":string,"input":string,"model"?:string}` | `{"turn":{"id":string,"threadId":string,"status":"running"|"completed"|"interrupted"|"failed"}}` | Start a turn. The result returns immediately with the running turn; model output and tool calls stream as [notifications](#notifications) (`turn/started` → `item/agentMessage/delta` / `item/*` → `turn/completed`). |
| `turn/interrupt` | `{"threadId":string,"turnId":string}` | `{}` | Interrupt a running turn; the turn ends with a `turn/interrupted` notification. |
| `fs/readFile` | `{"path":string}` | `{"dataBase64":string}` | Read a file and return base64 bytes. |
| `fs/writeFile` | `{"path":string,"dataBase64":string}` | `{}` | Decode base64 bytes and write the file. |
| `fs/createDirectory` | `{"path":string,"recursive"?:boolean}` | `{}` | Create a directory. `recursive` defaults to `true`. |
| `fs/getMetadata` | `{"path":string}` | `{"isDirectory":boolean,"isFile":boolean,"isSymlink":boolean,"createdAtMs":number,"modifiedAtMs":number}` | Stat a path. Timestamps are Unix epoch milliseconds, or `0` when unavailable. |
| `fs/readDirectory` | `{"path":string}` | `{"entries":[{"fileName":string,"isDirectory":boolean,"isFile":boolean}]}` | List directory entries sorted by file name. |
| `fs/remove` | `{"path":string,"recursive"?:boolean,"force"?:boolean}` | `{}` | Remove a file or directory. Use `recursive: true` for non-empty directories; use `force: true` to ignore missing paths. |
| `fs/copy` | `{"sourcePath":string,"destinationPath":string,"recursive"?:boolean}` | `{}` | Copy a file, or copy a directory when `recursive: true`. |
| `command/exec` | `{"command":string[],"cwd"?:string}` | `{"processId":string,"stdout":string,"stderr":string,"exitCode"?:number}` | Run one command to completion and capture output. `command[0]` is the executable; the rest are args. |
| `model/list` | `{}` | `{"providers":[{"id":string,"name":string,"baseUrl"?:string,"kind":string,"models":[{"id":string,"name":string,"context"?:number,"maxOutput"?:number}]}]}` | Read configured model providers and model metadata. |
| `model/set` | `{"threadId":string,"model":string}` | `{"thread":Thread}` | Set the active model for a thread; returns the updated thread metadata. |
| `config/read` | `{}` | `{"config":object}` | Read the resolved zode config as JSON. |
| `config/list` | `{}` | `{"keys":string[]}` | List top-level config keys. |
| `config/write` | `{"patch":object,"persist"?:boolean}` | `{"appliesTo":string}` | Merge a JSON patch into the config. `persist` defaults to `false` (session-only); `true` writes to disk. `appliesTo` reports the scope the change was applied to. |
| `skills/list` | `{}` | `{"skills":[{"name":string,"description":string}]}` | List available skills for the current working directory. |
| `skills/read` | `{"name":string}` | `{"name":string,"description":string,"prompt":string,"inputSchema":object}` | Read one skill definition by name. |
| `hooks/list` | `{}` | `{"hooks":[{"event":string,"tool"?:string,"script":string}]}` | List configured hooks. |
| `mcpServerStatus/list` | `{}` | `{"servers":[{"name":string,"enabled":boolean,"connected":boolean}]}` | List configured MCP servers and connection state. |
| `plugin/list` | `{}` | `{"plugins":[{"id":string,"kind":string,"name":string,"description":string,"detail":string,"enabled":boolean}]}` | List installed local plugins known to zode. |

## Notifications

A turn streams as server→client JSON-RPC **notifications** (no `id`, no
response). Register a notification handler (see each SDK README) to receive
them. Field names below are the exact wire keys; every frame carries `threadId`
and `turnId`.

| Method | Params | Emitted |
|--------|--------|---------|
| `turn/started` | `{"threadId":string,"turnId":string}` | Once, when a turn begins. |
| `item/agentMessage/delta` | `{"threadId":string,"turnId":string,"delta":string}` | For each streamed chunk of assistant text (`delta` is the incremental text). |
| `item/started` | `{"threadId":string,"turnId":string,"itemId":string,"item":{"id":string,"type":"dynamicToolCall","tool":string,"arguments":object,"status":"inProgress"}}` | When a tool call starts. The `item` object is nested; `itemId` also appears at top level. |
| `item/completed` | `{"threadId":string,"turnId":string,"itemId":string,"item":{"id":string,"type":"dynamicToolCall","status":"completed"|"failed","output":object}}` | When a tool call finishes. `item.status` is `completed` or `failed`. |
| `turn/error` | `{"threadId":string,"turnId":string,"error":{"code":string,"message":string}}` | A non-fatal error surfaced mid-turn. |
| `turn/completed` | `{"threadId":string,"turnId":string,"finalText":string,"usage":{"inputTokens":number,"outputTokens":number,"cacheRead":number,"cacheCreate":number}}` | Terminal: the turn finished successfully. `finalText` is the full assistant message; `usage` reports token counts. |
| `turn/interrupted` | `{"threadId":string,"turnId":string}` | Terminal: the turn was interrupted (via `turn/interrupt` or an approval `deny`). |
| `turn/failed` | `{"threadId":string,"turnId":string,"error":string}` | Terminal: the turn failed. `error` is a message string. |

Exactly one of `turn/completed`, `turn/interrupted`, or `turn/failed` ends each
turn.

## Server → client requests

When `approvalPolicy: "prompt"` is active, the server sends a JSON-RPC
**request** (it has an `id` and expects a response) to ask the client to approve
a side-effecting operation:

| Method | Params | Response |
|--------|--------|----------|
| `approval/request` | `{"approvalId":string,"kind":"tool"|"command"|"fsWrite","summary":string,"threadId"?:string,"turnId"?:string,"tool"?:string,"input"?:object}` | `{"decision":"allow"|"allowAlways"|"deny"}` |

`kind` says what is being approved (`tool` call, `command` exec, or `fsWrite`).
Answer with `allow` (once), `allowAlways` (this and equivalent future
operations), or `deny` (blocks it; the turn ends interrupted). A client that
does not register an approval handler denies by default.

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
| `turn/interrupt` | `ProtocolMethod::TurnInterrupt` | `ProtocolMethod.TurnInterrupt` | `ProtocolMethod.TURN_INTERRUPT` | `ProtocolMethodTurnInterrupt` | `ProtocolMethod.TurnInterrupt` |
| `fs/readFile` | `ProtocolMethod::FsReadFile` | `ProtocolMethod.FsReadFile` | `ProtocolMethod.FS_READ_FILE` | `ProtocolMethodFsReadFile` | `ProtocolMethod.FsReadFile` |
| `fs/writeFile` | `ProtocolMethod::FsWriteFile` | `ProtocolMethod.FsWriteFile` | `ProtocolMethod.FS_WRITE_FILE` | `ProtocolMethodFsWriteFile` | `ProtocolMethod.FsWriteFile` |
| `fs/createDirectory` | `ProtocolMethod::FsCreateDirectory` | `ProtocolMethod.FsCreateDirectory` | `ProtocolMethod.FS_CREATE_DIRECTORY` | `ProtocolMethodFsCreateDirectory` | `ProtocolMethod.FsCreateDirectory` |
| `fs/getMetadata` | `ProtocolMethod::FsGetMetadata` | `ProtocolMethod.FsGetMetadata` | `ProtocolMethod.FS_GET_METADATA` | `ProtocolMethodFsGetMetadata` | `ProtocolMethod.FsGetMetadata` |
| `fs/readDirectory` | `ProtocolMethod::FsReadDirectory` | `ProtocolMethod.FsReadDirectory` | `ProtocolMethod.FS_READ_DIRECTORY` | `ProtocolMethodFsReadDirectory` | `ProtocolMethod.FsReadDirectory` |
| `fs/remove` | `ProtocolMethod::FsRemove` | `ProtocolMethod.FsRemove` | `ProtocolMethod.FS_REMOVE` | `ProtocolMethodFsRemove` | `ProtocolMethod.FsRemove` |
| `fs/copy` | `ProtocolMethod::FsCopy` | `ProtocolMethod.FsCopy` | `ProtocolMethod.FS_COPY` | `ProtocolMethodFsCopy` | `ProtocolMethod.FsCopy` |
| `command/exec` | `ProtocolMethod::CommandExec` | `ProtocolMethod.CommandExec` | `ProtocolMethod.COMMAND_EXEC` | `ProtocolMethodCommandExec` | `ProtocolMethod.CommandExec` |
| `model/list` | `ProtocolMethod::ModelList` | `ProtocolMethod.ModelList` | `ProtocolMethod.MODEL_LIST` | `ProtocolMethodModelList` | `ProtocolMethod.ModelList` |
| `model/set` | `ProtocolMethod::ModelSet` | `ProtocolMethod.ModelSet` | `ProtocolMethod.MODEL_SET` | `ProtocolMethodModelSet` | `ProtocolMethod.ModelSet` |
| `config/read` | `ProtocolMethod::ConfigRead` | `ProtocolMethod.ConfigRead` | `ProtocolMethod.CONFIG_READ` | `ProtocolMethodConfigRead` | `ProtocolMethod.ConfigRead` |
| `config/list` | `ProtocolMethod::ConfigList` | `ProtocolMethod.ConfigList` | `ProtocolMethod.CONFIG_LIST` | `ProtocolMethodConfigList` | `ProtocolMethod.ConfigList` |
| `config/write` | `ProtocolMethod::ConfigWrite` | `ProtocolMethod.ConfigWrite` | `ProtocolMethod.CONFIG_WRITE` | `ProtocolMethodConfigWrite` | `ProtocolMethod.ConfigWrite` |
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
