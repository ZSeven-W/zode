# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build --workspace                 # Build all crates
cargo run -p zode                       # Run the TUI
cargo run -p zode -- -p "<prompt>"      # Headless single turn (stream to stdout)
cargo run -p zode -- --no-tui           # Plain readline REPL
cargo test --workspace                  # Run all tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                        # Licenses / advisories / bans
```

> Clone with `--recurse-submodules` — the agent runtime is the `vendor/agent`
> submodule. After pulling, `git submodule update --init` if it's missing.

## Architecture

Zode is an AI coding CLI in Rust. It consumes the `agent` runtime
(`vendor/agent` submodule, shared with OpenPencil) and adds the terminal
product layer. It is a Cargo workspace of three crates:

```text
zode/
├── vendor/agent/          agent-rs submodule (agent + agent-tools-code crates)
└── crates/
    ├── zode/              bin: arg parsing, headless modes (-p / --no-tui), dispatch
    ├── zode-core/         UI-agnostic: config, engine, tools, commands, history,
    │                      sandbox, skills, mcp, cost, instructions, approvals
    └── zode-tui/          ratatui chrome: app loop, chat, dialogs, themes, tabs
```

Dependency direction: `zode` → `zode-tui` → `zode-core` → `vendor/agent`.
`zode-core` never depends on the TUI or the binary.

### Key design points

- **The agent runtime is upstream.** Don't fork it; feed gaps back to agent-rs
  (e.g. the optional `TaskAgentConfig` cwd/file_cache/hooks fields).
- **Interactive permission lives in a tool decorator** (`PermissionGatedTool`
  + `ApprovalGate`), not in the QueryLoop — agent-rs 0.1.0 does not pump the
  approval queue, so the `PermissionManager` carries only hard-deny rules and
  runs in Bypass; interactive allow/always/deny happens in the gate.
- **MCP tools** are wrapped in `ZodeMcpTool` (agent-rs has no Tool adapter),
  named `mcp__<server>__<tool>`.
- **Skills** are loaded into a registry and surfaced via a `SkillTool` plus a
  system-prompt index.
- **Multi-session tabs**: each tab is an isolated `ZodeEngine` (own message
  store, cost tracker, turn state) built from an `EngineTemplate`, sharing one
  approval channel. Sub-agents (Task tool) inherit the parent's final gated +
  sandboxed tool registry plus its permissions/hooks/cwd/file_cache.

### Data flow

1. **Startup** (`crates/zode/src/main.rs`): parse args → load config → build
   provider/gate/sandbox → assemble engine → dispatch to TUI / `-p` / `--no-tui`.
2. **TUI** (`crates/zode-tui/src/app.rs`): `tokio::select!` over terminal input,
   agent events (tagged with `tab_id` + `turn_id`), approval requests, and a tick.
3. **Engine** (`crates/zode-core/src/engine.rs`): rebuilds a `QueryLoop` per turn
   from shared `Arc` state; tools are wrapped (sandbox → gate → ToolSearch).

## Conventions

- **Max 800 lines per file** — split when exceeded.
- **One component per file**, single responsibility.
- **kebab-case** for `.rs` filenames.
- **English comments** in source (`.rs`/`.toml`); Chinese is fine in spec/plan
  markdown and test fixtures.
- **Tests:** inline `#[cfg(test)]` modules; env-mutating tests use
  `#[serial_test::serial]`.

## Git Commit Convention

[Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`

**Types:** `feat`, `fix`, `refactor`, `perf`, `style`, `docs`, `test`, `chore`, `build`, `ci`

**Scopes:** `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`

**Rules:** Subject in English, lowercase start, no period, imperative mood. Body optional — explain **why** not what.

## Pre-Commit Checklist

```bash
cargo fmt --all -- --check && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo test --workspace
```

## OpenPencil control (`op-bridge`)

Zode can drive a running OpenPencil instance over its local MCP endpoint.
The feature is built across three crates:

- `zode-core/src/openpencil/` — config, port-file discovery, transport,
  client, installer, launcher, planner, tools (`op_read`/`op_write`).
- `zode-core/src/commands/op.rs` — `/op <subcommand>` parser.
- `zode-tui/src/app.rs` — `/op` slash-command handler + consent modal.

### `/op` slash command

Type `/op <subcommand>` in the TUI input.
The TUI popup shows subcommand hints while typing `/op ` (Up/Down/Tab to
navigate, Enter/Tab to confirm, Esc to dismiss).

| Command | Effect |
|---------|--------|
| `/op status` | Print connection state (connected / port / none) |
| `/op design 'F1=I("rect",{})'` | Run a batch_design DSL string |
| `/op get_document_info` | Call the MCP tool with empty args |
| `/op insert {"type":"rect","x":0,"y":0}` | Shorthand MCP call with JSON args |
| `/op call <tool> <json>` | Explicit tool-name + JSON args |

Available subcommands (autocomplete list): `status`, `design`, `insert`,
`update`, `delete`, `move`, `copy`, `page`, `vars`, `selection`, `call`.

`/op status` is a zode-side connection report, not an MCP `tools/call`.

### `op_read` / `op_write` tools

The agent can call OpenPencil tools directly via two tool wrappers:

- **`op_read`** — calls any tool that matches the read-only classification
  without requiring user approval. Classification is **prefix-based**: any
  tool whose name starts with `get_`, `list_`, `snapshot_`, `count_`,
  `find_`, `read_`, `export_`, or `search_` is read. In addition, a curated
  explicit set is always treated as read regardless of prefix:
  `read_nodes`, `batch_get`, `export_design_md`, `search_all_unique_properties`,
  and the full set of per-prefix tools listed above.
- **`op_write`** — calls any other MCP tool; gated by the standard
  `ApprovalGate` (asks the user before executing).

Both tools are registered in the `op` tool group and connect to OpenPencil
via `OpConnection::ensure` (which may trigger install/launch — see below).

### `openpencil.*` config keys

Set in `~/.zode/config.json` (or override the directory with `$ZODE_CONFIG_DIR`).
The file is JSON with camelCase keys; `openpencil` is a nested object:

```json
{
  "openpencil": {
    "releaseTag": "0.8.0",
    "autoLaunchGui": true,
    "installCommand": null
  }
}
```

Notes:
- `releaseTag` default is `"0.8.0"` (no leading `v`). The installer prepends
  `v` when building the download URL, so writing `"v0.8.0"` would double the
  prefix and break installs.
- `installCommand` is `Option<String>`. Omit the key (or use `null`) to use
  the platform default. An empty string `""` would override the default with
  an empty command and break installs.

All keys are optional; absent keys fall back to built-in defaults.

### Connect / install / launch flow

`OpConnection::ensure` runs on every `/op` subcommand or `op_read`/`op_write`
tool call:

1. **Discover** — reads `~/.openpencil/.op-mcp-port` for `{"port": N, "token": "..."}`.
2. **Ping** — JSON-RPC POST to `http://127.0.0.1:<port>/mcp` with body
   `{"jsonrpc":"2.0","id":1,"method":"ping","params":null}`. No `Authorization`
   header is sent (localhost trust boundary — both ping and tool calls are
   unauthenticated POSTs). The response must have `result.server=="openpencil-mcp"`,
   `result.mode=="live"`, and `result.token` equal to the token read from the
   port file (the server echoes its own token; the client validates by
   comparison, not by sending the token as a credential).
3. **Attach** — if ping succeeds, use the live connection.
4. **Install** — if no port file exists, prompt the user (consent modal) then
   run the platform install script:
   - Unix: `bash -c "$OP_INSTALL_SCRIPT"` with `OP_VERSION=<releaseTag>`.
   - Windows: `powershell.exe -NoProfile -Command "$OP_INSTALL_SCRIPT"`.
   The install command and argv are shown in the consent prompt before running.
5. **Launch** — if installed but not running (port file absent / ping fails),
   prompt the user then spawn `op start` as a detached background process.

The localhost trust boundary means: both ping and tool calls go to
`http://127.0.0.1:<port>/mcp` without auth headers. The token in the port
file is validated by comparing it against the value echoed in the ping
response — it is never sent as a credential.
