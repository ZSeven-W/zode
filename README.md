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

---

## Highlights

- **Multi-provider** — Anthropic, OpenAI, and any OpenAI-compatible API (DeepSeek, Moonshot, OpenRouter dialects), plus local Ollama
- **Rich tool surface** — file read/write/edit, code & content search, foreground and background shells, git, web fetch, notebooks, TODO tracking
- **Interactive permissions** — every mutating tool is gated: allow once / allow always / deny, with hard-deny rules and a `--sandbox` mode that confines writes to the working dir
- **Full-screen TUI** — streaming markdown with syntax highlighting, diff previews, slash-command autocomplete, 4 built-in themes, settings & help overlays
- **Multi-session tabs** — run several conversations side by side (`Ctrl+T`), each an isolated agent; resume past sessions with full history replay
- **Sub-agents** — delegate scoped work to `researcher` / `reviewer` / `general` child agents via the Task tool (they inherit the same gate, sandbox, and hooks)
- **Skills & MCP** — load `SKILL.md` instruction packs on demand and connect MCP servers (`mcp__<server>__<tool>`)
- **Hooks** — run external scripts on tool events (e.g. block dangerous commands, lint after edits)
- **Three-level instructions** — global (`~/.zode/`) → project root → cwd (`AGENTS.md` / `CLAUDE.md`)

## Install

**From source** (requires a recent stable Rust toolchain):

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binary at target/release/zode
```

> The agent runtime lives in the `vendor/agent` git submodule — always clone
> with `--recurse-submodules` (or run `git submodule update --init`).

## Quick Start

Create `~/.zode/config.json` with a provider:

```jsonc
{
  "provider": {
    "type": "anthropic",                 // "anthropic" | "openai" | "ollama"
    "apiKey": "sk-...",
    "model": "claude-sonnet-4-6"
  }
}
```

OpenAI-compatible providers set a `baseUrl` and (optionally) a `dialect`:

```jsonc
{
  "provider": {
    "type": "openai",
    "apiKey": "sk-...",
    "baseUrl": "https://api.deepseek.com/v1",
    "model": "deepseek-v4-pro",
    "dialect": "deepseek"                 // "standard" | "deepseek" | "moonshot" | "openrouter"
  }
}
```

Then run:

```bash
zode                       # full-screen TUI
zode -p "explain main.rs"  # headless: one prompt, stream to stdout, exit
zode --no-tui              # plain readline REPL
zode -c                    # continue the most recent session
zode -r <id>               # resume a session by id prefix
zode --yolo                # bypass approval prompts (deny rules still apply)
zode --sandbox             # confine mutating tools to the working directory
zode --model <id>          # override the model
zode --provider <name>     # pick a named provider from config.providers
```

You can also point at any provider without editing the config by exporting the
matching key (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); for Ollama the
`baseUrl` is taken from the environment when unset.

## Slash Commands

| Command | What it does |
|---|---|
| `/help` | Commands + keybindings overlay |
| `/clear` | Clear the conversation (and context) |
| `/model [id]` | Show / note the active model |
| `/config` | Show model + working directory |
| `/compact` | Context auto-compaction status |
| `/cost` | Token usage & cost so far (incl. sub-agents) |
| `/theme [id]` | Switch theme (`catppuccin-mocha`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Session picker — resume into a new tab with history |
| `/tasks` | Background shells + running turns panel |
| `/undo`, `/redo` | Undo / redo the last file edit |
| `/mcp` | List configured MCP servers |
| `/skills` | List available skills |
| `/yolo` | Bypass-approval mode |
| `/exit` | Quit |

## Keybindings

| Key | Action |
|---|---|
| `Enter` | Send message |
| `Shift`/`Alt`+`Enter` | Newline |
| `Ctrl+C` | Interrupt the turn (quit when idle) |
| `Ctrl+D` | Quit |
| `Ctrl+L` | Clear the screen |
| `Ctrl+O` | Settings |
| `Ctrl+T` / `Ctrl+W` | New tab / close tab |
| `Ctrl+1`–`9` / `Ctrl+Tab` | Jump to / cycle tabs |
| `Ctrl+B` | Background tasks panel |
| `F1` | Help |
| `PgUp` / `PgDn` | Scroll |
| `Esc` | Close the current overlay |

## Project Instructions

Zode reads instructions from a three-level hierarchy (later wins attention):
global `~/.zode/AGENTS.md` (or `instructions.md`) → project root → cwd. In each
directory it prefers `AGENTS.md` over `CLAUDE.md`. Skills live under
`.zode/skills/**/SKILL.md`; MCP servers in `~/.zode/mcp.json` ⊕ `.mcp.json`;
hooks in `~/.zode/hooks.json` ⊕ `.zode/hooks.json`.

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

## License

[MIT](LICENSE) &copy; ZSeven-W
