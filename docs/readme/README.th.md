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

- **รองรับหลาย provider**: Anthropic, OpenAI และ API ที่ compatible กับ OpenAI (DeepSeek, Moonshot, OpenRouter และ dialect อื่น ๆ) รวมถึง Ollama ในเครื่อง รองรับโมเดล output ขนาดใหญ่และ **context 1M** (`contextWindow` / `maxOutputTokens` ตั้งค่าได้)
- **เครื่องมือครบ**: อ่าน/เขียน/แก้ไขไฟล์ (รวม `MultiEdit` แก้หลายจุดแบบ atomic), ค้นหา code และ content, foreground/background shell, git, web fetch (บวก `WebSearch` แบบ optional เมื่อมี Tavily key), notebooks และ TODO tracking
- **ควบคุม browser**: เครื่องมือ `browser_*` ในตัวควบคุม managed Chromium หรือ Chrome profile จริงของคุณผ่าน Chrome bridge extension ของ zode ได้ ทั้ง navigate, click/type, ตรวจ DOM, ถ่าย screenshot, อ่าน console/network log และจัดกลุ่ม tab ที่ zode เปิด การ pair ทำเพียงครั้งเดียว — extension จะ reconnect อัตโนมัติข้ามการ restart ของ zode
- **Permission ไม่ block งาน**: เครื่องมือที่มี side effect ทุกตัวต้องผ่านการอนุมัติ (allow once / always / deny) แต่ prompt จะปักอยู่ inline และไม่ block คุณ พิมพ์ต่อเพื่อ queue คำสั่งถัดไปได้ขณะที่เครื่องมือรออนุมัติ พร้อมกฎ hard-deny
- **เปิด OS sandbox เป็นค่าเริ่มต้น**: shell commands รันภายใต้ sandbox-exec (macOS) / bwrap (Linux) ในโหมด `read-only` หรือ `workspace-write` โดย **outbound network ถูก deny เป็นค่าเริ่มต้น** สลับได้สดด้วย `/sandbox`; model ขอ escape สำหรับคำสั่งเดียวได้ (`dangerouslyDisableSandbox`) ซึ่ง **คุณเป็นผู้อนุมัติ** ที่ prompt
- **TUI เต็มจอ**: streaming Markdown พร้อม syntax highlighting, diff preview, slash-command autocomplete, prompt history (Up/Down), ธีมในตัว 11 แบบ, settings/help overlays, sidebar ด้านขวาที่ทนทาน และ **UI 15 ภาษา** (`/language`)
- **Session ที่คงทนและ V1-compatible**: คงสัญญา transcript `<id>.jsonl` เดิมไว้ พร้อมเพิ่ม journal, checkpoint, rewind, fork และ Git worktree แบบ isolated เป็นข้อมูล sidecar การ compact context ไม่ทำให้บทสนทนาที่มองเห็นหายไป — การ resume จะ replay ประวัติเต็มก่อน compact ขณะที่ context ของโมเดลยังคงกะทัดรัด
- **ช่องทาง automation**: JSON/JSONL headless output ที่เสถียร, การระบุ session ที่แม่นยำ, tool filters, exit code แบบ deterministic, ACP ผ่าน stdio และ operations dashboard ในเครื่อง
- **Multi-session tabs**: รันหลาย conversation เคียงกัน (`Ctrl+T`) แต่ละอันเป็น agent แยกกัน และ resume session เก่าพร้อม replay ประวัติเต็ม
- **Sub-agents, teams และ workflows**: delegate งานครั้งเดียวผ่าน Task tool, จ้าง teammate ทั้งแบบ internal และ external CLI, ประสานงานกันด้วย shared board และ file claim แล้วจัดการทั้งหมดผ่าน `/agents`, `/team` และ `/workflows`
- **การตั้งค่า local ที่พกพาได้**: อ่าน skills และ MCP configuration โดยตรงจาก Claude Code, Codex, Cursor, opencode และ Gemini โดยไม่ import plugin tree หรือ cache ที่ผลิตภัณฑ์เหล่านั้นติดตั้งไว้
- **Skills และ MCP**: โหลดชุดคำสั่ง `SKILL.md` แบบ on demand และเชื่อม MCP servers (`mcp__<server>__<tool>`); agents, skills และ MCP tools ที่สร้างขึ้นจะปรากฏเป็น slash commands
- **Hooks**: รัน external scripts เมื่อเกิด tool events (เช่น block คำสั่งอันตราย, lint หลังแก้ไข)
- **Instructions สามระดับ**: global (`~/.zode/`) → project root → cwd (`AGENTS.md` / `CLAUDE.md`)

## ติดตั้ง

### คำสั่งเดียว (binary สำเร็จรูป)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer จะตรวจ OS + CPU อัตโนมัติ, ดาวน์โหลด binary ที่ตรงจาก [release](https://github.com/ZSeven-W/zode/releases) ล่าสุด และวาง `zode` ไว้ใน PATH ปักหมุดเวอร์ชันหรือเปลี่ยนตำแหน่งได้:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

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

แตกไฟล์แล้ว move `zode` ไปยัง PATH (`sudo mv zode /usr/local/bin/`) Linux builds ใช้ glibc; macOS binaries ไม่ได้ sign (ใช้ `xattr -dr com.apple.quarantine ./zode` หาก Gatekeeper เตือน)

### Build จาก source

ต้องใช้ Rust 1.88 ขึ้นไป:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binary อยู่ที่ target/release/zode
```

> agent runtime อยู่ใน git submodule `vendor/agent` — ควร clone ด้วย
> `--recurse-submodules` เสมอ (หรือรัน `git submodule update --init`)

## Quick Start

วิธีที่ง่ายที่สุดคือเปิด `zode` แล้วรัน **`/connect`** ซึ่งเป็น interactive picker
ที่ใช้ข้อมูลจาก models.dev และเขียน config ให้คุณ

หากต้องการเขียน `~/.zode/config.json` เอง: **`providers`** คือ source of truth —
หนึ่ง entry ต่อหนึ่ง provider (credential ที่ใช้ร่วมกัน) ซึ่งบรรจุ **models** ได้หลายตัว —
และ **`provider`** ระดับบนสุดบันทึกโมเดลที่ *active* อยู่:

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // wire protocol: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // โมเดลที่ active
}
```

provider ที่ compatible กับ OpenAI (DeepSeek, Moonshot, OpenRouter, …) ต้องเพิ่ม
`baseUrl` + `dialect` และตั้งค่าเฉพาะโมเดลไว้ใน entry ของแต่ละโมเดล:

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

หนึ่ง provider entry บรรจุได้หลายโมเดล — สลับสดด้วย `/model`

จากนั้นรัน:

```bash
zode                       # TUI เต็มจอ
zode -p "explain main.rs"  # headless: หนึ่ง prompt, stream ไป stdout แล้วออก
zode --no-tui              # readline REPL ธรรมดา
zode -c                    # ทำงานต่อจาก session ล่าสุด
zode -r <id>               # resume session ตาม id prefix
zode --yolo                # ข้าม approval prompt (กฎ deny ยังมีผล)
zode --no-sandbox          # ปิด OS sandbox (เปิดเป็นค่าเริ่มต้น)
zode --sandbox-read-only   # sandbox โหมด read-only (deny การเขียนทั้งหมด)
zode --sandbox-allow-network  # อนุญาต outbound network ใน sandbox
zode --browser             # บังคับเปิดเครื่องมือ browser สำหรับ run นี้
zode --no-browser          # ปิดเครื่องมือ browser สำหรับ run นี้
zode --model <id>          # override โมเดล
zode --provider <name>     # เลือก provider ที่ตั้งชื่อไว้จาก config.providers
zode server                # โหมด JSON-RPC app-server ผ่าน stdio
zode acp                   # agent แบบ Agent Client Protocol ผ่าน stdio
zode dashboard             # ภาพรวม sessions/checkpoints/worktrees ในเครื่อง
```

