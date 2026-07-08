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
  <strong>Open-source, AI-native Coding-Assistent für dein Terminal.</strong><br/>
  Liest Code, führt Befehle aus, durchsucht Dateien und verwaltet git in einer schnellen Rust-TUI.
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

> Dieses lokalisierte README deckt Überblick und Schnellstart ab. Das [englische README](../../README.md) bleibt die Referenz für vollständige Benchmark-Details und aktuelle Langform-Hinweise.

## Highlights

- **Multi-provider**: Anthropic, OpenAI, OpenAI-kompatible APIs wie DeepSeek, Moonshot und OpenRouter sowie lokales Ollama.
- **Breite Werkzeugfläche**: Dateien lesen/schreiben/bearbeiten, Code- und Inhaltssuche, foreground/background shells, git, web fetch, notebooks und TODO tracking.
- **Browser-Steuerung**: `browser_*`-Tools steuern managed Chromium oder dein echtes Chrome-Profil über die Chrome-bridge-Erweiterung.
- **Nicht blockierende Berechtigungen**: Mutierende Tools laufen über allow once / always / deny, mit inline Approval prompt.
- **OS-Sandbox standardmäßig aktiv**: Shell-Befehle laufen unter macOS `sandbox-exec` oder Linux `bwrap`, ausgehendes Netzwerk ist standardmäßig gesperrt.
- **Vollbild-TUI**: Streaming Markdown, Syntax Highlighting, Diff Preview, Slash-command Autocomplete, Verlauf, Themes, Settings/Help overlays und UI in 15 Sprachen (`/language`).
- **Multi-session Tabs**: Mehrere isolierte Gespräche mit `Ctrl+T` parallel ausführen und frühere Sessions wiederaufnehmen.
- **Sub-agents und workflows**: Abgegrenzte Arbeit mit dem Task tool delegieren und über `/agents` und `/workflows` verwalten.
- **Skills, MCP und hooks**: `SKILL.md`-Pakete laden, MCP server verbinden und externe Skripte bei Tool-Events ausführen.

## Installation

### Eine Zeile

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Der Installer erkennt OS und CPU, lädt das passende Binary vom neuesten [Release](https://github.com/ZSeven-W/zode/releases) und legt `zode` in deinem `PATH` ab.

### Manueller Download

Lade das Archiv für deine Plattform von der [Releases-Seite](https://github.com/ZSeven-W/zode/releases):

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Entpacke und verschiebe `zode` in deinen `PATH`, zum Beispiel `sudo mv zode /usr/local/bin/`. Linux builds nutzen glibc; macOS binaries sind unsigniert. Falls Gatekeeper warnt, nutze `xattr -dr com.apple.quarantine ./zode`.

### Aus dem Quellcode

Benötigt eine aktuelle stabile Rust toolchain:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

Das Binary liegt unter `target/release/zode`. Die agent runtime ist das git submodule `vendor/agent`; clone mit `--recurse-submodules` oder führe `git submodule update --init` aus.

## Schnellstart

Starte `zode` und führe **`/connect`** aus. Der interaktive model picker schreibt die Konfiguration für dich.

Oder schreibe `~/.zode/config.json` manuell:

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

Häufige Startbefehle:

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

## Konfiguration

`providers` ist die Quelle für Provider; das top-level `provider` wählt das aktive Modell. OpenAI-compatible Provider nutzen meist `baseUrl` und `dialect`:

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
  "language": "de"
}
```

Ein Provider kann mehrere Modelle enthalten; mit `/model` wechselst du live. Die Sprache lässt sich auch mit `/language` ändern.

## Server mode und SDKs

`zode server` startet einen newline-delimited JSON-RPC server auf stdin/stdout, gedacht für Editor-Integrationen, lokale Automatisierung, Tests und SDK clients.

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDKs:

| SDK | Verzeichnis | Lokaler Test |
|-----|-------------|--------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## Browser-Steuerung

Zode enthält die Gruppe `tools:browser` für Screenshot/DOM/log reads, Navigation, Klicks, Eingaben, JavaScript und Tab management. Ziel ist managed Chromium oder dein Chrome über die Erweiterung in [`extensions/chrome/`](../../extensions/chrome/).

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

## Häufige Slash Commands

| Command | Aktion |
|---|---|
| `/help` | Commands und keybindings |
| `/connect` | Provider verbinden und wechseln |
| `/model [id]` | Aktives Modell anzeigen/setzen |
| `/sessions`, `/resume` | Sessions wiederaufnehmen |
| `/browser ...` | Browser control |
| `/tasks` | Background tasks |
| `/mcp` | MCP server verwalten |
| `/skills` | Skills auflisten |
| `/agents` | Sub-agents verwalten |
| `/workflows` | Workflows verwalten |
| `/sandbox ...` | OS sandbox steuern |
| `/language` | UI-Sprache wechseln |
| `/export [path]` | Markdown exportieren |
| `/exit` | Beenden |

Die vollständige Tabelle steht im [englischen README](../../README.md#slash-commands).

## Instructions, MCP und Skills

Zode liest instructions aus `~/.zode/`, project root und current directory; pro Ebene wird `AGENTS.md` vor `CLAUDE.md` bevorzugt. Skills liegen in `.zode/skills/**/SKILL.md`; MCP server in `~/.zode/mcp.json`, `.mcp.json` oder `.zode/mcp.json`.

Zode entdeckt auch Skills, Commands und MCP-Konfigurationen von Claude, Codex, opencode, Cursor und anderen Agents. Externe MCPs aus einem Projekt sind standardmäßig deaktiviert.

## ZSeven-W Ökosystem

Zode ist Teil des ZSeven-W stacks für AI-native development tools:

| Produkt | Beschreibung |
|---------|--------------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async runtime für LLM agents mit multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions und optionalen coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework, in dem eine `.op` file eine app ist und OpenPencil-style design artifacts mit ausführbarer Software verbindet. |
| [`noema`](https://github.com/ZSeven-W/noema) | Local-first, non-vector memory system für coding agents mit lexical recall, review queues, MCP, S3 offload und enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool für design-as-code workflows, das prompts direkt auf einem live canvas in UI verwandelt und concurrent agent teams unterstützt. |

## Benchmark

Zodes Benchmarks decken one-shot code generation, agentic read/run/edit/fix, multi-file tasks, schwierige Bugs, MCP/Skills/constraint following und Noema LOCOMO ab. Methodik und Ergebnisse stehen im [Benchmark-Abschnitt des englischen README](../../README.md#benchmark).

## Entwicklung

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## Beitragen

Beiträge sind willkommen. Nutze [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`.

## Lizenz

[MIT](../../LICENSE) &copy; ZSeven-W
