# zode v0.2.0-beta.2

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