คุณยังชี้ไปที่ provider ใดก็ได้โดยไม่ต้องแก้ config ด้วยการ export key ที่ตรงกัน
(`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); สำหรับ Ollama จะใช้ `baseUrl`
จาก environment เมื่อไม่ได้ตั้งค่าไว้

## Register external CLI teammates ด้วยตนเอง

Zode ใช้ agent CLI ของบุคคลที่สามที่ติดตั้งไว้เป็น Task worker แบบครั้งเดียว หรือ
เป็น teammate แบบ persistent/stateless ได้ การ register เป็นแบบ manual โดยตั้งใจ:
การติดตั้ง CLI หรือวางไว้ใน `PATH` **ไม่** ทำให้ model เห็นมัน เพิ่ม profile ใน
`externalAgents.agents` แล้วเปิด Zode ในโปรเจกต์ หรือรัน `/external-agents`
เพื่อตรวจ CLI ที่รองรับซึ่งอยู่ใน `PATH` แล้ว `/external-agents discover`
เพื่อเพิ่ม preset ที่พบทั้งหมดลง config ส่วนกลางอย่างชัดเจน คำสั่งนี้ผู้ใช้เป็นผู้เรียก;
การเริ่มโปรแกรมไม่เคยสแกนหรือลงทะเบียน external CLI อัตโนมัติ

| Agent profile | Executable | Task worker | Team mode | External CLI sandbox |
|---|---|---:|---:|---|
| `claude-code` | `claude` | ได้ | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | ได้ | persistent | workspace-write |
| `opencode` | `opencode` | ได้ | stateless | unknown |
| `cline` | `cline` | ได้ | stateless | unrestricted |
| `antigravity` | `agy` | ได้ | stateless | unknown |
| `cursor` | `cursor-agent` | ได้ | persistent | unrestricted |
| `kiro` | `kiro-cli` | ได้ | stateless | unrestricted |
| `pi` | `pi` | ได้ | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | ได้ | persistent | unrestricted |

ทุก profile ที่ register แล้วเข้าร่วม team ได้ profile ที่ resume ได้จะเก็บ session ID
และบทสนทนาของ CLI ข้าม assignment ไว้; CLI อื่น ๆ เป็น teammate แบบ stateless
ที่เปิด process ใหม่ในทุก assignment preset เหล่านี้ใช้ headless interface ที่มีเอกสารของ
[Cline](https://docs.cline.bot/usage/cli-overview),
[Antigravity](https://antigravity.google/docs/cli-best-practices),
[Cursor](https://cursor.com/docs/cli/headless),
[Kiro](https://kiro.dev/docs/cli/headless/), [Pi](https://pi.dev/docs/latest) และ
[Grok Build](https://docs.x.ai/build/cli/headless-scripting) ของ xAI เครื่องมืออื่น
รวมถึง Grok CLI ทางเลือก ใช้ custom profile ได้

### เพิ่ม CLI profile ด้วยตนเอง

ใส่ `externalAgents` ใน `~/.zode/config.json` สำหรับทุกโปรเจกต์ หรือใน
`<project>/.zode/config.json` สำหรับโปรเจกต์เดียว object ว่างจะ enable preset ที่รู้จัก
อย่างชัดเจนและ resolve executable ของมันบน `PATH` ที่ sanitize แล้ว:

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

เพิ่มเฉพาะ profile ที่ตั้งใจจะเปิดให้ model เห็น `command` แบบสั้นเช่น `cline`
จะถูก resolve บน `PATH`; path เช่น `./tools/my-agent` หรือ `/opt/agents/my-agent`
ก็ใช้ได้ preset ที่รู้จักรองรับ `enabled`, `command`, `extraArgs`, `envAllow`
และ `trusted`; `extraArgs` จะถูกต่อท้ายคำสั่ง preset ของ Zode

process ของ CLI เริ่มด้วย environment ที่ล้างแล้วซึ่งมีเพียง `PATH`, `HOME` และ
`TERM` (บวกตัวแปรที่จำเป็นบน Windows) จึงต้องเพิ่ม API key หรือตัวแปรที่จำเป็นอื่น ๆ
ลงใน `envAllow` อย่างชัดเจน สถานะ login เดิมภายใต้ `HOME` ยังทำงานได้ entry ของ
project ที่มีชื่อ profile เดียวกันจะแทนที่ entry ส่วนกลางทั้งอัน จึงต้องเขียน override
ทุกตัวที่โปรเจกต์ยังต้องใช้ซ้ำ

custom profile ประกาศ invocation และ protocol ทั้งหมด:

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

`promptTransport` เป็น `stdin`, `argv` หรือ `file`; `argv` ต้องมี argument `{prompt}`
แบบยืนเดี่ยว และ `file` ต้องมี `{prompt_file}` `output` เป็น `text`, generic `jsonl`,
`jsonl-claude` หรือ `jsonl-codex` profile แบบ generic JSONL ใช้ pointer RFC 6901
`textSource` และ `sessionIdSource` เพื่อดึง text ที่ stream มาและ session ID ที่ resume
ได้จาก event ใดก็ตาม `resumeArgs` ต้องมี token `{session_id}` แบบยืนเดี่ยวและจะถูกต่อท้าย
ใน turn ถัดไป; `resumeFlag` ยังคงไว้เป็นรูปแบบย่อ `<flag> <session-id>`

หาก CLI รับ session ID ที่ผู้เรียกเลือกได้ `newSessionArgs` มี token `{session_id}`
แบบยืนเดี่ยวได้ Zode จะสร้าง UUID, ต่อ argument ที่ expand แล้วใน run แรก และใช้
`resumeArgs` ใน assignment ถัดไป นี่ทำให้ CLI แบบ plain-text resume ได้โดยไม่ต้อง
parse ID จาก output

สิ่งนี้ทำให้ headless CLI ใดก็ได้กลายเป็น Task worker หรือ stateless teammate เพื่อคง
context บทสนทนาระหว่าง assignment ของ team มันต้องเปิดเผย session ID เพิ่มเติม หรือ
รับผ่าน `newSessionArgs` พร้อม invocation แบบ resume ที่ non-interactive

`effectiveSandbox` รับค่า `none`, `readOnly`, `workspaceWrite`, `unrestricted`
หรือ `unknown` และจะแสดงใน trust prompt

### จ้างและทำงานกับ teammate

บอก leader ด้วยภาษาปกติ; `team_hire` และ `team_send` เป็นเครื่องมือฝั่ง model
ไม่ใช่ slash command:

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

การจ้างครั้งแรกจะแสดง executable และ argument ที่ resolve แล้ว, working directory
และ sandbox ที่มีผลของ CLI การอนุมัติจะ delegate งานไปยัง process นั้นในโปรเจกต์ปัจจุบัน:
Zode gate การเปิด process แต่ **ไม่** gate ทุก file edit หรือ shell command ที่ external
CLI ทำ การ trust มีผลตลอด Zode session ปัจจุบัน; roster แบบ persistent ถูก recover จาก
`<cwd>/.zode/team/` แต่ external teammate ต้องถูก trust ใหม่หลัง restart หรือ
เปลี่ยน executable

ในการรันแบบ non-interactive/bypass (รวมถึง `--yolo`) Zode แสดง trust prompt ไม่ได้
และจะ fail closed ตั้ง `externalAgents.agents.<profile>.trusted` เป็น `true` เฉพาะเมื่อ
คุณตั้งใจให้ profile นั้นรันโดยไม่มี prompt

ใช้ `/team` เพื่อตรวจ roster และ board หลังจ้าง:

```text
/team                         # roster + board panel
/team status                  # roster แบบข้อความ
/team board                   # goal, notes, assignments และ claims ที่ใช้ร่วมกัน
/team dismiss implementer     # ลบ teammate
```

## คู่มือฟีเจอร์ (automation, session ที่คงทน และ operations)

### รัน headless แบบมีโครงสร้าง

`-p`, `--prompt-file` และ `--prompt-json` ใช้ headless engine เดียวกัน `json`
ปล่อย result object สุดท้ายหนึ่งอัน; `stream-json` ปล่อย JSON object
`zode.run-event.v1` หนึ่งอันต่อบรรทัด โหมดแบบมีโครงสร้างจะสงวน stdout ไว้สำหรับ
output ที่ machine อ่านได้ และใช้ exit code ที่เสถียร: `0` สำเร็จ, `10` provider error,
`11` permission denied, `12` ถึง turn/limit, `13` ถูก interrupt (Ctrl-C),
`14` partial result, `15` session targeting error

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# ID แบบเป๊ะไม่ทำ prefix-match; fork ไม่แก้ไข source session
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

pattern deny ของ tool ชนะ pattern allow และถูกสืบทอดโดย Task sub-agent
`--permission-mode` รับ `default`, `dont-ask`, `accept-edits` และ `bypass`;
`--yolo` ยังเป็นทางลัดของ bypass ขณะที่กฎ hard deny ยังมีผลเสมอ

### ต่อยอด Session V1 โดยตรง

transcript ยังคงเป็นไฟล์ V1 เดิมที่ `~/.zode/sessions/<id>.jsonl` และเป็น
transcript สำเนา **เดียว** จึงทำให้ Zode client รุ่นเก่าอ่านและเขียนต่อได้ metadata
ใหม่เป็นแบบ additive และอยู่ใน `~/.zode/sessions/<id>/` (`meta.json`, journal,
checkpoints และ snapshots) ไม่ต้องมี session format ใหม่หรือ migrate transcript

```bash
zode session list
zode session list --json
zode session show <id>                         # metadata + checkpoint ID
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # preview แบบรู้ conflict
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

checkpoint จะถูกจับก่อน turn ที่มีการแก้ไข rewind จะ restore เนื้อหาไฟล์ที่ track ไว้
และ prefix ของ transcript, รายงาน conflict แทนที่จะเขียนทับการเปลี่ยนแปลงใหม่กว่า
และบันทึกเป็น branch เชิงตรรกะใหม่แทนการลบประวัติ ผลของ worktree fork สามารถ
apply กลับได้อย่างชัดเจนเมื่อการทดลองพร้อม

**การ compact ไม่ทำให้บทสนทนาที่มองเห็นหายไป** เมื่อการ compact context
แทนที่ข้อความเก่าด้วย summary ต้นฉบับจะถูกเก็บไว้ใน sidecar แบบ additive
(`~/.zode/sessions/<id>/compacted.jsonl`) การ resume session, การกด `Ctrl+L`,
`/export` และ side panel ของ Chrome จะแสดงประวัติเต็มก่อน compact
ขณะที่โมเดลยังได้รับเฉพาะ context ที่ compact แล้ว fork จะพกไฟล์เก็บถาวรไปด้วย
(กรองตาม transcript ของตัวเอง), `/clear` จะลบมัน และการลบ session จะลบ
sidecar ทั้งหมด

### Permission rules และ sandbox profiles

กฎอยู่ใน `permissions.rules` ใน `config.json` ได้ หรืออยู่ในไฟล์ JSON เดี่ยวที่ส่งด้วย
`--rules` field matcher ใช้ RFC 6901 JSON pointer; deny มาก่อน ask ซึ่งมาก่อน allow
ไฟล์เดี่ยวต้องเป็น array ของ rule หรือ `{ "rules": [...] }`; ไม่ต้องห่อด้วย object
`permissions` ระดับบนสุด

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

profile ในตัวคือ `read-only`, `workspace`, `workspace-network` และ `unconfined`
profile ที่นิยามใน config ใช้ field เดียวกับตัวอย่างข้างบน

### Plugins และ static marketplaces

managed plugin สามารถ contribute skills, commands, agents, hooks, MCP servers,
LSP servers และ sandboxed JavaScript UI renderers Zode รับ `plugin.json`,
`.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.grok-plugin/plugin.json`
และ `.claude-plugin/plugin.json` รองรับ array ของ component path แบบ Codex และ
Claude Code และเคารพ `defaultEnabled` ของ Claude Code เมื่อติดตั้งครั้งแรก component
เฉพาะ host เช่น Codex apps/connectors และ Claude Code themes, monitors หรือ
output styles จะถูกละไว้; plugin ที่มีแต่ app จะถูกปฏิเสธเพราะไม่มี component ที่ใช้กับ
Zode ได้ การติดตั้งเป็น immutable snapshot พร้อม provenance และ SHA-256 tree hash
เนื้อหา plugin ที่ executable จะไม่ถูก activate หากไม่มี flag `--trust` อย่างชัดเจน

#### เริ่มต้น JavaScript UI plugin อย่างรวดเร็ว

UI plugin ที่เล็กที่สุดมี manifest และไฟล์ JavaScript หนึ่งไฟล์:

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

ติดตั้ง local directory หรือ GitHub repository/subdirectory แล้ว restart process Zode
ที่กำลังรันเพื่อให้โหลด snapshot ใหม่:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

ใช้ `zode plugin update my-plugin` หลังแก้ไข source ต้องใช้ `--trust` เพราะ JavaScript,
hooks, MCP servers และ network access ที่ประกาศไว้เป็นความสามารถแบบ executable
การติดตั้งและ update จะพิมพ์ permission ที่ plugin ประกาศ (network hosts, env vars,
context scopes) update ที่ manifest ขอ permission *กว้างกว่า* snapshot ที่ติดตั้งไว้จะถูก
ปฏิเสธ เว้นแต่คุณรันซ้ำด้วย `--trust` — Git source ที่เคลื่อนไหวอยู่จะขยายสิทธิ์ของตัวเอง
แบบเงียบ ๆ ไม่ได้

#### UI render API

UI plugin สามารถ contribute row แบบ declarative เหนือหมายเลขเวอร์ชันใน sidebar
ทันที — รวมสูงสุด 6 row ใช้ร่วมกันทุก plugin ตามลำดับการโหลด ประกาศ JavaScript
entrypoint ใน manifest:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

register renderer แบบ synchronous ด้วย `zode.ui.sidebar` context เป็น read-only
JSON snapshot ที่มี field ของ terminal, session, model, status, token และ
context-window ผลลัพธ์ถูก render โดย Zode; script ไม่ได้รับ bridge ของ filesystem,
network, terminal หรือ Ratatui

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

tone ที่รองรับคือ `default`, `muted`, `accent`, `success`, `warning` และ `danger`;
span ยังรับ `bold` และ `italic` renderer ต้องเป็น synchronous แต่ละ script จำกัดที่
256 KiB, JS memory 8 MiB และ 25 ms ต่อการ evaluate และ renderer จะถูก re-evaluate
อย่างมากทุก 250 ms (output ที่ cache ถูกใช้ซ้ำระหว่างการ evaluate) output ของ sidebar
จำกัด 6 บรรทัดต่อ renderer (รวม 6 บรรทัดทุก plugin), แต่ละบรรทัด 16 span และ text
2,048 byte control character ถูก sanitize โดย host

status bar ก็ขยายได้ มันยังคงเป็น 1 row เมื่อไม่มี plugin คืน content และขยายเป็น 2 row
อย่าง dynamic เมื่อ renderer `zode.ui.statusLine` แบบ synchronous คืน span Zode เก็บ
core status และ safety indicator ไว้ที่ row แรก; output ของ plugin ถูกจัดวางที่ row ที่สอง

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Render context และ permissions

renderer ทุกตัวได้รับ field พื้นฐานต่อไปนี้โดยไม่ต้องขอ context permission เพิ่ม:

| Field | โครงสร้างและความหมาย |
| --- | --- |
| `ctx.apiVersion` | เวอร์ชัน Context API; ปัจจุบัน `1` |
| `ctx.app` | `{ version, effort }` |
| `ctx.terminal` | `{ width, height }` หน่วยเป็น terminal cell |
| `ctx.session` | `{ id, title, cwd, busy }` ของ task ที่ active |
| `ctx.model` | `{ id, provider }` |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` มี `{ enabled, readOnly, network }` |
| `ctx.tokens` | ตัวนับ `{ input, output }` token |
| `ctx.context` | `{ used, window, usedPercent }`; เปอร์เซ็นต์อาจเป็น `null` |
| `ctx.data` | ผลลัพธ์เฉพาะจาก data source ที่ plugin นี้ register เท่านั้น |

section ที่ละเอียดกว่านี้จะถูกละไว้ เว้นแต่ plugin ขอ scope ที่ตรงกันใน
`permissions.context`:

| Scope | Field ที่เปิดเผย | โครงสร้างและข้อจำกัด |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` เริ่มจาก 1 |
| `workspace` | `ctx.workspace.modifiedFiles` | Git `{ path, added, removed }` สูงสุด 50 entry |
| `tools` | `ctx.tools.available` | ชื่อ tool ที่เปิดใช้สำหรับ task ปัจจุบัน เรียงแล้ว |
| `tools` | `ctx.tools.active` | ชื่อ tool ที่กำลังทำงานอยู่ |
| `tools` | `ctx.tools.recent` | record `{ name, status, durationMs }` สูงสุด 20 อัน |
| `tasks` | `ctx.tasks.todoStatuses` | เฉพาะ string สถานะ todo ไม่มีข้อความ todo |
| `tasks` | `ctx.tasks.subagents` | record `{ type, status }` ไม่มี prompt หรือ transcript |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }` ไม่มีข้อความ goal |
| `services` | `ctx.services.mcp` | record `{ name, connected }` |
| `services` | `ctx.services.lsp` | record `{ language, running }` |

ตัวอย่าง:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` เป็น API เชิงสังเกต: มันบอก renderer ว่ามี tool อะไรบ้างและ tool ใดกำลัง
ทำงานหรือเคยทำงาน UI plugin เรียก tool ไม่ได้ input/output ของ tool, prompt, เนื้อหา
transcript, ข้อความ todo/goal, ค่า environment และ credential ไม่ถูกรวมไว้ และ API นี้
ข้าม approval system ของ Zode ไม่ได้

