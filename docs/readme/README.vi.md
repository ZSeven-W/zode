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
  <strong>Trợ lý coding open-source, AI-native cho terminal.</strong><br/>
  Đọc code, chạy command, tìm file và quản lý git — tất cả trong một Rust TUI nhanh.
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

> README bản địa hóa này bao gồm phần tổng quan và quick start. [README tiếng Anh](../../README.md) vẫn là nguồn chuẩn cho chi tiết benchmark đầy đủ và các ghi chú dài mới nhất.

## Điểm nổi bật

- **Multi-provider**: Anthropic, OpenAI và mọi API tương thích OpenAI (các dialect DeepSeek, Moonshot, OpenRouter), cùng Ollama local. Hỗ trợ model output lớn và **context 1M** (`contextWindow` / `maxOutputTokens` đều cấu hình được).
- **Bộ công cụ rộng**: đọc/ghi/sửa file (gồm `MultiEdit` sửa nhiều đoạn nguyên tử), tìm kiếm code và content, foreground/background shell, git, web fetch (cộng `WebSearch` tùy chọn với Tavily key), notebooks và TODO tracking.
- **Browser control**: công cụ `browser_*` tích hợp có thể điều khiển một managed Chromium hoặc Chrome profile thật của bạn qua Chrome bridge extension — navigate, click/type, kiểm tra DOM, chụp screenshot, đọc console/network logs và gom nhóm các tab do zode mở. Pairing chỉ cần một lần — extension tự reconnect qua các lần khởi động lại zode.
- **Permission không block**: mọi mutating tool đều đi qua cổng phê duyệt (allow once / always / deny), nhưng prompt hiển thị inline và không chặn bạn — cứ tiếp tục gõ để xếp hàng lệnh kế tiếp trong khi một tool đang chờ, kèm cả hard-deny rules.
- **OS sandbox, bật mặc định**: shell commands chạy dưới sandbox-exec (macOS) / bwrap (Linux) ở chế độ `read-only` hoặc `workspace-write`, với **outbound network bị từ chối theo mặc định**. Bật/tắt live bằng `/sandbox`; model có thể xin thoát sandbox cho một lệnh (`dangerouslyDisableSandbox`) mà **bạn tự phê duyệt** ở prompt.
- **TUI toàn màn hình**: streaming Markdown kèm syntax highlighting, diff preview, slash-command autocomplete, prompt history (Up/Down), 11 chủ đề tích hợp, settings/help overlays, sidebar phải với các mục co giãn ổn định và **UI 15 ngôn ngữ** (`/language`).
- **Session bền, tương thích V1**: giữ nguyên contract transcript `<id>.jsonl` hiện có, đồng thời bổ sung journals, checkpoints, rewind, fork và Git worktree tách biệt dưới dạng dữ liệu sidecar. Context compaction không bao giờ làm mất hội thoại nhìn thấy — resume replay đầy đủ lịch sử trước compaction trong khi context của model vẫn giữ dạng compact.
- **Bề mặt automation**: JSON/JSONL headless output ổn định, nhắm session chính xác, tool filters, exit code xác định, ACP qua stdio và một dashboard vận hành cục bộ.
- **Multi-session tabs**: chạy nhiều conversation song song (`Ctrl+T`), mỗi tab là một agent tách biệt; resume session cũ với replay đầy đủ lịch sử.
- **Sub-agents, team và workflows**: delegate việc một lần qua Task tool, thuê teammate nội bộ hoặc external-CLI bền, điều phối chúng bằng board chung và file claims, quản lý qua `/agents`, `/team` và `/workflows`.
- **Cấu hình cục bộ dễ di chuyển**: đọc trực tiếp cấu hình skills và MCP của Claude Code, Codex, Cursor, opencode và Gemini, nhưng không bao giờ import cây plugin đã cài hay cache của chúng.
- **Skills & MCP**: nạp package hướng dẫn `SKILL.md` theo yêu cầu và kết nối MCP servers (`mcp__<server>__<tool>`); agents, skills và MCP tools đã tạo đều xuất hiện dưới dạng slash command.
- **Hooks**: chạy external scripts trên các tool event (ví dụ chặn command nguy hiểm, lint sau khi edit).
- **Instructions ba cấp**: global (`~/.zode/`) → project root → cwd (`AGENTS.md` / `CLAUDE.md`).

## Cài đặt

### Một dòng (binary dựng sẵn)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer tự phát hiện OS + CPU, tải binary phù hợp từ [release](https://github.com/ZSeven-W/zode/releases) mới nhất và đặt `zode` vào `PATH`. Ghim một phiên bản hoặc đổi vị trí cài đặt:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

### Tải thủ công

