# zode v0.1.0-beta.8

**zode** is an open-source, AI-native coding assistant for your terminal: it reads your code, runs commands, searches files, and manages git — all from a fast Rust TUI, with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, config, and behavior may change before 1.0. Please file issues!

This release is about **reliability with long sessions and smaller models**: zode now compacts before the context window can strangle a turn, brakes degenerate tool-call loops, and gives every model a map of your repository instead of letting it explore blind.

## Long-session reliability

- **Compaction fires before the window is full, not after it hurts.** The
  runtime's reactive auto-compaction now uses the same usage-calibrated
  occupancy as the per-request output clamp, so a near-full context compacts
  instead of squeezing `max_tokens` to the floor and truncating a tool call
  mid-JSON (the `tool_use input was cut off …` error at ~96% ctx). The TUI's
  between-turn guard also budgets for the completion, not just the prompt.
- **Compaction keeps your instructions.** Summaries now restate user-issued
  directives verbatim ("only look in X", "don't touch Y") under a *Standing
  user constraints* line — they stay binding after older turns are replaced.
- **Interjections take precedence.** A message typed mid-turn is delivered to
  the model with explicit override framing, so a steered correction changes
  the plan instead of being noted and ignored.
- **Input during compaction is queued, not lost.** Typing while zode compacts
  (or runs a `!cmd`) queues the message and auto-sends it the moment the
  operation finishes — previously it could sit unread until some later turn.
  Image-only messages (a pasted screenshot with no text) now send on Enter,
  and queue like text when the tab is busy.

## Tool-loop guard

Weak models can collapse into replaying the identical tool call verbatim —
observed in the wild as 137 consecutive identical greps until the user pressed
Esc. The query loop now hashes each iteration's calls *and* results: the 3rd
identical iteration injects a visible "change your approach" nudge into the
conversation, and the 6th ends the turn with an explicit `tool-call loop
detected` error instead of burning API calls until interrupted.

## Exploration scaffolding

- **Repository map** (`repoMap`, default on) — the system prompt carries
  tracked-file counts per directory, so cold starts target reads and searches
  instead of grepping the whole tree one guess at a time. It lives in the
  system prompt, so it survives compaction by construction.
- **Exploration discipline** — batch related searches, never re-issue an
  already-run command, switch strategy after two misses, honor user scope
  limits until lifted.
- **Read-set recap** — once 8 distinct files have been touched (then at each
  doubling), a reminder lists them so the model stops re-reading what is
  already in the conversation.

## A calmer transcript

- **Grouped tool activity** — adjacent tool calls collapse into dim summary
  lines ("ran 3 shell commands"), usage rows fold underneath, the streaming
  tail stays open, and a jump-to-bottom pill appears when you scroll far up.
  Click a summary to expand the calls behind it.
- **Adaptive status HUD** — a compact block above the status line with the
  per-turn tool tally, running subagents (with model labels), access mode,
  live shells, and connected MCP servers.
- **Question dialogs wrap** — long CJK questions and options wrap
  unicode-width-aware instead of clipping at a fixed 76 columns.
- **`Shift+Tab`** toggles YOLO (auto-approve) and ask mode.

## Toggles that stick

`/yolo` and `/sandbox` changes now persist to the **global** config on
success, so every workspace's next launch keeps your choice (project config
and state can still override per repo; stale per-project entries are cleaned
up automatically). Headless runs (`-p`, `--no-tui`) stay flag-explicit — a
script never inherits an interactive bypass.

## Auto-update, hardened

- Picks the **highest version** across the release list — pre-releases
  included — instead of trusting publish order, so a backfilled stable can
  never hide a newer beta.
- **Windows now self-updates** via a move-aside swap (the running exe is
  renamed, the new build takes its place, leftovers are cleaned on the next
  launch).
- The TUI shows a one-time **"self-updated — restart to apply"** notice
  instead of updating silently.

## Plugins & automation

- **Sandboxed JavaScript hooks** — a `hooks.json` entry ending in `.js` runs
  in-process in the QuickJS sandbox (no fs/network/process access, 8 MiB +
  100 ms bounds) instead of spawning an external process. Ships a runnable
  demo plugin (`examples/plugins/zode-hook-demo`).
- **Dynamic headers + `zode.crypto`** — `zode.data.define` header values can
  be JS functions computed at request time (method/url/body/timestamp/secrets
  in, headers out), with `sha256hex` / `hmacSha256Hex` bridges for signed API
  requests. Contributed by **@Tifancy** — thanks!
- **`/plugin` lists installed packages** — packages from
  `zode plugin install` now appear with their own enable/disable section,
  routed to the install registry.
- **Pick a page element from the Chrome side panel** — a DevTools-style
  picker (pure CDP, no new permissions, no content script) attaches the exact
  element — unique selector included — to your question, so a follow-up click
  or edit targets precisely what you pointed at. Password values are never
  read.
