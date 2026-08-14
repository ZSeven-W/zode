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
  <strong>面向終端機的開源 AI 原生程式碼助理。</strong><br/>
  能讀取程式碼、執行命令、搜尋檔案、管理 git，並透過快速的 Rust TUI 完成這些工作。
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

> 這是本地化 README，涵蓋產品概覽與快速開始。完整基準測試細節與最新長文說明以 [英文 README](../../README.md) 為準。

## 亮點

- **多供應商**：支援 Anthropic、OpenAI、OpenAI 相容 API（DeepSeek、Moonshot、OpenRouter 等）以及本機 Ollama。
- **豐富的工具面**：檔案讀寫與編輯（含原子多處編輯 `MultiEdit`）、程式碼與內容搜尋、前景/背景 shell、git、網頁擷取（搭配 Tavily key 可啟用 `WebSearch`）、notebook、TODO 追蹤。
- **瀏覽器控制**：內建 `browser_*` 工具可驅動託管的 Chromium，或透過 Chrome bridge 擴充功能控制你正在使用的 Chrome。配對只需一次——擴充功能會在 zode 重新啟動後自動重新連線。
- **非阻塞式權限**：會變更狀態的工具都會經過 allow once / always / deny 審核，審核提示以行內方式顯示，不會阻擋你繼續輸入。
- **預設開啟 OS 沙箱**：shell 命令在 macOS `sandbox-exec` 或 Linux `bwrap` 中執行，預設禁止對外網路。
- **全螢幕 TUI**：串流 Markdown、語法高亮、diff 預覽、斜線命令補全、歷史輸入、11 套內建佈景主題、設定與說明浮層，以及 15 種 UI 語言（`/language`）。
- **V1 相容的持久工作階段**：保留既有的 `<id>.jsonl` 工作階段協定，同時以旁路資料新增 journal、checkpoint、rewind、fork 與隔離的 Git worktree。上下文壓縮不會遺失可見對話——還原工作階段時會完整重播壓縮前的歷史，模型上下文仍維持壓縮後的精簡形態。
- **自動化介面**：穩定的 JSON/JSONL headless 輸出、精確工作階段定位、工具過濾、確定性結束碼、stdio ACP 與本機 dashboard。
- **多工作階段分頁**：用 `Ctrl+T` 並排執行多個隔離的工作階段，並可還原歷史工作階段。
- **子代理、團隊與工作流程**：透過 Task 委派一次性任務，手動註冊內部或外部 CLI 隊友，並用 `/agents`、`/team`、`/workflows` 管理。
- **技能、MCP 與 hooks**：按需載入 `SKILL.md`，連接 MCP 伺服器，並在工具事件上執行外部指令碼。

## 安裝

### 一行安裝

**macOS / Linux：**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell)：**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

