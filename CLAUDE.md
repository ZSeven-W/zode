# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build --workspace                 # Build all crates
cargo run -p zode                       # Run the TUI
cargo run -p zode -- -p "<prompt>"      # Headless single turn (stream to stdout)
cargo run -p zode -- --no-tui           # Plain readline REPL
cargo test --workspace                  # Run all tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                        # Licenses / advisories / bans
```

> Clone with `--recurse-submodules` — the agent runtime is the `vendor/agent`
> submodule. After pulling, `git submodule update --init` if it's missing.

## Architecture

Zode is an AI coding CLI in Rust. It consumes the `agent` runtime
(`vendor/agent` submodule, shared with OpenPencil) and adds the terminal
product layer. It is a Cargo workspace of three crates:

```text
zode/
├── vendor/agent/          agent-rs submodule (agent + agent-tools-code crates)
└── crates/
    ├── zode/              bin: arg parsing, headless modes (-p / --no-tui), dispatch
    ├── zode-core/         UI-agnostic: config, engine, tools, commands, history,
    │                      sandbox, skills, mcp, cost, instructions, approvals
    └── zode-tui/          ratatui chrome: app loop, chat, dialogs, themes, tabs
```

Dependency direction: `zode` → `zode-tui` → `zode-core` → `vendor/agent`.
`zode-core` never depends on the TUI or the binary.

### Key design points

- **The agent runtime is upstream.** Don't fork it; feed gaps back to agent-rs
  (e.g. the optional `TaskAgentConfig` cwd/file_cache/hooks fields).
- **Interactive permission lives in a tool decorator** (`PermissionGatedTool`
  + `ApprovalGate`), not in the QueryLoop — agent-rs 0.1.0 does not pump the
  approval queue, so the `PermissionManager` carries only hard-deny rules and
  runs in Bypass; interactive allow/always/deny happens in the gate.
- **MCP tools** are wrapped in `ZodeMcpTool` (agent-rs has no Tool adapter),
  named `mcp__<server>__<tool>`.
- **Skills** are loaded into a registry and surfaced via a `SkillTool` plus a
  system-prompt index.
- **Multi-session tabs**: each tab is an isolated `ZodeEngine` (own message
  store, cost tracker, turn state) built from an `EngineTemplate`, sharing one
  approval channel. Sub-agents (Task tool) inherit the parent's final gated +
  sandboxed tool registry plus its permissions/hooks/cwd/file_cache.

### Data flow

1. **Startup** (`crates/zode/src/main.rs`): parse args → load config → build
   provider/gate/sandbox → assemble engine → dispatch to TUI / `-p` / `--no-tui`.
2. **TUI** (`crates/zode-tui/src/app.rs`): `tokio::select!` over terminal input,
   agent events (tagged with `tab_id` + `turn_id`), approval requests, and a tick.
3. **Engine** (`crates/zode-core/src/engine.rs`): rebuilds a `QueryLoop` per turn
   from shared `Arc` state; tools are wrapped (sandbox → gate → ToolSearch).

## Conventions

- **Max 800 lines per file** — split when exceeded.
- **One component per file**, single responsibility.
- **kebab-case** for `.rs` filenames.
- **English comments** in source (`.rs`/`.toml`); Chinese is fine in spec/plan
  markdown and test fixtures.
- **Tests:** inline `#[cfg(test)]` modules; env-mutating tests use
  `#[serial_test::serial]`.

## Git Commit Convention

[Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`

**Types:** `feat`, `fix`, `refactor`, `perf`, `style`, `docs`, `test`, `chore`, `build`, `ci`

**Scopes:** `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`

**Rules:** Subject in English, lowercase start, no period, imperative mood. Body optional — explain **why** not what.

## Pre-Commit Checklist

```bash
cargo fmt --all -- --check && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo test --workspace
```
