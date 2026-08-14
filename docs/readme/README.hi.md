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

- **Multi-provider** — Anthropic, OpenAI, और कोई भी OpenAI-compatible API (DeepSeek, Moonshot, OpenRouter dialects), साथ ही local Ollama. Large-output और **1M-context** models support करता है (`contextWindow` / `maxOutputTokens` configurable हैं)।
- **Rich tool surface** — file read/write/edit (atomic multi-hunk `MultiEdit` सहित), code व content search, foreground और background shells, git, web fetch (Tavily key से optional `WebSearch` भी), notebooks, TODO tracking.
- **Browser control** — built-in `browser_*` tools एक managed Chromium instance या zode Chrome bridge extension के जरिए आपके real Chrome profile को drive कर सकते हैं: navigate, click/type, DOM inspect, screenshots, console/network logs पढ़ना, और zode द्वारा खोले गए tabs को group करना। Pairing सिर्फ एक बार होती है — zode restarts के बाद extension अपने-आप reconnect हो जाता है।
- **Non-blocking permissions** — हर mutating tool gated है (allow once / always / deny), पर prompt inline dock होता है और आपको block नहीं करता: एक tool के इंतज़ार में भी आप follow-up type करके queue कर सकते हैं, और hard-deny rules रहते हैं।
- **OS sandbox, default on** — shell commands sandbox-exec (macOS) / bwrap (Linux) के तहत `read-only` या `workspace-write` mode में चलते हैं, और **outbound network default रूप से denied** है। `/sandbox` से live toggle करें; model एक single command के लिए escape माँग सकता है (`dangerouslyDisableSandbox`) जिसे **आप** prompt पर authorize करते हैं।
- **Full-screen TUI** — syntax highlighting के साथ streaming markdown, diff previews, slash-command autocomplete, prompt history (Up/Down), 11 built-in themes, settings व help overlays, resilient right sidebar sections, और **15-language UI** (`/language`)।
- **Durable, V1-compatible sessions** — मौजूदा `<id>.jsonl` transcript contract बरकरार, साथ ही sidecar data के रूप में journals, checkpoints, rewind, fork और isolated Git worktrees। Context compaction visible conversation कभी नहीं खोती — resume की गई sessions compaction से पहले की पूरी history replay करती हैं, जबकि model context compact रहता है।
- **Automation surfaces** — stable JSON/JSONL headless output, exact session targeting, tool filters, deterministic exit codes, stdio पर ACP, और एक local operations dashboard।
- **Multi-session tabs** — `Ctrl+T` से कई conversations साथ-साथ चलाएँ, हर एक isolated agent; पुरानी sessions को पूरी history replay के साथ resume करें।
- **Sub-agents, teams और workflows** — Task tool से one-shot काम delegate करें, persistent internal या external-CLI teammates hire करें, shared board व file claims से coordinate करें, और `/agents`, `/team`, `/workflows` से manage करें।
- **Portable local configuration** — Claude Code, Codex, Cursor, opencode और Gemini से direct skills व MCP configuration पढ़ता है, पर उनके installed plugin trees या caches को import नहीं करता।
- **Skills & MCP** — `SKILL.md` instruction packs on demand load करें और MCP servers connect करें (`mcp__<server>__<tool>`); बनाए गए agents, skills और MCP tools slash commands के रूप में दिखते हैं।
- **Hooks** — tool events पर external scripts चलाएँ (जैसे dangerous commands block करना, edits के बाद lint करना)।
- **Three-level instructions** — global (`~/.zode/`) → project root → cwd (`AGENTS.md` / `CLAUDE.md`)।

## Install

### One line (prebuilt binaries)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer आपके OS + CPU को auto-detect करता है, latest [release](https://github.com/ZSeven-W/zode/releases) से matching binary download करता है और `zode` को आपके PATH पर रखता है। Version pin करें या location बदलें:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

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

Unpack करें और `zode` को अपने PATH में move करें (`sudo mv zode /usr/local/bin/`)। Linux builds glibc हैं; macOS binaries unsigned हैं (Gatekeeper शिकायत करे तो `xattr -dr com.apple.quarantine ./zode`)।

### From source

Rust 1.88 या नया चाहिए:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binary target/release/zode पर
```

> Agent runtime `vendor/agent` git submodule में रहता है — हमेशा `--recurse-submodules` के साथ clone करें (या `git submodule update --init` चलाएँ)।

## Quick Start

सबसे आसान तरीका है `zode` launch करके **`/connect`** चलाना — यह models.dev-backed interactive picker है जो आपके लिए config लिख देता है।

`~/.zode/config.json` हाथ से लिखना हो तो: **`providers`** source of truth है — हर provider की एक entry (shared credentials) जिसमें एक या अधिक **models** होते हैं — और top-level **`provider`** *active* model रिकॉर्ड करता है:

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // wire protocol: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // the active model
}
```

OpenAI-compatible providers (DeepSeek, Moonshot, OpenRouter, …) एक `baseUrl` + `dialect` जोड़ते हैं, और per-model settings हर model की entry में रहती हैं:

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

एक ही provider entry में कई models रह सकते हैं — `/model` से live switch करें।

फिर चलाएँ:

```bash
zode                       # full-screen TUI
zode -p "explain main.rs"  # headless: एक prompt, stdout पर stream, exit
zode --no-tui              # plain readline REPL
zode -c                    # सबसे हाल की session जारी रखें
zode -r <id>               # id prefix से session resume करें
zode --yolo                # approval prompts bypass (deny rules फिर भी लागू)
zode --no-sandbox          # OS sandbox disable करें (default ON है)
zode --sandbox-read-only   # read-only mode में sandbox (सभी writes deny)
zode --sandbox-allow-network  # sandbox के अंदर outbound network allow करें
zode --browser             # इस run के लिए built-in browser tools force-enable
zode --no-browser          # इस run के लिए built-in browser tools disable
zode --model <id>          # model override करें
zode --provider <name>     # config.providers से एक named provider चुनें
zode server                # stdio पर JSON-RPC app-server mode
zode acp                   # stdio पर Agent Client Protocol agent
zode dashboard             # local sessions/checkpoints/worktrees overview
```

Config edit किए बिना किसी भी provider पर point करने के लिए matching key export करें (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); Ollama के लिए `baseUrl` unset होने पर environment से लिया जाता है।

## External CLI teammates को manually register करना

