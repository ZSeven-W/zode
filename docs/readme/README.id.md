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

- **Multi-provider**: Anthropic, OpenAI, API kompatibel OpenAI seperti DeepSeek, Moonshot, OpenRouter, serta Ollama lokal.
- **Tool surface yang kaya**: baca/tulis/edit file, pencarian kode dan konten, foreground/background shell, git, web fetch, notebook, dan TODO tracking.
- **Kontrol browser**: tool `browser_*` dapat mengendalikan managed Chromium atau profil Chrome asli melalui ekstensi Chrome bridge.
- **Permission non-blocking**: tool yang mengubah state melewati allow once / always / deny, dengan prompt approval inline.
- **OS sandbox aktif default**: perintah shell berjalan di `sandbox-exec` macOS atau `bwrap` Linux, dan outbound network ditolak secara default.
- **TUI layar penuh**: streaming Markdown, syntax highlighting, diff preview, slash-command autocomplete, prompt history, 11 tema bawaan, settings/help overlays, dan UI 15 bahasa (`/language`).
- **Tab multi-session**: jalankan beberapa percakapan terisolasi dengan `Ctrl+T` dan resume session lama.
- **Sub-agents, team, dan workflows**: delegasikan Tasks sekali jalan, daftarkan teammate internal atau CLI eksternal secara manual, lalu kelola lewat `/agents`, `/team`, dan `/workflows`.
- **Skills, MCP, dan hooks**: muat paket `SKILL.md`, hubungkan MCP server, dan jalankan script eksternal pada event tool.

## Instalasi