安裝器會自動辨識 OS 與 CPU，從最新 [release](https://github.com/ZSeven-W/zode/releases) 下載相符的二進位檔，並把 `zode` 放到 `PATH`。

### 手動下載

在 [releases 頁面](https://github.com/ZSeven-W/zode/releases) 下載對應平台的封存檔：

| OS | 架構 | 資源 |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

解壓後把 `zode` 移到 `PATH` 中，例如 `sudo mv zode /usr/local/bin/`。Linux 版本以 glibc 建置；macOS 二進位檔未簽章，若 Gatekeeper 提示可執行 `xattr -dr com.apple.quarantine ./zode`。

### 從原始碼建置

需要 Rust 1.88 或更新版本：

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

二進位檔位於 `target/release/zode`。agent runtime 是 `vendor/agent` git submodule，因此請使用 `--recurse-submodules` 複製，或執行 `git submodule update --init`。

## 快速開始

最簡單的方式是啟動 `zode` 並執行 **`/connect`**。它會開啟一個互動式模型選擇器，並為你寫入設定。

也可以手動撰寫 `~/.zode/config.json`：

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

常用啟動方式：

```bash
zode                          # 全螢幕 TUI
zode -p "explain main.rs"     # headless：執行一次提示並輸出到 stdout
zode --no-tui                 # 一般的 readline REPL
zode -c                       # 繼續最近的工作階段
zode -r <id>                  # 依 id 前綴還原工作階段
zode --yolo                   # 略過審核提示（硬性拒絕規則仍生效）
zode --no-sandbox             # 關閉 OS 沙箱
zode --sandbox-read-only      # 唯讀沙箱
zode --sandbox-allow-network  # 允許沙箱內對外網路
zode --browser                # 強制啟用瀏覽器工具
zode --model <id>             # 覆寫模型
zode --provider <name>        # 選擇設定中的供應商
zode server                   # 透過 stdio 執行 JSON-RPC app-server
zode acp                      # 透過 stdio 執行 ACP agent
zode dashboard                # 檢視本機工作階段、checkpoint 與 worktree
```

## 手動註冊外部 CLI 隊友

Zode 可以把第三方 agent CLI 當作一次性 Task worker，或加入可持續對話的
team。註冊是明確的：即使可執行檔已經位於 `PATH`，Zode 也不會自動暴露給
模型；必須在 `externalAgents.agents` 中新增 profile。
也可以執行 `/external-agents` 檢視 `PATH` 中受支援的 CLI，再執行
`/external-agents discover` 將發現的預置明確註冊到全域設定。Zode 啟動時仍不會自動掃描或註冊。

| Profile | 命令 | Task | Team 模式 | 外部 CLI 沙箱 |
|---|---|---:|---:|---|
| `claude-code` | `claude` | 是 | persistent | unrestricted |
| `codex` | `codex` | 是 | persistent | workspace-write |
| `opencode` | `opencode` | 是 | stateless | unknown |
| `cline` | `cline` | 是 | stateless | unrestricted |
| `antigravity` | `agy` | 是 | stateless | unknown |
| `cursor` | `cursor-agent` | 是 | persistent | unrestricted |
| `kiro` | `kiro-cli` | 是 | stateless | unrestricted |
| `pi` | `pi` | 是 | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | 是 | persistent | unrestricted |

### 新增 profile

在全域 `~/.zode/config.json` 或專案層級的 `.zode/config.json` 中設定。已知
profile 使用空物件即可手動啟用；`command` 可寫 `PATH` 中的裸命令名或路徑：

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

只新增確實要暴露的 profile。已知預設可覆寫 `enabled`、`command`、
`extraArgs`、`envAllow` 與 `trusted`。自訂 profile 的
`promptTransport` 支援 `stdin`、`argv`、`file`；`output` 支援 `text`、
通用的 `jsonl`、`jsonl-claude`、`jsonl-codex`。通用 JSONL 透過 RFC 6901
`textSource` 與 `sessionIdSource` 擷取文字與工作階段 ID；`resumeArgs` 必須包含
獨立的 `{session_id}`。沒有可還原工作階段的 CLI 會作為無狀態 teammate，每次派發
啟動新程序，也可作為一次性 Task worker。
`newSessionArgs` 也可包含獨立的 `{session_id}`：Zode 會為首次執行產生
ID，後續派發使用 `resumeArgs`。

外部程序只繼承 `PATH`、`HOME`、`TERM` 等基礎環境；API key 等變數需加入
`envAllow` 或 `authEnv`。一般模式首次雇用會顯示命令、工作目錄與沙箱並要求
信任；Zode 只審核程序啟動，不會逐項審核外部 CLI 的檔案變更與 shell 命令。
`--yolo` 等無互動模式必須明確設定 `trusted: true` 才會執行。

### 使用 team

`team_hire` 與 `team_send` 是模型工具，不是斜線命令。直接告訴 leader：

```text
雇用 `codex`，命名為 `implementer`，負責實作認證重構並執行測試。
把任務發給 `implementer`，編輯前先 claim `src/auth/`。
```

之後用 `/team`、`/team board` 檢視名冊與協作板，用
`/team dismiss implementer` 移除隊友。team 狀態儲存在
`<cwd>/.zode/team/`，但外部 CLI 的信任授權不會跨 Zode 程序持久化。

## 新功能使用指南

### 結構化 headless

`-p`、`--prompt-file` 與 `--prompt-json` 共用同一套 headless 引擎。`json`
只輸出最終結果物件；`stream-json` 逐行輸出版本化的
`zode.run-event.v1` 事件。結構化模式會把 stdout 專用於機器可讀資料，並採用
穩定的結束碼：`0` 成功、`10` provider 錯誤、`11` 權限拒絕、`12` 輪次上限、`13` 中斷（Ctrl-C）、`14` 部分結果、`15` 工作階段定位錯誤。

```bash
zode -p "修復失敗測試" --output-format json --max-turns 12
zode -p "審查儲存庫" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"總結目前工作區"}'

# 精確 ID 不做前綴比對；fork 不會修改來源工作階段。
zode -p "繼續工作" --session-id my-session
zode -p "嘗試另一種方案" --fork-session my-session --fork-worktree
```

工具 deny 規則優先於 allow，並會傳遞給 Task 子代理。`--permission-mode`
支援 `default`、`dont-ask`、`accept-edits`、`bypass`；`--yolo` 仍是 bypass
的捷徑，但硬性拒絕規則始終生效。

### 直接擴充 Session V1

工作階段 transcript 仍然只有一份：`~/.zode/sessions/<id>.jsonl`。舊版 Zode
可以繼續讀寫它；新版也直接讀寫同一個檔案。新增資料只放在
`~/.zode/sessions/<id>/` 旁路目錄中，包括 `meta.json`、journal、checkpoint
與 snapshot，因此無需新增工作階段版本，也沒有 transcript 雙寫問題。

```bash
zode session list
zode session list --json
zode session show <id>                         # 檢視 metadata 與 checkpoint ID
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # 只預覽衝突與變更
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

系統會在有變更副作用的 turn 前建立 checkpoint。Rewind 會還原已追蹤檔案與
工作階段訊息前綴，遇到新變更時回報衝突而不是覆寫；歷史 journal 不會被刪除，
而是建立新的邏輯分支。worktree fork 的結果需要透過 `apply-back` 明確合回。

**壓縮不遺失可見對話。** 上下文壓縮把舊訊息替換為摘要時，原文會保存到旁路封存
（`~/.zode/sessions/<id>/compacted.jsonl`）。還原工作階段、`Ctrl+L` 重繪、
`/export` 與 Chrome 側欄都會顯示壓縮前的完整歷史，而模型收到的仍是壓縮後的
上下文。fork 會攜帶封存（依自身 transcript 過濾），`/clear` 會刪除封存，
刪除工作階段時整個旁路目錄一併移除。

### 權限規則與沙箱 profile

權限規則可以寫進 `config.json` 的 `permissions.rules`，也可以用
`--rules ./permissions.json` 臨時載入。欄位比對使用 RFC 6901 JSON pointer；
優先順序固定為 deny > ask > allow。
獨立的 rules 檔案只能是規則陣列或 `{ "rules": [...] }`，不要再套一層
`permissions`。

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
zode -p "只做檢查" --sandbox-profile read-only
zode -p "執行檢查" --sandbox-profile workspace
zode -p "下載相依套件" --sandbox-profile workspace-network
zode -p "執行 CI" --sandbox-profile ci --rules ./permissions.json
```

內建 profile 為 `read-only`、`workspace`、`workspace-network` 與
`unconfined`；也可以像上例一樣定義自己的 profile。

### 外掛與靜態 marketplace

受管理的外掛可包含 skills、commands、agents、hooks、MCP、LSP 與受限的 JavaScript
UI 渲染器。Zode 支援
`plugin.json`、`.zode-plugin/plugin.json`、`.codex-plugin/plugin.json`、
`.grok-plugin/plugin.json` 與 `.claude-plugin/plugin.json`。同時支援 Codex
與 Claude Code 的元件路徑陣列，並在首次安裝時遵循 Claude Code 的
`defaultEnabled`。Codex apps/connectors 以及 Claude Code themes、monitors、
output styles 等宿主專屬元件會被忽略；僅包含 app 的外掛會因為沒有 Zode 可用元件而
拒絕安裝。安裝內容會複製為帶有來源與 SHA-256 tree hash 的不可變快照；包含可執行
能力的外掛只有在明確傳入 `--trust` 後才會啟用。

#### JavaScript UI 外掛快速開始

最小的 UI 外掛只需要 manifest 與一個 JavaScript 檔案：

```text
my-plugin/
├── plugin.json
└── scripts/
    └── ui.js
```

`plugin.json`：

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

可以安裝本機目錄，也可以直接安裝 GitHub 儲存庫或儲存庫子目錄。正在執行的 Zode
需要重新啟動，才會載入新的外掛快照：

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

修改原始碼後執行 `zode plugin update my-plugin` 更新已安裝的快照。由於
JavaScript、hooks、MCP server 與宣告的網路存取都屬於可執行能力，安裝時必須明確
傳入 `--trust`。安裝與更新都會列印外掛宣告的權限（網路網域、環境變數、context
scope）。如果更新後的 manifest 申請的權限**超出**已安裝快照，更新會被拒絕，
必須重新攜帶 `--trust` 才能接受——活動的 Git 來源無法悄悄擴大自己的授權。

#### UI 渲染 API

UI 外掛可以在 sidebar 版本號正上方渲染宣告式內容——所有外掛依載入順序合計
最多 6 行。manifest 指定 JS 進入點：

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

JS 透過 `zode.ui.sidebar` 註冊同步渲染函式。`ctx` 是唯讀 JSON 快照，包含終端機、
工作階段、模型、狀態、token 與上下文視窗資訊；指令碼不會取得檔案系統、網路、終端機或
Ratatui 控制代碼，最終樣式與寬度由 Zode 控制。

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

`tone` 支援 `default`、`muted`、`accent`、`success`、`warning`、`danger`，
span 還支援 `bold` 與 `italic`。renderer 必須是同步函式。每個指令碼最大
256 KiB，每次執行最多使用 8 MiB JS 記憶體與 25 ms，且 renderer 最快每 250 ms
重新求值一次（間隔內沿用快取輸出）；sidebar 每個 renderer 最多 6 行（所有外掛
合計也是 6 行），每行最多 16 個 span、2,048 位元組文字，控制字元會由宿主清理。

狀態列也可以擴充：沒有外掛輸出時保持 1 行；同步的
`zode.ui.statusLine` renderer 傳回 spans 後，版面會動態擴充為 2 行。Zode
本身的核心狀態與安全提示固定在第一行，外掛輸出合併到第二行。

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### 渲染上下文與權限

每個 renderer 都能直接取得以下基礎欄位，無需額外申請 context 權限：

| 欄位 | 結構與含義 |
| --- | --- |
| `ctx.apiVersion` | 上下文 API 版本，目前為 `1`。 |
| `ctx.app` | `{ version, effort }`。 |
| `ctx.terminal` | `{ width, height }`，單位為終端機 cell。 |
| `ctx.session` | 目前任務的 `{ id, title, cwd, busy }`。 |
| `ctx.model` | `{ id, provider }`。 |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`；`sandbox` 為 `{ enabled, readOnly, network }`。 |
| `ctx.tokens` | `{ input, output }` token 計數。 |
| `ctx.context` | `{ used, window, usedPercent }`；無法計算時百分比為 `null`。 |
| `ctx.data` | 僅包含目前外掛自己註冊的背景資料來源結果。 |

更豐富的資訊只有在 `permissions.context` 宣告對應 scope 後才會出現：

| Scope | 暴露欄位 | 結構與限制 |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`；`active` 從 1 開始。 |
| `workspace` | `ctx.workspace.modifiedFiles` | 最多 50 筆 Git `{ path, added, removed }`。 |
| `tools` | `ctx.tools.available` | 目前任務啟用的工具名，已排序。 |
| `tools` | `ctx.tools.active` | 目前正在執行的工具名。 |
| `tools` | `ctx.tools.recent` | 最近最多 20 筆 `{ name, status, durationMs }`。 |
| `tasks` | `ctx.tasks.todoStatuses` | 只有 Todo 狀態，不包含 Todo 正文。 |
| `tasks` | `ctx.tasks.subagents` | `{ type, status }`，不包含 prompt 或對話。 |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`，不包含 Goal 正文。 |
| `services` | `ctx.services.mcp` | `{ name, connected }`。 |
| `services` | `ctx.services.lsp` | `{ language, running }`。 |

例如：

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` 是觀察介面：外掛可以知道有哪些工具、哪些工具正在執行或最近執行過，
但 UI 外掛不能直接呼叫工具。工具輸入/輸出、prompt、對話正文、Todo/Goal 正文、
環境變數值與憑證都不會暴露，也不能藉此繞過 Zode 既有的審核系統。

#### 背景 HTTP 資料

UI 外掛還可以註冊背景 HTTP 資料來源。網路網域與憑證環境變數必須在 manifest 中
明確宣告：

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

請求採用宣告式設定，在渲染路徑之外執行。Zode 只在 Rust 請求層把環境變數組裝進
header，JS 無法讀取 token：

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

`zode.data.define(key, config)` 的 key 長度為 1–64，只能包含字母、數字、底線
與連字號。`request` 支援 `url`、`method`、`headers`、可選的 JSON `body` 與
`timeoutMs`。預設使用 `GET`、3 秒逾時與 60 秒更新間隔，目前只允許 HTTPS
`GET`/`POST`。一般 header 值是字串；秘密 header 使用
`{ "env": "NAME", "prefix": "Bearer " }`。環境變數還必須列在
`permissions.env` 中，只會由 Rust 請求層在傳送時讀取，永遠不會傳回給 JS。

Zode 會停用重新導向與 proxy、驗證並固定公網 DNS、拒絕 localhost/私網、把回應限制為
256 KiB、把請求逾時限制在 500 ms 到 10 秒，並將更新間隔限制在 10 秒到 1 小時。
`*.example.com` 只比對子網域，不比對裸網域 `example.com`。

每個外掛只能看到自己的資料。`ctx.data.<key>` 的結果是
`{ ok, status, data, updatedAt }`，請求失敗時為
`{ ok: false, error, updatedAt }`。JSON 回應會成為物件或陣列，非 JSON 回應會
成為字串；HTTP 錯誤狀態仍會提供 `status` 與 `data`，同時 `ok` 為 `false`。

呼叫私有配額或 Coding Plan API 時，需要在啟動 Zode 前提供環境變數：

```bash
CODING_PLAN_TOKEN=... zode
```

[完整的可執行範例](../../examples/plugins/zode-ui-demo/)會在 sidebar 與狀態列顯示
模型、上下文與工具活動，並透過 `zode.data.define` 讀取公開的 GitHub API 配額。

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# marketplace 是本機目錄或 Git 靜態索引，不依賴 Zode 雲端服務。
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # 同名時指定來源
zode plugin marketplace update
```

### ACP、dashboard、OTLP 與 PTY 測試

`zode acp` 透過 stdio 實作 ACP initialize/new/load/fork/prompt/cancel，向用戶端
串流傳送訊息、思考與工具事件，透過用戶端請求權限，並支援用戶端提供的 stdio、
HTTP、SSE MCP server。它與 TUI/headless 共用同一套 V1 相容的工作階段儲存。

```bash
zode acp
zode dashboard
zode dashboard --json
```

OTLP 預設關閉，必須明確設定 `ZODE_OTEL=1`。匯出的只有不含內容的生命週期、
工具名、狀態與 token usage 屬性；不會匯出 prompt、模型文字、工具輸入/輸出、
檔案路徑或錯誤訊息。

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "執行測試" --output-format json
```

儲存庫還提供真實 PTY + VT100 虛擬螢幕測試工具，可記錄 raw diagnostics 與螢幕快照：

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` 會依序驅動真實終端機的等待、按鍵、resize 與 snapshot；按鍵寫法
支援 `<Enter>`、`<Esc>`、`<Tab>`、方向鍵、`<Backspace>`、`<C-c>`、`<C-d>`
與 `<C-l>`：

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

本地開源版本明確不包含 xAI 專用帳號/計費，也不建置由 Zode 營運的雲端 marketplace
服務。

## 設定要點

`providers` 是模型供應商的來源；頂層 `provider` 指向目前活動的模型。OpenAI 相容供應商通常需要 `baseUrl` 與 `dialect`：

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
  "language": "zh-CN"
}
```

一個 provider 可以包含多個模型，並可透過 `/model` 即時切換。`language` 也可透過 `/language` 修改。

## Server 模式與 SDK

`zode server` 會在 stdin/stdout 上啟動以換行分隔的 JSON-RPC server，適用於編輯器整合、本機自動化、測試與 SDK 用戶端。

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

這裡的「不支援」只限定 app-server 協定本身：它暫不提供託管 marketplace 管理、
遠端控制、Realtime、背景終端機、thread archive/fork、goals 與 app connector。
上文的本機 Session V1 命令與靜態外掛 marketplace 是獨立的 CLI 能力，不受此限制。

SDK 位於 [`sdk/`](../../sdk/)：

| SDK | 目錄 | 本地測試 |
|-----|------|----------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

方法、參數、傳回值與 SDK enum/constant 名稱記錄在 [`sdk/` 方法參考](../../sdk/README.md#method-reference) 中。

## 瀏覽器控制

Zode 提供 `tools:browser` 工具組：

- `browser_read`：截圖、DOM 快照、console/network 日誌、分頁讀取。
- `browser_act`：導覽、點擊、輸入、按鍵、捲動。
- `browser_eval`：執行 JavaScript。
- `browser_tabs`：管理分頁。

可選目標：

- **managed**：Zode 啟動並控制專用的 Chromium profile。
- **bridge**：透過 [`extensions/chrome/`](../../extensions/chrome/) 中的 MV3 擴充功能控制你正在使用的 Chrome profile。

首次升級後執行一次新版 Zode 並執行 `/browser pair`。這會註冊僅允許固定擴充功能 ID
呼叫的 Chrome Native Messaging host。**配對只需一次**：擴充功能儲存長期 token 後
會自動重新連線——瀏覽器啟動、擴充功能更新、以及斷線後約每 30 秒靜默重試，zode
重新啟動不再需要重新配對。Chrome 會攔截外部程式開啟的 `chrome-extension://` 網址
（ERR_BLOCKED_BY_CLIENT，macOS/Windows/Linux 皆然），因此配對頁由**擴充功能自己**
在約 30 秒內自動彈出（連接埠已預填，輸入聊天中顯示的 6 位配對碼即可）；也可以
手動把該網址**輸入到網址列**開啟（手動輸入的導覽不受攔截）。
之後即使沒有開啟 Zode CLI，側欄也會自動
啟動一個無終端機的本機 Zode daemon，並使用已儲存的 token 還原任務與歷史記錄。
從側欄提交任務時，瀏覽器工具會綁定側欄旁邊的目前頁面，因此「分析目前頁面」會
直接讀取現有分頁，不再新建分頁；獨立的 TUI/CLI 自動化仍使用 `zode` 分頁群組。
側欄中的「這個」「目前內容」等模糊表達也預設指向目前頁面，Agent 會先讀取頁面；
只有使用者明確詢問專案、程式碼或本機檔案時，才會優先檢查本機工作區。

常用命令：

```bash
/browser
/browser status
/browser launch
/browser close
/browser pair
/browser target managed
/browser target bridge        # 切換到擴充功能橋接，並儲存為下次啟動的預設目標
/browser screenshot [path]
```

擴充功能載入、更新、CRX 打包與 smoke test 步驟見 [`extensions/chrome/README.md`](../../extensions/chrome/README.md)。

## 桌面控制

Zode 還能透過作業系統的無障礙(accessibility)API 驅動原生桌面應用程式,不侷限於瀏覽器:

- `desktop_read`:讀取無障礙樹(視窗、元素及其 ref)。
- `desktop_act`:依元素點擊、輸入、捲動、設值。
- `desktop_screenshot`:擷取螢幕。

唯讀讀取不需核可;有副作用的桌面操作走與其他工具相同的 允許一次/一律允許/拒絕 核可流程。

各平台後端:

- **macOS** — Accessibility(AX)API。
- **Windows** — UI Automation(UIA)。
- **Linux** — AT-SPI。
- **Electron 應用程式** — 透過 Chrome DevTools Protocol 附加。

**假游標與 Esc 急停。** Zode 從不移動你真正的滑鼠。macOS 上一個零權限的覆蓋層
(`zode-overlay`)會畫出一個*假*游標,沿平滑的 Dubins 路徑飛向每個操作目標,方便你
跟隨 Agent 的動作(輸入的文字不會顯示在覆蓋層中)。桌面自動化進行時,全域 **Esc**
會中斷所有執行中的回合並隱藏覆蓋層(與 TUI 的 Esc 同一條急停路徑)。其他平台照常
執行桌面操作,只是沒有視覺化。

沒有 US 鍵盤配置對應鍵碼的字元(CJK、部分標點)會透過系統剪貼簿投遞(寫入 →
合成貼上 → 還原原剪貼簿),讓自訂按鍵處理的應用程式也能收到真正的字元。

```bash
/desktop          # 顯示桌面目標與權限狀態
/desktop status   # 同上
```

設定位於 `~/.zode/config.json` 的 `desktop.*`:

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor`(預設 `true`)繪製 macOS 覆蓋層游標;`escCancel`(預設 `true`)在自動化
期間啟用全域 Esc 中斷;`overlayHelperPath`(預設 `null`)覆寫 `zode-overlay` 輔助程式
路徑——輔助程式缺失時僅關閉視覺化。桌面自動化首次使用可能會請求系統權限(如 macOS
輔助使用)。

## 常用斜線命令

| 命令 | 作用 |
|---|---|
| `/help` | 命令與快捷鍵說明 |
| `/connect` | 連接並切換目前的 provider |
| `/model [id]` | 檢視或設定目前的模型 |
| `/theme [id]` | 切換佈景主題（`catppuccin-mocha`、`aurora-forge`、`ember-atelier`、`sakura-paper`、`arctic-day`、`lavender-mist`、`citrus-grove`、`verdant-signal`、`cyberpunk`、`minimal`、`hacker`） |
| `/config` | 檢視模型與工作目錄 |
| `/sessions`, `/resume` | 還原歷史工作階段 |
| `/browser ...` | 瀏覽器控制面板與命令 |
| `/tasks` | 背景 shell 與執行中的 turn |
| `/mcp` | 管理 MCP server |
| `/skills` | 列出可用技能 |
| `/agents` | 管理子代理 |
| `/external-agents [list\|discover]` | 檢視 `PATH` 中受支援的外部 CLI，或明確註冊所有發現的預置 |
| `/team [status\|board\|dismiss <name>]` | 檢視持久隊友名冊與共享 board，或移除隊友 |
| `/workflows` | 管理與執行 JS 工作流程 |
| `/sandbox ...` | 檢視或控制 OS 沙箱 |
| `/language` | 切換 UI 語言 |
| `/export [path]` | 匯出工作階段為 Markdown |
| `/exit` | 結束 |

完整命令表見 [英文 README](../../README.md#slash-commands)。

## 專案指令、MCP 與技能

Zode 會依層級讀取指令：全域 `~/.zode/`、專案根目錄、目前工作目錄；每一層優先使用 `AGENTS.md`，再回退到 `CLAUDE.md`。技能位於 `.zode/skills/**/SKILL.md`，MCP server 位於 `~/.zode/mcp.json`、`.mcp.json` 或 `.zode/mcp.json`，hooks 位於 `~/.zode/hooks.json` 或 `.zode/hooks.json`。

Zode 會讀取 Claude Code、Codex、opencode、Cursor、Gemini 等其他 agent 的直接
skills 目錄與 MCP 設定，但不會掃描這些產品安裝的 plugin tree 或 plugin cache。
需要沿用外掛時，請透過 `zode plugin install ... --trust` 明確安裝；透過 Zode
安裝時仍相容 Codex 與 Claude Code 的外掛包格式。

## ZSeven-W 生態系

Zode 屬於 ZSeven-W 面向 AI 原生開發工具的一組產品：

| 產品 | 定位 |
|------|------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | 純 Rust 非同步 LLM agent runtime，提供多供應商串流輸出、工具調度、權限、MCP、成本追蹤、附件、工作階段與可選的編碼工具。 |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust 原生跨平台 UI 框架，讓 `.op` 檔案成為應用程式，把 OpenPencil 風格的設計產物連接到可執行軟體。 |
| [`noema`](https://github.com/ZSeven-W/noema) | 面向編碼 agent 的 local-first、非向量記憶系統，包含詞法召回、review queue、MCP、S3 offload 與企業策略控制。 |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | 開源 AI 原生向量設計工具，面向 design-as-code 工作流程，可在即時畫布上把 prompt 轉成 UI，並支援並行的 agent teams。 |

## 基準測試

Zode 的 benchmark 涵蓋 one-shot 程式碼生成、agentic 讀/跑/改/修、多檔案任務、疑難 bug、MCP/Skills/約束遵循，以及 Noema LOCOMO runner。完整方法、重現命令與結果表見 [英文 README 的 Benchmark 部分](../../README.md#benchmark)，套件位於 [`benchmarks/`](../../benchmarks/)。

## 開發

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## 測試報告

全部套件通過；端到端自測跑通真實進化閉環——工具組適應度 → 生成的 JS 基因 →
容量淘汰 → 基因組持久化——並輸出 `SELF-TEST PASSED`：

| 套件 | 命令 | 结果 |
|---|---|---|
| Harness 核心、进化层、进程插件 | `cargo test -p cordis-rs` | 50 passed |
| 进化集成（工具组适应度、基因组恢复） | `cargo test -p zode-core --lib evolution::` | 5 passed |
| QuickJS 基因层（源码热替换、中断、内存上限） | `cargo test -p zode-core --test js_plugin_it` | 4 passed |
| zode-core 全量套件（含进化接线） | `cargo test -p zode-core --lib` | 983 passed |

```sh
cargo run -p zode-core --example evolution_self_test
```

- Hook 管線把每次工具結果按工具組計分（`uses − 10·failures − 100·panics − 5·restarts`）；
  `unfit_groups()` 列出值得停用的組。
- 基因池有硬性容量上限：agent 進化出新候選時，最弱的基因被淘汰（自測中依序淘汰
  `git` → `todo` → `shell`）；最適者存活。
- 生成基因是 JavaScript——無需編譯器——每個基因有記憶體上限與中斷時限；失控基因
  被隔離而不會傷害 zode。
- 基因組持久化到 `<config-dir>/evolution/genome.json`，重啟後帶著適應度恢復；
  `dispose()` 回收全部 fiber、監聽器與事件歷史。

完整報告（含實際輸出與被釘住的回歸）見 `crates/cordis-rs/README.md`。

## 貢獻

歡迎貢獻。請遵循 [Conventional Commits](https://www.conventionalcommits.org/)：`<type>(<scope>): <subject>`，常見 scope 包括 `core`、`tui`、`cli`、`tools`、`config`、`build`、`ci`、`docs`。

## 授權條款

[MIT](../../LICENSE) &copy; ZSeven-W