Zode किसी installed third-party agent CLI को one-shot Task worker, या persistent/stateless teammate की तरह इस्तेमाल कर सकता है। Registration जानबूझकर manual है: किसी CLI को install करना या उसे `PATH` पर रखना उसे model के सामने expose **नहीं** करता। `externalAgents.agents` के तहत एक profile जोड़ें, फिर project में Zode start करें। या `/external-agents` चलाकर `PATH` पर मौजूद supported CLIs देखें, फिर `/external-agents discover` से हर detected preset को global config में explicitly जोड़ें। यह command user-triggered है; startup कभी automatic scan या register नहीं करता।

| Agent profile | Executable | Task worker | Team mode | External CLI sandbox |
|---|---|---:|---:|---|
| `claude-code` | `claude` | हाँ | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | हाँ | persistent | workspace-write |
| `opencode` | `opencode` | हाँ | stateless | unknown |
| `cline` | `cline` | हाँ | stateless | unrestricted |
| `antigravity` | `agy` | हाँ | stateless | unknown |
| `cursor` | `cursor-agent` | हाँ | persistent | unrestricted |
| `kiro` | `kiro-cli` | हाँ | stateless | unrestricted |
| `pi` | `pi` | हाँ | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | हाँ | persistent | unrestricted |

हर registered profile team में शामिल हो सकता है। Resumable profiles CLI का session ID और conversation assignments के बीच बनाए रखते हैं; बाकी CLIs stateless teammates हैं जो हर assignment के लिए नया process शुरू करते हैं।

### CLI profile manually जोड़ना

सभी projects के लिए `externalAgents` को `~/.zode/config.json` में, या एक project के लिए `<project>/.zode/config.json` में रखें। Empty object एक known preset को explicitly enable करता है और उसका executable sanitized `PATH` पर resolve करता है:

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

सिर्फ वही profiles जोड़ें जिन्हें आप expose करना चाहते हैं। `cline` जैसा bare `command` `PATH` पर resolve होता है; `./tools/my-agent` या `/opt/agents/my-agent` जैसे paths भी स्वीकार्य हैं। Known presets `enabled`, `command`, `extraArgs`, `envAllow` और `trusted` का सम्मान करते हैं; `extraArgs` Zode के preset invocation में append होता है।

CLI processes cleared environment से शुरू होते हैं जिसमें केवल `PATH`, `HOME` और `TERM` (साथ ही ज़रूरी Windows variables) होते हैं, इसलिए API keys या अन्य ज़रूरी variables `envAllow` में explicitly जोड़ें। `HOME` के तहत मौजूद login state काम करता रहता है। समान profile नाम वाली project entry पूरे global entry को replace कर देती है, इसलिए project को जो भी override चाहिए उसे दोहराएँ।

एक custom profile पूरा invocation और protocol घोषित करता है:

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

`promptTransport` `stdin`, `argv` या `file` है; `argv` को एक standalone `{prompt}` argument चाहिए और `file` को `{prompt_file}`. `output` `text`, generic `jsonl`, `jsonl-claude` या `jsonl-codex` है। Generic JSONL profiles किसी भी event से streamed text और एक resumable session ID निकालने के लिए RFC 6901 `textSource` व `sessionIdSource` pointers इस्तेमाल करते हैं। `resumeArgs` में standalone `{session_id}` token होना चाहिए और यह बाद के turns पर append होता है; `resumeFlag` shorthand `<flag> <session-id>` रूप के रूप में रखा गया है।

अगर CLI caller-selected session ID स्वीकारता है, तो `newSessionArgs` में standalone `{session_id}` token हो सकता है। Zode एक UUID generate करता है, पहली run पर expanded arguments append करता है, और बाद के assignments पर `resumeArgs` इस्तेमाल करता है। इससे plain-text CLI भी resumable बन जाती है। `effectiveSandbox` `none`, `readOnly`, `workspaceWrite`, `unrestricted` या `unknown` स्वीकारता है और trust prompt में दिखता है।

### Teammate को hire और उसके साथ काम करना

Leader से normal language में कहें; `team_hire` और `team_send` model-facing tools हैं, slash commands नहीं:

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

पहली hire resolved executable व arguments, working directory, और CLI का effective sandbox दिखाती है। इसे approve करने पर वह काम current project में उस process को delegate होता है: Zode process launch को gate करता है, पर external CLI के हर file edit या shell command को gate **नहीं** करता। Trust grants current Zode session भर चलती हैं; persistent roster `<cwd>/.zode/team/` से recover होता है, पर restart या executable बदलने के बाद external teammate को फिर trust करना पड़ता है। Non-interactive/bypass runs (सहित `--yolo`) में Zode trust prompt नहीं दिखा सकता और fail-closed रहता है। किसी profile को बिना prompt चलाने के लिए ही `externalAgents.agents.<profile>.trusted` को `true` सेट करें।

Hire के बाद roster और board देखने के लिए `/team`:

```text
/team                         # roster + board panel
/team status                  # text roster
/team board                   # shared goal, notes, assignments और claims
/team dismiss implementer     # teammate हटाएँ
```

Board host-managed है `<cwd>/.zode/team/` के तहत: `board.json` एक stable lock पर atomically लिखा जाता है, section updates एक revision counter पर CAS होते हैं। Claims subtree-aware TTL leases हैं जिनकी holder identity host inject करता है (tool input से कभी नहीं) और जो canonical cwd तक सीमित रहती हैं।

## Automation, Durable Sessions और Operations

### Structured headless runs

`-p`, `--prompt-file` और `--prompt-json` सभी एक ही headless engine इस्तेमाल करते हैं। `json` एक final result object देता है; `stream-json` हर line पर एक `zode.run-event.v1` JSON object देता है। Structured modes stdout को machine-readable output के लिए reserve रखते हैं और stable exit codes इस्तेमाल करते हैं: `0` success, `10` provider error, `11` permission denied, `12` turn/limit reached, `13` interrupted (Ctrl-C), `14` partial result, `15` session targeting error.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# Exact IDs prefix-match नहीं करते। एक fork अपनी source session को कभी mutate नहीं करता।
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Tool deny patterns allow patterns पर जीतते हैं और Task sub-agents को inherit होते हैं। `--permission-mode` `default`, `dont-ask`, `accept-edits` और `bypass` स्वीकारता है; `--yolo` bypass का shortcut है, जबकि hard deny rules फिर भी लागू रहते हैं।

### V1-compatible sessions, checkpoints और worktrees

