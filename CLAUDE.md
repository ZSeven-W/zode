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
- `zode-core/src/commands/op.rs` — `/op <design request>` parser.
- `zode-tui/src/app.rs` — `/op` slash-command handler + consent modal.

### `/op` slash command

Type `/op <design request>` in the TUI input. This is the primary user-facing
OpenPencil flow: zode connects to a running OpenPencil instance, launches it if
needed after consent, then runs the design pipeline (plan → skeleton → content
→ refine). Users do not need to know OpenPencil MCP tool names for normal
design generation.

Compatibility / diagnostic forms:

| Command | Effect |
|---------|--------|
| `/op <design request>` | Run the design pipeline from natural language |
| `/op status` | Print connection state (connected / port / none) |
| `/op call <tool> <json>` | Hidden escape hatch for explicit MCP tool calls |
| `/op design '<operations>'` | Hidden compatibility path for `batch_design` `operations` |
| `/op generate <prompt>` | Hidden alias for `/op <prompt>` |

Autocomplete hints recommend only `status` and `call`. Raw OpenPencil MCP tool
names are intentionally not advertised in the user-facing command flow.

`/op status` is a zode-side connection report, not an MCP `tools/call`.

### `op_read` / `op_write` tools

The agent can call OpenPencil tools directly via two tool wrappers:

- **`op_read`** — calls any tool that matches the read-only classification
  without requiring user approval. Classification uses a small write override
  for create-like tools such as `export_nodes`, then a curated read allowlist
  plus read prefixes (`get_`, `list_`, `snapshot_`, `count_`, `find_`,
  `read_`, `export_`, `search_`). The explicit read set includes
  `open_document`, `get_editor_state`, `get_variables`, `get_guidelines`,
  `get_style_guide`, `get_screenshot`, `ToolSearch`, `find_empty_space`,
  `read_nodes`, `batch_get`, `export_design_md`, and
  `search_all_unique_properties`.
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

### Design generation (`op_design` / `/op <prompt>`)

Zode includes a deterministic design-pipeline orchestrator that generates a
full OpenPencil page from a natural-language prompt. Zode owns all the op MCP
calls; the agent never calls `design_skeleton`, `design_content`, or
`design_refine` directly.

**Pipeline — four steps, always in order:**

1. **Plan** — a direct LLM call (`DirectLlmContentGenerator`) produces a
   `DesignPlan`: a root frame, an ordered list of `SectionPlan`s (each with a
   skeleton spec and a content intent), and optional style/canvas hints. One
   automatic retry on error.
2. **Skeleton** — `design_skeleton` is called with the plan's `to_skeleton_args()`
   output. Returns `rootId` + `sectionIds`; parsed by `normalize_skeleton`.
   Failure here aborts — nothing else can proceed without section IDs.
3. **Content** — for each section, a second direct LLM call produces child
   PenNode JSON, then `design_content` places the nodes into that section's
   frame. Section failures are best-effort (collected in `DesignResult::failures`,
   never abort the remaining sections or the refine step). One automatic retry per
   section.
4. **Refine** — `design_refine` is called best-effort on the root frame. An error
   is folded into `DesignResult::refine` as `{error: "…"}`, not surfaced as a
   hard failure.

**Content generation** uses direct LLM calls (`llm_oneshot` → streamed
`TextDelta` events) rather than spawning a sub-agent.

**Install-agnostic guidance.** A built-in baseline (`BASELINE` constant in
`design.rs`) is always applied — it works with zero plugins. It describes an
8pt spacing scale, consistent type scale, restrained palette, generous
whitespace, and grid alignment. When the `frontend-design` or
`openpencil-design` skills are installed, their prompts are appended under
named headings via `load_guidance`. Missing skills are silently skipped (a
debug log is emitted). The pipeline never errors if a skill is absent.

**`op_design` tool** — registered in the `op` tool group (`plugin.rs`).
Safety class: `Mutating`; requires user approval via `ApprovalGate`. Input:
`{ "prompt": "<string>" }`. Drives the full pipeline against a live
OpenPencil instance and returns `{ sections, failures, refine }`.

**`/op <prompt>`** — TUI slash command. Maps to `OpCommand::Generate` in
`commands/op.rs`. `/op generate <prompt>` remains a hidden compatibility alias;
autocomplete does not advertise it.

**Key source locations:**

- `zode-core/src/openpencil/design.rs` — types (`DesignPlan`, `SectionPlan`,
  `Skeleton`, `DesignResult`), helpers (`normalize_skeleton`, `extract_json`,
  `plan_from_json`, `llm_oneshot`), guidance (`BASELINE`, `Guidance`,
  `load_guidance`), `ContentGenerator` trait + `DirectLlmContentGenerator`,
  `DesignOrchestrator::run`.
- `zode-core/src/openpencil/tools.rs` — `OpDesignTool`, `OpDesignDeps`.
- `zode-core/src/commands/op.rs` — `OpCommand::Generate` arm.

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