#### Background HTTP data

UI plugin ยัง register background HTTP data source ได้ ต้องประกาศ network และ secret
access ใน manifest:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

request เป็นแบบ declarative และรันนอก render path secret environment variable ถูก
ประกอบเข้า header โดย Zode และไม่เคยถูกเปิดเผยต่อ JavaScript:

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

`zode.data.define(key, config)` รับ key ยาว 1–64 อักขระที่เป็นตัวอักษร/ตัวเลข,
underscore หรือ hyphen `request` รองรับ `url`, `method`, `headers`, `body` แบบ JSON
เป็น option และ `timeoutMs` ค่าเริ่มต้นคือ `GET`, timeout 3 วินาที และ refresh 60 วินาที
รับเฉพาะ HTTPS `GET` และ `POST` header แบบ literal เป็น string; secret header ใช้
`{ "env": "NAME", "prefix": "Bearer " }` environment variable ต้องปรากฏใน
`permissions.env` ด้วย จะถูกอ่านโดย Rust เท่านั้นตอนสร้าง request และไม่เคยถูกคืนสู่
JavaScript

Zode ปิด redirect และ proxy, validate และ pin public DNS address, ปฏิเสธ
localhost/private network, จำกัด response ที่ 256 KiB, clamp request timeout ที่
500 ms–10 วินาที และ clamp refresh interval ที่ 10 วินาที–1 ชั่วโมง wildcard เช่น
`*.example.com` จับ subdomain แต่ไม่จับ host เปล่า `example.com`

