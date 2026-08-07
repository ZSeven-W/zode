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
- **넓은 도구 표면**: 파일 읽기/쓰기/편집(원자적 multi-hunk `MultiEdit` 포함), 코드 및 콘텐츠 검색, foreground/background shell, git, web fetch(Tavily key 로 선택적 `WebSearch` 활성화), notebook, TODO tracking.
- **브라우저 제어**: 내장 `browser_*` 도구로 managed Chromium 을 제어하거나 Chrome bridge extension 으로 현재 사용 중인 Chrome 을 제어합니다. 페어링은 한 번만 하면 되며, extension 은 zode 를 재시작해도 자동으로 다시 연결됩니다.
- **비차단 권한**: 상태를 변경하는 도구는 allow once / always / deny 승인을 거치며, 승인 prompt 는 인라인으로 표시되어 입력을 막지 않습니다.
- **OS sandbox 기본 활성화**: shell 명령은 macOS `sandbox-exec` 또는 Linux `bwrap` 안에서 실행되며 outbound network 는 기본 차단됩니다.
- **전체 화면 TUI**: streaming Markdown, syntax highlighting, diff preview, slash-command autocomplete, prompt history, 11개 내장 테마, settings/help overlay, 15개 UI 언어(`/language`).
- **V1 호환 지속 세션**: 기존 `<id>.jsonl` 세션 프로토콜을 유지하면서 sidecar 데이터로 journal, checkpoint, rewind, fork, 격리된 Git worktree 를 추가합니다. 컨텍스트 압축은 보이는 대화를 잃지 않습니다 — resume 시 압축 전 전체 히스토리를 다시 재생하고, 모델 컨텍스트는 압축된 상태를 유지합니다.
- **자동화 인터페이스**: 안정적인 JSON/JSONL headless 출력, 정확한 세션 타깃팅, 도구 필터링, 결정적 exit code, stdio ACP, 로컬 dashboard.
- **멀티 세션 tabs**: `Ctrl+T` 로 여러 isolated conversation 을 나란히 실행하고 과거 session 을 resume 할 수 있습니다.
- **Sub-agents, team 및 workflows**: Task 로 일회성 작업을 위임하고 내부 또는 외부 CLI teammate 를 수동 등록하여 `/agents`, `/team`, `/workflows` 로 관리합니다.
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

Rust 1.88 이상이 필요합니다.

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

binary 는 `target/release/zode` 에 생성됩니다. agent runtime 은 `vendor/agent` git submodule 이므로 `--recurse-submodules` 로 clone 하거나 `git submodule update --init` 을 실행하세요.

## 빠른 시작

