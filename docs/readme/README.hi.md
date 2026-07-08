<p align="center">
  <img src="../../assets/brand/zode-logo.png" alt="Zode logo" width="96" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2021-f74c00?style=flat-square&logo=rust&logoColor=white" alt="Rust" />
  <img src="https://img.shields.io/badge/TUI-ratatui-7c3aed?style=flat-square" alt="ratatui" />
  <img src="https://img.shields.io/badge/license-MIT-22c55e?style=flat-square" alt="MIT License" />
</p>

<h1 align="center">Zode</h1>

<p align="center">
  <strong>टर्मिनल के लिए open-source, AI-native coding assistant.</strong><br/>
  यह आपका code पढ़ता है, commands चलाता है, files खोजता है और तेज Rust TUI से git संभालता है।
</p>

<p align="center">
  <a href="../../README.md">English</a> |
  <a href="README.zh.md">简体中文</a> |
  <a href="README.zh-tw.md">繁體中文</a> |
  <a href="README.ja.md">日本語</a> |
  <a href="README.ko.md">한국어</a> |
  <a href="README.es.md">Español</a> |
  <a href="README.fr.md">Français</a> |
  <a href="README.de.md">Deutsch</a> |
  <a href="README.pt.md">Português</a> |
  <a href="README.ru.md">Русский</a> |
  <a href="README.hi.md">हिन्दी</a> |
  <a href="README.id.md">Bahasa Indonesia</a> |
  <a href="README.th.md">ไทย</a> |
  <a href="README.tr.md">Türkçe</a> |
  <a href="README.vi.md">Tiếng Việt</a>
</p>

---

> यह localized README product overview और quick start को cover करता है। पूरे benchmark details और latest long-form notes के लिए [English README](../../README.md) source of truth है।

## Highlights

- **Multi-provider**: Anthropic, OpenAI, DeepSeek/Moonshot/OpenRouter जैसे OpenAI-compatible APIs, और local Ollama.
- **Rich tool surface**: file read/write/edit, code और content search, foreground/background shells, git, web fetch, notebooks और TODO tracking.
- **Browser control**: `browser_*` tools managed Chromium या Chrome bridge extension के जरिए आपके real Chrome profile को control कर सकते हैं।
- **Non-blocking permissions**: mutating tools allow once / always / deny approval से गुजरते हैं, और approval prompt input को block नहीं करता।
- **OS sandbox default on**: shell commands macOS `sandbox-exec` या Linux `bwrap` में चलते हैं; outbound network default रूप से denied है।
- **Full-screen TUI**: streaming Markdown, syntax highlighting, diff preview, slash-command autocomplete, prompt history, themes, settings/help overlays और 15-language UI (`/language`).
- **Multi-session tabs**: `Ctrl+T` से कई isolated conversations साथ-साथ चलाएँ और पुरानी sessions resume करें।
- **Sub-agents और workflows**: Task tool से scoped work delegate करें और `/agents`, `/workflows` से manage करें।
- **Skills, MCP और hooks**: `SKILL.md` packs load करें, MCP servers connect करें और tool events पर external scripts चलाएँ।

## Install

