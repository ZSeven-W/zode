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
  <strong>ターミナル向けのオープンソース AI ネイティブ coding assistant。</strong><br/>
  コードを読み、コマンドを実行し、ファイルを検索し、git を扱います。すべて高速な Rust TUI から操作できます。
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

> これはローカライズ版 README です。製品概要とクイックスタートをまとめています。完全なベンチマーク詳細と最新の長文説明は [英語 README](../../README.md) を正とします。

## ハイライト

- **複数プロバイダー**：Anthropic、OpenAI、OpenAI 互換 API（DeepSeek、Moonshot、OpenRouter など）、ローカル Ollama に対応。
- **豊富なツール**：ファイルの読み書きと編集、コード/コンテンツ検索、foreground/background shell、git、web fetch、notebook、TODO tracking。
- **ブラウザー制御**：組み込みの `browser_*` ツールで managed Chromium、または Chrome bridge 拡張経由で既存 Chrome を操作。
- **ノンブロッキング権限**：変更を伴うツールは allow once / always / deny で確認され、プロンプトは入力を妨げません。
- **OS サンドボックスが標準有効**：shell コマンドは macOS `sandbox-exec` または Linux `bwrap` で実行され、デフォルトで outbound network を拒否。
- **フルスクリーン TUI**：streaming Markdown、syntax highlight、diff preview、slash command autocomplete、履歴、11 個の組み込みテーマ、設定/ヘルプ overlay、15 言語 UI（`/language`）。
- **マルチセッション tabs**：`Ctrl+T` で複数の独立した会話を並行実行し、過去の session も復元可能。
- **サブエージェント、team、workflow**：Task で一回限りの作業を委任し、内部または外部 CLI の teammate を手動登録して `/agents`、`/team`、`/workflows` で管理。
- **Skills / MCP / hooks**：必要に応じて `SKILL.md` を読み込み、MCP server を接続し、tool event に外部 script を実行。

## インストール

### ワンライン

**macOS / Linux：**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell)：**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

