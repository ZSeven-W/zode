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
  <strong>Asisten coding open-source dan AI-native untuk terminal.</strong><br/>
  Membaca kode, menjalankan perintah, mencari file, dan mengelola git dari TUI Rust yang cepat.
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

> README lokal ini mencakup gambaran produk dan quick start. [README bahasa Inggris](../../README.md) tetap menjadi sumber utama untuk detail benchmark lengkap dan catatan panjang terbaru.

## Sorotan

- **Multi-provider**: Anthropic, OpenAI, dan API apa pun yang kompatibel dengan OpenAI (dialek DeepSeek, Moonshot, OpenRouter), serta Ollama lokal. Mendukung model dengan output besar dan **konteks 1M** (`contextWindow` / `maxOutputTokens` dapat dikonfigurasi).
- **Tool surface yang kaya**: baca/tulis/edit file (termasuk `MultiEdit` multi-hunk yang atomik), pencarian kode dan konten, foreground/background shell, git, web fetch (plus `WebSearch` opsional dengan Tavily key), notebook, dan TODO tracking.
- **Kontrol browser**: tool `browser_*` bawaan dapat mengendalikan managed Chromium atau profil Chrome asli Anda melalui ekstensi Chrome bridge zode: navigasi, klik/ketik, inspeksi DOM, tangkap screenshot, baca log console/network, dan kelompokkan tab yang dibuka zode. Pairing hanya dilakukan sekali — ekstensi menyambung ulang secara otomatis di antara restart zode.
- **Kontrol desktop**: tool `desktop_read`/`desktop_act`/`desktop_screenshot` menggerakkan aplikasi native lewat API accessibility OS (macOS AX / Windows UIA / Linux AT-SPI / Electron CDP), lengkap dengan ghost cursor dan penghenti Esc global.
- **Permission non-blocking**: setiap tool yang mengubah state melewati gate (allow once / always / deny), tetapi prompt-nya tampil inline dan tidak pernah memblokir Anda: Anda bisa terus mengetik untuk mengantrekan lanjutan sementara sebuah tool menunggu, disertai aturan hard-deny.
- **OS sandbox, aktif secara default**: perintah shell berjalan di bawah sandbox-exec (macOS) / bwrap (Linux) dalam mode `read-only` atau `workspace-write`, dengan **outbound network ditolak secara default**. Ubah live dengan `/sandbox`; model dapat meminta escape untuk satu perintah (`dangerouslyDisableSandbox`) yang **Anda otorisasi** di prompt.
- **TUI layar penuh**: streaming Markdown dengan syntax highlighting, diff preview, autocomplete slash-command, riwayat prompt (Up/Down), 11 tema bawaan, overlay settings & help, section sidebar kanan yang tangguh, dan **UI 15 bahasa** (`/language`).
- **Session persisten dan kompatibel V1**: mempertahankan kontrak transcript `<id>.jsonl` yang ada sambil menambahkan journal, checkpoint, rewind, fork, dan Git worktree terisolasi sebagai data sidecar. Kompaksi konteks tidak pernah menghilangkan percakapan yang terlihat — session yang di-resume memutar ulang riwayat lengkap pra-kompaksi sementara konteks model tetap ringkas.
- **Permukaan otomasi**: output headless JSON/JSONL yang stabil, penargetan session yang eksak, filter tool, exit code deterministik, ACP melalui stdio, dan dashboard operasi lokal.
- **Tab multi-session**: jalankan beberapa percakapan berdampingan (`Ctrl+T`), masing-masing agent terisolasi; resume session lama dengan replay riwayat penuh.
- **Sub-agents, team & workflows**: delegasikan pekerjaan sekali jalan lewat tool Task, rekrut teammate internal atau CLI eksternal yang persisten, koordinasikan mereka dengan board bersama dan klaim file, serta kelola lewat `/agents`, `/team`, dan `/workflows`.
- **Konfigurasi lokal yang portabel**: membaca skills dan konfigurasi MCP langsung dari Claude Code, Codex, Cursor, opencode, dan Gemini, tanpa pernah mengimpor pohon plugin atau cache yang mereka instal.
- **Skills & MCP**: muat paket instruksi `SKILL.md` sesuai kebutuhan dan hubungkan MCP server (`mcp__<server>__<tool>`); agents, skills, dan tool MCP yang dibuat muncul sebagai slash command.
- **Hooks**: jalankan script eksternal pada event tool (mis. blokir perintah berbahaya, lint setelah edit).
- **Instruksi tiga tingkat**: global (`~/.zode/`) → root project → cwd (`AGENTS.md` / `CLAUDE.md`).

## Instalasi

### Satu baris (binary prebuilt)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer mendeteksi OS + CPU Anda, mengunduh binary yang sesuai dari [release](https://github.com/ZSeven-W/zode/releases) terbaru, lalu menaruh `zode` di `PATH`. Pin sebuah versi atau ubah lokasinya:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

### Download manual

