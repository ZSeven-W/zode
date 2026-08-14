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
- **Per-turn system reminders** (`zode-core/src/reminders.rs`): an
  `AfterToolUse` hook tracks fs-tool mtimes / TodoWrite calls; `turn_blocks`
  prepends a `<system-reminder>` block for external file changes, stale todo
  lists, and git-branch drift. Every notice fires once (baselines advance).
- **Compacted history stays displayable** (`zode-core/src/sessions/archive.rs`):
  compaction tombstones replaced messages in the live store, and the
  transcript `.jsonl` persists that view. Before any full transcript rewrite,
  newly-tombstoned originals are diffed off the old file into an additive
  sidecar (`<sessions>/<id>/compacted.jsonl`). The engine keeps a
  display-only `compacted_overlay` (populated by `compact_sized`, seeded from
  the sidecar on resume, carried in `CarryState`); resume/Ctrl+L/extension
  history rebuilds merge it over tombstones via
  `sessions::overlay_compacted_originals`, so restoring a session no longer
  loses the pre-compaction conversation while the model context stays
  compacted. `/clear` drops both overlay and archive; `fork` copies the
  archive; the TUI session delete removes the whole sidecar dir (same as
  CLI `session rm`). The engine's display overlay is ALSO fed into the
  archive at save time (`save_with_originals`), so messages compacted
  before their first save still get preserved when the harness captured
  them; `/export` renders through the overlay too. Accepted tradeoffs
  (reviewed): archive writes stay best-effort (never fail the transcript
  save — losing the current turn to protect old history is the worse
  trade); no cross-process transaction spans old-transcript read + archive
  merge + rewrite (two zode processes writing the SAME session id is not a
  supported flow; in-process saves are serialized by the TUI save lock);
  team-internal teammate transcripts persist raw (compacted resume equals
  the live teammate's context; no UI renders their history); the in-loop
  QueryLoop auto-compact still can't capture originals the engine never
  saw (vendor hooks carry no message payload — upstream agent-rs work).
- **Append safety is identity-checked, not count-checked**: compaction
  tombstones IN PLACE (count preserved), so `Session::append`'s count guard
  alone can pass on a rewritten prefix and splice index-shifted duplicates
  (bricking the transcript on load). Two zode-side guards force a full
  rewrite instead: the engine's `prefix_dirty` latch (set by the PostCompact
  hook + `compact_sized`, event-loss-proof — a compact notice dropped by the
  abort fence can't be missed) and the `PersistedWatermark` identity check
  in `zode-tui/src/tab.rs` (uuid + tombstone-kind of the last persisted
  message). Residual benign gap: in-loop microcompact rewrites tool-result
  payloads in place without firing hooks, so an abort-raced micro notice can
  leave un-elided tool results on disk (larger resume context, never
  corruption).
- **Session ledger** (`zode-core/src/session_ledger.rs`): the L2 memory layer
  between the transcript (L1) and noema (L3). Harness-maintained write-ahead
  working state — user requests verbatim, shell-command heads with latest
  outcomes (hook-fed), and compaction analysis bullets teed from the noema
  sink (OPEN QUESTION included). Rendered (~2k-token cap) into the
  post-compact restore message beside files + recall, so a lossy summary
  can't erase session facts. Session-scoped: carried via `CarryState`,
  wiped by `/clear`, never persisted to disk.
- **Effort maps to real knobs**: `map_effort` (engine.rs) forwards `low|medium|high`
  when reasoning is opted in; on Anthropic, `/effort high` maps to an 8192
  thinking budget and `/effort medium` to 4096 (`low` stays thinking-free —
  the fast tier) on legacy models, and to adaptive thinking
  (`thinking:{type:"adaptive"}` + `output_config.effort`) on Opus 4.6+/Sonnet
  4.6+/Sonnet 5/Opus 4.7/4.8/Fable 5; OpenAI reasoning requests use
  `max_completion_tokens`. Interleaved thinking (reasoning carried across
  tool calls in one turn) needs the vendor Anthropic provider to add the
  beta + block handling — upstream agent-rs work, not zode-side.
- **Tool surface extras**: `MultiEdit` (zode-native,
  `zode-core/src/tools/multi-edit.rs`) applies several FileEdit-style
  replacements to one file atomically (all validated in memory, one write,
  any failure leaves the file untouched); tracked by undo/reminder hooks and
  the fs-escalation pass like FileEdit. `WebSearch` registers only when a
  Tavily key exists (`webSearch.tavilyApiKey` config or `$TAVILY_API_KEY`) —
  no key, no tool, so the model never sees an uncallable name.
  `tools.deferNonCore` (default false) opts the standard profile into the
  lite-style narrowing: ~20 everyday tools stay visible, the long tail
  (browser/desktop/op/team/LSP/…) is reachable via ToolSearch.
- **Per-turn date drift**: the system prompt's date is frozen at session
  start for prompt-cache stability; `ReminderTracker::note_date` reports a
  midnight crossing once per day change through the per-turn reminder.
  (Tests that assemble engines with a hardcoded past date and assert exact
  stored user text will trip this reminder — assemble with today's date.)
- **Auto-resume after overflow compaction**: a turn cut off by the context
  limit (`is_context_overflow_error` in zode-tui app.rs) latches
  `SessionTab::resume_after_compact`; the latch also forces the between-turn
  auto-compaction to run, and the next SUCCESSFUL compaction queues
  `COMPACT_RESUME_PROMPT` so the interrupted task continues instead of the
  tab sitting silently idle. Interactive tabs only (scheduler turns keep
  fail-closed retries); a user-typed prompt or `/clear` clears the latch;
  the synthetic prompt is excluded from prompt history like the goal-loop
  prompts.

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
  client, installer, launcher, planner, tools (`OpRead`/`OpWrite`).
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

### `OpRead` / `OpWrite` tools

The agent can call OpenPencil tools directly via two tool wrappers:

- **`OpRead`** — calls any tool that matches the read-only classification
  without requiring user approval. Classification uses a small write override
  for create-like tools such as `export_nodes`, then a curated read allowlist
  plus read prefixes (`get_`, `list_`, `snapshot_`, `count_`, `find_`,
  `read_`, `export_`, `search_`). The explicit read set includes
  `open_document`, `get_editor_state`, `get_variables`, `get_guidelines`,
  `get_style_guide`, `get_screenshot`, `ToolSearch`, `find_empty_space`,
  `read_nodes`, `batch_get`, `export_design_md`, and
  `search_all_unique_properties`.
- **`OpWrite`** — calls any other MCP tool; gated by the standard
  `ApprovalGate` (asks the user before executing).

Both tools are registered in the `op` tool group and connect to OpenPencil
via `OpConnection::ensure` (which may trigger install/launch — see below).
Their connection, HTTP, and design-pipeline work use the root turn's abort
controller. If a remote mutation is cancelled after dispatch, its outcome is
latched as unverified external work so a scheduler turn cannot replay it.

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

### Design generation (`OpDesign` / `/op <prompt>`)

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

**`OpDesign` tool** — registered in the `op` tool group (`plugin.rs`).
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

`OpConnection::ensure` runs on every `/op` subcommand or `OpRead`/`OpWrite`
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
   The child is owned by a tracked supervisor with a five-minute deadline,
   64-KiB-per-stream capture, abort propagation, and process-tree termination
   (Unix process group / Windows `taskkill /T`). Installer output cannot grow
   the turn without bound, and the watchdog does not release the turn until the
   supervisor has reaped it.
5. **Launch** — if installed but not running (port file absent / ping fails),
   prompt the user then spawn `op start` as a detached background process. This
   detach is intentional product behavior, not supervised foreground work; it
   sets the turn's unresolved-external-work latch. A scheduler-owned turn that
   auto-launches the GUI is therefore stopped/disabled for human review instead
   of automatically replayed.

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
| `BrowserRead` | `screenshot`, `snapshot`, `console`, `network`, `tabs`, `downloads` | `ReadOnly` | None — registered ungated, like `OpRead` |
| `BrowserAct` | `navigate`, `click`, `type`, `key`, `scroll` | `Mutating` | `PermissionGatedTool` + `BrowserGateView` |
| `BrowserEval` | arbitrary JS expression | `Mutating` | `PermissionGatedTool` + `BrowserGateView`, own independent always-allow flag |
| `BrowserTabs` | `new`, `close`, `select` | `Mutating` | `PermissionGatedTool` + `BrowserGateView` |
| `BrowserUpload` | set absolute local paths on a file input selected by `selector` or `ref` | `Mutating` | Independent per-call approval; no always-allow |

All browser tools take a lease on the session (`BrowserSession::lease`), which
serializes every backend operation across tabs and concurrent tool calls —
the browser has one "current tab", so overlapping navigate/click/eval calls
from different agent turns would otherwise race.

The mutating trio is wrapped in `browser_gated()` (`gate.rs`) *before*
`wrap_mutating_tools` runs in `engine.rs`, and their names are added to the
mutating-allow list passed into that pass, so they are never double-gated
behind a second, context-blind `PermissionGatedTool`. `BrowserEval` gets
its own `PermissionGatedTool` instance (and thus its own always-allow flag)
independent of `BrowserAct`/`BrowserTabs` — allowing "always allow" for
navigation doesn't silently also allow-always arbitrary JS execution.
`BrowserUpload` canonicalizes and validates every path before prompting,
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
| `/browser target <managed\|bridge>` | Switch target and persist it to global `browser.defaultTarget`; `bridge` routes tools through the paired Chrome extension |
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

`BrowserRead` action `screenshot` always saves the JPEG to
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
`BrowserRead {"action":"downloads"}` returns only downloads observed by the
current backend session, newest first. A completed entry includes `path` only
when CDP reports the actual saved path.

### Chrome extension bridge

The bridge extension requires its `downloads` permission and must be reloaded
or redistributed after this feature update. Its download cache starts when the
bridge WebSocket connects and never queries `chrome.downloads` history, so
downloads from before that connection cannot leak through `BrowserRead`.
Entries not provably caused by the controlled tab are marked with conservative
`profile` or `unknown` attribution. An older extension returns an explicit
`bridge extension too old / downloads unsupported` error.

The bridge target controls the user's real Chrome profile. Run `/browser pair`
to start a `127.0.0.1` WebSocket listener, then open the zode bridge extension
popup and enter the displayed WS port and 6-digit code. A successful pairing
stores a long-term token in Chrome storage and in `~/.zode/browser-bridge.json`
(0600); pairing is ONE-TIME — the extension reconnects with the token
automatically (browser startup, install/update, and a 1-minute `chrome.alarms`
retry while disconnected; requires the `alarms` permission, extension reload
needed after this update). The auto path only reconnects, never launches zode.
Chrome BLOCKS `chrome-extension://` URLs launched by external programs
(ERR_BLOCKED_BY_CLIENT — macOS `open`, Windows `start`, Linux `xdg-open`
alike), so zode cannot reliably open the pairing page. The extension opens
it ITSELF: a pre-auth `probe` hello (`ClientHello::Probe` →
`ServerHello::PairingStatus { active }`, answer-and-close, origin-checked,
reveals only the pairing-window bit) lets the worker poll the default
bridge port on its ~30s alarm; while a window is active it opens
`popup.html?port=…` via `chrome.tabs.create` (extension-origin navigation
is not blocked), deduped by a 2-minute cooldown. The `/browser pair` note
explains this and keeps the type-the-URL-in-the-address-bar fallback
(typed navigation is browser-initiated and allowed) plus the store link.

Extension version 0.5.0 also requests `nativeMessaging`. Every normal TUI
launch registers the running zode executable as host
`ai.zode.browser_bridge` for the fixed extension origin and stores the current
workspace in `~/.zode/browser-native-host.json`. When the side panel cannot
reach the saved WebSocket port, it starts that native host, which enters
`TuiApp::run_extension_daemon`: the existing extension task, agent, approval,
history, and WebSocket code runs without terminal setup or rendering. The
native pipe is only a lifecycle/bootstrap channel; browser and task payloads
continue to use the authenticated localhost WebSocket. Closing the native port
shuts down the daemon.

The bridge drives one sticky tab. A side-panel `turn/start` first targets the
active page beside the panel, allowing `BrowserRead` to analyze that page
without creating or grouping a new tab. Standalone TUI/CLI bridge acquisition
still creates a background `about:blank` tab in the "zode" tab group rather
than taking over a human tab. Explicit human navigation of a controlled tab
(address-bar / bookmark / omnibox `webNavigation` transitions) or a
`canceled_by_user` debugger detach hands it back, and the next standalone
action acquires a new tab. zode's own `Page.navigate` calls are rewritten to
`transitionType: "link"` so they never trigger the handoff listener.
Screenshots briefly activate a background tab and restore the previously
active one.

Extension task engines pin their `browser_*` tools to the bridge target:
`extension_tasks.rs` assembles them via
`EngineTemplate::with_browser_target_override(Some(BrowserTarget::Bridge))`,
which flows through `BrowserToolDeps::target_override` /
`BrowserSession::lease_as` and the `BrowserGateView` `_target` field. A
side-panel turn therefore always drives the page beside the panel — never a
managed Chrome — regardless of the session-wide `/browser target`
selection, which still governs TUI-created tabs. The pin lives only on the
assembling template; extension reassembles never write it back to the
global template.

The extension stream forwards assistant text AND extended-thinking deltas
(`message/delta`, thinking uses `role: "thinking"`); tool payloads
(input/output) are deliberately never forwarded — only tool identity and
failed/completed status. Every ToolUse bumps a per-turn segment counter and
text/thinking messageIds carry it (`…:assistant-<n>` / `…:thinking-<n>`),
so the panel timeline interleaves thinking/text runs with the tool calls
between them in true order. The React panel renders consecutive tool calls
as one compact activity group (one row per call).

Extension turns append a hidden `browser_side_panel_context` text block in
`zode-tui/src/app/extension_tasks.rs`. It tells the agent that the active page
is the primary context, that ambiguous/deictic page questions should call
`BrowserRead` before answering, and that it must not inspect the local
workspace unless the user explicitly asks about project code or files. The
block is sent to the engine but omitted from the panel's displayed user text.

The extension version tracks the workspace crate version: `package.json`
carries the full value while the manifest's `version` carries its numeric
core, since Chrome only accepts dotted integers there. On a pre-release
(e.g. `0.1.0-beta.9`) the full value also goes in the manifest's
`version_name`; on a stable release the two are equal and `version_name`
is omitted. `manifest.test.js` reads `Cargo.toml` and enforces this.

Clipboard images paste into the composer (`clipboardImages` in `App.tsx` renames
each blob to `pasted-<stamp>.<ext>` before the normal attachment upload path).
Image attachments carry a `previewUrl` object URL: the composer chip renders it
as a thumbnail, and on submit `rememberTurnImages` moves it into a bounded
(40-turn) `taskId:turnId` map that `MessageCard` reads, so the sent picture also
shows inside its user bubble. These previews are panel-local — snapshot
`HistoryMessage`s carry neither `turnId` nor image data, so a reload or an
authoritative snapshot refresh drops them.

### Side-panel element picker

The composer's element button ("选择页面元素提问") starts a DevTools-style
picker on the page beside the panel. It is pure CDP — `DOM.enable` +
`Overlay.setInspectMode {searchForNode}` on the already-attached debugger
session — so it adds no manifest permission, no `host_permissions`, and no
content script. The click arrives as `Overlay.inspectNodeRequested`;
`background.js` resolves the backend node, runs one `Runtime.callFunctionOn`
that returns the element summary (unique CSS selector, label, tag, text,
bounding box, attributes, capped `outerHTML`, page title/URL, iframe flag),
and broadcasts `zode-element-picked`. A password input's value is never
captured.

Starting a pick makes the active tab the controlled tab (the same retarget a
side-panel turn performs), so the selector the model receives and the tab
`browser_*` tools drive are the same page. Picks are broadcast, but only the
panel that started one adopts it, so a second window can't steal it. A pick is
cancelled by the button, Esc in the panel, page navigation/close, debugger
detach, or a two-minute timeout.

The panel holds one picked element per task beside the draft, and `submit()`
adds it to `turn/start` as `selection` **only when present** — an older zode
(whose `TurnStartParams` is `deny_unknown_fields`) therefore keeps accepting
ordinary turns, and the panel maps its `unknown field \`selection\`` error to a
"upgrade zode" notice while keeping the chip for retry. `arm_extension_turn`
turns the selection into a hidden `<browser_selected_element>` content block
(sent to the model, omitted from the panel transcript) plus a
`[Selected element: …]` line in the displayed text, mirroring attachments. A
selection alone is a valid turn.

The toolbar icon is theme-adaptive: an offscreen document (`offscreen.html`,
`MATCH_MEDIA` reason, `offscreen` permission) posts `zode-theme` messages on
load/change/ping and the worker calls `chrome.action.setIcon` to swap between
the dark `icons/zode-*.png` and light `icons/zode-light-*.png` sets. Static
manifest icons stay dark.

Install and update details live in `extensions/chrome/README.md`. The
extension ID is `hmnlhofbekmkhmifkfkkmmpigijlkcca` — the Chrome-Web-Store
build, the single shipping channel. The Rust bridge server checks that ID in
the WebSocket Origin header AND in the native-messaging manifest's
`allowed_origins` — but the accept list is **config-driven, not hardcoded**:
`browser.extensionIds` (an array; unset → the published ID) is installed via
`bridge::server::set_allowed_extension_ids` before any listener accepts a
connection. Setting the key REPLACES the list, and its FIRST entry is also
what the pairing/connect popup URL targets (`primary_extension_id()`). A
locally-built extension is NOT covered by the store's key — the manifest's
own `key` yields a different ID (unpacked/CRX), so add that ID there to pair
against a dev build. Store upload ZIP: `extensions/chrome/pack-store.sh`
(strips the `key`); `is_invocation_arg` shape-checks any
`chrome-extension://<id>/` origin (it runs before config loads; real access
stays gated by the manifest allowlist + WS token/Origin).

### Integration tests

Unit tests run under plain `cargo test --workspace` (mock backend, no
Chrome required). A real-Chrome end-to-end test is opt-in and `#[ignore]`d
by default:

```bash
ZODE_BROWSER_IT=1 cargo test -p zode-core --test browser_it -- --ignored
```

It launches a headless managed Chrome, navigates a `data:` URL, evaluates
JS, screenshots, snapshots, clicks by ref, and checks console-log capture.

## Desktop ghost cursor & Esc stop

Desktop automation is visualized by `crates/zode-overlay` — a zero-permission
macOS helper (borderless, click-through, never-key windows) spawned lazily on
the first desktop action. zode never moves the real mouse cursor; the overlay
draws a fake one (Dubins-path flight, ported from pi-computer-use, MIT).

- Wire: JSON lines on the helper's stdin (`show`/`move`/`chip`/`hide`/`quit`),
  duplicated serialize/parse types pinned by identical golden tests in
  `zode-core/src/desktop/overlay.rs` and `zode-overlay/src/proto.rs`.
- Helper discovery: `desktop.overlayHelperPath`, else `zode-overlay` next to
  the zode executable; a missing helper silently disables visualization.
- The AX actor sends a fire-and-forget overlay command *before* each action
  (element center + owning CGWindowID for click/scroll/set_value; a generic
  `⌨` chip for type/key). Typed text is never shown in the overlay.
- While desktop automation is active, a CGEventTap (armed in
  `DesktopSession::lease`/`resolve_backend`, `desktop/esc-watch.rs`) swallows
  global Esc and interrupts ALL running turns (same path as TUI Esc), then
  disarms and hides the overlay; it also disarms at turn end. Tap-creation
  failure is non-fatal (Esc support simply absent).
- Config (`desktop.*`): `ghostCursor` (default true), `escCancel` (default
  true), `overlayHelperPath` (default null).
- Opt-in IT (needs a logged-in macOS session):
  `ZODE_DESKTOP_IT=1 cargo test -p zode-overlay --test overlay-it -- --ignored`.

### macOS text input strategy (`type` action)

`type_text` (`zode-core/src/desktop/ax/input.rs`) sends each char with its
real US-layout virtual keycode plus the unicode payload. Text containing any
char without a keycode (CJK, punctuation) is delivered via the pasteboard
instead (`ax/paste.rs`): write general pasteboard → synthesize Cmd+V to the
target pid → restore the previous pasteboard **text** after a 300 ms settle.
Rationale: apps with custom key handling (WeChat 4.x) read the keycode, not
the payload — keycode 0 is kVK_ANSI_A, so payload-only synthesis rendered
every such char as "a". Known limitation: a non-text pasteboard (image/files)
cannot be saved and is lost on the paste path. Window tokens minted by
`DesktopRead windows` are the backend's window index and round-trip through
`DesktopSession::resolve_window` unchanged.

## /loop, /schedule & task timing

- **`/loop <30s|5m|1h> [--max N] <prompt>`** — session-only recurring turns on
  the current tab; `list` / `stop [id]`. Minimum interval 30s. A due prompt is
  queued via the same `queued_input` path as the goal loop (never interrupts a
  running turn; skips a trigger while its prompt is still queued).
- **`/schedule add <hh:mm|mon hh:mm|every 2h> <prompt>`** — persisted to
  `~/.zode/schedules.json` (atomic tmp+rename; corrupt files are quarantined to
  `.corrupt`). Missed triggers while zode is not running are skipped, never
  replayed. Cross-process dedup is first-writer-wins on `lastFiredMs`
  (exact epoch milliseconds, so two 30-second slots in one minute remain
  distinct). Fire/retry/roster mutations are compare-and-swap updates under the
  store lock; each active attempt also holds a stable per-schedule OS file
  lock. `list` / `rm <id>` / `enable|disable <id>`.
- Both live in `zode-core/src/scheduler/` (pure `due()` core, driven by the TUI
  tick); parsers in `commands/loop-sched.rs`.
- **Background watchdog scope** — only scheduler-owned `/loop` and `/schedule`
  turns are registered with `zode-tui/src/app/watchdog.rs`; ordinary
  interactive turns are not. This is an in-process turn watchdog, not an OS
  supervisor: it does not restart zode after a process crash or machine
  restart.
- **Liveness and cancellation** — source-side provider, tool, and nested-agent
  activity shares a turn signal and refreshes it before UI-channel delivery,
  avoiding false timeouts when the UI is busy. `inactivityTimeoutSecs` is an
  idle limit and `maxRuntimeSecs` is an absolute turn limit. A breach first
  sends the normal cooperative abort, then hard-aborts the local turn task
  after `abortGraceSecs` if no terminal event has arrived. The scheduler slot
  and attempt lease are released only after every tracked provider/tool/hook/
  subprocess/nested-agent worker reaches quiescence. A second five-second miss
  quarantines the tab/store and disables the job while retaining its lease;
  real quiescence is still required before final persistence and release. A
  watchdog timeout is journaled as a failed, partial turn rather than as a user
  interruption.
- **Queued-attempt liveness** — the same inactivity duration is the maximum
  claim-to-start wait. A scheduler preflight failure keeps the exact occurrence
  queued rather than consuming it; expiry is side-effect-free failure and uses
  the normal bounded retry/backoff path. `/watchdog` and `/tasks` show queue age.
- **Recovery** — failures retry with capped exponential backoff; success resets
  the consecutive count. After `maxRetries` is exhausted, the loop is stopped
  or the persisted schedule is disabled. Manual interruption, job removal,
  explicit disabling, and tab close suppress pending recovery. The safety
  policy is conservative around non-idempotent work: automatically retry only
  when no side effect was observed; if a mutation may have completed, stop or
  disable the job and wait for human review. Manual cancellation after a
  mutating tool started follows the same fail-closed rule. Intentionally
  detached work (`BashRun`, detached GUI launch) latches unresolved external
  work and stops recurrence even when the local turn otherwise succeeds.
  Persisted active-attempt tokens are
  paired with per-job OS locks: a contended lock belongs to another live zode
  process and is not touched; a free lock with the exact token is an orphaned,
  execution-state-unknown attempt and disables the schedule on startup rather
  than replaying it.
- **Lease finalization** — a terminal roster CAS must commit before its OS lease
  is released. Transient I/O failures enter a retrying finalizer; a stale CAS is
  classified under the store lock and durably disabled without clearing a
  different owner's token. Graceful shutdown rejects new work and drains these
  finalizers. Claimed-but-unstarted fires restore their prior watermark, and
  claimed retries restore their exact retry token; explicit edit/remove/disable
  remains cancellation. Scheduler turns skip detached post-turn extraction.
- **External quiescence boundary** — the worker/lease fence proves local
  provider, tool, hook, subprocess, and nested-agent ownership has ended. An
  MCP server, browser extension, desktop actor, or other remote system may have
  accepted a mutation that its protocol cannot revoke. Cancellation latches
  that state as unresolved and disables the job for human review; never claim
  that the remote action itself was rolled back.
- **Watchdog config and visibility** — top-level camelCase
  `backgroundWatchdog` supports `enabled` (default `true`),
  `inactivityTimeoutSecs` (`900`), `maxRuntimeSecs` (`3600`),
  `abortGraceSecs` (`10`), `maxRetries` (`3`), `initialBackoffSecs` (`5`), and
  `maxBackoffSecs` (`300`). `/watchdog status` reports effective config plus
  live/retry state; `/tasks` includes the same health lines beside background
  shells and running turns.
- **Due-check anchoring** — `ScheduleSpec::next_after` returns a time strictly
  *after* its `now` argument, so `Scheduler::due()` anchors it on the job's own
  persisted history, never on process startup or the observing wall clock.
  Interval slots are exact multiples from that anchor; daily/weekly jobs keep
  their intrinsic calendar phase, and missed backlog coalesces to the latest
  due slot. It stamps `last_fired_ms` to the trigger point, not the observing
  tick, so `fire_ms_hint` and the cross-process CAS dedup agree. `due()` reads no
  clock of its own. `/schedule enable` atomically writes a fresh persisted
  anchor, so disabled time is not replayed.
- **Runtime roster convergence** — the TUI refreshes the fallible authoritative
  schedule roster while running, imports persisted retries, revalidates queued
  claims before start, and recovers only provably orphaned active tokens.
  Interval schedules use absolute epoch slots in the host path so DST fallback
  cannot pause or duplicate their elapsed-time cadence. The persistence promise
  is process-crash recovery, not sudden-power-loss or hardware durability.
- **Job prompts are plain prompts** — `parse_loop` / `parse_schedule` reject a
  leading `/` *or* `!`. Slash dispatch is active-tab-scoped and its non-turn
  paths can't hand back the pending-attribution entry, so allowing it would
  mis-target background tabs and leak attribution; the `!cmd` shell branch
  returns from `submit()` before that entry is consumed, with the same leak.
- **Loops die with their tab** — `close_active_tab` calls
  `Scheduler::stop_loops_for_owner`, otherwise `due()` would keep incrementing
  `runs` for a job with no tab to run on (burning a `--max N` budget with zero
  executions) and the job would linger in `/loop list`.
- **Scheduler injection** — due prompts are queued via `queued_input`.
  `dispatch_scheduler_queued()` drains scheduler-owned prompts from the tick on
  EVERY idle tab, active included (the active tab routes back through `submit()`
  so tick- and keypress-driven drains behave identically) — otherwise an
  unattended `/loop` waits for a keypress while anti-pileup swallows later fires.
  User-typed queued input is never auto-drained. Turn spawn is the
  tab-parameterized `start_turn_on_tab`. Failure attribution uses an
  occurrence-aware FIFO keyed by `(tab_id, prompt)`, so equal-text jobs remain
  distinct. A persisted schedule atomically claims its fire timestamp, active
  token, and OS attempt lease before entering this queue; the exact lease moves
  into the turn and remains held until tracked workers and final transcript/
  index persistence finish. Queue edit/removal, `/loop stop`, `/schedule
  rm|disable`, and tab close purge only the matching occurrences and exact
  tokens. Persistence failure quarantines the job rather than releasing it for
  overlap.
- **Interval formatting** — schedule list/confirmation echo renders intervals as
  compact round-trippable tokens (e.g. `every 2h`, not `every 2h 00m`).
- **DST handling** — a nonexistent local time (spring-forward gap) skips *past*
  that occurrence to the following one, so a job scheduled inside the gap can
  never wedge; ambiguous (fall-back) times fire once at the earliest valid
  instant. Never epoch-0.
- **Timing** — `TurnRecorder` stamps `durationMs` on `tool.completed` and
  `turn.completed` run events (journaled; old journals parse as `None`). The
  TUI shows per-tool `· 1.2s` suffixes, a `✓ done · 34s · 3 tools` turn footer,
  and humanized elapsed in `/tasks` — all via
  `zode_core::duration_fmt::format_duration_ms`.
- **UI strings** — new UI text is added to the `EXTRA` hand-maintained overlay
  in `zode-core/src/i18n.rs`.

## External agents

Zode can register explicitly configured external agent CLIs as `Task` tool
`agent_type`s. Known manual presets cover Claude Code, Codex, opencode, Cline,
Google Antigravity, Cursor CLI, Kiro CLI, Pi, and xAI Grok Build; arbitrary CLI
commands use custom profiles. Design doc:
`docs/superpowers/specs/2026-07-16-agent-team-design.md` (Phase A).

- **Registration is manual and resolution is stat-only**: PATH is never
  scanned to auto-register CLIs. Only `externalAgents.agents` entries appear;
  their bare command names are resolved on a sanitized PATH (or an explicit
  path), canonicalized, and never executed before approval.
- **Explicit discovery**: `/external-agents` performs a stat-only scan of known
  preset binary names and shows registration state. `/external-agents discover`
  atomically adds missing presets to global config without overwriting existing
  entries; the TUI then reassembles the active idle engine. This command does
  not change the manual-only startup policy.
- **Trust model (not a sandbox)**: an external agent runs IN-PLACE and is
  not gated per-operation by zode. The first call shows a dedicated trust
  approval (full argv, cwd, env allowlist names, the CLI's own sandbox
  level, content hash; "version unverified"). Approving "session" stores a
  fingerprint grant in CarryState (never persisted); the binary is
  re-hashed before every spawn and `--version` is verified after first
  approval. `--yolo` fails closed unless the profile sets `trusted: true`.
- **Self-gated Task router**: `ZodeTaskTool` (crates/zode-core/src/task_tool.rs)
  wraps the upstream TaskTool. Internal agent_types keep today's gating
  bit-for-bit; `permissions.ask=["Task"]` yields exactly one prompt per
  call. Plan/read-only mode has no Task at all (unchanged).
- **Runner hygiene** (crates/zode-core/src/external_agents/): env starts
  from `env_clear()` + allowlist (loader vars always refused; provider API
  keys not passed by default — use `envAllow`), prompt goes via stdin where
  supported, stdout/stderr drain concurrently, kill uses the process group,
  aftermath reports best-effort `changed_files` and clears the whole
  FileStateCache (needs `FileStateCache::clear()` from vendor/agent).
- **Cost**: external usage is attributed to `external:<profile>:<model>`,
  never the parent model (`__external_agent__` result discriminant).
- **Config** (`externalAgents`, camelCase): `enabled`, `timeoutSecs`
  (default 1800), `maxConcurrent` (process-wide, default 2), `agents` map
  (preset profiles use `command`/`extraArgs`/`envAllow`/`trusted`; custom
  profiles add `args`, `promptTransport`, `output`, and optional generic JSONL
  `textSource` / `sessionIdSource` pointers plus `resumeArgs` and optional
  host-selected `newSessionArgs`). Argv transport requires a `{prompt}`
  placeholder; session templates require a `{session_id}` placeholder.
  Same-key entries replace wholesale across config layers.

Opt-in real-CLI integration test:

```bash
ZODE_EXTAGENT_IT=1 cargo test -p zode-core --test extagent_it -- --ignored
```

## Self-evolving harness

Zode carries a process-wide self-evolving harness
('zode-core/src/evolution.rs', built on 'crates/cordis-rs') that runs a
generate → evaluate → select → retire loop over its capability units:

- **Built-in tools stay Rust.** The evolvable layer is **generated
  JavaScript** evaluated by 'zode-core/src/js_plugin.rs' in a dedicated
  QuickJS runtime (rquickjs, already a workspace dep) — no compiler is
  required on the target machine, and hot replacement is just evaluating
  new source. Each JsPlugin gets a hard memory limit
  (JS_GENE_MEMORY_LIMIT, default 16 MiB) and a per-call interrupt deadline;
  a runaway/leaking gene becomes a Failed fiber and quarantines through
  the evolution layer instead of harming the host.
- **Genes are tool groups today** (the same grouping '/plugin' uses); each
  group is a lazily-spawned fiber in a bounded gene pool
  ('evolution.capacity', default 64). Fitness comes from the agent hook
  pipeline: every AfterToolUse/PostToolUseFailure event is attributed to
  its tool group ('plugin::group_of') and scored
  (uses − 10·failures − 100·panics − 5·restarts). 'unfit_groups()' names
  candidates for the plugin manager to disable.
- **Generated genes**: 'EvolutionHarness::spawn_js_gene(name, source,
  config, provenance)' installs a JS gene into the same pool; the JS
  source is the content hash (dedupe + the code key the agent persists
  generated code under). The guest protocol is a factory returning
  'apply(host)' with host.log/on/emit/effect/config; guest callbacks stay
  inside the JS runtime under a global registry, so nothing 'js-lifetimed
  crosses into Rust.
- **Genome persistence**: '<config-dir>/evolution/genome.json' (atomic
  write, debounced every 32 results + on drop); tool-group genes restore
  with carried fitness on restart, generated JS genes are skipped unless
  their source is respawned explicitly.
- **Config** ('evolution', camelCase): 'enabled' (default true),
  'capacity', 'maxRestarts'. 'init_from_config' runs from
  'EngineTemplate::new', so every entry path (TUI/CLI/extension daemon)
  shares one genome; engine tests disable it via 'test_cfg()' and cover
  the hook path in 'assemble_registers_evolution_hook'.
- The cordis-rs side ('crates/cordis-rs/src/process.rs') additionally
  supports subprocess plugins (any executable speaking a JSON-lines
  protocol) for environments that DO have a compiler: dispose kills the
  child process, and swapping binaries = live replacement.

## Agent team (Phase B)

Zode can build a collaborating team of internal and external agents on top of
the external-agent layer. Design: `docs/superpowers/specs/2026-07-16-agent-team-design.md`.

- **Teammates** are `Internal` (a persistent in-process QueryLoop over a shared
  MessageStore, with its own provider/model) or `External` (a manually
  registered agent CLI). Resumable external profiles preserve conversation
  context; one-shot profiles run as stateless teammates with a fresh process
  per send. The leader is the root model.
- **Tools** (group `team`, `tools:team` to disable): `TeamHire`
  (`{agent,name,role,provider?,model?,tools?}` — external hires need a
  one-time TeamMemberSession trust approval; internal hires don't),
  `TeamSend` (`{to,message,claims?}` — busy-check → atomic claim → dispatch,
  returns the reply plus `@ask` relays), `TeamDismiss`, `TeamList`, and the
  board/claim tools (`TeamBoardRead/update/append`, `TeamClaim/release`).
- **Board** is host-managed under `<cwd>/.zode/team/` (the `.zode` sandbox
  carveout stays read-only for tools): `board.json` written atomically under a
  stable `board.lock`, section updates CAS'd on a revision counter. Claims are
  subtree-aware TTL leases, holder-identity injected by the host (never from
  tool input), confined to the canonical cwd. A claim prevents double-claiming,
  not out-of-bounds writes (detected after the fact via changed_files).
- **Lifecycle**: one `Arc<TeamManager>` per tab, carried across engine
  rebuilds in `CarryState`; `TeamManager::shutdown()` is explicit (Drop can't
  await). The exclusive `team.lock` fs4 OS lock is the sole ownership
  authority (dead process auto-releases; no heartbeat takeover); acquired
  lazily on the first mutating op. team.json carries an HMAC — a mismatch
  quarantines the file rather than auto-recovering.
- **Collaboration** is leader-mediated (turn-end, not live): teammates end a
  reply line with `@ask <name>: <question>`; the leader relays it. Plays
  (pipeline / debate / swarm) are prompt guidance injected only when the team
  group is active.
- **Internal teammates** run an in-process QueryLoop that shares the leader's
  permission gate / hooks / file cache (same sandbox + edit history), inherit
  a role-filtered tool set (reviewer/researcher default read-only; a hire
  `tools` list may only narrow), pull model/system from a matching AgentDef,
  and report their token usage per teammate. Their history persists per
  teammate under `~/.zode/agent/sessions/team/`.
- **Persistence & recovery**: hire/dismiss/send write `team.json` (HMAC-signed,
  §4.2); on the first mutating op after a restart the roster is recovered
  (resumable external teammates keep their session id, while every external
  teammate re-approves trust on the next send; internal teammates rebuild
  their provider and reload history). An
  HMAC mismatch quarantines the file. Claims are TTL-renewed while a send is
  in flight so long tasks keep their reservation.
- **`/team`** — bare `/team` opens a read-only roster + board panel (↑↓ scroll,
  Esc close); `/team status` / `/team board` print text; `/team dismiss <name>`
  removes a teammate. Plan/read-only mode keeps only `TeamList` /
  `TeamBoardRead`.

Opt-in real-CLI team test:

```bash
ZODE_EXTAGENT_IT=1 cargo test -p zode-core --test team_it -- --ignored
```
