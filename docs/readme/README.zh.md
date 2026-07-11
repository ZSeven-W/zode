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
  <strong>面向终端的开源 AI 原生编码助手。</strong><br/>
  能读取代码、运行命令、搜索文件、管理 git，并通过快速的 Rust TUI 完成这些工作。
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

> 这是本地化 README，覆盖产品概览和快速上手。完整基准测试细节和最新长文说明以 [英文 README](../../README.md) 为准。

## 亮点

- **多提供商**：支持 Anthropic、OpenAI、OpenAI 兼容 API（DeepSeek、Moonshot、OpenRouter 等）以及本地 Ollama。
- **丰富工具面**：文件读写与编辑、代码和内容搜索、前台/后台 shell、git、网页抓取、notebook、TODO 跟踪。
- **浏览器控制**：内置 `browser_*` 工具可驱动托管 Chromium，或通过 Chrome bridge 扩展控制你正在使用的 Chrome。
- **非阻塞权限**：会修改状态的工具都经过 allow once / always / deny 审批，审批提示内联显示，不阻塞你继续输入。
- **默认开启 OS 沙箱**：shell 命令在 macOS `sandbox-exec` 或 Linux `bwrap` 中运行，默认禁止出站网络。
- **全屏 TUI**：流式 Markdown、语法高亮、diff 预览、斜杠命令补全、历史输入、主题、设置与帮助浮层，以及 15 种 UI 语言（`/language`）。
- **多会话标签页**：用 `Ctrl+T` 并排运行多个隔离会话，并可恢复历史会话。
- **子代理与工作流**：通过 Task 工具委派范围明确的工作，并用 `/agents`、`/workflows` 管理。
- **技能、MCP 与 hooks**：按需加载 `SKILL.md`，连接 MCP 服务器，并在工具事件上运行外部脚本。

## 安装

### 一行安装

**macOS / Linux：**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell)：**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