Transcript मूल V1 file ही रहता है: `~/.zode/sessions/<id>.jsonl`. यही transcript की **एकमात्र** copy है, इसलिए पुराने Zode clients इसे पढ़ते-लिखते रह सकते हैं। नई metadata additive है और `~/.zode/sessions/<id>/` में रहती है (`meta.json`, journal, checkpoints और snapshots)। कोई नया session format या transcript migration नहीं चाहिए।

```bash
zode session list
zode session list --json
zode session show <id>                         # metadata + checkpoint IDs
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # conflict-aware preview
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

हर mutating turn से पहले एक checkpoint capture होता है। Rewind tracked file content और transcript prefix restore करता है, newer changes को overwrite करने के बजाय conflicts report करता है, और history मिटाने के बजाय एक नई logical journal branch record करता है। Worktree forks को experiment तैयार होने पर explicitly apply back किया जा सकता है।

**Compaction visible conversation कभी नहीं खोती।** जब context compaction पुराने messages को एक summary से replace करती है, तो originals एक additive sidecar में संरक्षित रहते हैं (`~/.zode/sessions/<id>/compacted.jsonl`)। Session resume करना, `Ctrl+L` दबाना, `/export` और Chrome side panel — सभी compaction से पहले की पूरी history दिखाते हैं, जबकि model को सिर्फ compacted context ही मिलता रहता है। Forks archive साथ ले जाते हैं (अपने ही transcript पर filtered), `/clear` इसे हटा देता है, और session delete करने पर पूरा sidecar हट जाता है।

### Permission rules और sandbox profiles

Rules `config.json` में `permissions.rules` के तहत रह सकते हैं, या `--rules` से पास किए गए standalone JSON file में। Field matcher एक RFC 6901 JSON pointer इस्तेमाल करता है; deny, ask पर precedence रखता है, जो allow पर precedence रखता है। Standalone file या तो एक rule array हो, या `{ "rules": [...] }`; इसे top-level `permissions` object में wrap नहीं किया जाता।

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

Built-in profiles `read-only`, `workspace`, `workspace-network` और `unconfined` हैं। Config-defined profiles ऊपर दिखाए गए वही sandbox fields इस्तेमाल करते हैं।

### Plugins और static marketplaces

एक managed plugin skills, commands, agents, hooks, MCP servers, LSP servers और sandboxed JavaScript UI renderers contribute कर सकता है। Zode `plugin.json`, `.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.grok-plugin/plugin.json` और `.claude-plugin/plugin.json` स्वीकारता है। Codex और Claude Code के component path arrays supported हैं, और पहली install पर Claude Code का `defaultEnabled` सम्मानित होता है। Host-only components (जैसे Codex apps/connectors और Claude Code themes, monitors, output styles) ignore होते हैं; एक app-only plugin reject होता है क्योंकि उसमें कोई Zode-compatible component नहीं। Installs provenance और एक SHA-256 tree hash के साथ immutable snapshots होते हैं। Executable plugin content explicit `--trust` flag के बिना कभी activate नहीं होता।

#### JavaScript UI plugin quick start

सबसे छोटे UI plugin में एक manifest और एक JavaScript file होती है:

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

एक local directory या GitHub repository/subdirectory install करें, फिर नए snapshot को load करने के लिए चल रही Zode process को restart करें:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Source बदलने के बाद `zode plugin update my-plugin` इस्तेमाल करें। `--trust` ज़रूरी है क्योंकि JavaScript, hooks, MCP servers और declared network access executable capabilities हैं। Install और update plugin की declared permission grant (network hosts, env vars, context scopes) print करते हैं। एक ऐसा update जिसका manifest installed snapshot से **व्यापक** permissions माँगता है, तब तक reject होता है जब तक आप उसे फिर `--trust` के साथ न चलाएँ — एक moving Git source अपना grant चुपके से नहीं बढ़ा सकता।

#### UI render API

UI plugins sidebar version के ठीक ऊपर declarative rows contribute कर सकते हैं — load order में सभी plugins मिलाकर कुल अधिकतम छह rows। Manifest में एक JavaScript entrypoint declare करें:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

`zode.ui.sidebar` से एक synchronous renderer register करें। Context एक read-only JSON snapshot है जिसमें terminal, session, model, status, token और context-window fields होते हैं। Result Zode render करता है; scripts को कोई filesystem, network, terminal या Ratatui bridge नहीं मिलता।

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

Supported tones `default`, `muted`, `accent`, `success`, `warning` और `danger` हैं; spans `bold` और `italic` भी स्वीकारते हैं। Renderer synchronous होना चाहिए। हर script 256 KiB, 8 MiB JS memory, और per-evaluation 25 ms तक सीमित है, और renderers अधिकतम हर 250 ms में फिर evaluate होते हैं (evaluations के बीच cached output reuse होता है)। Sidebar output per renderer 6 lines (सभी plugins मिलाकर कुल 6) तक सीमित है, हर line 16 spans और 2,048 bytes text तक। Control characters host द्वारा sanitize होते हैं।

Status bar भी extensible है। जब कोई plugin content नहीं देता तो यह एक row रहती है और जब एक synchronous `zode.ui.statusLine` renderer spans देता है तो dynamically दो rows हो जाती है। Zode अपनी core status व safety indicators पहली row पर रखता है; plugin output दूसरी पर compose होता है।

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Render context और permissions

हर renderer को बिना अतिरिक्त context permission माँगे निम्न base fields मिलते हैं:

| Field | Shape और meaning |
| --- | --- |
| `ctx.apiVersion` | Context API version; अभी `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | terminal cells में `{ width, height }`. |
| `ctx.session` | active task के लिए `{ id, title, cwd, busy }`. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` में `{ enabled, readOnly, network }`. |
| `ctx.tokens` | `{ input, output }` token counters. |
| `ctx.context` | `{ used, window, usedPercent }`; percentage `null` हो सकता है. |
| `ctx.data` | केवल इसी plugin द्वारा register किए data sources के results. |

Richer sections तब तक omit रहते हैं जब तक plugin `permissions.context` में matching scope न माँगे:

| Scope | Exposed field | Shape और limits |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` one-based. |
| `workspace` | `ctx.workspace.modifiedFiles` | अधिकतम 50 `{ path, added, removed }` Git entries. |
| `tools` | `ctx.tools.available` | active task के लिए enabled tools के sorted names. |
| `tools` | `ctx.tools.active` | अभी execute हो रहे tools के names. |
| `tools` | `ctx.tools.recent` | अधिकतम 20 `{ name, status, durationMs }` records. |
| `tasks` | `ctx.tasks.todoStatuses` | केवल todo status strings, todo text के बिना. |
| `tasks` | `ctx.tasks.subagents` | `{ type, status }` records, prompts/transcripts के बिना. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, goal text के बिना. |
| `services` | `ctx.services.mcp` | `{ name, connected }` records. |
| `services` | `ctx.services.lsp` | `{ language, running }` records. |

