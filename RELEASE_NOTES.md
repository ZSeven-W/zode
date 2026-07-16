# zode v0.1.0-beta.6

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
- **New methods** — `model/set` supports thread-level model changes and
  per-turn overrides. `config/write` applies a whitelisted shallow patch and
  can atomically persist it; `persist` defaults to `false`.
- **Five SDKs** (Rust, TypeScript, Python, Go, Kotlin/JVM) ship event
  subscription + approval handlers, shared protocol fixtures, and end-to-end
  tests against a real `zode` binary. See [`sdk/README.md`](sdk/README.md).
- **GitHub Packages** — the TypeScript SDK is published as
  `@zseven-w/zode-sdk@0.1.0-beta.6` under the npm `beta` dist-tag, and the
  Kotlin SDK as `com.zseven.zode:zode-sdk:0.1.0-beta.6`. Both package jobs
  build and test before publishing.
- **All five SDKs in the release pipeline** — the Go module is versioned by
  `sdk/go/v0.1.0-beta.6`; Python ships a wheel and source distribution; Rust
  ships a standalone source bundle containing the SDK and protocol dependency.
  The Python and Rust archives are attached to this GitHub Release because GitHub
  Packages does not provide PyPI or Cargo registries.

> 🚨 **BREAKING CHANGE — default approval policy is now `readOnly`.**
> `initialize` previously left side-effecting work effectively unrestricted;
> it now **denies** tool calls, `command/exec`, and filesystem writes unless
> the client passes `approvalPolicy: "auto"` (run without asking) or
> `"prompt"` (confirm each operation via `approval/request`). Clients that ran
> commands or wrote files without setting a policy **must** now set one. The
> accepted policy is echoed back in the `initialize` result.

> 📦 **TypeScript package rename.** The npm scope is now
> `@zseven-w/zode-sdk` (previously `@zseven/zode-sdk`) so it can be published
> under this repository owner's GitHub Packages scope. Update imports and
> package-manager configuration when upgrading. GitHub npm and Maven installs
> require credentials with `read:packages`.

## Browser bridge and task side panel

- **A full task side panel** — extension 0.5.0 rebuilds the Chrome side panel
  with React and TypeScript. Start or select a session shared with the TUI,
  choose a model and approval policy, watch streaming output, stop a turn, and
  answer approvals without switching the terminal's active tab.
- **Extension-only daemon startup** — one normal zode launch followed by
  `/browser pair` registers a Native Messaging host and stores the bridge
  token. The extension can then start an authenticated, extension-only zode
  daemon on demand when the CLI is closed, using the most recently registered
  workspace and shutting down when Chrome disconnects.
- **The current page stays current** — side-panel turns treat the adjacent
  active page as their primary browser context, so requests such as “analyze
  this page” do not open a replacement tab. Standalone TUI/CLI automation keeps
  using an isolated, sticky tab in zode's Chrome tab group; screenshot focus is
  restored afterward, and manually navigating that tab causes zode to hand it
  back and create another automation tab.
- **Upload and download support** — browser tools can populate file inputs and
  upload files, while downloads created during the current bridge session are
  exposed through a bounded session cache. Side-panel turns accept up to eight
  attachments / 20 MiB total (5 MiB per image, 1 MiB per UTF-8 text or code
  file).
- **Packaging and polish** — toolbar icons follow Chrome's light/dark theme,
  Windows pairing launches Chrome directly, and every tagged release now
  builds, tests, and attaches `zode-browser-bridge-<version>.zip`.

After upgrading, run `/browser pair` once with the new CLI and reload the
extension from `chrome://extensions`. Older bridge builds can still drive the
traditional browser automation path, but do not provide the side panel or
Native Messaging auto-start.

## Windows sandbox and input reliability

- **Tier 1 sandbox by default** — Windows commands and file
  operations now run through a low-integrity restricted token with capability
  ACLs scoped to the configured workspace roots. This replaces the previous
  no-op Windows backend and does not require administrator privileges.