แต่ละ plugin เห็นเฉพาะ data ของตัวเอง `ctx.data.<key>` มี
`{ ok, status, data, updatedAt }` หรือ `{ ok: false, error, updatedAt }`
response แบบ JSON กลายเป็น object/array; response ที่ไม่ใช่ JSON กลายเป็น string
HTTP error status ยังมี `status` และ `data` พร้อม `ok: false`

เริ่ม Zode พร้อม secret ที่จำเป็นใน environment เมื่อใช้ private quota หรือ
coding-plan API:

```bash
CODING_PLAN_TOKEN=... zode
```

[ตัวอย่างที่รันได้ครบ](../../examples/plugins/zode-ui-demo/) แสดง model/context/tool
activity ใน sidebar และ status line และใช้ `zode.data.define` สำหรับ GitHub API quota
สาธารณะ

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# marketplace เป็น static index แบบ local/Git ไม่ใช่บริการที่ Zode host
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # ระบุแหล่งเมื่อจำเป็น
zode plugin marketplace update
```

### ACP, dashboard, telemetry และ TUI regression tests

`zode acp` implement ACP initialize/new/load/fork/prompt/cancel ผ่าน stdio, stream
message/thought/tool update, ขอ permission ผ่าน client และรับ MCP server แบบ stdio,
HTTP และ SSE ที่ client ส่งมา ข้อมูล session ใช้ store แบบ V1-compatible เดียวกับ TUI
และ headless CLI

```bash
zode acp
zode dashboard
zode dashboard --json
```

OTLP export ปิดเป็นค่าเริ่มต้นและต้อง opt-in อย่างชัดเจน มัน export เฉพาะ attribute
lifecycle/tool-name/status/usage ที่ไม่มีเนื้อหา: prompt, ข้อความที่ generate,
tool input/output, file path และ error message ไม่เคยถูกส่ง

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

สำหรับ TUI regression scenario บน terminal จริง workspace มี PTY + VT100 harness
ที่บันทึก raw diagnostics และ virtual-screen snapshot:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` ขับ terminal จริงด้วยลำดับ wait, key input, resize และ snapshot
(notation ของ key รองรับ `<Enter>`, `<Esc>`, `<Tab>`, `<Up>`, `<Down>`, `<Left>`,
`<Right>`, `<Backspace>`, `<C-c>`, `<C-d>` และ `<C-l>`):

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

implementation แบบ local/open นี้จงใจไม่รวมบัญชีเฉพาะ xAI, billing หรือ cloud
marketplace service ที่ดำเนินการโดย Zode

## Configuration

`providers` คือ source of truth ของ provider definitions ส่วน `provider` ระดับบนสุด
ชี้ active model โดย OpenAI-compatible providers มักใช้ `baseUrl` และ `dialect`:

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

หนึ่ง provider มีหลาย models ได้ และเปลี่ยนสดด้วย `/model` ส่วนภาษาเปลี่ยนได้ด้วย
`/language`

key config ระดับบนสุดที่เป็น option (ทุกตัวมีค่าเริ่มต้นที่เหมาะสม):