`ctx.tools` एक observation API है: यह renderer को बताता है कि कौन से tools मौजूद हैं और कौन चल रहे या चल चुके हैं। UI plugins tool invoke नहीं कर सकते। Tool inputs/outputs, prompts, transcript content, todo/goal text, environment values और credentials include नहीं होते, और यह API Zode के approval system को bypass नहीं कर सकती।

#### Background HTTP data

UI plugins background HTTP data sources भी register कर सकते हैं। Network और secret access manifest में declare करना ज़रूरी है:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

Request declarative है और render path के बाहर चलता है। Secret environment variables Zode द्वारा headers में assemble होते हैं और JavaScript को कभी expose नहीं होते:

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

`zode.data.define(key, config)` एक 1–64 character alphanumeric, underscore या hyphen key स्वीकारता है। `request` `url`, `method`, `headers`, optional JSON `body` और `timeoutMs` support करता है। Defaults `GET`, 3-second timeout और 60-second refresh हैं। केवल HTTPS `GET` और `POST` स्वीकार्य हैं। Literal headers strings होते हैं; एक secret header `{ "env": "NAME", "prefix": "Bearer " }` इस्तेमाल करता है। Environment variable को `permissions.env` में भी होना चाहिए, इसे केवल Rust request बनाते समय पढ़ता है, और JavaScript को कभी नहीं लौटाया जाता।

Zode redirects और proxies disable करता है, public DNS addresses validate व pin करता है, localhost/private networks reject करता है, responses को 256 KiB पर cap करता है, request timeouts को 500 ms–10 seconds पर clamp करता है, और refresh intervals को 10 seconds–1 hour पर clamp करता है। `*.example.com` जैसा wildcard subdomains match करता है पर bare `example.com` host को नहीं।

हर plugin केवल अपना data देखता है। `ctx.data.<key>` में `{ ok, status, data, updatedAt }` या `{ ok: false, error, updatedAt }` होता है। JSON responses objects/arrays बनते हैं; non-JSON responses strings। एक HTTP error status फिर भी `status` और `data` शामिल करता है, `ok: false` के साथ।

एक private quota या coding-plan API इस्तेमाल करते समय Zode को ज़रूरी secret के साथ start करें:

```bash
CODING_PLAN_TOKEN=... zode
```

[पूरा runnable example](../../examples/plugins/zode-ui-demo/) sidebar और status line में model/context/tool activity दिखाता है और एक public GitHub API quota के लिए `zode.data.define` इस्तेमाल करता है।

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# एक marketplace एक local/Git static index है, कोई Zode-hosted service नहीं।
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # ज़रूरत हो तो disambiguate करें
zode plugin marketplace update
```

### ACP, dashboard, telemetry और PTY tests

`zode acp` stdio पर ACP initialize/new/load/fork/prompt/cancel implement करता है, message/thought/tool updates stream करता है, client के जरिए permissions माँगता है, और client-supplied stdio, HTTP व SSE MCP servers स्वीकारता है। Session data TUI व headless CLI जैसा ही V1-compatible store इस्तेमाल करता है।

```bash
zode acp
zode dashboard
zode dashboard --json
```

OTLP export default off है और explicit opt-in चाहिए। यह केवल content-free lifecycle/tool-name/status/usage attributes export करता है: prompts, generated text, tool inputs/outputs, file paths और error messages कभी नहीं भेजे जाते।

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

Real-terminal TUI regression scenarios के लिए workspace में एक PTY + VT100 harness है जो raw diagnostics और virtual-screen snapshots record करता है:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` real terminal को ordered waits, key input, resizes और snapshots से drive करता है (key notation `<Enter>`, `<Esc>`, `<Tab>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>` और `<C-l>` support करता है):

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

यह local/open implementation जानबूझकर xAI-specific accounts, billing या Zode-operated cloud marketplace service शामिल नहीं करता।

### Optional top-level config keys

सभी keys के sensible defaults हैं:

```jsonc
{
  "maxOutputTokens": 16384,      // per-turn output cap (बड़ी file writes के लिए बढ़ाएँ)
  "contextWindow": 1000000,      // model context window — 1M model के लिए 1000000 सेट करें
  "temperature": 0,              // कम = अधिक deterministic
  "language": "hi",              // UI language (15 locales); /language से भी
  "effort": "medium",            // reasoning effort; Anthropic पर medium/high असली thinking budgets पर map होते हैं
  "autonomousOrchestration": true, // sub-agent + workflow orchestration (default on)
  "subagentMaxIterations": 0,      // optional child guard; omitted/0 = unbounded
  "tools": {
    "deferNonCore": false        // true: ~20 everyday tools visible रखें, बाकी को ToolSearch के पीछे defer करें
  },
  "webSearch": {
    "tavilyApiKey": null         // WebSearch tool enable करता है (या $TAVILY_API_KEY सेट करें)
  },
  "sandbox": {
    "enabled": true,             // shell commands के लिए OS sandbox (default on)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // sandbox के अंदर outbound network allow करें
    "writableRoots": []          // extra writable dirs (workspace-write)
  },
  "browser": {
    "enabled": true,             // browser_* tools और /browser panel (default on)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // managed Chromium launch mode
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // unattended /loop और /schedule turns watch करें
    "inactivityTimeoutSecs": 900, // provider/tool activity के बिना 15 min बाद abort
    "maxRuntimeSecs": 3600,      // per background turn absolute one-hour cap
    "abortGraceSecs": 10,        // hard-stop से पहले cooperative cancellation का इंतज़ार
    "maxRetries": 3,             // exhaustion से पहले consecutive recovery attempts
    "initialBackoffSecs": 5,     // पहली retry delay
    "maxBackoffSecs": 300        // exponential retry backoff का cap
  }
}
```

> Sandbox shell commands confine करता है (macOS: sandbox-exec; Linux: `bwrap`, जो installed होना चाहिए)। अगर configured sandbox verify नहीं हो पाता तो startup fail-closed होता है; इसके बिना चलाने के लिए explicit `--no-sandbox` flag इस्तेमाल करें। Network default रूप से denied है। अगर किसी command को genuinely escape चाहिए, तो model `dangerouslyDisableSandbox: true` सेट करता है और **आप** approval prompt पर authorize करते हैं — या पूरे sandbox को `/sandbox` से live toggle करें।