가장 쉬운 방법은 `zode` 를 실행한 뒤 **`/connect`** 를 사용하는 것입니다. interactive model picker 가 열리고 설정을 대신 작성해 줍니다.

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
zode                          # 전체 화면 TUI
zode -p "explain main.rs"     # headless: prompt 를 한 번 실행하고 stdout 으로 출력
zode --no-tui                 # 일반 readline REPL
zode -c                       # 가장 최근 session 이어서 진행
zode -r <id>                  # id prefix 로 session 복원
zode --yolo                   # 승인 prompt 건너뛰기(hard deny 규칙은 계속 적용)
zode --no-sandbox             # OS sandbox 끄기
zode --sandbox-read-only      # 읽기 전용 sandbox
zode --sandbox-allow-network  # sandbox 내 outbound network 허용
zode --browser                # 브라우저 도구 강제 활성화
zode --model <id>             # model 오버라이드
zode --provider <name>        # 설정의 provider 선택
zode server                   # stdio 로 JSON-RPC app-server 실행
zode acp                      # stdio 로 ACP agent 실행
zode dashboard                # 로컬 session, checkpoint, worktree 보기
```

## 외부 CLI teammate 수동 등록

Zode 는 타사 agent CLI 를 일회성 Task worker 또는 대화를 이어가는 team 의
teammate 로 사용할 수 있습니다. 등록은 명시적입니다. 실행 파일이 `PATH` 에
있더라도 Zode 는 자동으로 model 에 노출하지 않으며 `externalAgents.agents` 에
profile 을 추가해야 합니다. `/external-agents` 로 `PATH` 의 지원 CLI 를
확인하고, `/external-agents discover` 로 발견된 preset 을 전역 설정에 명시적으로
등록할 수도 있습니다. Zode 는 시작 시에도 자동 검색이나 등록을 하지 않습니다.

| Profile | Command | Task | Team mode | 외부 CLI sandbox |
|---|---|---:|---:|---|
| `claude-code` | `claude` | yes | persistent | unrestricted |
| `codex` | `codex` | yes | persistent | workspace-write |
| `opencode` | `opencode` | yes | stateless | unknown |
| `cline` | `cline` | yes | stateless | unrestricted |
| `antigravity` | `agy` | yes | stateless | unknown |
| `cursor` | `cursor-agent` | yes | persistent | unrestricted |
| `kiro` | `kiro-cli` | yes | stateless | unrestricted |
| `pi` | `pi` | yes | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | yes | persistent | unrestricted |

### Profile 추가

모든 project 에 적용하려면 전역 `~/.zode/config.json`, 한 project 에만
적용하려면 `.zode/config.json` 에 설정합니다. 알려진 profile 은 빈 object 로
수동 활성화할 수 있고 `command` 에는 `PATH` 의 이름이나 path 를 쓸 수 있습니다.

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

실제로 노출할 profile 만 추가하세요. 알려진 preset 은 `enabled`, `command`,
`extraArgs`, `envAllow`, `trusted` 를 오버라이드할 수 있습니다. Custom profile 의
`promptTransport` 는 `stdin`, `argv`, `file` 을, `output` 은 `text`, generic
`jsonl`, `jsonl-claude`, `jsonl-codex` 를 지원합니다. Generic JSONL 은 RFC 6901
`textSource` 와 `sessionIdSource` 로 text 와 session ID 를 추출하며,
`resumeArgs` 에는 독립된 `{session_id}` token 이 필요합니다. Resume 가능한
session 이 없는 CLI 는 send 마다 새 process 를 쓰는 stateless teammate 가 되고,
일회성 Task worker 로도 사용할 수 있습니다. `newSessionArgs` 에도 독립된
`{session_id}` 를 넣을 수 있는데, Zode 가 첫 run 의 ID 를 생성하고 이후
assignment 에서는 `resumeArgs` 를 사용합니다.

외부 process 는 기본적으로 `PATH`, `HOME`, `TERM` 등 기본 환경만 상속하므로 API
key 같은 변수는 `envAllow` 또는 `authEnv` 에 추가해야 합니다. 일반 mode 에서는
첫 hire 때 command, 작업 디렉터리, sandbox 를 표시하고 trust 를 요청합니다.
Zode 는 process 시작만 gate 하며 외부 CLI 의 개별 file edit 나 shell command 는
gate 하지 않습니다. `--yolo` 같은 비대화형 mode 에서는 명시적인
`trusted: true` 가 설정되어야 실행됩니다.

### Team 사용

`team_hire` 와 `team_send` 는 model-facing tool 이며 slash command 가 아닙니다.
leader 에게 자연어로 요청하세요.

```text
`codex` 를 `implementer` 라는 이름으로 hire 하고 인증 refactor 와 test 실행을 맡겨 주세요.
편집 전에 `src/auth/` 를 claim 한 뒤 task 를 `implementer` 에게 보내 주세요.
```

이후 `/team`, `/team board` 로 roster 와 협업 board 를 확인하고,
`/team dismiss implementer` 로 teammate 를 제거합니다. Team state 는
`<cwd>/.zode/team/` 에 저장되지만 외부 CLI 의 trust grant 는 Zode process
사이에 유지되지 않습니다.

## 새 기능 사용 가이드

### 구조화된 headless

`-p`, `--prompt-file`, `--prompt-json` 은 같은 headless 엔진을 공유합니다. `json`
은 최종 결과 object 만 출력하고, `stream-json` 은 버전이 붙은
`zode.run-event.v1` event 를 줄 단위로 출력합니다. 구조화 mode 는 stdout 을
기계가 읽을 수 있는 데이터 전용으로 쓰고, 안정적인 exit code 를 사용합니다: `0`
성공, `10` provider 오류, `11` 권한 거부, `12` 턴 한도, `13` 중단(Ctrl-C), `14`
부분 결과, `15` 세션 타깃팅 오류.

```bash
zode -p "fix the failing test" --output-format json --max-turns 12
zode -p "review the repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the current workspace"}'

# 정확한 ID 는 prefix 매칭을 하지 않으며 fork 는 원본 session 을 수정하지 않습니다.
zode -p "continue the work" --session-id my-session
zode -p "try an alternative" --fork-session my-session --fork-worktree
```

도구 deny 규칙은 allow 보다 우선하며 Task sub-agent 에도 전파됩니다.
`--permission-mode` 는 `default`, `dont-ask`, `accept-edits`, `bypass` 를
지원합니다. `--yolo` 는 여전히 bypass 의 단축키이지만 hard deny 규칙은 항상
적용됩니다.

### Session V1 직접 확장

session transcript 는 여전히 하나입니다: `~/.zode/sessions/<id>.jsonl`. 구버전
Zode 도 이 파일을 계속 읽고 쓸 수 있고, 신버전도 같은 파일을 직접 읽고 씁니다.
추가 데이터는 `~/.zode/sessions/<id>/` sidecar 디렉터리에만 저장되며 여기에는
`meta.json`, journal, checkpoint, snapshot 이 들어갑니다. 따라서 새 session
버전을 만들 필요도, transcript 를 이중으로 기록할 필요도 없습니다.

```bash
zode session list
zode session list --json
zode session show <id>                         # metadata 와 checkpoint ID 보기
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # 충돌과 변경만 미리 보기
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

