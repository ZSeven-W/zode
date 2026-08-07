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

- **Multi-provider**: Anthropic, OpenAI und jede OpenAI-kompatible API (DeepSeek, Moonshot, OpenRouter-Dialekte) sowie lokales Ollama. Unterstützt Modelle mit großem Output und **1M-Kontext** (`contextWindow` / `maxOutputTokens` sind konfigurierbar).
- **Breite Werkzeugfläche**: Dateien lesen/schreiben/bearbeiten (einschließlich atomarem Multi-Hunk-`MultiEdit`), Code- und Inhaltssuche, foreground/background shells, git, web fetch (plus optionales `WebSearch` mit einem Tavily-Key), notebooks und TODO-Tracking.
- **Browser-Steuerung**: Integrierte `browser_*`-Tools steuern eine managed Chromium-Instanz oder dein echtes Chrome-Profil über die zode-Chrome-bridge-Erweiterung: navigieren, klicken/tippen, DOM inspizieren, Screenshots aufnehmen, console/network-Logs lesen und von zode geöffnete Tabs gruppieren. Das Pairing ist einmalig – die Erweiterung verbindet sich über zode-Neustarts hinweg automatisch neu.
- **Nicht blockierende Berechtigungen**: Jedes mutierende Tool läuft über eine Freigabe (allow once / always / deny), der Prompt dockt aber inline an und blockiert dich nie – du kannst weiter tippen und eine Folgeanfrage einreihen, während ein Tool wartet, mit harten Deny-Regeln.
- **OS-Sandbox, standardmäßig an**: Shell-Befehle laufen unter sandbox-exec (macOS) / bwrap (Linux) im Modus `read-only` oder `workspace-write`, mit **standardmäßig gesperrtem ausgehendem Netzwerk**. Live umschaltbar mit `/sandbox`; das Modell kann für einen einzelnen Befehl einen Ausbruch anfordern (`dangerouslyDisableSandbox`), den **du** am Prompt autorisierst.
- **Vollbild-TUI**: Streaming-Markdown mit Syntax-Highlighting, Diff-Vorschau, Slash-command-Autocomplete, Prompt-Verlauf (Up/Down), 11 integrierte Themes, Settings- und Help-Overlays, robuste Sidebar-Sektionen rechts und **UI in 15 Sprachen** (`/language`).
- **Dauerhafte, V1-kompatible Sessions**: behalten den bestehenden `<id>.jsonl`-Transcript-Vertrag und ergänzen ihn als Sidecar-Daten um Journals, Checkpoints, Rewind, Fork und isolierte Git-Worktrees. Die Kontext-Compaction verliert nie die sichtbare Konversation – wiederaufgenommene Sessions spielen die vollständige Historie von vor der Compaction ab, während der Modell-Kontext kompakt bleibt.
- **Automatisierungsflächen**: stabile JSON/JSONL-headless-Ausgabe, exakte Session-Adressierung, Tool-Filter, deterministische Exit-Codes, ACP über stdio und ein lokales Operations-Dashboard.
- **Multi-session Tabs**: mehrere Gespräche nebeneinander (`Ctrl+T`), jedes ein isolierter Agent; frühere Sessions mit vollständigem History-Replay wiederaufnehmen.
- **Sub-agents, Teams und Workflows**: einmalige Arbeit über das Task-Tool delegieren, persistente interne oder externe CLI-Teammates anheuern, sie über ein gemeinsames Board und File-Claims koordinieren und die Flächen mit `/agents`, `/team` und `/workflows` verwalten.
- **Portable lokale Konfiguration**: liest direkte Skills- und MCP-Konfiguration von Claude Code, Codex, Cursor, opencode und Gemini, importiert aber nie deren installierte Plugin-Bäume oder Caches.
- **Skills und MCP**: `SKILL.md`-Instruktionspakete bei Bedarf laden und MCP-Server verbinden (`mcp__<server>__<tool>`); erzeugte Agents, Skills und MCP-Tools erscheinen als Slash-Commands.
- **Hooks**: externe Skripte bei Tool-Events ausführen (z. B. gefährliche Befehle blockieren, nach Edits linten).
- **Drei-Ebenen-Instruktionen**: global (`~/.zode/`) → Projektwurzel → cwd (`AGENTS.md` / `CLAUDE.md`).

## Installation

### Eine Zeile (vorgebaute Binaries)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Der Installer erkennt OS und CPU automatisch, lädt das passende Binary vom neuesten [Release](https://github.com/ZSeven-W/zode/releases) und legt `zode` in deinem `PATH` ab. Eine Version pinnen oder den Zielort ändern:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

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

Entpacke und verschiebe `zode` in deinen `PATH` (`sudo mv zode /usr/local/bin/`). Linux-Builds nutzen glibc; macOS-Binaries sind unsigniert (`xattr -dr com.apple.quarantine ./zode`, falls Gatekeeper warnt).

### Aus dem Quellcode

Benötigt Rust 1.88 oder neuer:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# Binary unter target/release/zode
```

> Die agent runtime liegt im git submodule `vendor/agent` – klone immer mit
> `--recurse-submodules` (oder führe `git submodule update --init` aus).

## Schnellstart

Am einfachsten startest du `zode` und führst **`/connect`** aus – ein interaktiver, von models.dev gestützter Picker, der die Konfiguration für dich schreibt.

Um `~/.zode/config.json` von Hand zu schreiben: **`providers`** ist die Quelle der Wahrheit – ein Eintrag pro Provider (geteilte Zugangsdaten) mit einem oder mehreren **models** –, und das top-level **`provider`** hält das *aktive* Modell fest:

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // Wire-Protokoll: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // das aktive Modell
}
```

OpenAI-kompatible Provider (DeepSeek, Moonshot, OpenRouter, …) ergänzen `baseUrl` und `dialect`; die Einstellungen pro Modell liegen im jeweiligen Model-Eintrag:

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

Ein Provider-Eintrag kann mehrere Modelle halten – mit `/model` wechselst du live zwischen ihnen.

Dann ausführen:

```bash
zode                       # Vollbild-TUI
zode -p "explain main.rs"  # headless: ein Prompt, Stream nach stdout, Exit
zode --no-tui              # einfacher readline-REPL
zode -c                    # jüngste Session fortsetzen
zode -r <id>               # Session per id-Präfix wiederaufnehmen
zode --yolo                # Freigabe-Prompts umgehen (Deny-Regeln gelten weiter)
zode --no-sandbox          # OS-Sandbox deaktivieren (sie ist standardmäßig AN)
zode --sandbox-read-only   # Sandbox im Read-only-Modus (alle Writes verweigern)
zode --sandbox-allow-network  # ausgehendes Netzwerk in der Sandbox erlauben
zode --browser             # integrierte Browser-Tools für diesen Lauf erzwingen
zode --no-browser          # integrierte Browser-Tools für diesen Lauf deaktivieren
zode --model <id>          # das Modell überschreiben
zode --provider <name>     # einen benannten Provider aus config.providers wählen
zode server                # JSON-RPC-app-server-Modus über stdio
zode acp                   # Agent-Client-Protocol-Agent über stdio
zode dashboard             # lokale Übersicht über Sessions/Checkpoints/Worktrees
```

Du kannst auch ohne Änderung der Konfiguration auf jeden Provider zeigen, indem du den passenden Key exportierst (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); bei Ollama wird die `baseUrl` aus der Umgebung übernommen, wenn sie nicht gesetzt ist.

