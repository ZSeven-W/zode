# zode v0.1.0-beta.3

**zode** is an open-source, AI-native coding assistant for your terminal: it reads your code, runs commands, searches files, and manages git — all from a fast Rust TUI, with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, config, and behavior may change before 1.0. Please file issues!

## What's new in beta.3

- **Autonomous goal loop** — `/goal <task>` now works toward the goal across turns until it's done (the agent calls a `goal_complete` tool to stop); **Esc** or `/goal clear` halts it. The sidebar shows the active goal + elapsed time.
- **Switchable pricing currency** — `/currency` (or **Settings → Currency**) converts the cost display live between USD / CNY / EUR / GBP / JPY / KRW / INR / TWD / HKD.
- **Full UI localization** — the status bar, sidebar, badges, permission prompts, and menus are now translated across all **15 languages** (`/language`).
- **Third-party Anthropic gateways** — custom Anthropic-compatible endpoints (LongCat, DeepSeek) now authenticate correctly (`Authorization: Bearer`).
- **Readable transcript** — markdown renders with proper section / paragraph / bullet spacing, and tool output (stdout, file content) is shown inline.
- **Context management** — the `% ctx` badge refreshes right after `/compact`, the loop auto-compacts near the window limit, and a spinner shows while compacting.
- **Resilience** — transient provider stream failures retry with exponential backoff.
- **Fixes** — copy-on-select & clipboard errors, and the "thinking process piling into one block" display issue.

---

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

The installers auto-detect your OS + CPU, download the matching binary from this release, and drop `zode` on your PATH. Pin a version with `--version v0.1.0-beta.3` (sh) or `$env:ZODE_VERSION='v0.1.0-beta.3'` (ps1).

> Because this is a **pre-release**, GitHub's "latest" excludes it from some tooling — the installers above resolve the newest release *including* betas, so they pick this up automatically.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.1.0-beta.3-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.1.0-beta.3-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.1.0-beta.3-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.1.0-beta.3-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.1.0-beta.3-x64-windows.zip` |
| Windows | ARM64 | `zode-0.1.0-beta.3-arm64-windows.zip` |

Unpack and move `zode` (or `zode.exe`) onto your PATH:

```sh
tar -xzf zode-0.1.0-beta.3-x64-linux.tar.gz
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
- Non-blocking permission gate + OS sandbox (sandbox-exec / bwrap), outbound network denied by default
- Full-screen TUI: streaming markdown, syntax highlighting, diff previews, autocomplete, history, themes, 15-language UI
- Multi-session tabs, sub-agents & workflows, skills + MCP servers, hooks, three-level instructions

## Supported platforms

Prebuilt binaries for **macOS** (arm64 + x64), **Linux** (x86_64 + arm64, glibc), and **Windows** (x64 + arm64). Every binary is built natively on its own architecture in CI.

## Notes & caveats

- Linux builds are **glibc** (dynamically linked) — they run on mainstream distributions (Ubuntu/Debian/Fedora/Arch, …). A static musl build is not part of this beta.
- The OS sandbox is enforced on macOS (`sandbox-exec`) and Linux (`bwrap`); on Windows it is a no-op (commands run ungated) — review tool calls accordingly.
- macOS binaries are ad-hoc signed but not notarized — **no Apple Developer certificate is required to run zode**. The `curl` / `irm` installers don't trip Gatekeeper (curl doesn't quarantine downloads). Only a **manual browser download** of the tarball is quarantined; if so, run `xattr -dr com.apple.quarantine ./zode` once.

---

**Full changelog:** https://github.com/ZSeven-W/zode/commits/v0.1.0-beta.3