```jsonc
{
  "maxOutputTokens": 16384,      // เพดาน output ต่อ turn (เพิ่มสำหรับการเขียนไฟล์ใหญ่)
  "contextWindow": 1000000,      // context window ของโมเดล — ตั้ง 1000000 สำหรับโมเดล 1M
  "temperature": 0,              // ต่ำ = deterministic มากขึ้น
  "language": "th",              // ภาษา UI (15 locale); ผ่าน /language ได้เช่นกัน
  "effort": "medium",            // reasoning effort; บน Anthropic ค่า medium/high จะ map เป็น thinking budget จริง
  "autonomousOrchestration": true, // orchestration ของ sub-agent + workflow (เปิดเป็นค่าเริ่มต้น)
  "subagentMaxIterations": 0,      // guard ของ child เป็น option; ละไว้/0 = ไม่จำกัด
  "tools": {
    "deferNonCore": false        // true: คงเครื่องมือใช้ประจำ ~20 ตัวให้มองเห็น ที่เหลือ defer ไว้หลัง ToolSearch
  },
  "webSearch": {
    "tavilyApiKey": null         // เปิดใช้เครื่องมือ WebSearch (หรือตั้ง $TAVILY_API_KEY)
  },
  "sandbox": {
    "enabled": true,             // OS sandbox สำหรับ shell commands (เปิดเป็นค่าเริ่มต้น)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // อนุญาต outbound network ใน sandbox
    "writableRoots": []          // dir ที่เขียนได้เพิ่ม (workspace-write)
  },
  "browser": {
    "enabled": true,             // เครื่องมือ browser_* และ /browser panel (เปิดเป็นค่าเริ่มต้น)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // โหมด launch ของ managed Chromium
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // เฝ้า turn /loop และ /schedule ที่ไม่มีคนดู
    "inactivityTimeoutSecs": 900, // abort หลัง 15 นาทีที่ไม่มี provider/tool activity
    "maxRuntimeSecs": 3600,      // เพดานสัมบูรณ์หนึ่งชั่วโมงต่อ background turn
    "abortGraceSecs": 10,        // รอ cancellation แบบร่วมมือก่อน hard-stop
    "maxRetries": 3,             // จำนวน recovery ติดกันก่อนหมดโควตา
    "initialBackoffSecs": 5,     // ดีเลย์ retry ครั้งแรก
    "maxBackoffSecs": 300        // เพดาน backoff แบบ exponential
  }
}
```

> sandbox จำกัด shell commands (macOS: sandbox-exec; Linux: `bwrap` ซึ่งต้องติดตั้งไว้)
> การเริ่มโปรแกรมจะ fail closed หาก verify sandbox ที่ตั้งค่าไว้ไม่ได้; ใช้ flag
> `--no-sandbox` อย่างชัดเจนเพื่อรันโดยไม่มีมัน network ถูก deny เป็นค่าเริ่มต้น
> หากคำสั่งจำเป็นต้อง escape จริง ๆ model ตั้ง `dangerouslyDisableSandbox: true`
> และ **คุณ** อนุมัติที่ approval prompt — หรือสลับ sandbox ทั้งหมดสดด้วย `/sandbox`

> `contextWindow` ขับ auto-compaction — ตั้งให้ตรงกับ window จริงของโมเดล (เช่น
> `1000000`) แนะนำให้ใช้ค่า **ต่อโมเดล** ที่
> `providers.<name>.models.<id>.contextWindow` (มีลำดับความสำคัญสูงกว่า); key ระดับ
> บนสุดข้างบนเป็น fallback ส่วนกลาง และ zode ยังเติมจาก catalog models.dev ที่ bundle
> มาเมื่อไม่ได้ตั้งทั้งสองที่ อย่าตั้งให้เกิน window จริง: การประเมินเกินจริงทำให้ request
> overflow และ provider ปฏิเสธ turn

## Server mode และ SDK

`zode server` เปิด newline-delimited JSON-RPC server บน stdin/stdout ออกแบบมาสำหรับ
editor integration, local automation, tests และ SDK client ที่ต้องการความสามารถเดิม
ของ zode โดยไม่ต้องเปิด TUI

```bash
zode server                      # stdio (ค่าเริ่มต้น) — สิ่งที่ SDK spawn
zode server --listen stdio://    # อย่างเดียวกัน เขียนเต็ม
zode server --listen ws://127.0.0.1:0   # loopback WebSocket + Bearer auth
zode server --listen off         # ไม่เริ่มอะไรเลยแล้วออก
```

Server mode เปิดเผยพฤติกรรมที่ขับด้วย zode:

- initialization + capability discovery (พร้อม `approvalPolicy` เป็น
  `readOnly` (ค่าเริ่มต้น) / `auto` / `prompt`)
- lifecycle ของ thread metadata และ **streaming turns** — output ของโมเดลและ tool
  call มาเป็น JSON-RPC notification; `turn/interrupt` ยกเลิก turn
- **interactive approvals** — policy `prompt` ขับ frame `approval/request`
  จาก server→client ที่ตอบด้วย `allow` / `allowAlways` / `deny`
- filesystem read/write/create/stat/list/remove/copy และ one-shot `command/exec`
- model list/set, config read/list/write และ skills, hooks, สถานะ MCP-server
  และ plugin list แบบ read-only

WebSocket transport bind เฉพาะ loopback และเขียนไฟล์ credential `0600`
`<config-dir>/server.json` (`{port, pid, token}`); client authenticate ด้วย
`Authorization: Bearer <token>` ดู [`sdk/README.md`](../../sdk/README.md) สำหรับ
protocol เต็ม, ชื่อ field ของ notification และตัวอย่างต่อภาษา

สำหรับ app-server protocol นี้โดยเฉพาะ การจัดการ hosted marketplace, remote-control,
Realtime, standalone process spawn, background terminals, thread archive/fork, goals
และ app connector ยังอยู่นอกขอบเขต คำสั่ง session ในเครื่องและ static-plugin
marketplace ที่บันทึกไว้ข้างต้นเป็น CLI surface แยกต่างหาก

SDK อยู่ใน [`sdk/`](../../sdk/):

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

