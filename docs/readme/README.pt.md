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
  <strong>Assistente de programação open-source e AI-native para o terminal.</strong><br/>
  Lê seu código, executa comandos, busca arquivos e gerencia git a partir de uma TUI rápida em Rust.
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

> Este README localizado cobre a visão geral e o início rápido. O [README em inglês](../../README.md) continua sendo a referência para detalhes completos de benchmarks e notas longas atualizadas.

## Destaques

- **Multi-provider**: Anthropic, OpenAI, APIs compatíveis com OpenAI como DeepSeek, Moonshot e OpenRouter, além de Ollama local.
- **Superfície rica de ferramentas**: leitura/escrita/edição de arquivos, busca de código e conteúdo, shells em primeiro e segundo plano, git, web fetch, notebooks e TODO tracking.
- **Controle de navegador**: ferramentas `browser_*` controlam um Chromium gerenciado ou seu perfil real do Chrome via extensão Chrome bridge.
- **Permissões sem bloqueio**: ferramentas com efeitos colaterais passam por allow once / always / deny, com prompt inline.
- **Sandbox do OS por padrão**: comandos shell rodam sob `sandbox-exec` no macOS ou `bwrap` no Linux, com rede de saída negada por padrão.
- **TUI em tela cheia**: Markdown em streaming, syntax highlighting, diff preview, slash-command autocomplete, histórico, temas, overlays de settings/help e UI em 15 idiomas (`/language`).
- **Abas multi-sessão**: rode conversas isoladas em paralelo com `Ctrl+T` e retome sessões anteriores.
- **Sub-agents e workflows**: delegue tarefas bem delimitadas com a ferramenta Task e gerencie com `/agents` e `/workflows`.
- **Skills, MCP e hooks**: carregue pacotes `SKILL.md`, conecte servidores MCP e execute scripts externos em eventos de ferramentas.

## Instalação

### Uma linha

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

O instalador detecta OS e CPU, baixa o binário correto do último [release](https://github.com/ZSeven-W/zode/releases) e coloca `zode` no `PATH`.

### Download manual

Baixe o arquivo da sua plataforma na [página de releases](https://github.com/ZSeven-W/zode/releases):

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Descompacte e mova `zode` para o `PATH`, por exemplo `sudo mv zode /usr/local/bin/`. Builds Linux usam glibc; binários macOS não são assinados. Se o Gatekeeper reclamar, use `xattr -dr com.apple.quarantine ./zode`.

### A partir do código-fonte

Requer uma toolchain Rust estável recente:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

O binário fica em `target/release/zode`. O runtime do agente é o submodule git `vendor/agent`; clone com `--recurse-submodules` ou rode `git submodule update --init`.

## Início rápido

A maneira mais simples é iniciar `zode` e executar **`/connect`**. Isso abre um seletor interativo de modelos e grava a configuração.

Você também pode escrever `~/.zode/config.json` manualmente:

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

Comandos comuns:

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

## Configuração

`providers` é a fonte de verdade dos provedores; o `provider` no topo aponta para o modelo ativo. Provedores compatíveis com OpenAI normalmente usam `baseUrl` e `dialect`:

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
  "language": "pt"
}
```

Um provider pode ter vários modelos, e `/model` alterna entre eles ao vivo. O idioma também pode ser alterado com `/language`.

## Server mode e SDKs

`zode server` inicia um servidor JSON-RPC delimitado por novas linhas em stdin/stdout, voltado a integrações com editores, automação local, testes e clientes SDK.

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDKs:

| SDK | Diretório | Teste local |
|-----|-----------|-------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## Controle do navegador

Zode inclui o grupo `tools:browser` para ler screenshots/DOM/logs, navegar, clicar, digitar, executar JavaScript e gerenciar abas. Ele usa Chromium gerenciado ou seu Chrome real pela extensão em [`extensions/chrome/`](../../extensions/chrome/).

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

## Slash commands comuns

| Comando | Ação |
|---|---|
| `/help` | Ajuda de comandos e atalhos |
| `/connect` | Conectar e alternar provider |
| `/model [id]` | Mostrar ou definir modelo ativo |
| `/sessions`, `/resume` | Retomar sessões |
| `/browser ...` | Controle do navegador |
| `/tasks` | Tarefas em segundo plano |
| `/mcp` | Gerenciar servidores MCP |
| `/skills` | Listar skills |
| `/agents` | Gerenciar sub-agents |
| `/workflows` | Gerenciar workflows |
| `/sandbox ...` | Controlar sandbox |
| `/language` | Alterar idioma da UI |
| `/export [path]` | Exportar Markdown |
| `/exit` | Sair |

A tabela completa está no [README em inglês](../../README.md#slash-commands).

## Instruções, MCP e skills

Zode lê instruções de `~/.zode/`, raiz do projeto e diretório atual; em cada nível prefere `AGENTS.md` e depois `CLAUDE.md`. Skills ficam em `.zode/skills/**/SKILL.md`; servidores MCP em `~/.zode/mcp.json`, `.mcp.json` ou `.zode/mcp.json`.

Zode também descobre skills, comandos e configurações MCP de Claude, Codex, opencode, Cursor e outros agentes. MCPs externos encontrados dentro do projeto ficam desativados por padrão.

## Ecossistema ZSeven-W

Zode faz parte do stack ZSeven-W de ferramentas AI-native para desenvolvimento:

| Produto | O que é |
|---------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Runtime async em Rust puro para LLM agents, com streaming multi-provider, tool dispatch, permissões, MCP, cost tracking, attachments, sessions e coding tools opcionais. |
| [`jian`](https://github.com/ZSeven-W/jian) | Framework UI cross-platform nativo de Rust em que um arquivo `.op` é um app, ligando artefatos de design estilo OpenPencil a software executável. |
| [`noema`](https://github.com/ZSeven-W/noema) | Sistema de memória local-first e non-vector para coding agents, com lexical recall, review queues, MCP, S3 offload e enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Ferramenta open-source AI-native de design vetorial para workflows design-as-code, transformando prompts em UI no live canvas com concurrent agent teams. |

## Benchmark

Os benchmarks de Zode cobrem code generation one-shot, fluxo agentic de ler/rodar/editar/corrigir, tarefas multiarquivo, bugs difíceis, MCP/Skills/constraint following e Noema LOCOMO. Metodologia e resultados completos estão na seção [Benchmark do README em inglês](../../README.md#benchmark).

## Desenvolvimento

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## Contribuição

Contribuições são bem-vindas. Use [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`.

## Licença

[MIT](../../LICENSE) &copy; ZSeven-W
