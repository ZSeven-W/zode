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
  <strong>터미널을 위한 오픈 소스 AI 네이티브 코딩 어시스턴트.</strong><br/>
  코드를 읽고, 명령을 실행하고, 파일을 검색하고, git 을 관리합니다. 모두 빠른 Rust TUI 안에서 동작합니다.
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

> 이 문서는 로컬라이즈된 README 로, 제품 개요와 빠른 시작을 다룹니다. 전체 벤치마크 세부 정보와 최신 장문 설명은 [영문 README](../../README.md)를 기준으로 합니다.

## 주요 특징

- **다중 provider**: Anthropic, OpenAI, OpenAI-compatible API(DeepSeek, Moonshot, OpenRouter 등), 로컬 Ollama 지원.
- **넓은 도구 표면**: 파일 읽기/쓰기/편집, 코드 및 콘텐츠 검색, foreground/background shell, git, web fetch, notebook, TODO tracking.
- **브라우저 제어**: 내장 `browser_*` 도구로 managed Chromium 을 제어하거나 Chrome bridge extension 으로 현재 Chrome profile 을 제어합니다.
- **비차단 권한**: 변경 작업은 allow once / always / deny 승인을 거치며, 승인 prompt 는 입력을 막지 않습니다.
- **OS sandbox 기본 활성화**: shell 명령은 macOS `sandbox-exec` 또는 Linux `bwrap` 안에서 실행되며 outbound network 는 기본 차단됩니다.
- **전체 화면 TUI**: streaming Markdown, syntax highlighting, diff preview, slash-command autocomplete, prompt history, themes, settings/help overlays, 15개 UI 언어(`/language`).
- **멀티 세션 tabs**: `Ctrl+T` 로 여러 isolated conversation 을 나란히 실행하고 과거 session 을 resume 할 수 있습니다.
- **Sub-agents 및 workflows**: Task tool 로 scope 가 명확한 작업을 위임하고 `/agents`, `/workflows` 로 관리합니다.
- **Skills, MCP, hooks**: `SKILL.md` 를 필요할 때 로드하고 MCP server 를 연결하며 tool event 에 외부 script 를 실행합니다.

## 설치

### 한 줄 설치

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

installer 는 OS 와 CPU 를 감지해 최신 [release](https://github.com/ZSeven-W/zode/releases) 에서 맞는 binary 를 다운로드하고 `zode` 를 `PATH` 에 배치합니다.

### 수동 다운로드

[releases page](https://github.com/ZSeven-W/zode/releases) 에서 platform 에 맞는 archive 를 받으세요.

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

압축을 풀고 `zode` 를 `PATH` 로 옮기세요. 예: `sudo mv zode /usr/local/bin/`. Linux build 는 glibc 기반이며 macOS binary 는 unsigned 입니다. Gatekeeper 경고가 나오면 `xattr -dr com.apple.quarantine ./zode` 를 실행하세요.

### 소스에서 빌드

최근 stable Rust toolchain 이 필요합니다.

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

binary 는 `target/release/zode` 에 생성됩니다. agent runtime 은 `vendor/agent` git submodule 이므로 `--recurse-submodules` 로 clone 하거나 `git submodule update --init` 을 실행하세요.

## 빠른 시작

가장 쉬운 방법은 `zode` 를 실행한 뒤 **`/connect`** 를 사용하는 것입니다. interactive model picker 가 설정을 작성합니다.

`~/.zode/config.json` 을 직접 작성할 수도 있습니다.

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

자주 쓰는 실행 방식:

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

## 설정 핵심

`providers` 는 model provider 의 source of truth 이며 top-level `provider` 는 active model 을 가리킵니다. OpenAI-compatible provider 는 보통 `baseUrl` 과 `dialect` 가 필요합니다.

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
  "language": "ko"
}
```

하나의 provider 는 여러 model 을 가질 수 있고 `/model` 로 live switch 할 수 있습니다. `language` 는 `/language` 로도 변경됩니다.

## Server mode 와 SDK

`zode server` 는 stdin/stdout 에 newline-delimited JSON-RPC server 를 시작합니다. editor integration, local automation, test, SDK client 에 적합합니다.

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDK 는 [`sdk/`](../../sdk/) 아래에 있습니다.

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## 브라우저 제어

Zode 는 `tools:browser` group 을 제공하며 screenshot/DOM/log 읽기, navigation/click/type, JavaScript 실행, tab 관리를 지원합니다. target 은 managed Chromium 또는 [`extensions/chrome/`](../../extensions/chrome/) 의 MV3 extension 을 통한 현재 Chrome 입니다.

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

## 자주 쓰는 slash commands

| Command | 설명 |
|---|---|
| `/help` | command 와 keybinding |
| `/connect` | provider 연결 및 전환 |
| `/model [id]` | active model 표시/설정 |
| `/sessions`, `/resume` | session 복원 |
| `/browser ...` | browser control |
| `/tasks` | background tasks |
| `/mcp` | MCP server 관리 |
| `/skills` | skills 목록 |
| `/agents` | sub-agent 관리 |
| `/workflows` | workflow 관리 |
| `/sandbox ...` | OS sandbox 제어 |
| `/language` | UI 언어 전환 |
| `/export [path]` | Markdown export |
| `/exit` | 종료 |

전체 command 표는 [영문 README](../../README.md#slash-commands)를 참고하세요.

## Project instructions, MCP, Skills

Zode 는 global `~/.zode/`, project root, current directory 의 세 단계에서 instructions 를 읽습니다. 각 directory 에서는 `AGENTS.md` 를 우선하고 없으면 `CLAUDE.md` 를 사용합니다. Skills 는 `.zode/skills/**/SKILL.md`, MCP server 는 `~/.zode/mcp.json`, `.mcp.json`, `.zode/mcp.json` 에 둘 수 있습니다.

Claude, Codex, opencode, Cursor 등 다른 agent 의 skills, commands, MCP configuration 도 발견할 수 있습니다. project 내부에서 발견된 외부 MCP 는 기본적으로 disabled 입니다.

## ZSeven-W 생태계

Zode 는 ZSeven-W 의 AI-native development tools stack 중 하나입니다.

| Product | 설명 |
|---------|------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async LLM agent runtime 으로 multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions, optional coding tools 를 제공합니다. |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework 입니다. `.op` file 을 app 으로 취급해 OpenPencil-style design artifacts 를 runnable software 로 연결합니다. |
| [`noema`](https://github.com/ZSeven-W/noema) | Coding agents 를 위한 local-first non-vector memory system 으로 lexical recall, review queues, MCP, S3 offload, enterprise policy controls 를 포함합니다. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool 입니다. design-as-code workflow 에서 prompts 를 live canvas 의 UI 로 바꾸고 concurrent agent teams 를 지원합니다. |

## 벤치마크

Zode benchmark 는 one-shot code generation, agentic read/run/edit/fix, multi-file task, tricky bugs, MCP/Skills/constraint following, Noema LOCOMO runner 를 포함합니다. 방법과 재현 명령, 상세 결과는 [영문 README 의 Benchmark](../../README.md#benchmark)를 참고하세요.

## 개발

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## 기여

Contributions are welcome. [Conventional Commits](https://www.conventionalcommits.org/) 의 `<type>(<scope>): <subject>` 형식을 사용해주세요.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