> `contextWindow` auto-compaction चलाता है — इसे अपने model की असली window पर सेट करें (जैसे `1000000`)। `providers.<name>.models.<id>.contextWindow` के तहत **per-model** value को प्राथमिकता दें (यह precedence लेती है); ऊपर वाली top-level key एक global fallback है, और दोनों unset होने पर zode इसे bundled models.dev catalog से भर देता है। इसे असली window से ऊपर सेट **न** करें: overestimate करने से requests overflow होती हैं और provider turn reject कर देता है।

## Server mode और SDKs

`zode server` stdin/stdout पर एक newline-delimited JSON-RPC server start करता है। यह editor integrations, local automation, tests और उन SDK clients के लिए है जिन्हें TUI launch किए बिना zode की मौजूदा capabilities चाहिए।

```bash
zode server                      # stdio (default) — जो SDKs spawn करते हैं
zode server --listen stdio://    # वही, स्पष्ट रूप से
zode server --listen ws://127.0.0.1:0   # loopback WebSocket + Bearer auth
zode server --listen off         # कुछ शुरू न करें और exit करें
```

Server mode zode-backed behavior expose करता है:

- initialization + capability discovery (एक `approvalPolicy` के साथ: `readOnly` (default) / `auto` / `prompt`)
- thread metadata lifecycle और **streaming turns** — model output और tool calls JSON-RPC notifications के रूप में आते हैं; `turn/interrupt` एक turn cancel करता है
- **interactive approvals** — `prompt` policy server→client `approval/request` frames चलाती है जिनका उत्तर `allow` / `allowAlways` / `deny` से होता है
- filesystem read/write/create/stat/list/remove/copy और one-shot `command/exec`
- model list/set, config read/list/write, और read-only skills, hooks, MCP-server status व plugin lists

WebSocket transport केवल loopback bind करता है और एक `0600` `<config-dir>/server.json` credentials file (`{port, pid, token}`) लिखता है; clients `Authorization: Bearer <token>` से authenticate करते हैं। पूरे protocol, notification field names और per-language examples के लिए [`sdk/README.md`](../../sdk/README.md) देखें।

इस app-server protocol के लिए विशेष रूप से hosted marketplace management, remote-control, Realtime, standalone process spawn, background terminals, thread archive/fork, goals और app connectors out of scope रहते हैं। ऊपर documented local session व static-plugin marketplace commands अलग CLI surfaces हैं।

SDKs [`sdk/`](../../sdk/) के तहत रहते हैं:

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

