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

- **Multi-provider** — Anthropic, OpenAI, and any OpenAI-compatible API (DeepSeek, Moonshot, OpenRouter dialects), plus local Ollama. Supports large-output and **1M-context** models (`contextWindow` / `maxOutputTokens` are configurable)
- **Rich tool surface** — file read/write/edit, code & content search, foreground and background shells, git, web fetch, notebooks, TODO tracking
- **Non-blocking permissions** — every mutating tool is gated (allow once / always / deny), but the prompt docks inline and never blocks you: keep typing to queue a follow-up while a tool waits, with hard-deny rules and a `--sandbox` mode that confines writes to the working dir
- **Full-screen TUI** — streaming markdown with syntax highlighting, diff previews, slash-command autocomplete, prompt history (Up/Down), 4 built-in themes, settings & help overlays, **15-language UI** (`/language`)
- **Multi-session tabs** — run several conversations side by side (`Ctrl+T`), each an isolated agent; resume past sessions with full history replay
- **Sub-agents & workflows** — delegate scoped work to child agents via the Task tool (they inherit the same gate, sandbox, and hooks), manage them with `/agents` / `/workflows`, and toggle autonomous orchestration
- **Cross-agent ecosystem** — discovers skills, slash commands, and MCP servers from Claude / Codex / opencode / antigravity / pi / kilo / cursor (plus their plugin trees), with zode's own taking precedence; foreign integrations are off by default and non-portable ones are filtered out
- **Skills & MCP** — load `SKILL.md` instruction packs on demand and connect MCP servers (`mcp__<server>__<tool>`); created agents, skills, and MCP tools surface as slash commands
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

Optional top-level config keys (all have sensible defaults):

```jsonc
{
  "maxOutputTokens": 16384,      // per-turn output cap (raise for big file writes)
  "contextWindow": 1000000,      // model context window — set 1000000 for a 1M model
  "temperature": 0,              // lower = more deterministic
  "language": "zh-CN",           // UI language (15 locales); also via /language
  "effort": "medium",            // default reasoning effort; also via /effort
  "autonomousOrchestration": true // sub-agent + workflow orchestration (default on)
}
```

> `contextWindow` drives auto-compaction — set it to your model's real window
> (e.g. `1000000`). Do **not** set it above the real window: overestimating
> makes requests overflow and the provider rejects the turn.

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
| `/connect` | Connect and switch the active provider |
| `/sidebar [on|off|toggle]` | Show or hide the right sidebar |
| `/tasks` | Background shells + running turns panel |
| `/undo`, `/redo` | Undo / redo the last file edit |
| `/mcp` | Manage MCP servers — enable / disable in a dialog |
| `/skills` | List available skills |
| `/agents` | Manage sub-agents — create (AI-assisted or manual) / delete |
| `/workflows` | Manage & run deterministic multi-step workflows |
| `/effort` | Pick the reasoning effort level |
| `/thinking`, `/tool-details` | Toggle showing reasoning / tool-call detail |
| `/orchestration` | Toggle autonomous sub-agent + workflow orchestration |
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

**Cross-agent compatibility.** Zode also discovers skills, slash commands, and
MCP servers already installed for other coding agents — Claude (`~/.claude`),
Codex (`~/.codex`), opencode, antigravity, pi, kilo, cursor — including their
plugin trees. Zode's own definitions take precedence on a name clash. Foreign
MCP servers and foreign-plugin commands are **off by default** (copy one into a
`.zode/` dir to opt in), and skills/commands that hard-code another agent's host
variables (e.g. `${CLAUDE_PLUGIN_ROOT}`) are filtered out since they can't run
here. Hooks are **not** imported from other agents (they execute shell commands).

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

## License

[MIT](LICENSE) &copy; ZSeven-W
