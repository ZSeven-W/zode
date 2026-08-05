# zode v0.1.0

**zode** is an open-source, AI-native coding assistant for your terminal: it reads your code, runs commands, searches files, and manages git — all from a fast Rust TUI, with non-blocking permissions and an on-by-default OS sandbox.

**This is the first stable release.** Nine betas of hardening later, the config
format, CLI surface, and JSON-RPC protocol are stable within 0.1.x.

The headline work since beta.9: **context that doesn't evaporate**. Long
agentic sessions used to grind — tool results cleared mid-turn, the model
re-running the same test dozens of times to re-read output it had just lost,
compaction waiting until 95%+ and then dropping the plot. 0.1.0 rebuilds that
whole pipeline around a three-layer memory.

## Three-layer memory

- **L1 — the transcript.** Micro-compaction no longer clears the current
  turn's tool results: ages count *human* turns, not tool batches, so a long
  agentic turn keeps everything it just read. The "model re-runs the same
  command to recover cleared output" loop is gone at the mechanism level.
- **L2 — the session ledger (new).** A harness-maintained, write-ahead record
  of the session: every user request verbatim, every shell command with its
  latest outcome, and the decisions / constraints / requirements / open
  questions accumulated along the way. After every compaction it is
  re-injected beside the restored files and memory recall — recorded as it
  happened, so a lossy summary can't erase it. Wiped by `/clear`, carried
  across model switches.
- **L3 — durable memory (noema).** Per-turn extraction and the
  compaction-analysis sink continue to feed the persistent store, with
  recall on later turns and sessions.

## Compaction that fires on time

- **Auto-compaction now triggers at 85% context occupancy** (the number the
  badge shows) instead of an internal formula that let small-completion
  models coast to 95%+. Tune it with `compact.autoCompactPercent` (50–97).
  A separate fixed guard still protects `prompt + max_tokens` provider
  validation.
- **Pinned contexts rescue themselves.** When compaction is masked (circuit
  breaker / no-progress latch) and the window is effectively full, zode now
  runs a free, deterministic emergency clear of old tool results — sparing
  anything the model hasn't seen yet — and continues the turn. Only when even
  that can't recover does the turn stop with clear instructions, instead of
  burning calls against a provider 400.
- **Command-repeat guard.** Re-running the same build/test just to see a
  different slice of its output now draws a nudge on the 5th run (doubling
  after): redirect the full output to a file once and read that. Edits reset
  the count — a re-run after a real change is never flagged.

## A transcript you can actually read

- **Tool runs up to 8 calls show their real rows** (pattern / file / command)
  instead of folding into a "ran N commands" summary; longer runs still fold.
- **Collapsed rows leak their first output line** — `▸ Tool Bash done ·
  Compiling zode v0.1.0 … (+12)` instead of a bare status.
- **Input summaries carry more arguments** (a Grep's path beside its pattern),
  with what-was-asked keys leading and file payloads/secrets never shown.
- `/model` switches that cross the standard↔lite boundary now re-apply the
  full weak-model accommodation bundle.

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

The installers auto-detect your OS + CPU, download the matching binary from this release, and drop `zode` on your PATH. Pin a version with `--version v0.1.0` (sh) or `$env:ZODE_VERSION='v0.1.0'` (ps1). Already on a beta? `zode update` gets you here.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.1.0-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.1.0-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.1.0-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.1.0-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.1.0-x64-windows.zip` |
| Windows | ARM64 | `zode-0.1.0-arm64-windows.zip` |

Unpack and move `zode` (or `zode.exe`) onto your PATH:

```sh
tar -xzf zode-0.1.0-x64-linux.tar.gz
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

## What's in 0.1.0

- Multi-provider chat (Anthropic / OpenAI-compatible / Ollama), large-output & 1M-context aware
- Full tool surface: file read/write/edit, code & content search, fg/bg shells, git, web fetch, notebooks, TODOs
- Browser and desktop automation, recurring `/loop` & `/schedule` tasks with a background watchdog
- Non-blocking permission gate + OS sandbox (`sandbox-exec` / `bwrap` on
  macOS/Linux; restricted-token / AppContainer tiers on Windows)
- Full-screen TUI: streaming markdown, syntax highlighting, diff previews, autocomplete, history, themes, 15-language UI, sandboxed UI plugins
- Multi-session tabs, sub-agents, teams & workflows, skills + MCP servers, hooks, three-level instructions
- Automatic weak-model accommodations (lite profile) with runtime learning
- Streaming JSON-RPC server mode with five SDKs (Rust / TypeScript / Python / Go / Kotlin)

## Supported platforms

Prebuilt binaries for **macOS** (arm64 + x64), **Linux** (x86_64 + arm64,
glibc), and **Windows** (x64 + arm64). Every architecture is built and packaged
independently in CI; the Intel macOS binary is cross-compiled on Apple's arm64
runner.

## Notes & caveats

- Linux builds are **glibc** (dynamically linked) — they run on mainstream distributions (Ubuntu/Debian/Fedora/Arch, …). A static musl build is not yet available.
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

**Full changelog:** https://github.com/ZSeven-W/zode/compare/v0.1.0-beta.9...v0.1.0