Tải archive cho platform của bạn từ [trang releases](https://github.com/ZSeven-W/zode/releases):

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Giải nén rồi di chuyển `zode` vào `PATH` (`sudo mv zode /usr/local/bin/`). Linux builds dùng glibc; macOS binaries chưa ký (chạy `xattr -dr com.apple.quarantine ./zode` nếu Gatekeeper phàn nàn).

### Build từ source

Cần Rust 1.88 trở lên:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binary tại target/release/zode
```

> Agent runtime nằm trong git submodule `vendor/agent` — luôn clone bằng
> `--recurse-submodules` (hoặc chạy `git submodule update --init`).

## Quick Start

Cách dễ nhất là chạy `zode` rồi dùng **`/connect`** — một picker tương tác dựa trên models.dev, tự ghi config cho bạn.

Để tự viết `~/.zode/config.json`: **`providers`** là source of truth — mỗi provider một entry (credential dùng chung) chứa một hoặc nhiều **models** — còn **`provider`** cấp cao nhất ghi lại model đang *active*:

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // wire protocol: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // model đang active
}
```

Provider tương thích OpenAI (DeepSeek, Moonshot, OpenRouter, …) thêm `baseUrl` + `dialect`, và thiết lập theo từng model nằm trong entry của model đó:

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

Một provider entry có thể chứa nhiều model — đổi live giữa chúng bằng `/model`.

Sau đó chạy:

```bash
zode                       # TUI toàn màn hình
zode -p "explain main.rs"  # headless: một prompt, stream ra stdout, thoát
zode --no-tui              # readline REPL đơn giản
zode -c                    # tiếp tục session gần nhất
zode -r <id>               # resume session theo id prefix
zode --yolo                # bỏ qua approval prompt (deny rules vẫn áp dụng)
zode --no-sandbox          # tắt OS sandbox (mặc định BẬT)
zode --sandbox-read-only   # sandbox chế độ read-only (từ chối mọi ghi)
zode --sandbox-allow-network  # cho phép outbound network trong sandbox
zode --browser             # ép bật browser tools tích hợp cho lần chạy này
zode --no-browser          # tắt browser tools tích hợp cho lần chạy này
zode --model <id>          # override model
zode --provider <name>     # chọn provider có tên trong config.providers
zode server                # chế độ JSON-RPC app-server qua stdio
zode acp                   # agent Agent Client Protocol qua stdio
zode dashboard             # tổng quan sessions/checkpoints/worktrees cục bộ
```

Bạn cũng có thể trỏ tới bất kỳ provider nào mà không cần sửa config bằng cách export key tương ứng (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); với Ollama, `baseUrl` được lấy từ environment khi không được đặt.

## Đăng ký thủ công teammate CLI bên ngoài

Zode có thể dùng một agent CLI bên thứ ba đã cài làm Task worker một lần, hoặc làm teammate bền hay stateless. Việc đăng ký cố ý là thủ công: cài một CLI hoặc để nó trong `PATH` **không** làm nó lộ ra cho model. Thêm một profile vào `externalAgents.agents`, rồi khởi động Zode trong project. Hoặc chạy `/external-agents` để kiểm tra các CLI được hỗ trợ hiện có trong `PATH`, rồi `/external-agents discover` để thêm rõ ràng mọi preset đã phát hiện vào config global. Lệnh này do người dùng kích hoạt; lúc khởi động Zode không bao giờ tự quét hay đăng ký external CLI.

| Agent profile | Executable | Task worker | Team mode | Sandbox của CLI ngoài |
|---|---|---:|---:|---|
| `claude-code` | `claude` | có | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | có | persistent | workspace-write |
| `opencode` | `opencode` | có | stateless | unknown |
| `cline` | `cline` | có | stateless | unrestricted |
| `antigravity` | `agy` | có | stateless | unknown |
| `cursor` | `cursor-agent` | có | persistent | unrestricted |
| `kiro` | `kiro-cli` | có | stateless | unrestricted |
| `pi` | `pi` | có | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | có | persistent | unrestricted |

Mọi profile đã đăng ký đều có thể tham gia team. Profile hỗ trợ resume giữ lại session ID và conversation của CLI qua các lần giao việc; các CLI khác là teammate stateless, khởi động process mới cho mỗi lần giao việc. Các preset dùng giao diện headless được tài liệu hóa của [Cline](https://docs.cline.bot/usage/cli-overview), [Antigravity](https://antigravity.google/docs/cli-best-practices), [Cursor](https://cursor.com/docs/cli/headless), [Kiro](https://kiro.dev/docs/cli/headless/), [Pi](https://pi.dev/docs/latest) và [Grok Build](https://docs.x.ai/build/cli/headless-scripting) của xAI. Các công cụ khác, kể cả các Grok CLI thay thế, có thể dùng custom profile.

### Thêm profile CLI thủ công

Đặt `externalAgents` vào `~/.zode/config.json` cho mọi project, hoặc vào `<project>/.zode/config.json` cho một project. Một object rỗng bật rõ ràng một preset đã biết và resolve executable của nó trên `PATH` đã được làm sạch:

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

Chỉ thêm những profile bạn thực sự muốn cung cấp. Một `command` trần như `cline` được resolve trên `PATH`; các path như `./tools/my-agent` hay `/opt/agents/my-agent` cũng được chấp nhận. Preset đã biết tôn trọng `enabled`, `command`, `extraArgs`, `envAllow` và `trusted`; `extraArgs` được nối vào lệnh gọi preset của Zode.

Process CLI khởi động với environment đã xóa sạch, chỉ còn `PATH`, `HOME` và `TERM` (cộng các biến Windows bắt buộc), nên hãy thêm rõ ràng API key hoặc biến cần thiết khác vào `envAllow`. Trạng thái đăng nhập hiện có dưới `HOME` vẫn hoạt động. Một entry project trùng tên profile sẽ thay thế toàn bộ entry global, nên hãy lặp lại mọi override mà project vẫn cần.

Một custom profile khai báo đầy đủ lệnh gọi và protocol:

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

`promptTransport` là `stdin`, `argv` hoặc `file`; `argv` cần một argument `{prompt}` độc lập còn `file` cần `{prompt_file}`. `output` là `text`, `jsonl` generic, `jsonl-claude` hoặc `jsonl-codex`. Profile JSONL generic dùng pointer RFC 6901 `textSource` và `sessionIdSource` để trích text stream và một session ID có thể resume từ bất kỳ event nào. `resumeArgs` phải chứa token `{session_id}` độc lập và được nối vào ở các turn sau; `resumeFlag` được giữ lại như dạng viết tắt `<flag> <session-id>`.

Nếu một CLI chấp nhận session ID do caller chọn, `newSessionArgs` có thể chứa một token `{session_id}` độc lập. Zode sinh một UUID, nối các argument đã mở rộng ở lần chạy đầu, rồi dùng `resumeArgs` ở các lần giao việc sau. Điều này cũng khiến một CLI plain-text có thể resume mà không cần parse ID từ output của nó.

Nhờ vậy bất kỳ headless CLI nào cũng trở thành Task worker hoặc teammate stateless. Để giữ context conversation giữa các lần giao việc trong team, nó còn phải lộ một session ID, hoặc nhận một session ID qua `newSessionArgs`, cùng một lệnh gọi resume phi tương tác.

`effectiveSandbox` nhận `none`, `readOnly`, `workspaceWrite`, `unrestricted` hoặc `unknown` và được hiển thị trong trust prompt.

### Thuê và làm việc với teammate

Hãy yêu cầu leader bằng ngôn ngữ tự nhiên; `team_hire` và `team_send` là tool hướng model, không phải slash command:

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

Lần hire đầu tiên hiển thị executable và argument đã resolve, working directory và sandbox hiệu lực của CLI. Phê duyệt nó sẽ giao việc cho process đó trong project hiện tại: Zode gate việc khởi động process, nhưng **không** gate từng file edit hay shell command mà CLI ngoài thực hiện. Trust grant tồn tại suốt session Zode hiện tại; roster bền được khôi phục từ `<cwd>/.zode/team/`, nhưng một teammate ngoài phải được trust lại sau khi khởi động lại hoặc executable đổi.

Trong các lần chạy phi tương tác/bypass (kể cả `--yolo`), Zode không thể hiển thị trust prompt và fail closed. Chỉ đặt `externalAgents.agents.<profile>.trusted` thành `true` khi bạn cố ý muốn profile đó chạy không cần prompt.

Dùng `/team` để kiểm tra roster và board sau khi hire:

```text
/team                         # panel roster + board
/team status                  # roster dạng text
/team board                   # goal chung, ghi chú, assignment và claims
/team dismiss implementer     # xóa teammate
```

## Automation, session bền và vận hành

### Headless có cấu trúc

`-p`, `--prompt-file` và `--prompt-json` đều dùng cùng một headless engine. `json` phát ra một object kết quả cuối; `stream-json` phát ra mỗi dòng một object JSON `zode.run-event.v1`. Các chế độ có cấu trúc dành riêng stdout cho output máy đọc được và dùng exit code ổn định: `0` thành công, `10` lỗi provider, `11` từ chối quyền, `12` đạt giới hạn turn/limit, `13` bị ngắt (Ctrl-C), `14` kết quả một phần, `15` lỗi nhắm session.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# ID chính xác không khớp theo prefix. Một fork không bao giờ sửa session nguồn.
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Tool deny pattern thắng allow pattern và được kế thừa bởi Task sub-agent. `--permission-mode` nhận `default`, `dont-ask`, `accept-edits` và `bypass`; `--yolo` vẫn là lối tắt cho bypass, còn hard deny rules vẫn áp dụng.

### Session tương thích V1, checkpoints và worktrees

Transcript vẫn là file V1 gốc tại `~/.zode/sessions/<id>.jsonl`. Đây là bản transcript **duy nhất**, nên các client Zode cũ vẫn đọc/ghi được. Metadata mới là bổ sung và nằm trong `~/.zode/sessions/<id>/` (`meta.json`, journal, checkpoints và snapshots). Không cần format session mới hay migrate transcript.

```bash
zode session list
zode session list --json
zode session show <id>                         # metadata + checkpoint IDs
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # preview có nhận biết conflict
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

Một checkpoint được chụp trước một turn có mutate. Rewind khôi phục nội dung file được theo dõi và prefix transcript, báo cáo conflict thay vì ghi đè thay đổi mới hơn, và ghi một nhánh journal logic mới thay vì xóa lịch sử. Worktree fork có thể được apply-back rõ ràng khi thử nghiệm sẵn sàng.

**Compaction không bao giờ làm mất phần hội thoại nhìn thấy.** Khi context compaction thay các message cũ bằng một bản tóm tắt, bản gốc được giữ trong một sidecar bổ sung (`~/.zode/sessions/<id>/compacted.jsonl`). Resume một session, nhấn `Ctrl+L`, `/export` và Chrome side panel đều hiển thị đầy đủ lịch sử trước compaction, trong khi model vẫn chỉ nhận context đã compact. Fork mang theo archive (được lọc theo transcript của chính nó), `/clear` xóa nó, và xóa session sẽ xóa toàn bộ sidecar.

### Permission rules và sandbox profiles

Rules có thể nằm dưới `permissions.rules` trong `config.json`, hoặc trong một file JSON độc lập truyền qua `--rules`. Một field matcher dùng JSON pointer RFC 6901; deny ưu tiên hơn ask, ask ưu tiên hơn allow. File độc lập phải là một mảng rule hoặc `{ "rules": [...] }`; nó không được bọc trong một object `permissions` cấp cao nhất.

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

Các profile tích hợp là `read-only`, `workspace`, `workspace-network` và `unconfined`. Profile do config định nghĩa dùng cùng các field sandbox nêu trên. Trên Windows, sandbox áp dụng theo tier tùy khả năng của nền tảng; nếu sandbox đã cấu hình không thể xác minh, khởi động sẽ fail closed và bạn phải dùng `--no-sandbox` để chạy không có nó.

### Plugins và static marketplaces

Một managed plugin có thể đóng góp skills, commands, agents, hooks, MCP servers, LSP servers và các JavaScript UI renderer chạy trong sandbox. Zode chấp nhận `plugin.json`, `.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.grok-plugin/plugin.json` và `.claude-plugin/plugin.json`. Mảng path component của Codex và Claude Code được hỗ trợ, và `defaultEnabled` của Claude Code được tôn trọng ở lần cài đầu. Các component chỉ dành cho host như Codex apps/connectors và themes, monitors hay output styles của Claude Code bị bỏ qua; một plugin chỉ có app sẽ bị từ chối vì không có component tương thích Zode. Bản cài là snapshot bất biến kèm provenance và SHA-256 tree hash. Nội dung plugin có khả năng thực thi không bao giờ được kích hoạt nếu thiếu cờ `--trust` rõ ràng.

#### Quick start cho JavaScript UI plugin

UI plugin nhỏ nhất gồm một manifest và một file JavaScript:

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

Cài một thư mục cục bộ hoặc một repository/subdirectory GitHub, rồi khởi động lại process Zode đang chạy để nó nạp snapshot mới:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Dùng `zode plugin update my-plugin` sau khi đổi source. `--trust` là bắt buộc vì JavaScript, hooks, MCP servers và network access đã khai báo đều là năng lực thực thi. Install và update in ra grant quyền plugin đã khai báo (network hosts, env vars, context scopes). Một update mà manifest xin quyền *rộng hơn* snapshot đã cài sẽ bị từ chối trừ khi bạn chạy lại với `--trust` — một Git source đang di chuyển không thể âm thầm mở rộng grant của chính nó.

#### UI render API

UI plugin có thể đóng góp các dòng khai báo ngay phía trên phần version của sidebar — tối đa tổng cộng sáu dòng, chia sẻ giữa mọi plugin theo thứ tự nạp. Khai báo một entrypoint JavaScript trong manifest:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

Đăng ký một renderer đồng bộ với `zode.ui.sidebar`. Context là một snapshot JSON chỉ đọc chứa các field terminal, session, model, status, token và context-window. Kết quả được Zode render; script không nhận filesystem, network, terminal hay Ratatui bridge.

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

Các tone được hỗ trợ là `default`, `muted`, `accent`, `success`, `warning` và `danger`; span còn nhận `bold` và `italic`. Renderer phải đồng bộ. Mỗi script giới hạn 256 KiB, 8 MiB JS memory và 25 ms mỗi lần đánh giá, và renderer được đánh giá lại nhiều nhất mỗi 250 ms (output cache được tái dùng giữa các lần). Output sidebar giới hạn 6 dòng mỗi renderer (6 dòng tổng cộng qua các plugin), mỗi dòng 16 span và 2.048 byte text. Control character được host làm sạch.

Status bar cũng mở rộng được. Nó giữ một dòng khi không plugin nào trả nội dung và giãn thành hai dòng động khi một renderer đồng bộ `zode.ui.statusLine` trả về spans. Zode giữ status lõi và chỉ báo an toàn ở dòng đầu; output plugin được ghép vào dòng thứ hai.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Render context và permissions

Mỗi renderer nhận các field cơ bản sau mà không cần xin thêm quyền context:

| Field | Cấu trúc và ý nghĩa |
| --- | --- |
| `ctx.apiVersion` | Version Context API; hiện là `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | `{ width, height }` theo ô terminal. |
| `ctx.session` | `{ id, title, cwd, busy }` của task đang active. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` chứa `{ enabled, readOnly, network }`. |
| `ctx.tokens` | Bộ đếm token `{ input, output }`. |
| `ctx.context` | `{ used, window, usedPercent }`; phần trăm có thể `null`. |
| `ctx.data` | Kết quả chỉ thuộc các data source do chính plugin này đăng ký. |

Các mục phong phú hơn bị bỏ trừ khi plugin xin scope tương ứng trong `permissions.context`:

| Scope | Field lộ ra | Cấu trúc và giới hạn |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` tính từ 1. |
| `workspace` | `ctx.workspace.modifiedFiles` | Tối đa 50 entry Git `{ path, added, removed }`. |
| `tools` | `ctx.tools.available` | Tên các tool được bật cho task active, đã sắp xếp. |
| `tools` | `ctx.tools.active` | Tên các tool đang chạy. |
| `tools` | `ctx.tools.recent` | Tối đa 20 bản ghi `{ name, status, durationMs }`. |
| `tasks` | `ctx.tasks.todoStatuses` | Chỉ chuỗi trạng thái todo, không có nội dung todo. |
| `tasks` | `ctx.tasks.subagents` | Bản ghi `{ type, status }`, không có prompt hay transcript. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, không có nội dung goal. |
| `services` | `ctx.services.mcp` | Bản ghi `{ name, connected }`. |
| `services` | `ctx.services.lsp` | Bản ghi `{ language, running }`. |

Ví dụ:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` là một API quan sát: nó cho renderer biết những tool nào tồn tại và tool nào đang hoặc đã chạy. UI plugin không thể gọi một tool. Input/output của tool, prompt, nội dung transcript, nội dung todo/goal, giá trị environment và credential đều không được đưa vào, và API không thể vượt qua hệ thống phê duyệt của Zode.

#### Dữ liệu HTTP nền

UI plugin cũng có thể đăng ký các data source HTTP chạy nền. Network và secret access phải được khai báo trong manifest:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

Request là khai báo và chạy ngoài render path. Biến environment bí mật được Zode lắp vào header và không bao giờ lộ cho JavaScript:

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

`zode.data.define(key, config)` nhận key dài 1–64 ký tự gồm chữ, số, gạch dưới hoặc gạch nối. `request` hỗ trợ `url`, `method`, `headers`, `body` JSON tùy chọn và `timeoutMs`. Mặc định là `GET`, timeout 3 giây và refresh 60 giây. Chỉ chấp nhận HTTPS `GET` và `POST`. Header dạng literal là chuỗi; header bí mật dùng `{ "env": "NAME", "prefix": "Bearer " }`. Biến environment còn phải xuất hiện trong `permissions.env`, chỉ được Rust đọc khi dựng request và không bao giờ trả về cho JavaScript.

Zode tắt redirect và proxy, xác thực và pin địa chỉ DNS công khai, từ chối localhost/mạng riêng, giới hạn response ở 256 KiB, kẹp request timeout trong 500 ms–10 giây, và kẹp refresh interval trong 10 giây–1 giờ. Một wildcard như `*.example.com` khớp subdomain nhưng không khớp host trần `example.com`.

Mỗi plugin chỉ thấy dữ liệu của chính nó. `ctx.data.<key>` chứa `{ ok, status, data, updatedAt }` hoặc `{ ok: false, error, updatedAt }`. Response JSON trở thành object/array; response không phải JSON trở thành chuỗi. Một HTTP error status vẫn kèm `status` và `data`, với `ok: false`.

Khởi động Zode với secret cần thiết trong environment khi dùng một API quota hoặc coding-plan riêng tư:

```bash
CODING_PLAN_TOKEN=... zode
```

[Ví dụ chạy được đầy đủ](../../examples/plugins/zode-ui-demo/) hiển thị hoạt động model/context/tool trong sidebar và status line, và dùng `zode.data.define` cho một quota GitHub API công khai.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# Một marketplace là một index tĩnh local/Git, không phải dịch vụ do Zode host.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # phân định khi cần
zode plugin marketplace update
```

### ACP, dashboard, telemetry và TUI regression tests

`zode acp` triển khai ACP initialize/new/load/fork/prompt/cancel qua stdio, stream các update message/thought/tool, yêu cầu quyền qua client và chấp nhận MCP server stdio, HTTP và SSE do client cấp. Dữ liệu session dùng cùng store tương thích V1 như TUI và headless CLI.

```bash
zode acp
zode dashboard
zode dashboard --json
```

Xuất OTLP tắt theo mặc định và cần opt-in rõ ràng. Nó chỉ xuất các attribute lifecycle/tool-name/status/usage không chứa nội dung: prompt, text sinh ra, input/output của tool, đường dẫn file và error message không bao giờ được gửi.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

Cho các kịch bản regression TUI trên terminal thật, workspace có sẵn một harness PTY + VT100 ghi lại raw diagnostics và snapshot màn hình ảo:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` điều khiển terminal thật với các wait có thứ tự, key input, resize và snapshot (ký hiệu phím hỗ trợ `<Enter>`, `<Esc>`, `<Tab>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>` và `<C-l>`):

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

Bản triển khai local/open này cố ý không bao gồm tài khoản, billing riêng của xAI hay một dịch vụ cloud marketplace do Zode vận hành.

Các key config cấp cao tùy chọn (đều có mặc định hợp lý):

```jsonc
{
  "maxOutputTokens": 16384,      // giới hạn output mỗi turn (nâng lên để ghi file lớn)
  "contextWindow": 1000000,      // context window của model — đặt 1000000 cho model 1M
  "temperature": 0,              // càng thấp càng xác định
  "language": "vi",              // ngôn ngữ UI (15 locale); cũng đổi qua /language
  "effort": "medium",            // reasoning effort; trên Anthropic, medium/high map sang thinking budget thật
  "autonomousOrchestration": true, // điều phối sub-agent + workflow (mặc định bật)
  "subagentMaxIterations": 0,      // guard con tùy chọn; bỏ/0 = không giới hạn
  "tools": {
    "deferNonCore": false        // true: giữ ~20 tool thường dùng hiển thị, defer phần còn lại sau ToolSearch
  },
  "webSearch": {
    "tavilyApiKey": null         // bật tool WebSearch (hoặc đặt $TAVILY_API_KEY)
  },
  "sandbox": {
    "enabled": true,             // OS sandbox cho shell commands (mặc định bật)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // cho phép outbound network trong sandbox
    "writableRoots": []          // các thư mục ghi được thêm (workspace-write)
  },
  "browser": {
    "enabled": true,             // tools browser_* và panel /browser (mặc định bật)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // chế độ launch managed Chromium
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // giám sát các turn /loop và /schedule không người trông
    "inactivityTimeoutSecs": 900, // abort sau 15 phút không có hoạt động provider/tool
    "maxRuntimeSecs": 3600,      // trần tuyệt đối một giờ cho mỗi turn nền
    "abortGraceSecs": 10,        // chờ hủy hợp tác trước khi hard-stop
    "maxRetries": 3,             // số lần recovery liên tiếp trước khi cạn
    "initialBackoffSecs": 5,     // độ trễ retry đầu tiên
    "maxBackoffSecs": 300        // trần cho exponential retry backoff
  }
}
```

> Sandbox giam giữ shell commands (macOS: sandbox-exec; Linux: `bwrap`, phải được
> cài). Khởi động fail closed nếu sandbox đã cấu hình không xác minh được; dùng
> cờ `--no-sandbox` rõ ràng để chạy không có nó. Network bị từ chối theo mặc
> định. Nếu một lệnh thực sự cần thoát, model đặt `dangerouslyDisableSandbox:
> true` và **bạn** phê duyệt tại approval prompt — hoặc bật/tắt cả sandbox live
> bằng `/sandbox`.

> `contextWindow` điều khiển auto-compaction — hãy đặt đúng window thật của model
> (ví dụ `1000000`). Ưu tiên giá trị **theo model** dưới
> `providers.<name>.models.<id>.contextWindow` (nó được ưu tiên); key cấp cao
> phía trên là fallback global, và zode cũng điền nó từ catalog models.dev đóng
> gói sẵn khi không có giá trị nào được đặt. **Đừng** đặt cao hơn window thật:
> ước tính quá mức làm request tràn và provider từ chối turn.

## Server mode và SDK

`zode server` khởi động một JSON-RPC server phân tách bằng newline trên stdin/stdout. Nó dành cho tích hợp editor, automation cục bộ, tests và SDK client muốn dùng năng lực sẵn có của zode mà không cần mở TUI.

```bash
zode server                      # stdio (mặc định) — thứ các SDK spawn
zode server --listen stdio://    # tương tự, viết đầy đủ
zode server --listen ws://127.0.0.1:0   # WebSocket loopback + Bearer auth
zode server --listen off         # không khởi động gì và thoát
```

Server mode lộ ra hành vi do zode hậu thuẫn:

- khởi tạo + khám phá capability (với `approvalPolicy` là `readOnly` (mặc định) / `auto` / `prompt`)
- lifecycle metadata của thread và **streaming turns** — output model và tool call đến dưới dạng JSON-RPC notification; `turn/interrupt` hủy một turn
- **approval tương tác** — policy `prompt` điều khiển các frame server→client `approval/request` được trả lời bằng `allow` / `allowAlways` / `deny`
- đọc/ghi/tạo/stat/list/remove/copy filesystem và `command/exec` một lần
- list/set model, đọc/list/ghi config, và danh sách read-only skills, hooks, trạng thái MCP-server và plugin

Transport WebSocket chỉ bind loopback và ghi một file credential `0600` `<config-dir>/server.json` (`{port, pid, token}`); client xác thực bằng `Authorization: Bearer <token>`. Xem [`sdk/README.md`](../../sdk/README.md) để biết protocol đầy đủ, tên field notification và ví dụ theo từng ngôn ngữ.

Riêng với protocol app-server này, quản lý marketplace host, remote-control, Realtime, spawn process độc lập, background terminal, thread archive/fork, goals và app connector vẫn nằm ngoài phạm vi. Các lệnh session cục bộ và static-plugin marketplace nêu trên là các bề mặt CLI riêng.

SDK nằm dưới [`sdk/`](../../sdk/):

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

Mỗi SDK lộ ra một tập enum/constant `ProtocolMethod` native cho các tên method ổn định hiện tại, nên các tích hợp tránh được chuỗi JSON-RPC hard-code. Params, hình dạng result và tên enum/constant SDK của mọi method được hỗ trợ đều được tài liệu hóa trong [tài liệu method của `sdk/`](../../sdk/README.md#method-reference).

## Browser control

Zode có sẵn group `tools:browser` cho browser automation. Agent có thể dùng `browser_read` để chụp screenshot, DOM snapshot, console logs, network logs và đọc tab; `browser_act` để navigate, click, gõ phím, nhấn key và scroll; `browser_eval` để chạy JavaScript; và `browser_tabs` để quản lý tab. Việc kiểm tra browser chỉ đọc không cần gate; hành động browser có mutate dùng cùng luồng phê duyệt allow-once / always / deny như các tool có side-effect khác.

Có hai browser target:

- **managed** — zode launch và điều khiển một Chromium profile riêng.
- **bridge** — zode điều khiển chính Chrome profile bạn đang dùng thông qua extension MV3 đóng gói sẵn trong [`extensions/chrome/`](../../extensions/chrome/).

Với target bridge, hãy nạp extension một lần từ `extensions/chrome`, rồi chạy `/browser pair`. Chrome chặn các URL `chrome-extension://` do chương trình bên ngoài mở (ERR_BLOCKED_BY_CLIENT — trên macOS, Windows lẫn Linux đều vậy), nên nỗ lực tự mở trang của zode có thể thất bại — thay vào đó, chính extension sẽ tự mở trang pairing của nó trong vòng ~30 giây sau `/browser pair`, với port đã điền sẵn; hãy nhập pairing code 6 chữ số hiển thị trong chat. Cách dự phòng thủ công: tự gõ URL `chrome-extension://…/popup.html?port=…` vào thanh địa chỉ (điều hướng gõ tay được coi là do browser khởi phát nên không bị chặn). **Pairing chỉ cần một lần**: extension lưu một token dài hạn và tự reconnect — khi browser khởi động, khi extension cập nhật, và retry khoảng mỗi 30 giây trong lúc mất kết nối — nên khởi động lại zode không bao giờ yêu cầu pair lại. Nó tự reconnect vào một CLI đang chạy hoặc tự khởi động một zode daemon chỉ-extension khi cần. Các tab do zode mở được đặt vào một Chrome tab group tên `zode`.

### Chrome task side panel

Chạy zode CLI đã cập nhật và `/browser pair` một lần. Bấm biểu tượng trên toolbar sẽ mở side panel; sau đó nó tự khởi động zode khi không có process CLI nào đang chạy. Trang pair vẫn là một luồng code/token nhỏ, và các task vẫn được chia sẻ với các session TUI mà không đổi focus terminal.

Turn từ side panel bind browser tools bridge vào trang đang hiển thị cạnh panel, nên các yêu cầu như “analyze this page” dùng `browser_read` trên tab hiện có thay vì mở tab mới. Browser automation TUI và CLI độc lập vẫn tiếp tục dùng các tab do zode sở hữu trong tab group `zode`. Trang đang active cũng là context mặc định cho các prompt side-panel mơ hồ; file project cục bộ chỉ được kiểm tra khi người dùng hỏi rõ về chúng.

Panel có thể gửi text, chọn model, chọn các access mode `readOnly`, `prompt` và `auto`, stream response và Stop một turn đang chạy. Một turn có thể đính kèm tối đa 8 file và tổng 20 MiB: ảnh PNG, JPEG, GIF và WebP tối đa 5 MiB mỗi ảnh, cộng file text UTF-8 và code tối đa 1 MiB mỗi file. Input PDF, Office, archive, executable và không phải UTF-8 bị từ chối.

Sau khi cập nhật extension, bấm Reload trên `chrome://extensions`. Các phiên bản extension cũ vẫn tương thích với browser automation nhưng không có task side panel. Trên Windows, zode xác định và launch Chrome trực tiếp cho các extension URL thay vì gọi shell default-browser, tránh redirect sang Microsoft Store khi Chrome đã được cài.

Các lệnh hữu ích:

```bash
/browser                         # mở panel điều khiển browser
/browser status                  # hiển thị trạng thái target/running/paired
/browser launch                  # launch managed browser
/browser close                   # đóng managed browser
/browser pair                    # pair hoặc reconnect Chrome bridge extension
/browser target managed          # dùng managed Chromium của zode
/browser target bridge           # dùng extension và lưu làm default cho lần launch sau
/browser screenshot [path]       # chụp screenshot browser
```

Xem [`extensions/chrome/README.md`](../../extensions/chrome/README.md) để biết cách nạp extension, cập nhật, đóng gói CRX và các bước smoke-test.

## Desktop control

Zode có thể điều khiển ứng dụng desktop native qua các OS accessibility API, không chỉ browser. Agent dùng `desktop_read` để đọc accessibility tree (windows, elements và ref của chúng), `desktop_act` để click, gõ, scroll và set value theo element, và `desktop_screenshot` để chụp màn hình. Việc đọc chỉ-đọc không cần gate; hành động desktop có mutate dùng cùng luồng phê duyệt allow-once / always / deny như các tool có side-effect khác.

Backend được chọn theo từng platform:

- **macOS** — Accessibility (AX) API.
- **Windows** — UI Automation (UIA).
- **Linux** — AT-SPI.
- **Ứng dụng Electron** — attach qua Chrome DevTools Protocol.

**Ghost cursor và Esc stop.** Zode không bao giờ di chuyển chuột thật của bạn. Trên macOS, một overlay không cần quyền (`zode-overlay`) vẽ một con trỏ *giả* bay theo đường Dubins mượt tới mục tiêu của mỗi hành động, để bạn theo dõi những gì agent đang làm; text đã gõ không bao giờ hiển thị trong overlay. Trong khi desktop automation đang hoạt động, một phím **Esc** toàn cục ngắt mọi turn đang chạy và ẩn overlay (cùng đường stop như Esc của TUI). Các platform khác chạy hành động desktop mà không có phần visualize.

CJK và các text khác không có keycode theo bố cục US được gửi qua system pasteboard (ghi → tổng hợp paste → khôi phục clipboard trước đó) để các ứng dụng có xử lý phím tùy chỉnh nhận đúng ký tự thật.

```bash
/desktop            # hiển thị desktop target và trạng thái quyền
/desktop status     # tương tự, rõ ràng
```

Config nằm dưới `desktop.*` trong `~/.zode/config.json`:

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor` (mặc định `true`) vẽ con trỏ overlay macOS; `escCancel` (mặc định `true`) trang bị phím Esc-toàn-cục ngắt trong lúc automation; `overlayHelperPath` (mặc định `null`) override vị trí helper `zode-overlay` — helper thiếu chỉ đơn giản tắt phần visualize. Desktop automation có thể yêu cầu quyền OS (ví dụ macOS Accessibility) ở lần dùng đầu.

## Background Turn Watchdog, /loop và /schedule

Các turn `/loop` và `/schedule` do scheduler sở hữu chạy dưới một liveness watchdog in-process. Hoạt động provider, tool và nested-agent làm mới một heartbeat phía nguồn dùng chung, còn `maxRuntimeSecs` là trần tuyệt đối. Khi hết một trong hai timeout, zode yêu cầu hủy hợp tác, chờ `abortGraceSecs`, rồi hard-stop task turn cục bộ nếu nó vẫn chưa drain. Dừng task chưa đủ để nhả slot scheduler: zode còn chờ mọi provider, tool, hook, subprocess reader và nested-agent worker được theo dõi lặng đi. Nếu boundary thứ hai đó không đạt trong năm giây, tab/store bị cách ly, job bị disable, và live-attempt lease của nó được giữ đến khi worker thực sự thoát.

Các lần thử thất bại dùng exponential backoff có giới hạn từ `initialBackoffSecs` đến `maxBackoffSecs`. Một turn thành công xóa bộ đếm thất bại liên tiếp; khi `maxRetries` cạn, zode dừng loop hoặc disable schedule đã lưu. Ngắt thủ công, xóa job và disable rõ ràng sẽ hủy recovery đang chờ thay vì tạo retry mới khi chưa có mutation nào bắt đầu. Recovery cố ý thận trọng quanh side-effect: zode chỉ retry tự động khi chưa quan sát thấy side-effect; nếu một mutation có thể đã xảy ra, kể cả hủy thủ công giữa lúc mutate, nó dừng/disable job và chờ người xem xét. Các tool cố ý detach công việc (`BashRun` hoặc một GUI đã detach) cũng dừng lặp lại sau turn đó. Cùng giới hạn inactivity giới hạn cả hàng đợi claim-to-start: nếu một tab bận hoặc preflight turn khiến một occurrence được sở hữu không thể start, nó trở thành một watchdog failure không side-effect bình thường và vào cùng chính sách retry có giới hạn thay vì giữ cross-process lease mãi mãi.

Quiescence là một bảo đảm cục bộ. Công việc đã được một MCP server, browser extension, desktop actor hay hệ thống ngoài khác chấp nhận có thể không hỗ trợ revoke. Nếu một call như vậy bị ngắt, zode đánh dấu kết quả là chưa giải quyết, disable job scheduler, và yêu cầu bạn xác minh trạng thái ngoài trước khi bật lại.

Dùng `/watchdog status` cho cấu hình và sức khỏe theo từng turn/retry. Cùng trạng thái đó xuất hiện trong `/tasks` bên cạnh background shells và running turns; tuổi queue đã claim và các fence terminal-persistence cũng được hiển thị ở đó.

Đây là watchdog cho các turn scheduler bên trong process zode hiện tại. Nó không phải OS process supervisor và không thể khởi động lại zode sau một crash hay khởi động lại máy; dùng service manager của nền tảng khi cần restart cấp process. Các schedule đã lưu ghi một active-attempt token được backing bởi một OS file lock theo từng schedule. Lúc khởi động, một lock đang tranh chấp được để yên vì một process zode khác vẫn sở hữu nó; một lock rảnh với token đã lưu chính xác là orphan từ một lần thoát bẩn, nên zode disable schedule đó dưới dạng execution-state-unknown thay vì replay âm thầm. Contract recovery này bao phủ process crash. Nó không tuyên bố độ bền cấp storage khi mất điện đột ngột hay hỏng phần cứng, và không thay thế một OS service manager.

Fire timestamp và active-attempt token được claim nguyên tử trước khi một prompt đã lưu vào tab queue, nên công việc đã queue là độc quyền qua các process zode. Cùng lease đó di chuyển theo prompt vào turn và được giữ suốt quá trình persist transcript/index cuối. Sửa, xóa hoặc disable một occurrence đã queue là hủy rõ ràng và chỉ xóa active token khớp của nó. Thoát ứng dụng nhẹ nhàng thì khôi phục chính xác fire watermark chưa start hoặc retry token, nên nó không thể tiêu thụ công việc chưa từng chạy. Một lần ghi roster terminal thất bại giữ lease trong một finalizer đang retry; token xung đột bị disable bền để xem xét trước khi nhả. Turn scheduler bỏ qua trích xuất bộ nhớ sau turn đã detach, và thoát nhẹ nhàng drain quiescence worker cộng persist terminal trước khi hủy tab của chúng. Pha lặp lại là chuẩn: interval slot dùng số học epoch tuyệt đối từ anchor đã lưu (kể cả qua DST fallback), calendar schedule giữ pha wall-clock, và backlog bị lỡ gộp về slot đến hạn muộn nhất. Một process đang chạy cũng làm mới roster để disable/remove từ xa, retry và đổi quyền sở hữu orphan có hiệu lực mà không cần restart.

### `/loop`, `/schedule` và task timing

```bash
/loop <interval> [--max N] <prompt>   # chạy prompt lặp trong tab hiện tại; list / stop [id]
/schedule add <when> <prompt>         # lưu prompt theo lịch; list / rm <id> / enable|disable <id>
/watchdog [status]                    # cấu hình watchdog turn nền, sức khỏe và retry đang chờ
/tasks                                # background shells, running turns và panel sức khỏe watchdog
```

`/loop` chạy các turn lặp lại chỉ trong session trên tab hiện tại (interval tối thiểu 30s); một prompt đến hạn được queue qua cùng đường queued input nên không bao giờ ngắt một turn đang chạy. `/schedule` được persist vào `~/.zode/schedules.json` (tmp+rename nguyên tử); các trigger bị lỡ trong lúc zode không chạy sẽ được bỏ qua, không replay. Thời lượng được hiển thị inline: hậu tố `· 1.2s` theo từng tool, footer turn `✓ done · 34s · 3 tools`, và elapsed đọc-được trong `/tasks`.

## Slash commands

| Command | Tác dụng |
|---|---|
| `/help` | Overlay commands + keybindings |
| `/clear` | Xóa conversation (và context) |
| `/model [id]` | Hiển thị / ghi nhận model active |
| `/config` | Hiển thị model + working directory |
| `/compact` | Trạng thái auto-compaction context |
| `/cost` | Token usage & chi phí đến hiện tại (gồm cả sub-agent) |
| `/theme [id]` | Đổi theme (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Session picker — resume vào tab mới kèm lịch sử |
| `/connect` | Connect và switch provider active |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Hiện/ẩn sidebar phải; gập các mục MCP / modified-files / todo |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Panel và lệnh điều khiển browser; pair Chrome bridge extension hoặc chuyển giữa managed Chromium và Chrome profile của bạn |
| `/desktop [status]` | Hiển thị desktop target và trạng thái quyền |
| `/loop <interval> [--max N] <prompt>` | Chạy prompt lặp trong tab hiện tại; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Persist một prompt theo lịch; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Hiển thị cấu hình, sức khỏe và retry đang chờ của watchdog turn nền |
| `/tasks` | Panel background shells, running turns và sức khỏe watchdog |
| `/undo`, `/redo` | Undo / redo file edit gần nhất |
| `/mcp` | Quản lý MCP servers — enable / disable trong một dialog |
| `/skills` | Liệt kê skills khả dụng |
| `/agents` | Quản lý sub-agents — tạo (AI-assisted hoặc thủ công) / xóa |
| `/external-agents [list\|discover]` | Liệt kê external CLI được hỗ trợ trong `PATH`, hoặc đăng ký rõ ràng mọi preset đã phát hiện |
| `/team [status\|board\|dismiss <name>]` | Kiểm tra roster teammate bền và board chung, hoặc xóa một teammate |
| `/workflows` | Quản lý & chạy workflow scripted bằng JS (điều phối `agent()`/`parallel()`/`pipeline()`, thực thi xác định bởi zode) |
| `/effort` | Chọn mức reasoning effort |
| `/thinking`, `/tool-details` | Bật/tắt hiển thị reasoning / chi tiết tool-call |
| `/orchestration` | Bật/tắt điều phối sub-agent + workflow tự động |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | Hiển thị / điều khiển OS sandbox lúc chạy |
| `/language` | Đổi ngôn ngữ UI (15 locale) |
| `/export [path]` | Export transcript ra Markdown (một thư mục sẽ nhận tên mặc định) |
| `/yolo` | Chế độ bypass-approval |
| `/exit` | Thoát |

Agents và skills đã tạo, cùng các MCP tool đã kết nối, cũng xuất hiện dưới dạng slash command động (ví dụ `/<name>`) và có thể được gọi trực tiếp.

## Keybindings

> Trên macOS các chord ứng dụng bên dưới dùng **`Cmd`** (⌘); trên Windows/Linux chúng dùng `Ctrl`. `Ctrl+C/D/L/V` giữ nguyên `Ctrl` ở mọi nơi (quy ước terminal).

| Phím | Hành động |
|---|---|
| `Enter` | Gửi message (queue nếu một turn đang chạy) |
| `Shift`/`Alt`+`Enter` | Xuống dòng |
| `Up` / `Down` | Gọi lại prompt đã gửi trước/sau (hoặc di chuyển lựa chọn autocomplete) |
| `Ctrl+C` | Ngắt turn (thoát khi idle) |
| `Ctrl+D` | Thoát |
| `Ctrl+L` | Vẽ lại conversation từ store (khôi phục view bị trống; dùng `/clear` để bỏ) |
| `Ctrl+V` | Paste (text hoặc đường dẫn ảnh) |
| `Cmd/Ctrl+O` | Settings |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | Tab mới / đóng tab |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Nhảy tới / xoay vòng tab |
| `Cmd/Ctrl+B` | Panel background tasks |
| `Cmd/Ctrl+G` | Bật/tắt sidebar |
| `F1` | Help |
| `PgUp` / `PgDn` | Cuộn conversation |
| `Home` / `End` | Nhảy về đầu / mới nhất của conversation |
| `Esc` | Đóng overlay hiện tại (hoặc ngắt một turn đang chạy) |

## Project instructions

Zode đọc instructions theo phân cấp ba cấp (cấp sau thắng attention): global `~/.zode/AGENTS.md` (hoặc `instructions.md`) → project root → cwd. Trong mỗi thư mục, nó ưu tiên `AGENTS.md` hơn `CLAUDE.md`. Skills nằm dưới `.zode/skills/**/SKILL.md`; MCP servers trong `~/.zode/mcp.json` ⊕ `.mcp.json`; hooks trong `~/.zode/hooks.json` ⊕ `.zode/hooks.json`.

**Cấu hình cross-agent.** Zode đọc trực tiếp cấu hình skills và MCP của Claude Code, Codex, Cursor, opencode, Gemini và các agent cục bộ liên quan. Cây plugin đã cài và cache plugin thuộc các sản phẩm đó không bao giờ được quét. Để tái dùng một plugin, hãy cài source của nó rõ ràng bằng `zode plugin install ... --trust`; format package của Codex và Claude Code vẫn được hỗ trợ cho plugin cài qua Zode.

## Cấu hình MCP servers

MCP servers nằm trong cùng config nested-precedence như mọi thứ khác — `~/.zode/mcp.json` cho mọi project, `.mcp.json` hoặc `.zode/mcp.json` ở project root để giới hạn một server cho một repo. Không registry, không restart-and-pray: sửa file, rồi `/mcp` (hoặc relaunch) để nạp nó.

### stdio (spawn một server cục bộ)

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

`command`/`args` spawn server như một subprocess pipe qua stdio. Giá trị `env` hỗ trợ thay thế `$NAME` / `${NAME}` với environment process của chính zode (mở rộng ngay trước khi connect, không ghi ra disk) — tiện để giữ token khỏi chính file config.

### Streamable HTTP (server từ xa)

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

`"transport": "http"` kết nối bằng transport Streamable HTTP của MCP spec hiện tại — một `url` duy nhất, không cần cấu hình endpoint SSE riêng. `"sse"` được chấp nhận như cách viết tương đương; cả hai resolve về cùng connector. `headers` được forward nguyên văn (kể cả `Authorization`, nên Bearer/Basic/custom đều dùng được) và hỗ trợ cùng thay thế `$VAR` như `env`. Thêm `"enabled": false` vào bất kỳ server nào để giữ định nghĩa mà không connect — `/mcp` cũng bật/tắt điều này theo từng server mà không cần sửa file bằng tay.

Mọi tool mà một server đã kết nối lộ ra đều xuất hiện dưới dạng `mcp__<server>__<tool>`, gọi được bởi agent như bất kỳ built-in tool nào (và `@`-mention được trong ô input). Zode cũng đọc trực tiếp cấu hình MCP của Claude Code, Codex, Cursor, opencode và Gemini; định nghĩa MCP ngoài trong project được khám phá ở trạng thái disabled và có thể bật qua `/mcp`. `openpencil` là tên dành riêng và bị bỏ qua.

## Cài đặt Skills & Command Markdown

Cả hai đều là Markdown thuần trên disk — không registry, không bước build. Thả một file vào là nó có hiệu lực ở lần launch tiếp theo (hoặc `/skills` để kiểm tra thứ đã nạp).

### Cài một skill

Một skill là một thư mục có `SKILL.md` bên trong. Đặt nó dưới project (`.zode/skills/`) hoặc home dir (`~/.zode/skills/`):

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

Skill giờ xuất hiện trong `/skills`, agent có thể tự gọi nó qua Skill tool, và nó cũng trở thành một slash command động — gõ `/code-review look at src/lib.rs` mở rộng thành một prompt chạy skill. Các file phụ cạnh `SKILL.md` (references, scripts) đi kèm skill. Thư mục skills trực tiếp của Claude Code, Codex, opencode, Cursor và các agent liên quan được quét; skill nằm sâu trong cây plugin hay cache của các sản phẩm đó thì không.

### Cài một command (prompt Markdown)

Một custom slash command là một file `.md` duy nhất mà **filename là tên command** và body là prompt nó submit. Bất kỳ thứ gì bạn gõ sau command sẽ được nối vào body:

```bash
mkdir -p .zode/commands            # hoặc ~/.zode/commands cho mọi project
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

Giờ `/changelog` submit prompt đó, và `/changelog only the sidebar work` nối argument của bạn phía sau. Command trong `~/.claude/commands` và `~/.codex/commands` (cùng các tương đương cấp project) cũng được nạp; command bên trong một *cây plugin ngoài* mặc định tắt — copy file `.md` vào một thư mục `.zode/commands/` để opt in.

## Hệ sinh thái ZSeven-W

Zode là một phần trong stack rộng hơn của ZSeven-W cho công cụ phát triển AI-native:

| Sản phẩm | Vai trò |
|----------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async runtime cho LLM agents: multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions và optional coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework, nơi một file `.op` là app, kết nối OpenPencil-style design artifacts với runnable software. |
| [`noema`](https://github.com/ZSeven-W/noema) | Local-first, non-vector memory system cho coding agents, với lexical recall, review queues, MCP access, S3 offload và enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool cho design-as-code workflows, biến prompt thành UI trực tiếp trên live canvas với concurrent agent teams. |

## Benchmark

Zode benchmarks bao gồm one-shot code generation, agentic read/run/edit/fix, multi-file tasks, tricky bugs, MCP/Skills/constraint following và Noema LOCOMO runner. Methodology, lệnh reproduce và bảng kết quả đầy đủ nằm trong [phần Benchmark của README tiếng Anh](../../README.md#benchmark); các suite nằm trong [`benchmarks/`](../../benchmarks/).

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

Hoan nghênh contributions. Vui lòng theo [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`, với các scope thường gặp như `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
