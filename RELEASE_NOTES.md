# zode v0.1.0-beta.5

**zode** is an open-source, AI-native coding assistant for your terminal: it reads your code, runs commands, searches files, and manages git — all from a fast Rust TUI, with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, config, and behavior may change before 1.0. Please file issues!

## Server mode & SDKs

`zode server` graduates from a metadata-only registry to a **streaming
JSON-RPC runtime**. New in this release:

- **Streaming turns** — `turn/start` returns immediately and streams model
  output and tool calls as notifications (`turn/started`,
  `item/agentMessage/delta`, `item/started` / `item/completed`,
  `turn/completed` with `finalText` + token `usage`, plus `turn/interrupted` /
  `turn/failed`). `turn/interrupt` cancels a running turn.
- **Interactive approvals** — the `prompt` policy drives server→client
  `approval/request` frames the client answers with
  `{ "decision": "allow" | "allowAlways" | "deny" }`.
- **WebSocket transport** — `zode server --listen ws://127.0.0.1:0` serves over
  a loopback WebSocket, publishing a `0600` `<config-dir>/server.json`
  credentials file (`{port, pid, token}`) and authenticating upgrades with
  `Authorization: Bearer <token>`. stdio (`zode server`) remains the default.
- **New methods** — `model/set` and `config/write` join the surface.
- **Five SDKs** (Rust, TypeScript, Python, Go, Kotlin/JVM) ship event
  subscription + approval handlers. See [`sdk/README.md`](sdk/README.md).

> 🚨 **BREAKING CHANGE — default approval policy is now `readOnly`.**
> `initialize` previously left side-effecting work effectively unrestricted;
> it now **denies** tool calls, `command/exec`, and filesystem writes unless
> the client passes `approvalPolicy: "auto"` (run without asking) or
> `"prompt"` (confirm each operation via `approval/request`). Clients that ran
> commands or wrote files without setting a policy **must** now set one. The
> accepted policy is echoed back in the `initialize` result.

## What's new in beta.5

- **OpenPencil single-command design flow** — `/op <design request>` is now the primary user-facing OpenPencil path. Raw MCP tool access is hidden from normal autocomplete, with `/op status` and the explicit `/op call <tool> <json>` escape hatch still available.
- **Browser bridge improvements** — Chrome bridge pairing, token persistence, reconnect handling, tab grouping, and popup status handling are more reliable. The bridge listener stays stable across tab swaps and expected reconnect misses are quieted.
- **Verification-first goal loops** — `run_check` records fresh verification evidence, and `goal_complete` now requires a passing check after the latest mutation before ending an autonomous loop.
- **Harness diagnostics** — tool calls can be traced to durable JSONL, Markdown exports can point to the full trace, headless/TUI tool result lines surface failed tool details and file locations, and compaction avoids persisting meta-instructions as durable memory.
- **Tool reliability** — foreground shell timeouts now explain that no `shell_id` was created, broad `git add -A` / `git add .` staging is blocked, shell commands detach from the controlling TTY, and Grep caps huge minified match lines around the actual hit.
- **TUI polish** — prompt history is scoped per session tab, shortcut labels consistently advertise `Ctrl+`, resized sidebar rows stay constrained, and recently added UI strings are translated.

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

The installers auto-detect your OS + CPU, download the matching binary from this release, and drop `zode` on your PATH. Pin a version with `--version v0.1.0-beta.5` (sh) or `$env:ZODE_VERSION='v0.1.0-beta.5'` (ps1).

> Because this is a **pre-release**, GitHub's "latest" excludes it from some tooling — the installers above resolve the newest release *including* betas, so they pick this up automatically.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.1.0-beta.5-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.1.0-beta.5-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.1.0-beta.5-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.1.0-beta.5-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.1.0-beta.5-x64-windows.zip` |
| Windows | ARM64 | `zode-0.1.0-beta.5-arm64-windows.zip` |

Unpack and move `zode` (or `zode.exe`) onto your PATH:

```sh
tar -xzf zode-0.1.0-beta.5-x64-linux.tar.gz
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

**Full changelog:** https://github.com/ZSeven-W/zode/compare/v0.1.0-beta.4...v0.1.0-beta.5
