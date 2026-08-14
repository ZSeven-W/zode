# zode v0.2.0-beta.4

**zode** is an open-source, AI-native coding assistant for your terminal: it
reads your code, runs commands, searches files, and manages git from a fast
Rust TUI with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, configuration, and behavior may change before 0.2.0 is
> stable. Please file issues with a minimal reproduction when something goes
> wrong.

This beta makes the harness self-evolving: a Cordis-inspired plugin core
(`crates/cordis-rs`), a fitness-driven evolution layer over zode's own
capability units, and a QuickJS gene runtime so the agent can generate and
hot-swap its own plugins — bounded in memory, quarantined on failure.

## A self-evolving harness

- **Cordis-inspired core** (`crates/cordis-rs`): scoped contexts
  (extend/isolate/intercept), fiber-owned lifecycle (spawn/dispose/restart),
  dependency scheduling (`inject`), a five-mode event bus, lazy services,
  and hard `MemoryBudget` caps with live stats. Everything a plugin
  acquires is freed when its fiber disposes — dropping the root context
  collects the harness.
- **Evolution layer** (`zode-core/src/evolution.rs`): tool results from the
  hook pipeline feed per-group fitness (`uses − 10·failures − 100·panics −
  5·restarts`); a bounded gene pool evicts the weakest genes as the agent
  evolves new candidates; the genome persists to
  `<config-dir>/evolution/genome.json` and restores with fitness across
  restarts. Config: `evolution.*` (`enabled`/`capacity`/`maxRestarts`).
- **QuickJS gene layer** (`zode-core/src/js_plugin.rs`): generated plugins
  are JavaScript — no compiler required on the target machine. Each gene
  runs in its own QuickJS runtime with a 16 MiB memory limit and per-call
  interrupt deadlines; a runaway gene fails its fiber and quarantines
  instead of harming zode. Built-in tools stay Rust.
- **Subprocess plugins** (`crates/cordis-rs/src/process.rs`): any
  executable speaking a JSON-lines protocol becomes a swappable plugin;
  disposing the fiber kills the child process, so compiled binaries can be
  hot-replaced where a compiler is available.
- **Observability**: `internal/plugin`, `internal/status`, and
  `internal/service` events; `unfit_groups()` names disable candidates for
  the plugin manager.

## Tests

50 harness tests (including the process and evolution suites) plus
zode-core evolution and QuickJS-gene integration tests; the end-to-end
self-test (`cargo run -p zode-core --example evolution_self_test`) exercises
the full generate → evaluate → select → retire loop and prints
`SELF-TEST PASSED`. A test report is included in the README in all 15
languages.

# zode v0.2.0-beta.3

**zode** is an open-source, AI-native coding assistant for your terminal: it
reads your code, runs commands, searches files, and manages git from a fast
Rust TUI with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, configuration, and behavior may change before 0.2.0 is
> stable. Please file issues with a minimal reproduction when something goes
> wrong.

This beta hardens the harness for long, CJK-heavy, weak-model sessions — the
compaction 400s are gone for good, MCP tools finally show their real
parameter contracts, and DeepSeek (flash included) drives tool loops that
recover instead of wedging.

## The compaction 400 wedge is closed

- **Store splits are pair-safe in every shape.** The token-midpoint split
  snaps to `tool_use → tool_result` boundaries via a balance sweep (a cut is
  legal iff no unanswered tool_use crosses it), so no split can ever orphan
  one half of a pair — including a transcript that is a single pair, and
  multi-tool batches answered across several user messages.
- **Every request is defensively sanitized.** A store damaged by an OLDER
  compaction (a severed pair) is repaired on the request copy — orphans
  downgraded to text instead of 400ing every subsequent request — and
  sessions are repaired once at load time, so even old transcripts recover.
- **Conservative CJK estimates.** CJK now counts 2 tokens per char (real BPE
  runs 1.5–3): CJK-heavy sessions compact before the provider 400s instead
  of after. The estimation-only paths can opt into exact tiktoken counts.