### Satu baris

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer mendeteksi OS dan CPU, mengunduh binary yang sesuai dari [release](https://github.com/ZSeven-W/zode/releases) terbaru, lalu menaruh `zode` di `PATH`.

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

Ekstrak lalu pindahkan `zode` ke `PATH`, misalnya `sudo mv zode /usr/local/bin/`. Build Linux memakai glibc; binary macOS tidak ditandatangani. Jika Gatekeeper memperingatkan, gunakan `xattr -dr com.apple.quarantine ./zode`.

### Build dari source

Butuh stable Rust toolchain yang baru:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

Binary berada di `target/release/zode`. Agent runtime berada di git submodule `vendor/agent`; clone dengan `--recurse-submodules` atau jalankan `git submodule update --init`.

## Quick Start

Cara termudah adalah menjalankan `zode` lalu **`/connect`**. Model picker interaktif akan menulis konfigurasi.

Anda juga bisa menulis `~/.zode/config.json` secara manual:

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

Perintah umum:

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

## Mendaftarkan teammate CLI eksternal secara manual

Zode dapat memakai agent CLI sebagai Task worker sekali jalan atau teammate
persisten. Pendaftaran sengaja dibuat manual: executable di `PATH` tidak
otomatis diekspos ke model. Tambahkan profile di `externalAgents.agents`.
Gunakan `/external-agents` untuk melihat CLI yang didukung di `PATH`, lalu
`/external-agents discover` untuk mendaftarkan semua preset yang ditemukan secara eksplisit ke config global. Startup tidak pernah memindai atau mendaftar otomatis.

| Profile | Command | Task | Mode team | Sandbox CLI eksternal |
|---|---|---:|---:|---|
| `claude-code` | `claude` | ya | persistent | unrestricted |
| `codex` | `codex` | ya | persistent | workspace-write |
| `opencode` | `opencode` | ya | stateless | unknown |
| `cline` | `cline` | ya | stateless | unrestricted |
| `antigravity` | `agy` | ya | stateless | unknown |
| `cursor` | `cursor-agent` | ya | persistent | unrestricted |
| `kiro` | `kiro-cli` | ya | stateless | unrestricted |
| `pi` | `pi` | ya | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | ya | persistent | unrestricted |

### Menambahkan profile

Gunakan `~/.zode/config.json` secara global atau `.zode/config.json` untuk
project. Object kosong mengaktifkan preset yang dikenal secara manual;
`command` boleh berupa nama di `PATH` atau path.

```jsonc
{
  "externalAgents": {
    "agents": {
      "claude-code": {},
      "codex": {},
      "opencode": {},
      "cline": {},
      "antigravity": {},
      "cursor": {},
      "kiro": {},
      "pi": {},
      "grok": {},
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

Tambahkan hanya profile yang ingin diekspos. Custom profile mendukung `stdin`,
`argv`, atau `file` untuk `promptTransport` serta `text`, `jsonl` generik,
`jsonl-claude`, atau `jsonl-codex` untuk `output`. JSONL generik mengekstrak
text dan session ID memakai pointer RFC 6901 `textSource` dan
`sessionIdSource`. `resumeArgs` wajib memuat token `{session_id}` tersendiri.
CLI tanpa resume menjadi teammate stateless dengan process baru per kiriman,
dan tetap dapat menjadi Task worker sekali jalan.
`newSessionArgs` juga dapat memuat `{session_id}` tersendiri: Zode membuat ID
untuk run pertama, lalu memakai `resumeArgs` untuk assignment berikutnya.

Process eksternal secara default hanya menerima `PATH`, `HOME`, dan `TERM`;
tambahkan API key ke `envAllow` atau `authEnv`. Saat hire pertama, Zode
menampilkan command, cwd, dan sandbox lalu meminta trust. Zode hanya melakukan
gate pada awal process, bukan setiap file edit atau shell command CLI eksternal.
Mode non-interaktif seperti `--yolo` memerlukan `trusted: true` secara eksplisit.

### Menggunakan team

`team_hire` dan `team_send` adalah tools model. Minta leader dengan bahasa biasa:

```text
Hire `codex` sebagai teammate `implementer` untuk refactor autentikasi dan tests.
Claim `src/auth/` sebelum mengirim task kepadanya.
```

`/team` dan `/team board` menampilkan status; `/team dismiss implementer`
menghapus teammate. Team state disimpan di `<cwd>/.zode/team/`, tetapi trust
grant CLI eksternal hanya berlaku selama process Zode saat ini.

## Konfigurasi

`providers` adalah sumber utama untuk provider; `provider` di level atas menunjuk model aktif. Provider kompatibel OpenAI biasanya memakai `baseUrl` dan `dialect`:

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
  "language": "id"
}
```

Satu provider dapat memuat beberapa model, dan `/model` dapat menggantinya live. Bahasa juga dapat diganti dengan `/language`.

## Server mode dan SDK

`zode server` menjalankan newline-delimited JSON-RPC server di stdin/stdout untuk integrasi editor, otomasi lokal, test, dan SDK client.

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDK:

| SDK | Direktori | Test lokal |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## Kontrol browser

Zode memiliki grup `tools:browser` untuk membaca screenshot/DOM/log, navigasi, klik, mengetik, menjalankan JavaScript, dan mengelola tab. Targetnya bisa managed Chromium atau Chrome asli lewat ekstensi [`extensions/chrome/`](../../extensions/chrome/).

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

## Slash commands umum

| Command | Fungsi |
|---|---|
| `/help` | Bantuan command dan keybinding |
| `/connect` | Connect dan ganti provider |
| `/model [id]` | Lihat atau set model aktif |
| `/theme [id]` | Ganti tema (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Resume session |
| `/browser ...` | Kontrol browser |
| `/tasks` | Background tasks |
| `/mcp` | Kelola MCP server |
| `/skills` | Daftar skills |
| `/agents` | Kelola sub-agents |
| `/external-agents [list\|discover]` | Lihat CLI eksternal yang didukung di `PATH` atau daftarkan preset yang ditemukan secara eksplisit |
| `/team [status\|board\|dismiss <name>]` | Lihat teammate persisten dan board bersama, atau hapus teammate |
| `/workflows` | Kelola workflows |
| `/sandbox ...` | Kontrol OS sandbox |
| `/language` | Ganti bahasa UI |
| `/export [path]` | Export Markdown |
| `/exit` | Keluar |

Tabel lengkap ada di [README bahasa Inggris](../../README.md#slash-commands).

## Instructions, MCP, dan Skills

Zode membaca instructions dari `~/.zode/`, root project, dan current directory; di setiap level, `AGENTS.md` diprioritaskan sebelum `CLAUDE.md`. Skills berada di `.zode/skills/**/SKILL.md`; MCP server di `~/.zode/mcp.json`, `.mcp.json`, atau `.zode/mcp.json`.

Zode juga menemukan skills, commands, dan MCP configurations dari Claude, Codex, opencode, Cursor, dan agent lain. MCP eksternal yang ditemukan di dalam project dinonaktifkan secara default.

## Ekosistem ZSeven-W

Zode adalah bagian dari stack ZSeven-W untuk AI-native development tools:

| Produk | Apa itu |
|--------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Runtime async Rust murni untuk LLM agents, dengan multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions, dan optional coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Framework UI cross-platform native Rust, tempat file `.op` menjadi app dan menghubungkan artefak desain gaya OpenPencil ke software yang dapat dijalankan. |
| [`noema`](https://github.com/ZSeven-W/noema) | Sistem memory local-first dan non-vector untuk coding agents, dengan lexical recall, review queues, MCP, S3 offload, dan enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Tool desain vector open-source AI-native untuk workflow design-as-code, mengubah prompt menjadi UI langsung di live canvas dengan concurrent agent teams. |

## Benchmark

Benchmark Zode mencakup one-shot code generation, agentic read/run/edit/fix, tugas multi-file, tricky bugs, MCP/Skills/constraint following, dan Noema LOCOMO. Metodologi dan hasil lengkap ada di bagian [Benchmark README bahasa Inggris](../../README.md#benchmark).

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

Kontribusi diterima. Gunakan [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