수정 side effect 가 있는 turn 전에 checkpoint 가 생성됩니다. Rewind 는 추적
파일과 session 메시지 prefix 를 복원하고, 새 변경을 만나면 덮어쓰지 않고 충돌로
보고합니다. 과거 journal 은 삭제되지 않고 새 논리 branch 를 만듭니다. worktree
fork 의 결과는 `apply-back` 으로 명시적으로 병합해야 합니다.

**압축은 보이는 대화를 잃지 않습니다.** 컨텍스트 압축이 오래된 메시지를 요약으로
대체할 때 원본은 추가 전용 sidecar(`~/.zode/sessions/<id>/compacted.jsonl`)
에 보존됩니다. session resume, `Ctrl+L`, `/export`, Chrome side panel 은 모두
압축 전 전체 히스토리를 표시하고, 모델은 계속 압축된 컨텍스트만 받습니다.
fork 는 자신의 transcript 로 필터링된 archive 를 함께 가져가고, `/clear` 는
archive 를 삭제하며, session 을 삭제하면 sidecar 전체가 제거됩니다.

### 권한 규칙과 sandbox profile

권한 규칙은 `config.json` 의 `permissions.rules` 에 넣거나
`--rules ./permissions.json` 으로 일시적으로 로드할 수 있습니다. 필드 매칭에는
RFC 6901 JSON pointer 를 사용하고 우선순위는 deny > ask > allow 로 고정됩니다.
독립된 rules 파일은 규칙 배열이거나 `{ "rules": [...] }` 여야 하며, 다시
`permissions` 로 감싸면 안 됩니다.

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
zode -p "checks only" --sandbox-profile read-only
zode -p "run checks" --sandbox-profile workspace
zode -p "download deps" --sandbox-profile workspace-network
zode -p "run CI" --sandbox-profile ci --rules ./permissions.json
```

내장 profile 은 `read-only`, `workspace`, `workspace-network`, `unconfined`
이며, 위 예시처럼 직접 profile 을 정의할 수도 있습니다. Windows 에서는 sandbox 가
계층(tier) 으로 동작합니다: OS 지원 여부에 따라 사용 가능한 격리를 적용하고,
가장 강한 tier 를 쓸 수 없으면 안전한 fallback tier 로 낮춥니다.

### 플러그인과 정적 marketplace

관리형 플러그인에는 skills, commands, agents, hooks, MCP, LSP, 그리고 제한된
JavaScript UI 렌더러가 포함될 수 있습니다. Zode 는
`plugin.json`, `.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`,
`.grok-plugin/plugin.json`, `.claude-plugin/plugin.json` 을 지원합니다. Codex 와
Claude Code 의 컴포넌트 경로 배열을 함께 지원하며, 최초 설치 시 Claude Code 의
`defaultEnabled` 를 따릅니다. Codex apps/connectors 나 Claude Code themes,
monitors, output styles 같은 호스트 전용 컴포넌트는 무시되고, app 만 담긴
플러그인은 Zode 가 쓸 수 있는 컴포넌트가 없어 설치가 거부됩니다. 설치 내용은
출처와 SHA-256 tree hash 가 붙은 불변 snapshot 으로 복사되며, 실행 가능한 능력을
포함한 플러그인은 명시적으로 `--trust` 를 전달해야 활성화됩니다.

#### JavaScript UI 플러그인 빠른 시작

최소한의 UI 플러그인은 manifest 와 JavaScript 파일 하나만 있으면 됩니다.

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

로컬 디렉터리를 설치할 수도 있고, GitHub 저장소나 저장소 하위 디렉터리를 바로
설치할 수도 있습니다. 실행 중인 Zode 는 새 플러그인 snapshot 을 로드하려면
재시작해야 합니다.

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

소스를 수정한 뒤에는 `zode plugin update my-plugin` 으로 설치된 snapshot 을
갱신합니다. JavaScript, hooks, MCP server, 선언된 network 접근은 모두 실행 가능한
능력이므로 설치 시 반드시 `--trust` 를 전달해야 합니다. 설치와 업데이트는
플러그인이 선언한 권한(network 도메인, 환경 변수, context scope) 을 출력합니다.
업데이트된 manifest 가 설치된 snapshot **보다 넓은** 권한을 요청하면 업데이트가
거부되며, 다시 `--trust` 를 전달해야 수락됩니다. 활성 Git 소스가 자신의 권한을
조용히 확대할 수는 없습니다.

#### UI 렌더 API

UI 플러그인은 sidebar 버전 번호 바로 위에 선언적 콘텐츠를 렌더링할 수
있습니다. 모든 플러그인은 로드 순서대로 합쳐서 최대 6줄까지 표시됩니다. manifest
가 JS 진입점을 지정합니다.

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

JS 는 `zode.ui.sidebar` 로 동기 렌더 함수를 등록합니다. `ctx` 는 terminal,
session, model, status, token, context window 정보를 담은 읽기 전용 JSON
snapshot 입니다. script 는 파일 시스템, network, terminal, Ratatui 핸들에
접근할 수 없으며 최종 스타일과 너비는 Zode 가 제어합니다.

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

`tone` 은 `default`, `muted`, `accent`, `success`, `warning`, `danger` 를
지원하고 span 은 `bold` 와 `italic` 도 지원합니다. renderer 는 반드시 동기
함수여야 합니다. script 당 최대 256 KiB, 실행 1회당 최대 8 MiB JS 메모리와
25 ms 를 사용하며, renderer 는 최대 250 ms 마다 한 번씩만 다시 평가됩니다(그
간격 안에서는 캐시된 출력 재사용). sidebar 는 renderer 당 최대 6줄(모든 플러그인
합산도 6줄), 줄당 최대 16 span, 텍스트 2,048 바이트이며 제어 문자는 호스트가
정리합니다.

status line 도 확장할 수 있습니다. 플러그인 출력이 없으면 1줄을 유지하고, 동기
`zode.ui.statusLine` renderer 가 span 을 반환하면 레이아웃이 2줄로 확장됩니다.
Zode 자체의 핵심 상태와 보안 알림은 첫 줄에 고정되고 플러그인 출력은 둘째 줄에
병합됩니다.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### 렌더 context 와 권한

모든 renderer 는 별도의 context 권한 없이 다음 기본 필드에 접근할 수 있습니다.

| 필드 | 구조와 의미 |
| --- | --- |
| `ctx.apiVersion` | context API 버전, 현재 `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | `{ width, height }`, 단위는 terminal cell. |
| `ctx.session` | 현재 작업의 `{ id, title, cwd, busy }`. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` 는 `{ enabled, readOnly, network }`. |
| `ctx.tokens` | `{ input, output }` token 수. |
| `ctx.context` | `{ used, window, usedPercent }`; 계산이 불가능하면 백분율은 `null`. |
| `ctx.data` | 현재 플러그인이 직접 등록한 백그라운드 데이터 소스 결과만 포함. |

더 풍부한 정보는 `permissions.context` 에서 해당 scope 를 선언해야 나타납니다.

| Scope | 노출 필드 | 구조와 제한 |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` 는 1부터 시작. |
| `workspace` | `ctx.workspace.modifiedFiles` | 최대 50개 Git `{ path, added, removed }`. |
| `tools` | `ctx.tools.available` | 현재 작업에서 활성화된 도구 이름, 정렬됨. |
| `tools` | `ctx.tools.active` | 지금 실행 중인 도구 이름. |
| `tools` | `ctx.tools.recent` | 최근 최대 20개 `{ name, status, durationMs }`. |
| `tasks` | `ctx.tasks.todoStatuses` | Todo 상태만, Todo 본문 제외. |
| `tasks` | `ctx.tasks.subagents` | `{ type, status }`, prompt 나 대화 제외. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, Goal 본문 제외. |
| `services` | `ctx.services.mcp` | `{ name, connected }`. |
| `services` | `ctx.services.lsp` | `{ language, running }`. |

예:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` 는 관찰용 인터페이스입니다. 플러그인은 어떤 도구가 있고 어느 것이
실행 중이거나 최근 실행되었는지 알 수 있지만 UI 플러그인이 직접 도구를 호출할
수는 없습니다. 도구 입력/출력, prompt, 대화 본문, Todo/Goal 본문, 환경 변수 값,
자격 증명은 노출되지 않으며 이를 통해 Zode 기존 승인 시스템을 우회할 수도
없습니다.

#### 백그라운드 HTTP 데이터

UI 플러그인은 백그라운드 HTTP 데이터 소스도 등록할 수 있습니다. network
도메인과 자격 증명 환경 변수는 manifest 에서 명시적으로 선언해야 합니다.

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

요청은 선언적으로 구성되며 렌더 경로 밖에서 실행됩니다. Zode 는 Rust 요청
계층에서만 환경 변수를 header 에 조립하고 JS 는 token 을 읽을 수 없습니다.

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

`zode.data.define(key, config)` 의 key 는 길이 1–64 이고 문자, 숫자, 밑줄,
하이픈만 쓸 수 있습니다. `request` 는 `url`, `method`, `headers`, 선택적 JSON
`body`, `timeoutMs` 를 지원합니다. 기본값은 `GET`, 3초 timeout, 60초 refresh
간격이며 현재는 HTTPS `GET`/`POST` 만 허용됩니다. 일반 header 값은 문자열이고,
비밀 header 는 `{ "env": "NAME", "prefix": "Bearer " }` 형식을 씁니다. 환경
변수는 `permissions.env` 에도 나열되어야 하며 전송 시 Rust 요청 계층에서만
읽히고 절대 JS 로 반환되지 않습니다.

Zode 는 redirect 와 proxy 를 비활성화하고, 공인 DNS 를 검증·고정하며,
localhost/사설망을 거부하고, 응답을 256 KiB 로 제한하고, 요청 timeout 을 500 ms
~ 10초로, refresh 간격을 10초 ~ 1시간으로 제한합니다. `*.example.com` 은
하위 도메인만 매칭하고 bare 도메인 `example.com` 은 매칭하지 않습니다.

각 플러그인은 자신의 데이터만 볼 수 있습니다. `ctx.data.<key>` 결과는
`{ ok, status, data, updatedAt }` 형태이고, 요청 실패 시
`{ ok: false, error, updatedAt }` 입니다. JSON 응답은 object 나 array 가 되고
비 JSON 응답은 문자열이 됩니다. HTTP 오류 상태에서도 `status` 와 `data` 는
제공되며 이때 `ok` 는 `false` 입니다.

사설 quota 나 Coding Plan API 를 호출하려면 Zode 를 시작하기 전에 환경 변수를
제공해야 합니다.

```bash
CODING_PLAN_TOKEN=... zode
```

[완전히 실행 가능한 예제](../../examples/plugins/zode-ui-demo/)는 sidebar 와
status line 에 model, context, 도구 활동을 표시하고 `zode.data.define` 으로
공개 GitHub API quota 를 읽습니다.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# marketplace 는 로컬 디렉터리나 Git 정적 인덱스이며 Zode 클라우드 서비스에 의존하지 않습니다.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # 동명일 때 출처 지정
zode plugin marketplace update
```

