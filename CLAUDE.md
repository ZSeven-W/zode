# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
zig build              # Build the zode executable
zig build run          # Build and run (TUI mode)
zig build test         # Run all Zig tests
```

## Architecture

Zode is an AI coding CLI written in Zig with a **microkernel + plugin** architecture. The kernel manages config, permissions, plugin lifecycle, and sessions. All tools (file ops, shell, search, git, web) are plugins.

### Workspace Layout

```text
zode/
├── vendor/agent/       Agent SDK (Zig) — provider abstraction, streaming, tools
├── vendor/openpencil/  OpenPencil shared code (git submodule)
├── src/
│   ├── main.zig        Entry point — arg parsing, TUI or REPL dispatch
│   ├── cli/            CLI logic — repl, commands, config reader
│   ├── tui/            TUI app (vaxis/vxfw) — app, theme, 15 components
│   ├── kernel/         Config, permissions, plugin registry, sessions
│   └── plugins/        Built-in plugins (fs, shell, search, git, web)
└── build.zig           Build configuration
```

### Key Components

- **Agent SDK** (`vendor/agent/`) — Provider abstraction (Anthropic/OpenAI-compat/Ollama), streaming SSE, query engine, tool registry, sliding-window context
- **TUI** (`src/tui/`) — vaxis/vxfw-based terminal UI with FlexColumn layout, Catppuccin Mocha theme, background agent thread + mutex queue
- **Config** (`~/.zode/config.json`) — JSON config with provider settings (apiKey, baseUrl, model)

### Data Flow

1. **Startup** (`main.zig`): Parse args → dispatch to TUI app or text REPL
2. **TUI mode**: InputBox → on_submit → background agent thread → mutex queue → tick-based drain → MessageList
3. **Engine init**: CLI flags → `~/.zode/config.json` → `ANTHROPIC_API_KEY` env var (priority order)

## Conventions

- **Max 800 lines per file** — split when exceeded
- **One component per file**, single responsibility
- **snake_case** for Zig filenames
- **Tests:** `zig build test` — inline tests in each module

## Git Commit Convention

[Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`

**Types:** `feat`, `fix`, `refactor`, `perf`, `style`, `docs`, `test`, `chore`

**Scopes:** `kernel`, `tui`, `cli`, `agent`, `config`, `permissions`, `session`

**Rules:** Subject in English, lowercase start, no period, imperative mood. Body optional — explain **why** not what. One commit per change.

## Pre-Commit Checklist

```bash
zig build test         # All tests must pass
```
