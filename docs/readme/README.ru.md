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
  <strong>Open-source AI-native ассистент для разработки в терминале.</strong><br/>
  Читает код, запускает команды, ищет файлы и управляет git через быструю Rust TUI.
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

> Это локализованный README с обзором продукта и быстрым стартом. [Английский README](../../README.md) остается источником истины для полных деталей benchmark и актуальных длинных примечаний.

## Основные возможности

- **Несколько провайдеров**: Anthropic, OpenAI, OpenAI-compatible API вроде DeepSeek, Moonshot и OpenRouter, а также локальный Ollama.
- **Широкий набор инструментов**: чтение/запись/редактирование файлов, поиск по коду и контенту, foreground/background shells, git, web fetch, notebooks и TODO tracking.
- **Управление браузером**: инструменты `browser_*` управляют managed Chromium или реальным профилем Chrome через расширение Chrome bridge.
- **Неблокирующие разрешения**: все mutating tools проходят allow once / always / deny, а prompt одобрения не мешает продолжать ввод.
- **OS sandbox по умолчанию**: shell-команды запускаются через `sandbox-exec` на macOS или `bwrap` на Linux; исходящая сеть по умолчанию запрещена.
- **Полноэкранная TUI**: streaming Markdown, syntax highlighting, diff preview, slash-command autocomplete, history, themes, settings/help overlays и UI на 15 языках (`/language`).
- **Multi-session tabs**: запускайте несколько изолированных диалогов через `Ctrl+T` и возобновляйте прошлые sessions.
- **Sub-agents и workflows**: делегируйте ограниченные задачи через Task tool и управляйте ими через `/agents` и `/workflows`.
- **Skills, MCP и hooks**: загружайте пакеты `SKILL.md`, подключайте MCP servers и запускайте внешние scripts на событиях tools.

## Установка

### В одну строку

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer определяет OS и CPU, скачивает подходящий binary из последнего [release](https://github.com/ZSeven-W/zode/releases) и кладет `zode` в `PATH`.

### Ручная загрузка

Скачайте archive для своей платформы со [страницы releases](https://github.com/ZSeven-W/zode/releases):

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Распакуйте и переместите `zode` в `PATH`, например `sudo mv zode /usr/local/bin/`. Linux builds используют glibc; macOS binaries не подписаны. Если Gatekeeper предупреждает, выполните `xattr -dr com.apple.quarantine ./zode`.

### Из исходников

Нужна свежая stable Rust toolchain:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

Binary появится в `target/release/zode`. Agent runtime находится в git submodule `vendor/agent`; clone делайте с `--recurse-submodules` или выполните `git submodule update --init`.

## Быстрый старт

Самый простой путь: запустить `zode` и выполнить **`/connect`**. Интерактивный model picker запишет конфигурацию.

Можно также вручную создать `~/.zode/config.json`:

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

Частые команды запуска:

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

## Конфигурация

`providers` — источник истины для providers; top-level `provider` указывает active model. OpenAI-compatible providers обычно используют `baseUrl` и `dialect`:

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
  "language": "ru"
}
```

Один provider может содержать несколько models; переключение выполняется через `/model`. Язык также меняется через `/language`.

## Server mode и SDK

`zode server` запускает newline-delimited JSON-RPC server на stdin/stdout для editor integrations, local automation, tests и SDK clients.

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDK:

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## Управление браузером

Zode включает группу `tools:browser` для чтения screenshots/DOM/logs, navigation/click/type, выполнения JavaScript и управления tabs. Target — managed Chromium или реальный Chrome через extension в [`extensions/chrome/`](../../extensions/chrome/).

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

## Частые slash commands

| Command | Действие |
|---|---|
| `/help` | Commands и keybindings |
| `/connect` | Подключить и переключить provider |
| `/model [id]` | Показать или задать active model |
| `/sessions`, `/resume` | Возобновить sessions |
| `/browser ...` | Browser control |
| `/tasks` | Background tasks |
| `/mcp` | Управление MCP servers |
| `/skills` | Список skills |
| `/agents` | Управление sub-agents |
| `/workflows` | Управление workflows |
| `/sandbox ...` | Управление OS sandbox |
| `/language` | Смена языка UI |
| `/export [path]` | Экспорт Markdown |
| `/exit` | Выход |

Полная таблица есть в [английском README](../../README.md#slash-commands).

## Instructions, MCP и Skills

Zode читает instructions из `~/.zode/`, project root и current directory; в каждом каталоге предпочитает `AGENTS.md`, затем `CLAUDE.md`. Skills находятся в `.zode/skills/**/SKILL.md`; MCP servers — в `~/.zode/mcp.json`, `.mcp.json` или `.zode/mcp.json`.

Zode также находит skills, commands и MCP configurations от Claude, Codex, opencode, Cursor и других agents. Внешние MCP, найденные внутри проекта, отключены по умолчанию.

## Экосистема ZSeven-W

Zode входит в стек ZSeven-W для AI-native development tools:

| Продукт | Описание |
|---------|----------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async runtime для LLM agents: multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions и optional coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework, где `.op` file является app и связывает OpenPencil-style design artifacts с runnable software. |
| [`noema`](https://github.com/ZSeven-W/noema) | Local-first, non-vector memory system для coding agents с lexical recall, review queues, MCP, S3 offload и enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool для design-as-code workflows, превращающий prompts в UI на live canvas и поддерживающий concurrent agent teams. |

## Benchmark

Benchmarks Zode покрывают one-shot code generation, agentic read/run/edit/fix, multi-file tasks, tricky bugs, MCP/Skills/constraint following и Noema LOCOMO. Методология и полные результаты описаны в [Benchmark section английского README](../../README.md#benchmark).

## Разработка

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## Вклад

Contributions welcome. Используйте [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
