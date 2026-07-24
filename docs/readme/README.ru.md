<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="../../assets/logo.png">
    <img src="../../assets/logo-light.png" alt="Zode logo" width="96" />
  </picture>
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

> Это локализованный README с обзором продукта и быстрым стартом. [Английский README](../../README.md) остаётся источником истины для полных деталей benchmark и актуальных длинных примечаний.

## Основные возможности

- **Несколько провайдеров**: Anthropic, OpenAI и любой OpenAI-compatible API (диалекты DeepSeek, Moonshot, OpenRouter), а также локальный Ollama. Поддерживаются модели с большим выводом и **контекстом на 1M** (`contextWindow` / `maxOutputTokens` настраиваются).
- **Широкий набор инструментов**: чтение/запись/редактирование файлов, поиск по коду и контенту, foreground/background shells, git, web fetch, notebooks и TODO tracking.
- **Управление браузером**: встроенные инструменты `browser_*` управляют managed Chromium или вашим реальным профилем Chrome через расширение Chrome bridge — навигация, click/type, инспекция DOM, screenshots, чтение console/network logs и группировка открытых Zode вкладок.
- **Неблокирующие разрешения**: каждый mutating tool проходит согласование (allow once / always / deny), но prompt пристыкован inline и не блокирует вас — можно продолжать печатать, пока инструмент ждёт, а hard-deny правила действуют всегда.
- **OS sandbox, включён по умолчанию**: shell-команды выполняются под sandbox-exec (macOS) / bwrap (Linux) в режиме `read-only` или `workspace-write`, при этом **исходящая сеть по умолчанию запрещена**. Переключается на лету через `/sandbox`; модель может запросить escape для одной команды (`dangerouslyDisableSandbox`), который **авторизуете вы** в prompt.
- **Полноэкранная TUI**: streaming Markdown с подсветкой синтаксиса, diff preview, автодополнение slash-команд, история ввода (Up/Down), 11 встроенных тем, overlays настроек и помощи, устойчивая правая боковая панель и **UI на 15 языках** (`/language`).
- **Долговечные, V1-совместимые sessions**: сохраняется существующий контракт transcript `<id>.jsonl`, к которому добавляются journals, checkpoints, rewind, fork и изолированные Git worktrees как sidecar-данные.
- **Поверхности автоматизации**: стабильный JSON/JSONL вывод в headless, точное указание session, фильтры инструментов, детерминированные exit codes, ACP через stdio и локальный operations dashboard.
- **Multi-session tabs**: запускайте несколько диалогов рядом (`Ctrl+T`), каждый — изолированный agent; возобновляйте прошлые sessions с полным воспроизведением истории.
- **Sub-agents, команды и workflows**: делегируйте разовую работу через Task, нанимайте постоянных внутренних или внешних CLI-teammates, координируйте их через общую board и file claims, управляйте через `/agents`, `/team` и `/workflows`.
- **Переносимая локальная конфигурация**: читает прямые skills и MCP-конфигурацию из Claude Code, Codex, Cursor, opencode и Gemini, при этом никогда не импортируя их установленные plugin trees и кэши.
- **Skills и MCP**: загружайте пакеты инструкций `SKILL.md` по требованию и подключайте MCP servers (`mcp__<server>__<tool>`); созданные agents, skills и MCP tools появляются как slash-команды.
- **Hooks**: запускайте внешние скрипты на событиях инструментов (например, блокировать опасные команды, линтить после правок).
- **Инструкции трёх уровней**: глобальный (`~/.zode/`) → корень проекта → cwd (`AGENTS.md` / `CLAUDE.md`).

## Установка

### В одну строку (готовые бинарники)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer автоматически определяет вашу OS + CPU, скачивает подходящий binary из последнего [release](https://github.com/ZSeven-W/zode/releases) и кладёт `zode` в `PATH`. Можно закрепить версию или изменить расположение:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

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

Распакуйте и переместите `zode` в `PATH` (`sudo mv zode /usr/local/bin/`). Linux builds используют glibc; macOS binaries не подписаны (`xattr -dr com.apple.quarantine ./zode`, если Gatekeeper предупреждает).

### Из исходников

Требуется Rust 1.88 или новее:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binary в target/release/zode
```

> Agent runtime находится в git submodule `vendor/agent` — всегда клонируйте с `--recurse-submodules` (или выполните `git submodule update --init`).

## Быстрый старт

Самый простой путь — запустить `zode` и выполнить **`/connect`**: интерактивный picker на данных models.dev запишет конфигурацию за вас.

Чтобы написать `~/.zode/config.json` вручную: **`providers`** — источник истины (одна запись на провайдера с общими учётными данными, содержащая одну или несколько **models**), а top-level **`provider`** фиксирует *активную* модель:

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // wire protocol: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // активная модель
}
```

OpenAI-compatible провайдеры (DeepSeek, Moonshot, OpenRouter, …) добавляют `baseUrl` + `dialect`, а настройки конкретной модели живут в её записи:

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

Одна запись провайдера может содержать несколько моделей — переключайтесь между ними на лету через `/model`.

Затем запустите:

```bash
zode                       # полноэкранная TUI
zode -p "explain main.rs"  # headless: один prompt, поток в stdout, выход
zode --no-tui              # обычный readline REPL
zode -c                    # продолжить последнюю session
zode -r <id>               # возобновить session по префиксу id
zode --yolo                # обойти prompt согласования (deny-правила всё равно действуют)
zode --no-sandbox          # отключить OS sandbox (он ВКЛЮЧЁН по умолчанию)
zode --sandbox-read-only   # sandbox в режиме read-only (запретить любую запись)
zode --sandbox-allow-network  # разрешить исходящую сеть внутри sandbox
zode --browser             # принудительно включить встроенные browser tools для этого запуска
zode --no-browser          # отключить встроенные browser tools для этого запуска
zode --model <id>          # переопределить модель
zode --provider <name>     # выбрать именованный провайдер из config.providers
zode server                # режим JSON-RPC app-server через stdio
zode acp                   # Agent Client Protocol agent через stdio
zode dashboard             # локальный обзор sessions/checkpoints/worktrees
```

