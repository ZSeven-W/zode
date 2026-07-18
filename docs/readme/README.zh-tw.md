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
  讀取程式碼、執行命令、搜尋檔案、管理 git，全部透過快速的 Rust TUI 完成。
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

- **多提供商**：支援 Anthropic、OpenAI、OpenAI 相容 API（DeepSeek、Moonshot、OpenRouter 等）與本機 Ollama。
- **豐富工具面**：檔案讀寫與編輯、程式碼和內容搜尋、前台/背景 shell、git、web fetch、notebook、TODO 追蹤。
- **瀏覽器控制**：內建 `browser_*` 工具可驅動受管 Chromium，或透過 Chrome bridge 擴充功能控制你正在使用的 Chrome。
- **非阻塞權限**：有副作用的工具都會經過 allow once / always / deny 審批，提示內嵌顯示，不會阻止你繼續輸入。
- **預設啟用 OS 沙箱**：shell 命令在 macOS `sandbox-exec` 或 Linux `bwrap` 中執行，預設禁止對外網路。
- **全螢幕 TUI**：串流 Markdown、語法高亮、diff 預覽、斜線命令補全、歷史提示、11 套內建主題、設定與說明浮層，以及 15 種 UI 語言（`/language`）。
- **多會話分頁**：用 `Ctrl+T` 並排執行多個隔離對話，也可恢復歷史會話。
- **子代理、團隊與工作流**：透過 Task 委派一次性工作，手動註冊內部或外部 CLI 隊友，並用 `/agents`、`/team`、`/workflows` 管理。
- **Skills、MCP 與 hooks**：按需載入 `SKILL.md`，連接 MCP server，並在工具事件上執行外部腳本。

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

