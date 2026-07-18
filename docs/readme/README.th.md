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
  <strong>ผู้ช่วยเขียนโค้ดแบบ open-source และ AI-native สำหรับ terminal.</strong><br/>
  อ่านโค้ด รันคำสั่ง ค้นหาไฟล์ และจัดการ git ผ่าน Rust TUI ที่รวดเร็ว
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

> README ฉบับแปลนี้ครอบคลุมภาพรวมผลิตภัณฑ์และ quick start ส่วนรายละเอียด benchmark และบันทึกแบบยาวล่าสุดให้ยึด [README ภาษาอังกฤษ](../../README.md) เป็นหลัก

## จุดเด่น

- **รองรับหลาย provider**: Anthropic, OpenAI, API ที่ compatible กับ OpenAI เช่น DeepSeek, Moonshot, OpenRouter และ Ollama ในเครื่อง
- **เครื่องมือครบ**: อ่าน/เขียน/แก้ไขไฟล์, ค้นหา code และ content, foreground/background shell, git, web fetch, notebooks และ TODO tracking
- **ควบคุม browser**: เครื่องมือ `browser_*` ควบคุม managed Chromium หรือ Chrome profile จริงผ่าน Chrome bridge extension
- **Permission ไม่ block งาน**: เครื่องมือที่มี side effect ต้องผ่าน allow once / always / deny และ approval prompt อยู่ inline
- **เปิด OS sandbox เป็นค่าเริ่มต้น**: shell commands รันใน macOS `sandbox-exec` หรือ Linux `bwrap` โดย outbound network ถูก deny เป็นค่าเริ่มต้น
- **TUI เต็มจอ**: streaming Markdown, syntax highlighting, diff preview, slash-command autocomplete, prompt history, ธีมในตัว 11 แบบ, settings/help overlays และ UI 15 ภาษา (`/language`)
- **Multi-session tabs**: รันหลาย conversation แบบ isolated ด้วย `Ctrl+T` และ resume sessions เก่าได้
- **Sub-agents, teams และ workflows**: delegate Tasks แบบครั้งเดียว, register internal หรือ external CLI teammates ด้วยตนเอง แล้วจัดการผ่าน `/agents`, `/team` และ `/workflows`
- **Skills, MCP และ hooks**: โหลด `SKILL.md`, เชื่อมต่อ MCP servers และรัน external scripts ตอนเกิด tool events

## ติดตั้ง