- **Opt-in Tier 2 network isolation** — set
  `sandbox.windowsTier` to `"elevated"` (`"appcontainer"` and `"strict"` are
  aliases) to launch commands inside an AppContainer without network
  capabilities, including loopback. Tier 1 constrains filesystem access but
  does **not** enforce network denial.
- **Verified enforcement** — Windows CI builds the full workspace and runs
  dedicated Tier 1 / Tier 2 integration probes covering in-root and out-of-root
  access, read-only behavior, and AppContainer network denial.
- **Reliable Windows paste and clipboard handling** — clipboard text is
  preserved and emitted as UTF-8; coalesced multiline paste bursts remain in
  the composer instead of becoming several submissions; duplicate surrogate
  key events are collapsed; and asynchronous clipboard helpers now have time
  and byte limits.

Tier 1 is the default Windows sandbox; Tier 2 is the optional AppContainer
backend for network isolation. Tier 1 does not isolate the network, and
compatibility allowances for build tools and atomic renames mean the sandbox
should remain one layer in the approval model rather than a boundary for
hostile code.

## Agent runtime and model routing

- **Provider-aware vision routing** — image capability is derived from
  models.dev metadata and legacy attachment declarations using the exact
  provider/model pair. Explicit `supportsImages` settings still win, and
  `/connect` persists the selected model's image capability.
- **Native reasoning controls** — `/effort` maps to Anthropic adaptive thinking
  and effort on supported models, preserves the older budget shape where
  required, and sends `reasoning_effort` plus `max_completion_tokens` only to
  OpenAI-compatible providers that explicitly enable reasoning.
- **Sub-agents can finish naturally** — task sub-agents no longer stop at a
  small fixed iteration count. Leave `subagentMaxIterations` absent or `0` for
  no fixed limit, or set a positive value when a headless or cost-sensitive
  workflow needs a hard budget.
- **Fresh-state reminders** — one-shot reminders warn the model when a file it
  read has changed or disappeared, the git branch has drifted, or a todo list
  has gone five turns without an update, reducing edits based on stale state.
- **More resilient sessions** — session indexes use cross-process locking,
  atomic snapshots, backups, corrupt-index archival, and orphan JSONL recovery.
  Agent histories also tolerate a torn final JSONL line without discarding the
  valid history before it.
- **Higher-signal tool output** — the agent runtime adds ripgrep-backed context
  and multiline search, clips huge minified lines around the real match, and
  structurally compresses noisy command output such as `git status`, `ls`, and
  `cargo test` before applying output limits.

## LSP and project memory

- **Reliable lazy LSP startup** — language servers still start on the first
  `lsp_*` call, but missing rustup components are no longer mistaken for real
  binaries. zode resolves or installs the required component while still
  preferring a usable executable already on `PATH`.
- **Actionable failures instead of long retries** — server exit preserves the
  tail of stderr, fails pending requests immediately, and marks the process
  dead. EPIPE handling briefly waits for the real exit diagnostic instead of
  repeatedly returning only a timeout or “broken pipe.”
- **Honest cold-start diagnostics** — `lsp_diagnostics` reports `analyzing`
  until the first diagnostic publication arrives instead of reporting a false
  zero-diagnostics result. The system prompt also advertises symbols,
  references, hover, and diagnostics tools when an LSP is enabled.
- **Code-aware Noema anchors** — memories can now carry paths, symbols, commit,
  language, error signatures, and commands, with a dedicated `error_fix` kind.
  Paths are normalized against the turn's working directory and project
  identity remains stable across subdirectories and linked worktrees.
- **More relevant recall** — malformed optional anchors degrade safely, secret
  material remains gated, error-fix deduplication understands anchors, and
  recall scores overlapping file/symbol/error context more highly. Existing
  memories require no migration.

## TUI and interaction polish

- **Eleven built-in themes** — the catalog grows from four to eleven with
  Aurora Forge, Ember Atelier, Sakura Paper, Arctic Day, Lavender Mist, Citrus
  Grove, and Verdant Signal. Catppuccin Mocha remains the default, and a custom
  theme with the same id still overrides a built-in.