SDK แต่ละตัวเปิดเผยชุด `ProtocolMethod` enum/constant แบบ native สำหรับชื่อ method
เสถียรปัจจุบัน จึงหลีกเลี่ยง JSON-RPC string แบบ hard-code ได้ params, รูปร่าง result
และชื่อ SDK enum/constant ของทุก method ที่รองรับมีบันทึกไว้ใน
[method reference ของ `sdk/`](../../sdk/README.md#method-reference)

## Browser control

Zode มี tool group `tools:browser` สำหรับ automation ของ browser agent ใช้
`browser_read` สำหรับ screenshot, DOM snapshot, console log, network log และการอ่าน
tab; `browser_act` สำหรับ navigate, click, type, key press และ scroll; `browser_eval`
สำหรับ JavaScript; และ `browser_tabs` สำหรับจัดการ tab การตรวจ browser แบบ read-only
ไม่ต้องผ่าน gate; browser action ที่มีการแก้ไขใช้ flow อนุมัติ allow-once / always / deny
เดียวกับ tool ที่มี side effect อื่น ๆ

มี browser target สองแบบ:

- **managed** — zode launch และควบคุม Chromium profile เฉพาะ
- **bridge** — zode ควบคุม Chrome profile ที่คุณใช้อยู่แล้วผ่าน MV3 extension ที่
  bundle มาใน [`extensions/chrome/`](../../extensions/chrome/)

สำหรับ target bridge ให้โหลด extension จาก `extensions/chrome` ครั้งเดียว แล้วรัน
`/browser pair` Chrome จะบล็อก URL `chrome-extension://` ที่เปิดโดยโปรแกรมภายนอก
(ERR_BLOCKED_BY_CLIENT — เหมือนกันทั้งบน macOS, Windows และ Linux) ดังนั้นความ
พยายามของ zode เองที่จะเปิดหน้านั้นอาจล้มเหลว — แต่ extension จะเปิดหน้า pairing
ของตัวเองภายในราว 30 วินาทีหลัง `/browser pair` โดยกรอก port ให้ล่วงหน้าแล้ว
ให้กรอก pairing code 6 หลักที่แสดงในแชท หรือใช้วิธีสำรองด้วยการพิมพ์ URL
`chrome-extension://…/popup.html?port=…` ลงในแถบที่อยู่ด้วยตัวเอง (การนำทาง
ที่พิมพ์เองถือว่าเริ่มโดย browser จึงไม่ถูกบล็อก) **การ pair ทำเพียงครั้งเดียว**: extension
เก็บ token ระยะยาวไว้และ reconnect อัตโนมัติ — ตอน browser เริ่มทำงาน ตอน
extension อัปเดต และ retry ราวทุก 30 วินาทีระหว่างที่ขาดการเชื่อมต่อ — ดังนั้นการ
restart zode ไม่ต้อง pair ใหม่ มันจะ reconnect กับ CLI ที่รันอยู่
หรือ auto-start zode daemon แบบ extension-only เมื่อจำเป็น tab ที่ zode เปิดจะถูกวางใน
Chrome tab group ชื่อ `zode`

### Side panel ของ Chrome task

รัน zode CLI ที่อัปเดตแล้วและ `/browser pair` ครั้งเดียว การคลิก toolbar icon จะเปิด
side panel; หลังจากนั้นมันจะ auto-start zode อัตโนมัติเมื่อไม่มี process CLI ทำงานอยู่
หน้า pairing ยังเป็น code/token flow เล็ก ๆ และ task ยังใช้ร่วมกับ TUI session โดยไม่
เปลี่ยน focus ของ terminal

turn จาก side panel จะ bind browser tool ของ bridge เข้ากับหน้าที่แสดงอยู่ข้าง panel
ขณะนั้น จึงทำให้ request เช่น "วิเคราะห์หน้านี้" ใช้ `browser_read` บน tab ที่มีอยู่แทน
การเปิด tab ใหม่ ส่วน automation ของ browser แบบ standalone TUI และ CLI ยังใช้ tab ที่
zode เป็นเจ้าของใน tab group `zode` หน้าที่ active ยังเป็น context เริ่มต้นสำหรับ prompt
กำกวมจาก side panel; ไฟล์โปรเจกต์ในเครื่องจะถูกตรวจเฉพาะเมื่อผู้ใช้ถามถึงมันอย่างชัดเจน

panel ส่งข้อความได้, เลือกโมเดล, เลือก access mode `readOnly`, `prompt` และ `auto`,
stream response และ Stop turn ที่กำลังรัน หนึ่ง turn แนบไฟล์ได้มากสุด 8 ไฟล์ รวม 20 MiB:
รูป PNG, JPEG, GIF และ WebP ไฟล์ละไม่เกิน 5 MiB บวก text UTF-8 และไฟล์ code ไฟล์ละ
ไม่เกิน 1 MiB ส่วน PDF, Office, archive, executable และ input ที่ไม่ใช่ UTF-8 จะถูกปฏิเสธ

หลังอัปเดต extension ให้คลิก Reload ที่ `chrome://extensions` extension เวอร์ชันเก่ายัง
ใช้ browser automation ได้แต่ไม่มี task side panel บน Windows zode จะค้นหาและ launch
Chrome โดยตรงสำหรับ extension URL แทนการเรียก shell ของ default browser เพื่อเลี่ยงการ
redirect ไป Microsoft Store เมื่อ Chrome ติดตั้งไว้แล้ว

คำสั่งที่มีประโยชน์:

```bash
/browser                         # เปิด browser control panel
/browser status                  # แสดงสถานะ target/running/paired
/browser launch                  # launch managed browser
/browser close                   # ปิด managed browser
/browser pair                    # pair หรือ reconnect Chrome bridge extension
/browser target managed          # ใช้ managed Chromium ของ zode
/browser target bridge           # ใช้ extension และบันทึกเป็น default ครั้งถัดไป
/browser screenshot [path]       # ถ่าย screenshot ของ browser
```

ดู [`extensions/chrome/README.md`](../../extensions/chrome/README.md) สำหรับขั้นตอน
โหลด extension, update, CRX packaging และ smoke-test

## Desktop control

Zode ขับ native desktop application ผ่าน accessibility API ของ OS ได้ ไม่จำกัดแค่
browser agent ใช้ `desktop_read` เพื่ออ่าน accessibility tree (windows, elements และ
ref ของมัน), `desktop_act` เพื่อ click, type, scroll และ set value ตาม element และ
`desktop_screenshot` เพื่อถ่ายหน้าจอ การอ่านแบบ read-only ไม่ต้องผ่าน gate; desktop
action ที่มีการแก้ไขใช้ flow อนุมัติ allow-once / always / deny เดียวกับ tool ที่มี side
effect อื่น ๆ

backend ถูกเลือกตาม platform:

- **macOS** — Accessibility (AX) API
- **Windows** — UI Automation (UIA)
- **Linux** — AT-SPI
- **Electron apps** — attach ผ่าน Chrome DevTools Protocol

**Ghost cursor และ Esc stop.** Zode ไม่เคยขยับเมาส์จริงของคุณ บน macOS overlay ที่ไม่
ต้องขอ permission (`zode-overlay`) จะวาด cursor *ปลอม* ที่บินไปตาม Dubins path ที่
ราบรื่นสู่เป้าหมายของแต่ละ action เพื่อให้คุณตามได้ว่า agent ทำอะไร; ข้อความที่พิมพ์ไม่เคย
แสดงใน overlay ขณะที่ desktop automation ทำงานอยู่ **Esc** แบบ global จะ interrupt ทุก
turn ที่กำลังรันและซ่อน overlay (เส้นทาง stop เดียวกับ Esc ของ TUI) platform อื่นรัน
desktop action โดยไม่มี visualization

CJK และ text อื่นที่ไม่มี keycode แบบ US-layout จะถูกส่งผ่าน system pasteboard
(write → synthesize paste → restore clipboard เดิม) เพื่อให้แอปที่มี key handling
เฉพาะทางได้รับอักขระจริง

```bash
/desktop            # แสดง desktop target และสถานะ permission
/desktop status     # อย่างเดียวกัน แบบชัดเจน
```

config อยู่ภายใต้ `desktop.*` ใน `~/.zode/config.json`:

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor` (ค่าเริ่มต้น `true`) วาด overlay cursor ของ macOS; `escCancel`
(ค่าเริ่มต้น `true`) arm การ interrupt แบบ global-Esc ระหว่าง automation;
`overlayHelperPath` (ค่าเริ่มต้น `null`) override ตำแหน่ง helper `zode-overlay` —
helper ที่หายไปเพียงปิด visualization desktop automation อาจขอ permission ของ OS
(เช่น macOS Accessibility) ในการใช้ครั้งแรก

## Background Turn Watchdog

turn `/loop` และ `/schedule` ที่ scheduler เป็นเจ้าของรันภายใต้ liveness watchdog
แบบ in-process activity ของ provider, tool และ nested-agent จะ refresh heartbeat ฝั่ง
source ที่ใช้ร่วมกัน ขณะที่ `maxRuntimeSecs` ยังเป็นเพดานสัมบูรณ์ เมื่อ timeout ตัวใด
ตัวหนึ่ง zode ขอ cancellation แบบร่วมมือ, รอ `abortGraceSecs` แล้ว hard-stop task ของ
turn ในเครื่องหากยังไม่ drain การหยุด task ยังไม่พอที่จะปล่อย scheduler slot: zode ยังรอ
ให้ provider, tool, hook, subprocess reader และ nested-agent worker ที่ track ไว้ทุกตัว
quiesce หากไม่ถึง boundary ที่สองนั้นภายใน 5 วินาที tab/store จะถูก quarantine, job ถูก
disable และ live-attempt lease ของมันยังถูกถือไว้จนกว่า worker จะออกจริง

attempt ที่ล้มเหลวใช้ exponential backoff แบบมีขอบเขตจาก `initialBackoffSecs` ถึง
`maxBackoffSecs` turn ที่สำเร็จจะเคลียร์นับ failure ติดกัน; เมื่อ `maxRetries` หมด zode
หยุด loop หรือ disable schedule ที่ persist ไว้ การ interrupt ด้วยมือ, การลบ job และการ
disable อย่างชัดเจนจะยกเลิก recovery ที่ค้างอยู่แทนการสร้าง retry ใหม่เมื่อยังไม่มี mutation
เริ่ม recovery ระมัดระวังกับ side effect โดยตั้งใจ: zode retry อัตโนมัติเฉพาะเมื่อไม่พบ
side effect; หาก mutation อาจเกิดขึ้นแล้ว รวมถึง cancellation กลาง mutation มันจะ
stop/disable job และรอ human review tool ที่ detach งานโดยตั้งใจ (`BashRun` หรือ GUI ที่
detach) ก็จะหยุด recurrence หลัง turn นั้น inactivity limit เดียวกันจำกัดคิว
claim-to-start: หาก tab ที่ busy หรือ turn preflight ทำให้ occurrence ที่ถือครองอยู่เริ่ม
ไม่ได้ มันจะกลายเป็น watchdog failure แบบไม่มี side effect และเข้าสู่ policy retry แบบมี
ขอบเขตเดียวกันแทนการถือ cross-process lease ตลอดไป

Quiescence เป็นการรับประกันในเครื่อง งานที่ remote MCP server, browser extension,
desktop actor หรือระบบภายนอกอื่นรับไปแล้วอาจไม่รองรับการ revoke หาก call เช่นนั้นถูก
interrupt zode จะ mark ผลเป็น unresolved, disable scheduler job และให้คุณ verify สถานะ
ภายนอกก่อนเปิดใช้อีกครั้ง

ใช้ `/watchdog status` สำหรับ configuration และ health ต่อ turn/retry สถานะเดียวกัน
ปรากฏใน `/tasks` ควบคู่กับ background shell และ turn ที่กำลังรัน; อายุ queue ที่ claim
แล้วและ terminal-persistence fence ก็แสดงที่นั่นด้วย

นี่เป็น watchdog สำหรับ scheduler turn ภายใน process zode ปัจจุบัน มันไม่ใช่ OS process
supervisor และ restart zode หลัง crash หรือ machine restart ไม่ได้; ใช้ service manager
ของ platform เมื่อต้องการ restart ระดับ process schedule ที่ persist ไว้บันทึก
active-attempt token ที่หนุนด้วย OS file lock ต่อ schedule ตอน startup lock ที่มีการ
แย่งชิงจะถูกปล่อยไว้เพราะ process zode อื่นยังถือครอง; lock ที่ว่างและมี token ที่ persist
ตรงกันเป๊ะคือ orphan จากการออกที่ไม่สะอาด zode จึง disable schedule นั้นว่า
execution-state-unknown แทนการ replay เงียบ ๆ สัญญา recovery นี้ครอบคลุม process
crash ไม่รับประกัน durability ระดับ storage เมื่อไฟดับกะทันหันหรือ hardware เสีย และ
ไม่แทนที่ OS service manager

fire timestamp และ active-attempt token ถูก claim อย่าง atomic ก่อน persisted prompt
เข้าคิว tab จึงทำให้งานในคิว exclusive ข้าม process zode อยู่แล้ว lease เดียวกันเคลื่อนไป
กับ prompt เข้าสู่ turn และถูกถือไว้จน transcript/index persistence สุดท้าย การ edit,
remove หรือ disable occurrence ในคิวเป็น cancellation อย่างชัดเจนและเคลียร์เฉพาะ active
token ที่ตรงกัน การออกโปรแกรมแบบ graceful กลับ restore fire watermark ที่ยังไม่เริ่มหรือ
retry token ให้ตรงเป๊ะ จึงไม่กิน work ที่ไม่เคยรัน การเขียน roster ปลายทางที่ล้มเหลวจะเก็บ
lease ไว้ใน finalizer ที่ retry; token ที่ขัดแย้งจะถูก disable แบบถาวรเพื่อ review ก่อนปล่อย
scheduler turn ข้าม detached post-turn memory extraction และการออกแบบ graceful จะ
drain worker quiescence บวก terminal persistence ก่อนทำลาย tab phase ของ recurrence
เป็น canonical: interval slot ใช้ absolute epoch arithmetic จาก anchor ที่ persist ไว้
(รวมถึงข้าม DST fallback), calendar schedule คง wall-clock phase และ backlog ที่พลาดจะ
coalesce สู่ due slot ล่าสุด process ที่รันอยู่ยัง refresh roster เพื่อให้การ disable/remove
จากภายนอก, retry และการเปลี่ยนเจ้าของ orphan มีผลโดยไม่ต้อง restart

## Slash commands ที่ใช้บ่อย

| Command | ใช้ทำอะไร |
|---|---|
| `/help` | overlay ของ commands + keybindings |
| `/clear` | ล้าง conversation (และ context) |
| `/model [id]` | แสดง / บันทึกโมเดลที่ active |
| `/config` | แสดงโมเดล + working directory |
| `/compact` | สถานะ context auto-compaction |
| `/cost` | token usage และ cost จนถึงตอนนี้ (รวม sub-agents) |
| `/theme [id]` | สลับธีม (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | session picker — resume เข้า tab ใหม่พร้อมประวัติ |
| `/connect` | connect และสลับ provider ที่ active |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | แสดง/ซ่อน sidebar ขวา; พับ section MCP / modified-files / todo |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | browser control panel และคำสั่ง; pair Chrome bridge extension หรือสลับระหว่าง managed Chromium กับ Chrome profile ของคุณ |
| `/loop <interval> [--max N] <prompt>` | รัน prompt แบบซ้ำใน tab ปัจจุบัน; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | persist prompt ที่ตั้งเวลา; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | แสดง configuration, health และ retry ที่ค้างของ background-turn watchdog |
| `/tasks` | panel background shell, turn ที่กำลังรัน และ watchdog health |
| `/undo`, `/redo` | undo / redo การแก้ไขไฟล์ล่าสุด |
| `/mcp` | จัดการ MCP servers — enable / disable ใน dialog |
| `/skills` | รายการ skills ที่มี |
| `/agents` | จัดการ sub-agents — สร้าง (AI-assisted หรือ manual) / ลบ |
| `/external-agents [list\|discover]` | ดู external CLI ที่รองรับใน `PATH` หรือลงทะเบียน preset ที่พบทั้งหมดอย่างชัดเจน |
| `/team [status\|board\|dismiss <name>]` | ตรวจ roster ของ teammate และ shared board หรือลบ teammate |
| `/workflows` | จัดการและรัน workflow ที่ scripted ด้วย JS |
| `/effort` | เลือกระดับ reasoning effort |
| `/thinking`, `/tool-details` | สลับการแสดง reasoning / รายละเอียด tool-call |
| `/orchestration` | สลับ orchestration ของ sub-agent + workflow แบบ autonomous |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | แสดง / ควบคุม OS sandbox ตอน runtime |
| `/language` | สลับภาษา UI (15 locale) |
| `/export [path]` | export transcript เป็น Markdown |
| `/yolo` | โหมด bypass-approval |
| `/exit` | ออก |

agents และ skills ที่สร้างขึ้น รวมถึง MCP tool ที่เชื่อมต่อ ก็ปรากฏเป็น slash command
แบบ dynamic (เช่น `/<name>`) และเรียกได้โดยตรง ตารางเต็มดูได้ใน
[README ภาษาอังกฤษ](../../README.md#slash-commands)

## Keybindings

> บน macOS chord ของแอปด้านล่างใช้ **`Cmd`** (⌘); บน Windows/Linux ใช้ `Ctrl`
> ส่วน `Ctrl+C/D/L/V` ยังเป็น `Ctrl` ทุกที่ (ตาม convention ของ terminal)

| Key | Action |
|---|---|
| `Enter` | ส่งข้อความ (queue หาก turn กำลังรัน) |
| `Shift`/`Alt`+`Enter` | ขึ้นบรรทัดใหม่ |
| `Up` / `Down` | เรียก prompt ก่อนหน้า / ถัดไป (หรือเลื่อน autocomplete) |
| `Ctrl+C` | interrupt turn (ออกเมื่อ idle) |
| `Ctrl+D` | ออก |
| `Ctrl+L` | วาด conversation ใหม่จาก store (กู้ view ที่ว่างเปล่า; ใช้ `/clear` เพื่อทิ้ง) |
| `Ctrl+V` | paste (text หรือ path ของรูป) |
| `Cmd/Ctrl+O` | Settings |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | tab ใหม่ / ปิด tab |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | กระโดดไป / วน tab |
| `Cmd/Ctrl+B` | panel background tasks |
| `Cmd/Ctrl+G` | สลับ sidebar |
| `F1` | Help |
| `PgUp` / `PgDn` | เลื่อน conversation |
| `Home` / `End` | กระโดดไปบนสุด / ล่าสุดของ conversation |
| `Esc` | ปิด overlay ปัจจุบัน (หรือ interrupt turn ที่กำลังรัน) |

## Project instructions

Zode อ่าน instructions จากลำดับชั้นสามระดับ (ระดับหลังชนะความสนใจ): global
`~/.zode/AGENTS.md` (หรือ `instructions.md`) → project root → cwd ในแต่ละ directory
มัน prefer `AGENTS.md` ก่อน `CLAUDE.md` Skills อยู่ใน `.zode/skills/**/SKILL.md`;
MCP servers ใน `~/.zode/mcp.json` ⊕ `.mcp.json`; hooks ใน `~/.zode/hooks.json` ⊕
`.zode/hooks.json`

**Cross-agent configuration.** Zode อ่าน skills และ MCP configuration โดยตรงจาก
Claude Code, Codex, Cursor, opencode, Gemini และ local agent ที่เกี่ยวข้อง plugin tree
ที่ติดตั้งไว้และ plugin cache ของผลิตภัณฑ์เหล่านั้นจะไม่ถูกสแกน หากต้องการใช้ plugin ซ้ำ
ให้ติดตั้ง source ของมันอย่างชัดเจนด้วย `zode plugin install ... --trust`; รูปแบบ package
ของ Codex และ Claude Code ยังรองรับสำหรับ plugin ที่ติดตั้งผ่าน Zode

## ตั้งค่า MCP servers

MCP servers อยู่ใน config แบบ nested-precedence เดียวกับทุกอย่าง —
`~/.zode/mcp.json` สำหรับทุกโปรเจกต์, `.mcp.json` หรือ `.zode/mcp.json` ที่ project root
เพื่อ scope ให้ repo เดียว ไม่มี registry ไม่ต้อง restart-and-pray: แก้ไฟล์แล้ว `/mcp`
(หรือเปิดใหม่) เพื่อโหลด

### stdio (spawn local server)

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

MCP tools ปรากฏเป็น `mcp__<server>__<tool>` และเรียกเป็น slash command แบบ dynamic
ได้ ดูรายละเอียด transport (stdio/HTTP/SSE) เพิ่มเติมใน
[README ภาษาอังกฤษ](../../README.md)

## Instructions, MCP และ Skills

Zode ยัง discover skills, commands และ MCP configuration จาก Claude, Codex, opencode,
Cursor และ agent อื่น ๆ Skills อยู่ใน `.zode/skills/**/SKILL.md` และปรากฏผ่าน `/skills`
พร้อมดัชนีใน system prompt; agents, skills และ MCP tools ที่สร้างขึ้นก็เป็น slash command
แบบ dynamic

## ZSeven-W Ecosystem

Zode เป็นส่วนหนึ่งของ stack เครื่องมือ AI-native development ของ ZSeven-W:

| Product | คืออะไร |
|---------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async runtime สำหรับ LLM agents พร้อม multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions และ optional coding tools |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework ที่ทำให้ `.op` file เป็น app และเชื่อม OpenPencil-style design artifacts ไปสู่ runnable software |
| [`noema`](https://github.com/ZSeven-W/noema) | Local-first, non-vector memory system สำหรับ coding agents พร้อม lexical recall, review queues, MCP, S3 offload และ enterprise policy controls |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool สำหรับ design-as-code workflows ที่แปลง prompts เป็น UI บน live canvas และรองรับ concurrent agent teams |

## Benchmark

Benchmarks ของ Zode ครอบคลุม one-shot code generation, agentic read/run/edit/fix,
multi-file tasks, tricky bugs, MCP/Skills/constraint following และ Noema LOCOMO runner
ดู methodology, คำสั่ง reproduce และตารางผลลัพธ์เต็มได้ใน
[Benchmark section ของ README ภาษาอังกฤษ](../../README.md#benchmark) ส่วน suite อยู่ใน
[`benchmarks/`](../../benchmarks/)

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

## รายงานการทดสอบ

ทุกชุดทดสอบผ่าน; เซลฟ์เทสต์แบบ end-to-end รันวงจรวิวัฒนาการจริง — ฟิตเนสของกลุ่ม
เครื่องมือ → ยีน JS ที่ถูกสร้าง → การคัดเลือกตามความจุ → การคงอยู่ของจีโนม — และพิมพ์
`SELF-TEST PASSED`:

| ชุดทดสอบ | คำสั่ง | ผลลัพธ์ |
|---|---|---|
| แกน harness, เลเยอร์วิวัฒนาการ, ปลั๊กอินแบบโปรเซส | `cargo test -p cordis-rs` | 50 passed |
| การผสานวิวัฒนาการ (ฟิตเนสกลุ่มเครื่องมือ, การกู้คืนจีโนม) | `cargo test -p zode-core --lib evolution::` | 5 passed |
| เลเยอร์ยีน QuickJS (สลับโค้ด, อินเทอร์รัปต์, จำกัดหน่วยความจำ) | `cargo test -p zode-core --test js_plugin_it` | 4 passed |
| ชุดทดสอบทั้งหมดของ zode-core (รวมการเชื่อมต่อวิวัฒนาการ) | `cargo test -p zode-core --lib` | 983 passed |

```sh
cargo run -p zode-core --example evolution_self_test
```

- ไปป์ไลน์ฮุกให้คะแนนผลลัพธ์ของเครื่องมือเทียบกับกลุ่มของมัน
  (`uses − 10·failures − 100·panics − 5·restarts`); `unfit_groups()` ระบุกลุ่มที่ควรปิด
  การใช้งาน
- พูลยีนมีขีดจำกัดความจุ: ยีนที่อ่อนแอที่สุดถูกไล่ออกเมื่อเอเจนต์สร้างผู้สมัครใหม่
  (เซลฟ์เทสต์ไล่ `git` → `todo` → `shell`); ยีนที่แข็งแกร่งที่สุดอยู่รอด
- ยีนที่ถูกสร้างคือ JavaScript — ไม่ต้องใช้คอมไพเลอร์ — พร้อมขีดจำกัดหน่วยความจำและ
  กำหนดเวลาอินเทอร์รัปต์ต่อยีน; ยีนที่ควบคุมไม่ได้ถูกกักกันแทนที่จะทำร้าย zode
- จีโนมถูกบันทึกที่ `<config-dir>/evolution/genome.json` และกู้คืนพร้อมฟิตเนสข้ามการ
  รีสตาร์ท; `dispose()` คืนทรัพยากร fiber, listener และประวัติเหตุการณ์ทั้งหมด

รายงานฉบับเต็มพร้อมเอาต์พุตที่สังเกตได้และรีเกรสชันที่แก้แล้ว:
`crates/cordis-rs/README.md`

## Contributing

ยินดีรับ contributions โปรดใช้ [Conventional Commits](https://www.conventionalcommits.org/):
`<type>(<scope>): <subject>` โดย scope ที่พบบ่อยคือ `core`, `tui`, `cli`, `tools`,
`config`, `build`, `ci`, `docs`

## License

[MIT](../../LICENSE) &copy; ZSeven-W