### คำสั่งเดียว

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer จะตรวจ OS และ CPU, ดาวน์โหลด binary ที่ตรงจาก [release](https://github.com/ZSeven-W/zode/releases) ล่าสุด และวาง `zode` ไว้ใน `PATH`

### ดาวน์โหลดเอง

ดาวน์โหลด archive ที่ตรงกับ platform จาก [releases page](https://github.com/ZSeven-W/zode/releases):

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

แตกไฟล์แล้ว move `zode` ไปยัง `PATH` เช่น `sudo mv zode /usr/local/bin/` Linux builds ใช้ glibc; macOS binaries ไม่ได้ sign ถ้า Gatekeeper เตือนให้ใช้ `xattr -dr com.apple.quarantine ./zode`

### Build จาก source

ต้องใช้ stable Rust toolchain ที่ค่อนข้างใหม่:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

binary อยู่ที่ `target/release/zode` ส่วน agent runtime อยู่ใน git submodule `vendor/agent` จึงควร clone ด้วย `--recurse-submodules` หรือรัน `git submodule update --init`

## Quick Start

วิธีที่ง่ายที่สุดคือเปิด `zode` แล้วรัน **`/connect`** ซึ่งจะเปิด interactive model picker และเขียน config ให้

หรือเขียน `~/.zode/config.json` เอง:

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

คำสั่งที่ใช้บ่อย:

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

## Register external CLI teammates ด้วยตนเอง

Zode ใช้ agent CLI เป็น Task worker แบบครั้งเดียวหรือ persistent teammate ได้
การ register เป็นแบบ manual โดยตั้งใจ: executable ที่อยู่ใน `PATH` จะไม่ถูก
เปิดให้ model อัตโนมัติ ต้องเพิ่ม profile ใน `externalAgents.agents`
ใช้ `/external-agents` เพื่อดู CLI ที่รองรับใน `PATH` และ
`/external-agents discover` เพื่อลงทะเบียน preset ที่พบทั้งหมดลงใน config ส่วนกลางอย่างชัดเจน การเริ่มต้นโปรแกรมจะไม่สแกนหรือลงทะเบียนอัตโนมัติ

| Profile | Command | Task | Team mode | External CLI sandbox |
|---|---|---:|---:|---|
| `claude-code` | `claude` | ได้ | persistent | unrestricted |
| `codex` | `codex` | ได้ | persistent | workspace-write |
| `opencode` | `opencode` | ได้ | stateless | unknown |
| `cline` | `cline` | ได้ | stateless | unrestricted |
| `antigravity` | `agy` | ได้ | stateless | unknown |
| `cursor` | `cursor-agent` | ได้ | persistent | unrestricted |
| `kiro` | `kiro-cli` | ได้ | stateless | unrestricted |
| `pi` | `pi` | ได้ | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | ได้ | persistent | unrestricted |

### เพิ่ม profile

ใช้ `~/.zode/config.json` สำหรับ global หรือ `.zode/config.json` สำหรับ project
object ว่างจะ enable preset ที่รู้จักแบบ manual; `command` เป็นชื่อใน `PATH`
หรือ path ก็ได้

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

เพิ่มเฉพาะ profiles ที่ต้องการเปิดให้ model ใช้ Custom profile รองรับ `stdin`,
`argv` หรือ `file` ใน `promptTransport` และรองรับ `text`, generic `jsonl`,
`jsonl-claude` หรือ `jsonl-codex` ใน `output` Generic JSONL ดึง text และ
session ID ผ่าน RFC 6901 pointers `textSource` และ `sessionIdSource`
`resumeArgs` ต้องมี token `{session_id}` แยกต่างหาก CLI ที่ resume ไม่ได้
จะเป็น stateless teammate ที่เปิด process ใหม่ต่อการส่งแต่ละครั้ง และยังใช้
เป็น Task worker แบบครั้งเดียวได้
`newSessionArgs` สามารถมี `{session_id}` แยกต่างหากได้ โดย Zode จะสร้าง ID
สำหรับ run แรก แล้วใช้ `resumeArgs` ใน assignment ถัดไป

External process จะได้รับเพียง `PATH`, `HOME` และ `TERM` เป็นค่าเริ่มต้น
ให้เพิ่ม API keys ใน `envAllow` หรือ `authEnv` ตอน hire ครั้งแรก Zode จะแสดง
command, cwd และ sandbox แล้วขอ trust Zode gate เฉพาะการเริ่ม process
ไม่ใช่ทุก file edit หรือ shell command ของ external CLI โหมด non-interactive
เช่น `--yolo` ต้องกำหนด `trusted: true` อย่างชัดเจน

### ใช้งาน team

`team_hire` และ `team_send` เป็น model tools ให้บอก leader ด้วยภาษาปกติ:

```text
Hire `codex` เป็น teammate ชื่อ `implementer` เพื่อทำ auth refactor และ tests
Claim `src/auth/` ก่อนส่ง task ให้ teammate
```

`/team` และ `/team board` ใช้ดู status; `/team dismiss implementer` ใช้ลบ
teammate Team state อยู่ใน `<cwd>/.zode/team/` แต่ external CLI trust grants
จะอยู่เฉพาะใน Zode process ปัจจุบัน

## Configuration

`providers` คือ source of truth ของ provider definitions ส่วน top-level `provider` ชี้ active model โดย OpenAI-compatible providers มักใช้ `baseUrl` และ `dialect`:

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
  "language": "th"
}
```

หนึ่ง provider มีหลาย models ได้ และเปลี่ยนสดด้วย `/model` ส่วนภาษาเปลี่ยนได้ด้วย `/language`

## Server mode และ SDK

`zode server` เปิด newline-delimited JSON-RPC server บน stdin/stdout สำหรับ editor integrations, local automation, tests และ SDK clients

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

## Browser control

Zode มี `tools:browser` group สำหรับอ่าน screenshots/DOM/logs, navigate/click/type, run JavaScript และ manage tabs โดย target เป็น managed Chromium หรือ Chrome จริงผ่าน extension ใน [`extensions/chrome/`](../../extensions/chrome/)

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

## Slash commands ที่ใช้บ่อย

| Command | ใช้ทำอะไร |
|---|---|
| `/help` | Commands และ keybindings |
| `/connect` | Connect/switch provider |
| `/model [id]` | Show/set active model |
| `/theme [id]` | สลับธีม (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Resume sessions |
| `/browser ...` | Browser control |
| `/tasks` | Background tasks |
| `/mcp` | Manage MCP servers |
| `/skills` | List skills |
| `/agents` | Manage sub-agents |
| `/external-agents [list\|discover]` | ดู external CLI ที่รองรับใน `PATH` หรือลงทะเบียน preset ที่พบอย่างชัดเจน |
| `/team [status\|board\|dismiss <name>]` | ดู persistent teammates และ shared board หรือลบ teammate |
| `/workflows` | Manage workflows |
| `/sandbox ...` | Control OS sandbox |
| `/language` | Switch UI language |
| `/export [path]` | Export Markdown |
| `/exit` | Exit |

ตารางเต็มอยู่ใน [README ภาษาอังกฤษ](../../README.md#slash-commands)

## Instructions, MCP และ Skills

Zode อ่าน instructions จาก `~/.zode/`, project root และ current directory โดยแต่ละระดับ prefer `AGENTS.md` ก่อน `CLAUDE.md` Skills อยู่ใน `.zode/skills/**/SKILL.md`; MCP servers อยู่ใน `~/.zode/mcp.json`, `.mcp.json` หรือ `.zode/mcp.json`

Zode ยัง discover skills, commands และ MCP configurations จาก Claude, Codex, opencode, Cursor และ agents อื่นๆ ด้วย MCP ภายนอกที่พบใน project จะ disabled เป็นค่าเริ่มต้น

## ZSeven-W Ecosystem

Zode เป็นส่วนหนึ่งของ stack เครื่องมือ AI-native development ของ ZSeven-W:

| Product | คืออะไร |
|---------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async runtime สำหรับ LLM agents พร้อม multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions และ optional coding tools |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework ที่ทำให้ `.op` file เป็น app และเชื่อม OpenPencil-style design artifacts ไปสู่ runnable software |
| [`noema`](https://github.com/ZSeven-W/noema) | Local-first, non-vector memory system สำหรับ coding agents พร้อม lexical recall, review queues, MCP, S3 offload และ enterprise policy controls |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool สำหรับ design-as-code workflows ที่แปลง prompts เป็น UI บน live canvas และรองรับ concurrent agent teams |

## Benchmark

Benchmarks ของ Zode ครอบคลุม one-shot code generation, agentic read/run/edit/fix, multi-file tasks, tricky bugs, MCP/Skills/constraint following และ Noema LOCOMO ดู methodology และผลลัพธ์เต็มได้ใน [Benchmark section ของ README ภาษาอังกฤษ](../../README.md#benchmark)

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

ยินดีรับ contributions โปรดใช้ [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`

## License

[MIT](../../LICENSE) &copy; ZSeven-W