## Externe CLI-Teammates

Zode kann eine installierte Drittanbieter-Agent-CLI als einmaligen Task-Worker oder als persistenten bzw. zustandslosen Teammate nutzen. Die Registrierung ist bewusst manuell: Eine CLI zu installieren oder auf den `PATH` zu legen macht sie dem Modell **nicht** zugänglich. Füge ein Profil unter `externalAgents.agents` hinzu und starte Zode dann im Projekt. Oder führe `/external-agents` aus, um unterstützte CLIs auf dem `PATH` zu inspizieren, und dann `/external-agents discover`, um jedes erkannte Preset ausdrücklich zur globalen Konfiguration hinzuzufügen. Dieser Befehl wird vom Nutzer ausgelöst; der Start scannt oder registriert nie automatisch externe CLIs.

| Agent-Profil | Executable | Task-Worker | Team-Modus | Sandbox der externen CLI |
|---|---|---:|---:|---|
| `claude-code` | `claude` | ja | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | ja | persistent | workspace-write |
| `opencode` | `opencode` | ja | stateless | unknown |
| `cline` | `cline` | ja | stateless | unrestricted |
| `antigravity` | `agy` | ja | stateless | unknown |
| `cursor` | `cursor-agent` | ja | persistent | unrestricted |
| `kiro` | `kiro-cli` | ja | stateless | unrestricted |
| `pi` | `pi` | ja | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | ja | persistent | unrestricted |