Можно также указать любой провайдер без правки конфигурации, экспортировав соответствующий ключ (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); для Ollama `baseUrl` берётся из окружения, если не задан.

## Ручная регистрация внешних CLI-teammates

Zode может использовать установленный сторонний agent CLI как разовый Task worker или как постоянного/stateless teammate. Регистрация сделана намеренно ручной: установка CLI или помещение его в `PATH` **не** открывает его модели. Добавьте profile в `externalAgents.agents`, затем запустите Zode в проекте. Либо выполните `/external-agents`, чтобы посмотреть поддерживаемые CLI, сейчас находящиеся в `PATH`, и затем `/external-agents discover`, чтобы явно добавить каждый обнаруженный preset в глобальную конфигурацию. Эта команда запускается пользователем; при старте Zode внешние CLI не сканируются и не регистрируются автоматически.

| Profile | Executable | Task worker | Режим team | Sandbox внешней CLI |
|---|---|---:|---:|---|
| `claude-code` | `claude` | да | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | да | persistent | workspace-write |
| `opencode` | `opencode` | да | stateless | unknown |
| `cline` | `cline` | да | stateless | unrestricted |
| `antigravity` | `agy` | да | stateless | unknown |
| `cursor` | `cursor-agent` | да | persistent | unrestricted |
| `kiro` | `kiro-cli` | да | stateless | unrestricted |
| `pi` | `pi` | да | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | да | persistent | unrestricted |

