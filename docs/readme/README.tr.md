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
  <strong>Terminaliniz için open-source, AI-native kodlama asistanı.</strong><br/>
  Kodunuzu okur, komut çalıştırır, dosya arar ve hızlı bir Rust TUI üzerinden git yönetir.
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

> Bu yerelleştirilmiş README ürün özeti ve hızlı başlangıcı kapsar. Tam benchmark ayrıntıları ve güncel uzun açıklamalar için [İngilizce README](../../README.md) kaynak kabul edilir.

## Öne çıkanlar

- **Multi-provider** — Anthropic, OpenAI ve herhangi bir OpenAI-compatible API (DeepSeek, Moonshot, OpenRouter dialect'leri) ile yerel Ollama. Large-output ve **1M-context** modelleri destekler (`contextWindow` / `maxOutputTokens` yapılandırılabilir).
- **Zengin araç yüzeyi** — file read/write/edit (atomik çok parçalı `MultiEdit` dâhil), code ve content search, foreground/background shells, git, web fetch (artı Tavily key ile opsiyonel `WebSearch`), notebooks ve TODO tracking.
- **Browser control** — yerleşik `browser_*` araçları managed bir Chromium örneğini ya da zode Chrome bridge extension üzerinden gerçek Chrome profilinizi sürebilir: navigate, click/type, DOM inceleme, screenshot alma, console/network loglarını okuma ve zode'un açtığı sekmeleri gruplama. Pairing tek seferliktir — extension zode yeniden başlatmaları arasında otomatik olarak yeniden bağlanır.
- **Non-blocking permissions** — durum değiştiren her araç gate'lenir (allow once / always / deny), ama prompt satır içinde durur ve sizi engellemez: bir araç beklerken yazmaya devam edip sıradaki isteği kuyruğa alabilirsiniz; hard-deny kuralları geçerlidir.
- **Varsayılan açık OS sandbox** — shell komutları sandbox-exec (macOS) / bwrap (Linux) altında `read-only` veya `workspace-write` modunda, **outbound network varsayılan olarak kapalı** çalışır. `/sandbox` ile canlı olarak değiştirin; model tek bir komut için escape isteyebilir (`dangerouslyDisableSandbox`), buna **siz** prompt'ta izin verirsiniz.
- **Full-screen TUI** — syntax highlighting'li streaming Markdown, diff önizlemeleri, slash-command autocomplete, prompt geçmişi (Up/Down), 11 yerleşik tema, settings/help overlay'leri, dayanıklı sağ sidebar bölümleri ve **15-language UI** (`/language`).
- **Dayanıklı, V1-uyumlu oturumlar** — mevcut `<id>.jsonl` transcript sözleşmesi korunurken journal, checkpoint, rewind, fork ve izole Git worktree'ler yan veri olarak eklenir. Context compaction görünür konuşmayı asla kaybetmez — resume, compaction öncesi tam history'yi yeniden oynatırken model context'i compact kalır.
- **Otomasyon yüzeyleri** — kararlı JSON/JSONL headless çıktı, tam oturum hedefleme, tool filter'ları, deterministik exit code'lar, stdio üzerinden ACP ve yerel operasyon dashboard'u.
- **Multi-session tabs** — birden çok konuşmayı yan yana çalıştırın (`Ctrl+T`), her biri izole bir agent; geçmiş oturumları tam history replay ile resume edin.
- **Sub-agents, teams ve workflows** — Task aracıyla tek seferlik işleri delege edin, kalıcı internal veya external-CLI teammate'leri hire edin, onları paylaşılan bir board ve file claim'leriyle koordine edin ve yüzeyleri `/agents`, `/team`, `/workflows` ile yönetin.
- **Taşınabilir yerel yapılandırma** — Claude Code, Codex, Cursor, opencode ve Gemini'den doğrudan skills ve MCP yapılandırmasını okur; bu ürünlerin kurulu plugin ağaçlarını veya cache'lerini asla içe aktarmaz.
- **Skills ve MCP** — `SKILL.md` talimat paketlerini istek üzerine yükleyin ve MCP servers bağlayın (`mcp__<server>__<tool>`); oluşturulan agent'lar, skills ve MCP araçları slash command olarak görünür.
- **Hooks** — tool event'lerinde external script'ler çalıştırın (örn. tehlikeli komutları engelleyin, edit sonrası lint yapın).
- **Üç seviyeli talimatlar** — global (`~/.zode/`) → proje kökü → cwd (`AGENTS.md` / `CLAUDE.md`).

## Kurulum

### Tek satır (önceden derlenmiş binary'ler)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer OS + CPU'nuzu otomatik algılar, en son [release](https://github.com/ZSeven-W/zode/releases) üzerinden uygun binary'yi indirir ve `zode` komutunu `PATH` içine koyar. Bir sürüme sabitleyin veya konumu değiştirin:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

### Manuel indirme

Platformunuza uygun arşivi [releases page](https://github.com/ZSeven-W/zode/releases) üzerinden indirin:

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Sonra arşivi açıp `zode` dosyasını `PATH` içine taşıyın (`sudo mv zode /usr/local/bin/`). Linux builds glibc kullanır; macOS binaries imzasızdır (Gatekeeper uyarırsa `xattr -dr com.apple.quarantine ./zode`).

### Source'tan build

Rust 1.88 veya daha yeni bir sürüm gerekir:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binary: target/release/zode
```

> Agent runtime `vendor/agent` git submodule içinde yaşar — daima `--recurse-submodules` ile clone edin (veya `git submodule update --init` çalıştırın).

## Quick Start

En kolay yol `zode` başlatıp **`/connect`** çalıştırmaktır — models.dev destekli, yapılandırmayı sizin için yazan interaktif bir seçici.

`~/.zode/config.json` dosyasını elle yazmak için: **`providers`** kaynak niteliğindedir — her provider için bir giriş (paylaşılan kimlik bilgileri) bir veya daha fazla **model** tutar — ve top-level **`provider`** *aktif* modeli kaydeder:

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

OpenAI-compatible provider'lar (DeepSeek, Moonshot, OpenRouter, …) bir `baseUrl` + `dialect` ekler ve per-model ayarları her modelin girişinde yaşar:

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

Bir provider girişi birden çok model tutabilir — `/model` ile aralarında canlı geçiş yapın.

Ardından çalıştırın:

```bash
zode                       # full-screen TUI
zode -p "explain main.rs"  # headless: tek prompt, stdout'a stream, çıkış
zode --no-tui              # düz readline REPL
zode -c                    # en son oturumu sürdür
zode -r <id>               # id prefix'ine göre oturumu resume et
zode --yolo                # onay prompt'larını atla (deny kuralları hâlâ geçerli)
zode --no-sandbox          # OS sandbox'ı kapat (varsayılan AÇIK)
zode --sandbox-read-only   # read-only modda sandbox (tüm yazmaları reddet)
zode --sandbox-allow-network  # sandbox içinde outbound network'e izin ver
zode --browser             # bu çalıştırma için yerleşik browser araçlarını zorla aç
zode --no-browser          # bu çalıştırma için yerleşik browser araçlarını kapat
zode --model <id>          # modeli override et
zode --provider <name>     # config.providers'tan adlı bir provider seç
zode server                # stdio üzerinde JSON-RPC app-server modu
zode acp                   # stdio üzerinde Agent Client Protocol agent'ı
zode dashboard             # yerel sessions/checkpoints/worktrees genel görünümü
```

Config'i düzenlemeden herhangi bir provider'a, ilgili anahtarı export ederek de işaret edebilirsiniz (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); Ollama için `baseUrl` ayarlanmadığında ortamdan alınır.

## External CLI teammates'i manuel kaydetme

Zode kurulu bir üçüncü taraf agent CLI'yi tek seferlik Task worker olarak ya da kalıcı veya durumsuz (stateless) teammate olarak kullanabilir. Kayıt bilinçli olarak manueldir: bir CLI'yi kurmak veya onu `PATH` üzerine koymak onu modele **açmaz**. `externalAgents.agents` altına bir profile ekleyin, sonra Zode'u proje içinde başlatın. Ya da `/external-agents` çalıştırıp o an `PATH` üzerinde bulunan desteklenen CLI'ları inceleyin, ardından `/external-agents discover` ile bulunan her preset'i global config'e açıkça ekleyin. Bu komut kullanıcı tarafından tetiklenir; başlangıç asla external CLI'ları otomatik taramaz veya kaydetmez.

| Agent profile | Executable | Task worker | Team mode | External CLI sandbox |
|---|---|---:|---:|---|
| `claude-code` | `claude` | evet | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | evet | persistent | workspace-write |
| `opencode` | `opencode` | evet | stateless | unknown |
| `cline` | `cline` | evet | stateless | unrestricted |
| `antigravity` | `agy` | evet | stateless | unknown |
| `cursor` | `cursor-agent` | evet | persistent | unrestricted |
| `kiro` | `kiro-cli` | evet | stateless | unrestricted |
| `pi` | `pi` | evet | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | evet | persistent | unrestricted |

Kayıtlı her profile bir team'e katılabilir. Resume edilebilir profile'lar CLI'nin session ID'sini ve konuşmasını atamalar arasında korur; diğer CLI'lar her atama için yeni bir process başlatan stateless teammate'lerdir. Preset'ler [Cline](https://docs.cline.bot/usage/cli-overview), [Antigravity](https://antigravity.google/docs/cli-best-practices), [Cursor](https://cursor.com/docs/cli/headless), [Kiro](https://kiro.dev/docs/cli/headless/), [Pi](https://pi.dev/docs/latest) ve xAI'nin [Grok Build](https://docs.x.ai/build/cli/headless-scripting) belgelenmiş headless arayüzlerini kullanır. Alternatif Grok CLI'ları dâhil diğer araçlar bir custom profile kullanabilir.

### Bir CLI profile'ını manuel ekleme

`externalAgents`'i tüm projeler için `~/.zode/config.json` içine, tek bir proje için `<project>/.zode/config.json` içine koyun. Boş bir object bilinen bir preset'i açıkça etkinleştirir ve executable'ını sanitize edilmiş `PATH` üzerinde çözer:

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

Yalnızca açmak istediğiniz profile'ları ekleyin. `cline` gibi çıplak bir `command` `PATH` üzerinde çözülür; `./tools/my-agent` veya `/opt/agents/my-agent` gibi path'ler de kabul edilir. Bilinen preset'ler `enabled`, `command`, `extraArgs`, `envAllow` ve `trusted` değerlerine uyar; `extraArgs` Zode'un preset invocation'ına eklenir.

CLI process'leri yalnızca `PATH`, `HOME` ve `TERM` (artı gerekli Windows değişkenleri) içeren temizlenmiş bir ortamla başlar, bu yüzden API key'lerini veya gereken diğer değişkenleri açıkça `envAllow` içine ekleyin. `HOME` altındaki mevcut login state çalışmaya devam eder. Aynı profile adına sahip bir proje girişi tüm global girişi değiştirir, bu yüzden projenin hâlâ ihtiyaç duyduğu her override'ı tekrar yazın.

Bir custom profile tüm invocation'ı ve protokolü bildirir:

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

`promptTransport` değeri `stdin`, `argv` veya `file`'dır; `argv` bağımsız bir `{prompt}` argümanı, `file` ise `{prompt_file}` gerektirir. `output` değeri `text`, generic `jsonl`, `jsonl-claude` veya `jsonl-codex`'tir. Generic JSONL profile'ları streamed text'i ve resume edilebilir bir session ID'yi herhangi bir event'ten çıkarmak için RFC 6901 `textSource` ve `sessionIdSource` pointer'ları kullanır. `resumeArgs` bağımsız bir `{session_id}` token içermeli ve sonraki turn'lerde eklenir; `resumeFlag` kısayolu `<flag> <session-id>` biçimi olarak korunur.

Bir CLI caller tarafından seçilen bir session ID kabul ediyorsa, `newSessionArgs` bağımsız bir `{session_id}` token içerebilir. Zode bir UUID üretir, ilk çalıştırmada genişletilmiş argümanları ekler ve sonraki atamalarda `resumeArgs` kullanır. Bu, düz metin bir CLI'yi çıktısından ID parse etmeden de resume edilebilir kılar.

Bu, herhangi bir headless CLI'nin bir Task worker veya stateless teammate olmasını sağlar. Team atamaları arasında konuşma bağlamını korumak için CLI'nin ayrıca bir session ID sunması ya da `newSessionArgs` üzerinden birini kabul etmesi ve non-interactive bir resume invocation'ı olması gerekir.

`effectiveSandbox` değeri `none`, `readOnly`, `workspaceWrite`, `unrestricted` veya `unknown` kabul eder ve trust prompt'unda gösterilir.

### Teammate'i hire etme ve onunla çalışma

Leader'a normal dille sorun; `team_hire` ve `team_send` model-facing araçlardır, slash command değil:

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

İlk hire çözülen executable ve argümanları, working directory'yi ve CLI'nin efektif sandbox'ını gösterir. Onaylamak işi mevcut projede o process'e delege eder: Zode process launch'ını gate eder, ancak external CLI'nin her file edit'ini veya shell command'ını gate **etmez**. Trust grant'leri mevcut Zode oturumu boyunca sürer; kalıcı roster `<cwd>/.zode/team/` içinden kurtarılır, ancak bir external teammate restart veya executable değişiminden sonra yeniden trust edilmelidir.

Non-interactive/bypass çalıştırmalarda (`--yolo` dâhil) Zode trust prompt'unu gösteremez ve fail-closed davranır. `externalAgents.agents.<profile>.trusted` değerini yalnızca o profile'ın prompt olmadan çalışmasını bilinçli olarak istediğinizde `true` yapın.

Hire sonrası roster ve board'u incelemek için `/team` kullanın:

```text
/team                         # roster + board paneli
/team status                  # text roster
/team board                   # paylaşılan hedef, notlar, atamalar ve claim'ler
/team dismiss implementer     # teammate'i kaldır
```

## Otomasyon, kalıcı oturumlar ve operasyon

### Yapılandırılmış headless çalıştırmalar

`-p`, `--prompt-file` ve `--prompt-json` aynı headless engine'i kullanır. `json` tek bir final result object yayar; `stream-json` satır başına bir `zode.run-event.v1` JSON object yayar. Yapılandırılmış modlar stdout'u makine tarafından okunabilir çıktıya ayırır ve kararlı exit code'lar kullanır: `0` başarı, `10` provider error, `11` permission denied, `12` turn/limit reached, `13` interrupted (Ctrl-C), `14` partial result, `15` session targeting error.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# Tam ID'ler prefix eşleşmesi yapmaz. Bir fork kaynak oturumunu asla değiştirmez.
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Tool deny pattern'leri allow pattern'lerine üstün gelir ve Task sub-agent'ları tarafından miras alınır. `--permission-mode` değerleri `default`, `dont-ask`, `accept-edits` ve `bypass`'tır; `--yolo` bypass için bir kısayol olarak kalır, ancak hard deny kuralları hâlâ geçerlidir.

### V1-uyumlu oturumlar, checkpoint'ler ve worktree'ler

Transcript, `~/.zode/sessions/<id>.jsonl` konumundaki orijinal V1 dosyası olarak kalır. Bu **tek** transcript kopyasıdır, böylece eski Zode client'ları onu okumaya ve yazmaya devam edebilir. Yeni metadata additive'dir ve `~/.zode/sessions/<id>/` içinde yaşar (`meta.json`, journal, checkpoint'ler ve snapshot'lar). Yeni bir oturum formatı veya transcript migration gerekmez.

```bash
zode session list
zode session list --json
zode session show <id>                         # metadata + checkpoint ID'leri
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # conflict-aware önizleme
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

Durum değiştiren bir turn'den önce bir checkpoint yakalanır. Rewind, izlenen dosya içeriğini ve transcript prefix'ini geri yükler, daha yeni değişiklikleri üzerine yazmak yerine conflict'leri raporlar ve history'yi silmek yerine yeni bir mantıksal journal branch kaydeder. Worktree fork'ları deney hazır olduğunda açıkça apply-back edilebilir.

**Compaction görünür konuşmayı asla kaybetmez.** Context compaction eski mesajları bir özetle değiştirdiğinde, orijinaller additive bir sidecar'da (`~/.zode/sessions/<id>/compacted.jsonl`) korunur. Bir oturumu resume etmek, `Ctrl+L`, `/export` ve Chrome side panel'i compaction öncesi tam history'yi gösterir; model ise yalnızca compact edilmiş context'i almaya devam eder. Fork'lar arşivi (kendi transcript'lerine filtrelenmiş olarak) taşır, `/clear` onu kaldırır ve bir oturumu silmek tüm sidecar'ı kaldırır.

### Permission kuralları ve sandbox profile'ları

Kurallar `config.json` içindeki `permissions.rules` altında ya da `--rules` ile geçirilen bağımsız bir JSON dosyasında yaşayabilir. Bir field matcher RFC 6901 JSON pointer kullanır; deny, ask'a; ask, allow'a üstün gelir. Bağımsız dosya ya bir kural dizisi ya da `{ "rules": [...] }` olmalıdır; top-level bir `permissions` object'i içine sarılmaz.

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

Yerleşik profile'lar `read-only`, `workspace`, `workspace-network` ve `unconfined`'dır. Config'te tanımlı profile'lar yukarıda gösterilen aynı sandbox alanlarını kullanır. Windows'ta sandbox, mevcut backend'e göre tier'lar hâlinde uygulanır; ayrıntılar için [İngilizce README](../../README.md) kaynaktır.

### Plugin'ler ve statik marketplace'ler

Yönetilen bir plugin skills, commands, agents, hooks, MCP servers, LSP servers ve sandbox'lanmış JavaScript UI renderer'ları katkı sağlayabilir. Zode `plugin.json`, `.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.grok-plugin/plugin.json` ve `.claude-plugin/plugin.json` kabul eder. Codex ve Claude Code component path dizileri desteklenir ve Claude Code'un `defaultEnabled` değeri ilk kurulumda uygulanır. Codex apps/connectors ve Claude Code themes, monitors veya output styles gibi host-only component'ler yok sayılır; yalnızca app içeren bir plugin, Zode-uyumlu component'i olmadığı için reddedilir. Kurulumlar provenance ve SHA-256 tree hash ile immutable snapshot'lardır. Executable plugin içeriği açık `--trust` flag'i olmadan asla etkinleştirilmez.

#### JavaScript UI plugin hızlı başlangıç

En küçük UI plugin bir manifest ve bir JavaScript dosyası içerir:

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

Yerel bir dizini ya da bir GitHub repository/subdirectory'yi kurun, ardından çalışan bir Zode process'ini yeniden başlatın ki yeni snapshot'ı yüklesin:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Kaynağı değiştirdikten sonra `zode plugin update my-plugin` kullanın. `--trust` gereklidir çünkü JavaScript, hooks, MCP servers ve bildirilen network erişimi executable yeteneklerdir. Install ve update, plugin'in bildirdiği permission grant'ını (network hosts, env vars, context scopes) yazdırır. Manifest'i kurulu snapshot'tan **daha geniş** izinler isteyen bir güncelleme, `--trust` ile yeniden çalıştırılmadıkça reddedilir — hareketli bir Git kaynağı kendi grant'ını sessizce genişletemez.

#### UI render API

UI plugin'leri sidebar sürümünün hemen üstüne declarative satırlar katkılayabilir — yükleme sırasına göre tüm plugin'ler arasında paylaşılan toplam en fazla altı satır. Manifest'te bir JavaScript entrypoint bildirin:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

`zode.ui.sidebar` ile senkron bir renderer kaydedin. Context, terminal, session, model, status, token ve context-window alanlarını içeren read-only bir JSON snapshot'tır. Sonuç Zode tarafından render edilir; script'ler filesystem, network, terminal veya Ratatui köprüsü almaz.

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

Desteklenen tone'lar `default`, `muted`, `accent`, `success`, `warning` ve `danger`'dır; span'ler ayrıca `bold` ve `italic` kabul eder. Bir renderer senkron olmalıdır. Her script 256 KiB, 8 MiB JS bellek ve değerlendirme başına 25 ms ile sınırlıdır ve renderer'lar en fazla her 250 ms'de bir yeniden değerlendirilir (değerlendirmeler arasında cache'lenmiş çıktı yeniden kullanılır). Sidebar çıktısı renderer başına 6 satır (plugin'ler arasında toplam 6), her satır 16 span ve 2.048 byte text ile sınırlıdır. Kontrol karakterleri host tarafından temizlenir.

Status bar da genişletilebilir. Hiçbir plugin içerik döndürmediğinde tek satır kalır ve senkron bir `zode.ui.statusLine` renderer'ı span döndürdüğünde dinamik olarak iki satıra büyür. Zode kendi çekirdek status ve güvenlik göstergelerini ilk satırda tutar; plugin çıktısı ikinci satırda birleştirilir.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Render context ve permissions

Her renderer aşağıdaki temel alanları ek bir context permission istemeden alır:

| Alan | Yapı ve anlam |
| --- | --- |
| `ctx.apiVersion` | Context API sürümü; şu an `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | Terminal cell cinsinden `{ width, height }`. |
| `ctx.session` | Aktif task için `{ id, title, cwd, busy }`. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` `{ enabled, readOnly, network }` içerir. |
| `ctx.tokens` | `{ input, output }` token sayaçları. |
| `ctx.context` | `{ used, window, usedPercent }`; yüzde `null` olabilir. |
| `ctx.data` | Yalnızca bu plugin'in kaydettiği data source'lara ait sonuçlar. |

Daha zengin bölümler, plugin `permissions.context` içinde ilgili scope'u istemedikçe atlanır:

| Scope | Açığa çıkan alan | Yapı ve limitler |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` 1-tabanlıdır. |
| `workspace` | `ctx.workspace.modifiedFiles` | En fazla 50 `{ path, added, removed }` Git kaydı. |
| `tools` | `ctx.tools.available` | Aktif task için etkin araçların sıralı adları. |
| `tools` | `ctx.tools.active` | O an çalışan araçların adları. |
| `tools` | `ctx.tools.recent` | En fazla 20 `{ name, status, durationMs }` kaydı. |
| `tasks` | `ctx.tasks.todoStatuses` | Yalnızca todo status string'leri, todo metni olmadan. |
| `tasks` | `ctx.tasks.subagents` | Prompt veya transcript olmadan `{ type, status }` kayıtları. |
| `tasks` | `ctx.tasks.goal` | Goal metni olmadan `{ active, turn }`. |
| `services` | `ctx.services.mcp` | `{ name, connected }` kayıtları. |
| `services` | `ctx.services.lsp` | `{ language, running }` kayıtları. |

Örneğin:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` bir gözlem API'sidir: bir renderer'a hangi araçların var olduğunu ve hangilerinin çalıştığını veya çalışmış olduğunu söyler. UI plugin'leri bir aracı çağıramaz. Tool input'ları, tool output'ları, prompt'lar, transcript içeriği, todo/goal metni, ortam değerleri ve kimlik bilgileri dâhil edilmez ve API Zode'un onay sistemini atlayamaz.

#### Arka plan HTTP verisi

UI plugin'leri arka plan HTTP data source'ları da kaydedebilir. Network ve secret erişimi manifest'te bildirilmelidir:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

İstek declarative'dir ve render path'inin dışında çalışır. Secret ortam değişkenleri Zode tarafından header'lara birleştirilir ve JavaScript'e asla açığa çıkmaz:

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

`zode.data.define(key, config)` 1–64 karakterlik alfanümerik, alt çizgi veya tire içeren bir key kabul eder. `request` şunları destekler: `url`, `method`, `headers`, opsiyonel JSON `body` ve `timeoutMs`. Varsayılanlar `GET`, 3 saniyelik timeout ve 60 saniyelik refresh'tir. Yalnızca HTTPS `GET` ve `POST` kabul edilir. Literal header'lar string'dir; bir secret header `{ "env": "NAME", "prefix": "Bearer " }` kullanır. Ortam değişkeni ayrıca `permissions.env` içinde görünmeli, yalnızca istek oluşturulurken Rust tarafından okunur ve JavaScript'e asla döndürülmez.

Zode redirect ve proxy'leri devre dışı bırakır, public DNS adreslerini doğrular ve sabitler (pin), localhost/private network'leri reddeder, yanıtları 256 KiB ile sınırlar, request timeout'larını 500 ms–10 saniye arasına ve refresh aralıklarını 10 saniye–1 saat arasına sıkıştırır. `*.example.com` gibi bir wildcard subdomain'leri eşler ama çıplak `example.com` host'unu eşlemez.

Her plugin yalnızca kendi verisini görür. `ctx.data.<key>` ya `{ ok, status, data, updatedAt }` ya da `{ ok: false, error, updatedAt }` içerir. JSON yanıtları object/array olur; JSON olmayan yanıtlar string olur. Bir HTTP error status yine de `status` ve `data` içerir, `ok: false` ile.

Bir private quota veya coding-plan API kullanırken Zode'u gereken secret'ı ortamında bulundurarak başlatın:

```bash
CODING_PLAN_TOKEN=... zode
```

[Tam çalıştırılabilir örnek](../../examples/plugins/zode-ui-demo/) sidebar ve status line'da model/context/tool aktivitesini gösterir ve public bir GitHub API quota'sı için `zode.data.define` kullanır.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# Bir marketplace yerel/Git statik bir index'tir, Zode-hosted bir servis değil.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # gerekirse belirsizliği gider
zode plugin marketplace update
```

### ACP, dashboard, telemetry ve TUI regression testleri

`zode acp`, stdio üzerinde ACP initialize/new/load/fork/prompt/cancel uygular, message/thought/tool güncellemelerini stream eder, izinleri client üzerinden ister ve client tarafından sağlanan stdio, HTTP ve SSE MCP server'ları kabul eder. Session verisi TUI ve headless CLI ile aynı V1-uyumlu store'u kullanır.

```bash
zode acp
zode dashboard
zode dashboard --json
```

OTLP export varsayılan olarak kapalıdır ve açık bir opt-in gerektirir. Yalnızca içeriksiz lifecycle/tool-name/status/usage attribute'larını export eder: prompt'lar, üretilen metin, tool input/output'ları, file path'leri ve error mesajları asla gönderilmez.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

Gerçek-terminal TUI regression senaryoları için workspace, raw diagnostics ve virtual-screen snapshot'ları kaydeden bir PTY + VT100 harness'i içerir:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` gerçek terminali sıralı wait'ler, key input'ları, resize'lar ve snapshot'larla sürer (key notasyonu `<Enter>`, `<Esc>`, `<Tab>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>` ve `<C-l>` destekler):

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

Bu yerel/açık implementasyon bilinçli olarak xAI'ye özgü hesapları, faturalandırmayı veya Zode tarafından işletilen bir cloud marketplace servisini içermez.

Opsiyonel top-level config anahtarları (hepsinin makul varsayılanları vardır):

```jsonc
{
  "maxOutputTokens": 16384,      // per-turn output cap (raise for big file writes)
  "contextWindow": 1000000,      // model context window — set 1000000 for a 1M model
  "temperature": 0,              // lower = more deterministic
  "language": "tr",              // UI language (15 locales); also via /language
  "effort": "medium",            // reasoning effort; Anthropic'te medium/high gerçek thinking budget'larına eşlenir
  "autonomousOrchestration": true, // sub-agent + workflow orchestration (default on)
  "subagentMaxIterations": 0,      // optional child guard; omitted/0 = unbounded
  "tools": {
    "deferNonCore": false        // true: ~20 günlük aracı görünür tut, kalanını ToolSearch arkasına ertele
  },
  "webSearch": {
    "tavilyApiKey": null         // WebSearch aracını etkinleştirir (veya $TAVILY_API_KEY ayarlayın)
  },
  "sandbox": {
    "enabled": true,             // OS sandbox for shell commands (default on)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // allow outbound network inside the sandbox
    "writableRoots": []          // extra writable dirs (workspace-write)
  },
  "browser": {
    "enabled": true,             // browser_* tools and /browser panel (default on)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // managed Chromium launch mode
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // watch unattended /loop and /schedule turns
    "inactivityTimeoutSecs": 900, // abort after 15 minutes without provider/tool activity
    "maxRuntimeSecs": 3600,      // absolute one-hour cap per background turn
    "abortGraceSecs": 10,        // wait for cooperative cancellation before hard-stop
    "maxRetries": 3,             // consecutive recovery attempts before exhaustion
    "initialBackoffSecs": 5,     // first retry delay
    "maxBackoffSecs": 300        // cap for exponential retry backoff
  }
}
```

> Sandbox shell komutlarını sınırlar (macOS: sandbox-exec; Linux: kurulu olması gereken `bwrap`). Yapılandırılmış sandbox doğrulanamazsa başlangıç fail-closed olur; onsuz çalışmak için açık `--no-sandbox` flag'ini kullanın. Network varsayılan olarak reddedilir. Bir komut gerçekten escape etmeye ihtiyaç duyarsa, model `dangerouslyDisableSandbox: true` ayarlar ve buna onay prompt'unda **siz** izin verirsiniz — ya da tüm sandbox'ı `/sandbox` ile canlı olarak açıp kapatın.

> `contextWindow` auto-compaction'ı sürer — onu modelinizin gerçek window'una ayarlayın (örn. `1000000`). `providers.<name>.models.<id>.contextWindow` altındaki **per-model** değeri tercih edin (o öncelik alır); yukarıdaki top-level anahtar global bir fallback'tir ve ikisi de ayarlı değilse zode onu paketlenmiş models.dev kataloğundan da doldurur. Onu gerçek window'un üzerine **ayarlamayın**: fazla tahmin request'leri taşırır ve provider turn'ü reddeder.

## Server mode ve SDK'lar

`zode server`, stdin/stdout üzerinde newline-delimited bir JSON-RPC server başlatır. Editor entegrasyonları, yerel otomasyon, testler ve TUI'yi başlatmadan zode'un mevcut yeteneklerini isteyen SDK client'ları için tasarlanmıştır.

```bash
zode server                      # stdio (varsayılan) — SDK'ların spawn ettiği
zode server --listen stdio://    # aynı şey, açıkça yazılmış
zode server --listen ws://127.0.0.1:0   # loopback WebSocket + Bearer auth
zode server --listen off         # hiçbir şey başlatma ve çık
```

Server mode, zode destekli davranışı açığa çıkarır:

- initialization + capability discovery (bir `approvalPolicy` ile: `readOnly` (varsayılan) / `auto` / `prompt`)
- thread metadata lifecycle ve **streaming turns** — model çıktısı ve tool call'ları JSON-RPC notification olarak gelir; `turn/interrupt` bir turn'ü iptal eder
- **interactive approvals** — `prompt` policy, `allow` / `allowAlways` / `deny` ile yanıtlanan server→client `approval/request` frame'lerini sürer
- filesystem read/write/create/stat/list/remove/copy ve tek seferlik `command/exec`
- model list/set, config read/list/write ve read-only skills, hooks, MCP-server status ve plugin list'leri

WebSocket transport yalnızca loopback'e bağlanır ve bir `0600` `<config-dir>/server.json` credentials dosyası (`{port, pid, token}`) yazar; client'lar `Authorization: Bearer <token>` ile authenticate olur. Tam protokol, notification field adları ve dil başına örnekler için [`sdk/README.md`](../../sdk/README.md) dosyasına bakın.

Özellikle bu app-server protokolü için hosted marketplace yönetimi, remote-control, Realtime, standalone process spawn, background terminals, thread archive/fork, goals ve app connector'lar kapsam dışıdır. Yukarıda belgelenen yerel session ve static-plugin marketplace komutları ayrı CLI yüzeyleridir.

SDK'lar [`sdk/`](../../sdk/) altında yaşar:

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

Her SDK, mevcut stabil method adları için native bir `ProtocolMethod` enum/constant seti sunar, böylece entegrasyonlar hard-coded JSON-RPC string'lerinden kaçınabilir. Desteklenen her method'un params, result şekli ve SDK enum/constant adı [`sdk/` method reference](../../sdk/README.md#method-reference) içinde belgelenmiştir.

Makinenizde mevcut olan SDK kontrollerini şununla çalıştırın:

```bash
scripts/test-sdks.sh
```

Protokol fixture'ları `zode-app-server-protocol`'den üretilir:

```bash
cargo run -p zode-app-server-protocol --bin export -- sdk/fixtures/jsonrpc
```

## Browser control

Zode, browser otomasyonu için bir `tools:browser` grubu içerir. Agent şunları kullanabilir: screenshot'lar, DOM snapshot'ları, console log'ları, network log'ları ve tab okumaları için `browser_read`; navigate, click, type, key press ve scroll için `browser_act`; JavaScript için `browser_eval`; ve tab yönetimi için `browser_tabs`. Read-only browser incelemesi gate'lenmez; durum değiştiren browser action'ları diğer yan etkili araçlarla aynı allow-once / always / deny onay akışını kullanır.

İki browser target vardır:

- **managed** — zode özel bir Chromium profile'ını başlatır ve kontrol eder.
- **bridge** — zode, [`extensions/chrome/`](../../extensions/chrome/) içindeki paketlenmiş MV3 extension aracılığıyla zaten kullandığınız Chrome profile'ını kontrol eder.

Bridge target için extension'ı `extensions/chrome` üzerinden bir kez yükleyin, sonra `/browser pair` çalıştırın. Chrome, harici programların açtığı `chrome-extension://` URL'lerini engeller (ERR_BLOCKED_BY_CLIENT — macOS, Windows ve Linux'ta aynı şekilde), bu yüzden zode'un sayfayı kendisinin açma denemesi başarısız olabilir — bunun yerine extension, `/browser pair` sonrasındaki ~30 saniye içinde kendi pairing sayfasını port önceden doldurulmuş olarak açar; sohbette gösterilen 6 haneli pairing kodunu oraya girin. Elle yedek yöntem olarak `chrome-extension://…/popup.html?port=…` URL'sini adres çubuğuna kendiniz yazabilirsiniz (elle yazılan gezinme tarayıcı tarafından başlatılmış sayılır ve engellenmez). **Pairing tek seferliktir**: extension uzun ömürlü bir token saklar ve otomatik olarak yeniden bağlanır — browser başlangıcında, extension güncellemelerinde ve bağlantı kopukken yaklaşık 30 saniyede bir retry ile — böylece zode'u yeniden başlatmak asla yeni bir pairing istemez. Çalışan bir CLI'ye yeniden bağlanır ya da gerektiğinde yalnızca-extension bir zode daemon'unu otomatik başlatır. Zode tarafından açılan tab'lar `zode` adlı bir Chrome tab group'una yerleştirilir.

### Chrome task side panel

Güncel zode CLI'yi çalıştırıp `/browser pair`'i bir kez çalıştırın. Toolbar simgesine tıklamak side panel'i açar; sonrasında hiçbir CLI process çalışmıyorken zode'u otomatik başlatır. Pairing sayfası küçük bir code/token akışı olarak kalır ve task'lar terminal odağını değiştirmeden TUI oturumlarıyla paylaşılır.

Side-panel turn'leri bridge browser araçlarını panelin yanında o an gösterilen sayfaya bağlar, böylece "bu sayfayı analiz et" gibi istekler yeni bir tab açmak yerine mevcut tab'da `browser_read` kullanır. Bağımsız TUI ve CLI browser otomasyonu `zode` tab group'undaki zode-owned tab'ları kullanmaya devam eder. Aktif sayfa aynı zamanda belirsiz side-panel prompt'ları için varsayılan bağlamdır; yerel proje dosyaları yalnızca kullanıcı açıkça sorunca incelenir.

Panel metin gönderebilir, bir model seçebilir, `readOnly`, `prompt` ve `auto` erişim modlarını seçebilir, yanıtı stream edebilir ve çalışan bir turn'ü Stop edebilir. Bir turn en fazla 8 dosya ve toplam 20 MiB ekleyebilir: her biri en fazla 5 MiB PNG, JPEG, GIF ve WebP image'ları ile her biri en fazla 1 MiB UTF-8 text ve code dosyaları. PDF, Office, arşiv, executable ve UTF-8 olmayan girdiler reddedilir.

Bir extension güncellemesinden sonra `chrome://extensions` üzerinde Reload'a tıklayın. Eski extension sürümleri browser otomasyonuyla uyumlu kalır ama task side panel'e sahip değildir. Windows'ta zode, extension URL'leri için Chrome'u varsayılan-tarayıcı shell'ini çağırmak yerine doğrudan bulup başlatır, böylece Chrome zaten kuruluyken Microsoft Store yönlendirmesini önler.

Yararlı komutlar:

```bash
/browser                         # browser control panel'i aç
/browser status                  # target/running/paired durumunu göster
/browser launch                  # managed browser'ı başlat
/browser close                   # managed browser'ı kapat
/browser pair                    # Chrome bridge extension'ı pair et veya yeniden bağla
/browser target managed          # zode'un managed Chromium'unu kullan
/browser target bridge           # extension'ı kullan ve sonraki-başlangıç varsayılanı olarak kaydet
/browser screenshot [path]       # bir browser screenshot'ı yakala
```

Extension yükleme, güncelleme, CRX paketleme ve smoke-test adımları için [`extensions/chrome/README.md`](../../extensions/chrome/README.md) dosyasına bakın.

## Desktop control

Zode, yalnızca browser'ı değil, native desktop uygulamalarını da OS accessibility API'leri üzerinden sürebilir. Agent, accessibility ağacını (pencereler, element'ler ve ref'leri) okumak için `desktop_read`, element bazında click/type/scroll ve değer set etmek için `desktop_act` ve ekranı yakalamak için `desktop_screenshot` kullanır. Read-only okumalar gate'lenmez; durum değiştiren desktop action'ları diğer yan etkili araçlarla aynı allow-once / always / deny onay akışını kullanır.

Backend'ler platform başına seçilir:

- **macOS** — Accessibility (AX) API.
- **Windows** — UI Automation (UIA).
- **Linux** — AT-SPI.
- **Electron uygulamaları** — Chrome DevTools Protocol üzerinden attach.

**Ghost cursor ve Esc stop.** Zode gerçek farenizi asla hareket ettirmez. macOS'ta zero-permission bir overlay (`zode-overlay`), her action'ın hedefine düzgün bir Dubins path boyunca uçan *sahte* bir cursor çizer, böylece agent'ın ne yaptığını takip edebilirsiniz; yazılan metin overlay'de asla gösterilmez. Desktop otomasyonu aktifken global bir **Esc**, çalışan her turn'ü keser ve overlay'i gizler (TUI'nin Esc'iyle aynı stop path'i). Diğer platformlar desktop action'larını görselleştirme olmadan çalıştırır.

US-layout keycode'u olmayan CJK ve diğer metinler sistem pasteboard'u üzerinden iletilir (yaz → paste sentezle → önceki clipboard'ı geri yükle), böylece özel key handling'e sahip uygulamalar gerçek karakterleri alır.

```bash
/desktop            # desktop target ve permission durumunu göster
/desktop status     # aynısı, açıkça
```

Config `~/.zode/config.json` içindeki `desktop.*` altında yaşar:

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor` (varsayılan `true`) macOS overlay cursor'ını çizer; `escCancel` (varsayılan `true`) otomasyon sırasında global-Esc kesintisini kurar; `overlayHelperPath` (varsayılan `null`) `zode-overlay` helper konumunu override eder — eksik bir helper görselleştirmeyi basitçe devre dışı bırakır. Desktop otomasyonu ilk kullanımda OS izni isteyebilir (örn. macOS Accessibility).

## Arka plan turn watchdog'u, /loop ve /schedule

Scheduler'a ait `/loop` ve `/schedule` turn'leri, in-process bir liveness watchdog'u altında çalışır. Provider, tool ve nested-agent aktivitesi paylaşılan source-side bir heartbeat'i tazeler; `maxRuntimeSecs` mutlak bir cap olarak kalır. Her iki timeout'ta da zode cooperative cancellation ister, `abortGraceSecs` bekler ve hâlâ drain olmamışsa yerel turn task'ını hard-stop eder. Task'ı durdurmak, scheduler slot'unu serbest bırakmaya yetmez: zode ayrıca her izlenen provider, tool, hook, subprocess reader ve nested-agent worker'ın quiescence'a ulaşmasını bekler. Bu ikinci sınıra beş saniye içinde ulaşılmazsa, tab/store karantinaya alınır, job devre dışı bırakılır ve live-attempt lease'i worker'lar gerçekten çıkana kadar tutulur.

Başarısız denemeler `initialBackoffSecs`'ten `maxBackoffSecs`'e kadar sınırlı exponential backoff kullanır. Başarılı bir turn ardışık-başarısızlık sayacını temizler; `maxRetries` tükendiğinde zode loop'u durdurur veya kalıcı schedule'ı devre dışı bırakır. Manuel kesinti, job kaldırma ve açık devre dışı bırakma, hiçbir mutasyon başlamadıysa başka bir retry yaratmak yerine bekleyen recovery'yi iptal eder. Recovery yan etkiler etrafında bilinçli olarak muhafazakârdır: zode yalnızca bir yan etki gözlemlemediğinde otomatik retry yapar; bir mutasyon zaten gerçekleşmiş olabilirse (mutasyon ortasında manuel bir iptal dâhil) job'u durdurur/devre dışı bırakır ve insan incelemesini bekler. İşi bilinçli olarak detach eden araçlar (`BashRun` veya detached bir GUI) da o turn'den sonra tekrarı durdurur. Aynı inactivity limiti claim-to-start kuyruklamasını sınırlar: meşgul bir tab veya turn preflight, sahip olunan bir occurrence'ın başlamasını engellerse, bu normal, yan-etkisiz bir watchdog başarısızlığına dönüşür ve cross-process lease'ini sonsuza dek tutmak yerine aynı sınırlı retry policy'ye girer.

Quiescence yerel bir garantidir. Bir remote MCP server, browser extension, desktop actor veya diğer external sistem tarafından zaten kabul edilmiş iş, revoke desteklemeyebilir. Böyle bir call kesilirse, zode sonucunu unresolved işaretler, scheduler job'unu devre dışı bırakır ve yeniden etkinleştirmeden önce external state'i doğrulamanızı ister.

Yapılandırma ve per-turn/retry sağlığı için `/watchdog status` kullanın. Aynı state `/tasks` içinde background shell'ler ve çalışan turn'lerin yanında görünür; claimed kuyruk yaşı ve terminal-persistence fence'leri de orada gösterilir.

Bu, mevcut zode process'i içindeki scheduler turn'leri için bir watchdog'dur. Bir OS process supervisor'ı değildir ve bir crash veya makine restart'ından sonra zode'u yeniden başlatamaz; process seviyesinde restart gerektiğinde platformunuzun service manager'ını kullanın. Kalıcı schedule'lar, per-schedule bir OS file lock ile desteklenen bir active-attempt token kaydeder. Başlangıçta, contended bir lock rahat bırakılır çünkü başka bir zode process onu hâlâ sahiplenir; tam kalıcı token'a sahip serbest bir lock, temiz olmayan bir çıkıştan kalma bir orphan'dır, bu yüzden zode o schedule'ı sessizce replay etmek yerine execution-state-unknown olarak devre dışı bırakır. Bu recovery sözleşmesi process crash'lerini kapsar. Ani güç kaybı veya arızalı donanım karşısında storage-seviyesi dayanıklılık iddia etmez ve bir OS service manager'ının yerini almaz.

Fire timestamp ve active-attempt token, kalıcı bir prompt bir tab kuyruğuna girmeden önce atomik olarak claim edilir, böylece kuyruğa alınan iş zode process'leri arasında zaten exclusive'dir. O lease prompt'la birlikte turn'e taşınır ve final transcript/index persistence boyunca tutulur. Kuyruğa alınmış bir occurrence'ı düzenlemek, kaldırmak veya devre dışı bırakmak açık bir iptaldir ve yalnızca eşleşen active token'ı temizler. Graceful application exit ise, hiç çalışmamış işi tüketemeyeceği için tam başlatılmamış fire watermark'ını veya retry token'ını geri yükler.

`/loop` ve `/schedule` bu watchdog'un sürdüğü yinelenen çalıştırmaları planlar:

```bash
/loop <interval> [--max N] <prompt>   # mevcut tab'da yinelenen bir prompt çalıştır
/loop list                            # aktif loop'ları listele
/loop stop [id]                       # bir loop'u (veya hepsini) durdur
/schedule add <when> <prompt>         # planlanmış bir prompt'u kalıcılaştır
/schedule list                        # planlanmış job'ları listele
/schedule rm <id>                     # bir schedule'ı kaldır
/schedule enable|disable <id>         # bir schedule'ı etkinleştir/devre dışı bırak
```

`<interval>` en az 30s'dir (örn. `30s`, `5m`, `1h`); `<when>` `hh:mm`, `mon hh:mm` veya `every 2h` kabul eder. Recurrence phase kanonik'tir: interval slot'ları kalıcı anchor'dan mutlak epoch aritmetiği kullanır (DST fallback dâhil), calendar schedule'lar wall-clock phase'lerini korur ve kaçırılan backlog en son due slot'a birleşir. Çalışan bir process ayrıca roster'ı tazeler, böylece remote disable/remove, retry ve orphan sahiplik değişiklikleri restart olmadan etkili olur.

### Task timing

`TurnRecorder`, `tool.completed` ve `turn.completed` run event'lerine `durationMs` damgalar (journal'lanır; eski journal'lar `None` olarak parse edilir). TUI per-tool `· 1.2s` sonekleri, bir `✓ done · 34s · 3 tools` turn footer'ı ve `/tasks` içinde humanize edilmiş geçen süre gösterir — hepsi `zode_core::duration_fmt::format_duration_ms` üzerinden.

## Sık kullanılan slash commands

| Command | İşlev |
|---|---|
| `/help` | Commands + keybindings overlay |
| `/clear` | Konuşmayı (ve context'i) temizle |
| `/model [id]` | Aktif modeli göster / not et |
| `/config` | Model + working directory göster |
| `/compact` | Context auto-compaction durumu |
| `/cost` | Şu ana kadarki token kullanımı ve maliyet (sub-agent'lar dâhil) |
| `/theme [id]` | Temayı değiştir (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Session picker — history ile yeni bir tab'a resume et |
| `/connect` | Aktif provider'ı bağla ve değiştir |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Sağ sidebar'ı göster/gizle; MCP / modified-files / todo bölümlerini katla |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Browser control panel ve komutları |
| `/desktop [status]` | Desktop target ve permission durumunu göster |
| `/loop <interval> [--max N] <prompt>` | Mevcut tab'da yinelenen bir prompt çalıştır; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Planlanmış bir prompt'u kalıcılaştır; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Background-turn watchdog yapılandırması, sağlığı ve bekleyen retry'ler |
| `/tasks` | Background shell'ler, çalışan turn'ler ve watchdog sağlık paneli |
| `/undo`, `/redo` | Son file edit'i geri al / yinele |
| `/mcp` | MCP server'ları yönet — bir dialog'da enable / disable |
| `/skills` | Mevcut skills'i listele |
| `/agents` | Sub-agent'ları yönet — oluştur (AI-destekli veya manuel) / sil |
| `/external-agents [list\|discover]` | `PATH` üzerindeki desteklenen external CLI'ları listele veya bulunan her preset'i açıkça kaydet |
| `/team [status\|board\|dismiss <name>]` | Kalıcı teammate roster'ını ve shared board'u incele veya bir teammate'i kaldır |
| `/workflows` | JS-scripted workflow'ları yönet ve çalıştır (`agent()`/`parallel()`/`pipeline()` orchestration) |
| `/effort` | Reasoning effort seviyesini seç |
| `/thinking`, `/tool-details` | Reasoning / tool-call detayı gösterimini aç/kapat |
| `/orchestration` | Autonomous sub-agent + workflow orchestration'ı aç/kapat |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | OS sandbox'ı runtime'da göster / kontrol et |
| `/language` | UI dilini değiştir (15 locale) |
| `/export [path]` | Transcript'i Markdown'a export et (bir dizin varsayılan ad alır) |
| `/yolo` | Bypass-approval modu |
| `/exit` | Çık |

Oluşturulan agent'lar ve skills ile bağlı MCP araçları da dinamik slash command (örn. `/<name>`) olarak görünür ve doğrudan çağrılabilir.

## Keybindings

> macOS'ta aşağıdaki uygulama chord'ları **`Cmd`** (⌘) kullanır; Windows/Linux'ta `Ctrl` kullanır. `Ctrl+C/D/L/V` her yerde `Ctrl` kalır (terminal geleneği).

| Key | Action |
|---|---|
| `Enter` | Mesaj gönder (bir turn çalışıyorsa kuyruğa alır) |
| `Shift`/`Alt`+`Enter` | Yeni satır |
| `Up` / `Down` | Önceki / sonraki gönderilmiş prompt'u geri çağır (veya autocomplete seçimini taşı) |
| `Ctrl+C` | Turn'ü kes (boştayken çık) |
| `Ctrl+D` | Çık |
| `Ctrl+L` | Konuşmayı store'dan yeniden çiz (boşalmış görünümü kurtarır; atmak için `/clear`) |
| `Ctrl+V` | Yapıştır (metin veya image path'leri) |
| `Cmd/Ctrl+O` | Settings |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | Yeni tab / tab kapat |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Tab'a atla / tab'lar arasında geç |
| `Cmd/Ctrl+B` | Background tasks paneli |
| `Cmd/Ctrl+G` | Sidebar'ı aç/kapat |
| `F1` | Help |
| `PgUp` / `PgDn` | Konuşmayı kaydır |
| `Home` / `End` | Konuşmanın başına / en sonuna atla |
| `Esc` | Mevcut overlay'i kapat (veya çalışan bir turn'ü kes) |

## Proje talimatları

Zode, talimatları üç seviyeli bir hiyerarşiden okur (sonraki attention kazanır): global `~/.zode/AGENTS.md` (veya `instructions.md`) → proje kökü → cwd. Her dizinde `CLAUDE.md`'den önce `AGENTS.md`'yi tercih eder. Skills `.zode/skills/**/SKILL.md` altında yaşar; MCP servers `~/.zode/mcp.json` ⊕ `.mcp.json` içinde; hooks `~/.zode/hooks.json` ⊕ `.zode/hooks.json` içinde.

**Cross-agent yapılandırma.** Zode, Claude Code, Codex, Cursor, opencode, Gemini ve ilgili yerel agent'lardan doğrudan skills ve MCP yapılandırmasını okur. Bu ürünlere ait kurulu plugin ağaçları ve plugin cache'leri asla taranmaz. Bir plugin'i yeniden kullanmak için kaynağını `zode plugin install ... --trust` ile açıkça kurun; Zode üzerinden kurulan plugin'ler için Codex ve Claude Code paket formatları desteklenmeye devam eder.

## MCP server'larını yapılandırma

MCP server'lar her şeyle aynı nested-precedence config'te yaşar — tüm projeler için `~/.zode/mcp.json`, tek bir repo'ya scope etmek için proje kökünde `.mcp.json` veya `.zode/mcp.json`. Registry yok, restart-and-pray yok: dosyayı düzenleyin, sonra almak için `/mcp` (veya relaunch).

### stdio (yerel bir server spawn et)

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

`command`/`args`, server'ı stdio üzerinden pipe'lanan bir subprocess olarak spawn eder. `env` değerleri zode'un kendi process ortamına karşı `$NAME` / `${NAME}` substitution destekler (bağlanmadan hemen önce genişletilir, diske yazılmaz) — token'ları config dosyasının dışında tutmak için kullanışlıdır.

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

`"transport": "http"`, mevcut MCP spec'inin Streamable HTTP transport'uyla bağlanır — tek bir `url`, yapılandırılacak ayrı bir SSE endpoint yok. `"sse"` eşdeğer bir yazım olarak kabul edilir (bazı config'ler — ve MCP server'ların kendi kurulum belgeleri — hâlâ öyle adlandırır); ikisi de aynı connector'a çözülür. `headers` verbatim iletilir (`Authorization` dâhil, böylece Bearer/Basic/custom şemalar çalışır) ve `env` ile aynı `$VAR` substitution'ı destekler. Herhangi bir server'a bağlanmadan tanımını korumak için `"enabled": false` ekleyin — `/mcp` bunu dosyayı elle düzenlemeden server başına da açıp kapatır.

### Kullanma

Bağlı bir server'ın sunduğu her araç `mcp__<server>__<tool>` olarak görünür ve agent tarafından herhangi bir yerleşik araç gibi çağrılabilir (ve input kutusunda `@`-mentionable). `/mcp`, keşfedilen her server'ı listeleyen bir dialog açar — connected / disconnected / disabled — Space ile birini açıp kapatmak için; sidebar'ın katlanabilir `mcp` bölümü (▼ header'ına tıklayın veya `/sidebar mcp`) aynı canlı bağlantı state'ini bir bakışta yansıtır.

Zode ayrıca Claude Code, Codex, Cursor, opencode ve Gemini'den doğrudan MCP yapılandırmasını okur. Home yapılandırması kullanıcının kurulumu olarak ele alınır; proje-yerel foreign MCP tanımları disabled olarak keşfedilir ve `/mcp` üzerinden etkinleştirilebilir. Başka bir ürünün kurulu plugin ağacına gömülü MCP bildirimleri taranmaz. `openpencil` rezervedir — op-bridge onu native olarak sürer, bu yüzden o adla bildirilen herhangi bir server yok sayılır.

## Skills ve command Markdown kurma

Her ikisi de diskteki düz Markdown'dur — registry yok, build adımı yok. Bir dosyayı içine bırakın ve bir sonraki launch'ta canlı olur (veya neyin yüklendiğini kontrol etmek için `/skills`).

### Bir skill kurma

Bir skill, içinde `SKILL.md` olan bir klasördür. Onu proje (`.zode/skills/`) veya home dizininiz (`~/.zode/skills/`) altına koyun:

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

Skill artık `/skills` içinde görünür, agent onu Skill aracı üzerinden kendi başına çağırabilir ve dinamik bir slash command olur — `/code-review look at src/lib.rs` yazmak, skill'i çalıştıran bir prompt'a genişler. `SKILL.md` yanındaki ekstra dosyalar (referanslar, script'ler) skill ile birlikte gelir. Claude Code, Codex, opencode, Cursor ve ilgili agent'lara ait doğrudan skills dizinleri taranır. Bu ürünlerin kurulu plugin ağaçlarına veya cache'lerine gömülü skills taranmaz; burada kullanmak isterseniz plugin'i açıkça Zode üzerinden kurun.

### Bir command kurma (prompt Markdown)

Custom bir slash command, **dosya adı command adı olan** ve gövdesi gönderdiği prompt olan tek bir `.md` dosyasıdır. Command'dan sonra yazdığınız her şey gövdeye eklenir:

```bash
mkdir -p .zode/commands            # veya tüm projeler için ~/.zode/commands
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

Artık `/changelog` o prompt'u gönderir ve `/changelog only the sidebar work` argümanlarınızı sonuna ekler. `~/.claude/commands` ve `~/.codex/commands` (ve proje seviyesi eşdeğerleri) içindeki command'lar da yüklenir; bir *foreign plugin tree* içindeki command'lar varsayılan olarak kapalıdır — opt-in için `.md`'yi bir `.zode/commands/` dizinine kopyalayın.

## ZSeven-W Ekosistemi

Zode, ZSeven-W'nin AI-native geliştirme araçları için daha geniş bir stack'inin parçasıdır:

| Product | Nedir |
|---------|-------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | LLM agent'ları göndermek için pure-Rust async runtime: multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions ve opsiyonel coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Bir `.op` dosyasının bir app olduğu Rust-native cross-platform UI framework; OpenPencil-tarzı design artifact'lerini runnable software'e bağlar. |
| [`noema`](https://github.com/ZSeven-W/noema) | Coding agent'ları için local-first, non-vector memory system; lexical recall, review queues, MCP access, S3 offload ve enterprise policy controls içerir. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Design-as-code workflow'ları için open-source AI-native vector design tool; prompt'ları live canvas üzerinde doğrudan UI'a dönüştürür ve concurrent agent teams destekler. |

## Benchmark

Zode benchmark'ları one-shot code generation, agentic read/run/edit/fix, multi-file tasks, tricky bugs, MCP/Skills/constraint following ve Noema LOCOMO runner'ını kapsar. Beş boyutta **Zode + DeepSeek-v4-pro Claude ile eşleşir**, her task *gizli* bir grader tarafından puanlanır. Tam methodology, reproduction komutları ve sonuç tabloları [İngilizce README'nin Benchmark bölümünde](../../README.md#benchmark); suite'ler [`benchmarks/`](../../benchmarks/) içinde yaşar.

## Development

```bash
cargo build --workspace                 # her şeyi build et
cargo run -p zode                       # TUI'yi çalıştır
cargo run -p zode -- -p "<prompt>"      # headless single turn
cargo test --workspace                  # tüm testler
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check                        # licenses / advisories / bans
```

## Contributing

Katkılar memnuniyetle karşılanır. Lütfen [Conventional Commits](https://www.conventionalcommits.org/) formatını kullanın: `<type>(<scope>): <subject>`; yaygın scope'lar `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