安装器会自动识别 OS 和 CPU，从最新 [release](https://github.com/ZSeven-W/zode/releases) 下载匹配的二进制，并把 `zode` 放到 `PATH`。

### 手动下载

在 [releases 页面](https://github.com/ZSeven-W/zode/releases) 下载对应平台的归档：

| OS | 架构 | 资源 |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

解压后把 `zode` 移到 `PATH` 中，例如 `sudo mv zode /usr/local/bin/`。Linux 构建基于 glibc；macOS 二进制未签名，如果 Gatekeeper 提示可运行 `xattr -dr com.apple.quarantine ./zode`。

### 从源码构建

需要近期稳定版 Rust 工具链：

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

二进制位于 `target/release/zode`。agent runtime 是 `vendor/agent` git submodule，因此请使用 `--recurse-submodules` 克隆，或运行 `git submodule update --init`。

## 快速开始

最简单的方式是启动 `zode` 并运行 **`/connect`**。它会打开一个交互式模型选择器，并为你写入配置。

也可以手动写 `~/.zode/config.json`：

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

常用启动方式：

```bash
zode                          # 全屏 TUI
zode -p "explain main.rs"     # headless：执行一次提示并输出到 stdout
zode --no-tui                 # 普通 readline REPL
zode -c                       # 继续最近的会话
zode -r <id>                  # 按 id 前缀恢复会话
zode --yolo                   # 跳过审批提示（硬拒绝规则仍生效）
zode --no-sandbox             # 关闭 OS 沙箱
zode --sandbox-read-only      # 只读沙箱
zode --sandbox-allow-network  # 允许沙箱内出站网络
zode --browser                # 强制启用浏览器工具
zode --model <id>             # 覆盖模型
zode --provider <name>        # 选择配置中的提供商
zode server                   # 通过 stdio 运行 JSON-RPC app-server
```

## 配置要点

`providers` 是模型提供商的来源；顶层 `provider` 指向当前活动模型。OpenAI 兼容提供商通常需要 `baseUrl` 和 `dialect`：

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

一个 provider 可以包含多个模型，并可通过 `/model` 实时切换。`language` 也可通过 `/language` 修改。

## Server 模式与 SDK

`zode server` 会在 stdin/stdout 上启动换行分隔的 JSON-RPC server，适用于编辑器集成、本地自动化、测试和 SDK 客户端。

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDK 位于 [`sdk/`](../../sdk/)：

| SDK | 目录 | 本地测试 |
|-----|------|----------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

方法、参数、返回值和 SDK enum/constant 名称记录在 [`sdk/` 方法参考](../../sdk/README.md#method-reference) 中。

## 浏览器控制

Zode 提供 `tools:browser` 工具组：

- `browser_read`：截图、DOM 快照、console/network 日志、标签页读取。
- `browser_act`：导航、点击、输入、按键、滚动。
- `browser_eval`：执行 JavaScript。
- `browser_tabs`：管理标签页。

可选目标：

- **managed**：Zode 启动并控制专用 Chromium profile。
- **bridge**：通过 [`extensions/chrome/`](../../extensions/chrome/) 中的 MV3 扩展控制你正在使用的 Chrome profile。

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

扩展加载、更新、CRX 打包和 smoke test 步骤见 [`extensions/chrome/README.md`](../../extensions/chrome/README.md)。

## 常用斜杠命令

| 命令 | 作用 |
|---|---|
| `/help` | 命令和快捷键帮助 |
| `/connect` | 连接并切换当前 provider |
| `/model [id]` | 查看或设置当前模型 |
| `/config` | 查看模型与工作目录 |
| `/sessions`, `/resume` | 恢复历史会话 |
| `/browser ...` | 浏览器控制面板和命令 |
| `/tasks` | 后台 shell 和运行中的 turn |
| `/mcp` | 管理 MCP server |
| `/skills` | 列出可用技能 |
| `/agents` | 管理子代理 |
| `/workflows` | 管理和运行 JS 工作流 |
| `/sandbox ...` | 查看或控制 OS 沙箱 |
| `/language` | 切换 UI 语言 |
| `/export [path]` | 导出会话为 Markdown |
| `/exit` | 退出 |

完整命令表见 [英文 README](../../README.md#slash-commands)。

## 项目指令、MCP 与技能

Zode 会按层级读取指令：全局 `~/.zode/`、项目根目录、当前工作目录；每层优先使用 `AGENTS.md`，再回退到 `CLAUDE.md`。技能位于 `.zode/skills/**/SKILL.md`，MCP server 位于 `~/.zode/mcp.json`、`.mcp.json` 或 `.zode/mcp.json`，hooks 位于 `~/.zode/hooks.json` 或 `.zode/hooks.json`。

Zode 也可以发现 Claude、Codex、opencode、Cursor 等其他 agent 已安装的技能、命令和 MCP 配置。跨 agent 导入默认偏保守；项目内发现的外部 MCP 默认禁用，需要你显式启用。

## ZSeven-W 生态

Zode 属于 ZSeven-W 面向 AI 原生开发工具的一组产品：

| 产品 | 定位 |
|------|------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | 纯 Rust 异步 LLM agent runtime，提供多提供商流式输出、工具调度、权限、MCP、成本跟踪、附件、会话和可选编码工具。 |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust 原生跨平台 UI 框架，让 `.op` 文件成为应用，把 OpenPencil 风格设计产物连接到可运行软件。 |
| [`noema`](https://github.com/ZSeven-W/noema) | 面向编码 agent 的 local-first、非向量记忆系统，包含词法召回、review queue、MCP、S3 offload 和企业策略控制。 |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | 开源 AI 原生向量设计工具，面向 design-as-code 工作流，可在实时画布上把 prompt 转成 UI，并支持并发 agent teams。 |

## 基准测试

Zode 的 benchmark 覆盖 one-shot 代码生成、agentic 读/跑/改/修、多文件任务、疑难 bug、MCP/Skills/约束遵循，以及 Noema LOCOMO runner。完整方法、复现命令和结果表见 [英文 README 的 Benchmark 部分](../../README.md#benchmark)，套件位于 [`benchmarks/`](../../benchmarks/)。

## 开发

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## 贡献

欢迎贡献。请遵循 [Conventional Commits](https://www.conventionalcommits.org/)：`<type>(<scope>): <subject>`，常见 scope 包括 `core`、`tui`、`cli`、`tools`、`config`、`build`、`ci`、`docs`。

## 许可证

[MIT](../../LICENSE) &copy; ZSeven-W
