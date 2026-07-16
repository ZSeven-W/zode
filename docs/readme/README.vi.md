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
  Đọc code, chạy command, tìm file và quản lý git trong một Rust TUI nhanh.
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

- **Multi-provider**: Anthropic, OpenAI, API tương thích OpenAI như DeepSeek, Moonshot, OpenRouter, cùng Ollama local.
- **Bộ công cụ rộng**: đọc/ghi/sửa file, tìm kiếm code và content, foreground/background shell, git, web fetch, notebooks và TODO tracking.
- **Browser control**: công cụ `browser_*` điều khiển managed Chromium hoặc Chrome profile thật thông qua Chrome bridge extension.
- **Permission không block**: mọi mutating tool đi qua allow once / always / deny, với approval prompt inline.
- **OS sandbox bật mặc định**: shell commands chạy trong macOS `sandbox-exec` hoặc Linux `bwrap`, outbound network bị từ chối theo mặc định.
- **TUI toàn màn hình**: streaming Markdown, syntax highlighting, diff preview, slash-command autocomplete, prompt history, 11 chủ đề tích hợp, settings/help overlays và UI 15 ngôn ngữ (`/language`).
- **Multi-session tabs**: chạy nhiều conversation tách biệt bằng `Ctrl+T` và resume session cũ.
- **Sub-agents và workflows**: delegate công việc có scope rõ bằng Task tool, quản lý qua `/agents` và `/workflows`.
- **Skills, MCP và hooks**: load package `SKILL.md`, kết nối MCP servers và chạy external scripts trên tool events.

## Cài đặt

### Một dòng

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

Installer tự phát hiện OS và CPU, tải binary phù hợp từ [release](https://github.com/ZSeven-W/zode/releases) mới nhất và đặt `zode` vào `PATH`.

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

Giải nén và di chuyển `zode` vào `PATH`, ví dụ `sudo mv zode /usr/local/bin/`. Linux builds dùng glibc; macOS binaries chưa ký. Nếu Gatekeeper cảnh báo, chạy `xattr -dr com.apple.quarantine ./zode`.

### Build từ source

Cần stable Rust toolchain tương đối mới:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

Binary nằm ở `target/release/zode`. Agent runtime là git submodule `vendor/agent`; hãy clone bằng `--recurse-submodules` hoặc chạy `git submodule update --init`.

## Quick Start

Cách dễ nhất là chạy `zode` rồi dùng **`/connect`**. Interactive model picker sẽ ghi config.

Bạn cũng có thể tự viết `~/.zode/config.json`:

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

Các lệnh thường dùng:

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

## Cấu hình

`providers` là source of truth cho provider definitions; top-level `provider` trỏ đến active model. OpenAI-compatible providers thường dùng `baseUrl` và `dialect`:

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
  "language": "vi"
}
```

Một provider có thể chứa nhiều model, và `/model` cho phép đổi live. Ngôn ngữ cũng đổi bằng `/language`.

## Server mode và SDK

`zode server` khởi động newline-delimited JSON-RPC server trên stdin/stdout cho editor integrations, local automation, tests và SDK clients.

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

Zode có group `tools:browser` để đọc screenshots/DOM/logs, navigate/click/type, chạy JavaScript và quản lý tabs. Target là managed Chromium hoặc Chrome thật qua extension trong [`extensions/chrome/`](../../extensions/chrome/).

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

## Slash commands thường dùng

| Command | Tác dụng |
|---|---|
| `/help` | Commands và keybindings |
| `/connect` | Connect/switch provider |
| `/model [id]` | Show/set active model |
| `/theme [id]` | Đổi chủ đề (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Resume sessions |
| `/browser ...` | Browser control |
| `/tasks` | Background tasks |
| `/mcp` | Manage MCP servers |
| `/skills` | List skills |
| `/agents` | Manage sub-agents |
| `/workflows` | Manage workflows |
| `/sandbox ...` | Control OS sandbox |
| `/language` | Switch UI language |
| `/export [path]` | Export Markdown |
| `/exit` | Exit |

Bảng đầy đủ nằm trong [README tiếng Anh](../../README.md#slash-commands).

## Instructions, MCP và Skills

Zode đọc instructions từ `~/.zode/`, project root và current directory; mỗi cấp ưu tiên `AGENTS.md` trước `CLAUDE.md`. Skills nằm trong `.zode/skills/**/SKILL.md`; MCP servers nằm trong `~/.zode/mcp.json`, `.mcp.json` hoặc `.zode/mcp.json`.

Zode cũng discover skills, commands và MCP configurations của Claude, Codex, opencode, Cursor và agents khác. External MCP tìm thấy trong project sẽ disabled theo mặc định.

## Hệ sinh thái ZSeven-W

Zode là một phần trong stack AI-native development tools của ZSeven-W:

| Sản phẩm | Vai trò |
|----------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async runtime cho LLM agents, với multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions và optional coding tools. |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework, nơi một file `.op` là app và kết nối OpenPencil-style design artifacts với runnable software. |
| [`noema`](https://github.com/ZSeven-W/noema) | Local-first, non-vector memory system cho coding agents, với lexical recall, review queues, MCP, S3 offload và enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool cho design-as-code workflows, biến prompts thành UI trực tiếp trên live canvas với concurrent agent teams. |

## Benchmark

Zode benchmarks bao gồm one-shot code generation, agentic read/run/edit/fix, multi-file tasks, tricky bugs, MCP/Skills/constraint following và Noema LOCOMO. Methodology và kết quả đầy đủ nằm trong [Benchmark section của README tiếng Anh](../../README.md#benchmark).

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

Hoan nghênh contributions. Dùng [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