安裝器會自動偵測 OS 和 CPU，從最新 [release](https://github.com/ZSeven-W/zode/releases) 下載匹配的二進位檔，並把 `zode` 放到 `PATH`。

### 手動下載

請從 [releases 頁面](https://github.com/ZSeven-W/zode/releases) 下載平台對應的封存檔：

| OS | 架構 | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

解壓後將 `zode` 移到 `PATH` 中，例如 `sudo mv zode /usr/local/bin/`。Linux build 使用 glibc；macOS 二進位檔未簽章，如果 Gatekeeper 提示可執行 `xattr -dr com.apple.quarantine ./zode`。

### 從原始碼建置

需要近期穩定版 Rust toolchain：

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

二進位檔位於 `target/release/zode`。agent runtime 是 `vendor/agent` git submodule，請使用 `--recurse-submodules` clone，或執行 `git submodule update --init`。

## 快速開始

啟動 `zode` 後執行 **`/connect`** 是最簡單的方式。它會開啟互動式模型選擇器並寫入設定。

你也可以手動寫 `~/.zode/config.json`：

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

## 手動註冊外部 CLI 隊友

Zode 可將第三方 agent CLI 作為一次性 Task worker，或加入可持續對話的
team。註冊是明確操作：即使執行檔已在 `PATH`，Zode 也不會自動暴露給模型；
必須在 `externalAgents.agents` 中加入 profile。
也可執行 `/external-agents` 查看 `PATH` 中支援的 CLI，再執行
`/external-agents discover` 將偵測到的預設明確註冊到全域設定。Zode 啟動時仍不會自動掃描或註冊。

| Profile | 命令 | Task | Team 模式 | 外部 CLI sandbox |
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

### 加入 profile

在全域 `~/.zode/config.json` 或專案 `.zode/config.json` 設定。已知
profile 使用空物件即可手動啟用；`command` 可使用 `PATH` 中的命令名稱或路徑：

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

只加入確實要暴露的 profile。已知 preset 可覆寫 `enabled`、`command`、
`extraArgs`、`envAllow`、`trusted`。自訂 profile 的 `promptTransport`
支援 `stdin`、`argv`、`file`；`output` 支援 `text`、通用 `jsonl`、
`jsonl-claude`、`jsonl-codex`。通用 JSONL 透過 RFC 6901 `textSource` 與
`sessionIdSource` 擷取文字和 session ID；`resumeArgs` 必須含有獨立的
`{session_id}`。無法恢復 session 的 CLI 會以無狀態 teammate 運作，每次派發
啟動新程序，也可作為一次性 Task worker。
`newSessionArgs` 也可包含獨立的 `{session_id}`：Zode 會為首次執行產生
ID，之後的派發則使用 `resumeArgs`。

外部程序只繼承 `PATH`、`HOME`、`TERM` 等基本環境；API key 需加入
`envAllow` 或 `authEnv`。首次雇用會顯示命令、工作目錄和 sandbox 並要求
信任；Zode 只審批程序啟動，不會逐項審批外部 CLI 的檔案修改與 shell 命令。
`--yolo` 等非互動模式必須明確設定 `trusted: true`。

### 使用 team

`team_hire` 與 `team_send` 是模型工具，不是斜線命令。直接告訴 leader：

```text
雇用 `codex`，命名為 `implementer`，負責實作驗證重構並執行測試。
將工作交給 `implementer`，編輯前先 claim `src/auth/`。
```

之後以 `/team`、`/team board` 查看名冊與協作板，以
`/team dismiss implementer` 移除隊友。team 狀態保存在
`<cwd>/.zode/team/`，但外部 CLI 的信任授權不會跨 Zode 程序持久化。

## 設定重點

`providers` 是模型提供商的來源；頂層 `provider` 指向目前使用中的模型。OpenAI 相容提供商通常需要 `baseUrl` 和 `dialect`：

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
  "language": "zh-TW"
}
```

一個 provider 可包含多個模型，並可透過 `/model` 即時切換。`language` 也可透過 `/language` 修改。

## Server 模式與 SDK

`zode server` 會在 stdin/stdout 上啟動 newline-delimited JSON-RPC server，適用於編輯器整合、本機自動化、測試與 SDK client。

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDK 位於 [`sdk/`](../../sdk/)：

| SDK | 目錄 | 本機測試 |
|-----|------|----------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## 瀏覽器控制

Zode 提供 `tools:browser` 工具組，支援讀取截圖/DOM/log、導航、點擊、輸入、執行 JavaScript 與管理分頁。可使用受管 Chromium，或透過 [`extensions/chrome/`](../../extensions/chrome/) 的 MV3 擴充功能控制現有 Chrome。

常用命令：

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

## 常用斜線命令

| 命令 | 作用 |
|---|---|
| `/help` | 命令與快捷鍵說明 |
| `/connect` | 連接並切換目前 provider |
| `/model [id]` | 查看或設定目前模型 |
| `/theme [id]` | 切換主題（`catppuccin-mocha`、`aurora-forge`、`ember-atelier`、`sakura-paper`、`arctic-day`、`lavender-mist`、`citrus-grove`、`verdant-signal`、`cyberpunk`、`minimal`、`hacker`） |
| `/sessions`, `/resume` | 恢復歷史會話 |
| `/browser ...` | 瀏覽器控制 |
| `/tasks` | 背景工作 |
| `/mcp` | 管理 MCP server |
| `/skills` | 列出技能 |
| `/agents` | 管理子代理 |
| `/external-agents [list\|discover]` | 查看 `PATH` 中支援的外部 CLI，或明確註冊所有偵測到的預設 |
| `/team [status\|board\|dismiss <name>]` | 查看持久隊友名冊和共享 board，或移除隊友 |
| `/workflows` | 管理工作流 |
| `/sandbox ...` | 控制 OS 沙箱 |
| `/language` | 切換 UI 語言 |
| `/export [path]` | 匯出 Markdown |
| `/exit` | 離開 |

完整命令表見 [英文 README](../../README.md#slash-commands)。

## 專案指令、MCP 與 Skills

Zode 會依序讀取全域 `~/.zode/`、專案根目錄、目前工作目錄中的指令；每層優先使用 `AGENTS.md`，再使用 `CLAUDE.md`。Skills 位於 `.zode/skills/**/SKILL.md`，MCP server 位於 `~/.zode/mcp.json`、`.mcp.json` 或 `.zode/mcp.json`。

Zode 也會發現 Claude、Codex、opencode、Cursor 等其他 agent 的技能、命令與 MCP 設定。專案內發現的外部 MCP 預設停用，需要你明確啟用。

## ZSeven-W 生態

Zode 是 ZSeven-W AI 原生開發工具產品線的一部分：

| 產品 | 定位 |
|------|------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | 純 Rust 非同步 LLM agent runtime，提供多提供商串流、工具調度、權限、MCP、成本追蹤、附件、會話與可選 coding tools。 |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust 原生跨平台 UI framework，讓 `.op` 檔案成為 app，並把 OpenPencil 風格設計產物連到可執行軟體。 |
| [`noema`](https://github.com/ZSeven-W/noema) | 面向 coding agents 的 local-first、非向量記憶系統，包含 lexical recall、review queues、MCP、S3 offload 與 enterprise policy controls。 |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | 開源 AI 原生向量設計工具，面向 design-as-code workflow，可在 live canvas 上把 prompts 轉成 UI，並支援 concurrent agent teams。 |

## 基準測試

Zode 的 benchmark 覆蓋 one-shot 程式碼生成、agentic 讀/跑/改/修、多檔任務、疑難 bug、MCP/Skills/約束遵循，以及 Noema LOCOMO runner。完整方法、復現命令和結果表見 [英文 README 的 Benchmark 部分](../../README.md#benchmark)。

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

## 貢獻

歡迎貢獻。請遵循 [Conventional Commits](https://www.conventionalcommits.org/)：`<type>(<scope>): <subject>`。

## 授權

[MIT](../../LICENSE) &copy; ZSeven-W
