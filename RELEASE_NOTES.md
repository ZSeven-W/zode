# zode v0.1.0-beta.7

**zode** is an open-source, AI-native coding assistant for your terminal: it reads your code, runs commands, searches files, and manages git — all from a fast Rust TUI, with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, config, and behavior may change before 1.0. Please file issues!

## Desktop automation

Zode can now drive native desktop apps, not just the browser.

- **`desktop_read` / `desktop_act` / `desktop_screenshot`** — read the
  accessibility tree, click/type/scroll/set-value by element, and capture the
  screen, across **macOS** (AX), **Windows** (UI Automation), **Linux**
  (AT-SPI), and **Electron** apps (CDP attach).
- **Ghost cursor + Esc stop** — a zero-permission overlay draws a fake cursor
  flying along a Dubins path to each action; zode never moves your real mouse.
  While automation is active a global **Esc** interrupts every running turn and
  hides the overlay.
- **`/desktop`** reports target/permission state; config lives under
  `desktop.*` (`ghostCursor`, `escCancel`, `overlayHelperPath`).

## Background-turn watchdog, `/loop` & `/schedule`

Unattended and recurring work is now first-class.

- **`/loop <30s|5m|1h> [--max N] <prompt>`** — session-only recurring turns on
  the current tab (`list` / `stop`).
- **`/schedule add <hh:mm|mon hh:mm|every 2h> <prompt>`** — persisted cron-like
  schedules (`~/.zode/schedules.json`, atomic writes, cross-process first-writer
  dedup, DST-aware). `list` / `rm` / `enable|disable`.
- **Background watchdog** — scheduler-owned turns get an idle/runtime watchdog
  with cooperative-then-hard abort, capped-backoff retry, and a fail-closed
  policy that stops (not blindly replays) a job when a mutation may have run.
  `/watchdog` and `/tasks` show live health and queue age. Configurable under
  `backgroundWatchdog.*`.
- **Turn timing** — per-tool `· 1.2s` suffixes, a `✓ done · 34s · 3 tools` turn
  footer, and humanized elapsed times in `/tasks`.

## JavaScript UI plugin extensions

Managed plugins can now contribute UI, evaluated in a sandboxed QuickJS runtime
with no filesystem, network, or terminal bridge.

- **`zode.ui.sidebar` / `zode.ui.statusLine`** — synchronous renderers that
  return declarative rows/spans; the host renders them (tones, bold/italic).
- **`zode.data.define`** — background HTTPS `GET`/`POST` data sources; secret
  env vars are assembled into request headers by Rust and never exposed to JS,
  with pinned public-DNS resolution, redirects/proxies disabled, and
  response/timeout/refresh caps.
- **Read-only render context** gated by `permissions.context` (tabs, workspace,
  tools, tasks, services); plugins observe tool identity/status but never tool
  inputs/outputs, prompts, or credentials.
- **Consent & safety** — install/update print the declared permission grant,
  `zode plugin update` refuses to widen permissions without a fresh `--trust`,
  and disabling/uninstalling a plugin tears down its renderers and data tasks.
  Codex `.codex-plugin` manifests and Claude `defaultEnabled` are supported.

## Agent team & external CLI teammates

- **Manual external-agent registration** — installed CLIs are never exposed to
  the model just for being on `PATH`; add profiles to `externalAgents.agents`
  (`/external-agents discover` adds known presets). First use shows a trust
  approval; a self-gated `ZodeTaskTool` routes `Task` to internal or external
  agents without changing today's gating.
- **Agent team (`/team`)** — hire persistent internal or external-CLI teammates,
  coordinate them with a shared host-managed board and subtree-aware file
  claims, and inspect the roster/board. Leader-mediated `@ask` relays keep
  collaboration turn-end and reviewable.

## Reliability & fail-closed hardening

- The background watchdog's "unresolved external work" fence was tightened so it
  no longer fires spuriously: a normally-exiting subprocess on Windows no longer
  latches on every success, read-only LSP queries no longer latch on timeout,
  and MCP tool calls gained a configurable timeout (`mcpToolTimeoutSecs`, `0`
  disables the local bound) instead of a hardcoded 60s.
- Closing a tab mid-turn now finalizes the run journal and checkpoint instead of
  leaving a dangling turn; graceful shutdown shows a draining notice and accepts
  a second **Ctrl+C** to force-quit instead of freezing.
- `/loop` intervals are capped so a huge value can't panic; the schedule store
  no longer erases rows it considers invalid as a side effect of an unrelated
  update; a poisoned watchdog lock fails closed.

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

The installers auto-detect your OS + CPU, download the matching binary from this release, and drop `zode` on your PATH. Pin a version with `--version v0.1.0-beta.7` (sh) or `$env:ZODE_VERSION='v0.1.0-beta.7'` (ps1).

> Because this is a **pre-release**, GitHub's "latest" excludes it from some tooling — the installers above resolve the newest release *including* betas, so they pick this up automatically.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.1.0-beta.7-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.1.0-beta.7-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.1.0-beta.7-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.1.0-beta.7-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.1.0-beta.7-x64-windows.zip` |
| Windows | ARM64 | `zode-0.1.0-beta.7-arm64-windows.zip` |

Unpack and move `zode` (or `zode.exe`) onto your PATH:

```sh
tar -xzf zode-0.1.0-beta.7-x64-linux.tar.gz
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
- macOS binaries are ad-hoc signed but not notarized — **no Apple Developer certificate is required to run zode**. The `curl` / `irm` installers don't trip Gatekeeper (curl doesn't quarantine downloads). Only a **manual browser download** of the tarball is quarantined; if so, run `xattr -dr com.apple.quarantine ./zode` once.

---

**Full changelog:** https://github.com/ZSeven-W/zode/compare/v0.1.0-beta.6...v0.1.0-beta.7