Jedes registrierte Profil kann einem Team beitreten. Resumable Profile bewahren die Session-ID der CLI und die Konversation über Aufträge hinweg; andere CLIs sind zustandslose Teammates, die für jeden Auftrag einen frischen Prozess starten. Die Presets nutzen die dokumentierten Headless-Schnittstellen von [Cline](https://docs.cline.bot/usage/cli-overview), [Antigravity](https://antigravity.google/docs/cli-best-practices), [Cursor](https://cursor.com/docs/cli/headless), [Kiro](https://kiro.dev/docs/cli/headless/), [Pi](https://pi.dev/docs/latest) und xAIs [Grok Build](https://docs.x.ai/build/cli/headless-scripting). Andere Tools, einschließlich alternativer Grok-CLIs, können ein Custom-Profil verwenden.

### Ein CLI-Profil manuell hinzufügen

Lege `externalAgents` in `~/.zode/config.json` für alle Projekte oder in `<project>/.zode/config.json` für ein einzelnes Projekt ab. Ein leeres Objekt aktiviert ein bekanntes Preset ausdrücklich und löst sein Executable auf dem bereinigten `PATH` auf:

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

Füge nur die Profile hinzu, die du wirklich freigeben willst. Ein bloßes `command` wie `cline` wird auf dem `PATH` aufgelöst; Pfade wie `./tools/my-agent` oder `/opt/agents/my-agent` werden ebenfalls akzeptiert. Bekannte Presets honorieren `enabled`, `command`, `extraArgs`, `envAllow` und `trusted`; `extraArgs` wird an Zodes Preset-Aufruf angehängt.

CLI-Prozesse starten mit einer geleerten Umgebung, die nur `PATH`, `HOME` und `TERM` enthält (plus die nötigen Windows-Variablen); füge API-Keys oder andere benötigte Variablen daher ausdrücklich zu `envAllow` hinzu. Vorhandener Login-State unter `HOME` funktioniert weiter. Ein Projekt-Eintrag mit demselben Profilnamen ersetzt den gesamten globalen Eintrag – wiederhole also jede Überschreibung, die das Projekt weiterhin braucht.

Ein Custom-Profil deklariert den vollständigen Aufruf und das Protokoll:

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

`promptTransport` ist `stdin`, `argv` oder `file`; `argv` erfordert ein eigenständiges `{prompt}`-Argument und `file` erfordert `{prompt_file}`. `output` ist `text`, generisches `jsonl`, `jsonl-claude` oder `jsonl-codex`. Generische JSONL-Profile nutzen RFC-6901-Pointer `textSource` und `sessionIdSource`, um gestreamten Text und eine resumable Session-ID aus jedem Event zu extrahieren. `resumeArgs` muss einen eigenständigen `{session_id}`-Token enthalten und wird bei späteren Turns angehängt; `resumeFlag` bleibt als Kurzform `<flag> <session-id>` erhalten.

Wenn eine CLI eine vom Aufrufer gewählte Session-ID akzeptiert, kann `newSessionArgs` einen eigenständigen `{session_id}`-Token enthalten. Zode erzeugt eine UUID, hängt die expandierten Argumente beim ersten Lauf an und nutzt bei späteren Aufträgen `resumeArgs`. Das macht auch eine Plain-Text-CLI resumable, ohne eine ID aus ihrer Ausgabe parsen zu müssen.

So wird jede Headless-CLI zum Task-Worker oder zustandslosen Teammate. Um den Konversationskontext zwischen Team-Aufträgen zu bewahren, muss sie zusätzlich eine Session-ID exponieren oder eine über `newSessionArgs` akzeptieren, plus einen nicht-interaktiven Resume-Aufruf.

`effectiveSandbox` akzeptiert `none`, `readOnly`, `workspaceWrite`, `unrestricted` oder `unknown` und wird im Trust-Prompt angezeigt.

### Teammate anheuern und mit ihm arbeiten

Bitte den Leader in normaler Sprache; `team_hire` und `team_send` sind Model-Tools, keine Slash-Commands:

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

Der erste Hire zeigt das aufgelöste Executable und seine Argumente, das Arbeitsverzeichnis und die effektive Sandbox der CLI. Wird er genehmigt, delegiert Zode Arbeit an diesen Prozess im aktuellen Projekt: Zode gate't den Prozessstart, aber **nicht** jeden File-Edit oder Shell-Befehl der externen CLI. Trust-Grants gelten für die aktuelle Zode-Session; der persistente Roster wird aus `<cwd>/.zode/team/` wiederhergestellt, ein externer Teammate muss aber nach einem Neustart oder einer Executable-Änderung erneut vertraut werden.

In nicht-interaktiven/Bypass-Läufen (einschließlich `--yolo`) kann Zode den Trust-Prompt nicht zeigen und schlägt fail-closed fehl. Setze `externalAgents.agents.<profile>.trusted` nur dann auf `true`, wenn du dieses Profil bewusst ohne den Prompt laufen lassen willst.

Nutze nach dem Anheuern `/team`, um Roster und Board zu inspizieren:

```text
/team                         # Roster- und Board-Panel
/team status                  # Text-Roster
/team board                   # gemeinsames Ziel, Notizen, Zuweisungen und Claims
/team dismiss implementer     # den Teammate entfernen
```

## Neue Funktionen im Detail

### Strukturierte Headless-Läufe

`-p`, `--prompt-file` und `--prompt-json` nutzen alle dieselbe Headless-Engine. `json` gibt ein finales Ergebnisobjekt aus; `stream-json` gibt pro Zeile ein `zode.run-event.v1`-JSON-Objekt aus. Strukturierte Modi reservieren stdout für maschinenlesbare Ausgabe und nutzen stabile Exit-Codes: `0` Erfolg, `10` Provider-Fehler, `11` Berechtigung verweigert, `12` Turn-/Limit erreicht, `13` unterbrochen (Ctrl-C), `14` partielles Ergebnis, `15` Session-Adressierungsfehler.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# Exakte IDs machen kein Präfix-Matching. Ein Fork mutiert nie seine Quell-Session.
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Tool-Deny-Muster schlagen Allow-Muster und werden von Task-Sub-Agents geerbt. `--permission-mode` akzeptiert `default`, `dont-ask`, `accept-edits` und `bypass`; `--yolo` bleibt eine Abkürzung für bypass, während harte Deny-Regeln weiterhin gelten.

### V1-kompatible Sessions, Checkpoints und Worktrees

Das Transcript bleibt die originale V1-Datei unter `~/.zode/sessions/<id>.jsonl`. Sie ist die **einzige** Transcript-Kopie, sodass ältere Zode-Clients sie weiter lesen und schreiben können. Neue Metadaten sind additiv und liegen in `~/.zode/sessions/<id>/` (`meta.json`, Journal, Checkpoints und Snapshots). Es sind kein neues Session-Format und keine Transcript-Migration nötig.

```bash
zode session list
zode session list --json
zode session show <id>                         # Metadaten + Checkpoint-IDs
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # konfliktbewusste Vorschau
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

Vor einem mutierenden Turn wird ein Checkpoint erfasst. Rewind stellt getrackten Dateiinhalt und das Transcript-Präfix wieder her, meldet Konflikte statt neuere Änderungen zu überschreiben und legt einen neuen logischen Journal-Branch an, statt Historie zu löschen. Worktree-Forks können bei Bedarf ausdrücklich zurückgeführt werden, wenn das Experiment fertig ist.

**Compaction verliert nie die sichtbare Konversation.** Wenn die Kontext-Compaction alte Nachrichten durch eine Zusammenfassung ersetzt, bleiben die Originale in einem additiven Sidecar erhalten (`~/.zode/sessions/<id>/compacted.jsonl`). Das Wiederaufnehmen einer Session, `Ctrl+L`, `/export` und das Chrome-Sidepanel zeigen alle die vollständige Historie von vor der Compaction, während das Modell weiterhin nur den kompaktierten Kontext erhält. Forks tragen das Archiv mit (gefiltert auf ihr eigenes Transcript), `/clear` entfernt es, und das Löschen einer Session entfernt das gesamte Sidecar.

### Berechtigungsregeln und Sandbox-Profile

Regeln können unter `permissions.rules` in `config.json` liegen oder in einer eigenständigen JSON-Datei, die mit `--rules` übergeben wird. Ein Field-Matcher nutzt einen RFC-6901-JSON-Pointer; deny hat Vorrang vor ask, das Vorrang vor allow hat. Die eigenständige Datei muss entweder ein Regel-Array oder `{ "rules": [...] }` sein; sie wird nicht in ein top-level `permissions`-Objekt gewickelt.

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

Eingebaute Profile sind `read-only`, `workspace`, `workspace-network` und `unconfined`. Konfigurationsdefinierte Profile nutzen dieselben Sandbox-Felder wie oben gezeigt.

### Plugins und statische Marketplaces

Ein verwaltetes Plugin kann Skills, Commands, Agents, Hooks, MCP-Server, LSP-Server und sandboxed JavaScript-UI-Renderer beitragen. Zode akzeptiert `plugin.json`, `.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.grok-plugin/plugin.json` und `.claude-plugin/plugin.json`. Die Component-Path-Arrays von Codex und Claude Code werden unterstützt, und Claude Codes `defaultEnabled` wird bei der Erstinstallation honoriert. Host-only-Komponenten wie Codex-Apps/-Connectors und Claude-Code-Themes, -Monitors oder -Output-Styles werden ignoriert; ein reines App-Plugin wird abgelehnt, weil es keine Zode-kompatible Komponente hat. Installationen sind unveränderliche Snapshots mit Provenienz und einem SHA-256-Tree-Hash. Ausführbarer Plugin-Inhalt wird nie ohne das ausdrückliche `--trust`-Flag aktiviert.

#### JavaScript-UI-Plugin – Schnellstart

Das kleinste UI-Plugin enthält ein Manifest und eine JavaScript-Datei:

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

Installiere ein lokales Verzeichnis oder ein GitHub-Repository/-Unterverzeichnis und starte dann einen laufenden Zode-Prozess neu, damit er den neuen Snapshot lädt:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Nutze `zode plugin update my-plugin` nach einer Änderung der Quelle. `--trust` ist erforderlich, weil JavaScript, Hooks, MCP-Server und deklarierter Netzwerkzugriff ausführbare Fähigkeiten sind. Installation und Update drucken die deklarierte Berechtigungsgabe des Plugins (Netzwerk-Hosts, Env-Variablen, Context-Scopes). Ein Update, dessen Manifest *breitere* Berechtigungen anfordert als der installierte Snapshot, wird abgelehnt, es sei denn, du führst es erneut mit `--trust` aus – eine bewegliche Git-Quelle kann ihre eigene Gabe nicht still ausweiten.

#### UI-Render-API

UI-Plugins können deklarative Zeilen direkt oberhalb der Sidebar-Version beitragen – insgesamt höchstens sechs Zeilen, geteilt über alle Plugins in Ladereihenfolge. Deklariere einen JavaScript-Entrypoint im Manifest:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

Registriere einen synchronen Renderer mit `zode.ui.sidebar`. Der Kontext ist ein read-only JSON-Snapshot mit Terminal-, Session-, Model-, Status-, Token- und Context-Window-Feldern. Das Ergebnis wird von Zode gerendert; Skripte erhalten keine Dateisystem-, Netzwerk-, Terminal- oder Ratatui-Brücke.

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

Unterstützte Tones sind `default`, `muted`, `accent`, `success`, `warning` und `danger`; Spans akzeptieren zusätzlich `bold` und `italic`. Ein Renderer muss synchron sein. Jedes Skript ist auf 256 KiB, 8 MiB JS-Speicher und 25 ms pro Auswertung begrenzt, und Renderer werden höchstens alle 250 ms neu ausgewertet (zwischen Auswertungen wird gecachte Ausgabe wiederverwendet). Sidebar-Ausgabe ist auf 6 Zeilen pro Renderer (6 insgesamt über alle Plugins), jede Zeile auf 16 Spans und 2.048 Byte Text begrenzt. Steuerzeichen werden vom Host bereinigt.

Auch die Statusleiste ist erweiterbar. Sie bleibt eine Zeile, wenn kein Plugin Inhalt zurückgibt, und wächst dynamisch auf zwei Zeilen, sobald ein synchroner `zode.ui.statusLine`-Renderer Spans zurückgibt. Zode behält seine Kern-Status- und Sicherheitsanzeigen in der ersten Zeile; Plugin-Ausgabe wird in der zweiten komponiert.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Render-Kontext und Berechtigungen

Jeder Renderer erhält die folgenden Basisfelder, ohne eine zusätzliche Context-Berechtigung anzufordern:

| Feld | Struktur und Bedeutung |
| --- | --- |
| `ctx.apiVersion` | Version der Context-API; aktuell `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | `{ width, height }` in Terminal-Zellen. |
| `ctx.session` | `{ id, title, cwd, busy }` für die aktive Task. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` enthält `{ enabled, readOnly, network }`. |
| `ctx.tokens` | `{ input, output }` Token-Zähler. |
| `ctx.context` | `{ used, window, usedPercent }`; der Prozentwert kann `null` sein. |
| `ctx.data` | Ergebnisse, die nur zu von diesem Plugin registrierten Datenquellen gehören. |

Reichhaltigere Sektionen werden weggelassen, sofern das Plugin nicht den passenden Scope in `permissions.context` anfordert:

| Scope | Exponiertes Feld | Struktur und Grenzen |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` ist 1-basiert. |
| `workspace` | `ctx.workspace.modifiedFiles` | Bis zu 50 `{ path, added, removed }` Git-Einträge. |
| `tools` | `ctx.tools.available` | Sortierte Namen der für die aktive Task aktivierten Tools. |
| `tools` | `ctx.tools.active` | Namen der aktuell ausgeführten Tools. |
| `tools` | `ctx.tools.recent` | Bis zu 20 `{ name, status, durationMs }` Datensätze. |
| `tasks` | `ctx.tasks.todoStatuses` | Nur Todo-Status-Strings, ohne Todo-Text. |
| `tasks` | `ctx.tasks.subagents` | `{ type, status }` Datensätze, ohne Prompts oder Transcripts. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, ohne Goal-Text. |
| `services` | `ctx.services.mcp` | `{ name, connected }` Datensätze. |
| `services` | `ctx.services.lsp` | `{ language, running }` Datensätze. |

Zum Beispiel:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` ist eine Beobachtungs-API: Sie sagt einem Renderer, welche Tools existieren und welche laufen oder liefen. UI-Plugins können kein Tool aufrufen. Tool-Eingaben, Tool-Ausgaben, Prompts, Transcript-Inhalt, Todo-/Goal-Text, Umgebungswerte und Zugangsdaten sind nicht enthalten, und die API kann Zodes Freigabesystem nicht umgehen.

#### Background-HTTP-Daten

UI-Plugins können auch Background-HTTP-Datenquellen registrieren. Netzwerk- und Geheimnis-Zugriff müssen im Manifest deklariert werden:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

Die Anfrage ist deklarativ und läuft außerhalb des Render-Pfads. Geheime Umgebungsvariablen werden von Zode in Header zusammengesetzt und nie an JavaScript exponiert:

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

`zode.data.define(key, config)` akzeptiert einen 1–64 Zeichen langen Key aus Buchstaben, Ziffern, Unterstrichen oder Bindestrichen. `request` unterstützt `url`, `method`, `headers`, optionales JSON-`body` und `timeoutMs`. Defaults sind `GET`, 3 Sekunden Timeout und 60 Sekunden Refresh. Nur HTTPS `GET` und `POST` werden akzeptiert. Literale Header sind Strings; ein geheimer Header nutzt `{ "env": "NAME", "prefix": "Bearer " }`. Die Umgebungsvariable muss außerdem in `permissions.env` erscheinen, wird nur von Rust beim Bauen der Anfrage gelesen und nie an JavaScript zurückgegeben.

Zode deaktiviert Redirects und Proxies, validiert und pinnt öffentliche DNS-Adressen, lehnt localhost/private Netzwerke ab, deckelt Antworten bei 256 KiB, klemmt Request-Timeouts auf 500 ms–10 Sekunden und Refresh-Intervalle auf 10 Sekunden–1 Stunde. Ein Wildcard wie `*.example.com` matcht Subdomains, aber nicht den bloßen Host `example.com`.

Jedes Plugin sieht nur seine eigenen Daten. `ctx.data.<key>` enthält `{ ok, status, data, updatedAt }` oder `{ ok: false, error, updatedAt }`. JSON-Antworten werden zu Objekten/Arrays; nicht-JSON-Antworten werden zu Strings. Ein HTTP-Fehlerstatus enthält weiterhin `status` und `data`, mit `ok: false`.

Starte Zode mit dem benötigten Geheimnis in seiner Umgebung, wenn du eine private Quota- oder Coding-Plan-API nutzt:

```bash
CODING_PLAN_TOKEN=... zode
```

Das [vollständige lauffähige Beispiel](../../examples/plugins/zode-ui-demo/) zeigt Model-/Context-/Tool-Aktivität in Sidebar und Statusleiste und nutzt `zode.data.define` für eine öffentliche GitHub-API-Quota.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# Ein Marketplace ist ein lokaler/Git-basierter statischer Index, kein von Zode gehosteter Dienst.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # bei Bedarf disambiguieren
zode plugin marketplace update
```

### ACP, Dashboard, Telemetrie und PTY-Tests

`zode acp` implementiert ACP initialize/new/load/fork/prompt/cancel über stdio, streamt message-/thought-/tool-Updates, fordert Berechtigungen über den Client an und akzeptiert vom Client bereitgestellte stdio-, HTTP- und SSE-MCP-Server. Session-Daten nutzen denselben V1-kompatiblen Store wie TUI und Headless-CLI.

```bash
zode acp
zode dashboard
zode dashboard --json
```

Der OTLP-Export ist standardmäßig aus und erfordert ein ausdrückliches Opt-in. Er exportiert nur inhaltsfreie Lifecycle-/Tool-Namen-/Status-/Usage-Attribute: Prompts, generierter Text, Tool-Eingaben/-Ausgaben, Dateipfade und Fehlermeldungen werden nie gesendet.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

Für echte-Terminal-TUI-Regressionsszenarien enthält der Workspace ein PTY-+-VT100-Harness, das Raw-Diagnostics und Virtual-Screen-Snapshots aufzeichnet:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` treibt das echte Terminal mit geordneten Waits, Tasteneingaben, Resizes und Snapshots (die Tastennotation unterstützt `<Enter>`, `<Esc>`, `<Tab>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>` und `<C-l>`):

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

Diese lokale/offene Implementierung enthält bewusst keine xAI-spezifischen Accounts, kein Billing und keinen von Zode betriebenen Cloud-Marketplace-Dienst.

Optionale top-level Konfigurationsschlüssel (alle mit sinnvollen Defaults):

```jsonc
{
  "maxOutputTokens": 16384,      // Output-Cap pro Turn (für große Dateischreibvorgänge erhöhen)
  "contextWindow": 1000000,      // Context-Window des Modells — für ein 1M-Modell auf 1000000 setzen
  "temperature": 0,              // niedriger = deterministischer
  "language": "zh-CN",           // UI-Sprache (15 Locales); auch über /language
  "effort": "medium",            // Reasoning-Effort; auf Anthropic mappen medium/high auf echte Thinking-Budgets
  "autonomousOrchestration": true, // Sub-agent- + Workflow-Orchestrierung (Default an)
  "subagentMaxIterations": 0,      // optionaler Child-Guard; weggelassen/0 = unbegrenzt
  "tools": {
    "deferNonCore": false        // true: ~20 Alltags-Tools sichtbar halten, den Rest hinter ToolSearch zurückstellen
  },
  "webSearch": {
    "tavilyApiKey": null         // aktiviert das WebSearch-Tool (oder $TAVILY_API_KEY setzen)
  },
  "sandbox": {
    "enabled": true,             // OS-Sandbox für Shell-Befehle (Default an)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // ausgehendes Netzwerk in der Sandbox erlauben
    "writableRoots": []          // zusätzliche beschreibbare Verzeichnisse (workspace-write)
  },
  "browser": {
    "enabled": true,             // browser_*-Tools und /browser-Panel (Default an)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // Launch-Modus des managed Chromium
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // unbeaufsichtigte /loop- und /schedule-Turns überwachen
    "inactivityTimeoutSecs": 900, // nach 15 Minuten ohne Provider-/Tool-Aktivität abbrechen
    "maxRuntimeSecs": 3600,      // absolute Ein-Stunden-Grenze pro Background-Turn
    "abortGraceSecs": 10,        // auf kooperativen Abbruch warten, bevor hart gestoppt wird
    "maxRetries": 3,             // aufeinanderfolgende Recovery-Versuche vor Erschöpfung
    "initialBackoffSecs": 5,     // erste Retry-Verzögerung
    "maxBackoffSecs": 300        // Deckel für exponentielles Retry-Backoff
  }
}
```

> Die Sandbox schränkt Shell-Befehle ein (macOS: sandbox-exec; Linux: `bwrap`,
> das installiert sein muss). Der Start schlägt fail-closed fehl, wenn die
> konfigurierte Sandbox nicht verifiziert werden kann; nutze das ausdrückliche
> `--no-sandbox`-Flag, um ohne sie zu laufen. Netzwerk ist standardmäßig
> gesperrt. Muss ein Befehl wirklich ausbrechen, setzt das Modell
> `dangerouslyDisableSandbox: true` und **du** autorisierst es am
> Freigabe-Prompt – oder schaltest die ganze Sandbox live mit `/sandbox` um.

> `contextWindow` treibt die Auto-Compaction – setze ihn auf das echte Window
> deines Modells (z. B. `1000000`). Bevorzuge den **pro-Modell**-Wert unter
> `providers.<name>.models.<id>.contextWindow` (er hat Vorrang); der top-level
> Key oben ist ein globaler Fallback, und zode füllt ihn auch aus dem
> mitgelieferten models.dev-Katalog, wenn keiner gesetzt ist. Setze ihn **nicht**
> über das echte Window: Überschätzung lässt Requests überlaufen, und der
> Provider lehnt den Turn ab.

## Server-Modus und SDKs

`zode server` startet einen newline-delimited JSON-RPC-Server auf stdin/stdout. Er ist gedacht für Editor-Integrationen, lokale Automatisierung, Tests und SDK-Clients, die Zodes bestehende Fähigkeiten ohne Start der TUI nutzen wollen.

```bash
zode server                      # stdio (Default) — was die SDKs spawnen
zode server --listen stdio://    # dasselbe, ausgeschrieben
zode server --listen ws://127.0.0.1:0   # Loopback-WebSocket + Bearer-Auth
zode server --listen off         # nichts starten und beenden
```

Der Server-Modus exponiert zode-gestütztes Verhalten:

- Initialisierung + Capability-Discovery (mit einer `approvalPolicy` von `readOnly` (Default) / `auto` / `prompt`)
- Thread-Metadaten-Lifecycle und **Streaming-Turns** – Modell-Output und Tool-Calls kommen als JSON-RPC-Notifications an; `turn/interrupt` bricht einen Turn ab
- **interaktive Freigaben** – die `prompt`-Policy treibt server→client `approval/request`-Frames, die mit `allow` / `allowAlways` / `deny` beantwortet werden
- Dateisystem read/write/create/stat/list/remove/copy und ein einmaliges `command/exec`
- Model list/set, Config read/list/write und read-only Skills, Hooks, MCP-Server-Status und Plugin-Listen

Der WebSocket-Transport bindet nur an Loopback und schreibt eine `0600`-`<config-dir>/server.json`-Credentials-Datei (`{port, pid, token}`); Clients authentifizieren mit `Authorization: Bearer <token>`. Siehe [`sdk/README.md`](../../sdk/README.md) für das vollständige Protokoll, die Notification-Feldnamen und Beispiele pro Sprache.

Speziell für dieses app-server-Protokoll bleiben gehostetes Marketplace-Management, Remote-Control, Realtime, Standalone-Process-Spawn, Background-Terminals, Thread-Archive/-Fork, Goals und App-Connectors außerhalb des Umfangs. Die oben dokumentierten lokalen Session- und statischen Plugin-Marketplace-Befehle sind separate CLI-Flächen.

Die SDKs liegen unter [`sdk/`](../../sdk/):

| SDK | Verzeichnis | Lokaler Test |
|-----|-------------|--------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

Jedes SDK exponiert ein natives `ProtocolMethod`-Enum/Constant-Set für die aktuellen stabilen Methodennamen, damit Integrationen hartkodierte JSON-RPC-Strings vermeiden können. Params, Ergebnisform und SDK-Enum/Constant-Name jeder unterstützten Methode sind in der [`sdk/`-Methodenreferenz](../../sdk/README.md#method-reference) dokumentiert.

## Browser-Steuerung

Zode enthält eine `tools:browser`-Gruppe für Browser-Automatisierung. Der Agent kann `browser_read` für Screenshots, DOM-Snapshots, Console-Logs, Network-Logs und Tab-Reads nutzen; `browser_act` für Navigation, Klicks, Tippen, Tastendruck und Scrollen; `browser_eval` für JavaScript; und `browser_tabs` für Tab-Verwaltung. Read-only-Browser-Inspektion ist ungated; mutierende Browser-Aktionen nutzen denselben allow-once / always / deny-Freigabefluss wie andere Tools mit Seiteneffekten.

Es gibt zwei Browser-Ziele:

- **managed** – zode startet und steuert ein dediziertes Chromium-Profil.
- **bridge** – zode steuert das Chrome-Profil, das du bereits nutzt, über die mitgelieferte MV3-Erweiterung in [`extensions/chrome/`](../../extensions/chrome/).

Für das bridge-Ziel lädst du die Erweiterung einmal aus `extensions/chrome` und führst dann `/browser pair` aus. Zode öffnet die Erweiterungsseite mit vorausgefülltem lokalem WebSocket-Port und Pairing-Code – kommt dieser Tab leer hoch (Chrome verweigert `chrome-extension://`-URLs von der Kommandozeile manchmal), klicke auf das zode-Toolbar-Icon und gib Port + Pairing-Code manuell ein. **Das Pairing ist einmalig**: Die Erweiterung speichert ein langlebiges Token und verbindet sich automatisch neu – beim Browser-Start, bei Erweiterungs-Updates und in einem Ein-Minuten-Retry-Rhythmus, solange die Verbindung getrennt ist – ein zode-Neustart verlangt also nie ein erneutes Pairing. Sie verbindet sich mit einer laufenden CLI wieder oder startet bei Bedarf automatisch einen erweiterungsseitigen zode-Daemon. Von zode geöffnete Tabs werden in einer Chrome-Tab-Gruppe namens `zode` platziert.

### Chrome-Task-Sidepanel

Führe die aktualisierte zode-CLI aus und `/browser pair` einmal. Ein Klick auf das Toolbar-Icon öffnet das Sidepanel; danach startet es zode automatisch, wenn kein CLI-Prozess läuft. Die Pairing-Seite bleibt ein kleiner Code/Token-Fluss, und Tasks bleiben mit TUI-Sessions geteilt, ohne den Terminal-Fokus zu ändern.

Sidepanel-Turns binden bridge-Browser-Tools an die Seite, die aktuell neben dem Panel gezeigt wird, sodass Anfragen wie „analysiere diese Seite“ `browser_read` auf dem bestehenden Tab nutzen statt einen neuen zu öffnen. Standalone-TUI- und -CLI-Browser-Automatisierung verwendet weiter zode-eigene Tabs in der `zode`-Tab-Gruppe. Die aktive Seite ist auch der Standardkontext für mehrdeutige Sidepanel-Prompts; lokale Projektdateien werden nur inspiziert, wenn der Nutzer ausdrücklich danach fragt.

Das Panel kann Text senden, ein Modell wählen, die Zugriffsmodi `readOnly`, `prompt` und `auto` wählen, die Antwort streamen und einen laufenden Turn stoppen. Ein Turn kann höchstens 8 Dateien und 20 MiB gesamt anhängen: PNG-, JPEG-, GIF- und WebP-Bilder bis 5 MiB pro Datei, plus UTF-8-Text- und Code-Dateien bis 1 MiB pro Datei. PDF-, Office-, Archiv-, ausführbare und Nicht-UTF-8-Eingaben werden abgelehnt.

Klicke nach einem Erweiterungs-Update auf Reload unter `chrome://extensions`. Ältere Erweiterungsversionen bleiben mit der Browser-Automatisierung kompatibel, haben aber kein Task-Sidepanel. Unter Windows lokalisiert und startet zode Chrome direkt für Erweiterungs-URLs, statt die Standardbrowser-Shell aufzurufen, und vermeidet so eine Microsoft-Store-Umleitung, wenn Chrome bereits installiert ist.

Nützliche Befehle:

```bash
/browser                         # das Browser-Steuerungspanel öffnen
/browser status                  # Ziel-/Running-/Paired-State zeigen
/browser launch                  # den managed Browser starten
/browser close                   # den managed Browser schließen
/browser pair                    # die Chrome-bridge-Erweiterung pairen oder neu verbinden
/browser target managed          # zodes managed Chromium nutzen
/browser target bridge           # die Erweiterung nutzen und als Next-Launch-Default speichern
/browser screenshot [path]       # einen Browser-Screenshot aufnehmen
```

Siehe [`extensions/chrome/README.md`](../../extensions/chrome/README.md) für Erweiterungs-Laden, -Update, CRX-Packaging und Smoke-Test-Schritte.

## Desktop-Steuerung

Zode kann native Desktop-Anwendungen über die Accessibility-APIs des Betriebssystems steuern, nicht nur den Browser. Der Agent nutzt `desktop_read`, um den Accessibility-Baum zu lesen (Fenster, Elemente und ihre refs), `desktop_act`, um per Element zu klicken, zu tippen, zu scrollen und Werte zu setzen, und `desktop_screenshot`, um den Bildschirm aufzunehmen. Read-only-Reads sind ungated; mutierende Desktop-Aktionen nutzen denselben allow-once / always / deny-Freigabefluss wie andere Tools mit Seiteneffekten.

Backends werden pro Plattform gewählt:

- **macOS** – die Accessibility-(AX-)API.
- **Windows** – UI Automation (UIA).
- **Linux** – AT-SPI.
- **Electron-Apps** – Attach über das Chrome DevTools Protocol.

**Ghost-Cursor und Esc-Stopp.** Zode bewegt nie deine echte Maus. Auf macOS zeichnet ein Zero-Permission-Overlay (`zode-overlay`) einen *falschen* Cursor, der entlang eines glatten Dubins-Pfades zum Ziel jeder Aktion fliegt, sodass du verfolgen kannst, was der Agent tut; getippter Text wird nie im Overlay gezeigt. Während Desktop-Automatisierung aktiv ist, unterbricht ein globales **Esc** jeden laufenden Turn und blendet das Overlay aus (derselbe Stopp-Pfad wie das Esc der TUI). Andere Plattformen führen Desktop-Aktionen ohne die Visualisierung aus.

CJK und anderer Text ohne einen US-Layout-Keycode wird über die System-Pasteboard ausgeliefert (schreiben → Paste synthetisieren → vorherige Zwischenablage wiederherstellen), sodass Apps mit eigener Tastenbehandlung die echten Zeichen erhalten.

```bash
/desktop            # Desktop-Ziel und Berechtigungsstatus zeigen
/desktop status     # dasselbe, explizit
```

Die Konfiguration liegt unter `desktop.*` in `~/.zode/config.json`:

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor` (Default `true`) zeichnet den macOS-Overlay-Cursor; `escCancel` (Default `true`) armiert die globale Esc-Unterbrechung während der Automatisierung; `overlayHelperPath` (Default `null`) überschreibt den Ort des `zode-overlay`-Helpers – ein fehlender Helper deaktiviert schlicht die Visualisierung. Desktop-Automatisierung kann beim ersten Einsatz eine OS-Berechtigung anfordern (z. B. macOS Accessibility).

## Background-Turn-Watchdog

Vom Scheduler besessene `/loop`- und `/schedule`-Turns laufen unter einem in-Prozess-Liveness-Watchdog. Provider-, Tool- und Nested-Agent-Aktivität aktualisiert einen geteilten quellseitigen Heartbeat, während `maxRuntimeSecs` eine absolute Grenze bleibt. Bei einem der Timeouts fordert zode einen kooperativen Abbruch an, wartet `abortGraceSecs` und stoppt die lokale Turn-Task hart, wenn sie sich immer noch nicht geleert hat. Die Task zu stoppen reicht nicht, um ihren Scheduler-Slot freizugeben: zode wartet zudem, bis jeder getrackte Provider, jedes Tool, jeder Hook, jeder Subprozess-Reader und jeder Nested-Agent-Worker zur Ruhe kommt. Wird diese zweite Grenze nicht binnen fünf Sekunden erreicht, wird der Tab/Store in Quarantäne gestellt, der Job deaktiviert und sein Live-Attempt-Lease gehalten, bis die Worker wirklich beendet sind.

Fehlgeschlagene Versuche nutzen begrenztes exponentielles Backoff von `initialBackoffSecs` bis `maxBackoffSecs`. Ein erfolgreicher Turn setzt seinen aufeinanderfolgenden Fehlerzähler zurück; ist `maxRetries` erschöpft, stoppt zode die Schleife oder deaktiviert den persistierten Zeitplan. Manuelle Unterbrechung, Job-Entfernung und explizites Deaktivieren brechen ausstehende Recovery ab, statt einen weiteren Retry zu erzeugen, wenn keine Mutation begann. Recovery ist bewusst konservativ um Seiteneffekte: zode wiederholt nur automatisch, wenn es keinen Seiteneffekt beobachtet hat; könnte eine Mutation bereits erfolgt sein – einschließlich einer manuellen Abbruchs mitten in einer Mutation –, stoppt/deaktiviert es den Job und wartet auf menschliche Prüfung. Tools, die Arbeit bewusst abkoppeln (`BashRun` oder ein detached GUI), stoppen ebenfalls die Wiederholung nach diesem Turn. Dieselbe Inaktivitätsgrenze begrenzt das Claim-to-Start-Queueing: Hält ein beschäftigter Tab oder ein Turn-Preflight eine besessene Occurrence vom Start ab, wird sie zu einem normalen seiteneffektfreien Watchdog-Fehler und tritt in dieselbe begrenzte Retry-Policy ein, statt ihren prozessübergreifenden Lease für immer zu halten.

Ruhe ist eine lokale Garantie. Arbeit, die bereits von einem entfernten MCP-Server, einer Browser-Erweiterung, einem Desktop-Actor oder einem anderen externen System akzeptiert wurde, unterstützt möglicherweise kein Rollback. Wird ein solcher Aufruf unterbrochen, markiert zode sein Ergebnis als ungelöst, deaktiviert den Scheduler-Job und verlangt, dass du den externen Zustand verifizierst, bevor du ihn wieder aktivierst.

Nutze `/watchdog status` für Konfiguration und pro-Turn-/Retry-Gesundheit. Derselbe Zustand erscheint in `/tasks` neben Background-Shells und laufenden Turns; auch Claimed-Queue-Alter und Terminal-Persistence-Fences werden dort gezeigt.

Dies ist ein Watchdog für Scheduler-Turns innerhalb des aktuellen zode-Prozesses. Er ist kein OS-Prozess-Supervisor und kann zode nach einem Crash oder Maschinen-Neustart nicht neu starten; nutze den Service-Manager deiner Plattform, wenn prozessweite Neustarts nötig sind. Persistierte Zeitpläne halten einen Active-Attempt-Token fest, gestützt durch einen OS-File-Lock pro Zeitplan. Beim Start wird ein umkämpfter Lock in Ruhe gelassen, weil ihn noch ein anderer zode-Prozess besitzt; ein freier Lock mit dem exakten persistierten Token ist ein Waise aus einem unsauberen Exit, weshalb zode diesen Zeitplan als execution-state-unknown deaktiviert, statt ihn still zu wiederholen. Dieser Recovery-Vertrag deckt Prozess-Crashes ab. Er beansprucht keine Storage-Level-Haltbarkeit bei plötzlichem Stromausfall oder defekter Hardware und ersetzt keinen OS-Service-Manager.

### `/loop`, `/schedule` und Task-Timing

- **`/loop <30s|5m|1h> [--max N] <prompt>`** – session-only wiederkehrende Turns auf dem aktuellen Tab; `list` / `stop [id]`. Mindestintervall 30s. Ein fälliger Prompt wird über denselben `queued_input`-Pfad eingereiht wie die Goal-Loop (unterbricht nie einen laufenden Turn; überspringt einen Trigger, solange sein Prompt noch eingereiht ist).
- **`/schedule add <hh:mm|mon hh:mm|every 2h> <prompt>`** – persistiert nach `~/.zode/schedules.json` (atomisches tmp+rename; korrupte Dateien werden nach `.corrupt` quarantänisiert). Verpasste Trigger, während zode nicht läuft, werden übersprungen, nie nachgeholt. `list` / `rm <id>` / `enable|disable <id>`.
- **`/watchdog [status]`** – zeigt Konfiguration, Gesundheit und ausstehende Retries des Background-Turn-Watchdogs.
- **`/tasks`** – Background-Shells, laufende Turns und Watchdog-Gesundheitspanel.

`TurnRecorder` stempelt `durationMs` auf `tool.completed`- und `turn.completed`-Run-Events. Die TUI zeigt pro-Tool-Suffixe `· 1.2s`, einen Turn-Footer `✓ done · 34s · 3 tools` und humanisierte verstrichene Zeit in `/tasks`.

## Slash-Commands

| Befehl | Wirkung |
|---|---|
| `/help` | Overlay mit Commands + Keybindings |
| `/clear` | Konversation (und Kontext) löschen |
| `/model [id]` | Aktives Modell zeigen / vermerken |
| `/config` | Modell + Arbeitsverzeichnis zeigen |
| `/compact` | Status der Kontext-Auto-Compaction |
| `/cost` | Token-Nutzung & Kosten bisher (inkl. Sub-Agents) |
| `/theme [id]` | Theme wechseln (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Session-Picker – in einen neuen Tab mit History wiederaufnehmen |
| `/connect` | Aktiven Provider verbinden und wechseln |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Rechte Sidebar zeigen/verbergen; MCP-/Modified-Files-/Todo-Sektionen falten |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Browser-Steuerungspanel und -Befehle; die Chrome-bridge-Erweiterung pairen oder zwischen managed Chromium und deinem Chrome-Profil wechseln |
| `/desktop [status]` | Desktop-Ziel und Berechtigungsstatus zeigen |
| `/loop <interval> [--max N] <prompt>` | Wiederkehrenden Prompt im aktuellen Tab ausführen; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Einen Zeitplan-Prompt persistieren; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Konfiguration, Gesundheit und ausstehende Retries des Background-Turn-Watchdogs zeigen |
| `/tasks` | Background-Shells, laufende Turns und Watchdog-Gesundheitspanel |
| `/undo`, `/redo` | Letzten Datei-Edit rückgängig / wiederherstellen |
| `/mcp` | MCP-Server verwalten – in einem Dialog aktivieren / deaktivieren |
| `/skills` | Verfügbare Skills auflisten |
| `/agents` | Sub-Agents verwalten – erstellen (KI-gestützt oder manuell) / löschen |
| `/external-agents [list\|discover]` | Unterstützte externe CLIs auf `PATH` auflisten oder jedes erkannte Preset ausdrücklich registrieren |
| `/team [status\|board\|dismiss <name>]` | Persistente Teammate-Roster und gemeinsames Board inspizieren oder einen Teammate entfernen |
| `/workflows` | JS-geskriptete Workflows verwalten & ausführen (`agent()`/`parallel()`/`pipeline()`-Orchestrierung, von zode deterministisch ausgeführt) |
| `/effort` | Das Reasoning-Effort-Level wählen |
| `/thinking`, `/tool-details` | Anzeige von Reasoning / Tool-Call-Detail umschalten |
| `/orchestration` | Autonome Sub-agent- + Workflow-Orchestrierung umschalten |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | Die OS-Sandbox zur Laufzeit zeigen / steuern |
| `/language` | Die UI-Sprache wechseln (15 Locales) |
| `/export [path]` | Transcript nach Markdown exportieren (ein Verzeichnis erhält einen Standardnamen) |
| `/yolo` | Bypass-Freigabe-Modus |
| `/exit` | Beenden |

Erzeugte Agents und Skills sowie verbundene MCP-Tools erscheinen ebenfalls als dynamische Slash-Commands (z. B. `/<name>`) und können direkt aufgerufen werden.

## Keybindings

> Auf macOS nutzen die App-Chords unten **`Cmd`** (⌘); auf Windows/Linux `Ctrl`. `Ctrl+C/D/L/V` bleiben überall `Ctrl` (Terminal-Konventionen).

| Taste | Aktion |
|---|---|
| `Enter` | Nachricht senden (reiht ein, wenn ein Turn läuft) |
| `Shift`/`Alt`+`Enter` | Zeilenumbruch |
| `Up` / `Down` | Vorherigen / nächsten eingereichten Prompt abrufen (oder Autocomplete-Auswahl bewegen) |
| `Ctrl+C` | Den Turn unterbrechen (beenden im Leerlauf) |
| `Ctrl+D` | Beenden |
| `Ctrl+L` | Konversation aus dem Store neu zeichnen (stellt eine geleerte Ansicht wieder her; `/clear` zum Verwerfen) |
| `Ctrl+V` | Einfügen (Text oder Bildpfade) |
| `Cmd/Ctrl+O` | Einstellungen |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | Neuer Tab / Tab schließen |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Zu Tabs springen / durchblättern |
| `Cmd/Ctrl+B` | Background-Tasks-Panel |
| `Cmd/Ctrl+G` | Die Sidebar umschalten |
| `F1` | Hilfe |
| `PgUp` / `PgDn` | Die Konversation scrollen |
| `Home` / `End` | Zum Anfang / Ende der Konversation springen |
| `Esc` | Das aktuelle Overlay schließen (oder einen laufenden Turn unterbrechen) |

## Projekt-Instruktionen

Zode liest Instruktionen aus einer Drei-Ebenen-Hierarchie (spätere gewinnen Aufmerksamkeit): global `~/.zode/AGENTS.md` (oder `instructions.md`) → Projektwurzel → cwd. In jedem Verzeichnis bevorzugt es `AGENTS.md` vor `CLAUDE.md`. Skills liegen unter `.zode/skills/**/SKILL.md`; MCP-Server in `~/.zode/mcp.json` ⊕ `.mcp.json`; Hooks in `~/.zode/hooks.json` ⊕ `.zode/hooks.json`.

**Cross-Agent-Konfiguration.** Zode liest direkte Skills- und MCP-Konfiguration von Claude Code, Codex, Cursor, opencode, Gemini und verwandten lokalen Agents. Installierte Plugin-Bäume und Plugin-Caches dieser Produkte werden nie gescannt. Um ein Plugin wiederzuverwenden, installiere seine Quelle ausdrücklich mit `zode plugin install ... --trust`; die Paketformate von Codex und Claude Code bleiben für über Zode installierte Plugins unterstützt.

## MCP-Server konfigurieren

MCP-Server liegen in derselben verschachtelten-Präzedenz-Konfiguration wie alles andere – `~/.zode/mcp.json` für alle Projekte, `.mcp.json` oder `.zode/mcp.json` an der Projektwurzel, um einen auf ein Repo zu beschränken. Keine Registry, kein Restart-and-pray: bearbeite die Datei, dann `/mcp` (oder Neustart), um ihn aufzunehmen.

### stdio (einen lokalen Server spawnen)

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

`command`/`args` spawnen den Server als Subprozess, verrohrt über stdio. `env`-Werte unterstützen `$NAME` / `${NAME}`-Substitution gegen zodes eigene Prozessumgebung (kurz vor dem Verbinden expandiert, nicht auf die Platte geschrieben) – praktisch, um Tokens aus der Config-Datei selbst herauszuhalten.

### Streamable HTTP (Remote-Server)

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

`"transport": "http"` verbindet mit dem Streamable-HTTP-Transport der aktuellen MCP-Spezifikation – eine einzige `url`, kein separater SSE-Endpoint zu konfigurieren. `"sse"` wird als äquivalente Schreibweise akzeptiert (manche Configs – und die Setup-Docs der MCP-Server selbst – nennen es weiterhin so); beide lösen sich auf denselben Connector auf. `headers` werden verbatim weitergeleitet (einschließlich `Authorization`, sodass Bearer-/Basic-/Custom-Schemata alle funktionieren) und unterstützen dieselbe `$VAR`-Substitution wie `env`. Füge `"enabled": false` zu einem Server hinzu, um seine Definition zu behalten, ohne ihn zu verbinden – `/mcp` schaltet dies auch pro Server um, ohne die Datei von Hand zu bearbeiten.

### Nutzung

Jedes Tool, das ein verbundener Server exponiert, erscheint als `mcp__<server>__<tool>`, vom Agent aufrufbar wie jedes eingebaute Tool (und im Eingabefeld per `@` erwähnbar). `/mcp` öffnet einen Dialog, der jeden entdeckten Server auflistet – connected / disconnected / disabled – mit Space zum Ein-/Ausschalten; die faltbare `mcp`-Sektion der Sidebar (Klick auf ihren ▼-Header oder `/sidebar mcp`) spiegelt denselben Live-Verbindungsstatus auf einen Blick.

Zode liest auch direkte MCP-Konfiguration von Claude Code, Codex, Cursor, opencode und Gemini. Home-Konfiguration wird als das Setup des Nutzers behandelt; projektlokale fremde MCP-Definitionen werden deaktiviert entdeckt und lassen sich über `/mcp` aktivieren. In den installierten Plugin-Baum eines anderen Produkts vergrabene MCP-Deklarationen werden nicht gescannt. `openpencil` ist reserviert – op-bridge treibt es nativ an, daher wird jeder unter diesem Namen deklarierte Server ignoriert.

## Skills & Command-Markdown installieren

Beide sind schlichtes Markdown auf der Platte – keine Registry, kein Build-Schritt. Lege eine Datei ab, und sie ist beim nächsten Start aktiv (oder `/skills`, um zu prüfen, was geladen wurde).

### Einen Skill installieren

Ein Skill ist ein Ordner mit einem `SKILL.md` darin. Lege ihn unter das Projekt (`.zode/skills/`) oder dein Home-Verzeichnis (`~/.zode/skills/`):

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

Der Skill erscheint nun in `/skills`, der Agent kann ihn selbst über das Skill-Tool aufrufen, und er wird auch zu einem dynamischen Slash-Command – `/code-review look at src/lib.rs` expandiert zu einem Prompt, der den Skill ausführt. Zusätzliche Dateien neben `SKILL.md` (References, Skripte) werden mit dem Skill ausgeliefert. Direkte Skills-Verzeichnisse von Claude Code, Codex, opencode, Cursor und verwandten Agents werden gescannt. In den installierten Plugin-Bäumen oder Caches dieser Produkte vergrabene Skills nicht; installiere das Plugin ausdrücklich über Zode, wenn du es hier nutzen willst.

### Einen Command installieren (Prompt-Markdown)

Ein Custom-Slash-Command ist eine einzelne `.md`-Datei, deren **Dateiname der Command-Name** ist und deren Body der Prompt ist, den er einreicht. Alles, was du nach dem Command tippst, wird an den Body angehängt:

```bash
mkdir -p .zode/commands            # oder ~/.zode/commands für alle Projekte
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

Nun reicht `/changelog` diesen Prompt ein, und `/changelog only the sidebar work` hängt deine Argumente danach an. Commands in `~/.claude/commands` und `~/.codex/commands` (und ihre projektbezogenen Äquivalente) werden ebenfalls geladen; Commands innerhalb eines *fremden Plugin-Baums* sind standardmäßig aus – kopiere die `.md` in ein `.zode/commands/`-Verzeichnis, um sie zu aktivieren.

## ZSeven-W Ökosystem

Zode ist Teil eines breiteren ZSeven-W-Stacks für AI-native Development-Tools:

| Produkt | Was es ist |
|---------|------------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Eine Pure-Rust-async-Runtime zum Ausliefern von LLM-Agents: Multi-Provider-Streaming, Tool-Dispatch, Permissions, MCP, Cost-Tracking, Attachments, Sessions und optionale Coding-Tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Ein Rust-natives cross-platform UI-Framework, in dem eine `.op`-Datei eine App ist und OpenPencil-artige Design-Artefakte mit lauffähiger Software verbindet. |
| [`noema`](https://github.com/ZSeven-W/noema) | Ein local-first, nicht-vektorielles Memory-System für Coding-Agents mit lexical Recall, Review-Queues, MCP-Zugriff, S3-Offload und Enterprise-Policy-Controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Ein open-source AI-natives Vektor-Design-Tool für design-as-code-Workflows, das Prompts direkt auf einem Live-Canvas in UI verwandelt, mit concurrent Agent-Teams. |

## Benchmark

Zodes Benchmarks decken one-shot Code-Generierung, agentic read/run/edit/fix, Multi-File-Tasks, schwierige Bugs, MCP-/Skills-/Constraint-Following und Noemas LOCOMO-Runner ab. Methodik, Reproduktionsbefehle und Ergebnistabellen stehen im [Benchmark-Abschnitt des englischen README](../../README.md#benchmark); die Suites liegen in [`benchmarks/`](../../benchmarks/).

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

Beiträge sind willkommen! Bitte folge [Conventional Commits](https://www.conventionalcommits.org/) – `<type>(<scope>): <subject>` mit Scopes wie `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`.

## Lizenz

[MIT](../../LICENSE) &copy; ZSeven-W