Ambil archive platform Anda dari [halaman releases](https://github.com/ZSeven-W/zode/releases):

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Ekstrak lalu pindahkan `zode` ke `PATH` (`sudo mv zode /usr/local/bin/`). Build Linux memakai glibc; binary macOS tidak ditandatangani (`xattr -dr com.apple.quarantine ./zode` bila Gatekeeper mengeluh).

### Build dari source

Butuh Rust 1.88 atau yang lebih baru:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binary di target/release/zode
```

> Agent runtime berada di git submodule `vendor/agent` — selalu clone dengan
> `--recurse-submodules` (atau jalankan `git submodule update --init`).

## Quick Start

Cara termudah adalah menjalankan `zode` lalu **`/connect`** — sebuah picker
interaktif berbasis models.dev yang menuliskan konfigurasi untuk Anda.

Untuk menulis `~/.zode/config.json` secara manual: **`providers`** adalah
sumber kebenaran — satu entri per provider (kredensial bersama) yang memuat
satu atau beberapa **model** — dan **`provider`** di level atas mencatat model
yang *aktif*:

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // wire protocol: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // model aktif
}
```

Provider yang kompatibel dengan OpenAI (DeepSeek, Moonshot, OpenRouter, …)
menambahkan `baseUrl` + `dialect`, dan pengaturan per-model berada di entri
tiap model:

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

Satu entri provider dapat memuat beberapa model — beralih di antaranya secara live dengan `/model`.

Lalu jalankan:

```bash
zode                       # TUI layar penuh
zode -p "explain main.rs"  # headless: satu prompt, stream ke stdout, keluar
zode --no-tui              # readline REPL biasa
zode -c                    # lanjutkan session terakhir
zode -r <id>               # resume session berdasarkan prefix id
zode --yolo                # lewati prompt approval (aturan deny tetap berlaku)
zode --no-sandbox          # nonaktifkan OS sandbox (aktif secara default)
zode --sandbox-read-only   # sandbox mode read-only (tolak semua tulis)
zode --sandbox-allow-network  # izinkan outbound network di dalam sandbox
zode --browser             # paksa aktifkan tool browser bawaan untuk run ini
zode --no-browser          # nonaktifkan tool browser bawaan untuk run ini
zode --model <id>          # timpa model
zode --provider <name>     # pilih provider bernama dari config.providers
zode server                # mode JSON-RPC app-server melalui stdio
zode acp                   # agent Agent Client Protocol melalui stdio
zode dashboard             # ringkasan sessions/checkpoints/worktrees lokal
```

Anda juga bisa mengarah ke provider mana pun tanpa mengedit config dengan
mengekspor key yang sesuai (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); untuk
Ollama, `baseUrl` diambil dari environment bila tidak diset.

## Mendaftarkan teammate CLI eksternal secara manual

Zode dapat memakai agent CLI pihak ketiga yang terpasang sebagai Task worker
sekali jalan atau sebagai teammate persisten maupun stateless. Pendaftaran
sengaja dibuat manual: memasang sebuah CLI atau meletakkannya di `PATH`
**tidak** mengeksposnya ke model. Tambahkan sebuah profile di bawah
`externalAgents.agents`, lalu jalankan Zode di dalam project. Atau jalankan
`/external-agents` untuk memeriksa CLI yang didukung dan saat ini ada di
`PATH`, lalu `/external-agents discover` untuk menambahkan setiap preset yang
terdeteksi ke config global secara eksplisit. Perintah ini dipicu pengguna;
startup tidak pernah memindai atau mendaftarkan CLI eksternal secara otomatis.

| Profile | Command | Task | Mode team | Sandbox CLI eksternal |
|---|---|---:|---:|---|
| `claude-code` | `claude` | ya | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | ya | persistent | workspace-write |
| `opencode` | `opencode` | ya | stateless | unknown |
| `cline` | `cline` | ya | stateless | unrestricted |
| `antigravity` | `agy` | ya | stateless | unknown |
| `cursor` | `cursor-agent` | ya | persistent | unrestricted |
| `kiro` | `kiro-cli` | ya | stateless | unrestricted |
| `pi` | `pi` | ya | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | ya | persistent | unrestricted |

Setiap profile yang terdaftar dapat bergabung ke team. Profile yang resumable
mempertahankan session ID CLI dan percakapan lintas penugasan; CLI lain adalah
teammate stateless yang memulai process baru untuk tiap penugasan. Preset
memakai antarmuka headless yang terdokumentasi dari [Cline](https://docs.cline.bot/usage/cli-overview),
[Antigravity](https://antigravity.google/docs/cli-best-practices),
[Cursor](https://cursor.com/docs/cli/headless),
[Kiro](https://kiro.dev/docs/cli/headless/), [Pi](https://pi.dev/docs/latest), dan
[Grok Build](https://docs.x.ai/build/cli/headless-scripting) dari xAI. Tool
lain, termasuk CLI Grok alternatif, dapat memakai custom profile.

### Menambahkan profile CLI secara manual

Letakkan `externalAgents` di `~/.zode/config.json` untuk semua project, atau
di `<project>/.zode/config.json` untuk satu project. Object kosong secara
eksplisit mengaktifkan preset yang dikenal dan me-resolve executable-nya di
`PATH` yang sudah disanitasi:

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

Tambahkan hanya profile yang ingin Anda ekspos. Sebuah `command` telanjang
seperti `cline` di-resolve di `PATH`; path seperti `./tools/my-agent` atau
`/opt/agents/my-agent` juga diterima. Preset yang dikenal menghormati
`enabled`, `command`, `extraArgs`, `envAllow`, dan `trusted`; `extraArgs`
ditambahkan ke invokasi preset Zode.

Process CLI dimulai dengan environment yang bersih hanya berisi `PATH`,
`HOME`, dan `TERM` (plus variabel Windows yang diperlukan), jadi tambahkan API
key atau variabel wajib lainnya ke `envAllow` secara eksplisit. Login state
yang ada di bawah `HOME` tetap berfungsi. Sebuah entri project dengan nama
profile yang sama menggantikan seluruh entri global, jadi ulangi setiap
override yang masih dibutuhkan project.

Sebuah custom profile mendeklarasikan invokasi dan protokol lengkap:

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

`promptTransport` bernilai `stdin`, `argv`, atau `file`; `argv` memerlukan
argumen `{prompt}` tersendiri dan `file` memerlukan `{prompt_file}`. `output`
bernilai `text`, `jsonl` generik, `jsonl-claude`, atau `jsonl-codex`. Profile
JSONL generik memakai pointer RFC 6901 `textSource` dan `sessionIdSource`
untuk mengekstrak text yang di-stream dan session ID yang resumable dari event
mana pun. `resumeArgs` wajib memuat token `{session_id}` tersendiri dan
ditambahkan pada turn berikutnya; `resumeFlag` tetap dipertahankan sebagai
bentuk singkat `<flag> <session-id>`.

Jika sebuah CLI menerima session ID yang dipilih pemanggil, `newSessionArgs`
dapat memuat token `{session_id}` tersendiri. Zode membuat sebuah UUID,
menambahkan argumen yang diperluas pada run pertama, dan memakai `resumeArgs`
pada penugasan berikutnya. Ini juga membuat CLI plain-text menjadi resumable
tanpa perlu mem-parse ID dari output-nya.

Ini memungkinkan CLI headless mana pun menjadi Task worker atau teammate
stateless. Untuk menjaga konteks percakapan antar penugasan team, ia harus
mengekspos session ID, atau menerima satu melalui `newSessionArgs`, plus
invokasi resume non-interaktif.

`effectiveSandbox` menerima `none`, `readOnly`, `workspaceWrite`,
`unrestricted`, atau `unknown` dan ditampilkan di prompt trust.

### Merekrut dan bekerja dengan teammate

Minta leader dengan bahasa biasa; `team_hire` dan `team_send` adalah tool yang
dihadapkan ke model, bukan slash command:

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

Hire pertama menampilkan executable dan argumen yang di-resolve, working
directory, serta sandbox efektif CLI. Menyetujuinya mendelegasikan pekerjaan
ke process itu di project saat ini: Zode melakukan gate pada peluncuran
process, tetapi **tidak** melakukan gate pada tiap edit file atau perintah
shell yang dilakukan CLI eksternal. Trust grant berlaku selama session Zode
saat ini; roster persisten dipulihkan dari `<cwd>/.zode/team/`, tetapi
teammate eksternal harus dipercaya ulang setelah restart atau perubahan
executable.

Pada run non-interaktif/bypass (termasuk `--yolo`), Zode tidak dapat
menampilkan prompt trust dan gagal secara tertutup. Set
`externalAgents.agents.<profile>.trusted` menjadi `true` hanya bila Anda
sengaja ingin profile itu berjalan tanpa prompt.

Gunakan `/team` untuk memeriksa roster dan board setelah merekrut:

```text
/team                         # panel roster + board
/team status                  # roster teks
/team board                   # tujuan bersama, catatan, penugasan, dan klaim
/team dismiss implementer     # hapus teammate
```

## Panduan fitur baru

### Headless terstruktur

`-p`, `--prompt-file`, dan `--prompt-json` memakai engine headless yang sama.
`json` memancarkan satu object hasil final; `stream-json` memancarkan satu
object JSON `zode.run-event.v1` per baris. Mode terstruktur mencadangkan
stdout untuk output yang dapat dibaca mesin dan memakai exit code yang stabil:
`0` sukses, `10` error provider, `11` permission ditolak, `12` batas turn
tercapai, `13` terinterupsi (Ctrl-C), `14` hasil parsial, `15` error
penargetan session.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# ID eksak tidak melakukan pencocokan prefix. Sebuah fork tidak pernah mengubah session sumbernya.
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Pola deny tool menang atas pola allow dan diwarisi oleh Task sub-agent.
`--permission-mode` menerima `default`, `dont-ask`, `accept-edits`, dan
`bypass`; `--yolo` tetap menjadi pintasan untuk bypass, sementara aturan hard
deny tetap berlaku.

### Session kompatibel V1, checkpoint, dan worktree

Transcript tetap berupa file V1 asli di `~/.zode/sessions/<id>.jsonl`. Itu
adalah **satu-satunya** salinan transcript, sehingga klien Zode lama tetap
dapat membaca dan menulisnya. Metadata baru bersifat aditif dan berada di
`~/.zode/sessions/<id>/` (`meta.json`, journal, checkpoint, dan snapshot).
Tidak ada format session baru atau migrasi transcript yang diperlukan.

```bash
zode session list
zode session list --json
zode session show <id>                         # metadata + ID checkpoint
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # preview yang sadar konflik
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

Sebuah checkpoint ditangkap sebelum turn yang mengubah state. Rewind
memulihkan isi file yang di-track dan prefix transcript, melaporkan konflik
alih-alih menimpa perubahan yang lebih baru, dan mencatat cabang journal logis
baru alih-alih menghapus riwayat. Fork worktree dapat di-apply-back secara
eksplisit ketika eksperimen sudah siap.

**Kompaksi tidak pernah menghilangkan percakapan yang terlihat.** Saat
kompaksi konteks mengganti pesan lama dengan ringkasan, aslinya disimpan
dalam sidecar aditif (`~/.zode/sessions/<id>/compacted.jsonl`). Me-resume
session, menekan `Ctrl+L`, `/export`, dan side panel Chrome semuanya
menampilkan riwayat lengkap pra-kompaksi, sementara model tetap hanya
menerima konteks yang sudah dikompaksi. Fork membawa arsip itu (difilter ke
transcript-nya sendiri), `/clear` menghapusnya, dan menghapus session
menghapus seluruh sidecar.

### Aturan permission dan profile sandbox

Aturan dapat berada di bawah `permissions.rules` di `config.json`, atau dalam
file JSON tersendiri yang di-pass dengan `--rules`. Matcher field memakai JSON
pointer RFC 6901; deny lebih diutamakan daripada ask, yang lebih diutamakan
daripada allow. File tersendiri harus berupa array aturan atau
`{ "rules": [...] }`; ia tidak dibungkus object `permissions` di level atas.

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

Profile bawaan adalah `read-only`, `workspace`, `workspace-network`, dan
`unconfined`. Profile yang didefinisikan di config memakai field sandbox yang
sama seperti di atas.

### Plugin dan static marketplace

Sebuah plugin terkelola dapat menyumbangkan skills, commands, agents, hooks,
MCP server, LSP server, dan renderer UI JavaScript yang tersandbox. Zode
menerima `plugin.json`, `.zode-plugin/plugin.json`,
`.codex-plugin/plugin.json`, `.grok-plugin/plugin.json`, dan
`.claude-plugin/plugin.json`. Array path komponen Codex dan Claude Code
didukung, dan `defaultEnabled` milik Claude Code dihormati saat instalasi
pertama. Komponen khusus host seperti apps/connectors Codex serta themes,
monitors, atau output styles Claude Code diabaikan; plugin yang hanya berisi
app ditolak karena tidak punya komponen yang kompatibel dengan Zode.
Instalasi adalah snapshot immutable dengan provenance dan SHA-256 tree hash.
Konten plugin yang executable tidak pernah diaktifkan tanpa flag `--trust`
yang eksplisit.

#### Quick start plugin UI JavaScript

Plugin UI terkecil berisi sebuah manifest dan satu file JavaScript:

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

Instal direktori lokal atau repository/subdirektori GitHub, lalu restart
process Zode yang sedang berjalan agar ia memuat snapshot baru:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Gunakan `zode plugin update my-plugin` setelah mengubah source. `--trust`
wajib karena JavaScript, hooks, MCP server, dan akses network yang
dideklarasikan adalah kapabilitas yang executable. Install dan update mencetak
grant permission yang dideklarasikan plugin (host network, env var, context
scope). Sebuah update yang manifest-nya meminta permission *lebih luas*
daripada snapshot terpasang akan ditolak kecuali Anda menjalankannya ulang
dengan `--trust` — sumber Git yang bergerak tidak dapat memperlebar grant-nya
sendiri secara diam-diam.

#### API render UI

Plugin UI dapat menyumbangkan baris deklaratif tepat di atas versi sidebar —
maksimal enam baris total, dibagi di antara semua plugin sesuai urutan muat.
Deklarasikan entrypoint JavaScript di manifest:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

Daftarkan renderer sinkron dengan `zode.ui.sidebar`. Context-nya adalah
snapshot JSON read-only yang berisi field terminal, session, model, status,
token, dan context-window. Hasilnya dirender oleh Zode; script tidak menerima
bridge filesystem, network, terminal, atau Ratatui.

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

Tone yang didukung adalah `default`, `muted`, `accent`, `success`, `warning`,
dan `danger`; span juga menerima `bold` dan `italic`. Renderer harus sinkron.
Tiap script dibatasi 256 KiB, 8 MiB memori JS, dan 25 ms per evaluasi, serta
renderer dievaluasi ulang paling cepat setiap 250 ms (output cache dipakai
ulang di antara evaluasi). Output sidebar dibatasi 6 baris per renderer (6
total lintas plugin), tiap baris 16 span dan 2.048 byte text. Karakter kontrol
disanitasi oleh host.

Status bar juga dapat diperluas. Ia tetap satu baris ketika tidak ada plugin
yang mengembalikan konten dan tumbuh menjadi dua baris secara dinamis ketika
renderer `zode.ui.statusLine` sinkron mengembalikan span. Zode menjaga status
inti dan indikator keamanannya di baris pertama; output plugin disusun di
baris kedua.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Context render dan permission

Setiap renderer menerima field dasar berikut tanpa perlu meminta permission
context tambahan:

| Field | Bentuk dan makna |
| --- | --- |
| `ctx.apiVersion` | Versi Context API; saat ini `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | `{ width, height }` dalam sel terminal. |
| `ctx.session` | `{ id, title, cwd, busy }` untuk task aktif. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` berisi `{ enabled, readOnly, network }`. |
| `ctx.tokens` | Penghitung token `{ input, output }`. |
| `ctx.context` | `{ used, window, usedPercent }`; persentase bisa `null`. |
| `ctx.data` | Hasil yang hanya milik data source yang didaftarkan plugin ini. |

Section yang lebih kaya dihilangkan kecuali plugin meminta scope yang cocok di
`permissions.context`:

| Scope | Field yang diekspos | Bentuk dan batasan |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` berbasis satu. |
| `workspace` | `ctx.workspace.modifiedFiles` | Hingga 50 entri Git `{ path, added, removed }`. |
| `tools` | `ctx.tools.available` | Nama tool yang aktif untuk task aktif, terurut. |
| `tools` | `ctx.tools.active` | Nama tool yang sedang berjalan. |
| `tools` | `ctx.tools.recent` | Hingga 20 record `{ name, status, durationMs }`. |
| `tasks` | `ctx.tasks.todoStatuses` | Hanya string status todo, tanpa teks todo. |
| `tasks` | `ctx.tasks.subagents` | Record `{ type, status }`, tanpa prompt atau transcript. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, tanpa teks goal. |
| `services` | `ctx.services.mcp` | Record `{ name, connected }`. |
| `services` | `ctx.services.lsp` | Record `{ language, running }`. |

Contohnya:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` adalah API observasi: ia memberi tahu renderer tool apa yang ada
dan tool mana yang sedang atau pernah berjalan. Plugin UI tidak dapat memanggil
sebuah tool. Input tool, output tool, prompt, isi transcript, teks todo/goal,
nilai environment, dan kredensial tidak disertakan, dan API tidak dapat
melewati sistem approval Zode.

#### Data HTTP latar belakang

Plugin UI juga dapat mendaftarkan data source HTTP latar belakang. Akses
network dan secret harus dideklarasikan di manifest:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

Request bersifat deklaratif dan berjalan di luar jalur render. Variabel
environment secret disusun menjadi header oleh Zode dan tidak pernah diekspos
ke JavaScript:

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

`zode.data.define(key, config)` menerima key alfanumerik, underscore, atau
hyphen sepanjang 1–64 karakter. `request` mendukung `url`, `method`,
`headers`, `body` JSON opsional, dan `timeoutMs`. Default-nya adalah `GET`,
timeout 3 detik, dan refresh 60 detik. Hanya HTTPS `GET` dan `POST` yang
diterima. Header literal berupa string; header secret memakai
`{ "env": "NAME", "prefix": "Bearer " }`. Variabel environment itu juga harus
muncul di `permissions.env`, dibaca hanya oleh Rust saat menyusun request, dan
tidak pernah dikembalikan ke JavaScript.

Zode menonaktifkan redirect dan proxy, memvalidasi serta men-pin alamat DNS
publik, menolak localhost/private network, membatasi respons pada 256 KiB,
membatasi timeout request pada 500 ms–10 detik, dan membatasi interval refresh
pada 10 detik–1 jam. Wildcard seperti `*.example.com` mencocokkan subdomain
tetapi bukan host `example.com` telanjang.

Setiap plugin hanya melihat datanya sendiri. `ctx.data.<key>` berisi
`{ ok, status, data, updatedAt }` atau `{ ok: false, error, updatedAt }`.
Respons JSON menjadi object/array; respons non-JSON menjadi string. Status
error HTTP tetap menyertakan `status` dan `data`, dengan `ok: false`.

Jalankan Zode dengan secret yang diperlukan di environment-nya saat memakai
kuota privat atau API coding-plan:

```bash
CODING_PLAN_TOKEN=... zode
```

[Contoh lengkap yang dapat dijalankan](../../examples/plugins/zode-ui-demo/)
menampilkan aktivitas model/context/tool di sidebar dan status line serta
memakai `zode.data.define` untuk kuota GitHub API publik.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# Sebuah marketplace adalah indeks statis lokal/Git, bukan layanan yang di-host Zode.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # perjelas bila perlu
zode plugin marketplace update
```

### ACP, dashboard, OTLP, dan test PTY

`zode acp` mengimplementasikan ACP initialize/new/load/fork/prompt/cancel
melalui stdio, men-stream update message/thought/tool, meminta permission
melalui client, dan menerima MCP server stdio, HTTP, dan SSE yang disuplai
client. Data session memakai store kompatibel V1 yang sama dengan TUI dan CLI
headless.

```bash
zode acp
zode dashboard
zode dashboard --json
```

Ekspor OTLP dinonaktifkan secara default dan memerlukan opt-in eksplisit. Ia
hanya mengekspor atribut lifecycle/nama-tool/status/usage yang bebas konten:
prompt, teks yang dihasilkan, input/output tool, path file, dan pesan error
tidak pernah dikirim.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

Untuk skenario regresi TUI di terminal nyata, workspace menyertakan harness
PTY + VT100 yang merekam raw diagnostics dan snapshot layar virtual:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` menggerakkan terminal nyata dengan urutan wait, input tombol,
resize, dan snapshot (notasi tombol mendukung `<Enter>`, `<Esc>`, `<Tab>`,
`<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>`, dan
`<C-l>`):

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

Implementasi lokal/terbuka ini sengaja tidak menyertakan akun, penagihan, atau
layanan marketplace cloud yang dioperasikan Zode khusus xAI.

## Konfigurasi

`providers` adalah sumber untuk provider model; `provider` di level atas
menunjuk model aktif. Provider kompatibel OpenAI biasanya memerlukan `baseUrl`
dan `dialect`. Key top-level opsional (semuanya punya default yang wajar):

```jsonc
{
  "maxOutputTokens": 16384,      // batas output per-turn (naikkan untuk penulisan file besar)
  "contextWindow": 1000000,      // context window model — set 1000000 untuk model 1M
  "temperature": 0,              // makin rendah makin deterministik
  "language": "id",              // bahasa UI (15 locale); juga via /language
  "effort": "medium",            // reasoning effort; di Anthropic, medium/high dipetakan ke thinking budget nyata
  "autonomousOrchestration": true, // orkestrasi sub-agent + workflow (default aktif)
  "subagentMaxIterations": 0,      // guard anak opsional; dihilangkan/0 = tak terbatas
  "tools": {
    "deferNonCore": false        // true: pertahankan ~20 tool sehari-hari tetap terlihat, tunda sisanya di balik ToolSearch
  },
  "webSearch": {
    "tavilyApiKey": null         // mengaktifkan tool WebSearch (atau set $TAVILY_API_KEY)
  },
  "sandbox": {
    "enabled": true,             // OS sandbox untuk perintah shell (default aktif)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // izinkan outbound network di dalam sandbox
    "writableRoots": []          // dir tambahan yang dapat ditulis (workspace-write)
  },
  "browser": {
    "enabled": true,             // tool browser_* dan panel /browser (default aktif)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // mode peluncuran managed Chromium
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // pantau turn /loop dan /schedule tanpa pengawasan
    "inactivityTimeoutSecs": 900, // abort setelah 15 menit tanpa aktivitas provider/tool
    "maxRuntimeSecs": 3600,      // batas absolut satu jam per turn latar belakang
    "abortGraceSecs": 10,        // tunggu pembatalan kooperatif sebelum hard-stop
    "maxRetries": 3,             // percobaan recovery berturut sebelum habis
    "initialBackoffSecs": 5,     // penundaan retry pertama
    "maxBackoffSecs": 300        // batas backoff retry eksponensial
  }
}
```

> Sandbox mengurung perintah shell (macOS: sandbox-exec; Linux: `bwrap`, yang
> harus terpasang). Startup gagal secara tertutup bila sandbox yang
> dikonfigurasi tidak dapat diverifikasi; gunakan flag `--no-sandbox`
> eksplisit untuk berjalan tanpanya. Network ditolak secara default. Bila
> sebuah perintah benar-benar perlu escape, model menyetel
> `dangerouslyDisableSandbox: true` dan **Anda** mengotorisasinya di prompt
> approval — atau ubah seluruh sandbox secara live dengan `/sandbox`.

> `contextWindow` menggerakkan auto-compaction — set ke window sebenarnya
> model Anda (mis. `1000000`). Utamakan nilai **per-model** di bawah
> `providers.<name>.models.<id>.contextWindow` (nilai itu diprioritaskan); key
> top-level di atas adalah fallback global, dan zode juga mengisinya dari
> katalog models.dev bawaan bila keduanya tidak diset. **Jangan** setel di
> atas window sebenarnya: overestimasi membuat request meluap dan provider
> menolak turn.

## Server mode dan SDK

`zode server` memulai server JSON-RPC yang dipisahkan newline di
stdin/stdout. Ia ditujukan untuk integrasi editor, otomasi lokal, test, dan
SDK client yang menginginkan kapabilitas zode yang ada tanpa meluncurkan TUI.

```bash
zode server                      # stdio (default) — yang di-spawn SDK
zode server --listen stdio://    # hal yang sama, dituliskan lengkap
zode server --listen ws://127.0.0.1:0   # WebSocket loopback + auth Bearer
zode server --listen off         # tidak memulai apa pun lalu keluar
```

Mode server mengekspos perilaku yang didukung zode:

- inisialisasi + penemuan kapabilitas (dengan `approvalPolicy` berupa
  `readOnly` (default) / `auto` / `prompt`)
- lifecycle metadata thread dan **streaming turns** — output model dan
  pemanggilan tool tiba sebagai notifikasi JSON-RPC; `turn/interrupt`
  membatalkan turn
- **approval interaktif** — kebijakan `prompt` menggerakkan frame
  server→client `approval/request` yang dijawab dengan `allow` / `allowAlways`
  / `deny`
- filesystem read/write/create/stat/list/remove/copy dan `command/exec` sekali
  jalan
- model list/set, config read/list/write, serta skills, hooks, status MCP
  server, dan daftar plugin yang read-only

Transport WebSocket mengikat loopback saja dan menulis file kredensial `0600`
`<config-dir>/server.json` (`{port, pid, token}`); client mengautentikasi
dengan `Authorization: Bearer <token>`. Lihat [`sdk/README.md`](../../sdk/README.md)
untuk protokol lengkap, nama field notifikasi, dan contoh per-bahasa.

Khusus protokol app-server ini, manajemen marketplace ter-host, remote
control, Realtime, spawn process mandiri, terminal latar belakang, thread
archive/fork, goals, dan app connector tetap di luar cakupan. Perintah session
lokal dan static-plugin marketplace yang didokumentasikan di atas adalah
permukaan CLI terpisah.

SDK berada di bawah [`sdk/`](../../sdk/):

| SDK | Direktori | Test lokal |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

Setiap SDK mengekspos set enum/konstanta `ProtocolMethod` native untuk nama
metode stabil saat ini, sehingga integrasi dapat menghindari string JSON-RPC
yang di-hardcode. Params, bentuk result, dan nama enum/konstanta SDK setiap
metode yang didukung didokumentasikan di [referensi metode `sdk/`](../../sdk/README.md#method-reference).

## Kontrol browser

Zode menyertakan grup `tools:browser` untuk otomasi browser. Agent dapat
memakai `browser_read` untuk screenshot, snapshot DOM, log console, log
network, dan pembacaan tab; `browser_act` untuk navigasi, klik, mengetik,
menekan tombol, dan scroll; `browser_eval` untuk JavaScript; dan
`browser_tabs` untuk manajemen tab. Inspeksi browser read-only tidak di-gate;
aksi browser yang mengubah state memakai alur approval allow-once / always /
deny yang sama seperti tool lain yang berefek samping.

Ada dua target browser:

- **managed** — zode meluncurkan dan mengendalikan profil Chromium khusus.
- **bridge** — zode mengendalikan profil Chrome yang sedang Anda pakai melalui
  ekstensi MV3 bawaan di [`extensions/chrome/`](../../extensions/chrome/).

Untuk target bridge, muat ekstensi sekali dari `extensions/chrome`, lalu
jalankan `/browser pair`. Chrome memblokir URL `chrome-extension://` yang
dibuka oleh program eksternal (ERR_BLOCKED_BY_CLIENT — sama saja di macOS,
Windows, dan Linux), sehingga upaya zode sendiri membuka halaman itu bisa
gagal — sebagai gantinya, ekstensi sendiri membuka halaman pairing-nya
dalam ~30 detik setelah `/browser pair`, dengan port sudah terisi; masukkan
kode pairing 6 digit yang ditampilkan di chat. Sebagai fallback manual,
ketik sendiri URL `chrome-extension://…/popup.html?port=…` di address bar
(navigasi yang diketik manual dianggap dimulai oleh browser dan
diizinkan). **Pairing hanya
dilakukan sekali**: ekstensi menyimpan token jangka panjang dan menyambung
ulang secara otomatis — saat browser dinyalakan, saat ekstensi diperbarui,
dan dengan percobaan ulang kira-kira setiap 30 detik selama terputus —
sehingga
me-restart zode tidak pernah meminta pairing lagi. Ia menyambung ulang ke
CLI yang berjalan atau menjalankan otomatis daemon zode khusus-ekstensi saat
diperlukan. Tab yang dibuka zode ditempatkan di grup tab Chrome bernama
`zode`.

### Side panel task Chrome

Jalankan CLI zode terbaru dan `/browser pair` sekali. Mengklik ikon toolbar
membuka side panel; setelahnya ia menjalankan zode secara otomatis ketika
tidak ada process CLI yang berjalan. Halaman pairing tetap berupa alur
kode/token kecil, dan task tetap dibagikan dengan session TUI tanpa mengubah
fokus terminal.

Turn side-panel mengikat tool browser bridge ke halaman yang sedang tampil di
samping panel, sehingga permintaan seperti "analisis halaman ini" memakai
`browser_read` pada tab yang ada alih-alih membuka yang baru. Otomasi browser
TUI dan CLI standalone tetap memakai tab milik zode di grup tab `zode`.
Halaman aktif juga menjadi context default untuk prompt side-panel yang
ambigu; file project lokal hanya diperiksa ketika pengguna secara eksplisit
menanyakannya.

Panel dapat mengirim teks, memilih model, memilih mode akses `readOnly`,
`prompt`, dan `auto`, men-stream respons, dan meng-Stop turn yang berjalan.
Satu turn dapat melampirkan maksimal 8 file dan total 20 MiB: gambar PNG,
JPEG, GIF, dan WebP hingga 5 MiB masing-masing, plus file teks dan kode UTF-8
hingga 1 MiB masing-masing. Input PDF, Office, archive, executable, dan
non-UTF-8 ditolak.

Setelah update ekstensi, klik Reload di `chrome://extensions`. Versi ekstensi
lama tetap kompatibel dengan otomasi browser tetapi tidak memiliki side panel
task. Di Windows, zode menemukan dan meluncurkan Chrome secara langsung untuk
URL ekstensi alih-alih memanggil shell default-browser, menghindari
pengalihan Microsoft Store ketika Chrome sudah terpasang.

Perintah yang berguna:

```bash
/browser                         # buka panel kontrol browser
/browser status                  # tampilkan status target/running/paired
/browser launch                  # luncurkan managed browser
/browser close                   # tutup managed browser
/browser pair                    # pairing atau sambung ulang ekstensi Chrome bridge
/browser target managed          # pakai managed Chromium milik zode
/browser target bridge           # pakai ekstensi dan simpan sebagai default peluncuran berikutnya
/browser screenshot [path]       # tangkap screenshot browser
```

Lihat [`extensions/chrome/README.md`](../../extensions/chrome/README.md) untuk
langkah pemuatan ekstensi, update, packaging CRX, dan smoke-test.

## Kontrol desktop

Zode dapat menggerakkan aplikasi desktop native melalui API accessibility OS,
tidak hanya browser. Agent memakai `desktop_read` untuk membaca accessibility
tree (window, elemen, dan ref-nya), `desktop_act` untuk klik, mengetik,
scroll, dan menyetel nilai per elemen, serta `desktop_screenshot` untuk
menangkap layar. Pembacaan read-only tidak di-gate; aksi desktop yang mengubah
state memakai alur approval allow-once / always / deny yang sama seperti tool
lain yang berefek samping.

Backend dipilih per platform:

- **macOS** — API Accessibility (AX).
- **Windows** — UI Automation (UIA).
- **Linux** — AT-SPI.
- **Aplikasi Electron** — attach melalui Chrome DevTools Protocol.

**Ghost cursor dan Esc stop.** Zode tidak pernah menggerakkan mouse asli Anda.
Di macOS, sebuah overlay tanpa-permission (`zode-overlay`) menggambar kursor
*palsu* yang terbang di sepanjang jalur Dubins yang halus menuju target tiap
aksi, sehingga Anda dapat mengikuti apa yang dikerjakan agent; teks yang
diketik tidak pernah ditampilkan di overlay. Selama otomasi desktop aktif,
**Esc** global menginterupsi setiap turn yang berjalan dan menyembunyikan
overlay (jalur stop yang sama dengan Esc di TUI). Platform lain menjalankan
aksi desktop tanpa visualisasi.

Teks CJK dan lainnya yang tidak memiliki keycode layout US dikirim melalui
pasteboard sistem (tulis → sintesis paste → pulihkan clipboard sebelumnya)
sehingga aplikasi dengan penanganan tombol khusus menerima karakter yang
sesungguhnya.

```bash
/desktop            # tampilkan target desktop dan status permission
/desktop status     # sama, eksplisit
```

Konfigurasi berada di bawah `desktop.*` di `~/.zode/config.json`:

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor` (default `true`) menggambar kursor overlay macOS; `escCancel`
(default `true`) mengaktifkan interupsi Esc global selama otomasi;
`overlayHelperPath` (default `null`) menimpa lokasi helper `zode-overlay` —
helper yang tidak ada cukup menonaktifkan visualisasi. Otomasi desktop dapat
meminta permission OS (mis. Accessibility macOS) pada penggunaan pertama.

## Watchdog turn latar belakang, /loop, dan /schedule

Turn `/loop` dan `/schedule` yang dimiliki scheduler berjalan di bawah
watchdog liveness in-process. Aktivitas provider, tool, dan nested-agent
menyegarkan heartbeat sisi-sumber bersama, sementara `maxRuntimeSecs` tetap
menjadi batas absolut. Pada salah satu timeout, zode meminta pembatalan
kooperatif, menunggu `abortGraceSecs`, lalu hard-stop task turn lokal jika ia
masih belum drain. Menghentikan task saja tidak cukup untuk melepaskan slot
scheduler-nya: zode juga menunggu setiap provider, tool, hook, subprocess
reader, dan nested-agent worker yang di-track mencapai quiescence. Jika batas
kedua itu tidak tercapai dalam lima detik, tab/store dikarantina, job
dinonaktifkan, dan lease live-attempt-nya tetap dipegang sampai worker
benar-benar keluar.

Percobaan yang gagal memakai backoff eksponensial berbatas dari
`initialBackoffSecs` ke `maxBackoffSecs`. Turn yang sukses membersihkan hitung
gagal berturutnya; begitu `maxRetries` habis, zode menghentikan loop atau
menonaktifkan schedule persisten. Interupsi manual, penghapusan job, dan
penonaktifan eksplisit membatalkan recovery yang tertunda alih-alih membuat
retry lain ketika tidak ada mutasi yang dimulai. Recovery sengaja konservatif
di sekitar efek samping: zode retry otomatis hanya ketika ia belum mengamati
efek samping; jika sebuah mutasi mungkin sudah terjadi, termasuk pembatalan
manual di tengah mutasi, ia menghentikan/menonaktifkan job dan menunggu review
manusia. Tool yang sengaja melepas pekerjaan (`BashRun` atau GUI ter-detach)
juga menghentikan pengulangan setelah turn itu. Batas inaktivitas yang sama
membatasi antrean claim-to-start: jika tab atau preflight turn yang sibuk
menghalangi occurrence yang dimiliki untuk mulai, ia menjadi kegagalan
watchdog biasa yang bebas efek samping dan masuk ke kebijakan retry berbatas
yang sama alih-alih memegang lease lintas-process selamanya.

Quiescence adalah jaminan lokal. Pekerjaan yang sudah diterima oleh MCP server
remote, ekstensi browser, desktop actor, atau sistem eksternal lain mungkin
tidak mendukung revokasi. Jika panggilan seperti itu diinterupsi, zode
menandai hasilnya unresolved, menonaktifkan job scheduler, dan mengharuskan
Anda memverifikasi state eksternal sebelum mengaktifkannya kembali.

Gunakan `/watchdog status` untuk konfigurasi dan kesehatan per-turn/retry.
State yang sama muncul di `/tasks` bersama background shell dan turn yang
berjalan; umur antrean yang di-claim dan fence persistensi terminal juga
ditampilkan di sana.

Ini adalah watchdog untuk turn scheduler di dalam process zode saat ini. Ia
bukan supervisor process OS dan tidak dapat me-restart zode setelah crash atau
restart mesin; gunakan service manager platform Anda ketika restart level
process diperlukan. Schedule persisten mencatat token active-attempt yang
didukung oleh file lock OS per-schedule. Saat startup, lock yang diperebutkan
dibiarkan karena process zode lain masih memilikinya; lock bebas dengan token
persisten yang eksak adalah orphan dari keluar yang tidak bersih, jadi zode
menonaktifkan schedule itu sebagai execution-state-unknown alih-alih
memutarnya ulang secara diam-diam. Kontrak recovery ini mencakup crash
process. Ia tidak mengklaim durabilitas level-storage lintas kehilangan daya
mendadak atau hardware gagal, dan tidak menggantikan service manager OS.

### `/loop`, `/schedule`, dan timing task

- **`/loop <30s|5m|1h> [--max N] <prompt>`** — turn berulang khusus-session di
  tab saat ini; `list` / `stop [id]`. Interval minimum 30s. Prompt yang jatuh
  tempo diantre lewat jalur `queued_input` yang sama (tidak pernah menginterupsi
  turn yang berjalan; melewati trigger selama prompt-nya masih diantre).
- **`/schedule add <hh:mm|mon hh:mm|every 2h> <prompt>`** — dipersist ke
  `~/.zode/schedules.json`. Trigger yang terlewat saat zode tidak berjalan
  dilewati, tidak pernah diputar ulang. Dedup lintas-process bersifat
  first-writer-wins pada `lastFiredMs`. `list` / `rm <id>` /
  `enable|disable <id>`.
- **Timing** — `TurnRecorder` menstempel `durationMs` pada event `tool.completed`
  dan `turn.completed`. TUI menampilkan suffix per-tool `· 1.2s`, footer turn
  `✓ done · 34s · 3 tools`, dan waktu berlalu yang dimanusiakan di `/tasks`.

## Slash command

| Command | Fungsinya |
|---|---|
| `/help` | Overlay command + keybinding |
| `/clear` | Bersihkan percakapan (dan context) |
| `/model [id]` | Tampilkan / catat model aktif |
| `/config` | Tampilkan model + working directory |
| `/compact` | Status auto-compaction context |
| `/cost` | Penggunaan token & biaya sejauh ini (termasuk sub-agent) |
| `/theme [id]` | Ganti tema (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Picker session — resume ke tab baru dengan riwayat |
| `/connect` | Connect dan ganti provider aktif |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Tampilkan/sembunyikan sidebar kanan; lipat section MCP / modified-files / todo |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Panel dan perintah kontrol browser; pairing ekstensi Chrome bridge atau beralih antara managed Chromium dan profil Chrome Anda |
| `/loop <interval> [--max N] <prompt>` | Jalankan prompt berulang di tab saat ini; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Persist prompt terjadwal; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Tampilkan konfigurasi, kesehatan, dan retry tertunda watchdog turn latar belakang |
| `/tasks` | Panel background shell, turn yang berjalan, dan kesehatan watchdog |
| `/undo`, `/redo` | Undo / redo edit file terakhir |
| `/mcp` | Kelola MCP server — enable / disable di dialog |
| `/skills` | Daftar skills yang tersedia |
| `/agents` | Kelola sub-agent — buat (dibantu AI atau manual) / hapus |
| `/external-agents [list\|discover]` | Daftar CLI eksternal yang didukung di `PATH`, atau daftarkan tiap preset yang terdeteksi secara eksplisit |
| `/team [status\|board\|dismiss <name>]` | Periksa roster teammate persisten dan board bersama, atau hapus teammate |
| `/workflows` | Kelola & jalankan workflow ber-script JS (orkestrasi `agent()`/`parallel()`/`pipeline()`, dieksekusi deterministik oleh zode) |
| `/effort` | Pilih level reasoning effort |
| `/thinking`, `/tool-details` | Toggle tampilan reasoning / detail pemanggilan tool |
| `/orchestration` | Toggle orkestrasi sub-agent + workflow otonom |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | Tampilkan / kendalikan OS sandbox saat runtime |
| `/language` | Ganti bahasa UI (15 locale) |
| `/export [path]` | Export transcript ke Markdown (direktori mendapat nama default) |
| `/yolo` | Mode bypass-approval |
| `/exit` | Keluar |

Agents dan skills yang dibuat, serta tool MCP yang tersambung, juga muncul
sebagai slash command dinamis (mis. `/<name>`) dan dapat dipanggil langsung.

## Keybinding

> Di macOS, chord aplikasi di bawah memakai **`Cmd`** (⌘); di Windows/Linux
> memakai `Ctrl`. `Ctrl+C/D/L/V` tetap `Ctrl` di mana pun (konvensi terminal).

| Tombol | Aksi |
|---|---|
| `Enter` | Kirim pesan (mengantre bila sebuah turn berjalan) |
| `Shift`/`Alt`+`Enter` | Baris baru |
| `Up` / `Down` | Panggil prompt sebelumnya / berikutnya (atau geser seleksi autocomplete) |
| `Ctrl+C` | Interupsi turn (keluar saat idle) |
| `Ctrl+D` | Keluar |
| `Ctrl+L` | Gambar ulang percakapan dari store (memulihkan view yang kosong; pakai `/clear` untuk membuang) |
| `Ctrl+V` | Paste (teks atau path gambar) |
| `Cmd/Ctrl+O` | Settings |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | Tab baru / tutup tab |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Loncat ke / putar tab |
| `Cmd/Ctrl+B` | Panel task latar belakang |
| `Cmd/Ctrl+G` | Toggle sidebar |
| `F1` | Help |
| `PgUp` / `PgDn` | Scroll percakapan |
| `Home` / `End` | Loncat ke atas / terbaru percakapan |
| `Esc` | Tutup overlay saat ini (atau interupsi turn yang berjalan) |

## Instruksi project

Zode membaca instruksi dari hierarki tiga tingkat (yang belakangan menang
perhatian): global `~/.zode/AGENTS.md` (atau `instructions.md`) → root project
→ cwd. Di tiap direktori ia mengutamakan `AGENTS.md` di atas `CLAUDE.md`.
Skills berada di bawah `.zode/skills/**/SKILL.md`; MCP server di
`~/.zode/mcp.json` ⊕ `.mcp.json`; hooks di `~/.zode/hooks.json` ⊕
`.zode/hooks.json`.

**Konfigurasi lintas-agent.** Zode membaca skills dan konfigurasi MCP langsung
dari Claude Code, Codex, Cursor, opencode, Gemini, dan agent lokal terkait.
Pohon plugin terpasang dan cache plugin milik produk-produk tersebut tidak
pernah dipindai. Untuk memakai ulang sebuah plugin, instal source-nya secara
eksplisit dengan `zode plugin install ... --trust`; format paket Codex dan
Claude Code tetap didukung untuk plugin yang dipasang melalui Zode.

## Mengonfigurasi MCP server

MCP server berada di config nested-precedence yang sama dengan yang lain —
`~/.zode/mcp.json` untuk semua project, `.mcp.json` atau `.zode/mcp.json` di
root project untuk membatasi satu ke sebuah repo. Tanpa registry, tanpa
restart-and-pray: edit file, lalu `/mcp` (atau relaunch) untuk memuatnya.

### stdio (spawn server lokal)

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

`command`/`args` men-spawn server sebagai subprocess yang di-pipe lewat stdio.
Nilai `env` mendukung substitusi `$NAME` / `${NAME}` terhadap environment
process zode sendiri (diperluas tepat sebelum menyambung, tidak ditulis ke
disk) — praktis untuk menjaga token keluar dari file config itu sendiri.

### Streamable HTTP (server remote)

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

`"transport": "http"` menyambung dengan transport Streamable HTTP dari spec
MCP saat ini — satu `url`, tanpa endpoint SSE terpisah untuk dikonfigurasi.
`"sse"` diterima sebagai ejaan yang setara (beberapa config — dan dokumen
setup MCP server sendiri — masih menyebutnya begitu); keduanya me-resolve ke
konektor yang sama. `headers` diteruskan verbatim (termasuk `Authorization`,
sehingga skema Bearer/Basic/kustom semua berfungsi) dan mendukung substitusi
`$VAR` yang sama seperti `env`. Tambahkan `"enabled": false` ke server mana pun
untuk menyimpan definisinya tanpa menyambungkannya — `/mcp` juga men-toggle
ini per server tanpa mengedit file secara manual.

### Memakainya

Setiap tool yang diekspos server yang tersambung muncul sebagai
`mcp__<server>__<tool>`, dapat dipanggil agent seperti tool bawaan mana pun
(dan bisa di-`@`-mention di kotak input). `/mcp` membuka dialog yang mendaftar
setiap server yang ditemukan — connected / disconnected / disabled — dengan
Space untuk men-toggle satu on atau off; section `mcp` yang dapat dilipat di
sidebar mencerminkan state koneksi live yang sama sekilas pandang.

Zode juga membaca konfigurasi MCP langsung dari Claude Code, Codex, Cursor,
opencode, dan Gemini. Konfigurasi home diperlakukan sebagai setup pengguna;
definisi MCP asing yang lokal-project ditemukan dalam keadaan disabled dan
dapat diaktifkan melalui `/mcp`. Deklarasi MCP yang terkubur dalam pohon
plugin terpasang produk lain tidak dipindai. `openpencil` dicadangkan —
op-bridge menggerakkannya secara native, sehingga server apa pun yang
dideklarasikan dengan nama itu diabaikan.

## Memasang Skills & Command Markdown

Keduanya adalah Markdown biasa di disk — tanpa registry, tanpa build step.
Letakkan sebuah file, dan ia aktif pada peluncuran berikutnya (atau `/skills`
untuk memeriksa apa yang dimuat).

### Memasang skill

Sebuah skill adalah folder dengan `SKILL.md` di dalamnya. Letakkan di bawah
project (`.zode/skills/`) atau home dir Anda (`~/.zode/skills/`):

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

Skill kini muncul di `/skills`, agent dapat memanggilnya sendiri via tool
Skill, dan ia juga menjadi slash command dinamis — mengetik `/code-review look
at src/lib.rs` diperluas menjadi prompt yang menjalankan skill. File tambahan
di samping `SKILL.md` (referensi, script) ikut terkirim bersama skill.
Direktori skills langsung milik Claude Code, Codex, opencode, Cursor, dan agent
terkait dipindai. Skills yang terkubur di dalam pohon plugin terpasang atau
cache produk-produk itu tidak; instal plugin secara eksplisit melalui Zode
bila ingin memakainya di sini.

### Memasang command (prompt Markdown)

Sebuah slash command kustom adalah satu file `.md` yang **nama filenya adalah
nama command** dan yang body-nya adalah prompt yang dikirimkannya. Apa pun
yang Anda ketik setelah command ditambahkan ke body:

```bash
mkdir -p .zode/commands            # atau ~/.zode/commands untuk semua project
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

Sekarang `/changelog` mengirim prompt itu, dan `/changelog only the sidebar
work` menambahkan argumen Anda setelahnya. Command di `~/.claude/commands` dan
`~/.codex/commands` (dan padanan level-project-nya) juga dimuat; command di
dalam *pohon plugin asing* nonaktif secara default — salin `.md`-nya ke sebuah
dir `.zode/commands/` untuk opt-in.

## Ekosistem ZSeven-W

Zode adalah bagian dari stack ZSeven-W yang lebih luas untuk tool
pengembangan AI-native:

| Produk | Apa itu |
|--------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Runtime async Rust murni untuk mengirim LLM agents: multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions, dan optional coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Framework UI cross-platform native Rust tempat file `.op` menjadi app, menghubungkan artefak desain gaya OpenPencil ke software yang dapat dijalankan. |
| [`noema`](https://github.com/ZSeven-W/noema) | Sistem memory local-first dan non-vector untuk coding agents, dengan lexical recall, review queues, akses MCP, S3 offload, dan enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Tool desain vector open-source AI-native untuk workflow design-as-code, mengubah prompt menjadi UI langsung di live canvas dengan concurrent agent teams. |

## Benchmark

Benchmark Zode mencakup one-shot code generation, agentic read/run/edit/fix,
tugas multi-file, tricky bugs (疑难杂症), instruction-following MCP/Skills/constraint,
dan Noema LOCOMO runner. Head-to-head, **Zode + DeepSeek-v4-pro menyamai
Claude** di seluruh dimensi, dengan setiap task dinilai oleh grader
tersembunyi. Metodologi lengkap, perintah reproduksi, dan tabel hasil ada di
bagian [Benchmark README bahasa Inggris](../../README.md#benchmark); seluruh
suite berada di [`benchmarks/`](../../benchmarks/).

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

Kontribusi diterima. Gunakan [Conventional Commits](https://www.conventionalcommits.org/):
`<type>(<scope>): <subject>`, dengan scope umum `core`, `tui`, `cli`, `tools`,
`config`, `build`, `ci`, `docs`.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