## Browser control

Zode has a built-in browser-automation subsystem: a process-wide
chromiumoxide-managed Chrome backend plus an extension bridge for the user's
real Chrome profile. It is built across two crates, the CLI entrypoint, and a
Chrome extension:

- `zode-core/src/browser/` — `backend.rs` (`BrowserBackend` trait + types +
  `BrowserError`), `managed.rs` (chromiumoxide `ManagedBackend` + launch
  supervisor), `session.rs` (process-wide `BrowserSession`, one shared
  backend slot, serialized leases), `gate.rs` (`BrowserGateView`, via the
  generalized `PermissionGatedTool` `GateView` hook), `tools.rs` (the shared
  browser tools), `upload.rs` (upload preflight and per-call approval),
  `managed-downloads.rs` (browser-lifetime download event cache),
  `snapshot_js.rs` (in-page JS that produces the
  ref-annotated accessibility outline), and `bridge/` (pairing/token state,
  localhost WebSocket server, and `BridgeBackend` RPC mapping).
- `zode-core/src/commands/browser.rs` — `/browser <subcommand>` parser.
- `zode-tui/src/ui/dialog/browser_panel.rs` — the `/browser` status panel;
  wired into `zode-tui/src/app.rs` alongside the slash-command handler.
- `extensions/chrome/` — MV3 extension, popup, pack script, and CRX artifact
  for controlling real Chrome through `chrome.debugger`.

### `browser_*` tools

| Tool | Actions | Safety | Approval |
|------|---------|--------|----------|
| `browser_read` | `screenshot`, `snapshot`, `console`, `network`, `tabs`, `downloads` | `ReadOnly` | None — registered ungated, like `op_read` |
| `browser_act` | `navigate`, `click`, `type`, `key`, `scroll` | `Mutating` | `PermissionGatedTool` + `BrowserGateView` |
| `browser_eval` | arbitrary JS expression | `Mutating` | `PermissionGatedTool` + `BrowserGateView`, own independent always-allow flag |
| `browser_tabs` | `new`, `close`, `select` | `Mutating` | `PermissionGatedTool` + `BrowserGateView` |
| `browser_upload` | set absolute local paths on a file input selected by `selector` or `ref` | `Mutating` | Independent per-call approval; no always-allow |

All browser tools take a lease on the session (`BrowserSession::lease`), which
serializes every backend operation across tabs and concurrent tool calls —
the browser has one "current tab", so overlapping navigate/click/eval calls
from different agent turns would otherwise race.

The mutating trio is wrapped in `browser_gated()` (`gate.rs`) *before*
`wrap_mutating_tools` runs in `engine.rs`, and their names are added to the
mutating-allow list passed into that pass, so they are never double-gated
behind a second, context-blind `PermissionGatedTool`. `browser_eval` gets
its own `PermissionGatedTool` instance (and thus its own always-allow flag)
independent of `browser_act`/`browser_tabs` — allowing "always allow" for
navigation doesn't silently also allow-always arbitrary JS execution.
`browser_upload` canonicalizes and validates every path before prompting,
shows canonical paths and sizes in the approval, and deliberately treats an
"always" response as one-call approval. Invalid paths never open a prompt.
`BrowserGateView` enriches every approval prompt with `_target`
(`"managed"`/`"bridge"`) and, when resolvable without blocking, `_page_url`
— session state the model's own tool-call input cannot be trusted to
report accurately. The tool group id is `browser` (disable via
`tools:browser`, like `tools:op`).

### `/browser` slash command

Bare `/browser` opens the TUI status panel (target, connection state, and
row actions: select target, manage permissions, reconnect extension,
toggle default-enabled). Subcommands are the scriptable fast path; the
popup shows hints while typing `/browser ` (Up/Down/Tab to navigate,
Enter/Tab to confirm, Esc to dismiss).

| Command | Effect |
|---------|--------|
| `/browser status` | Print target/running/headless state (session-local, no MCP-style round trip) |
| `/browser launch` | Launch the managed browser now |
| `/browser close` | Close the managed browser |
| `/browser pair` | Start a localhost bridge listener and print a 6-digit pairing code plus WS port for the Chrome extension |
| `/browser target <managed\|bridge>` | Switch target; `bridge` routes tools through the paired Chrome extension |
| `/browser screenshot [path]` | Take a screenshot, optionally to an explicit path |

### `--browser` / `--no-browser` CLI flags

Session-only overrides, never persisted to `config.json`:
`--browser` force-enables the `browser` tool group for this run;
`--no-browser` force-disables it. With neither flag, `browser.enabled`
(config, default `true`) decides.

### `browser.*` config keys

Set in `~/.zode/config.json`, camelCase JSON, nested under `browser`:

```json
{
  "browser": {
    "enabled": true,
    "executable": null,
    "headless": false,
    "profileDir": null,
    "defaultTarget": "managed",
    "viewport": { "width": 1280, "height": 800 }
  }
}
```

