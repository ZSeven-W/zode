<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo.png">
    <img src="assets/logo-light.png" alt="Zode logo" width="96" />
  </picture>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2021-f74c00?style=flat-square&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/TUI-ratatui-7c3aed?style=flat-square" alt="ratatui" />
  <img src="https://img.shields.io/badge/license-MIT-22c55e?style=flat-square" alt="MIT License" />
</p>

<h1 align="center">Zode</h1>

<p align="center">
  <strong>Open-source, AI-native coding assistant for your terminal.</strong><br/>
  Reads your code. Runs commands. Searches files. Manages git. All from a fast Rust TUI.
</p>

<p align="center">
  <a href="README.md">English</a> |
  <a href="docs/readme/README.zh.md">简体中文</a> |
  <a href="docs/readme/README.zh-tw.md">繁體中文</a> |
  <a href="docs/readme/README.ja.md">日本語</a> |
  <a href="docs/readme/README.ko.md">한국어</a> |
  <a href="docs/readme/README.es.md">Español</a> |
  <a href="docs/readme/README.fr.md">Français</a> |
  <a href="docs/readme/README.de.md">Deutsch</a> |
  <a href="docs/readme/README.pt.md">Português</a> |
  <a href="docs/readme/README.ru.md">Русский</a> |
  <a href="docs/readme/README.hi.md">हिन्दी</a> |
  <a href="docs/readme/README.id.md">Bahasa Indonesia</a> |
  <a href="docs/readme/README.th.md">ไทย</a> |
  <a href="docs/readme/README.tr.md">Türkçe</a> |
  <a href="docs/readme/README.vi.md">Tiếng Việt</a>
</p>

---

## Highlights

- **Multi-provider** — Anthropic, OpenAI, and any OpenAI-compatible API (DeepSeek, Moonshot, OpenRouter dialects), plus local Ollama. Supports large-output and **1M-context** models (`contextWindow` / `maxOutputTokens` are configurable)
- **Rich tool surface** — file read/write/edit, code & content search, foreground and background shells, git, web fetch, notebooks, TODO tracking
- **Browser control** — built-in `browser_*` tools can drive a managed Chromium instance or your real Chrome profile through the zode Chrome bridge extension: navigate, click/type, inspect DOM, capture screenshots, read console/network logs, and group zode-opened tabs
- **Non-blocking permissions** — every mutating tool is gated (allow once / always / deny), but the prompt docks inline and never blocks you: keep typing to queue a follow-up while a tool waits, with hard-deny rules
- **OS sandbox, on by default** — shell commands run under sandbox-exec (macOS) / bwrap (Linux) in `read-only` or `workspace-write` mode, with **outbound network denied by default**. Toggle live with `/sandbox`; the model can request an escape for a single command (`dangerouslyDisableSandbox`) which **you authorize** at the prompt
- **Full-screen TUI** — streaming markdown with syntax highlighting, diff previews, slash-command autocomplete, prompt history (Up/Down), 11 built-in themes, settings & help overlays, resilient right sidebar sections, **15-language UI** (`/language`)
- **Durable, V1-compatible sessions** — keep the existing `<id>.jsonl` transcript contract while adding journals, checkpoints, rewind, fork, and isolated Git worktrees as sidecar data
- **Automation surfaces** — stable JSON/JSONL headless output, exact session targeting, tool filters, deterministic exit codes, ACP over stdio, and a local operations dashboard
- **Multi-session tabs** — run several conversations side by side (`Ctrl+T`), each an isolated agent; resume past sessions with full history replay
- **Sub-agents, teams & workflows** — delegate one-shot work through the Task tool, hire persistent internal or external-CLI teammates, coordinate them with a shared board and file claims, and manage the surfaces with `/agents`, `/team`, and `/workflows`
- **Portable local configuration** — reads direct skills and MCP configuration from Claude Code, Codex, Cursor, opencode, and Gemini, while never importing their installed plugin trees or caches
- **Skills & MCP** — load `SKILL.md` instruction packs on demand and connect MCP servers (`mcp__<server>__<tool>`); created agents, skills, and MCP tools surface as slash commands
- **Hooks** — run a shell script or a sandboxed JavaScript hook on agent events (e.g. block dangerous commands, lint after edits)
- **Three-level instructions** — global (`~/.zode/`) → project root → cwd (`AGENTS.md` / `CLAUDE.md`)

## Install

### One line (prebuilt binaries)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