- **More robust sidebar chrome** — the sidebar uses a closed top edge and
  continuous left rail, with corrected content and mouse coordinates plus more
  stable section/footer behavior as terminal dimensions change.
- **Mouse support where it matters** — click session tabs, AskUserQuestion
  choices and submit controls, or permission choices 1/2/3 while the
  non-blocking prompt remains open over the chat.
- **Complete expanded tool output** — an expanded tool block now keeps the full
  retained output instead of the former short teaser. Transcript rendering
  expands tabs, strips control characters, and truncates CJK safely.
- **Safer session deletion and smoother redraws** — deleting a session requires
  pressing Delete twice on the same item; navigation, filtering, or Escape
  cancels the confirmation. Synchronous terminal updates remove full-screen
  flashes when toasts expire or the view scrolls.

## CI and release hardening

- CI now covers Rust fmt, Clippy with warnings denied, workspace tests,
  `cargo-deny`, a strict five-language SDK matrix against a real zode binary,
  full Windows workspace compilation, and dedicated Windows sandbox probes.
- Release tags fan out to six platform archives, the tested browser extension,
  install scripts, all five SDK channels, and idempotent TypeScript/Kotlin
  GitHub Packages jobs. The pipeline also creates or verifies the Go module tag
  and attaches tested Rust/Python archives to the GitHub Release.
- Fixture generation and staleness guards keep the shared JSON-RPC method list,
  sample frames, and SDK expectations aligned with the Rust protocol source.

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

The installers auto-detect your OS + CPU, download the matching binary from this release, and drop `zode` on your PATH. Pin a version with `--version v0.1.0-beta.6` (sh) or `$env:ZODE_VERSION='v0.1.0-beta.6'` (ps1).

> Because this is a **pre-release**, GitHub's "latest" excludes it from some tooling — the installers above resolve the newest release *including* betas, so they pick this up automatically.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.1.0-beta.6-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.1.0-beta.6-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.1.0-beta.6-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.1.0-beta.6-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.1.0-beta.6-x64-windows.zip` |
| Windows | ARM64 | `zode-0.1.0-beta.6-arm64-windows.zip` |

Unpack and move `zode` (or `zode.exe`) onto your PATH:

```sh
tar -xzf zode-0.1.0-beta.6-x64-linux.tar.gz
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
- Non-blocking permission gate + OS sandbox (`sandbox-exec` / `bwrap` on
  macOS/Linux; restricted-token / AppContainer tiers on Windows)
- Full-screen TUI: streaming markdown, syntax highlighting, diff previews, autocomplete, history, themes, 15-language UI
- Multi-session tabs, sub-agents & workflows, skills + MCP servers, hooks, three-level instructions

## Supported platforms

Prebuilt binaries for **macOS** (arm64 + x64), **Linux** (x86_64 + arm64,
glibc), and **Windows** (x64 + arm64). Every architecture is built and packaged
independently in CI; the Intel macOS binary is cross-compiled on Apple's arm64
runner.

## Notes & caveats

- Linux builds are **glibc** (dynamically linked) — they run on mainstream distributions (Ubuntu/Debian/Fedora/Arch, …). A static musl build is not part of this beta.
- The OS sandbox is enforced with `sandbox-exec` on macOS and `bwrap` on Linux.
  Windows uses the new restricted-token Tier 1 by default; this
  limits filesystem access but not network access. Opt into AppContainer Tier 2
  with `sandbox.windowsTier: "elevated"` when network denial
  (including loopback) is required. Continue reviewing tool calls on every OS.
- macOS binaries are ad-hoc signed but not notarized — **no Apple Developer certificate is required to run zode**. The `curl` / `irm` installers don't trip Gatekeeper (curl doesn't quarantine downloads). Only a **manual browser download** of the tarball is quarantined; if so, run `xattr -dr com.apple.quarantine ./zode` once.

---

**Full changelog:** https://github.com/ZSeven-W/zode/compare/v0.1.0-beta.5...v0.1.0-beta.6