Каждый зарегистрированный profile может войти в team. Resumable profiles сохраняют session ID и историю CLI между назначениями; остальные CLI — stateless teammates, запускающие новый process для каждого назначения. Presets используют документированные headless-интерфейсы [Cline](https://docs.cline.bot/usage/cli-overview), [Antigravity](https://antigravity.google/docs/cli-best-practices), [Cursor](https://cursor.com/docs/cli/headless), [Kiro](https://kiro.dev/docs/cli/headless/), [Pi](https://pi.dev/docs/latest) и [Grok Build](https://docs.x.ai/build/cli/headless-scripting) от xAI. Другие инструменты, включая альтернативные Grok CLI, можно подключить через custom profile.

### Добавление CLI profile вручную

Разместите `externalAgents` в `~/.zode/config.json` для всех проектов или в `<project>/.zode/config.json` для одного проекта. Пустой объект явно включает известный preset и разрешает его executable по санитизированному `PATH`:

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

Добавляйте только те profiles, которые действительно намерены открыть. Голое `command` вроде `cline` разрешается по `PATH`; пути вроде `./tools/my-agent` или `/opt/agents/my-agent` тоже принимаются. Известные presets учитывают `enabled`, `command`, `extraArgs`, `envAllow` и `trusted`; `extraArgs` дописывается к preset-вызову Zode.

CLI-процессы стартуют с очищенным окружением, содержащим только `PATH`, `HOME` и `TERM` (плюс необходимые Windows-переменные), поэтому явно добавляйте API keys и другие нужные переменные в `envAllow`. Существующее login-состояние под `HOME` продолжает работать. Запись проекта с тем же именем profile заменяет глобальную запись целиком, так что повторите каждое переопределение, которое проекту всё ещё нужно.

Custom profile объявляет полный вызов и протокол:

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

`promptTransport` — это `stdin`, `argv` или `file`; `argv` требует отдельный аргумент `{prompt}`, а `file` — `{prompt_file}`. `output` — это `text`, общий `jsonl`, `jsonl-claude` или `jsonl-codex`. Общие JSONL-profiles используют RFC 6901 pointers `textSource` и `sessionIdSource`, чтобы извлекать потоковый текст и resumable session ID из любого события. `resumeArgs` должен содержать отдельный токен `{session_id}` и дописывается на последующих turns; `resumeFlag` сохранён как сокращённая форма `<flag> <session-id>`.

Если CLI принимает session ID, выбранный вызывающей стороной, `newSessionArgs` может содержать отдельный токен `{session_id}`. Zode генерирует UUID, дописывает развёрнутые аргументы при первом запуске и использует `resumeArgs` в последующих назначениях. Это также делает plain-text CLI resumable без парсинга ID из вывода.

Так любая headless CLI становится Task worker или stateless teammate. Чтобы сохранять контекст диалога между назначениями в team, она должна дополнительно предоставлять session ID (или принимать его через `newSessionArgs`) плюс non-interactive resume-вызов.

`effectiveSandbox` принимает `none`, `readOnly`, `workspaceWrite`, `unrestricted` или `unknown` и отображается в trust prompt.

### Наём и работа с teammate

Просите leader обычным языком; `team_hire` и `team_send` — это tools модели, а не slash-команды:

```text
Найми внешний agent `codex` как teammate с именем `implementer`.
Его роль — реализовать refactor аутентификации и запустить целевые тесты.

Отправь `implementer` задачу сейчас и claim `src/auth/` для него перед редактированием.

Попроси `implementer` устранить замечания review, сохранив контекст его session.
```

Первый hire показывает разрешённый executable и аргументы, рабочий каталог и effective sandbox этой CLI. Одобрение делегирует работу этому процессу в текущем проекте: Zode gate'ит запуск процесса, но **не** gate'ит каждое редактирование файла или shell-команду, выполняемые внешней CLI. Trust grants действуют для текущей session Zode; постоянный roster восстанавливается из `<cwd>/.zode/team/`, но внешнему teammate нужно снова получить trust после перезапуска или смены executable.

В non-interactive/bypass запусках (включая `--yolo`) Zode не может показать trust prompt и fail closed. Устанавливайте `externalAgents.agents.<profile>.trusted` в `true` только тогда, когда осознанно хотите запускать этот profile без prompt.

Используйте `/team`, чтобы осмотреть roster и board после найма:

```text
/team                         # панель roster + board
/team status                  # текстовый roster
/team board                   # общая цель, заметки, назначения и claims
/team dismiss implementer     # удалить teammate
```

## Автоматизация, долговечные Sessions и Operations

### Структурированные headless-запуски

`-p`, `--prompt-file` и `--prompt-json` используют один headless-движок. `json` выдаёт один финальный result-объект; `stream-json` выдаёт по одному JSON-объекту `zode.run-event.v1` на строку. Структурированные режимы резервируют stdout для машиночитаемого вывода и используют стабильные exit codes: `0` успех, `10` ошибка провайдера, `11` доступ запрещён, `12` достигнут лимит turn/limit, `13` прервано (Ctrl-C), `14` частичный результат, `15` ошибка указания session.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# Точные ID не сопоставляются по префиксу. Fork никогда не изменяет исходную session.
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Deny-паттерны инструментов побеждают allow-паттерны и наследуются Task sub-agents. `--permission-mode` принимает `default`, `dont-ask`, `accept-edits` и `bypass`; `--yolo` остаётся сокращением для bypass, при этом hard deny-правила всё равно действуют.

### V1-совместимые sessions, checkpoints и worktrees

Transcript остаётся оригинальным V1-файлом по адресу `~/.zode/sessions/<id>.jsonl`. Это **единственная** копия transcript, поэтому старые клиенты Zode могут продолжать читать и писать его. Новые метаданные аддитивны и живут в `~/.zode/sessions/<id>/` (`meta.json`, journal, checkpoints и snapshots). Новый формат session или миграция transcript не требуются.

```bash
zode session list
zode session list --json
zode session show <id>                         # метаданные + ID checkpoints
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # предпросмотр с учётом конфликтов
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

Checkpoint снимается перед mutating turn. Rewind восстанавливает содержимое отслеживаемых файлов и префикс transcript, сообщает о конфликтах вместо перезаписи более новых изменений и записывает новую логическую ветку journal, а не удаляет историю. Worktree forks можно явно применить обратно, когда эксперимент готов.

### Правила разрешений и профили sandbox

Правила могут жить в `permissions.rules` внутри `config.json` или в отдельном JSON-файле, передаваемом через `--rules`. Field matcher использует RFC 6901 JSON pointer; deny приоритетнее ask, а ask приоритетнее allow. Отдельный файл должен быть либо массивом правил, либо `{ "rules": [...] }`; он не оборачивается в top-level объект `permissions`.

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

Встроенные profiles: `read-only`, `workspace`, `workspace-network` и `unconfined`. Profiles, определённые в конфигурации, используют те же поля sandbox, что показаны выше.

### Плагины и статические marketplaces

Управляемый plugin может добавлять skills, commands, agents, hooks, MCP servers, LSP servers и sandboxed JavaScript UI renderers. Zode принимает `plugin.json`, `.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.grok-plugin/plugin.json` и `.claude-plugin/plugin.json`. Поддерживаются массивы путей компонентов Codex и Claude Code, а `defaultEnabled` из Claude Code учитывается при первой установке. Host-only компоненты — Codex apps/connectors и Claude Code themes, monitors или output styles — игнорируются; plugin, состоящий только из app, отклоняется, так как в нём нет Zode-совместимого компонента. Установки — это неизменяемые snapshots с provenance и SHA-256 tree hash. Исполняемый контент plugin никогда не активируется без явного флага `--trust`.

#### Быстрый старт JavaScript UI plugin

Минимальный UI plugin содержит manifest и один JavaScript-файл:

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

Установите локальный каталог или GitHub-репозиторий/подкаталог, затем перезапустите запущенный процесс Zode, чтобы он загрузил новый snapshot:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Используйте `zode plugin update my-plugin` после изменения источника. `--trust` обязателен, потому что JavaScript, hooks, MCP servers и объявленный сетевой доступ — это исполняемые возможности. Install и update печатают объявленный grant разрешений plugin (сетевые хосты, env vars, context scopes). Update, чей manifest запрашивает *более широкие* разрешения, чем установленный snapshot, отклоняется, пока вы не повторите его с `--trust` — подвижный Git-источник не может молча расширить собственный grant.

#### UI render API

UI plugins могут добавлять декларативные строки непосредственно над версией в sidebar — не более шести строк суммарно, общих для всех plugins в порядке загрузки. Объявите JavaScript-entrypoint в manifest:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

Зарегистрируйте синхронный renderer через `zode.ui.sidebar`. Context — это read-only JSON-snapshot с полями terminal, session, model, status, token и context-window. Результат отрисовывает Zode; скрипты не получают доступа к файловой системе, сети, терминалу или Ratatui-мосту.

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

Поддерживаемые tones: `default`, `muted`, `accent`, `success`, `warning` и `danger`; spans также принимают `bold` и `italic`. Renderer должен быть синхронным. Каждый скрипт ограничен 256 KiB, 8 MiB JS-памяти и 25 ms на вычисление, а renderers переоцениваются не чаще одного раза в 250 ms (между вычислениями переиспользуется кэшированный вывод). Вывод sidebar ограничен 6 строками на renderer (6 суммарно для всех plugins), каждая строка — 16 spans и 2 048 байтами текста. Управляющие символы санируются хостом.

Status bar тоже расширяем. Он остаётся одной строкой, когда ни один plugin не возвращает контент, и динамически растёт до двух строк, когда синхронный renderer `zode.ui.statusLine` возвращает spans. Zode держит свой core status и индикаторы безопасности в первой строке; вывод plugin компонуется во второй.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Render context и разрешения

Каждый renderer получает следующие базовые поля без запроса дополнительного context-разрешения:

| Поле | Форма и значение |
| --- | --- |
| `ctx.apiVersion` | Версия Context API; сейчас `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | `{ width, height }` в ячейках терминала. |
| `ctx.session` | `{ id, title, cwd, busy }` для активной задачи. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` содержит `{ enabled, readOnly, network }`. |
| `ctx.tokens` | Счётчики токенов `{ input, output }`. |
| `ctx.context` | `{ used, window, usedPercent }`; процент может быть `null`. |
| `ctx.data` | Результаты только тех data sources, что зарегистрированы этим plugin. |

Более богатые секции опущены, пока plugin не запросит соответствующий scope в `permissions.context`:

| Scope | Открываемое поле | Форма и лимиты |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` начинается с единицы. |
| `workspace` | `ctx.workspace.modifiedFiles` | До 50 Git-записей `{ path, added, removed }`. |
| `tools` | `ctx.tools.available` | Отсортированные имена инструментов, включённых для активной задачи. |
| `tools` | `ctx.tools.active` | Имена инструментов, выполняющихся сейчас. |
| `tools` | `ctx.tools.recent` | До 20 записей `{ name, status, durationMs }`. |
| `tasks` | `ctx.tasks.todoStatuses` | Только строки статусов todo, без текста todo. |
| `tasks` | `ctx.tasks.subagents` | Записи `{ type, status }`, без prompts и transcripts. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, без текста цели. |
| `services` | `ctx.services.mcp` | Записи `{ name, connected }`. |
| `services` | `ctx.services.lsp` | Записи `{ language, running }`. |

Например:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` — это observation API: он сообщает renderer, какие инструменты существуют и какие выполняются или выполнялись. UI plugins не могут вызывать инструмент. Входы инструментов, их выходы, prompts, содержимое transcript, текст todo/goal, значения окружения и учётные данные не включаются, а этот API не может обойти систему согласования Zode.

#### Фоновые HTTP-данные

UI plugins также могут регистрировать фоновые HTTP data sources. Доступ к сети и секретам должен быть объявлен в manifest:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

Запрос декларативен и выполняется вне render-пути. Секретные переменные окружения собираются в заголовки самим Zode и никогда не открываются JavaScript:

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

`zode.data.define(key, config)` принимает key длиной 1–64 символа из букв, цифр, подчёркиваний или дефисов. `request` поддерживает `url`, `method`, `headers`, необязательный JSON `body` и `timeoutMs`. По умолчанию — `GET`, 3-секундный timeout и 60-секундный refresh. Принимаются только HTTPS `GET` и `POST`. Литеральные заголовки — строки; секретный заголовок использует `{ "env": "NAME", "prefix": "Bearer " }`. Переменная окружения должна также присутствовать в `permissions.env`, читается только Rust при сборке запроса и никогда не возвращается в JavaScript.

Zode отключает redirects и proxies, валидирует и пинует публичные DNS-адреса, отклоняет localhost/private-сети, ограничивает ответы 256 KiB, зажимает request timeouts в диапазон 500 ms–10 секунд и зажимает refresh-интервалы в диапазон 10 секунд–1 час. Wildcard вроде `*.example.com` совпадает с поддоменами, но не с голым хостом `example.com`.

Каждый plugin видит только свои данные. `ctx.data.<key>` содержит `{ ok, status, data, updatedAt }` либо `{ ok: false, error, updatedAt }`. JSON-ответы становятся объектами/массивами; не-JSON-ответы становятся строками. Ошибочный HTTP-статус всё равно включает `status` и `data`, с `ok: false`.

Запускайте Zode с нужным секретом в окружении при использовании приватного quota или coding-plan API:

```bash
CODING_PLAN_TOKEN=... zode
```

[Полный работающий пример](../../examples/plugins/zode-ui-demo/) показывает активность model/context/tool в sidebar и status line и использует `zode.data.define` для публичного quota GitHub API.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# Marketplace — это локальный/Git статический индекс, а не сервис на хостинге Zode.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # уточнить источник при необходимости
zode plugin marketplace update
```

### ACP, dashboard, telemetry и TUI regression-тесты

`zode acp` реализует ACP initialize/new/load/fork/prompt/cancel через stdio, стримит обновления message/thought/tool, запрашивает разрешения через клиента и принимает предоставленные клиентом stdio-, HTTP- и SSE-MCP-servers. Данные session используют тот же V1-совместимый store, что TUI и headless CLI.

```bash
zode acp
zode dashboard
zode dashboard --json
```

Экспорт OTLP выключен по умолчанию и требует явного opt-in. Экспортируются только не содержащие контента атрибуты lifecycle/tool-name/status/usage: prompts, сгенерированный текст, входы/выходы инструментов, пути к файлам и сообщения об ошибках никогда не отправляются.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

Для сценариев regression реального терминала в workspace есть PTY + VT100 harness, записывающий raw diagnostics и snapshots виртуального экрана:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` управляет реальным терминалом упорядоченными waits, вводом клавиш, resizes и snapshots (нотация клавиш поддерживает `<Enter>`, `<Esc>`, `<Tab>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>` и `<C-l>`):

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

Эта локальная/открытая реализация намеренно не включает специфичные для xAI аккаунты, биллинг или облачный marketplace-сервис, управляемый Zode.

## Watchdog фоновых turn, /loop и /schedule

Turns `/loop` и `/schedule`, принадлежащие планировщику, выполняются под внутрипроцессным liveness-watchdog. Активность провайдера, инструмента и вложенного agent обновляет общий source-side heartbeat, тогда как `maxRuntimeSecs` остаётся абсолютным лимитом. При любом timeout Zode запрашивает кооперативную отмену, ждёт `abortGraceSecs` и жёстко останавливает локальную turn-задачу, если она всё ещё не завершилась. Остановки задачи недостаточно для освобождения слота планировщика: Zode также ждёт, пока каждый отслеживаемый worker провайдера, инструмента, hook, subprocess-reader и вложенного agent придёт в состояние покоя. Если этот второй рубеж не достигнут за пять секунд, tab/store помещается в карантин, job отключается, а его live-attempt lease остаётся удерживаемым, пока workers действительно не выйдут.

Неудачные попытки используют ограниченный экспоненциальный backoff от `initialBackoffSecs` до `maxBackoffSecs`. Успешная turn сбрасывает счётчик последовательных неудач; как только `maxRetries` исчерпан, Zode останавливает loop или отключает сохранённое schedule. Ручное прерывание, удаление job и явное отключение отменяют ожидающее восстановление, а не создают новую попытку, если mutation не начиналась. Восстановление намеренно консервативно к побочным эффектам: Zode повторяет автоматически только если не наблюдал побочного эффекта; если mutation могла уже произойти — включая ручную отмену в середине mutation — он останавливает/отключает job и ждёт человеческого review. Инструменты, намеренно отсоединяющие работу (`BashRun` или отсоединённый GUI), также прекращают повтор после этой turn.

Покой (quiescence) — это локальная гарантия. Работа, уже принятая удалённым MCP server, browser extension, desktop actor или другой внешней системой, может не поддерживать отзыв. Если такой вызов прерван, Zode помечает его результат как unresolved, отключает job планировщика и требует, чтобы вы проверили внешнее состояние перед повторным включением.

Используйте `/watchdog status` для конфигурации и health по turn/retry. То же состояние появляется в `/tasks` рядом с фоновыми shells и запущенными turns; там же показываются возраст очереди claimed и рубежи terminal-persistence.

Это watchdog для turns планировщика внутри текущего процесса Zode. Это не OS-супервизор процессов, и он не может перезапустить Zode после сбоя или перезагрузки машины; используйте service manager вашей платформы, когда нужен перезапуск на уровне процесса.

**Тайминг.** `TurnRecorder` проставляет `durationMs` на run-событиях `tool.completed` и `turn.completed` (журналируется; старые journals парсятся как `None`). TUI показывает суффиксы `· 1.2s` для каждого инструмента, footer turn вида `✓ done · 34s · 3 tools` и humanized-время в `/tasks`.

## Ключевые опции конфигурации

Необязательные top-level ключи конфигурации (у всех есть разумные значения по умолчанию):

```jsonc
{
  "maxOutputTokens": 16384,      // лимит вывода за turn (повышайте для записи больших файлов)
  "contextWindow": 1000000,      // context window модели — задайте 1000000 для 1M-модели
  "temperature": 0,              // ниже = более детерминированно
  "language": "ru",              // язык UI (15 локалей); также через /language
  "effort": "medium",            // reasoning effort по умолчанию; также через /effort
  "autonomousOrchestration": true, // оркестрация sub-agent + workflow (по умолчанию вкл.)
  "subagentMaxIterations": 0,      // необязательный guard для потомков; 0/отсутствие = без лимита
  "sandbox": {
    "enabled": true,             // OS sandbox для shell-команд (по умолчанию вкл.)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // разрешить исходящую сеть внутри sandbox
    "writableRoots": []          // дополнительные каталоги для записи (workspace-write)
  },
  "browser": {
    "enabled": true,             // browser_* tools и панель /browser (по умолчанию вкл.)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // режим запуска managed Chromium
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // следить за unattended turns /loop и /schedule
    "inactivityTimeoutSecs": 900, // прервать после 15 минут без активности провайдера/инструмента
    "maxRuntimeSecs": 3600,      // абсолютный часовой лимит на фоновую turn
    "abortGraceSecs": 10,        // ждать кооперативной отмены перед hard-stop
    "maxRetries": 3,             // последовательных попыток восстановления до исчерпания
    "initialBackoffSecs": 5,     // задержка первого retry
    "maxBackoffSecs": 300        // потолок для экспоненциального retry backoff
  }
}
```

> Sandbox ограничивает shell-команды (macOS: sandbox-exec; Linux: `bwrap`, который должен быть установлен). Старт fail closed, если настроенный sandbox нельзя верифицировать; используйте явный флаг `--no-sandbox`, чтобы запуститься без него. Сеть запрещена по умолчанию. Если команде действительно нужен escape, модель ставит `dangerouslyDisableSandbox: true`, а **вы** авторизуете это в approval prompt — или переключаете весь sandbox на лету через `/sandbox`.

> `contextWindow` управляет auto-compaction — задайте его равным реальному window вашей модели (например, `1000000`). Предпочитайте **per-model** значение в `providers.<name>.models.<id>.contextWindow` (оно приоритетнее); top-level ключ выше — глобальный fallback, и zode также заполняет его из встроенного каталога models.dev, когда ни то, ни другое не задано. **Не** ставьте его выше реального window: переоценка приводит к переполнению запросов, и провайдер отклоняет turn.

## Server mode и SDK

`zode server` запускает newline-delimited JSON-RPC server на stdin/stdout. Он предназначен для интеграций с редакторами, локальной автоматизации, тестов и SDK-клиентов, которым нужны возможности zode без запуска TUI.

```bash
zode server                      # stdio (по умолчанию) — то, что запускают SDK
zode server --listen stdio://    # то же самое, но развёрнуто
zode server --listen ws://127.0.0.1:0   # loopback WebSocket + Bearer auth
zode server --listen off         # ничего не запускать и выйти
```

Server mode открывает поведение на основе zode:

- инициализация + обнаружение возможностей (с `approvalPolicy`: `readOnly` (по умолчанию) / `auto` / `prompt`)
- lifecycle метаданных thread и **streaming turns** — вывод модели и вызовы инструментов приходят как JSON-RPC notifications; `turn/interrupt` отменяет turn
- **интерактивные согласования** — политика `prompt` управляет server→client кадрами `approval/request`, на которые отвечают `allow` / `allowAlways` / `deny`
- filesystem read/write/create/stat/list/remove/copy и одноразовый `command/exec`
- model list/set, config read/list/write и read-only skills, hooks, статус MCP-server и списки plugins

WebSocket-транспорт слушает только loopback и пишет файл учётных данных `0600` `<config-dir>/server.json` (`{port, pid, token}`); клиенты аутентифицируются через `Authorization: Bearer <token>`. См. [`sdk/README.md`](../../sdk/README.md) для полного протокола, имён полей notifications и примеров по языкам.

SDK находятся в [`sdk/`](../../sdk/):

| SDK | Каталог | Локальный тест |
|-----|---------|----------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

Каждый SDK предоставляет нативный набор enum/constant `ProtocolMethod` для текущих стабильных имён методов, так что интеграции могут избегать хардкода JSON-RPC-строк. Все params, форма result и имя SDK enum/constant для каждого поддерживаемого метода задокументированы в [method reference `sdk/`](../../sdk/README.md#method-reference).

Только для этого app-server протокола вне охвата остаются: hosted-управление marketplace, remote-control, Realtime, standalone spawn процессов, фоновые терминалы, thread archive/fork, goals и app connectors. Локальные команды session и статического plugin-marketplace, описанные выше, — это отдельные CLI-поверхности.

## Управление браузером

Zode включает группу `tools:browser` для автоматизации браузера. Agent может использовать `browser_read` для screenshots, DOM snapshots, console logs, network logs и чтения tabs; `browser_act` для навигации, кликов, ввода, нажатий клавиш и прокрутки; `browser_eval` для JavaScript; и `browser_tabs` для управления вкладками. Read-only инспекция браузера не gate'ится; mutating действия в браузере используют тот же поток allow-once / always / deny, что и другие инструменты с побочными эффектами.

Есть два target для браузера:

- **managed** — zode запускает и контролирует выделенный профиль Chromium.
- **bridge** — zode контролирует профиль Chrome, который вы уже используете, через встроенное MV3-расширение в [`extensions/chrome/`](../../extensions/chrome/).

Для target bridge один раз загрузите расширение из `extensions/chrome`, затем выполните `/browser pair`. Zode откроет страницу расширения с предзаполненными локальным WebSocket-портом и pairing-кодом; после первого pairing расширение сохраняет token. Оно переподключается к запущенной CLI или автоматически стартует extension-only daemon Zode при необходимости. Вкладки, открытые zode, помещаются в Chrome tab group с именем `zode`.

### Боковая панель задач в Chrome

Запустите обновлённую CLI zode и один раз выполните `/browser pair`. Клик по иконке на панели инструментов открывает side panel; далее она автоматически стартует zode, когда нет запущенного процесса CLI. Страница pairing остаётся небольшим потоком код/token, а задачи остаются общими с сессиями TUI без смены фокуса терминала.

Turns из side panel привязывают bridge-инструменты браузера к странице, показанной рядом с панелью, так что запросы вроде «проанализируй эту страницу» используют `browser_read` на существующей вкладке, а не открывают новую. Автономная автоматизация браузера из TUI и CLI продолжает использовать вкладки, принадлежащие zode, в tab group `zode`. Активная страница также является контекстом по умолчанию для неоднозначных prompts из side panel; локальные файлы проекта инспектируются только когда пользователь явно о них спрашивает.

Панель может отправлять текст, выбирать модель, выбирать режимы доступа `readOnly`, `prompt` и `auto`, стримить ответ и Stop запущенной turn. Turn может приложить максимум 8 файлов и 20 MiB суммарно: изображения PNG, JPEG, GIF и WebP до 5 MiB каждое, плюс UTF-8 текст и файлы кода до 1 MiB каждый. PDF, Office, archive, executable и не-UTF-8 входы отклоняются.

После обновления расширения нажмите Reload на `chrome://extensions`. Старые версии расширения остаются совместимыми с автоматизацией браузера, но не имеют боковой панели задач. В Windows zode находит и запускает Chrome напрямую для extension-URL вместо вызова shell браузера по умолчанию, избегая перенаправления в Microsoft Store, когда Chrome уже установлен.

Полезные команды:

```bash
/browser                         # открыть панель управления браузером
/browser status                  # показать состояние target/running/paired
/browser launch                  # запустить managed-браузер
/browser close                   # закрыть managed-браузер
/browser pair                    # спарить или переподключить расширение Chrome bridge
/browser target managed          # использовать managed Chromium от zode
/browser target bridge           # использовать расширение и сохранить как default для следующего запуска
/browser screenshot [path]       # снять screenshot браузера
```

См. [`extensions/chrome/README.md`](../../extensions/chrome/README.md) для загрузки расширения, обновления, упаковки CRX и smoke-test.

## Управление рабочим столом

Zode умеет управлять нативными десктопными приложениями через API специальных
возможностей (accessibility) ОС, а не только браузером:

- `desktop_read` — чтение дерева доступности (окна, элементы и их ref).
- `desktop_act` — клик, ввод, прокрутка и установка значений по элементу.
- `desktop_screenshot` — снимок экрана.

Чтение не требует подтверждения; изменяющие desktop-действия проходят тот же
процесс подтверждения allow-once / always / deny, что и другие инструменты с
побочными эффектами.

Бэкенды выбираются по платформе:

- **macOS** — API Accessibility (AX).
- **Windows** — UI Automation (UIA).
- **Linux** — AT-SPI.
- **Приложения Electron** — подключение по Chrome DevTools Protocol.

**Призрачный курсор и остановка по Esc.** Zode никогда не двигает ваш настоящий
курсор. На macOS оверлей без прав (`zode-overlay`) рисует *фальшивый* курсор,
летящий по плавной траектории Дубинса к цели каждого действия, чтобы вы видели,
что делает агент (вводимый текст в оверлее не показывается). Пока идёт
desktop-автоматизация, глобальный **Esc** прерывает все выполняющиеся turn и
скрывает оверлей (тот же путь остановки, что и Esc в TUI). Другие платформы
выполняют desktop-действия без визуализации.

Символы без keycode раскладки US (CJK, часть пунктуации) доставляются через
системный буфер обмена (запись → синтез вставки → восстановление прежнего
буфера), чтобы приложения с собственной обработкой клавиш получали настоящие
символы.

```bash
/desktop          # показать цель и состояние прав desktop
/desktop status   # то же самое
```

Конфигурация — в `desktop.*` файла `~/.zode/config.json`:

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor` (по умолчанию `true`) рисует оверлейный курсор на macOS;
`escCancel` (по умолчанию `true`) включает глобальное прерывание по Esc во время
автоматизации; `overlayHelperPath` (по умолчанию `null`) переопределяет путь к
помощнику `zode-overlay` — при его отсутствии просто отключается визуализация.
Desktop-автоматизация при первом использовании может запросить разрешение ОС
(например, Accessibility на macOS).

## Slash-команды

| Команда | Действие |
|---|---|
| `/help` | Overlay команд и keybindings |
| `/clear` | Очистить диалог (и контекст) |
| `/model [id]` | Показать / отметить активную модель |
| `/config` | Показать модель + рабочий каталог |
| `/compact` | Статус auto-compaction контекста |
| `/cost` | Использование токенов и стоимость (включая sub-agents) |
| `/theme [id]` | Сменить тему (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Picker sessions — возобновить в новой вкладке с историей |
| `/connect` | Подключить и переключить активный провайдер |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Показать/скрыть правую боковую панель; свернуть секции MCP / изменённых файлов / todo |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Панель и команды управления браузером; спарить расширение Chrome bridge или переключаться между managed Chromium и профилем Chrome |
| `/loop <interval> [--max N] <prompt>` | Запускать повторяющийся prompt в текущей вкладке; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Сохранить запланированный prompt; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Показать конфигурацию watchdog фоновых turn, health и ожидающие retries |
| `/tasks` | Фоновые shells, запущенные turns и панель health watchdog |
| `/undo`, `/redo` | Отменить / повторить последнюю правку файла |
| `/mcp` | Управлять MCP servers — включать/отключать в диалоге |
| `/skills` | Список доступных skills |
| `/agents` | Управлять sub-agents — создавать (AI-assisted или вручную) / удалять |
| `/external-agents [list\|discover]` | Список поддерживаемых внешних CLI в `PATH` или явная регистрация каждого обнаруженного preset |
| `/team [status\|board\|dismiss <name>]` | Осмотреть постоянный roster teammates и общую board или удалить teammate |
| `/workflows` | Управлять и запускать JS-workflows (`agent()`/`parallel()`/`pipeline()`), детерминированно исполняемые zode |
| `/effort` | Выбрать уровень reasoning effort |
| `/thinking`, `/tool-details` | Переключить показ reasoning / деталей вызовов инструментов |
| `/orchestration` | Переключить автономную оркестрацию sub-agent + workflow |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | Показать / контролировать OS sandbox во время работы |
| `/language` | Сменить язык UI (15 локалей) |
| `/export [path]` | Экспортировать transcript в Markdown (для каталога подставляется имя по умолчанию) |
| `/yolo` | Режим обхода согласований |
| `/exit` | Выход |

Созданные agents и skills, а также подключённые MCP tools тоже появляются как динамические slash-команды (например, `/<name>`) и могут вызываться напрямую.

## Keybindings

> На macOS чорды приложения ниже используют **`Cmd`** (⌘); на Windows/Linux — `Ctrl`. `Ctrl+C/D/L/V` остаются `Ctrl` везде (соглашения терминала).

| Клавиша | Действие |
|---|---|
| `Enter` | Отправить сообщение (ставит в очередь, если turn выполняется) |
| `Shift`/`Alt`+`Enter` | Новая строка |
| `Up` / `Down` | Вспомнить предыдущий / следующий отправленный prompt (или двигать выбор в автодополнении) |
| `Ctrl+C` | Прервать turn (выход, когда idle) |
| `Ctrl+D` | Выход |
| `Ctrl+L` | Перерисовать диалог из store (восстанавливает пустой вид; используйте `/clear`, чтобы сбросить) |
| `Ctrl+V` | Вставить (текст или пути к изображениям) |
| `Cmd/Ctrl+O` | Настройки |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | Новая вкладка / закрыть вкладку |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Перейти к вкладке / циклически переключать вкладки |
| `Cmd/Ctrl+B` | Панель фоновых задач |
| `Cmd/Ctrl+G` | Переключить боковую панель |
| `F1` | Помощь |
| `PgUp` / `PgDn` | Прокрутить диалог |
| `Home` / `End` | Перейти к началу / последнему в диалоге |
| `Esc` | Закрыть текущий overlay (или прервать запущенную turn) |

## Инструкции проекта

Zode читает инструкции из трёхуровневой иерархии (позже перечисленный получает больше внимания): глобальный `~/.zode/AGENTS.md` (или `instructions.md`) → корень проекта → cwd. В каждом каталоге он предпочитает `AGENTS.md`, а не `CLAUDE.md`. Skills живут в `.zode/skills/**/SKILL.md`; MCP servers — в `~/.zode/mcp.json` ⊕ `.mcp.json`; hooks — в `~/.zode/hooks.json` ⊕ `.zode/hooks.json`.

**Кросс-agent конфигурация.** Zode читает прямые skills и MCP-конфигурацию из Claude Code, Codex, Cursor, opencode, Gemini и связанных локальных agents. Установленные plugin trees и plugin caches этих продуктов никогда не сканируются. Чтобы переиспользовать plugin, установите его источник явно через `zode plugin install ... --trust`; форматы пакетов Codex и Claude Code остаются поддержанными для plugins, установленных через Zode.

## Настройка MCP servers

MCP servers живут в той же конфигурации с вложенным приоритетом, что и всё остальное — `~/.zode/mcp.json` для всех проектов, `.mcp.json` или `.zode/mcp.json` в корне проекта, чтобы ограничить сервер одним репозиторием. Ни реестра, ни restart-and-pray: отредактируйте файл, затем `/mcp` (или перезапустите), чтобы подхватить его.

### stdio (запуск локального сервера)

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

`command`/`args` запускают сервер как subprocess по stdio. Значения `env` поддерживают подстановку `$NAME` / `${NAME}` против собственного окружения процесса zode (раскрывается прямо перед подключением, не пишется на диск) — удобно, чтобы держать токены вне самого файла конфигурации.

### Streamable HTTP (удалённый сервер)

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

`"transport": "http"` подключается через Streamable HTTP транспорт текущей спецификации MCP — один `url`, без отдельного SSE-endpoint. `"sse"` принимается как эквивалентное написание (некоторые конфигурации — и собственные setup-docs MCP-серверов — всё ещё так его называют); оба разрешаются в один и тот же connector. `headers` пересылаются дословно (включая `Authorization`, так что Bearer/Basic/custom схемы работают) и поддерживают ту же подстановку `$VAR`, что и `env`. Добавьте `"enabled": false` к любому серверу, чтобы сохранить его определение без подключения — `/mcp` также переключает это для каждого сервера без ручной правки файла.

### Использование

Каждый инструмент, который открывает подключённый сервер, появляется как `mcp__<server>__<tool>`, вызываемый agent как любой встроенный инструмент (и упоминаемый через `@` в поле ввода). `/mcp` открывает диалог со списком всех обнаруженных серверов — connected / disconnected / disabled — с Space для включения или выключения; сворачиваемая секция `mcp` в sidebar (клик по её заголовку ▼ или `/sidebar mcp`) отражает то же живое состояние подключений.

Zode также читает прямую MCP-конфигурацию из Claude Code, Codex, Cursor, opencode и Gemini. Домашняя конфигурация трактуется как настройка пользователя; project-local чужие MCP-определения обнаруживаются отключёнными и могут быть включены через `/mcp`. MCP-объявления, спрятанные в установленном plugin tree другого продукта, не сканируются. `openpencil` зарезервирован — op-bridge управляет им нативно, так что любой сервер, объявленный под этим именем, игнорируется.

## Установка Skills и командного Markdown

Оба — это обычный Markdown на диске: ни реестра, ни шага сборки. Положите файл, и он активен при следующем запуске (или `/skills`, чтобы проверить, что загрузилось).

### Установка skill

Skill — это папка с `SKILL.md` внутри. Положите её под проект (`.zode/skills/`) или в домашний каталог (`~/.zode/skills/`):

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

Теперь skill появляется в `/skills`, agent может вызвать его сам через Skill tool, и он также становится динамической slash-командой — набор `/code-review look at src/lib.rs` раскрывается в prompt, запускающий skill. Дополнительные файлы рядом с `SKILL.md` (references, scripts) поставляются вместе со skill. Прямые каталоги skills, принадлежащие Claude Code, Codex, opencode, Cursor и связанным agents, сканируются. Skills, спрятанные в установленных plugin trees или caches этих продуктов, — нет; установите plugin явно через Zode, если хотите использовать его здесь.

### Установка команды (prompt-Markdown)

Кастомная slash-команда — это один `.md`-файл, чьё **имя файла является именем команды**, а тело — prompt, который она отправляет. Всё, что вы печатаете после команды, дописывается к телу:

```bash
mkdir -p .zode/commands            # или ~/.zode/commands для всех проектов
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

Теперь `/changelog` отправляет этот prompt, а `/changelog only the sidebar work` дописывает ваши аргументы после него. Команды в `~/.claude/commands` и `~/.codex/commands` (и их аналоги на уровне проекта) тоже загружаются; команды внутри *чужого plugin tree* по умолчанию выключены — скопируйте `.md` в каталог `.zode/commands/`, чтобы включить.

## Экосистема ZSeven-W

Zode входит в более широкий стек ZSeven-W для AI-native инструментов разработки:

| Продукт | Что это |
|---------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async runtime для LLM agents: multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions и optional coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native кросс-платформенный UI framework, где `.op`-файл является app и связывает OpenPencil-style design artifacts с runnable software. |
| [`noema`](https://github.com/ZSeven-W/noema) | Local-first, non-vector система памяти для coding agents с lexical recall, review queues, MCP, S3 offload и enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool для design-as-code workflows, превращающий prompts в UI прямо на live canvas и поддерживающий concurrent agent teams. |

## Benchmark

Benchmarks Zode покрывают one-shot code generation, agentic read/run/edit/fix, multi-file tasks, tricky bugs, следование MCP/Skills/constraints и Noema LOCOMO runner. Полная методология, команды воспроизведения и таблицы результатов — в [разделе Benchmark английского README](../../README.md#benchmark); наборы находятся в [`benchmarks/`](../../benchmarks/).

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

Contributions welcome. Следуйте [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`, распространённые scopes — `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