- **Nested subagent modes and tools** — Task-spawned subagents can carry
  their own modes and tool sets.

## Fixes

- Mouse clicks resolve against the frame you actually saw (hit-testing no
  longer queries the live terminal, which also made headless environments
  swallow clicks).
- SDK test flake (`ETXTBSY`) from the write-script-then-exec race fixed by
  serializing script setup with spawns.

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

The installers auto-detect your OS + CPU, download the matching binary from this release, and drop `zode` on your PATH. Pin a version with `--version v0.1.0-beta.8` (sh) or `$env:ZODE_VERSION='v0.1.0-beta.8'` (ps1).

> Because this is a **pre-release**, GitHub's "latest" excludes it from some tooling — the installers above resolve the newest release *including* betas, so they pick this up automatically.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.1.0-beta.8-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.1.0-beta.8-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.1.0-beta.8-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.1.0-beta.8-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.1.0-beta.8-x64-windows.zip` |
| Windows | ARM64 | `zode-0.1.0-beta.8-arm64-windows.zip` |

Unpack and move `zode` (or `zode.exe`) onto your PATH:

```sh
tar -xzf zode-0.1.0-beta.8-x64-linux.tar.gz
sudo mv zode /usr/local/bin/
```

### From source

```sh
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode && cargo build --release -p zode   # → target/release/zode
```

---

## Quick start

Launch `zode` and run **`/connect`** — an interactive, models.dev-backed picker
that writes the provider config for you. Or write `~/.zode/config.json` by hand:
`providers` holds the credentials + models (one entry per provider), and the
top-level `provider` records the active model.

```sh
mkdir -p ~/.zode
cat > ~/.zode/config.json <<'JSON'
{
  "providers": {
    "anthropic": {
      "type": "anthropic",
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }
}
JSON
zode
```

OpenAI-compatible providers (DeepSeek, Moonshot, OpenRouter, …) add `baseUrl` +
`dialect` and use `type: "openai"`; local models use `type: "ollama"`. One entry
can hold several models — switch live with `/model`. `/help` lists everything;
see the [README](https://github.com/ZSeven-W/zode#readme) for full usage.

---

## What's in the beta

- Multi-provider chat (Anthropic / OpenAI-compatible / Ollama), large-output & 1M-context aware
- Full tool surface: file read/write/edit, code & content search, fg/bg shells, git, web fetch, notebooks, TODOs
- Browser and desktop automation, recurring `/loop` & `/schedule` tasks with a background watchdog
- Non-blocking permission gate + OS sandbox (`sandbox-exec` / `bwrap` on
  macOS/Linux; restricted-token / AppContainer tiers on Windows)
- Full-screen TUI: streaming markdown, syntax highlighting, diff previews, autocomplete, history, themes, 15-language UI, sandboxed UI plugins
- Multi-session tabs, sub-agents, teams & workflows, skills + MCP servers, hooks, three-level instructions
- Streaming JSON-RPC server mode with five SDKs (Rust / TypeScript / Python / Go / Kotlin)

## Supported platforms

Prebuilt binaries for **macOS** (arm64 + x64), **Linux** (x86_64 + arm64,
glibc), and **Windows** (x64 + arm64). Every architecture is built and packaged
independently in CI; the Intel macOS binary is cross-compiled on Apple's arm64
runner.

## Notes & caveats

- Linux builds are **glibc** (dynamically linked) — they run on mainstream distributions (Ubuntu/Debian/Fedora/Arch, …). A static musl build is not part of this beta.
- The OS sandbox is enforced with `sandbox-exec` on macOS and `bwrap` on Linux.
  Windows uses the restricted-token Tier 1 by default; this limits filesystem
  access but not network access. Opt into AppContainer Tier 2 with
  `sandbox.windowsTier: "elevated"` when network denial (including loopback) is
  required. Continue reviewing tool calls on every OS.
- Desktop automation uses OS accessibility APIs and may prompt for permission
  (e.g. macOS Accessibility). The ghost-cursor overlay is macOS-only; other
  platforms still run desktop actions without the visualization.
- The **Windows self-update swap ships in this release** but was validated on
  Unix + unit tests only — if it misbehaves, replace the binary manually and
  file an issue.
- macOS binaries are ad-hoc signed but not notarized — **no Apple Developer certificate is required to run zode**. The `curl` / `irm` installers don't trip Gatekeeper (curl doesn't quarantine downloads). Only a **manual browser download** of the tarball is quarantined; if so, run `xattr -dr com.apple.quarantine ./zode` once.

---

**Full changelog:** https://github.com/ZSeven-W/zode/compare/v0.1.0-beta.7...v0.1.0-beta.8
