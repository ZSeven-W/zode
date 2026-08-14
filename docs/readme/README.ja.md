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

- **複数プロバイダー** — Anthropic、OpenAI、OpenAI 互換 API（DeepSeek、Moonshot、OpenRouter 方言）、およびローカル Ollama。大容量出力と **1M コンテキスト**モデルにも対応（`contextWindow` / `maxOutputTokens` は設定可能）。
- **豊富なツール** — ファイルの読み書きと編集（アトミックなマルチハンク `MultiEdit` を含む）、コード/コンテンツ検索、foreground/background shell、git、web fetch（Tavily key で任意の `WebSearch` も有効化可能）、notebook、TODO tracking。
- **ブラウザー制御** — 組み込みの `browser_*` ツールで、managed Chromium インスタンスや zode Chrome bridge 拡張経由の既存 Chrome プロファイルを操作：navigate、click/type、DOM 検査、screenshot 取得、console/network log 読み取り、zode が開いた tab のグループ化。ペアリングは一度きりで、拡張は zode の再起動をまたいで自動的に再接続します。
- **ノンブロッキング権限** — 変更を伴うツールはすべて allow once / always / deny で確認されますが、プロンプトはインラインに表示され作業を妨げません。ツールが待機中でも入力を続けて次の指示を queue でき、hard-deny ルールも併用できます。
- **OS サンドボックスが標準有効** — shell コマンドは sandbox-exec（macOS）/ bwrap（Linux）の `read-only` または `workspace-write` モードで実行され、**outbound network はデフォルトで拒否**。`/sandbox` でライブ切り替え可能。モデルは 1 コマンド単位で脱出を要求でき（`dangerouslyDisableSandbox`）、それを**承認するのはあなた**です。
- **フルスクリーン TUI** — syntax highlight 付きの streaming Markdown、diff preview、slash command autocomplete、プロンプト履歴（Up/Down）、11 個の組み込みテーマ、設定/ヘルプ overlay、レジリエントな右サイドバーセクション、**15 言語 UI**（`/language`）。
- **永続的で V1 互換の session** — 既存の `<id>.jsonl` transcript 契約を維持しつつ、journal、checkpoint、rewind、fork、独立した Git worktree を sidecar データとして追加。コンテキスト圧縮で可視の会話が失われることはありません — 復元した session は圧縮前の履歴を完全に再生し、モデルのコンテキストはコンパクトなまま保たれます。
- **自動化インターフェイス** — 安定した JSON/JSONL headless 出力、正確な session ターゲティング、tool filter、決定的な exit code、stdio 上の ACP、ローカル operations dashboard。
- **マルチセッション tabs** — 複数の会話を並行実行（`Ctrl+T`）。各 tab は独立した agent で、過去の session を完全な履歴再生付きで復元できます。
- **サブエージェント、team、workflow** — Task tool で一回限りの作業を委任し、永続的な内部/外部 CLI の teammate を hire、共有 board と file claim で連携させ、`/agents`、`/team`、`/workflows` で管理。
- **移植可能なローカル設定** — Claude Code、Codex、Cursor、opencode、Gemini から直接の skills と MCP 設定を読み込みますが、それらのインストール済み plugin tree や cache は取り込みません。
- **Skills / MCP** — 必要に応じて `SKILL.md` の instruction pack を読み込み、MCP server を接続（`mcp__<server>__<tool>`）。作成した agent、skill、MCP tool は slash command として現れます。
- **Hooks** — tool event で外部 script を実行（例：危険なコマンドをブロック、編集後に lint）。
- **3 階層の instruction** — global（`~/.zode/`）→ project root → cwd（`AGENTS.md` / `CLAUDE.md`）。

## インストール

### ワンライン（ビルド済みバイナリ）

**macOS / Linux：**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell)：**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

