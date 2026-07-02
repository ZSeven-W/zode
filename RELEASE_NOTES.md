# zode v0.1.0-beta.4

**zode** is an open-source, AI-native coding assistant for your terminal: it reads your code, runs commands, searches files, and manages git — all from a fast Rust TUI, with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, config, and behavior may change before 1.0. Please file issues!

## What's new in beta.4

- **JS-scripted workflows** — `/workflows` now runs real orchestration scripts instead of prompt checklists. A workflow is a `.js` body using `await agent(prompt, {type})`, `parallel([...])`, and `pipeline(items, ...stages)`; zode executes it deterministically in a sandboxed QuickJS runtime, and every `agent()` call dispatches a gated sub-agent (approvals, sandbox, and the sidebar's live sub-agent view all apply). Create one with the `define_workflow` tool, run it with `run_workflow` or from the dialog.
- **Richer sidebar** — new collapsible sections for **MCP** server connection state, **modified files** (git working-tree changes with per-file `+/-` line counts), and a pinned version footer. Todo and sub-agent sections are collapsible too — click a ▼ header, or use `/sidebar mcp|files|todo`. The "…+N more" row on modified files opens a full scrollable list.
- **Tabbed question dialog** — the `AskUserQuestion` modal now shows one question per tab with a progress strip, auto-advances to the next unanswered question after each pick, de-duplicates model-supplied "Other" options, and has a cleaner bordered layout.
- **Streamable HTTP MCP** — remote MCP servers configure with `"transport": "http"` (`"sse"` accepted as an alias); `$VAR` substitution works in `url`/`headers`. Docs added for configuring MCP servers, installing skills, and command Markdown.
- **Fixes** — startup no longer hangs on terminals that don't answer the keyboard-protocol probe (bounded 800 ms timeout); the sidebar's left rail no longer disappears after a modal closes (forced repaint on overlay close / **Ctrl+L**).

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

The installers auto-detect your OS + CPU, download the matching binary from this release, and drop `zode` on your PATH. Pin a version with `--version v0.1.0-beta.4` (sh) or `$env:ZODE_VERSION='v0.1.0-beta.4'` (ps1).

> Because this is a **pre-release**, GitHub's "latest" excludes it from some tooling — the installers above resolve the newest release *including* betas, so they pick this up automatically.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.1.0-beta.4-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.1.0-beta.4-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.1.0-beta.4-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.1.0-beta.4-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.1.0-beta.4-x64-windows.zip` |
| Windows | ARM64 | `zode-0.1.0-beta.4-arm64-windows.zip` |

Unpack and move `zode` (or `zode.exe`) onto your PATH:

```sh
tar -xzf zode-0.1.0-beta.4-x64-linux.tar.gz
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

**Full changelog:** https://github.com/ZSeven-W/zode/commits/v0.1.0-beta.4