### ACP, dashboard, OTLP, PTY 테스트

`zode acp` 는 stdio 로 ACP initialize/new/load/fork/prompt/cancel 을 구현하고,
클라이언트로 메시지·사고·도구 event 를 스트리밍하며, 클라이언트를 통해 권한을
요청하고, 클라이언트가 제공하는 stdio, HTTP, SSE MCP server 를 지원합니다.
TUI/headless 와 동일한 V1 호환 session store 를 공유합니다.

```bash
zode acp
zode dashboard
zode dashboard --json
```

OTLP 는 기본 비활성이며 `ZODE_OTEL=1` 을 명시적으로 설정해야 합니다. 내보내는
것은 내용이 없는 lifecycle, 도구 이름, 상태, token usage 속성뿐입니다. prompt,
model 텍스트, 도구 입력/출력, 파일 경로, 오류 메시지는 내보내지 않습니다.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the tests" --output-format json
```

저장소에는 실제 PTY + VT100 가상 화면 테스트 도구도 있어 raw diagnostics 와
화면 snapshot 을 기록할 수 있습니다.

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` 은 실제 terminal 의 wait, keypress, resize, snapshot 을 순서대로
구동합니다. keypress 표기는 `<Enter>`, `<Esc>`, `<Tab>`, 방향키, `<Backspace>`,
`<C-c>`, `<C-d>`, `<C-l>` 을 지원합니다.

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

로컬 오픈 소스 버전은 xAI 전용 계정/청구를 포함하지 않으며 Zode 가 운영하는
클라우드 marketplace 서비스도 구축하지 않습니다.

## 설정 핵심

`providers` 는 model provider 의 source of truth 이고 top-level `provider` 는
active model 을 가리킵니다. OpenAI-compatible provider 는 보통 `baseUrl` 과
`dialect` 가 필요합니다.

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

하나의 provider 는 여러 model 을 가질 수 있고 `/model` 로 live switch 할 수
있습니다. `language` 는 `/language` 로도 변경됩니다.

## Server mode 와 SDK

`zode server` 는 stdin/stdout 에 newline-delimited JSON-RPC server 를 시작합니다.
editor integration, local automation, test, SDK client 에 적합합니다.

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