installer は OS と CPU を自動検出し、最新 [release](https://github.com/ZSeven-W/zode/releases) から対応する binary を取得して `zode` を `PATH` に置きます。バージョンを固定したり場所を変えるには：

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

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

展開後、`zode` を `PATH` 上に移動します（`sudo mv zode /usr/local/bin/`）。Linux build は glibc、macOS binary は未署名です（Gatekeeper が警告する場合は `xattr -dr com.apple.quarantine ./zode`）。

### ソースから build

Rust 1.88 以降が必要です。

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binary は target/release/zode
```

> agent runtime は `vendor/agent` git submodule にあります。必ず `--recurse-submodules` で clone するか、`git submodule update --init` を実行してください。

## クイックスタート

最も簡単なのは `zode` を起動して **`/connect`** を実行する方法です。models.dev をバックエンドとする interactive な picker が設定を書き込んでくれます。

`~/.zode/config.json` を手動で書く場合、**`providers`** が source of truth です。プロバイダーごとに 1 エントリ（共有 credential）を置き、その中に 1 つ以上の **models** を持たせます。top-level の **`provider`** が*アクティブ*なモデルを記録します。

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // wire protocol: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // the active model
}
```

OpenAI 互換プロバイダー（DeepSeek、Moonshot、OpenRouter など）は `baseUrl` と `dialect` を追加し、モデルごとの設定は各モデルのエントリに置きます。

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

1 つの provider エントリに複数の model を持たせ、`/model` でライブに切り替えられます。

続いて起動します。

```bash
zode                       # フルスクリーン TUI
zode -p "explain main.rs"  # headless：1 プロンプトを stdout にストリームして終了
zode --no-tui              # 素の readline REPL
zode -c                    # 直近の session を継続
zode -r <id>               # id prefix で session を復元
zode --yolo                # 承認プロンプトを bypass（deny ルールは有効）
zode --no-sandbox          # OS サンドボックスを無効化（デフォルトは ON）
zode --sandbox-read-only   # 読み取り専用サンドボックス（全書き込みを拒否）
zode --sandbox-allow-network  # サンドボックス内の outbound network を許可
zode --browser             # この実行で組み込みブラウザーツールを強制有効化
zode --no-browser          # この実行で組み込みブラウザーツールを無効化
zode --model <id>          # モデルを上書き
zode --provider <name>     # config.providers から named provider を選択
zode server                # stdio 上の JSON-RPC app-server モード
zode acp                   # stdio 上の Agent Client Protocol agent
zode dashboard             # ローカルの sessions/checkpoints/worktrees 概要
```

設定を編集せずに、対応する key（`ANTHROPIC_API_KEY`、`OPENAI_API_KEY` など）を export するだけでも任意のプロバイダーを指定できます。Ollama では未設定時に `baseUrl` を環境から取得します。

## 外部 CLI teammate の手動登録

Zode はインストール済みの第三者 agent CLI を一回限りの Task worker、または永続的/無状態の teammate として利用できます。登録は意図的に手動です。CLI をインストールしたり `PATH` に置いたりしても、モデルには**公開されません**。`externalAgents.agents` に profile を追加してから、その project で Zode を起動してください。あるいは `/external-agents` で `PATH` 上の対応 CLI を確認し、`/external-agents discover` で検出済みプリセットをすべて明示的にグローバル設定へ追加できます。このコマンドはユーザー起点で、起動時に外部 CLI を自動でスキャン/登録することはありません。

| Agent profile | Executable | Task worker | Team mode | 外部 CLI sandbox |
|---|---|---:|---:|---|
| `claude-code` | `claude` | yes | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | yes | persistent | workspace-write |
| `opencode` | `opencode` | yes | stateless | unknown |
| `cline` | `cline` | yes | stateless | unrestricted |
| `antigravity` | `agy` | yes | stateless | unknown |
| `cursor` | `cursor-agent` | yes | persistent | unrestricted |
| `kiro` | `kiro-cli` | yes | stateless | unrestricted |
| `pi` | `pi` | yes | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | yes | persistent | unrestricted |

登録済みの profile はすべて team に参加できます。resume 可能な profile は CLI の session ID と会話を assignment 間で保持し、それ以外の CLI は assignment ごとに新しい process を起動する無状態 teammate になります。プリセットは [Cline](https://docs.cline.bot/usage/cli-overview)、[Antigravity](https://antigravity.google/docs/cli-best-practices)、[Cursor](https://cursor.com/docs/cli/headless)、[Kiro](https://kiro.dev/docs/cli/headless/)、[Pi](https://pi.dev/docs/latest)、xAI の [Grok Build](https://docs.x.ai/build/cli/headless-scripting) の公式 headless インターフェイスを使用します。代替の Grok CLI を含むその他のツールは custom profile で利用できます。

### CLI profile を手動で追加する

全 project 共通なら `~/.zode/config.json`、単一 project なら `<project>/.zode/config.json` に `externalAgents` を置きます。空 object は既知のプリセットを明示的に有効化し、その executable を sanitize された `PATH` 上で解決します。

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

公開する profile だけを追加してください。`cline` のような裸の `command` は `PATH` 上で解決され、`./tools/my-agent` や `/opt/agents/my-agent` のような path も受け付けます。既知のプリセットは `enabled`、`command`、`extraArgs`、`envAllow`、`trusted` を尊重し、`extraArgs` は Zode のプリセット呼び出しに追加されます。

CLI process は `PATH`、`HOME`、`TERM`（および Windows で必須の変数）だけを含むクリアされた環境で起動するため、API key など必要な変数は `envAllow` に明示的に追加してください。`HOME` 配下の既存ログイン状態はそのまま機能します。同じ profile 名の project エントリはグローバルエントリを丸ごと置き換えるので、project で必要な override はすべて再記述してください。

custom profile は呼び出しと protocol を完全に宣言します。

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

`promptTransport` は `stdin`、`argv`、`file`。`argv` は独立した `{prompt}` 引数、`file` は `{prompt_file}` を必要とします。`output` は `text`、汎用 `jsonl`、`jsonl-claude`、`jsonl-codex`。汎用 JSONL profile は RFC 6901 の `textSource` / `sessionIdSource` pointer で任意の event から streamed text と resume 可能な session ID を抽出します。`resumeArgs` は独立した `{session_id}` token を含む必要があり、以降の turn で追加されます。`resumeFlag` は `<flag> <session-id>` 形式の shorthand として残されています。

CLI が呼び出し側指定の session ID を受け付ける場合、`newSessionArgs` に独立した `{session_id}` token を置けます。Zode は UUID を生成し、初回 run で展開済み引数を追加し、以降の assignment では `resumeArgs` を使います。これにより、出力から ID を parse しない plain-text CLI も resume 可能になります。

これにより任意の headless CLI を Task worker または無状態 teammate に変えられます。team assignment 間で会話 context を保持するには、加えて session ID を公開するか `newSessionArgs` で受け取り、非対話の resume 呼び出しを提供する必要があります。

`effectiveSandbox` は `none`、`readOnly`、`workspaceWrite`、`unrestricted`、`unknown` を受け付け、trust プロンプトに表示されます。

### teammate を hire して協働する

`team_hire` と `team_send` は model-facing tool であって slash command ではありません。leader に通常の文で依頼します。

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

初回 hire では解決された executable と引数、作業ディレクトリ、CLI の effective sandbox を表示します。承認すると、その process に現在の project で作業を委任します。Zode は process 起動を gate しますが、外部 CLI が実行する各 file edit や shell command は gate**しません**。trust grant は現在の Zode session の間だけ有効です。永続 roster は `<cwd>/.zode/team/` から復元されますが、外部 teammate は再起動や executable の変更後は再度 trust が必要です。

非対話/bypass 実行（`--yolo` を含む）では Zode は trust プロンプトを表示できず、fail closed します。その profile をプロンプトなしで実行させたいと意図的に判断したときだけ `externalAgents.agents.<profile>.trusted` を `true` に設定してください。

hire 後は `/team` で roster と board を確認します。

```text
/team                         # roster + board panel
/team status                  # text roster
/team board                   # 共有ゴール、メモ、割り当て、claim
/team dismiss implementer     # teammate を削除
```

## 自動化、永続 session、operations

### 構造化 headless 実行

`-p`、`--prompt-file`、`--prompt-json` はすべて同じ headless エンジンを使います。`json` は最終結果 object を 1 つ出力し、`stream-json` は 1 行につき 1 つの `zode.run-event.v1` JSON object を出力します。構造化モードは stdout を機械可読出力に予約し、安定した exit code を使います：`0` 成功、`10` provider エラー、`11` 権限拒否、`12` turn/limit 到達、`13` 中断（Ctrl-C）、`14` 部分結果、`15` session ターゲティングエラー。

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# 正確な ID は prefix 一致しません。fork は元 session を変更しません。
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

tool の deny パターンは allow パターンより優先され、Task サブエージェントに継承されます。`--permission-mode` は `default`、`dont-ask`、`accept-edits`、`bypass` を受け付けます。`--yolo` は bypass の shortcut のままですが、hard deny ルールは常に有効です。

### V1 互換の session、checkpoint、worktree

transcript は元の V1 ファイル `~/.zode/sessions/<id>.jsonl` のままです。これは transcript の**唯一の**コピーなので、旧 Zode client も引き続き読み書きできます。新しい metadata は追加的で `~/.zode/sessions/<id>/`（`meta.json`、journal、checkpoint、snapshot）に置かれます。新しい session フォーマットや transcript の移行は不要です。

```bash
zode session list
zode session list --json
zode session show <id>                         # metadata + checkpoint ID
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # conflict-aware プレビュー
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

checkpoint は変更を伴う turn の前に取得されます。rewind は追跡済みファイル内容と transcript prefix を復元し、より新しい変更を上書きせず conflict を報告し、履歴を削除する代わりに新しい論理 journal branch を記録します。worktree fork は実験の準備ができたら明示的に apply back できます。

**圧縮で可視の会話は失われません。** コンテキスト圧縮が古いメッセージを要約に置き換えるとき、原文は追加的な sidecar（`~/.zode/sessions/<id>/compacted.jsonl`）に保存されます。session の復元、`Ctrl+L`、`/export`、Chrome side panel はいずれも圧縮前の完全な履歴を表示し、モデルは圧縮後のコンテキストだけを受け取り続けます。fork はアーカイブを（自身の transcript にフィルタして）引き継ぎ、`/clear` はアーカイブを削除し、session を削除すると sidecar 全体が削除されます。

### 権限ルールとサンドボックス profile

ルールは `config.json` の `permissions.rules`、または `--rules` で渡す独立した JSON ファイルに置けます。field matcher は RFC 6901 JSON pointer を使い、優先順位は deny > ask > allow です。独立ファイルはルール配列または `{ "rules": [...] }` のいずれかで、top-level の `permissions` object で包みません。

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

組み込み profile は `read-only`、`workspace`、`workspace-network`、`unconfined` です。config で定義した profile は上に示したのと同じ sandbox field を使います。

### プラグインと静的 marketplace

管理された plugin は skills、commands、agents、hooks、MCP server、LSP server、サンドボックス化された JavaScript UI renderer を提供できます。Zode は `plugin.json`、`.zode-plugin/plugin.json`、`.codex-plugin/plugin.json`、`.grok-plugin/plugin.json`、`.claude-plugin/plugin.json` を受け付けます。Codex と Claude Code の component path 配列に対応し、初回インストール時に Claude Code の `defaultEnabled` を尊重します。Codex apps/connectors や Claude Code の themes、monitors、output styles などの host 専用 component は無視され、app 専用 plugin は Zode 互換の component を持たないため拒否されます。インストールは provenance と SHA-256 tree hash を伴う不変の snapshot です。実行可能な plugin コンテンツは、明示的な `--trust` フラグなしには決して有効化されません。

#### JavaScript UI plugin クイックスタート

最小の UI plugin は manifest と JavaScript ファイル 1 つで構成されます。

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

ローカルディレクトリ、または GitHub リポジトリ/サブディレクトリをインストールし、新しい snapshot を読み込ませるために実行中の Zode process を再起動します。

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

ソースを変更したら `zode plugin update my-plugin` を使います。JavaScript、hooks、MCP server、宣言された network アクセスは実行可能な capability なので `--trust` が必須です。install と update は plugin が宣言した権限（network host、env 変数、context scope）を表示します。manifest がインストール済み snapshot より**広い**権限を要求する update は、`--trust` を付けて再実行しない限り拒否されます。動く Git ソースが自分の権限を静かに拡大することはできません。

#### UI render API

UI plugin はサイドバーのバージョン表示のすぐ上に宣言的な行を追加できます。合計最大 6 行で、load 順にすべての plugin で共有されます。manifest に JavaScript entrypoint を宣言します。

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

`zode.ui.sidebar` で同期 renderer を登録します。context は terminal、session、model、status、token、context-window の各 field を含む read-only な JSON snapshot です。結果は Zode がレンダリングし、script には filesystem、network、terminal、Ratatui bridge は渡されません。

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

対応する tone は `default`、`muted`、`accent`、`success`、`warning`、`danger`。span は `bold` と `italic` も受け付けます。renderer は同期でなければなりません。各 script は 256 KiB、JS メモリ 8 MiB、1 回の評価あたり 25 ms に制限され、renderer は最速でも 250 ms ごとに再評価されます（評価間はキャッシュ出力を再利用）。sidebar 出力は renderer あたり 6 行（全 plugin 合計でも 6 行）、各行 16 span、テキスト 2,048 バイトに制限されます。制御文字は host が sanitize します。

status bar も拡張できます。どの plugin もコンテンツを返さない間は 1 行のままで、同期 `zode.ui.statusLine` renderer が span を返すと動的に 2 行に増えます。Zode はコアの status と安全指標を 1 行目に保持し、plugin 出力は 2 行目に構成されます。

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### render context と権限

各 renderer は追加の context 権限を要求せずに以下の基本 field を受け取ります。

| Field | 構造と意味 |
| --- | --- |
| `ctx.apiVersion` | Context API バージョン。現在は `1`。 |
| `ctx.app` | `{ version, effort }`。 |
| `ctx.terminal` | `{ width, height }`（terminal cell 単位）。 |
| `ctx.session` | アクティブ task の `{ id, title, cwd, busy }`。 |
| `ctx.model` | `{ id, provider }`。 |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`；`sandbox` は `{ enabled, readOnly, network }`。 |
| `ctx.tokens` | `{ input, output }` token カウンター。 |
| `ctx.context` | `{ used, window, usedPercent }`；percentage は `null` になり得ます。 |
| `ctx.data` | この plugin 自身が登録した data source の結果のみ。 |

より詳細なセクションは、plugin が `permissions.context` で対応する scope を要求した場合のみ現れます。

| Scope | 露出する field | 構造と制限 |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`；`active` は 1 始まり。 |
| `workspace` | `ctx.workspace.modifiedFiles` | 最大 50 件の `{ path, added, removed }` Git エントリ。 |
| `tools` | `ctx.tools.available` | アクティブ task で有効なツール名（ソート済み）。 |
| `tools` | `ctx.tools.active` | 現在実行中のツール名。 |
| `tools` | `ctx.tools.recent` | 最大 20 件の `{ name, status, durationMs }`。 |
| `tasks` | `ctx.tasks.todoStatuses` | Todo status 文字列のみ（Todo 本文なし）。 |
| `tasks` | `ctx.tasks.subagents` | `{ type, status }`（prompt や transcript なし）。 |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`（goal 本文なし）。 |
| `services` | `ctx.services.mcp` | `{ name, connected }`。 |
| `services` | `ctx.services.lsp` | `{ language, running }`。 |

例：

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` は観察 API です。どのツールが存在し、どれが実行中/実行済みかを renderer に伝えます。UI plugin はツールを呼び出せません。ツール入力/出力、prompt、transcript コンテンツ、todo/goal 本文、環境変数値、credential は含まれず、この API が Zode の承認システムを回避することもできません。

#### バックグラウンド HTTP データ

UI plugin はバックグラウンド HTTP data source も登録できます。network と secret アクセスは manifest で宣言する必要があります。

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

リクエストは宣言的で、render path の外で実行されます。secret 環境変数は Zode が header に組み立て、JavaScript には決して露出されません。

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

`zode.data.define(key, config)` は 1〜64 文字の英数字・アンダースコア・ハイフンからなる key を受け付けます。`request` は `url`、`method`、`headers`、任意の JSON `body`、`timeoutMs` に対応します。デフォルトは `GET`、3 秒 timeout、60 秒 refresh。HTTPS の `GET` と `POST` のみ受け付けます。リテラル header は文字列、secret header は `{ "env": "NAME", "prefix": "Bearer " }` を使います。環境変数は `permissions.env` にも記載する必要があり、リクエスト組み立て時に Rust だけが読み取り、JavaScript には返されません。

Zode は redirect と proxy を無効化し、公開 DNS アドレスを検証・固定し、localhost/private network を拒否し、レスポンスを 256 KiB に制限し、リクエスト timeout を 500 ms〜10 秒、refresh 間隔を 10 秒〜1 時間に clamp します。`*.example.com` のような wildcard はサブドメインに一致しますが、裸の `example.com` host には一致しません。

各 plugin は自分のデータのみを見ます。`ctx.data.<key>` は `{ ok, status, data, updatedAt }`、または `{ ok: false, error, updatedAt }` を含みます。JSON レスポンスは object/array に、非 JSON レスポンスは文字列になります。HTTP エラー status でも `status` と `data` を含み、`ok: false` になります。

private な quota や coding-plan API を使う場合は、必要な secret を環境に入れて Zode を起動します。

```bash
CODING_PLAN_TOKEN=... zode
```

[完全に動作する例](../../examples/plugins/zode-ui-demo/)は、sidebar と status line に model/context/tool のアクティビティを表示し、public な GitHub API quota に `zode.data.define` を使います。

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# marketplace はローカル/Git の静的インデックスであり、Zode がホストするサービスではありません。
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # 必要なら曖昧さを解消
zode plugin marketplace update
```

### ACP、dashboard、telemetry、TUI regression test

`zode acp` は stdio 上で ACP の initialize/new/load/fork/prompt/cancel を実装し、message/thought/tool の更新を stream し、client 経由で権限を要求し、client 提供の stdio/HTTP/SSE MCP server を受け付けます。session データは TUI や headless CLI と同じ V1 互換 store を使います。

```bash
zode acp
zode dashboard
zode dashboard --json
```

OTLP export はデフォルトで OFF で、明示的な opt-in が必要です。export されるのはコンテンツを含まない lifecycle/tool-name/status/usage 属性のみで、prompt、生成テキスト、tool 入力/出力、file path、error メッセージは決して送信されません。

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

実端末での TUI regression シナリオ向けに、workspace には raw diagnostics と virtual-screen snapshot を記録する PTY + VT100 harness が含まれます。

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` は順序付きの wait、key 入力、resize、snapshot で実端末を駆動します（key 記法は `<Enter>`、`<Esc>`、`<Tab>`、`<Up>`、`<Down>`、`<Left>`、`<Right>`、`<Backspace>`、`<C-c>`、`<C-d>`、`<C-l>` に対応）。

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

このローカル/オープンな実装は、xAI 固有のアカウント、課金、Zode 運営のクラウド marketplace サービスを意図的に含みません。

### 主な top-level config キー

いずれも妥当なデフォルトを持ちます。

```jsonc
{
  "maxOutputTokens": 16384,      // turn あたりの出力上限（大きなファイル書き込みでは上げる）
  "contextWindow": 1000000,      // モデルの context window — 1M モデルなら 1000000
  "temperature": 0,              // 低いほど決定的
  "language": "ja",              // UI 言語（15 locale）；/language でも変更
  "effort": "medium",            // reasoning effort；Anthropic では medium/high が実際の thinking budget にマップされます
  "autonomousOrchestration": true, // サブエージェント + workflow orchestration（デフォルト ON）
  "subagentMaxIterations": 0,      // 任意の子ガード；省略/0 = 無制限
  "tools": {
    "deferNonCore": false        // true: 約 20 個の日常ツールを見えるまま残し、残りを ToolSearch の背後に遅延
  },
  "webSearch": {
    "tavilyApiKey": null         // WebSearch ツールを有効化（$TAVILY_API_KEY でも可）
  },
  "sandbox": {
    "enabled": true,             // shell コマンド用 OS サンドボックス（デフォルト ON）
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // サンドボックス内の outbound network を許可
    "writableRoots": []          // 追加の書き込み可能ディレクトリ（workspace-write）
  },
  "browser": {
    "enabled": true,             // browser_* ツールと /browser パネル（デフォルト ON）
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // managed Chromium の起動モード
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // 無人の /loop と /schedule turn を監視
    "inactivityTimeoutSecs": 900, // provider/tool アクティビティなしで 15 分後に abort
    "maxRuntimeSecs": 3600,      // background turn ごとの絶対 1 時間上限
    "abortGraceSecs": 10,        // hard-stop 前に協調的キャンセルを待つ
    "maxRetries": 3,             // 枯渇までの連続 recovery 試行回数
    "initialBackoffSecs": 5,     // 最初の retry 遅延
    "maxBackoffSecs": 300        // 指数 retry backoff の上限
  }
}
```

> サンドボックスは shell コマンドを閉じ込めます（macOS: sandbox-exec、Linux: `bwrap`（要インストール））。設定されたサンドボックスが検証できない場合、起動は fail closed します。サンドボックスなしで実行するには明示的な `--no-sandbox` フラグを使ってください。network はデフォルトで拒否されます。コマンドが本当に脱出を必要とする場合、モデルが `dangerouslyDisableSandbox: true` を設定し、承認プロンプトで**あなた**が許可します。あるいは `/sandbox` でサンドボックス全体をライブに切り替えます。

> `contextWindow` は auto-compaction を駆動します。モデルの実際の window（例：`1000000`）に設定してください。`providers.<name>.models.<id>.contextWindow` の**モデル単位**の値を優先します（こちらが勝ちます）。上の top-level キーはグローバルな fallback で、どちらも未設定なら zode は同梱の models.dev カタログから補完します。実際の window を**超えて**設定しないでください。過大評価するとリクエストが溢れ、provider が turn を拒否します。

## Server mode と SDK

`zode server` は stdin/stdout 上で newline-delimited JSON-RPC server を起動します。editor 統合、local automation、test、TUI を起動せずに zode の既存機能を使いたい SDK client 向けです。

```bash
zode server                      # stdio（デフォルト）— SDK が spawn するもの
zode server --listen stdio://    # 同じことを明示的に記述
zode server --listen ws://127.0.0.1:0   # loopback WebSocket + Bearer 認証
zode server --listen off         # 何も起動せず終了
```

Server mode は zode-backed な振る舞いを公開します。

- 初期化 + capability discovery（`approvalPolicy` は `readOnly`（デフォルト）/ `auto` / `prompt`）
- thread metadata のライフサイクルと**ストリーミング turn** — モデル出力と tool 呼び出しは JSON-RPC 通知として届き、`turn/interrupt` が turn をキャンセル
- **対話的な承認** — `prompt` ポリシーは server→client の `approval/request` frame を駆動し、`allow` / `allowAlways` / `deny` で応答
- filesystem の read/write/create/stat/list/remove/copy と一回限りの `command/exec`
- model の list/set、config の read/list/write、read-only な skills、hooks、MCP-server の status、plugin リスト

WebSocket transport は loopback のみに bind し、`0600` の `<config-dir>/server.json` credential ファイル（`{port, pid, token}`）を書き込みます。client は `Authorization: Bearer <token>` で認証します。protocol、通知の field 名、言語別の例は [`sdk/README.md`](../../sdk/README.md) を参照してください。

この app-server protocol に限っては、ホスト型 marketplace 管理、remote-control、Realtime、standalone process spawn、background terminal、thread archive/fork、goal、app connector は範囲外です。上で説明したローカル session と静的 plugin marketplace コマンドは別の CLI サーフェスです。

SDK は [`sdk/`](../../sdk/) にあります。

| SDK | Directory | Local test |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

各 SDK は現在の安定 method 名の native な `ProtocolMethod` enum/constant セットを公開するため、統合側は hard-coded な JSON-RPC 文字列を避けられます。対応するすべての method の params、result 形状、SDK enum/constant 名は [`sdk/` method reference](../../sdk/README.md#method-reference) に記載されています。

## ブラウザー制御

Zode はブラウザー自動化のための `tools:browser` group を含みます。agent は `browser_read` で screenshot、DOM snapshot、console log、network log、tab 読み取りを行い、`browser_act` で navigation、click、type、key press、scroll を行い、`browser_eval` で JavaScript を実行し、`browser_tabs` で tab を管理します。読み取り専用のブラウザー検査は gate されません。変更を伴うブラウザー操作は、他の副作用のあるツールと同じ allow-once / always / deny の承認フローを使います。

target は 2 種類です。

- **managed** — zode が専用の Chromium profile を起動して制御します。
- **bridge** — zode が [`extensions/chrome/`](../../extensions/chrome/) に同梱された MV3 拡張を通じて、あなたが既に使っている Chrome profile を制御します。

bridge target では、拡張を `extensions/chrome` から一度 load し、`/browser pair` を実行します。Chrome は外部プログラムが開く `chrome-extension://` URL をブロックするため（ERR_BLOCKED_BY_CLIENT、macOS/Windows/Linux いずれも同様）、zode 自身によるページのオープンは失敗することがあります — 代わりに、`/browser pair` の実行から約 30 秒以内に拡張自身が pairing ページを開きます（port は事前入力済みなので、チャットに表示される 6 桁の pairing code を入力してください）。手動の代替手段として、`chrome-extension://…/popup.html?port=…` の URL をアドレスバーに自分で入力して開くこともできます（手入力によるナビゲーションはブラウザー起点なのでブロックされません）。**ペアリングは一度だけです**：拡張は長期 token を保存して自動的に再接続します — ブラウザー起動時、拡張の更新時、切断中は約 30 秒間隔のリトライで — そのため zode を再起動しても再ペアリングを求められることはありません。実行中の CLI に再接続するか、必要に応じて extension-only の zode daemon を自動起動します。zode が開いた tab は `zode` という名前の Chrome tab group に置かれます。

### Chrome task side panel

更新済みの zode CLI を実行し、`/browser pair` を一度だけ行います。ツールバーアイコンをクリックすると side panel が開きます。以降、CLI process が動いていなければ zode を自動起動します。pairing ページは小さな code/token フローのままで、task はターミナルの focus を変えずに TUI session と共有され続けます。

side-panel turn は bridge のブラウザーツールを panel の隣に表示中のページに bind するため、「このページを分析して」のようなリクエストは新しい tab を開かず既存 tab で `browser_read` を使います。standalone の TUI と CLI のブラウザー自動化は引き続き `zode` tab group の zode 所有 tab を使います。曖昧な side-panel prompt のデフォルト context もアクティブページで、ローカルの project ファイルはユーザーが明示的に尋ねた場合にのみ検査されます。

panel はテキスト送信、model 選択、アクセスモード `readOnly`／`prompt`／`auto` の選択、レスポンスの stream、実行中 turn の Stop ができます。1 つの turn には最大 8 ファイル・合計 20 MiB を添付できます：各 5 MiB までの PNG、JPEG、GIF、WebP 画像に加え、各 1 MiB までの UTF-8 テキスト/コードファイル。PDF、Office、archive、実行ファイル、非 UTF-8 入力は拒否されます。

拡張の更新後は `chrome://extensions` で Reload をクリックします。古い拡張バージョンはブラウザー自動化とは互換ですが、task side panel は持ちません。Windows では、Chrome が既にインストールされている場合、Microsoft Store へのリダイレクトを避けるため、default-browser shell を呼ぶ代わりに zode が拡張 URL のために Chrome を直接特定して起動します。

便利なコマンド：

```bash
/browser                         # ブラウザー制御パネルを開く
/browser status                  # target/running/paired 状態を表示
/browser launch                  # managed ブラウザーを起動
/browser close                   # managed ブラウザーを閉じる
/browser pair                    # Chrome bridge 拡張を pair/再接続
/browser target managed          # zode の managed Chromium を使う
/browser target bridge           # 拡張を使い、次回起動のデフォルトとして保存
/browser screenshot [path]       # ブラウザー screenshot を取得
```

拡張の load、更新、CRX packaging、smoke-test の手順は [`extensions/chrome/README.md`](../../extensions/chrome/README.md) を参照してください。

## デスクトップ制御

Zode はブラウザーだけでなく、OS のアクセシビリティ API を通じてネイティブのデスクトップアプリケーションも駆動できます。agent は `desktop_read` でアクセシビリティツリー（window、element、その ref）を読み、`desktop_act` で element 単位に click、type、scroll、値の設定を行い、`desktop_screenshot` で画面を取得します。読み取り専用の読み取りは gate されません。変更を伴うデスクトップ操作は、他の副作用のあるツールと同じ allow-once / always / deny の承認フローを使います。

backend は platform ごとに選択されます。

- **macOS** — Accessibility（AX）API。
- **Windows** — UI Automation（UIA）。
- **Linux** — AT-SPI。
- **Electron アプリ** — Chrome DevTools Protocol でアタッチ。

**ゴーストカーソルと Esc 急停。** Zode はあなたの実際のマウスを動かしません。macOS では zero-permission の overlay（`zode-overlay`）が*偽の*カーソルを描き、滑らかな Dubins path で各操作の target まで飛ばすので、agent の動作を追えます（入力されたテキストは overlay に表示されません）。デスクトップ自動化が有効な間、グローバルな **Esc** はすべての実行中 turn を中断し overlay を隠します（TUI の Esc と同じ急停パス）。他の platform では可視化なしでデスクトップ操作を実行します。

CJK など US 配列の keycode を持たないテキストは、システムの pasteboard 経由で投入されます（書き込み → paste を合成 → 直前のクリップボードを復元）。これにより、独自のキー処理を持つアプリでも実際の文字を受け取れます。

```bash
/desktop            # デスクトップ target と権限状態を表示
/desktop status     # 同上（明示的）
```

設定は `~/.zode/config.json` の `desktop.*` にあります。

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor`（デフォルト `true`）は macOS の overlay カーソルを描きます。`escCancel`（デフォルト `true`）は自動化中にグローバル Esc 中断を有効にします。`overlayHelperPath`（デフォルト `null`）は `zode-overlay` helper の場所を上書きします — helper がなければ可視化が無効になるだけです。デスクトップ自動化は初回使用時に OS 権限（例：macOS のアクセシビリティ）を求める場合があります。

## バックグラウンドターン watchdog

scheduler 所有の `/loop` と `/schedule` の turn は、in-process の liveness watchdog の下で実行されます。provider、tool、nested-agent のアクティビティが共有の source-side heartbeat を更新し、`maxRuntimeSecs` は絶対上限として残ります。どちらかの timeout で、zode は協調的キャンセルを要求し、`abortGraceSecs` 待ち、それでも drain しなければローカル turn task を hard-stop します。task を止めるだけでは scheduler slot は解放されません。zode は追跡中のすべての provider、tool、hook、subprocess reader、nested-agent worker の quiesce も待ちます。その 2 つ目の境界に 5 秒以内に到達しなければ、tab/store は quarantine され、job は無効化され、その live-attempt lease は worker が実際に exit するまで保持されます。

失敗した試行は `initialBackoffSecs` から `maxBackoffSecs` までの有界な指数 backoff を使います。turn が成功すると連続失敗カウントがクリアされます。`maxRetries` が枯渇すると、zode は loop を止めるか、永続 schedule を無効化します。手動中断、job 削除、明示的な無効化は、mutation が開始していなければ別の retry を作らず、保留中の recovery をキャンセルします。recovery は副作用に対して意図的に保守的です。zode は副作用を観測していない場合にのみ自動 retry します。mutation が既に起きた可能性がある場合（mutation 途中の手動キャンセルを含む）、job を止める/無効化し、人間のレビューを待ちます。作業を意図的に detach するツール（`BashRun` や detached GUI）も、その turn の後は再発を止めます。同じ inactivity 上限が claim-to-start の queue も制限します。busy な tab や turn preflight が所有 occurrence の start を妨げると、それは通常の副作用のない watchdog 失敗となり、cross-process lease を永久に保持する代わりに同じ有界 retry ポリシーに入ります。

quiesce はローカルな保証です。remote MCP server、browser 拡張、desktop actor、その他の外部システムに既に受理された作業は revocation をサポートしないことがあります。そのような呼び出しが中断された場合、zode はその結果を unresolved とマークし、scheduler job を無効化し、再有効化前に外部状態を検証するようあなたに要求します。

設定と per-turn/retry のヘルスは `/watchdog status` で確認します。同じ状態は `/tasks` にも background shell や実行中 turn と並んで表示され、claim された queue の経過時間や terminal-persistence の fence もそこに示されます。

これは現在の zode process 内の scheduler turn 向けの watchdog です。OS の process supervisor ではなく、crash や machine 再起動後に zode を再起動することはできません。process レベルの再起動が必要な場合は platform のサービスマネージャーを使ってください。永続 schedule は per-schedule の OS file lock に裏付けられた active-attempt token を記録します。起動時、競合中の lock は別の zode process が所有しているため放置され、正確な永続 token を持つ free な lock は unclean exit からの orphan なので、zode は静かに replay する代わりに、execution-state-unknown としてその schedule を無効化します。この recovery 契約は process crash をカバーします。突然の電源喪失や hardware 故障をまたぐ storage レベルの durability は主張せず、OS のサービスマネージャーを置き換えるものでもありません。

fire timestamp と active-attempt token は、永続 prompt が tab queue に入る前に atomic に claim されるため、queue された作業は zode process をまたいで既に排他的です。その同じ lease は prompt とともに turn へ移り、最終的な transcript/index 永続化まで保持されます。queue された occurrence の編集、削除、無効化は明示的なキャンセルで、対応する active token のみをクリアします。graceful なアプリ終了は、代わりに未 start の fire watermark または retry token を正確に復元するので、実行されなかった作業を消費できません。失敗した terminal roster write は lease を retrying finalizer に保持し、競合する token は解放前に review のため durably に無効化されます。scheduler turn は detached な post-turn memory 抽出をスキップし、graceful exit は tab を破棄する前に worker の quiesce と terminal 永続化を drain します。再発 phase は canonical です。interval slot は永続 anchor からの絶対 epoch 演算（DST fallback をまたぐ場合も含む）を使い、calendar schedule は wall-clock phase を保ち、逃した backlog は最新の due slot に coalesce します。実行中の process も roster を refresh するので、remote での disable/remove、retry、orphan の所有権変更が再起動なしで反映されます。

## スラッシュコマンド

| コマンド | 内容 |
|---|---|
| `/help` | コマンド + keybinding の overlay |
| `/clear` | 会話（と context）をクリア |
| `/model [id]` | active model の表示/記録 |
| `/config` | model と作業ディレクトリを表示 |
| `/compact` | context の auto-compaction 状態 |
| `/cost` | これまでの token 使用量とコスト（サブエージェント込み） |
| `/theme [id]` | テーマ切り替え（`catppuccin-mocha`、`aurora-forge`、`ember-atelier`、`sakura-paper`、`arctic-day`、`lavender-mist`、`citrus-grove`、`verdant-signal`、`cyberpunk`、`minimal`、`hacker`） |
| `/sessions`, `/resume` | session picker — 履歴付きで新しい tab に復元 |
| `/connect` | active provider を接続して切り替え |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | 右サイドバーの表示/非表示；MCP / modified-files / todo セクションの折りたたみ（▼ ヘッダーのクリックでも可） |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | ブラウザー制御パネルとコマンド；Chrome bridge 拡張を pair、または managed Chromium と自分の Chrome profile を切り替え |
| `/loop <interval> [--max N] <prompt>` | 現在の tab で再帰的な prompt を実行；`list` / `stop [id]` |
| `/schedule add <when> <prompt>` | scheduled prompt を永続化；`list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | background-turn watchdog の設定、ヘルス、保留 retry を表示 |
| `/tasks` | background shell、実行中 turn、watchdog ヘルスパネル |
| `/undo`, `/redo` | 直近のファイル編集を undo / redo |
| `/mcp` | MCP server を管理 — ダイアログで enable / disable |
| `/skills` | 利用可能な skill を一覧 |
| `/agents` | サブエージェントを管理 — 作成（AI 支援または手動）/ 削除 |
| `/external-agents [list\|discover]` | `PATH` 上の対応外部 CLI を一覧、または検出済みプリセットを明示的に登録 |
| `/team [status\|board\|dismiss <name>]` | 永続 teammate の roster と共有 board を確認、または teammate を削除 |
| `/workflows` | JS script の workflow を管理・実行（`agent()`/`parallel()`/`pipeline()` orchestration を zode が決定的に実行） |
| `/effort` | reasoning effort レベルを選択 |
| `/thinking`, `/tool-details` | reasoning / tool-call 詳細の表示を切り替え |
| `/orchestration` | 自律的なサブエージェント + workflow orchestration を切り替え |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | OS サンドボックスを実行時に表示/制御 |
| `/language` | UI 言語を切り替え（15 locale） |
| `/export [path]` | transcript を Markdown に export（ディレクトリ指定でデフォルト名） |
| `/yolo` | 承認 bypass モード |
| `/exit` | 終了 |

作成した agent と skill、接続済みの MCP tool も動的な slash command（例：`/<name>`）として現れ、直接呼び出せます。

## キーバインド

> macOS では以下のアプリ chord に **`Cmd`**（⌘）を使い、Windows/Linux では `Ctrl` を使います。`Ctrl+C/D/L/V` はどこでも `Ctrl` のままです（ターミナルの慣習）。

| キー | 動作 |
|---|---|
| `Enter` | メッセージ送信（turn 実行中なら queue） |
| `Shift`/`Alt`+`Enter` | 改行 |
| `Up` / `Down` | 前/次の送信済み prompt を呼び出す（または autocomplete の選択を移動） |
| `Ctrl+C` | turn を中断（idle 時は終了） |
| `Ctrl+D` | 終了 |
| `Ctrl+L` | store から会話を再描画（blank になった view を回復；破棄するには `/clear`） |
| `Ctrl+V` | 貼り付け（テキストまたは画像パス） |
| `Cmd/Ctrl+O` | 設定 |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | 新規 tab / tab を閉じる |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | tab へジャンプ / 巡回 |
| `Cmd/Ctrl+B` | background tasks パネル |
| `Cmd/Ctrl+G` | サイドバーの切り替え |
| `F1` | ヘルプ |
| `PgUp` / `PgDn` | 会話をスクロール |
| `Home` / `End` | 会話の先頭 / 最新へジャンプ |
| `Esc` | 現在の overlay を閉じる（または実行中 turn を中断） |

## Project instructions

Zode は 3 階層の hierarchy から instruction を読みます（後のものほど注意を引きます）：global `~/.zode/AGENTS.md`（または `instructions.md`）→ project root → cwd。各ディレクトリでは `CLAUDE.md` より `AGENTS.md` を優先します。Skills は `.zode/skills/**/SKILL.md`、MCP server は `~/.zode/mcp.json` ⊕ `.mcp.json`、hooks は `~/.zode/hooks.json` ⊕ `.zode/hooks.json` に置きます。

**Cross-agent 設定。** Zode は Claude Code、Codex、Cursor、opencode、Gemini および関連するローカル agent から直接の skills と MCP 設定を読み込みます。それらの製品に属するインストール済み plugin tree や plugin cache はスキャンされません。plugin を再利用するには、`zode plugin install ... --trust` でソースを明示的にインストールしてください。Zode 経由でインストールした plugin では Codex と Claude Code のパッケージ形式が引き続きサポートされます。

## MCP server の設定

MCP server は他のすべてと同じ nested-precedence 設定にあります — 全 project 共通は `~/.zode/mcp.json`、repo にスコープするには project root の `.mcp.json` または `.zode/mcp.json`。registry も restart-and-pray も不要です。ファイルを編集して `/mcp`（または再起動）で拾います。

### stdio（ローカル server を spawn）

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

`command`/`args` は server を subprocess として起動し stdio で pipe します。`env` の値は zode 自身の process 環境に対する `$NAME` / `${NAME}` 置換に対応します（接続直前に展開され、ディスクには書かれません）— token を config ファイル自体から外しておくのに便利です。

### Streamable HTTP（remote server）

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

`"transport": "http"` は現行 MCP 仕様の Streamable HTTP transport で接続します — 単一の `url` で、別の SSE endpoint を設定する必要はありません。`"sse"` も同等の綴りとして受け付けられ（一部の config や MCP server 自身のセットアップ文書はまだこう呼びます）、両者は同じ connector に解決されます。`headers` はそのまま転送され（`Authorization` を含むので Bearer/Basic/custom スキームが動作）、`env` と同じ `$VAR` 置換に対応します。定義を残したまま接続しないようにするには任意の server に `"enabled": false` を追加します — `/mcp` もファイルを手編集せず server ごとにこれを切り替えます。

### 使い方

接続済み server が公開するすべての tool は `mcp__<server>__<tool>` として現れ、agent が組み込み tool と同様に呼び出せます（入力欄で `@`-mention も可能）。`/mcp` は発見されたすべての server（connected / disconnected / disabled）を一覧するダイアログを開き、Space で on/off を切り替えます。サイドバーの折りたたみ可能な `mcp` セクション（▼ ヘッダーのクリック、または `/sidebar mcp`）が同じライブ接続状態を一目で映します。

Zode は Claude Code、Codex、Cursor、opencode、Gemini からの直接 MCP 設定も読みます。home の設定はユーザーのセットアップとして扱われ、project-local の外部 MCP 定義は disabled で発見され `/mcp` で有効化できます。別製品のインストール済み plugin tree に埋め込まれた MCP 宣言はスキャンされません。`openpencil` は予約済みで、op-bridge がネイティブに駆動するため、その名前で宣言された server は無視されます。

## Skills と command Markdown のインストール

どちらもディスク上の素の Markdown です — registry も build step もありません。ファイルを置けば次回起動で有効になります（`/skills` で何が load されたか確認できます）。

### skill をインストールする

skill は中に `SKILL.md` を持つフォルダです。project（`.zode/skills/`）または home ディレクトリ（`~/.zode/skills/`）に置きます。

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

これで skill は `/skills` に現れ、agent は Skill tool で自ら呼び出せ、動的な slash command にもなります — `/code-review look at src/lib.rs` と入力すると skill を実行する prompt に展開されます。`SKILL.md` の隣の追加ファイル（reference、script）も skill とともに配布されます。Claude Code、Codex、opencode、Cursor および関連 agent に属する直接の skills ディレクトリはスキャンされます。それら製品のインストール済み plugin tree や cache に埋もれた skill はスキャンされません。ここで使いたい場合は Zode 経由で plugin を明示的にインストールしてください。

### command（prompt Markdown）をインストールする

custom slash command は **ファイル名がコマンド名**で、本文が送信する prompt になる単一の `.md` ファイルです。コマンドの後に入力したものは本文に追加されます。

```bash
mkdir -p .zode/commands            # または全 project 用に ~/.zode/commands
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

これで `/changelog` がその prompt を送信し、`/changelog only the sidebar work` は引数を後ろに追加します。`~/.claude/commands` と `~/.codex/commands`（およびその project レベル相当）のコマンドも load されます。*外部 plugin tree* 内のコマンドはデフォルトで off です — opt-in するには `.md` を `.zode/commands/` ディレクトリにコピーしてください。

## ZSeven-W エコシステム

Zode は AI-native な開発ツール向けの、より広い ZSeven-W スタックの一部です。

| Product | 概要 |
|---------|------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | LLM agent を出荷するための pure-Rust async runtime：multi-provider streaming、tool dispatch、permissions、MCP、cost tracking、attachments、sessions、optional coding tools。 |
| [`jian`](https://github.com/ZSeven-W/jian) | `.op` ファイルを app として扱う Rust-native なクロスプラットフォーム UI framework。OpenPencil スタイルの design artifact を runnable software へつなげます。 |
| [`noema`](https://github.com/ZSeven-W/noema) | coding agent 向けの local-first な non-vector memory system。lexical recall、review queue、MCP アクセス、S3 offload、enterprise policy control を備えます。 |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | design-as-code workflow 向けのオープンソース AI-native vector design tool。prompt を live canvas 上の UI に直接変換し、concurrent agent team をサポートします。 |

## ベンチマーク

Zode の benchmark は one-shot code generation、agentic な read/run/edit/fix、多ファイル task、tricky bug、MCP/Skills/constraint following、Noema LOCOMO runner をカバーします。全次元で **Zode + DeepSeek-v4-pro は Claude に匹敵**し、各 task は*隠れた* grader で採点されます。方法、再現コマンド、詳細な結果テーブルは [英語 README の Benchmark](../../README.md#benchmark) を参照してください。suite は [`benchmarks/`](../../benchmarks/) にあります。

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

## テストレポート

全スイートがグリーン。エンドツーエンドのセルフテストは実際の進化ループ——ツール
グループ適応度 → 生成された JS 遺伝子 → キャパシティ淘汰 → ゲノム永続化——を走らせ、
`SELF-TEST PASSED` を出力します：

| スイート | コマンド | 結果 |
|---|---|---|
| Harness コア・進化レイヤー・プロセスプラグイン | `cargo test -p cordis-rs` | 50 passed |
| 進化インテグレーション（グループ適応度、ゲノム復元） | `cargo test -p zode-core --lib evolution::` | 5 passed |
| QuickJS 遺伝子レイヤー（ソース差し替え、割り込み、メモリ上限） | `cargo test -p zode-core --test js_plugin_it` | 4 passed |
| zode-core 全スイート（進化配線を含む） | `cargo test -p zode-core --lib` | 983 passed |

```sh
cargo run -p zode-core --example evolution_self_test
```

- フックパイプラインは各ツール結果をそのツールグループに対してスコアリングします
  （`uses − 10·failures − 100·panics − 5·restarts`）。`unfit_groups()` は無効化候補の
  グループを列挙します。
- 遺伝子プールには厳格なキャパシティがあり、エージェントが新しい候補を進化させると
  最弱の遺伝子が淘汰されます（セルフテストでは `git` → `todo` → `shell` の順）。
  最適者が生き残ります。
- 生成された遺伝子は JavaScript（コンパイラ不要）で、遺伝子ごとにメモリ上限と
  割り込み期限があります。暴走した遺伝子は隔離され、zode に害を与えません。
- ゲノムは `<config-dir>/evolution/genome.json` に永続化され、再起動時に適応度ごと
  復元されます。`dispose()` は全 fiber・リスナー・イベント履歴を回収します。

完全なレポート（観測出力と修正済みリグレッション）は `crates/cordis-rs/README.md` を
参照してください。

## コントリビューション

Contributions welcome! [Conventional Commits](https://www.conventionalcommits.org/) の `<type>(<scope>): <subject>` 形式に従ってください。scope は `core`、`tui`、`cli`、`tools`、`config`、`build`、`ci`、`docs` などです。

## License

[MIT](../../LICENSE) &copy; ZSeven-W