The installer auto-detects your OS + CPU, downloads the matching binary from
the latest [release](https://github.com/ZSeven-W/zode/releases), and puts `zode`
on your PATH. Pin a version or change the location:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

### Manual download

Grab the archive for your platform from the [releases page](https://github.com/ZSeven-W/zode/releases):

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Then unpack and move `zode` onto your PATH (`sudo mv zode /usr/local/bin/`).
Linux builds are glibc; macOS binaries are unsigned (`xattr -dr
com.apple.quarantine ./zode` if Gatekeeper complains).

### From source

Requires Rust 1.88 or newer:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binary at target/release/zode
```

> The agent runtime lives in the `vendor/agent` git submodule — always clone
> with `--recurse-submodules` (or run `git submodule update --init`).

## Quick Start

The easiest way is to launch `zode` and run **`/connect`** — an interactive,
models.dev-backed picker that writes the config for you.

To write `~/.zode/config.json` by hand: **`providers`** is the source of truth
— one entry per provider (shared credentials) holding one or more **models** —
and the top-level **`provider`** records the *active* model:

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // wire protocol: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // the active model
}
```

OpenAI-compatible providers (DeepSeek, Moonshot, OpenRouter, …) add a `baseUrl`
+ `dialect`, and per-model settings live in each model's entry:

```jsonc
{
  "providers": {
    "deepseek": {
      "type": "openai",
      "apiKey": "sk-...",
      "baseUrl": "https://api.deepseek.com/v1",
      "dialect": "deepseek",             // "standard" | "deepseek" | "moonshot" | "openrouter"
      "models": {
        "deepseek-v4-pro":  { "contextWindow": 1000000, "maxOutputTokens": 16384 },
        "deepseek-chat":    {}
      }
    }
  },
  "provider": { "model": "deepseek-v4-pro" }
}
```

One provider entry can hold several models — switch between them live with `/model`.

Then run:

```bash
zode                       # full-screen TUI
zode -p "explain main.rs"  # headless: one prompt, stream to stdout, exit
zode --no-tui              # plain readline REPL
zode -c                    # continue the most recent session
zode -r <id>               # resume a session by id prefix
zode --yolo                # bypass approval prompts (deny rules still apply)
zode --no-sandbox          # disable the OS sandbox (it is ON by default)
zode --sandbox-read-only   # sandbox in read-only mode (deny all writes)
zode --sandbox-allow-network  # allow outbound network inside the sandbox
zode --browser             # force-enable built-in browser tools for this run
zode --no-browser          # disable built-in browser tools for this run
zode --model <id>          # override the model
zode --provider <name>     # pick a named provider from config.providers
zode server                # JSON-RPC app-server mode over stdio
zode acp                   # Agent Client Protocol agent over stdio
zode dashboard             # local sessions/checkpoints/worktrees overview
```

You can also point at any provider without editing the config by exporting the
matching key (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); for Ollama the
`baseUrl` is taken from the environment when unset.

## External CLI Teammates

Zode can use an installed third-party agent CLI as a one-shot Task worker or
as a persistent or stateless teammate. Registration is deliberately manual:
installing a CLI or putting it on `PATH` does **not** expose it to the model.
Add a profile under `externalAgents.agents`, then start Zode in the project.
Or run `/external-agents` to inspect supported CLIs currently on `PATH`, then
`/external-agents discover` to explicitly add every detected preset to the
global config. This command is user-triggered; startup never scans or registers
external CLIs automatically.

| Agent profile | Executable | Task worker | Team mode | External CLI sandbox |
|---|---|---:|---:|---|
| `claude-code` | `claude` | yes | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | yes | persistent | workspace-write |
| `opencode` | `opencode` | yes | stateless | unknown |
| `cline` | `cline` | yes | stateless | unrestricted |
| `antigravity` | `agy` | yes | stateless | unknown |
| `cursor` | `cursor-agent` | yes | persistent | unrestricted |
| `kiro` | `kiro-cli` | yes | stateless | unrestricted |
| `pi` | `pi` | yes | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | yes | persistent | unrestricted |

Every registered profile can join a team. Resumable profiles preserve the
CLI's session ID and conversation across assignments; other CLIs are stateless
teammates that start a fresh process for each assignment. The presets use
the documented headless interfaces of [Cline](https://docs.cline.bot/usage/cli-overview),
[Antigravity](https://antigravity.google/docs/cli-best-practices),
[Cursor](https://cursor.com/docs/cli/headless),
[Kiro](https://kiro.dev/docs/cli/headless/), [Pi](https://pi.dev/docs/latest), and xAI's
[Grok Build](https://docs.x.ai/build/cli/headless-scripting). Other tools,
including alternative Grok CLIs, can use a custom profile.

### Add a CLI profile manually

Put `externalAgents` in `~/.zode/config.json` for all projects, or in
`<project>/.zode/config.json` for one project. An empty object explicitly
enables a known preset and resolves its executable on the sanitized `PATH`:

```jsonc
{
  "externalAgents": {
    "enabled": true,
    "timeoutSecs": 1800,
    "maxConcurrent": 2,
    "agents": {
      "claude-code": {},
      "codex": {
        "command": "codex",
        "extraArgs": ["--model", "your-model-id"],
        "envAllow": ["OPENAI_API_KEY"],
        "trusted": false
      },
      "opencode": {},
      "cline": {},
      "antigravity": {},
      "cursor": {},
      "kiro": {},
      "pi": {},
      "grok": {}
    }
  }
}
```

Add only the profiles you intend to expose. A bare `command` such as `cline`
is resolved on `PATH`; paths such as `./tools/my-agent` or
`/opt/agents/my-agent` are also accepted. Known presets honor `enabled`,
`command`, `extraArgs`, `envAllow`, and `trusted`; `extraArgs` is appended to
Zode's preset invocation.

CLI processes start with a cleared environment containing only `PATH`, `HOME`,
and `TERM` (plus required Windows variables), so explicitly add API keys or
other required variables to `envAllow`. Existing login state under `HOME`
continues to work. A project entry with the same profile name replaces the
whole global entry, so repeat every override that the project still needs.

A custom profile declares the complete invocation and protocol:

```jsonc
{
  "externalAgents": {
    "agents": {
      "my-agent": {
        "command": "my-agent",
        "args": ["run", "--json", "{prompt}"],
        "promptTransport": "argv",
        "output": "jsonl",
        "textSource": "/event/delta",
        "sessionIdSource": "/session/id",
        "resumeArgs": ["--session", "{session_id}"],
        "effectiveSandbox": "workspaceWrite",
        "authEnv": ["MY_AGENT_API_KEY"],
        "trusted": false
      }
    }
  }
}
```

`promptTransport` is `stdin`, `argv`, or `file`; `argv` requires a standalone
`{prompt}` argument and `file` requires `{prompt_file}`. `output` is `text`,
generic `jsonl`, `jsonl-claude`, or `jsonl-codex`. Generic JSONL profiles use
RFC 6901 `textSource` and `sessionIdSource` pointers to extract streamed text
and a resumable session ID from any event. `resumeArgs` must contain a
standalone `{session_id}` token and is appended on later turns; `resumeFlag`
is retained as the shorthand `<flag> <session-id>` form.

If a CLI accepts a caller-selected session ID, `newSessionArgs` can contain a
standalone `{session_id}` token. Zode generates a UUID, appends the expanded
arguments on the first run, and uses `resumeArgs` on later assignments. This
also makes a plain-text CLI resumable without parsing an ID from its output.

This lets any headless CLI become a Task worker or stateless teammate. To
preserve conversation context between team assignments, it must additionally
expose a session ID, or accept one through `newSessionArgs`, plus a
non-interactive resume invocation.

`effectiveSandbox` accepts `none`, `readOnly`, `workspaceWrite`,
`unrestricted`, or `unknown` and is displayed in the trust prompt.

### Hire and work with the teammate

Ask the leader in normal language; `team_hire` and `team_send` are model-facing
tools, not slash commands:

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

The first hire shows the resolved executable and arguments, working directory,
and the CLI's effective sandbox. Approving it delegates work to that process in
the current project: Zode gates the process launch, but does **not** gate each
file edit or shell command performed by the external CLI. Trust grants last
for the current Zode session; the persistent roster is recovered from
`<cwd>/.zode/team/`, but an external teammate must be trusted again after a
restart or executable change.

In non-interactive/bypass runs (including `--yolo`), Zode cannot show the trust
prompt and fails closed. Set `externalAgents.agents.<profile>.trusted` to
`true` only when you deliberately want that profile to run without the prompt.

Use `/team` to inspect the roster and board after hiring:

```text
/team                         # roster + board panel
/team status                  # text roster
/team board                   # shared goal, notes, assignments, and claims
/team dismiss implementer     # remove the teammate
```

## Automation, Durable Sessions, and Operations

### Structured headless runs

`-p`, `--prompt-file`, and `--prompt-json` all use the same headless engine.
`json` emits one final result object; `stream-json` emits one
`zode.run-event.v1` JSON object per line. Structured modes reserve stdout for
machine-readable output and use stable exit codes: `0` success, `10` provider
error, `11` permission denied, `12` turn/limit reached, `13` interrupted
(Ctrl-C), `14` partial result, `15` session targeting error.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# Exact IDs do not prefix-match. A fork never mutates its source session.
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Tool deny patterns win over allow patterns and are inherited by Task
sub-agents. `--permission-mode` accepts `default`, `dont-ask`, `accept-edits`,
and `bypass`; `--yolo` remains a shortcut for bypass, while hard deny rules
still apply.

### V1-compatible sessions, checkpoints, and worktrees

The transcript remains the original V1 file at
`~/.zode/sessions/<id>.jsonl`. It is the **only** transcript copy, so older Zode
clients can keep reading and writing it. New metadata is additive and lives in
`~/.zode/sessions/<id>/` (`meta.json`, journal, checkpoints, and snapshots).
No new session format or transcript migration is required.

```bash
zode session list
zode session list --json
zode session show <id>                         # metadata + checkpoint IDs
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # conflict-aware preview
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

A checkpoint is captured before a mutating turn. Rewind restores tracked file
content and the transcript prefix, reports conflicts instead of overwriting
newer changes, and records a new logical journal branch rather than deleting
history. Worktree forks can be applied back explicitly when the experiment is
ready.

### Permission rules and sandbox profiles

Rules can live under `permissions.rules` in `config.json`, or in a standalone
JSON file passed with `--rules`. A field matcher uses an RFC 6901 JSON pointer;
deny takes precedence over ask, which takes precedence over allow.
The standalone file must be either a rule array or `{ "rules": [...] }`; it is
not wrapped in a top-level `permissions` object.

```jsonc
{
  "permissions": {
    "deny": ["Remove"],
    "rules": [
      {
        "behavior": "allow",
        "tool": "Bash",
        "matcher": {
          "kind": "field",
          "pointer": "/command",
          "pattern": { "kind": "glob", "value": "git status*" }
        }
      },
      {
        "behavior": "deny",
        "tool": "Bash",
        "matcher": {
          "kind": "field",
          "pointer": "/command",
          "pattern": { "kind": "glob", "value": "*--force*" }
        }
      }
    ]
  },
  "sandbox": {
    "profiles": {
      "ci": {
        "enabled": true,
        "mode": "workspace-write",
        "network": false,
        "writableRoots": ["/tmp/build-cache"]
      }
    }
  }
}
```

```bash
zode -p "inspect only" --sandbox-profile read-only
zode -p "run checks" --sandbox-profile workspace
zode -p "download dependencies" --sandbox-profile workspace-network
zode -p "run CI" --sandbox-profile ci --rules ./permissions.json
```

Built-in profiles are `read-only`, `workspace`, `workspace-network`, and
`unconfined`. Config-defined profiles use the same sandbox fields shown above.

### Plugins and static marketplaces

A managed plugin can contribute skills, commands, agents, hooks, MCP servers,
LSP servers, and sandboxed JavaScript UI renderers. Zode accepts `plugin.json`, `.zode-plugin/plugin.json`,
`.codex-plugin/plugin.json`, `.grok-plugin/plugin.json`, and
`.claude-plugin/plugin.json`. Codex and Claude Code component path arrays are
supported, and Claude Code's `defaultEnabled` is honored on first install.
Host-only components such as Codex apps/connectors and Claude Code themes,
monitors, or output styles are ignored; an app-only plugin is rejected because
it has no Zode-compatible component. Installs are immutable snapshots with
provenance and a SHA-256 tree hash. Executable plugin content is never activated
without the explicit `--trust` flag.

#### JavaScript UI plugin quick start

The smallest UI plugin contains a manifest and one JavaScript file:

```text
my-plugin/
├── plugin.json
└── scripts/
    └── ui.js
```

`plugin.json`:

```json
{
  "name": "my-plugin",
  "version": "0.1.0",
  "ui": {
    "sidebar": "./scripts/ui.js",
    "statusLine": "./scripts/ui.js"
  },
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"],
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

Install a local directory or a GitHub repository/subdirectory, then restart a
running Zode process so it loads the new snapshot:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Use `zode plugin update my-plugin` after changing the source. `--trust` is
required because JavaScript, hooks, MCP servers, and declared network access are
executable capabilities. Install and update print the plugin's declared
permission grant (network hosts, env vars, context scopes). An update whose
manifest requests *broader* permissions than the installed snapshot is refused
unless you rerun it with `--trust` — a moving Git source cannot silently widen
its own grant.

#### UI render API

UI plugins can contribute declarative rows immediately above the sidebar
version — at most six rows in total, shared across all plugins in load order.
Declare a JavaScript entrypoint in the manifest:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

Register a synchronous renderer with `zode.ui.sidebar`. The context is a
read-only JSON snapshot containing terminal, session, model, status, token, and
context-window fields. The result is rendered by Zode; scripts receive no
filesystem, network, terminal, or Ratatui bridge.

```js
zode.ui.sidebar((ctx) => ({
  lines: [
    {
      spans: [
        { text: ctx.model.id, tone: "accent", bold: true },
        { text: `  ctx ${ctx.context.usedPercent ?? "?"}%`, tone: "muted" }
      ]
    }
  ]
}));
```

Supported tones are `default`, `muted`, `accent`, `success`, `warning`, and
`danger`; spans also accept `bold` and `italic`. A renderer must be synchronous.
Each script is limited to 256 KiB, 8 MiB of JS memory, and 25 ms per evaluation,
and renderers are re-evaluated at most every 250 ms (cached output is reused
between evaluations). Sidebar output is limited to 6 lines per renderer (6 in
total across plugins), each line to 16 spans and 2,048 bytes of text. Control
characters are sanitized by the host.

The status bar is also extensible. It remains one row when no plugin returns
content and grows to two rows dynamically when a synchronous
`zode.ui.statusLine` renderer returns spans. Zode keeps its core status and
safety indicators on the first row; plugin output is composed on the second.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Render context and permissions

Every renderer receives the following base fields without requesting additional
context permission:

| Field | Shape and meaning |
| --- | --- |
| `ctx.apiVersion` | Context API version; currently `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | `{ width, height }` in terminal cells. |
| `ctx.session` | `{ id, title, cwd, busy }` for the active task. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` contains `{ enabled, readOnly, network }`. |
| `ctx.tokens` | `{ input, output }` token counters. |
| `ctx.context` | `{ used, window, usedPercent }`; the percentage can be `null`. |
| `ctx.data` | Results belonging only to data sources registered by this plugin. |

Richer sections are omitted unless the plugin requests the matching scope in
`permissions.context`:

| Scope | Exposed field | Shape and limits |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` is one-based. |
| `workspace` | `ctx.workspace.modifiedFiles` | Up to 50 `{ path, added, removed }` Git entries. |
| `tools` | `ctx.tools.available` | Sorted names of tools enabled for the active task. |
| `tools` | `ctx.tools.active` | Names of tools currently executing. |
| `tools` | `ctx.tools.recent` | Up to 20 `{ name, status, durationMs }` records. |
| `tasks` | `ctx.tasks.todoStatuses` | Todo status strings only, without todo text. |
| `tasks` | `ctx.tasks.subagents` | `{ type, status }` records, without prompts or transcripts. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, without goal text. |
| `services` | `ctx.services.mcp` | `{ name, connected }` records. |
| `services` | `ctx.services.lsp` | `{ language, running }` records. |

For example:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` is an observation API: it tells a renderer which tools exist and
which tools are or were running. UI plugins cannot invoke a tool. Tool inputs,
tool outputs, prompts, transcript content, todo/goal text, environment values,
and credentials are not included, and the API cannot bypass Zode's approval
system.

#### Background HTTP data

UI plugins can also register background HTTP data sources. Network and secret
access must be declared in the manifest:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

The request is declarative and runs outside the render path. Secret environment
variables are assembled into headers by Zode and are never exposed to
JavaScript:

```js
zode.data.define("codingPlan", {
  refreshIntervalMs: 60000,
  request: {
    url: "https://quota.example.com/v1/usage",
    method: "GET",
    timeoutMs: 3000,
    headers: {
      Authorization: { env: "CODING_PLAN_TOKEN", prefix: "Bearer " }
    }
  }
});

zode.ui.statusLine((ctx) => ({
  spans: [
    {
      text: `remaining ${ctx.data.codingPlan?.data?.remaining ?? "…"}`,
      tone: "accent"
    }
  ]
}));
```

`zode.data.define(key, config)` accepts a 1–64 character alphanumeric,
underscore, or hyphen key. `request` supports `url`, `method`, `headers`,
optional JSON `body`, and `timeoutMs`. Defaults are `GET`, 3-second timeout,
and a 60-second refresh. Only HTTPS `GET` and `POST` are accepted. Literal
headers are strings; a secret header uses
`{ "env": "NAME", "prefix": "Bearer " }`. The environment variable must also
appear in `permissions.env`, is read only by Rust when building the request,
and is never returned to JavaScript.

Zode disables redirects and proxies, validates and pins public DNS addresses,
rejects localhost/private networks, caps responses at 256 KiB, clamps request
timeouts to 500 ms–10 seconds, and clamps refresh intervals to
10 seconds–1 hour. A wildcard such as `*.example.com` matches subdomains but
not the bare `example.com` host.

Each plugin sees only its own data. `ctx.data.<key>` contains
`{ ok, status, data, updatedAt }` or
`{ ok: false, error, updatedAt }`. JSON responses become objects/arrays;
non-JSON responses become strings. An HTTP error status still includes
`status` and `data`, with `ok: false`.

Start Zode with the required secret in its environment when using a private
quota or coding-plan API:

```bash
CODING_PLAN_TOKEN=... zode
```

The [complete runnable example](examples/plugins/zode-ui-demo/) displays
model/context/tool activity in the sidebar and status line and uses
`zode.data.define` for a public GitHub API quota.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# A marketplace is a local/Git static index, not a Zode-hosted service.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # disambiguate if needed
zode plugin marketplace update
```

### ACP, dashboard, telemetry, and TUI regression tests

`zode acp` implements ACP initialize/new/load/fork/prompt/cancel over stdio,
streams message/thought/tool updates, requests permissions through the client,
and accepts client-supplied stdio, HTTP, and SSE MCP servers. Session data uses
the same V1-compatible store as the TUI and headless CLI.

```bash
zode acp
zode dashboard
zode dashboard --json
```

OTLP export is off by default and requires an explicit opt-in. It exports only
content-free lifecycle/tool-name/status/usage attributes: prompts, generated
text, tool inputs/outputs, file paths, and error messages are never sent.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

For real-terminal TUI regression scenarios, the workspace includes a PTY +
VT100 harness that records raw diagnostics and virtual-screen snapshots:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` drives the real terminal with ordered waits, key input,
resizes, and snapshots (key notation supports `<Enter>`, `<Esc>`, `<Tab>`,
`<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>`, and
`<C-l>`):

```json
{
  "command": ["target/debug/zode", "--no-sandbox"],
  "rows": 40,
  "cols": 120,
  "steps": [
    { "action": "wait_for_text", "text": "zode", "timeout_ms": 5000 },
    { "action": "send_keys", "keys": "hello<Enter>" },
    { "action": "resize", "rows": 50, "cols": 140 },
    { "action": "snapshot", "path": "target/pty/after-input.json" }
  ]
}
```

This local/open implementation deliberately does not include xAI-specific
accounts, billing, or a Zode-operated cloud marketplace service.

Optional top-level config keys (all have sensible defaults):

```jsonc
{
  "maxOutputTokens": 16384,      // per-turn output cap (raise for big file writes)
  "contextWindow": 1000000,      // model context window — set 1000000 for a 1M model
  "temperature": 0,              // lower = more deterministic
  "language": "zh-CN",           // UI language (15 locales); also via /language
  "effort": "medium",            // default reasoning effort; also via /effort
  "autonomousOrchestration": true, // sub-agent + workflow orchestration (default on)
  "subagentMaxIterations": 0,      // optional child guard; omitted/0 = unbounded
  "sandbox": {
    "enabled": true,             // OS sandbox for shell commands (default on)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // allow outbound network inside the sandbox
    "writableRoots": []          // extra writable dirs (workspace-write)
  },
  "browser": {
    "enabled": true,             // browser_* tools and /browser panel (default on)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // managed Chromium launch mode
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // watch unattended /loop and /schedule turns
    "inactivityTimeoutSecs": 900, // abort after 15 minutes without provider/tool activity
    "maxRuntimeSecs": 3600,      // absolute one-hour cap per background turn
    "abortGraceSecs": 10,        // wait for cooperative cancellation before hard-stop
    "maxRetries": 3,             // consecutive recovery attempts before exhaustion
    "initialBackoffSecs": 5,     // first retry delay
    "maxBackoffSecs": 300        // cap for exponential retry backoff
  }
}
```

> The sandbox confines shell commands (macOS: sandbox-exec; Linux: `bwrap`,
> which must be installed). Startup fails closed if the configured sandbox
> cannot be verified; use the explicit `--no-sandbox` flag to run without it.
> Network is denied by default. If a command genuinely needs to escape, the
> model sets `dangerouslyDisableSandbox: true` and **you** authorize it at the
> approval prompt — or toggle the whole sandbox live with `/sandbox`.

> `contextWindow` drives auto-compaction — set it to your model's real window
> (e.g. `1000000`). Prefer the **per-model** value under
> `providers.<name>.models.<id>.contextWindow` (it takes precedence); the
> top-level key above is a global fallback, and zode also fills it from the
> bundled models.dev catalog when neither is set. Do **not** set it above the
> real window: overestimating makes requests overflow and the provider rejects
> the turn.

## Server Mode and SDKs

`zode server` starts a newline-delimited JSON-RPC server on stdin/stdout. It is
intended for editor integrations, local automation, tests, and SDK clients that
want zode's existing capabilities without launching the TUI.

```bash
zode server                      # stdio (default) — what the SDKs spawn
zode server --listen stdio://    # same thing, spelled out
zode server --listen ws://127.0.0.1:0   # loopback WebSocket + Bearer auth
zode server --listen off         # start nothing and exit
```

Server mode exposes zode-backed behavior:

- initialization + capability discovery (with an `approvalPolicy` of
  `readOnly` (default) / `auto` / `prompt`)
- thread metadata lifecycle and **streaming turns** — model output and tool
  calls arrive as JSON-RPC notifications; `turn/interrupt` cancels a turn
- **interactive approvals** — `prompt` policy drives server→client
  `approval/request` frames answered with `allow` / `allowAlways` / `deny`
- filesystem read/write/create/stat/list/remove/copy and one-shot `command/exec`
- model list/set, config read/list/write, and read-only skills, hooks,
  MCP-server status, and plugin lists

The WebSocket transport binds loopback only and writes a `0600`
`<config-dir>/server.json` credentials file (`{port, pid, token}`); clients
authenticate with `Authorization: Bearer <token>`. See
[`sdk/README.md`](sdk/README.md) for the full protocol, notification field
names, and per-language examples.

For this app-server protocol specifically, hosted marketplace management,
remote-control, Realtime, standalone process spawn, background terminals,
thread archive/fork, goals, and app connectors remain out of scope. The local
session and static-plugin marketplace commands documented above are separate
CLI surfaces.

SDKs live under [`sdk/`](sdk/):

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

Each SDK exposes a native `ProtocolMethod` enum/constant set for the current
stable method names, so integrations can avoid hard-coded JSON-RPC strings.
Every supported method's params, result shape, and SDK enum/constant name are
documented in the [`sdk/` method reference](sdk/README.md#method-reference).

Run the SDK checks that are available on your machine with:

```bash
scripts/test-sdks.sh
```

Protocol fixtures are generated from `zode-app-server-protocol`:

```bash
cargo run -p zode-app-server-protocol --bin export -- sdk/fixtures/jsonrpc
```

## Browser Control

Zode includes a `tools:browser` group for browser automation. The agent can use
`browser_read` for screenshots, DOM snapshots, console logs, network logs, and
tab reads; `browser_act` for navigation, clicks, typing, key presses, and
scrolling; `browser_eval` for JavaScript; and `browser_tabs` for tab
management. Read-only browser inspection is ungated; mutating browser actions
use the same allow-once / always / deny approval flow as other side-effecting
tools.

There are two browser targets:

- **managed** — zode launches and controls a dedicated Chromium profile.
- **bridge** — zode controls the Chrome profile you are already using through
  the bundled MV3 extension in [`extensions/chrome/`](extensions/chrome/).

For the bridge target, load the extension once from `extensions/chrome`, then
run `/browser pair`. Zode opens the extension page with the local WebSocket
port and pairing code pre-filled; after the first pairing, the extension stores
a token. It reconnects to a running CLI or auto-starts an extension-only zode
daemon when needed. Tabs opened by zode are placed in a Chrome tab group named
`zode`.

### Chrome task side panel

Run the updated zode CLI and `/browser pair` once. Clicking the toolbar icon
opens the side panel; afterwards it auto-starts zode automatically when no CLI
process is running. The pairing page remains a small code/token flow, and tasks
stay shared with TUI sessions without changing terminal focus.

Side-panel turns bind bridge browser tools to the page currently shown beside
the panel, so requests such as “analyze this page” use `browser_read` on the
existing tab instead of opening a new one. Standalone TUI and CLI browser
automation keeps using zode-owned tabs in the `zode` tab group. The active page
is also the default context for ambiguous side-panel prompts; local project
files are inspected only when the user explicitly asks about them.

The panel can send text, select a model, choose access modes `readOnly`,
`prompt`, and `auto`, stream the response, and Stop a running turn. A turn can
attach at most 8 files and 20 MiB total: PNG, JPEG, GIF, and WebP images up to
5 MiB each, plus UTF-8 text and code files up to 1 MiB each. PDF, Office,
archive, executable, and non-UTF-8 inputs are rejected.

After an extension update, click Reload on `chrome://extensions`. Older
extension versions remain compatible with browser automation but do not have
the task side panel. On Windows, zode locates and launches Chrome directly for
extension URLs instead of invoking the default-browser shell, avoiding
Microsoft Store redirection when Chrome is already installed.

Useful commands:

```bash
/browser                         # open the browser control panel
/browser status                  # show target/running/paired state
/browser launch                  # launch the managed browser
/browser close                   # close the managed browser
/browser pair                    # pair or reconnect the Chrome bridge extension
/browser target managed          # use zode's managed Chromium
/browser target bridge           # use the extension and save it as the next-launch default
/browser screenshot [path]       # capture a browser screenshot
```

See [`extensions/chrome/README.md`](extensions/chrome/README.md) for extension
loading, update, CRX packaging, and smoke-test steps.

## Desktop Control

Zode can drive native desktop applications through OS accessibility APIs, not
just the browser. The agent uses `desktop_read` to read the accessibility tree
(windows, elements, and their refs), `desktop_act` to click, type, scroll, and
set values by element, and `desktop_screenshot` to capture the screen.
Read-only reads are ungated; mutating desktop actions use the same
allow-once / always / deny approval flow as other side-effecting tools.

Backends are selected per platform:

- **macOS** — the Accessibility (AX) API.
- **Windows** — UI Automation (UIA).
- **Linux** — AT-SPI.
- **Electron apps** — attach over the Chrome DevTools Protocol.

**Ghost cursor and Esc stop.** Zode never moves your real mouse. On macOS a
zero-permission overlay (`zode-overlay`) draws a *fake* cursor that flies along
a smooth Dubins path to each action's target, so you can follow what the agent
is doing; typed text is never shown in the overlay. While desktop automation is
active, a global **Esc** interrupts every running turn and hides the overlay
(the same stop path as the TUI's Esc). Other platforms run desktop actions
without the visualization.

CJK and other text without a US-layout keycode is delivered via the system
pasteboard (write → synthesize paste → restore the previous clipboard) so apps
with custom key handling receive the real characters.

```bash
/desktop            # show desktop target and permission state
/desktop status     # same, explicit
```

Config lives under `desktop.*` in `~/.zode/config.json`:

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor` (default `true`) draws the macOS overlay cursor; `escCancel`
(default `true`) arms the global-Esc interrupt during automation;
`overlayHelperPath` (default `null`) overrides the `zode-overlay` helper
location — a missing helper simply disables the visualization. Desktop
automation may prompt for OS permission (e.g. macOS Accessibility) on first use.

## Background Turn Watchdog

Scheduler-owned `/loop` and `/schedule` turns run under an in-process liveness
watchdog. Provider, tool, and nested-agent activity refreshes a shared
source-side heartbeat, while
`maxRuntimeSecs` remains an absolute cap. On either timeout, zode requests
cooperative cancellation, waits `abortGraceSecs`, and hard-stops the local turn
task if it still has not drained. Stopping the task is not enough to release
its scheduler slot: zode also waits for every tracked provider, tool,
hook, subprocess reader, and nested-agent worker to quiesce. If that second
boundary is not reached within five seconds, the tab/store is quarantined, the
job is disabled, and its live-attempt lease remains held until the workers
actually exit.

Failed attempts use bounded exponential backoff from `initialBackoffSecs` to
`maxBackoffSecs`. A successful turn clears its consecutive-failure count; once
`maxRetries` is exhausted, zode stops the loop or disables the persisted
schedule. Manual interruption, job removal, and explicit disabling cancel
pending recovery instead of creating another retry when no mutation started.
Recovery is intentionally conservative around side effects: zode retries
automatically only when it has not observed a side effect; if a mutation may
already have happened, including a manual cancellation mid-mutation, it
stops/disables the job and waits for human review. Tools that deliberately
detach work (`BashRun` or a detached GUI) also stop recurrence after that turn.
The same inactivity limit bounds claim-to-start queueing: if a busy tab or
turn preflight keeps an owned occurrence from starting, it becomes a normal
side-effect-free watchdog failure and enters the same bounded retry policy
instead of holding its cross-process lease forever.

Quiescence is a local guarantee. Work already accepted by a remote MCP server,
browser extension, desktop actor, or other external system may not support
revocation. If such a call is interrupted, zode marks its result unresolved,
disables the scheduler job, and requires you to verify the external state
before re-enabling it.

Use `/watchdog status` for configuration and per-turn/retry health. The same
state appears in `/tasks` alongside background shells and running turns;
claimed queue age and terminal-persistence fences are shown there too.

This is a watchdog for scheduler turns inside the current zode process. It is
not an OS process supervisor and cannot restart zode after a crash or machine
restart; use your platform's service manager when process-level restarts are
required. Persisted schedules record an active-attempt token backed by a
per-schedule OS file lock. At startup, a contended lock is left alone because
another zode process still owns it; a free lock with the exact persisted token
is an orphan from an unclean exit, so zode disables that schedule as
execution-state-unknown instead of replaying it silently.
This recovery contract covers process crashes. It does not claim storage-level
durability across sudden power loss or failed hardware, and it does not replace
an OS service manager.

The fire timestamp and active-attempt token are claimed atomically before a
persisted prompt enters a tab queue, so queued work is already exclusive across
zode processes. That same lease moves with the prompt into the turn and remains
held through final transcript/index persistence. Editing, removing, or
disabling a queued occurrence is an explicit cancellation and clears only its
matching active token. Graceful application exit instead restores the exact
unstarted fire watermark or retry token, so it cannot consume work that never
ran. A terminal roster write that fails keeps the lease in a retrying finalizer;
a conflicting token is durably disabled for review before release. Scheduler
turns skip detached post-turn memory extraction, and graceful exit drains
worker quiescence plus terminal persistence before destroying their tabs.
Recurrence phase is canonical: interval slots use absolute epoch arithmetic
from the persisted anchor (including across DST fallback), calendar schedules
keep their wall-clock phase, and missed backlog coalesces to the latest due
slot. A running process also refreshes the roster so remote disable/remove,
retry, and orphan ownership changes take effect without a restart.

## Hooks

Hooks run your code on agent events (before/after a tool, session start/end,
user/assistant messages, compaction, and more). They are declared in
`hooks.json`, merged from `~/.zode/hooks.json`, the project's
`.zode/hooks.json`, and any installed plugin:

```json
{
  "hooks": [
    { "event": "before_tool_use", "tool": "Bash", "script": "~/.zode/hooks/guard.js" },
    { "event": "after_tool_use", "script": "~/.zode/hooks/lint.sh" }
  ]
}
```

Each entry matches an `event` (optionally narrowed to one `tool`) and points at
a `script`. Two kinds of script are supported:

- **Executable** (`.sh`, a binary, anything with a shebang) — the event is
  serialized to JSON on the script's **stdin**; the **exit code** decides the
  outcome: `0` proceed, `2` block the triggering action, anything else warn.
- **Sandboxed JavaScript** (`.js`) — run **in-process** in Zode's QuickJS
  sandbox (no filesystem, network, terminal, or process access; bounded memory
  and time), so no shell or Node is required. Register a synchronous handler
  with `zode.hook(fn)`; it receives the event and returns an outcome:

  ```js
  zode.hook((event) => {
    if (event.tool === "Bash" && /rm\s+-rf/.test(event.input.command || "")) {
      return { block: true, reason: "refusing destructive rm -rf" };
    }
    return { ok: true };
  });
  ```

  Return `{ ok: true }` / nothing / `true` to proceed, a non-empty string or
  `{ block: true, reason }` to block (the reason is written to the log), or
  `{ warn: <code> }` to warn. A `.js` hook's source is loaded when the session
  is assembled — restart (or open a new session) to pick up edits; executable
  hooks are re-read on every event.

Plugins ship hooks under `hooks/hooks.json`; a hook `script` may use
`${ZODE_PLUGIN_ROOT}` (the install dir) and `${ZODE_PLUGIN_DATA}` (a per-plugin
data dir). See the runnable [`examples/plugins/zode-hook-demo/`](examples/plugins/zode-hook-demo/)
— a JavaScript `before_tool_use` hook that blocks destructive Bash commands.
Installing a plugin with hooks requires `--trust`.

## Slash Commands

| Command | What it does |
|---|---|
| `/help` | Commands + keybindings overlay |
| `/clear` | Clear the conversation (and context) |
| `/model [id]` | Show / note the active model |
| `/config` | Show model + working directory |
| `/compact` | Context auto-compaction status |
| `/cost` | Token usage & cost so far (incl. sub-agents) |
| `/theme [id]` | Switch theme (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Session picker — resume into a new tab with history |
| `/connect` | Connect and switch the active provider |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Show/hide the right sidebar; fold the MCP / modified-files / todo sections (also click their ▼ headers) |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Browser control panel and commands; pair the Chrome bridge extension or switch between managed Chromium and your Chrome profile |
| `/loop <interval> [--max N] <prompt>` | Run a recurring prompt in the current tab; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Persist a scheduled prompt; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Show background-turn watchdog configuration, health, and pending retries |
| `/tasks` | Background shells, running turns, and watchdog health panel |
| `/undo`, `/redo` | Undo / redo the last file edit |
| `/mcp` | Manage MCP servers — enable / disable in a dialog |
| `/skills` | List available skills |
| `/agents` | Manage sub-agents — create (AI-assisted or manual) / delete |
| `/external-agents [list\|discover]` | List supported external CLIs on `PATH`, or explicitly register every detected preset |
| `/team [status\|board\|dismiss <name>]` | Inspect the persistent teammate roster and shared board, or dismiss a teammate |
| `/workflows` | Manage & run JS-scripted workflows (`agent()`/`parallel()`/`pipeline()` orchestration, executed deterministically by zode) |
| `/effort` | Pick the reasoning effort level |
| `/thinking`, `/tool-details` | Toggle showing reasoning / tool-call detail |
| `/orchestration` | Toggle autonomous sub-agent + workflow orchestration |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | Show / control the OS sandbox at runtime |
| `/language` | Switch the UI language (15 locales) |
| `/export [path]` | Export the transcript to Markdown (a dir gets a default name) |
| `/yolo` | Bypass-approval mode |
| `/exit` | Quit |

Created agents and skills, and connected MCP tools, also appear as dynamic
slash commands (e.g. `/<name>`) and can be invoked directly.

## Keybindings

> On macOS the app chords below use **`Cmd`** (⌘); on Windows/Linux they use
> `Ctrl`. `Ctrl+C/D/L/V` stay `Ctrl` everywhere (terminal conventions).

| Key | Action |
|---|---|
| `Enter` | Send message (queues if a turn is running) |
| `Shift`/`Alt`+`Enter` | Newline |
| `Up` / `Down` | Recall previous / next submitted prompt (or move the autocomplete selection) |
| `Ctrl+C` | Interrupt the turn (quit when idle) |
| `Ctrl+D` | Quit |
| `Ctrl+L` | Redraw the conversation from the store (recovers a blanked view; use `/clear` to discard) |
| `Ctrl+V` | Paste (text or image paths) |
| `Cmd/Ctrl+O` | Settings |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | New tab / close tab |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Jump to / cycle tabs |
| `Cmd/Ctrl+B` | Background tasks panel |
| `Cmd/Ctrl+G` | Toggle the sidebar |
| `F1` | Help |
| `PgUp` / `PgDn` | Scroll the conversation |
| `Home` / `End` | Jump to the top / latest of the conversation |
| `Esc` | Close the current overlay (or interrupt a running turn) |

## Project Instructions

Zode reads instructions from a three-level hierarchy (later wins attention):
global `~/.zode/AGENTS.md` (or `instructions.md`) → project root → cwd. In each
directory it prefers `AGENTS.md` over `CLAUDE.md`. Skills live under
`.zode/skills/**/SKILL.md`; MCP servers in `~/.zode/mcp.json` ⊕ `.mcp.json`;
hooks in `~/.zode/hooks.json` ⊕ `.zode/hooks.json`.

**Cross-agent configuration.** Zode reads direct skills and MCP configuration
from Claude Code, Codex, Cursor, opencode, Gemini, and related local agents.
Installed plugin trees and plugin caches belonging to those products are never
scanned. To reuse a plugin, install its source explicitly with
`zode plugin install ... --trust`; Codex and Claude Code package formats remain
supported for plugins installed through Zode.

## Configuring MCP Servers

MCP servers live in the same nested-precedence config as everything else —
`~/.zode/mcp.json` for all projects, `.mcp.json` or `.zode/mcp.json` at the
project root to scope one to a repo. No registry, no restart-and-pray: edit
the file, then `/mcp` (or relaunch) to pick it up.

### stdio (spawn a local server)

```json
{
  "servers": {
    "github": {
      "transport": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "$GITHUB_TOKEN" }
    }
  }
}
```

`command`/`args` spawn the server as a subprocess piped over stdio. `env`
values support `$NAME` / `${NAME}` substitution against zode's own process
environment (expanded right before connecting, not written to disk) — handy
for keeping tokens out of the config file itself.

### Streamable HTTP (remote server)

```json
{
  "servers": {
    "linear": {
      "transport": "http",
      "url": "https://mcp.linear.app/mcp",
      "headers": { "Authorization": "Bearer $LINEAR_TOKEN" }
    }
  }
}
```

`"transport": "http"` connects with the current MCP spec's Streamable HTTP
transport — a single `url`, no separate SSE endpoint to configure. `"sse"` is
accepted as an equivalent spelling (some configs — and MCP servers' own setup
docs — still call it that); both resolve to the same connector. `headers` are
forwarded verbatim (including `Authorization`, so Bearer/Basic/custom schemes
all work) and support the same `$VAR` substitution as `env`. Add `"enabled":
false` to any server to keep its definition around without connecting it —
`/mcp` also toggles this per server without hand-editing the file.

### Using it

Every tool a connected server exposes shows up as `mcp__<server>__<tool>`,
callable by the agent like any built-in tool (and `@`-mentionable in the
input box). `/mcp` opens a dialog listing every discovered server —
connected / disconnected / disabled — with Space to toggle one on or off; the
sidebar's collapsible `mcp` section (click its ▼ header, or `/sidebar mcp`)
mirrors the same live connection state at a glance.

Zode also reads direct MCP configuration from Claude Code, Codex, Cursor,
opencode, and Gemini. Home configuration is treated as the user's setup;
project-local foreign MCP definitions are discovered disabled and can be
enabled through `/mcp`. MCP declarations buried in another product's installed
plugin tree are not scanned. `openpencil` is reserved — op-bridge (see below)
drives it natively, so any server declared under that name is ignored.

## Installing Skills & Command Markdown

Both are plain Markdown on disk — no registry, no build step. Drop a file in,
and it's live on the next launch (or `/skills` to check what loaded).

### Install a skill

A skill is a folder with a `SKILL.md` inside. Put it under the project
(`.zode/skills/`) or your home dir (`~/.zode/skills/`):

```bash
mkdir -p .zode/skills/code-review
cat > .zode/skills/code-review/SKILL.md <<'EOF'
---
name: code-review
description: Review a diff for bugs, style, and missing tests
---

You are doing a focused code review. Read the diff or files the user points
at, then report findings ordered by severity: correctness first, then API
design, then style. For each finding give file:line and a suggested fix.
EOF
```

The skill now shows up in `/skills`, the agent can invoke it on its own via
the Skill tool, and it also becomes a dynamic slash command — typing
`/code-review look at src/lib.rs` expands to a prompt that runs the skill.
Extra files next to `SKILL.md` (references, scripts) ship with the skill.
Direct skills directories belonging to Claude Code, Codex, opencode, Cursor,
and related agents are scanned. Skills buried inside those products' installed
plugin trees or caches are not; install the plugin explicitly through Zode if
you want to use it here.

### Install a command (prompt Markdown)

A custom slash command is a single `.md` file whose **filename is the command
name** and whose body is the prompt it submits. Anything you type after the
command is appended to the body:

```bash
mkdir -p .zode/commands            # or ~/.zode/commands for all projects
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

Now `/changelog` submits that prompt, and `/changelog only the sidebar work`
appends your arguments after it. Commands in `~/.claude/commands` and
`~/.codex/commands` (and their project-level equivalents) are loaded too;
commands inside a *foreign plugin tree* are off by default — copy the `.md`
into a `.zode/commands/` dir to opt in.

## ZSeven-W Ecosystem

Zode is part of a broader ZSeven-W stack for AI-native development tools:

| Product | What it is |
|---------|------------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | A pure-Rust async runtime for shipping LLM agents: multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions, and optional coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | A Rust-native cross-platform UI framework where an `.op` file is an app, connecting OpenPencil-style design artifacts to runnable software. |
| [`noema`](https://github.com/ZSeven-W/noema) | A local-first, non-vector memory system for coding agents, with lexical recall, review queues, MCP access, S3 offload, and enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | An open-source AI-native vector design tool for design-as-code workflows, turning prompts into UI directly on a live canvas with concurrent agent teams. |

## Benchmark

Can a Zode harness driving an *open* model keep up with Claude? Across five
dimensions — from one-shot code generation to multi-file agentic work to
instruction-following — **Zode + DeepSeek-v4-pro matches Claude**, every task
scored by a *hidden* grader, head-to-head against Claude solving the same tasks:

| Dimension | Tasks | Claude | Zode + DeepSeek-v4-pro |
|-----------|:-----:|:------:|:----------------------:|
| Code generation (one-shot) | 31 | 100% | 97.8% — model edge; **→ 100% agentic** |
| Agentic harness (read/run/edit/fix) | 6 | 100% | **100%** |
| Tricky bugs (疑难杂症) | 9 | 100% | **100%** |
| Complex multi-file | 5 | 100% | **100%** |
| Instruction-following (MCP / Skills / constraints) | 25 | 100% | **100%** |

Details, methodology, and reproduction below; all suites live in
[`benchmarks/`](benchmarks/).

### Code generation

On a 31-task suite spanning six dimensions (algorithms, strings,
data-structures, math, parsing, edge-cases), each task scored by a hidden test:

| Dimension       | Tasks | Claude | Zode + DeepSeek-v4-pro |
|-----------------|:-----:|:------:|:----------------------:|
| algorithms      |   6   |  6/6   |          6/6           |
| strings         |   5   |  5/5   |          5/5           |
| data-structures |   5   |  5/5   |          5/5           |
| math            |   5   |  5/5   |          5/5           |
| parsing         |   5   |  5/5   |         5/5 \*         |
| edge-cases      |   5   |  5/5   |          5/5           |
| **Total**       | **31**| **31/31 (100%)** |  **31/31 (100%)**  |

\* Over 3 independent runs, Zode + DeepSeek averaged **97.8% pass@1 (91/93)**.
This is **one-shot, tools forbidden** ("reply with only the function"). The
only misses were two edge-case-heavy parsing tasks (`parse_ini`, `csv_parse_row`,
`eval_expr`) that the model gets right ~2 of 3 times — DeepSeek retains some
non-determinism even at `temperature: 0`, and these sit at its one-shot
reliability edge. Crucially, **that gap is the model, not the harness**: run the
*same* tasks agentically — letting Zode write the function, run its own checks,
and fix — and they pass **3/3 and 3/3** (`benchmarks/selfverify.py`). Self-
verification is exactly what the harness is for, which is why every agentic tier
below is 100%.

### Harness (agentic) — read, run, edit, fix

One-shot code-gen barely touches the harness. These tiers force the full agent
loop: the model gets a scratch repo and must **read** the files, **run** the
failing test to reproduce, **edit** the source, and **re-run** to verify — using
real tools (`Bash`, `FileRead`/`FileEdit`/`FileWrite`, `Grep`, `Glob`, `ListDir`).
Each is scored by a *hidden* grader (stronger than any visible test, so the agent
can't game it). Head-to-head, both tracks score the same:

| Tier | What it tests | Tasks | Claude | Zode + DeepSeek-v4-pro |
|------|---------------|:-----:|:------:|:----------------------:|
| Agentic fixes/implements | bug-fix, multi-file, class impl, algorithm, refactor | 6 | 100% | **100%** (18/18, 3 runs) |
| Tricky bugs (疑难杂症) | mutable-default, closure late-binding, dict-mutation-during-iter, shallow aliasing, generator exhaustion, `is` vs `==`, shared class attr, float truncation, off-by-one | 9 | 100% | **100%** (27/27, 3 runs) |
| Complex multi-file | template engine, chainable query engine, multi-file expression interpreter (lexer + evaluator with vars/functions), build-order resolver w/ cycle detection, cross-file precedence-bug trace | 5 | 100% | **100%** (15/15, 3 runs) |

Zode + DeepSeek diagnosed/implemented **every** task in **every** run by
exploring (`Grep`/`Glob`/`ListDir`), reproducing with the test harness, then
editing and re-running to verify — the same loop Claude uses. The harder and
more agentic the task, the more decisive the harness is.

### Instruction-following — MCP, Skills, and constraints

A separate suite measures whether the agent actually *obeys* — invoking the
right MCP tool, loading a skill and following its body's exact rule, and
respecting output-format and negative ("don't use any tool") constraints. The
checks are objective: each MCP tool returns an **unguessable** value (a hidden
offset / a secret token) and each skill produces an **unguessable signature**,
so a correct answer *proves* the tool/skill was actually used as instructed.

| Kind | Examples | Tasks | Claude (ref.) | Zode + DeepSeek-v4-pro |
|------|----------|:-----:|:-------------:|:----------------------:|
| MCP tools | call the named tool; use it even for trivial math | 4 | 100% | **100%** |
| Skills | load skill + obey its rule exactly | 3 | 100% | **100%** |
| Format / negative | JSON-only, exact word, layout, "use no tools" | 4 | 100% | **100%** |
| Hard | multi-tool **sequencing**, **buried** requirement, conditional tool use, skill + post-processing | 6 | 100% | **100%** |
| Adversarial | override ("ignore that, do X"), no-explanation, JSON-no-markdown, ignore-the-distractor, tool + transform | 8 | 100% | **100%** |
| **Total** | | **25** | **100%** | **100%** |

Zode + DeepSeek invoked the correct MCP tool / skill and obeyed every format,
negative, and adversarial constraint — chaining two MCP tools in order, pulling
the one real instruction out of a paragraph of distractor text, and following
overrides ("reply RED — actually, reply BLUE"). The system prompt also carries
an explicit instruction-adherence clause for robustness on the long tail.

**Efficiency.** Zode keeps the system-prompt + tool-schema prefix byte-stable
and **requests prompt caching on every turn** (`promptCache`, on by default):
for Anthropic / MiniMax it sends `cache_control` breakpoints so the prefix
isn't re-billed each turn; OpenAI-compatible providers (DeepSeek, OpenAI, …)
cache that prefix automatically. A steady-state DeepSeek session measured a
**93% input-token cache-hit rate** (`/cost` reports it live). Other knobs: a
configurable low `temperature` for determinism, and a system prompt with
general edge-case + instruction-adherence guidance (no task-specific hints).

**Methodology.** Zode runs headless (`zode -p "<task>" --yolo`,
`deepseek-v4-pro`, `temperature: 0`) in an isolated working directory; Claude's
column is Claude solving each task directly. Both are scored by the *same*
hidden tests in a sandboxed subprocess. Fully reproducible:

```bash
ZODE_CONFIG_DIR=~/.zode python3 benchmarks/run.py        --track both   # code-gen (one-shot)
ZODE_CONFIG_DIR=~/.zode python3 benchmarks/agentic.py    --track both   # agentic harness
ZODE_CONFIG_DIR=~/.zode python3 benchmarks/hardbugs.py   --track both   # tricky bugs
ZODE_CONFIG_DIR=~/.zode python3 benchmarks/complex.py    --track both   # complex multi-file
ZODE_CONFIG_DIR=~/.zode python3 benchmarks/selfverify.py                # one-shot gap → agentic
ZODE_CONFIG_DIR=~/.zode python3 benchmarks/instructions.py              # MCP / Skills / constraints
```

Zode can also act as the host LLM runner for Noema's LOCOMO memory benchmark.
Generate Noema answer/judge task JSONL from the Noema repo, then run:

Before answer/judge grading, Noema's top-200 LOCOMO retrieval proxy, using the Mem0
`mem0ai/memory-benchmarks` LOCOMO data at commit
`4b61c5d31b9c668a12b4f5e78064248a02c82d2b`, is `1536 / 1540 = 99.7%`
for `raw-plus-fact-layer` retrieval. Evidence-only recall is `1536 / 1536 =
100.0%` any-hit at top 200. The predict JSON is about 323 MB. The unbudgeted
answer-task file is about 201 MB after query-aware episode compaction; the
current v7 96k-budget answer-task file is about 148 MB.

Current zode-hosted judged result:

| Benchmark | Cutoff | Runner | Noema score | Mem0 score | Margin | Correct / total | Status |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| LoCoMo | top_200 | zode host runner, v7 96k prompts | 92.6623 | 92.5 | +0.1623 | 1427 / 1540 | exceeds Mem0 and Noema target |

The final answer summary has `unique=1540`, `answers.valid=1540`,
`answers.retryable=0`, `answers.empty=0`, and `answers.failed=0`. The final
judge summary has `unique=1540`, `judges.valid=1540`, `judges.correct=1427`,
`judges.wrong=113`, and `judges.retryable=0`.

```bash
python3 benchmarks/noema_locomo.py \
  --tasks /tmp/noema-locomo-answer-tasks-top200-budget96k.jsonl \
  --output /tmp/noema-locomo-answer-results.jsonl \
  --zode-config-dir ~/.zode \
  --jobs 1 \
  --resume \
  --retry-empty \
  --retry-failed \
  --summary-output /tmp/noema-locomo-answer-results.summary.json \
  --manifest-output /tmp/noema-locomo-answer-run.manifest.json \
  --stop-on-provider-blocker \
  --retries 2

# For recovered runs, Noema can emit a smaller retry-only 96k-budget task file:
#   /tmp/noema-locomo-answer-tasks-retry-zode-top200-budget96k.jsonl

python3 benchmarks/noema_locomo.py \
  --tasks /tmp/noema-locomo-judge-tasks-top200.jsonl \
  --output /tmp/noema-locomo-judge-results.jsonl \
  --zode-config-dir ~/.zode \
  --jobs 1 \
  --resume \
  --retry-empty \
  --retry-failed \
  --summary-output /tmp/noema-locomo-judge-results.summary.json \
  --manifest-output /tmp/noema-locomo-judge-run.manifest.json \
  --stop-on-provider-blocker \
  --retries 2

# Judge retries can likewise use:
#   /tmp/noema-locomo-judge-tasks-retry-top200.jsonl
```

The runner prints a compact summary and can write the same counts to
`--summary-output`. A task file can be inspected without an output file or host
LLM call:

```bash
python3 benchmarks/noema_locomo.py \
  --dry-run \
  --tasks /tmp/noema-locomo-answer-tasks-top200.jsonl \
  --summary-output /tmp/noema-locomo-answer-dry-run.summary.json \
  --manifest-output /tmp/noema-locomo-answer-dry-run.manifest.json
```

Existing result files can also be summarized without a task file or host LLM
call:

```bash
python3 benchmarks/noema_locomo.py \
  --summary-only \
  --fail-on-retryable \
  --output /tmp/noema-locomo-answer-results.jsonl \
  --summary-output /tmp/noema-locomo-answer-results.summary.json \
  --manifest-output /tmp/noema-locomo-answer-summary.manifest.json
```

With `--fail-on-retryable`, the runner exits with status 2 when latest results
still contain ordinary retryable answer or judge rows; if those retryables include
a provider blocker such as `http_402_payment_required`, summary-only exits with
status 3. Use Noema's `--locomo-fail-if-incomplete` gate for the authoritative
readiness view across predict, answer, and judge artifacts.
For retry batches that may hit provider balance or billing limits, run with
`--jobs 1 --stop-on-provider-blocker`; the runner stops at the first provider
blocker such as `http_402_payment_required`, writes the partial JSONL and
manifest, and exits with status 3 instead of burning through the remaining
tasks.
`--manifest-output` writes the runner provenance used for reproducible benchmark
reports: task/result paths, zode binary, provider/model names, retry settings,
run/skipped counts, `tasks_total`, `pending_before_run`,
`unrun_due_to_provider_blocker`, task input size stats, prompt character
distribution, a rough `ceil(prompt_chars / 4)` prompt-token estimate, the same
latest-row summary, provider-blocker status, and answer failure-reason buckets
such as `http_402_payment_required`.
Noema can embed that file into its combined run report with
`--locomo-host-manifest-input`, so the final report contains both memory-side
readiness and zode host-run provenance.
It intentionally does not include provider API keys.
Use `--zode-config-dir` to run against an isolated zode config directory instead
of whatever global config is active in the shell.

The final v7 96k answer run used append-only/latest-row semantics. The initial
full pass reused 81 matching fingerprint results, ran 1459 tasks, and produced
16 retryable answer rows; a resume pass with `--retry-empty --retry-failed`
recovered all 16, leaving zero retryable answer rows.
The current v7 96k-budget answer manifest records
`task_file_bytes=148381415`, `tasks_loaded=1540`, and prompt chars
`p50=95738`, `p95=95959`, `max=96000`. It estimates prompt input at `36611760`
tokens total, with `p50=23935`, `p95=23990`, and `max=24000` per task using the
runner's 4 chars/token estimate. It also records Noema prompt-retention stats:
`retrieval_results_in_prompt.mean=155.3422077922078`, `p95=192`, and
`truncated_memories.total=0`. These prompt and retention stats are also present
in Noema's retention artifact under `prompt_summary`. For comparison, the
unbudgeted compacted top-200 task file records `task_file_bytes=200747951`,
prompt chars `p50=130719`, `p95=147523`, `max=162186`, and about `49778930`
estimated prompt tokens total.
The current full judge-task artifact is
`/tmp/noema-locomo-judge-tasks-zode-top200-v7-96k-full.jsonl`; its zode
manifest records `task_file_bytes=2608433`, `tasks_loaded=1540`, prompt chars
`total=1683047`, `p50=1070`, `p95=1323`, `max=2110`, and estimated prompt input
tokens `total=421327`, `p50=268`, `p95=331`, `max=528`. The judge run reused 81
matching fingerprint results and ran 1459 tasks, with zero retryable judge rows.

Noema has also been dry-run compared at 48k, 64k, and 96k prompt budgets. The
conservative evidence-ID audit is now reproducible through Noema's
`--locomo-retention-output` command. It shows 96k is the current recommended
default: 48k retains `1515 / 1536` ID any-hits, 64k retains `1529 / 1536`, and
96k retains `1534 / 1536`, matching the top-200 ID baseline any-hit while still
reducing prompt tokens versus the unbudgeted task file. The 96k audit artifact
is `/tmp/noema-locomo-answer-retention-top200-budget96k.json`. Noema also writes
the combined proxy/retention/readiness report at
`/tmp/noema-locomo-run-report-top200-budget96k-v7-full.json`; the current report
has `completion.final_ready=true`, `completion.blocked_reason=ready`, and
`target_verdict.score=92.66233766233766`. `--locomo-target-output` plus
`--locomo-require-beats-mem0` can enforce the judged LOCOMO score against
Mem0's `92.5` LoCoMo target.

The suites, hidden graders, Claude's reference solutions, and the runners live in
[`benchmarks/`](benchmarks/).

## Architecture

Zode is a Cargo workspace of three crates over the shared `agent` runtime:

```text
zode/
├── vendor/agent/      agent-rs submodule (agent + agent-tools-code crates)
└── crates/
    ├── zode/          bin: arg parsing, headless modes (-p / --no-tui), dispatch
    ├── zode-core/     UI-agnostic: config, engine, tools, commands, history,
    │                  sandbox, skills, mcp, cost, instructions, approvals
    └── zode-tui/      ratatui chrome: app loop, chat, dialogs, themes, tabs
```

Key design points:

- **The agent runtime is upstream.** `zode-core` wraps it; gaps are fed back to agent-rs rather than forked.
- **Interactive approval lives in a tool decorator** (`PermissionGatedTool` + `ApprovalGate`), not in the query loop — the `PermissionManager` carries only hard-deny rules.
- **MCP tools** are surfaced through a `ZodeMcpTool` adapter; **skills** through a `SkillTool` plus a system-prompt index.
- **Each tab is an isolated engine** (own message store, cost tracker, turn state) sharing one approval channel; sub-agents inherit the parent's gated + sandboxed tools.

## Development

```bash
cargo build --workspace                 # build everything
cargo run -p zode                       # run the TUI
cargo run -p zode -- -p "<prompt>"      # headless single turn
cargo test --workspace                  # all tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                        # licenses / advisories / bans
```

## Contributing

Contributions welcome! Please follow [Conventional Commits](https://www.conventionalcommits.org/) — `<type>(<scope>): <subject>` with scopes like `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`.

## Acknowledgements

Thanks to rtk for the output-compression ideas and to ripgrep for the fast
search engine that powers Zode's Grep tool.

## License

[MIT](LICENSE) &copy; ZSeven-W