हर SDK current stable method names के लिए एक native `ProtocolMethod` enum/constant set expose करता है, ताकि integrations hard-coded JSON-RPC strings से बच सकें। हर supported method के params, result shape और SDK enum/constant नाम [`sdk/` method reference](../../sdk/README.md#method-reference) में documented हैं।

```bash
scripts/test-sdks.sh
```

Protocol fixtures `zode-app-server-protocol` से generate होते हैं:

```bash
cargo run -p zode-app-server-protocol --bin export -- sdk/fixtures/jsonrpc
```

## Browser Control

Zode में browser automation के लिए एक `tools:browser` group है। Agent screenshots, DOM snapshots, console logs, network logs और tab reads के लिए `browser_read`; navigation, clicks, typing, key presses व scrolling के लिए `browser_act`; JavaScript के लिए `browser_eval`; और tab management के लिए `browser_tabs` इस्तेमाल कर सकता है। Read-only browser inspection ungated है; mutating browser actions वही allow-once / always / deny approval flow इस्तेमाल करते हैं जो अन्य side-effecting tools।

दो browser targets हैं:

- **managed** — zode एक dedicated Chromium profile launch और control करता है।
- **bridge** — zode [`extensions/chrome/`](../../extensions/chrome/) में bundled MV3 extension के जरिए आपके पहले से इस्तेमाल हो रहे Chrome profile को control करता है।

Bridge target के लिए extension को एक बार `extensions/chrome` से load करें, फिर `/browser pair` चलाएँ। Chrome बाहरी programs से खोले गए `chrome-extension://` URLs को block कर देता है (ERR_BLOCKED_BY_CLIENT — macOS, Windows और Linux तीनों पर), इसलिए zode का खुद page खोलने का प्रयास fail हो सकता है — इसके बजाय extension खुद `/browser pair` के बाद ~30 seconds के भीतर अपनी pairing page खोलता है, जिसमें port pre-filled होता है; वहाँ chat में दिखाया गया 6-अंकों का pairing code enter करें। manual fallback के तौर पर `chrome-extension://…/popup.html?port=…` URL को address bar में खुद type करके भी खोल सकते हैं (हाथ से type की गई navigation browser-initiated मानी जाती है और allowed है)। **Pairing सिर्फ एक बार होती है**: extension एक long-term token store करता है और अपने-आप reconnect होता है — browser startup पर, extension updates पर, और disconnected रहते हुए लगभग हर 30 seconds पर retry करके — इसलिए zode restart करने पर दोबारा pair करने को कभी नहीं कहा जाता। यह एक चल रही CLI से reconnect करता है या ज़रूरत होने पर एक extension-only zode daemon auto-start करता है। Zode द्वारा खोले गए tabs `zode` नामक Chrome tab group में रखे जाते हैं।

### Chrome task side panel

Updated zode CLI चलाएँ और `/browser pair` एक बार करें। Toolbar icon पर click करने से side panel खुलता है; इसके बाद जब कोई CLI process न चल रहा हो तो यह zode को अपने-आप start कर देता है। Pairing page एक छोटा code/token flow रहता है, और tasks terminal focus बदले बिना TUI sessions के साथ shared रहते हैं।

Side-panel turns bridge browser tools को panel के बगल में दिखाए जा रहे current page से bind करते हैं, इसलिए "analyze this page" जैसे requests नया tab खोलने के बजाय मौजूदा tab पर `browser_read` इस्तेमाल करते हैं। Standalone TUI और CLI browser automation `zode` tab group में zode-owned tabs इस्तेमाल करता रहता है। Active page ambiguous side-panel prompts के लिए default context भी है; local project files केवल तभी inspect होते हैं जब user स्पष्ट रूप से उनके बारे में पूछे।

Panel text भेज सकता है, model चुन सकता है, access modes `readOnly`, `prompt` और `auto` चुन सकता है, response stream कर सकता है, और चल रहे turn को Stop कर सकता है। एक turn अधिकतम 8 files और कुल 20 MiB attach कर सकता है: PNG, JPEG, GIF और WebP images हर एक 5 MiB तक, साथ ही UTF-8 text व code files हर एक 1 MiB तक। PDF, Office, archive, executable और non-UTF-8 inputs reject होते हैं।

Extension update के बाद `chrome://extensions` पर Reload click करें। पुराने extension versions browser automation के साथ compatible रहते हैं पर उनमें task side panel नहीं है। Windows पर zode extension URLs के लिए default-browser shell invoke करने के बजाय Chrome को सीधे locate व launch करता है, ताकि Chrome पहले से installed होने पर Microsoft Store redirection से बचा जा सके।

उपयोगी commands:

```bash
/browser                         # browser control panel खोलें
/browser status                  # target/running/paired state दिखाएँ
/browser launch                  # managed browser launch करें
/browser close                   # managed browser close करें
/browser pair                    # Chrome bridge extension pair या reconnect करें
/browser target managed          # zode का managed Chromium इस्तेमाल करें
/browser target bridge           # extension इस्तेमाल करें और अगली launch का default save करें
/browser screenshot [path]       # एक browser screenshot capture करें
```

Extension loading, update, CRX packaging और smoke-test steps के लिए [`extensions/chrome/README.md`](../../extensions/chrome/README.md) देखें।

## Desktop automation

Browser के अलावा, Zode native desktop applications को accessibility APIs के जरिए automate कर सकता है। Agent windows और UI tree inspect करने के लिए `desktop_read`, elements पर click/type/key/scroll व values सेट करने के लिए `desktop_act`, और screen capture के लिए `desktop_screenshot` इस्तेमाल करता है। Browser tools की तरह, read-only inspection ungated है और mutating actions वही allow-once / always / deny approval flow से गुजरते हैं।

Backends per-platform हैं: macOS पर Accessibility (AX), Windows पर UI Automation (UIA), Linux पर AT-SPI, और Electron apps के लिए CDP। `desktop_read` द्वारा minted किए गए window tokens backend का window index होते हैं और `resolve_window` से unchanged round-trip करते हैं।

macOS पर automation एक zero-permission overlay helper (`zode-overlay`) से visualize होता है: zode कभी असली mouse cursor नहीं हिलाता — overlay एक fake "ghost cursor" draw करता है जो हर action से पहले target element तक उड़ता है। जब desktop automation active होता है, एक global **Esc** watcher हर running turn को interrupt कर सकता है (वही path जो TUI Esc का है), फिर overlay hide कर देता है।

`desktop.*` config keys:

```jsonc
{
  "desktop": {
    "ghostCursor": true,          // fake cursor visualization (macOS)
    "escCancel": true,            // desktop automation के दौरान global-Esc stop
    "overlayHelperPath": null     // custom zode-overlay helper path (default: auto)
  }
}
```

`ghostCursor` और `escCancel` default रूप से `true` हैं; `overlayHelperPath` default `null` है (zode executable के बगल में `zode-overlay` ढूँढा जाता है, न मिलने पर visualization चुपचाप disable रहती है)। `/desktop` desktop automation की status दिखाता है।

## Background Turn Watchdog, /loop और /schedule

Scheduler-owned `/loop` और `/schedule` turns एक in-process liveness watchdog के तहत चलते हैं। Provider, tool और nested-agent activity एक shared source-side heartbeat refresh करती है, जबकि `maxRuntimeSecs` एक absolute cap रहता है। किसी भी timeout पर zode cooperative cancellation माँगता है, `abortGraceSecs` इंतज़ार करता है, और अगर local turn task फिर भी drain न हो तो उसे hard-stop करता है। Task रोकना उसका scheduler slot release करने के लिए काफी नहीं: zode हर tracked provider, tool, hook, subprocess reader और nested-agent worker के quiesce होने का भी इंतज़ार करता है। अगर वह दूसरी boundary पाँच seconds में नहीं पहुँचती, तो tab/store quarantine होता है, job disable होता है, और उसका live-attempt lease तब तक held रहता है जब तक workers असल में exit न हो जाएँ।

Failed attempts `initialBackoffSecs` से `maxBackoffSecs` तक bounded exponential backoff इस्तेमाल करते हैं। एक successful turn अपना consecutive-failure count clear करता है; `maxRetries` खत्म होने पर zode loop रोक देता है या persisted schedule disable कर देता है। Manual interruption, job removal और explicit disabling pending recovery cancel करते हैं (जब कोई mutation शुरू न हुआ हो)। Recovery side effects के प्रति जानबूझकर conservative है: zode केवल तभी automatic retry करता है जब कोई side effect observe न हुआ हो; अगर कोई mutation पहले ही हो चुका हो सकता है (सहित mid-mutation manual cancellation), तो job stop/disable होता है और human review का इंतज़ार होता है। Work deliberately detach करने वाले tools (`BashRun` या detached GUI) उस turn के बाद recurrence रोक देते हैं।

Quiescence एक local guarantee है। किसी remote MCP server, browser extension, desktop actor या अन्य external system द्वारा पहले ही accept किया गया काम revocation support नहीं कर सकता। ऐसे किसी call के interrupt होने पर zode उसका result unresolved mark करता है, scheduler job disable करता है, और re-enable से पहले आपसे external state verify कराता है।

Configuration और per-turn/retry health के लिए `/watchdog status` इस्तेमाल करें। वही state `/tasks` में background shells व running turns के साथ दिखता है; claimed queue age और terminal-persistence fences भी वहीं दिखते हैं। यह current zode process के भीतर scheduler turns के लिए एक watchdog है — यह कोई OS process supervisor नहीं है और crash या machine restart के बाद zode को restart नहीं कर सकता।

`/loop` और `/schedule` slash commands:

```text
/loop <interval> [--max N] <prompt>   # current tab में recurring prompt; list / stop [id]
/schedule add <when> <prompt>         # persisted scheduled prompt; list / rm <id> / enable|disable <id>
```

`/loop` session-only recurring turns हैं (minimum interval 30s, जैसे `30s`, `5m`, `1h`) जो current tab पर queue होते हैं। `/schedule` `~/.zode/schedules.json` में persist होते हैं और clock times (`hh:mm`), weekday times (`mon hh:mm`) या intervals (`every 2h`) स्वीकारते हैं; zode के न चलने के दौरान छूटे triggers skip होते हैं, replay नहीं।

### Task timing

`TurnRecorder` `tool.completed` और `turn.completed` run events पर `durationMs` stamp करता है। TUI per-tool `· 1.2s` suffixes, एक `✓ done · 34s · 3 tools` turn footer, और `/tasks` में humanized elapsed time दिखाता है।

## Slash Commands

| Command | काम |
|---|---|
| `/help` | Commands + keybindings overlay |
| `/clear` | Conversation (और context) clear करें |
| `/model [id]` | Active model show / note करें |
| `/config` | Model + working directory दिखाएँ |
| `/compact` | Context auto-compaction status |
| `/cost` | अब तक का token usage व cost (sub-agents सहित) |
| `/theme [id]` | Theme बदलें (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Session picker — history के साथ नए tab में resume करें |
| `/connect` | Active provider connect व switch करें |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Right sidebar show/hide; MCP / modified-files / todo sections fold करें |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Browser control panel व commands |
| `/loop <interval> [--max N] <prompt>` | Current tab में recurring prompt चलाएँ; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Scheduled prompt persist करें; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Background-turn watchdog configuration, health व pending retries दिखाएँ |
| `/tasks` | Background shells, running turns और watchdog health panel |
| `/desktop` | Desktop automation status और target दिखाएँ |
| `/undo`, `/redo` | अंतिम file edit undo / redo करें |
| `/mcp` | MCP servers manage करें — dialog में enable / disable |
| `/skills` | उपलब्ध skills list करें |
| `/agents` | Sub-agents manage करें — create (AI-assisted या manual) / delete |
| `/external-agents [list\|discover]` | `PATH` पर supported external CLIs list करें, या हर detected preset register करें |
| `/team [status\|board\|dismiss <name>]` | Persistent teammate roster व shared board inspect करें, या teammate हटाएँ |
| `/workflows` | JS-scripted workflows manage व run करें |
| `/effort` | Reasoning effort level चुनें |
| `/thinking`, `/tool-details` | Reasoning / tool-call detail दिखाना toggle करें |
| `/orchestration` | Autonomous sub-agent + workflow orchestration toggle करें |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | Runtime पर OS sandbox show / control करें |
| `/language` | UI language switch करें (15 locales) |
| `/export [path]` | Transcript को Markdown में export करें |
| `/yolo` | Bypass-approval mode |
| `/exit` | Quit |

बनाए गए agents व skills, और connected MCP tools भी dynamic slash commands (जैसे `/<name>`) के रूप में दिखते हैं और सीधे invoke हो सकते हैं। पूरा command table [English README](../../README.md#slash-commands) में है।

## Keybindings

> macOS पर नीचे के app chords **`Cmd`** (⌘) इस्तेमाल करते हैं; Windows/Linux पर `Ctrl`. `Ctrl+C/D/L/V` हर जगह `Ctrl` रहते हैं (terminal conventions)।

| Key | Action |
|---|---|
| `Enter` | Message भेजें (turn चलने पर queue) |
| `Shift`/`Alt`+`Enter` | Newline |
| `Up` / `Down` | पिछला / अगला submitted prompt (या autocomplete selection move) |
| `Ctrl+C` | Turn interrupt करें (idle पर quit) |
| `Ctrl+D` | Quit |
| `Ctrl+L` | Store से conversation redraw करें |
| `Ctrl+V` | Paste (text या image paths) |
| `Cmd/Ctrl+O` | Settings |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | New tab / close tab |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Tabs पर jump / cycle |
| `Cmd/Ctrl+B` | Background tasks panel |
| `Cmd/Ctrl+G` | Sidebar toggle करें |
| `F1` | Help |
| `PgUp` / `PgDn` | Conversation scroll करें |
| `Home` / `End` | Conversation के top / latest पर jump करें |
| `Esc` | Current overlay close करें (या running turn interrupt करें) |

## Skills व Command Markdown install करना

दोनों disk पर plain Markdown हैं — कोई registry नहीं, कोई build step नहीं। एक file रखें, और अगली launch पर live (या `/skills` से जाँचें कि क्या load हुआ)।

### एक skill install करें

एक skill एक folder है जिसमें एक `SKILL.md` होती है। इसे project (`.zode/skills/`) या home dir (`~/.zode/skills/`) के तहत रखें:

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

Skill अब `/skills` में दिखता है, agent इसे Skill tool से खुद invoke कर सकता है, और यह एक dynamic slash command भी बन जाता है। Claude Code, Codex, opencode, Cursor और related agents की direct skills directories scan होती हैं; उन products के installed plugin trees या caches में दबी skills नहीं — उसे यहाँ इस्तेमाल करने के लिए plugin को Zode से explicitly install करें।

### एक command install करें (prompt Markdown)

एक custom slash command एक ही `.md` file है जिसका **filename command नाम** है और जिसका body वह prompt है जो submit होता है। Command के बाद जो भी type करें वह body में append होता है:

```bash
mkdir -p .zode/commands            # या सभी projects के लिए ~/.zode/commands
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

अब `/changelog` वह prompt submit करता है, और `/changelog only the sidebar work` आपके arguments उसके बाद append करता है। `~/.claude/commands` व `~/.codex/commands` (और उनके project-level equivalents) के commands भी load होते हैं; किसी *foreign plugin tree* के अंदर के commands default off हैं — opt-in करने के लिए `.md` को एक `.zode/commands/` dir में copy करें।

## Project instructions

Zode instructions को एक three-level hierarchy से पढ़ता है (बाद वाला attention जीतता है): global `~/.zode/AGENTS.md` (या `instructions.md`) → project root → cwd. हर directory में यह `CLAUDE.md` से पहले `AGENTS.md` prefer करता है। Skills `.zode/skills/**/SKILL.md` में रहती हैं; MCP servers `~/.zode/mcp.json` ⊕ `.mcp.json` में; hooks `~/.zode/hooks.json` ⊕ `.zode/hooks.json` में।

**Cross-agent configuration.** Zode Claude Code, Codex, Cursor, opencode, Gemini और related local agents से direct skills व MCP configuration पढ़ता है। उन products के installed plugin trees व plugin caches कभी scan नहीं होते। किसी plugin को reuse करने के लिए उसका source `zode plugin install ... --trust` से explicitly install करें; Zode के जरिए install किए plugins के लिए Codex व Claude Code package formats supported रहते हैं।

## MCP Servers configure करना

MCP servers बाकी सब जैसी nested-precedence config में रहते हैं — सभी projects के लिए `~/.zode/mcp.json`, एक repo scope के लिए project root पर `.mcp.json` या `.zode/mcp.json`. कोई registry नहीं: file edit करें, फिर उसे pick करने के लिए `/mcp` (या relaunch)।

### stdio (एक local server spawn करें)

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

`command`/`args` server को stdio पर piped subprocess के रूप में spawn करते हैं। `env` values zode के अपने process environment के विरुद्ध `$NAME` / `${NAME}` substitution support करते हैं (connect होने से ठीक पहले expand, disk पर नहीं लिखा जाता)।

### Streamable HTTP (remote server)

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

`"transport": "http"` current MCP spec के Streamable HTTP transport से connect करता है — एक ही `url`, कोई अलग SSE endpoint configure नहीं। `"sse"` एक equivalent spelling के रूप में स्वीकार्य है; दोनों उसी connector पर resolve होते हैं। `headers` verbatim forward होते हैं (सहित `Authorization`) और `env` जैसा ही `$VAR` substitution support करते हैं। किसी भी server में `"enabled": false` जोड़कर उसकी definition रखें पर connect न करें — `/mcp` इसे per server toggle भी करता है।

### इसका उपयोग

एक connected server जो भी tool expose करता है वह `mcp__<server>__<tool>` के रूप में दिखता है, किसी भी built-in tool की तरह agent द्वारा callable (और input box में `@`-mentionable)। `/mcp` एक dialog खोलता है जिसमें हर discovered server — connected / disconnected / disabled — Space से toggle होता है; sidebar का collapsible `mcp` section वही live connection state mirror करता है।

Zode Claude Code, Codex, Cursor, opencode और Gemini से direct MCP configuration भी पढ़ता है। Home configuration को user का setup माना जाता है; project-local foreign MCP definitions disabled discover होती हैं और `/mcp` से enable हो सकती हैं। किसी अन्य product के installed plugin tree में दबी MCP declarations scan नहीं होतीं। `openpencil` reserved है — op-bridge इसे natively drive करता है, इसलिए उस नाम से declared कोई भी server ignore होता है।

## ZSeven-W Ecosystem

Zode, ZSeven-W के AI-native development tools के व्यापक stack का हिस्सा है:

| Product | क्या है |
|---------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | LLM agents ship करने के लिए pure-Rust async runtime: multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions और optional coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework जहाँ एक `.op` file एक app है, और OpenPencil-style design artifacts को runnable software से जोड़ता है। |
| [`noema`](https://github.com/ZSeven-W/noema) | Coding agents के लिए local-first, non-vector memory system, जिसमें lexical recall, review queues, MCP access, S3 offload और enterprise policy controls हैं। |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Design-as-code workflows के लिए open-source AI-native vector design tool, जो prompts को live canvas पर सीधे UI में बदलता है और concurrent agent teams support करता है। |

## Benchmark

Zode के benchmarks one-shot code generation, agentic read/run/edit/fix, multi-file tasks, tricky bugs, MCP/Skills/constraint following, और Noema LOCOMO runner cover करते हैं। Head-to-head, हर task एक *hidden* grader द्वारा scored, **Zode + DeepSeek-v4-pro, Claude से मेल खाता है**। पूरी methodology, reproduction commands और result tables [English README के benchmark section](../../README.md#benchmark) में हैं; सभी suites [`benchmarks/`](../../benchmarks/) में रहते हैं।

## Development

```bash
cargo build --workspace                 # सब कुछ build करें
cargo run -p zode                       # TUI चलाएँ
cargo run -p zode -- -p "<prompt>"      # headless single turn
cargo test --workspace                  # सभी tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                        # licenses / advisories / bans
```

## टेस्ट रिपोर्ट

सभी सुइट हरे हैं; एंड-टू-एंड सेल्फ-टेस्ट असली इवोल्यूशन लूप चलाता है — टूल-ग्रुप
फिटनेस → जनरेटेड JS जीन → क्षमता-आधारित चयन → जीनोम पर्सिस्टेंस — और
`SELF-TEST PASSED` प्रिंट करता है:

| सुइट | कमांड | परिणाम |
|---|---|---|
| Harness कोर, इवोल्यूशन लेयर, प्रोसेस प्लगइन | `cargo test -p cordis-rs` | 50 passed |
| इवोल्यूशन इंटीग्रेशन (ग्रुप फिटनेस, जीनोम रीस्टोर) | `cargo test -p zode-core --lib evolution::` | 5 passed |
| QuickJS जीन लेयर (कोड बदलाव, इंटरप्ट, मेमोरी लिमिट) | `cargo test -p zode-core --test js_plugin_it` | 4 passed |
| zode-core पूर्ण सुइट (इवोल्यूशन वायरिंग सहित) | `cargo test -p zode-core --lib` | 983 passed |

```sh
cargo run -p zode-core --example evolution_self_test
```

- हुक पाइपलाइन हर टूल रिज़ल्ट को उसके टूल ग्रुप के विरुद्ध स्कोर करती है
  (`uses − 10·failures − 100·panics − 5·restarts`); `unfit_groups()` बंद करने लायक ग्रुप
  बताता है।
- जीन पूल की क्षमता सीमित है: एजेंट जब नए उम्मीदवार विकसित करता है तो सबसे कमज़ोर
  जीन हट जाते हैं (सेल्फ-टेस्ट में `git` → `todo` → `shell`); सबसे फ़िट बच जाते हैं।
- जनरेटेड जीन JavaScript होते हैं — कंपाइलर की ज़रूरत नहीं — हर जीन की मेमोरी लिमिट
  और इंटरप्ट डेडलाइन के साथ; बेकाबू जीन क्वारंटीन होता है, zode को नुकसान नहीं
  पहुँचाता।
- जीनोम `<config-dir>/evolution/genome.json` में सेव होता है और रीस्टार्ट के बाद
  फिटनेस के साथ बहाल होता है; `dispose()` हर fiber, listener और इतिहास रिकॉर्ड
  रिक्लेम करता है।

पूरी रिपोर्ट (देखा गया आउटपुट और ठीक किए गए रिग्रेशन): `crates/cordis-rs/README.md`।

## Contributing

Contributions का स्वागत है! कृपया [Conventional Commits](https://www.conventionalcommits.org/) follow करें — `<type>(<scope>): <subject>`, जिनमें scopes जैसे `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