여기서 말하는 "미지원" 은 app-server 프로토콜 자체에만 한정됩니다: managed
marketplace 관리, 원격 제어, Realtime, background terminal, thread
archive/fork, goals, app connector 는 아직 제공하지 않습니다. 위에서 설명한 로컬
Session V1 명령과 정적 플러그인 marketplace 는 별도의 CLI 기능이며 이 제한에
해당하지 않습니다.

SDK 는 [`sdk/`](../../sdk/) 아래에 있습니다.

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

메서드, 파라미터, 반환값, SDK enum/상수 이름은 [`sdk/` 메서드 레퍼런스](../../sdk/README.md#method-reference)에 정리되어 있습니다.

## 브라우저 제어

Zode 는 `tools:browser` 도구 그룹을 제공합니다.

- `browser_read`: screenshot, DOM snapshot, console/network 로그, 탭 읽기.
- `browser_act`: navigate, click, type, key, scroll.
- `browser_eval`: JavaScript 실행.
- `browser_tabs`: 탭 관리.

선택 가능한 target:

- **managed**: Zode 가 전용 Chromium profile 을 시작하고 제어합니다.
- **bridge**: [`extensions/chrome/`](../../extensions/chrome/) 의 MV3 extension 을 통해 현재 사용 중인 Chrome profile 을 제어합니다.

업그레이드 후에는 새 버전 Zode 를 한 번 실행하고 `/browser pair` 를 실행하세요.
이는 고정된 extension ID 만 호출을 허용하는 Chrome Native Messaging host 를
등록합니다. **페어링은 한 번만 필요합니다**: extension 이 장기 token 을 저장한
뒤 자동으로 다시 연결됩니다 — 브라우저 시작 시, extension 업데이트 시, 연결이
끊긴 동안에는 약 30초마다 재시도하므로 zode 를 재시작해도 다시 페어링할 필요가
없습니다. Chrome 은 외부 프로그램이 여는 `chrome-extension://` URL 을
차단하므로(ERR_BLOCKED_BY_CLIENT — macOS/Windows/Linux 모두 동일) zode 가
직접 페이지를 여는 시도는 실패할 수 있습니다. 대신 `/browser pair` 후 약 30초
안에 **extension 스스로** pairing 페이지를 엽니다(포트는 미리 입력되어 있으니
채팅에 표시된 6자리 페어링 코드를 입력하세요). 수동 대안으로
`chrome-extension://…/popup.html?port=…` URL 을 주소창에 직접 입력해 열 수도
있습니다(직접 입력한 내비게이션은 브라우저가 시작한 것이라 차단되지 않습니다). 이후에는 Zode CLI 를 열어 두지 않아도 side panel 이 terminal 없는
로컬 Zode daemon 을 자동으로 시작하고, 저장된 token 으로 task 와 기록을
복원합니다. side panel 에서 task 를 제출하면 브라우저 도구가 panel 옆의 현재
페이지에 바인딩되므로 "현재 페이지 분석" 은 새 탭을 만들지 않고 기존 탭을 바로
읽습니다. 독립적인 TUI/CLI 자동화는 여전히 `zode` 탭 그룹을 사용합니다. side
panel 에서 "이것", "현재 내용" 같은 모호한 표현도 기본적으로 현재 페이지를
가리키며, agent 는 먼저 페이지를 읽습니다. 사용자가 명시적으로 프로젝트, 코드,
로컬 파일에 대해 물을 때만 로컬 workspace 를 우선 확인합니다.

자주 쓰는 명령:

```bash
/browser
/browser status
/browser launch
/browser close
/browser pair
/browser target managed
/browser target bridge        # extension bridge 로 전환하고 다음 시작의 기본 target 으로 저장
/browser screenshot [path]
```

extension 로드, 업데이트, CRX 패키징, smoke test 단계는 [`extensions/chrome/README.md`](../../extensions/chrome/README.md)를 참고하세요.

## 데스크톱 제어

Zode 는 운영체제의 접근성(accessibility) API 로 브라우저에 국한되지 않고 네이티브
데스크톱 앱도 구동할 수 있습니다.

- `desktop_read`: 접근성 트리(창, 요소, 각 요소의 ref) 읽기.
- `desktop_act`: 요소 단위 click, type, scroll, set value.
- `desktop_screenshot`: 화면 캡처.

읽기 전용 조회는 승인이 필요 없습니다. side effect 가 있는 데스크톱 작업은 다른
도구와 동일한 allow once / always / deny 승인 흐름을 거칩니다.

플랫폼별 backend:

- **macOS** — Accessibility(AX) API.
- **Windows** — UI Automation(UIA).
- **Linux** — AT-SPI.
- **Electron 앱** — Chrome DevTools Protocol 로 attach.

**가짜 커서와 Esc 급정지.** Zode 는 실제 마우스 커서를 절대 움직이지 않습니다.
macOS 에서는 무권한 overlay(`zode-overlay`) 가 *가짜* 커서를 그려 부드러운 Dubins
경로를 따라 각 작업 대상으로 이동시키므로 agent 의 동작을 따라가기 쉽습니다(입력된
텍스트는 overlay 에 표시되지 않습니다). 데스크톱 자동화가 진행 중일 때 전역 **Esc**
는 실행 중인 모든 turn 을 중단하고 overlay 를 숨깁니다(TUI 의 Esc 와 같은 급정지
경로). 다른 플랫폼은 시각화 없이 데스크톱 작업을 그대로 수행합니다.

US 배열 keycode 가 없는 문자(CJK, 일부 문장 부호) 는 시스템 클립보드로
전달됩니다(쓰기 → paste 합성 → 원래 클립보드 복원). 그래서 커스텀 키 처리를 하는
앱에서도 실제 문자를 받을 수 있습니다.

```bash
/desktop          # 데스크톱 target 과 권한 상태 표시
/desktop status   # 위와 동일
```

설정은 `~/.zode/config.json` 의 `desktop.*` 에 있습니다.

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor`(기본 `true`) 는 macOS overlay 커서를 그리고, `escCancel`(기본
`true`) 은 자동화 중 전역 Esc 중단을 활성화하며, `overlayHelperPath`(기본
`null`) 은 `zode-overlay` helper 경로를 오버라이드합니다. helper 가 없으면
시각화만 꺼집니다. 데스크톱 자동화를 처음 사용할 때 시스템 권한(예: macOS 손쉬운
사용) 을 요청할 수 있습니다.

## 백그라운드 turn watchdog, `/loop`, `/schedule`, 작업 타이밍

Zode 는 무인 상태로 실행되는 turn 을 감시하고 반복 작업을 예약할 수 있습니다.

- **`/loop <30s|5m|1h> [--max N] <prompt>`** — 현재 탭에서 반복되는 turn 을
  세션 한정으로 실행합니다. `list` / `stop [id]` 를 지원하며 최소 간격은 30s
  입니다. 만기가 된 prompt 는 실행 중인 turn 을 절대 중단하지 않고 큐에
  들어갑니다.
- **`/schedule add <hh:mm|mon hh:mm|every 2h> <prompt>`** — `~/.zode/schedules.json`
  에 지속 저장됩니다(atomic tmp+rename, 손상된 파일은 `.corrupt` 로 격리). Zode
  가 실행되지 않는 동안 놓친 trigger 는 재생하지 않고 건너뜁니다. `list` /
  `rm <id>` / `enable|disable <id>` 를 지원합니다.
- 둘 다 `zode-core/src/scheduler/` 에 있으며 TUI tick 으로 구동됩니다.

**작업 타이밍.** `tool.completed` 와 `turn.completed` 이벤트에 `durationMs` 가
기록됩니다. TUI 는 도구별 `· 1.2s` suffix, `✓ done · 34s · 3 tools` turn footer,
`/tasks` 의 humanized 경과 시간을 표시합니다.

**watchdog.** 스케줄러가 소유한 `/loop` / `/schedule` turn 만 백그라운드
watchdog 에 등록됩니다. 일반 대화형 turn 은 등록되지 않습니다. 이는 프로세스 내
turn watchdog 이며 OS supervisor 가 아닙니다. 프로세스 crash 나 재부팅 후 Zode 를
다시 시작하지는 않습니다. `inactivityTimeoutSecs` 는 idle 한도이고
`maxRuntimeSecs` 는 절대 turn 한도입니다. 한도를 넘기면 먼저 협조적 abort 를
보내고, `abortGraceSecs` 후에도 terminal event 가 없으면 로컬 turn task 를 hard
abort 합니다. 실패는 상한이 있는 exponential backoff 로 재시도하며, `maxRetries`
소진 후에는 loop 를 멈추거나 지속 schedule 을 disable 합니다. 안전 정책은
비멱등 작업에 보수적입니다: side effect 가 관찰되지 않은 경우에만 자동 재시도하고,
mutation 이 완료되었을 수 있으면 작업을 멈추거나 disable 하고 사람의 검토를
기다립니다.

상태 확인:

```bash
/loop list
/loop stop
/schedule list
/watchdog status
/tasks
```

`backgroundWatchdog` 은 top-level camelCase 설정으로 `enabled`(기본 `true`),
`inactivityTimeoutSecs`(`900`), `maxRuntimeSecs`(`3600`), `abortGraceSecs`(`10`),
`maxRetries`(`3`), `initialBackoffSecs`(`5`), `maxBackoffSecs`(`300`) 를
지원합니다. `/watchdog status` 는 유효 설정과 live/retry 상태를, `/tasks` 는
백그라운드 shell 및 실행 중인 turn 옆에 같은 health 정보를 표시합니다.

## 자주 쓰는 slash commands

| Command | 설명 |
|---|---|
| `/help` | command 와 keybinding 도움말 |
| `/connect` | 현재 provider 연결 및 전환 |
| `/model [id]` | active model 표시/설정 |
| `/theme [id]` | 테마 전환 (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/config` | model 과 작업 디렉터리 보기 |
| `/sessions`, `/resume` | 과거 session 복원 |
| `/browser ...` | 브라우저 제어 패널과 명령 |
| `/desktop ...` | 데스크톱 target 과 권한 상태 |
| `/loop ...` | 세션 한정 반복 turn |
| `/schedule ...` | 지속되는 예약 작업 |
| `/watchdog status` | 백그라운드 watchdog 설정과 상태 |
| `/tasks` | 백그라운드 shell 과 실행 중인 turn |
| `/mcp` | MCP server 관리 |
| `/skills` | 사용 가능한 skills 목록 |
| `/agents` | sub-agent 관리 |
| `/external-agents [list\|discover]` | `PATH` 의 지원 외부 CLI 확인, 또는 발견된 preset 명시적 등록 |
| `/team [status\|board\|dismiss <name>]` | persistent teammate roster 와 shared board 확인, 또는 teammate 제거 |
| `/workflows` | JS workflow 관리 및 실행 |
| `/sandbox ...` | OS sandbox 확인/제어 |
| `/language` | UI 언어 전환 |
| `/export [path]` | session 을 Markdown 으로 export |
| `/exit` | 종료 |

전체 command 표는 [영문 README](../../README.md#slash-commands)를 참고하세요.

## Project instructions, MCP, Skills

Zode 는 계층별로 instructions 를 읽습니다: 전역 `~/.zode/`, project root, 현재
작업 디렉터리. 각 계층에서는 `AGENTS.md` 를 우선하고 없으면 `CLAUDE.md` 로
돌아갑니다. Skills 는 `.zode/skills/**/SKILL.md`, MCP server 는
`~/.zode/mcp.json`, `.mcp.json`, `.zode/mcp.json`, hooks 는 `~/.zode/hooks.json`
또는 `.zode/hooks.json` 에 둡니다.

Zode 는 Claude Code, Codex, opencode, Cursor, Gemini 등 다른 agent 의 직접
skills 디렉터리와 MCP 설정을 읽지만, 그런 제품이 설치한 plugin tree 나 plugin
cache 는 스캔하지 않습니다. 플러그인을 재사용하려면 `zode plugin install ... --trust`
로 명시적으로 설치하세요. Zode 로 설치할 때에도 Codex 와 Claude Code 의 플러그인
패키지 형식은 계속 호환됩니다.

## ZSeven-W 생태계

Zode 는 ZSeven-W 의 AI-native development tools stack 중 하나입니다.

| Product | 설명 |
|---------|------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async LLM agent runtime 으로 multi-provider streaming, tool dispatch, permissions, MCP, cost tracking, attachments, sessions, optional coding tools 를 제공합니다. |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework 입니다. `.op` file 을 app 으로 취급해 OpenPencil-style design artifacts 를 runnable software 로 연결합니다. |
| [`noema`](https://github.com/ZSeven-W/noema) | Coding agents 를 위한 local-first non-vector memory system 으로 lexical recall, review queues, MCP, S3 offload, enterprise policy controls 를 포함합니다. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool 입니다. design-as-code workflow 에서 prompts 를 live canvas 의 UI 로 바꾸고 concurrent agent teams 를 지원합니다. |

## 벤치마크

Zode benchmark 는 one-shot code generation, agentic read/run/edit/fix, multi-file
task, tricky bugs, MCP/Skills/constraint following, Noema LOCOMO runner 를
포함합니다. 방법, 재현 명령, 전체 결과 표는 [영문 README 의 Benchmark](../../README.md#benchmark)를 참고하세요. suite 는 [`benchmarks/`](../../benchmarks/) 에 있습니다.

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

기여를 환영합니다. [Conventional Commits](https://www.conventionalcommits.org/) 의
`<type>(<scope>): <subject>` 형식을 사용해 주세요. 자주 쓰는 scope 는 `core`,
`tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs` 입니다.

## License

[MIT](../../LICENSE) &copy; ZSeven-W