### One line

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer OS और CPU detect करता है, latest [release](https://github.com/ZSeven-W/zode/releases) से matching binary download करता है और `zode` को `PATH` पर रखता है।

### Manual download

[releases page](https://github.com/ZSeven-W/zode/releases) से अपने platform का archive लें:

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Unpack करें और `zode` को `PATH` में move करें, जैसे `sudo mv zode /usr/local/bin/`. Linux builds glibc हैं; macOS binaries unsigned हैं, इसलिए Gatekeeper warning पर `xattr -dr com.apple.quarantine ./zode` चलाएँ।

### Source से build

Recent stable Rust toolchain चाहिए:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

Binary `target/release/zode` में होगा। Agent runtime `vendor/agent` git submodule में है, इसलिए `--recurse-submodules` से clone करें या `git submodule update --init` चलाएँ।

## Quick Start

सबसे आसान तरीका है `zode` launch करके **`/connect`** चलाना। यह interactive model picker खोलता है और config लिखता है।

आप `~/.zode/config.json` manually भी लिख सकते हैं:

```jsonc
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
```

Common commands:

```bash
zode
zode -p "explain main.rs"
zode --no-tui
zode -c
zode -r <id>
zode --yolo
zode --no-sandbox
zode --sandbox-read-only
zode --sandbox-allow-network
zode --browser
zode --model <id>
zode --provider <name>
zode server
```

## Configuration

`providers` provider definitions की source of truth है; top-level `provider` active model बताता है। OpenAI-compatible providers आम तौर पर `baseUrl` और `dialect` use करते हैं:

```jsonc
{
  "providers": {
    "deepseek": {
      "type": "openai",
      "apiKey": "sk-...",
      "baseUrl": "https://api.deepseek.com/v1",
      "dialect": "deepseek",
      "models": {
        "deepseek-v4-pro": { "contextWindow": 1000000, "maxOutputTokens": 16384 },
        "deepseek-chat": {}
      }
    }
  },
  "provider": { "model": "deepseek-v4-pro" },
  "language": "hi"
}
```

एक provider में कई models हो सकते हैं; `/model` से live switch करें। Language `/language` से भी बदलती है।

## Server mode और SDKs

`zode server` stdin/stdout पर newline-delimited JSON-RPC server start करता है। यह editor integrations, local automation, tests और SDK clients के लिए है।

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDKs:

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## Browser Control

Zode `tools:browser` group देता है: screenshots/DOM/logs read करना, navigate/click/type करना, JavaScript चलाना और tabs manage करना। Target managed Chromium हो सकता है या [`extensions/chrome/`](../../extensions/chrome/) extension के जरिए real Chrome.

```bash
/browser
/browser status
/browser launch
/browser close
/browser pair
/browser target managed
/browser target bridge
/browser screenshot [path]
```

## Common slash commands

| Command | काम |
|---|---|
| `/help` | Commands और keybindings |
| `/connect` | Provider connect/switch |
| `/model [id]` | Active model show/set |
| `/sessions`, `/resume` | Sessions resume |
| `/browser ...` | Browser control |
| `/tasks` | Background tasks |
| `/mcp` | MCP servers manage |
| `/skills` | Skills list |
| `/agents` | Sub-agents manage |
| `/workflows` | Workflows manage |
| `/sandbox ...` | OS sandbox control |
| `/language` | UI language switch |
| `/export [path]` | Markdown export |
| `/exit` | Exit |

Full command table [English README](../../README.md#slash-commands) में है।

## Instructions, MCP और Skills

Zode instructions को `~/.zode/`, project root और current directory से पढ़ता है; हर level पर `AGENTS.md` को `CLAUDE.md` से पहले prefer करता है। Skills `.zode/skills/**/SKILL.md` में रहती हैं; MCP servers `~/.zode/mcp.json`, `.mcp.json` या `.zode/mcp.json` में।

Zode Claude, Codex, opencode, Cursor और दूसरे agents की skills, commands और MCP configurations भी discover करता है। Project के अंदर मिले external MCPs default disabled रहते हैं।

## ZSeven-W Ecosystem

Zode, ZSeven-W के AI-native development tools stack का हिस्सा है:

| Product | क्या है |
|---------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async runtime for LLM agents: multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions और optional coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework जहाँ `.op` file एक app है, और OpenPencil-style design artifacts को runnable software से जोड़ता है। |
| [`noema`](https://github.com/ZSeven-W/noema) | Coding agents के लिए local-first, non-vector memory system, जिसमें lexical recall, review queues, MCP, S3 offload और enterprise policy controls हैं। |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool for design-as-code workflows, जो prompts को live canvas पर UI में बदलता है और concurrent agent teams support करता है। |

## Benchmark

Zode benchmarks one-shot code generation, agentic read/run/edit/fix, multi-file tasks, tricky bugs, MCP/Skills/constraint following और Noema LOCOMO cover करते हैं। Methodology और complete results [English README benchmark section](../../README.md#benchmark) में हैं।

## Development

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## Contributing

Contributions welcome. [Conventional Commits](https://www.conventionalcommits.org/) format use करें: `<type>(<scope>): <subject>`.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
