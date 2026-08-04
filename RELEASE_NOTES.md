# zode v0.1.0-beta.9

**zode** is an open-source, AI-native coding assistant for your terminal: it reads your code, runs commands, searches files, and manages git — all from a fast Rust TUI, with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, config, and behavior may change before 1.0. Please file issues!

This release makes zode **work well with weak/fast models automatically**, and
lands a large batch of UX and reliability fixes on top of beta.8.

## Weak-model support — automatic, zero config

Fast/distilled models (deepseek-flash, glm, mini/haiku-class) degrade on long
tasks in ways frontier models don't: they lose the plot, pick the wrong tool,
and spiral into repetition. zode now detects them and adapts — **you don't
configure anything**.

- **Automatic profile.** A model is treated as *lite* when its name says so
  (flash/mini/nano/haiku/air as whole segments), when config sets
  `profile: "lite"` (per provider or per model), or — the important part —
  when its **runtime behavior** trips a loop guard. The verdict is remembered
  in `~/.zode/model-profiles.json` and the tab reassembles on the spot, so
  the next session starts adapted. `profile: "standard"` always opts out.
- **What lite does:** caps the effective context at 96k and compacts at 70%
  (capability decays with length faster than the window fills), narrows the
  visible tool surface (the rest stays reachable via ToolSearch), tightens the
  output budget, uses a stricter loop guard, and periodically re-anchors the
  todo plan so the model doesn't drift.
- **Loop guards, tiered.** Beyond the identical-call guard, a same-tool streak
  guard catches the "guess another keyword" spiral (a `ToolSearch` hunt for a
  tool that doesn't exist), and `ToolSearch` now returns the **complete tool
  roster** with a stop-searching hint on a miss — the hunt ends instead of
  looping.

## Unified tool names

All built-in tools are now CamelCase (`browser_read` → `BrowserRead`,
`run_check` → `RunCheck`, `team_hire` → `TeamHire`, …), consistent with
`FileRead`/`Bash`. Permission lists written with the old snake_case names are
normalized automatically on config load, so existing `allow`/`deny` grants and
`~/.zode/state.json` keep working.

## Prompt-cache stability

For providers with prefix caching (DeepSeek, Anthropic), zode keeps the
request prefix byte-stable across turns: the repository map is TTL-cached, the
git branch is carried across engine reassembles, and a prefix-shape diagnostic
names which of (system prompt, tool set) changed when a `/model` /`/yolo`
/`/sandbox` toggle does force a cache reset.

## Fixes & smaller changes

- **Compaction can't wedge anymore.** A hung summarize call (provider 5xx
  storm) no longer pins a tab on "compacting" forever — a hard timeout plus a
  breaker on interrupt recover it.
- **`MemoryImport` tool.** Memory stays built-in and automatic (recall is
  injected per turn, facts are remembered on their own); this new tool only
  migrates memory notes from other tools. Asking to "migrate memories" no
  longer sends the model hunting for a memory tool that didn't exist.
- **`AskUserQuestion`** accepts `{label, description}` option objects and a
  `choices` alias — the top cause of "the question tool keeps failing".
- **Team hires fail loudly.** Hiring an unknown/unregistered agent errors once
  with the full roster instead of silently creating a generic teammate the
  model then loops on. Teammate/profile names are ASCII-only (documented).
- **Paste keeps its shape.** CR / CRLF line endings are normalized, so a
  multi-line paste no longer scrambles into one wrapped line.
- **Chrome Web Store ready.** The bridge extension's accepted IDs are now
  config-driven (`browser.extensionIds`), so a store-published copy (which
  gets a new ID) works without a code change; `pack-store.sh` builds the store
  upload.
- **More transcript detail.** Short tool runs (≤2 calls) show their real
  rows instead of a "ran 1 command" summary line, closer to Claude Code.
- **Prompt recall is project-scoped** — Up/Down history is shared across
  sessions in the same workspace again.
- New **`/new`** alias for `/clear`; new **`zode update` / `zode upgrade`**
  command; `/yolo` + `/sandbox` toggles persist to the global config;
  `/clear` and model/access switches no longer stall the UI.

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

The installers auto-detect your OS + CPU, download the matching binary from this release, and drop `zode` on your PATH. Pin a version with `--version v0.1.0-beta.9` (sh) or `$env:ZODE_VERSION='v0.1.0-beta.9'` (ps1).

> Because this is a **pre-release**, GitHub's "latest" excludes it from some tooling — the installers above resolve the newest release *including* betas, so they pick this up automatically.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.1.0-beta.9-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.1.0-beta.9-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.1.0-beta.9-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.1.0-beta.9-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.1.0-beta.9-x64-windows.zip` |
| Windows | ARM64 | `zode-0.1.0-beta.9-arm64-windows.zip` |

Unpack and move `zode` (or `zode.exe`) onto your PATH:

```sh
tar -xzf zode-0.1.0-beta.9-x64-linux.tar.gz
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

**Full changelog:** https://github.com/ZSeven-W/zode/compare/v0.1.0-beta.8...v0.1.0-beta.9