- **Microcompaction keeps the useful bits.** Old tool results are pruned
  head-and-tail around the marker (errors conventionally live at the END of
  a command's output) instead of being wiped — weak models stop re-running
  the same commands to recover what they just read — and interactive
  question answers (AskUserQuestion) are never cleared.

## Weak-model (lite) accommodations

- **Empty responses retry once.** DeepSeek occasionally emits a bare `stop`;
  the loop retries instead of silently ending the turn.
- **Concrete loop nudges.** The identical-call / same-tool warnings now name
  the repeated tools and preview the identical results, so a weak model sees
  WHY its calls go nowhere.
- **Lite surfaces keep MCP usable.** MCP tools stay visible under the
  lite/deferred tool narrowing, and ToolSearch searches the executable set
  only — no more discovering a tool you cannot call.

## DeepSeek-first provider work

- **Stream idle watchdog.** A provider stream that stalls between chunks
  (default 300s) becomes a retryable timeout instead of pinning the turn.
- **Reasoning passback (OpenAI dialect).** `reasoning_content` deltas surface
  as thinking and replay on tool-call turns per DeepSeek's thinking-mode
  rule; the hand-rolled SSE transport also honors `Retry-After`.
- **MCP schemas on the wire.** Tools advertise the server-declared input
  schema, so the model stops guessing argument names (passing `instruction`
  where the server declares `text`) and failing the first call.
- **Overflow wording recognized.** DeepSeek's anthropic-compat
  "exceeds the context window" phrasing triggers the auto-compact resume.

## Config fixes

- Provider `contextWindow` / `maxOutputTokens` now merge across config
  layers (a project pinning a smaller window no longer silently loses it).

## Benchmarks

- New `harness_extra` suite (12 tasks): long tool chains, failure recovery,
  real-repo exploration with dynamic graders, and Chinese ops tasks.
- Baseline refresh on DeepSeek-V4-Flash: humaneval 31/31, agentic 6/6,
  complex 5/5, hardbugs 9/9, instructions 25/25, harness_extra 12/12.

## Install

```bash
curl -fsSL https://zode.dev/install.sh | bash -s -- v0.2.0-beta.3   # macOS / Linux
```

Or download a prebuilt binary (see the beta.2 section below for the full
platform matrix; substitute `0.2.0-beta.3` in the artifact names):

| Platform | Artifact |
|---|---|
| macOS | Apple Silicon (M1+) | `zode-0.2.0-beta.3-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.2.0-beta.3-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.2.0-beta.3-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.2.0-beta.3-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.2.0-beta.3-x64-windows.zip` |

---

# zode v0.2.0-beta.2 (previous beta)

**zode** is an open-source, AI-native coding assistant for your terminal: it
reads your code, runs commands, searches files, and manages git from a fast
Rust TUI with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, configuration, and behavior may change before 0.2.0 is
> stable. Please file issues with a minimal reproduction when something goes
> wrong.

This beta is about long sessions that survive: compaction no longer loses your
conversation, no longer wedges the provider, and no longer leaves an
interrupted task sitting silently idle — and pairing your real Chrome is now a
one-time act.

## Compaction you can trust

- **The visible conversation survives compaction.** Originals of compacted
  messages are preserved in an additive sidecar
  (`~/.zode/sessions/<id>/compacted.jsonl`). Resuming a session, `Ctrl+L`,
  `/export`, and the Chrome side panel all replay the full pre-compaction
  history while the model keeps receiving only the compacted context. Forks
  carry the archive (filtered to their own transcript); `/clear` and session
  deletion remove it.
- **No more permanent 400s right after compaction.** The store-mutating
  compaction split now snaps to `tool_use → tool_result` pair boundaries.
  Previously a mid-pair split left every subsequent request rejected by
  strict OpenAI-dialect gateways ("role 'tool' must follow 'tool_calls'") —
  the session was wedged for good.
- **Interrupted work resumes itself.** A turn cut off by the context-window
  limit now triggers compaction and, once the window is freed, automatically
  queues a continuation turn instead of leaving the tab idle mid-task.
- **Transcripts can no longer be corrupted by a raced compaction.** The
  incremental-append watermark now checks the identity (uuid + kind) of the
  last persisted message, and an engine-side latch survives dropped UI
  events — either guard alone forces the safe full rewrite.
- **Post-compaction file restoration actually works.** The recent-files
  tracker listened for a parameter name the file tools never send, so the
  restore message re-attached zero files in production. Fixed, with the
  session ledger and recall pack injected exactly once per turn.

## One-time browser pairing

- **Pair once, reconnect forever.** The extension stores a long-term token
  and now reconnects on browser startup, extension updates, and a ~30-second
  retry cadence while disconnected. Restarting zode never asks you to pair
  again.
- **The pairing page opens itself.** Chrome blocks `chrome-extension://`
  URLs launched by external programs on every OS (`ERR_BLOCKED_BY_CLIENT`),
  so the extension now probes the local bridge (a pre-auth check that
  reveals only "is a pairing window open") and opens its own pre-filled
  pairing page within ~30 seconds of `/browser pair`. Typing the URL into
  the address bar remains a manual fallback.
- The extension requires the new `alarms` permission — update or reload it
  after upgrading. Store versions are now four-part (`0.2.0-beta.2` →
  `0.2.0.2`) so prerelease uploads stay monotonic.

## Sharper tools, calmer harness

- **`MultiEdit`** applies several exact-substring edits to one file
  atomically: every edit validates against the in-memory result of the
  previous one and the file is written once — any failure leaves it
  untouched.
- **`WebSearch`** registers when a Tavily key is present
  (`webSearch.tavilyApiKey` or `$TAVILY_API_KEY`); without a key the model
  never sees an uncallable tool name.
- **`tools.deferNonCore`** opts the standard profile into lite-style tool
  narrowing: ~20 everyday tools stay visible, the long tail loads through
  ToolSearch.
- **`/effort medium` now means something on Anthropic** — a real thinking
  budget instead of a silent no-op; `high` keeps the larger budget.
- **The harness stopped pushing busywork.** Verification guidance applies to
  changes the agent made (no unrequested builds or test suites on analysis
  tasks); known locations are re-read by range instead of whole files;
  compaction summaries are treated as established facts, not exploration to
  redo; skills, OpenSpec, and orchestration guidance no longer prescribe
  blanket ceremony.
- **Composer newline everywhere:** a trailing `\` + Enter continues the line
  in any terminal; Shift+Enter works on kitty-protocol terminals (kitty,
  WezTerm, Ghostty, iTerm2 3.5+ with the protocol enabled).
- Date changes mid-session are announced once per day; `permissions.ask` on
  browser/desktop tools prompts once per call through the context-aware gate
  instead of stacking a second blind prompt; TUI session deletion removes
  the whole sidecar; docs and UI strings updated across all 15 languages.

## Install

### One line (recommended)

**macOS / Linux:**

```sh
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

The installers auto-detect your OS and CPU, download the matching binary, and
put `zode` on your PATH. To pin this beta, use `--version v0.2.0-beta.2` on
macOS/Linux or `$env:ZODE_VERSION='v0.2.0-beta.2'` on Windows.

Already installed? Run `zode update` (or `zode upgrade`) to fetch this beta;
the updater includes prereleases when selecting the newest available version.

> Because this is a pre-release, GitHub's "latest" endpoint may exclude it.
> zode's installers resolve the newest release including betas.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.2.0-beta.2-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.2.0-beta.2-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.2.0-beta.2-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.2.0-beta.2-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.2.0-beta.2-x64-windows.zip` |
| Windows | ARM64 | `zode-0.2.0-beta.2-arm64-windows.zip` |

### From source

```sh
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode && cargo build --release -p zode
```

## Supported platforms

Prebuilt binaries are provided for macOS (arm64 and x64), Linux (x86_64 and
arm64, glibc), and Windows (x64 and arm64). Every architecture is built and
packaged independently in CI.

## Notes and caveats

- Reload (or let Chrome update) the bridge extension after upgrading — it
  gained the `alarms` permission and a new background worker.
- Linux builds are glibc-linked; a static musl build is not included.
- The OS sandbox uses `sandbox-exec` on macOS and `bwrap` on Linux. Windows
  defaults to a restricted-token sandbox; opt into AppContainer Tier 2 with
  `sandbox.windowsTier: "elevated"` when network denial is required.
- macOS binaries are ad-hoc signed but not notarized. A manually downloaded
  archive may need `xattr -dr com.apple.quarantine ./zode` once.

---
