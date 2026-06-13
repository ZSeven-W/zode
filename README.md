<p align="center">
  <img src="https://img.shields.io/badge/TypeScript-5.8-3178c6?style=flat-square&logo=typescript&logoColor=white" alt="TypeScript" />
  <img src="https://img.shields.io/badge/Rust-NAPI-f74c00?style=flat-square&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/Bun-workspace-f472b6?style=flat-square&logo=bun&logoColor=white" alt="Bun" />
  <img src="https://img.shields.io/badge/license-MIT-22c55e?style=flat-square" alt="MIT License" />
</p>



<h1 align="center">Zode</h1>

<p align="center">
  <strong>Open-source, AI-native coding assistant for your terminal.</strong><br/>
  Reads your code. Runs commands. Searches files. Manages git. Extensible via plugins.
</p>

---

## Highlights

- **Multi-provider** — Anthropic, OpenAI, DeepSeek, Groq, Ollama, or any OpenAI-compatible API
- **25 built-in tools** — file ops, shell execution, code search, git, web fetch
- **Microkernel + plugins** — hot-reloadable plugins with lifecycle hooks
- **Permission engine** — granular auto / ask / deny rules per tool and command pattern
- **Full-screen TUI** — Catppuccin theme, gradient borders, animated spinner, markdown rendering
- **Rust-accelerated** — ripgrep search, tree-sitter highlighting, fast diff (with TS fallback)
- **Three-level config** — global (`~/.zode/`) → project → directory recursive
- **Enterprise ready** — remote `.well-known` policy, sandbox mode, YOLO mode

## Quick Start

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode && bun install
```

Set up a provider:

```bash
export ANTHROPIC_API_KEY=sk-...    # or OPENAI_API_KEY, DEEPSEEK_API_KEY, etc.
```

Run:

```bash
bun run apps/cli/src/index.ts
```

## Usage

```bash
# Default provider (from ~/.zode/config.json)
zode

# Specify provider and model
zode --provider anthropic --model claude-sonnet-4-20250514
zode --provider ollama --model llama3
zode --provider deepseek --model deepseek-chat

# YOLO mode — auto-approve tool calls (deny rules still apply)
zode --yolo

# Sandbox mode — restrict file access to project directory
zode --sandbox

# Resume last session
zode --resume
```

### Slash Commands

| Command  | Description        |
|----------|--------------------|
| `/help`  | Show help          |
| `/clear` | Clear conversation |
| `/undo`  | Undo last edit     |
| `/redo`  | Redo last edit     |
| `/exit`  | Exit Zode          |

## Configuration

Create `~/.zode/config.json`:

```jsonc
{
  "provider": {
    "default": "anthropic",
    "anthropic": {
      "apiKey": "sk-...",
      "model": "claude-sonnet-4-20250514"
    },
    // Any OpenAI-compatible API — set type: "openai" and provide baseUrl
    "deepseek": {
      "type": "openai",
      "apiKey": "sk-...",
      "baseUrl": "https://api.deepseek.com/v1",
      "model": "deepseek-chat"
    },
    "ollama": {
      "model": "llama3"
    }
  },
  "permissions": {
    "baseline": {},
    "rules": [
      { "tool": "exec", "level": "auto", "conditions": { "commandPrefix": ["git status", "git diff", "ls"] } },
      { "tool": "exec", "level": "deny", "conditions": { "commandBlacklist": ["rm -rf /"] } }
    ]
  }
}
```

## Architecture

```
                    ┌──────────────────────────────┐
                    │      CLI REPL / TUI           │
                    │      (apps/cli)               │
                    └──────┬───────────────┬────────┘
                           │               │
              ┌────────────▼──┐   ┌────────▼─────────┐
              │  Agent SDK    │   │     Kernel        │
              │  @zseven-w/   │   │  @zseven-w/       │
              │  agent        │   │  zode-kernel       │
              │               │   │                    │
              │ • Agent loop  │   │ • Config manager   │
              │ • Providers   │   │ • Permission engine│
              │ • Streaming   │   │ • Plugin registry  │
              │ • Context mgmt│   │ • Session manager  │
              └──────┬────────┘   └────────┬───────────┘
                     │                     │
              ┌──────▼─────────────────────▼──────────┐
              │          Plugin System                 │
              │                                        │
              │  plugin-fs · plugin-shell · plugin-git │
              │  plugin-search · plugin-web            │
              └──────┬─────────────────────┬───────────┘
                     │                     │
              ┌──────▼──────┐    ┌─────────▼──────────┐
              │  TUI Engine │    │  Native Layer       │
              │  zode-tui   │    │  zode-native (Rust) │
              │             │    │                      │
              │ Cell-based  │    │ ripgrep · tree-sitter│
              │ ANSI render │    │ diff · glob · watcher│
              └─────────────┘    └──────────────────────┘
```

<details>
<summary><strong>Workspace layout</strong></summary>

```
zode/
├── vendor/openpencil/       Git submodule — shared @zseven-w/agent SDK
├── packages/
│   ├── kernel/              Config, permissions, plugins, sessions, loaders
│   ├── native/              Rust NAPI: grep, glob, diff, highlight, watcher
│   └── tui/                 ANSI rendering engine, 15 components
├── plugins/
│   ├── plugin-fs/           read, write, edit, list, delete, move, info (7 tools)
│   ├── plugin-shell/        exec, background, status, kill (4 tools)
│   ├── plugin-search/       grep, glob, find_symbol (3 tools)
│   ├── plugin-git/          status, diff, log, commit, branch, checkout, stash, blame, pr (9 tools)
│   └── plugin-web/          search, fetch (2 tools)
└── apps/cli/                Entry point, TUI REPL, system prompt builder
```

</details>

<details>
<summary><strong>Key design decisions</strong></summary>

- **Microkernel + plugins** — tools are plugins; kernel handles lifecycle, permissions, config
- **Shared agent SDK** — `@zseven-w/agent` shared with [OpenPencil](https://github.com/ZSeven-W/openpencil) via git submodule
- **Two-phase permissions** — deny rules always win and cannot be relaxed by remote config
- **Rust native with TS fallback** — native module is optional; CLI works without compilation

</details>

## Project Instructions Compatibility

Zode reads project instructions from multiple formats for cross-tool compatibility:

| File | Scope |
|------|-------|
| `CLAUDE.md` / `AGENTS.md` | Per-directory (recursive) |
| `.claude/instructions.md` / `.agents/instructions.md` | Per-directory |
| `.claude/skills/*.md` / `.agents/skills/*.md` | Skills |
| `.claude/mcp.json` / `.agents/mcp.json` | MCP servers |
| `~/.zode/instructions.md` | Global |

> `.agents/` takes precedence over `.claude/` in the same directory.

## Development

```bash
bun install                          # Install dependencies
bun run test                         # Run all tests
bun --bun vitest run path/to/test    # Run specific test
npx tsc --noEmit                     # Type check
cd packages/native && cargo build    # Build Rust native module
```

## Contributing

Contributions welcome! Please follow [Conventional Commits](https://www.conventionalcommits.org/) — `<type>(<scope>): <subject>`.

## License

[MIT](LICENSE) &copy; ZSeven-W