Defaults (all fields optional; getters supply these when absent):
- `enabled` → `true`.
- `executable` → `null`, meaning auto-detect an installed Chrome / Chromium
  / Edge binary.
- `headless` → `false`.
- `profileDir` → `null`, meaning `~/.zode/browser-profile` (or
  `$ZODE_CONFIG_DIR/browser-profile` when that env var is set).
- `defaultTarget` → `"managed"`. `"bridge"` is also valid; if zode starts
  with bridge selected before the extension is connected, the first browser
  tool returns a pairing hint instead of silently falling back to managed.
- `viewport` → `1280x800`.

### Screenshot return path (content-blocks sentinel)

`browser_read` action `screenshot` always saves the JPEG to
`<config-dir>/screenshots/shot-<millis>.jpg` and returns a JSON object
shaped as the reserved `__agent_content_blocks__` sentinel (defined and
consumed in the `agent` submodule, `query/loop_.rs`):

```json
{ "__agent_content_blocks__": [ { "type": "image", ... } ], "text": "screenshot saved: <path>" }
```

The query loop only inlines the image blocks into the model's tool-result
turn when the active provider's capability bit `tool_result_images` is
`true` — currently only the Anthropic provider sets this. Every other
provider (OpenAI-compatible, Ollama) gets the `text` fallback only, i.e.
the saved file path, never the raw bytes. This gate — plus the sentinel's
exact-shape validation (top-level object holding *only*
`__agent_content_blocks__` + optional `text`; blocks must all be `Text` or
base64 `Image` under the same 5 MiB inline cap as file attachments; error
results never parse the sentinel; anything malformed degrades to the text
fallback) — lives entirely in `vendor/agent`, not in zode. Landing this
feature required bumping the `vendor/agent` submodule pointer to a version
that added the `tool_result_images` capability and the sentinel-to-blocks
conversion.

### Chrome 136+ and profiles

Chrome 136 and later refuse CDP (`--remote-debugging-port`) connections
against the user's default profile. `managed.rs` therefore always launches
against a dedicated, persistent profile directory (`profileDir`, see
above) rather than the user's regular Chrome profile — this doubles as the
M1 story for retaining login state across sessions (cookies persist in
that profile directory between launches). There is no cross-profile
credential sharing with the user's everyday browser.

Managed downloads are allowed into `<config-dir>/downloads` (normally
`~/.zode/downloads`), which is created with owner-only permissions on Unix.
`browser_read {"action":"downloads"}` returns only downloads observed by the
current backend session, newest first. A completed entry includes `path` only
when CDP reports the actual saved path.

### Chrome extension bridge

The bridge extension requires its `downloads` permission and must be reloaded
or redistributed after this feature update. Its download cache starts when the
bridge WebSocket connects and never queries `chrome.downloads` history, so
downloads from before that connection cannot leak through `browser_read`.
Entries not provably caused by the controlled tab are marked with conservative
`profile` or `unknown` attribution. An older extension returns an explicit
`bridge extension too old / downloads unsupported` error.

The bridge target controls the user's real Chrome profile. Run `/browser pair`
to start a `127.0.0.1` WebSocket listener, then open the zode bridge extension
popup and enter the displayed WS port and 6-digit code. A successful pairing
stores a long-term token in Chrome storage and in `~/.zode/browser-bridge.json`
(0600), so reconnects can authenticate with the token.

The bridge drives one sticky zode-owned tab: acquisition always creates a
fresh background `about:blank` tab in the "zode" tab group (never taking over
or focusing a human tab); explicit human navigation of that tab (address-bar
/ bookmark / omnibox `webNavigation` transitions) or a `canceled_by_user`
debugger detach hands it back, and the next action acquires a new tab. zode's
own `Page.navigate` calls are rewritten to `transitionType: "link"` so they
never trigger the handoff listener. Screenshots briefly activate the tab and
restore the previously active one.

The toolbar icon is theme-adaptive: an offscreen document (`offscreen.html`,
`MATCH_MEDIA` reason, `offscreen` permission) posts `zode-theme` messages on
load/change/ping and the worker calls `chrome.action.setIcon` to swap between
the dark `icons/zode-*.png` and light `icons/zode-light-*.png` sets. Static
manifest icons stay dark.

Install and update details live in `extensions/chrome/README.md`. The shipped
extension ID is `hcabdgpfhoclfgnknddadgfhhdnlkloc`; the manifest embeds the
public key so unpacked and packed installs use the same ID, which the Rust
bridge server checks in the WebSocket Origin header.

### Integration tests

Unit tests run under plain `cargo test --workspace` (mock backend, no
Chrome required). A real-Chrome end-to-end test is opt-in and `#[ignore]`d
by default:

```bash
ZODE_BROWSER_IT=1 cargo test -p zode-core --test browser_it -- --ignored
```

It launches a headless managed Chrome, navigates a `data:` URL, evaluates
JS, screenshots, snapshots, clicks by ref, and checks console-log capture.