installer は OS と CPU を自動検出し、最新 [release](https://github.com/ZSeven-W/zode/releases) から対応する binary を取得して `zode` を `PATH` に置きます。

### 手動ダウンロード

[releases page](https://github.com/ZSeven-W/zode/releases) から対象 platform の archive を取得してください。

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

展開後、`zode` を `PATH` 上に移動します（例：`sudo mv zode /usr/local/bin/`）。Linux build は glibc、macOS binary は未署名です。Gatekeeper が警告する場合は `xattr -dr com.apple.quarantine ./zode` を実行してください。

### ソースから build

最近の stable Rust toolchain が必要です。

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

binary は `target/release/zode` に出力されます。agent runtime は `vendor/agent` git submodule なので、`--recurse-submodules` で clone するか `git submodule update --init` を実行してください。

## クイックスタート

最も簡単なのは `zode` を起動して **`/connect`** を実行する方法です。interactive な model picker が設定を書き込みます。

手動で `~/.zode/config.json` を書くこともできます。

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

よく使う起動方法：

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

## 外部 CLI teammate の手動登録

Zode は第三者の agent CLI を一回限りの Task worker、または会話を継続する
teammate として利用できます。登録は明示的です。実行ファイルが `PATH` に
あっても自動登録されず、`externalAgents.agents` への追加が必要です。
`/external-agents` で `PATH` 上の対応 CLI を確認し、
`/external-agents discover` で検出済みプリセットを明示的にグローバル設定へ登録できます。起動時の自動スキャンや登録は行いません。

| Profile | Command | Task | Team mode | 外部 CLI sandbox |
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

### Profile を追加する

全 project 共通なら `~/.zode/config.json`、project 単位なら
`.zode/config.json` に設定します。既知の profile は空 object で手動有効化
でき、`command` は `PATH` 上の名前または path を指定できます。

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

公開する profile だけを追加してください。custom profile の
`promptTransport` は `stdin`、`argv`、`file`、`output` は `text`、
汎用 `jsonl`、`jsonl-claude`、`jsonl-codex` に対応します。汎用 JSONL は
RFC 6901 の `textSource` と `sessionIdSource` で text と session ID を抽出し、
`resumeArgs` には単独の `{session_id}` token が必要です。resume 非対応の CLI
は送信ごとに新しい process を使う stateless teammate になり、一回限りの
Task worker としても利用できます。
`newSessionArgs` に単独の `{session_id}` を指定すると、Zode が初回 run の
ID を生成し、以降の assignment では `resumeArgs` を使用します。

外部 process には基本的に `PATH`、`HOME`、`TERM` だけが渡されるため、
API key は `envAllow` または `authEnv` に追加します。初回 hire では command、
cwd、sandbox を表示して trust を求めます。Zode が gate するのは process 起動
だけで、外部 CLI の各 file edit や shell command ではありません。
`--yolo` など非対話 mode では明示的な `trusted: true` が必要です。

### Team で使う

`team_hire` と `team_send` は model-facing tool です。leader に通常の文で
依頼します。

```text
`codex` を `implementer` という teammate として hire し、認証 refactor と test を担当させてください。
編集前に `src/auth/` を claim してから task を送ってください。
```

`/team` と `/team board` で状態を確認し、`/team dismiss implementer` で
削除します。team state は `<cwd>/.zode/team/` に保存されますが、外部 CLI の
trust grant は Zode process をまたいで保存されません。

## 設定の要点

`providers` が model provider の source of truth で、top-level `provider` が active model を示します。OpenAI-compatible provider では通常 `baseUrl` と `dialect` を設定します。

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
  "language": "ja"
}
```

1 つの provider に複数 model を持たせ、`/model` で live switch できます。`language` は `/language` でも変更できます。

## Server mode と SDK

`zode server` は stdin/stdout 上で newline-delimited JSON-RPC server を起動します。editor integration、local automation、test、SDK client 向けです。

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDK は [`sdk/`](../../sdk/) にあります。

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## ブラウザー制御

Zode は `tools:browser` group を提供し、screenshot/DOM/log の読み取り、navigation/click/type、JavaScript 実行、tab 管理に対応します。target は managed Chromium または [`extensions/chrome/`](../../extensions/chrome/) の MV3 extension 経由の既存 Chrome です。

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

## よく使う slash command

| Command | 内容 |
|---|---|
| `/help` | command と keybinding |
| `/connect` | provider を接続して切り替え |
| `/model [id]` | active model の表示/設定 |
| `/theme [id]` | テーマを切り替え（`catppuccin-mocha`、`aurora-forge`、`ember-atelier`、`sakura-paper`、`arctic-day`、`lavender-mist`、`citrus-grove`、`verdant-signal`、`cyberpunk`、`minimal`、`hacker`） |
| `/sessions`, `/resume` | session 復元 |
| `/browser ...` | browser control |
| `/tasks` | background tasks |
| `/mcp` | MCP server 管理 |
| `/skills` | skills 一覧 |
| `/agents` | sub-agent 管理 |
| `/external-agents [list\|discover]` | `PATH` 上の対応外部 CLI を表示、または検出済みプリセットを明示的に登録 |
| `/team [status\|board\|dismiss <name>]` | 永続 teammate の roster と共有 board を確認、または teammate を削除 |
| `/workflows` | workflow 管理 |
| `/sandbox ...` | OS sandbox 制御 |
| `/language` | UI 言語切り替え |
| `/export [path]` | Markdown へ export |
| `/exit` | 終了 |

完全な command 表は [英語 README](../../README.md#slash-commands) を参照してください。

## Project instructions、MCP、Skills

Zode は global `~/.zode/`、project root、current directory の 3 階層から instruction を読みます。各 directory では `AGENTS.md` を優先し、なければ `CLAUDE.md` を使います。Skills は `.zode/skills/**/SKILL.md`、MCP server は `~/.zode/mcp.json`、`.mcp.json`、`.zode/mcp.json` に置けます。

Claude、Codex、opencode、Cursor など他 agent の skills、commands、MCP configuration も発見できます。project 内の外部 MCP は安全のため default disabled です。

## ZSeven-W エコシステム

Zode は ZSeven-W の AI-native development tools stack の一部です。

| Product | 概要 |
|---------|------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Pure-Rust async LLM agent runtime。multi-provider streaming、tool dispatch、permissions、MCP、cost tracking、attachments、sessions、optional coding tools を提供します。 |
| [`jian`](https://github.com/ZSeven-W/jian) | Rust-native cross-platform UI framework。`.op` file を app として扱い、OpenPencil-style design artifacts を runnable software へつなげます。 |
| [`noema`](https://github.com/ZSeven-W/noema) | Coding agents 向けの local-first non-vector memory system。lexical recall、review queues、MCP、S3 offload、enterprise policy controls を備えます。 |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Open-source AI-native vector design tool。design-as-code workflow で prompts を live canvas 上の UI に変換し、concurrent agent teams をサポートします。 |

## ベンチマーク

Zode の benchmark は one-shot code generation、agentic read/run/edit/fix、多ファイル task、tricky bug、MCP/Skills/constraint following、Noema LOCOMO runner を含みます。方法、再現手順、詳細結果は [英語 README の Benchmark](../../README.md#benchmark) を参照してください。

## 開発

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## コントリビューション

Contributions are welcome. [Conventional Commits](https://www.conventionalcommits.org/) の `<type>(<scope>): <subject>` 形式を使ってください。

## License

[MIT](../../LICENSE) &copy; ZSeven-W
