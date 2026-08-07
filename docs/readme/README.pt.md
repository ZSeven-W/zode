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

- **Multi-provider** — Anthropic, OpenAI e qualquer API compatível com OpenAI (dialetos DeepSeek, Moonshot, OpenRouter), além do Ollama local. Suporta modelos de saída grande e de **contexto de 1M** (`contextWindow` / `maxOutputTokens` são configuráveis).
- **Superfície rica de ferramentas** — leitura/escrita/edição de arquivos (incluindo o `MultiEdit` atômico multi-hunk), busca de código e conteúdo, shells em primeiro e segundo plano, git, web fetch (além do `WebSearch` opcional com uma chave Tavily), notebooks e TODO tracking.
- **Controle de navegador** — as ferramentas `browser_*` integradas controlam uma instância Chromium gerenciada ou seu perfil real do Chrome através da extensão Chrome bridge do zode: navegar, clicar/digitar, inspecionar o DOM, capturar screenshots, ler logs de console/rede e agrupar as abas abertas pelo zode. O pareamento é feito uma única vez — a extensão reconecta automaticamente entre reinícios do zode.
- **Permissões sem bloqueio** — toda ferramenta com efeito colateral é controlada (allow once / always / deny), mas o prompt aparece inline e nunca bloqueia você: continue digitando para enfileirar o próximo passo enquanto uma ferramenta espera, com regras de negação forçada (hard-deny).
- **Sandbox do OS, ativo por padrão** — comandos shell rodam sob sandbox-exec (macOS) / bwrap (Linux) em modo `read-only` ou `workspace-write`, com **rede de saída negada por padrão**. Alterne ao vivo com `/sandbox`; o modelo pode pedir escape para um único comando (`dangerouslyDisableSandbox`), que **você autoriza** no prompt.
- **TUI em tela cheia** — Markdown em streaming com syntax highlighting, prévia de diff, autocomplete de slash-commands, histórico de prompts (Up/Down), 11 temas integrados, overlays de settings e ajuda, seções resilientes na barra lateral direita e **UI em 15 idiomas** (`/language`).
- **Sessões duráveis e compatíveis com V1** — mantém o contrato de transcript `<id>.jsonl` existente e adiciona journals, checkpoints, rewind, fork e Git worktrees isoladas como dados auxiliares. A compactação de contexto nunca perde a conversa visível — sessões retomadas reproduzem o histórico completo pré-compactação enquanto o contexto do modelo permanece compacto.
- **Superfícies de automação** — saída headless estável em JSON/JSONL, targeting exato de sessão, filtros de ferramentas, exit codes determinísticos, ACP sobre stdio e um dashboard local de operações.
- **Abas multi-sessão** — rode várias conversas lado a lado (`Ctrl+T`), cada uma um agente isolado; retome sessões anteriores com replay completo do histórico.
- **Sub-agents, equipes e workflows** — delegue trabalho pontual pela ferramenta Task, contrate teammates internos ou de CLIs externas persistentes, coordene-os com um board compartilhado e file claims, e gerencie tudo com `/agents`, `/team` e `/workflows`.
- **Configuração local portátil** — lê skills e configuração MCP diretas de Claude Code, Codex, Cursor, opencode e Gemini, sem nunca importar suas árvores de plugins instaladas ou caches.
- **Skills e MCP** — carregue pacotes de instruções `SKILL.md` sob demanda e conecte servidores MCP (`mcp__<server>__<tool>`); agents, skills e ferramentas MCP criados aparecem como slash commands.
- **Hooks** — execute scripts externos em eventos de ferramentas (por exemplo, bloquear comandos perigosos, rodar lint após edições).
- **Instruções em três níveis** — global (`~/.zode/`) → raiz do projeto → cwd (`AGENTS.md` / `CLAUDE.md`).

## Instalação

### Uma linha (binários pré-compilados)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

O instalador detecta automaticamente seu OS + CPU, baixa o binário correspondente do [release](https://github.com/ZSeven-W/zode/releases) mais recente e coloca `zode` no seu PATH. Para fixar uma versão ou mudar a localização:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

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

Depois descompacte e mova `zode` para o PATH (`sudo mv zode /usr/local/bin/`). Builds Linux usam glibc; binários macOS não são assinados (`xattr -dr com.apple.quarantine ./zode` se o Gatekeeper reclamar).

### A partir do código-fonte

Requer Rust 1.88 ou mais recente:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binário em target/release/zode
```

> O runtime do agente vive no submodule git `vendor/agent` — sempre clone com `--recurse-submodules` (ou rode `git submodule update --init`).

## Início rápido

A maneira mais simples é iniciar `zode` e executar **`/connect`** — um seletor interativo apoiado em models.dev que grava a configuração para você.

Para escrever `~/.zode/config.json` manualmente: **`providers`** é a fonte de verdade — uma entrada por provider (credenciais compartilhadas) contendo um ou mais **models** — e o **`provider`** de topo registra o modelo *ativo*:

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // protocolo de wire: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // o modelo ativo
}
```

Providers compatíveis com OpenAI (DeepSeek, Moonshot, OpenRouter, …) adicionam `baseUrl` + `dialect`, e as configurações por modelo ficam na entrada de cada modelo:

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

Uma única entrada de provider pode conter vários modelos — alterne entre eles ao vivo com `/model`.

Depois execute:

```bash
zode                       # TUI em tela cheia
zode -p "explain main.rs"  # headless: um prompt, stream para stdout, sai
zode --no-tui              # REPL readline simples
zode -c                    # continua a sessão mais recente
zode -r <id>               # retoma uma sessão por prefixo de id
zode --yolo                # ignora prompts de aprovação (regras de deny ainda valem)
zode --no-sandbox          # desativa o sandbox do OS (fica ATIVO por padrão)
zode --sandbox-read-only   # sandbox em modo somente leitura (nega toda escrita)
zode --sandbox-allow-network  # permite rede de saída dentro do sandbox
zode --browser             # força as ferramentas de navegador integradas nesta execução
zode --no-browser          # desativa as ferramentas de navegador integradas nesta execução
zode --model <id>          # sobrescreve o modelo
zode --provider <name>     # escolhe um provider nomeado de config.providers
zode server                # modo app-server JSON-RPC sobre stdio
zode acp                   # agente Agent Client Protocol sobre stdio
zode dashboard             # visão geral local de sessões/checkpoints/worktrees
```

Você também pode apontar para qualquer provider sem editar a config, exportando a chave correspondente (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); para o Ollama, o `baseUrl` é obtido do ambiente quando não definido.

## Controle do navegador

Zode inclui o grupo `tools:browser` para automação de navegador. O agente pode usar `browser_read` para screenshots, snapshots de DOM, logs de console, logs de rede e leitura de abas; `browser_act` para navegação, cliques, digitação, teclas e scroll; `browser_eval` para JavaScript; e `browser_tabs` para gerenciar abas. A inspeção somente leitura do navegador é liberada (ungated); ações de navegador com efeito colateral usam o mesmo fluxo de aprovação allow-once / always / deny das demais ferramentas.

Existem dois alvos de navegador:

- **managed** — o zode inicia e controla um perfil Chromium dedicado.
- **bridge** — o zode controla o perfil Chrome que você já usa, através da extensão MV3 incluída em [`extensions/chrome/`](../../extensions/chrome/).

Para o alvo bridge, carregue a extensão uma vez a partir de `extensions/chrome` e rode `/browser pair`. O Chrome bloqueia URLs `chrome-extension://` abertas por programas externos (ERR_BLOCKED_BY_CLIENT — igualmente em macOS, Windows e Linux), então a tentativa do próprio zode de abrir a página pode falhar — em vez disso, a própria extensão abre sua página de pareamento em ~30 segundos após `/browser pair`, com a porta pré-preenchida; digite ali o código de pareamento de 6 dígitos mostrado no chat. Como alternativa manual, digite você mesmo a URL `chrome-extension://…/popup.html?port=…` na barra de endereços (a navegação digitada à mão é iniciada pelo navegador e é permitida). **O pareamento é único**: a extensão armazena um token de longo prazo e reconecta automaticamente — na inicialização do navegador, em atualizações da extensão e com uma nova tentativa aproximadamente a cada 30 segundos enquanto estiver desconectada — então reiniciar o zode nunca pede pareamento de novo. Ela reconecta a uma CLI em execução ou inicia automaticamente um daemon zode apenas de extensão quando necessário. As abas abertas pelo zode ficam num grupo de abas do Chrome chamado `zode`.

### Painel lateral de tarefas no Chrome

Rode a CLI atualizada do zode e `/browser pair` uma vez. Clicar no ícone da toolbar abre o painel lateral; depois disso, ele inicia o zode automaticamente quando nenhum processo de CLI está rodando. A página de pareamento continua sendo um pequeno fluxo de código/token, e as tarefas permanecem compartilhadas com as sessões da TUI sem mudar o foco do terminal.

Turnos do painel lateral vinculam as ferramentas de navegador bridge à página exibida ao lado do painel, então pedidos como "analise esta página" usam `browser_read` na aba existente em vez de abrir uma nova. A automação de navegador autônoma da TUI e da CLI continua usando abas do próprio zode no grupo `zode`. A página ativa também é o contexto padrão para prompts ambíguos do painel lateral; arquivos locais do projeto só são inspecionados quando o usuário pergunta explicitamente sobre eles.

O painel pode enviar texto, selecionar um modelo, escolher os modos de acesso `readOnly`, `prompt` e `auto`, transmitir a resposta e parar (Stop) um turno em execução. Um turno pode anexar no máximo 8 arquivos e 20 MiB no total: imagens PNG, JPEG, GIF e WebP de até 5 MiB cada, além de arquivos de texto e código UTF-8 de até 1 MiB cada. Entradas PDF, Office, de arquivo compactado, executáveis e não-UTF-8 são rejeitadas.

Após uma atualização da extensão, clique em Reload em `chrome://extensions`. Versões mais antigas da extensão continuam compatíveis com a automação de navegador, mas não têm o painel lateral de tarefas. No Windows, o zode localiza e inicia o Chrome diretamente para URLs de extensão em vez de invocar o shell do navegador padrão, evitando o redirecionamento para a Microsoft Store quando o Chrome já está instalado.

Comandos úteis:

```bash
/browser                         # abre o painel de controle do navegador
/browser status                  # mostra estado de target/running/paired
/browser launch                  # inicia o navegador gerenciado
/browser close                   # fecha o navegador gerenciado
/browser pair                    # pareia ou reconecta a extensão Chrome bridge
/browser target managed          # usa o Chromium gerenciado do zode
/browser target bridge           # usa a extensão e salva como padrão do próximo início
/browser screenshot [path]       # captura um screenshot do navegador
```

Veja [`extensions/chrome/README.md`](../../extensions/chrome/README.md) para carregamento da extensão, atualização, empacotamento CRX e passos de smoke-test.

## Automação de desktop

Além do navegador, o zode pode dirigir aplicações desktop nativas através do grupo de ferramentas `desktop`. O agente usa `desktop_read` para inspecionar janelas e a árvore de acessibilidade, `desktop_act` para ações de mouse e teclado (mover, clicar, scroll, digitar, teclas, definir valores) e `desktop_screenshot` para capturar a tela. Ações somente leitura são liberadas; ações com efeito colateral usam o mesmo fluxo de aprovação allow-once / always / deny das demais ferramentas com efeito colateral.

Cada plataforma usa sua API de acessibilidade nativa:

- **macOS** — Accessibility (AX)
- **Windows** — UI Automation (UIA)
- **Linux** — AT-SPI
- **Electron** — CDP

O zode **nunca move o cursor real do mouse**. Enquanto a automação de desktop está ativa, um helper de overlay (`zode-overlay`) desenha um cursor fantasma (ghost cursor) que voa por um caminho de Dubins até o elemento-alvo, para você acompanhar o que o agente está fazendo. O overlay é um helper macOS de permissão zero (janelas sem borda, click-through e never-key), iniciado sob demanda na primeira ação de desktop; um helper ausente simplesmente desativa a visualização.

Também enquanto a automação de desktop está ativa, um CGEventTap global captura a tecla **Esc** e interrompe imediatamente todos os turnos em execução (mesmo caminho do Esc da TUI), então se desarma e oculta o overlay. Falha ao criar o tap não é fatal (o suporte a Esc simplesmente fica ausente).

Configuração (`desktop.*`): `ghostCursor` (padrão `true`), `escCancel` (padrão `true`) e `overlayHelperPath` (padrão `null`). O comando `/desktop` mostra o estado da automação de desktop.

## Watchdog de turnos em segundo plano, /loop e /schedule

Turnos `/loop` e `/schedule` de propriedade do scheduler rodam sob um watchdog de liveness in-process. Atividade de provider, ferramentas e agentes aninhados atualiza um heartbeat compartilhado do lado da fonte, enquanto `maxRuntimeSecs` continua sendo um limite absoluto. Em qualquer timeout, o zode pede cancelamento cooperativo, aguarda `abortGraceSecs` e faz hard-stop da tarefa local se ela ainda não tiver drenado. Parar a tarefa não basta para liberar o slot do scheduler: o zode também aguarda todo provider, ferramenta, hook, leitor de subprocesso e worker de agente aninhado rastreados entrarem em quiescência. Se essa segunda fronteira não for alcançada em cinco segundos, a aba/store é colocada em quarentena, o job é desabilitado e sua lease de tentativa ativa permanece retida até os workers realmente saírem.

Tentativas falhas usam backoff exponencial limitado, de `initialBackoffSecs` a `maxBackoffSecs`. Um turno bem-sucedido zera a contagem consecutiva de falhas; uma vez esgotado `maxRetries`, o zode para o loop ou desabilita o schedule persistido. Interrupção manual, remoção do job e desabilitação explícita cancelam a recuperação pendente. A recuperação é intencionalmente conservadora quanto a efeitos colaterais: o zode só repete automaticamente quando não observou efeito colateral; se uma mutação pode já ter ocorrido, ele para/desabilita o job e aguarda revisão humana.

A quiescência é uma garantia local. Trabalho já aceito por um servidor MCP remoto, extensão de navegador, ator de desktop ou outro sistema externo pode não suportar revogação. Se tal chamada for interrompida, o zode marca o resultado como não resolvido, desabilita o job do scheduler e exige que você verifique o estado externo antes de reabilitá-lo.

Comandos e timing:

- **`/loop <30s|5m|1h> [--max N] <prompt>`** — turnos recorrentes apenas na sessão, na aba atual; `list` / `stop [id]`. Intervalo mínimo de 30s. Um prompt vencido é enfileirado pelo mesmo caminho `queued_input` (nunca interrompe um turno em execução).
- **`/schedule add <hh:mm|mon hh:mm|every 2h> <prompt>`** — persistido em `~/.zode/schedules.json`. Disparos perdidos enquanto o zode não está rodando são ignorados, nunca repetidos. `list` / `rm <id>` / `enable|disable <id>`.
- **`/watchdog [status]`** — reporta a configuração efetiva mais o estado ao vivo/de retry.
- **`/tasks`** — inclui as mesmas linhas de saúde ao lado dos shells em segundo plano e turnos em execução.

Este é um watchdog para turnos do scheduler dentro do processo zode atual. Ele **não** é um supervisor de processos do OS e não pode reiniciar o zode após um crash ou reinício da máquina; use o gerenciador de serviços da sua plataforma quando reinícios em nível de processo forem necessários.

O timing dos turnos é registrado: o `TurnRecorder` grava `durationMs` nos eventos `tool.completed` e `turn.completed`. A TUI mostra sufixos de duração por ferramenta (`· 1.2s`), um rodapé de turno (`✓ done · 34s · 3 tools`) e tempo decorrido humanizado em `/tasks`.

## Automação, sessões duráveis e operações

### Execuções headless estruturadas

`-p`, `--prompt-file` e `--prompt-json` usam o mesmo motor headless. `json` emite um objeto de resultado final; `stream-json` emite um objeto JSON `zode.run-event.v1` por linha. Os modos estruturados reservam o stdout para saída legível por máquina e usam exit codes estáveis: `0` sucesso, `10` erro de provider, `11` permissão negada, `12` limite de turnos/atingido, `13` interrompido (Ctrl-C), `14` resultado parcial, `15` erro de targeting de sessão.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# IDs exatos não fazem correspondência por prefixo. Um fork nunca altera sua sessão de origem.
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Padrões de deny de ferramentas vencem sobre padrões de allow e são herdados pelos sub-agents da Task. `--permission-mode` aceita `default`, `dont-ask`, `accept-edits` e `bypass`; `--yolo` continua sendo um atalho para bypass, enquanto regras de deny forçado ainda se aplicam.

### Sessões, checkpoints e worktrees compatíveis com V1

O transcript continua sendo o arquivo V1 original em `~/.zode/sessions/<id>.jsonl`. Essa é a **única** cópia do transcript, então clientes zode antigos podem continuar lendo e escrevendo nela. Os novos metadados são aditivos e ficam em `~/.zode/sessions/<id>/` (`meta.json`, journal, checkpoints e snapshots). Nenhum novo formato de sessão ou migração de transcript é necessário.

```bash
zode session list
zode session list --json
zode session show <id>                         # metadados + IDs de checkpoint
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # prévia consciente de conflitos
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

Um checkpoint é capturado antes de um turno com mutação. O rewind restaura o conteúdo dos arquivos rastreados e o prefixo do transcript, reporta conflitos em vez de sobrescrever mudanças mais novas, e registra um novo ramo lógico no journal em vez de apagar o histórico. Forks em worktree podem ser aplicados de volta explicitamente quando o experimento estiver pronto.

**A compactação nunca perde a conversa visível.** Quando a compactação de contexto substitui mensagens antigas por um resumo, os originais são preservados em um sidecar aditivo (`~/.zode/sessions/<id>/compacted.jsonl`). Retomar uma sessão, pressionar `Ctrl+L`, `/export` e o painel lateral do Chrome exibem o histórico completo pré-compactação, enquanto o modelo continua recebendo apenas o contexto compactado. Forks carregam esse arquivo (filtrado para o próprio transcript), `/clear` o remove, e apagar uma sessão remove o sidecar inteiro.

### Regras de permissão e perfis de sandbox

Regras podem viver sob `permissions.rules` em `config.json`, ou em um arquivo JSON autônomo passado com `--rules`. Um field matcher usa um JSON pointer RFC 6901; deny tem precedência sobre ask, que tem precedência sobre allow. O arquivo autônomo deve ser um array de regras ou `{ "rules": [...] }`; ele não é embrulhado num objeto `permissions` de topo.

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

Os perfis integrados são `read-only`, `workspace`, `workspace-network` e `unconfined`. Perfis definidos na config usam os mesmos campos de sandbox mostrados acima. No macOS o sandbox usa sandbox-exec; no Linux usa `bwrap` (que precisa estar instalado). O Windows tem suporte de sandbox por tiers. A inicialização falha de forma fechada (fail-closed) se o sandbox configurado não puder ser verificado; use `--no-sandbox` explicitamente para rodar sem ele.

### Plugins e marketplaces estáticos

Um plugin gerenciado pode contribuir skills, commands, agents, hooks, servidores MCP, servidores LSP e renderizadores de UI JavaScript em sandbox. O zode aceita `plugin.json`, `.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.grok-plugin/plugin.json` e `.claude-plugin/plugin.json`. Arrays de caminho de componentes do Codex e do Claude Code são suportados, e o `defaultEnabled` do Claude Code é respeitado na primeira instalação. Componentes exclusivos de host, como apps/connectors do Codex e themes, monitors ou output styles do Claude Code, são ignorados; um plugin somente-app é rejeitado por não ter componente compatível com o Zode. Instalações são snapshots imutáveis com proveniência e um hash SHA-256 da árvore. Conteúdo de plugin executável nunca é ativado sem a flag `--trust` explícita.

#### Início rápido de plugin de UI JavaScript

O menor plugin de UI contém um manifest e um arquivo JavaScript:

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

Instale um diretório local ou um repositório/subdiretório do GitHub, e reinicie um processo zode em execução para que ele carregue o novo snapshot:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Use `zode plugin update my-plugin` após mudar a fonte. `--trust` é obrigatório porque JavaScript, hooks, servidores MCP e o acesso de rede declarado são capacidades executáveis. Instalação e atualização imprimem o grant de permissão declarado pelo plugin (hosts de rede, variáveis de ambiente, escopos de contexto). Uma atualização cujo manifest solicite permissões *mais amplas* que o snapshot instalado é recusada, a menos que você repita com `--trust` — uma fonte Git em movimento não pode ampliar silenciosamente o próprio grant.

#### API de renderização de UI

Plugins de UI podem contribuir linhas declarativas logo acima da versão na barra lateral — no máximo seis linhas no total, compartilhadas por todos os plugins na ordem de carregamento. Declare um entrypoint JavaScript no manifest:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

Registre um renderizador síncrono com `zode.ui.sidebar`. O contexto é um snapshot JSON somente leitura contendo campos de terminal, sessão, modelo, status, token e janela de contexto. O resultado é renderizado pelo Zode; os scripts não recebem nenhuma ponte de filesystem, rede, terminal ou Ratatui.

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

Os tons suportados são `default`, `muted`, `accent`, `success`, `warning` e `danger`; spans também aceitam `bold` e `italic`. Um renderizador deve ser síncrono. Cada script é limitado a 256 KiB, 8 MiB de memória JS e 25 ms por avaliação, e os renderizadores são reavaliados no máximo a cada 250 ms (a saída em cache é reutilizada entre avaliações). A saída da sidebar é limitada a 6 linhas por renderizador (6 no total entre plugins), cada linha a 16 spans e 2.048 bytes de texto. Caracteres de controle são higienizados pelo host.

A barra de status também é extensível. Ela permanece com uma linha quando nenhum plugin retorna conteúdo e cresce dinamicamente para duas linhas quando um renderizador síncrono `zode.ui.statusLine` retorna spans. O Zode mantém seus indicadores de status e segurança principais na primeira linha; a saída do plugin é composta na segunda.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Contexto de renderização e permissões

Todo renderizador recebe os campos base a seguir sem solicitar permissão de contexto adicional:

| Campo | Estrutura e significado |
| --- | --- |
| `ctx.apiVersion` | Versão da API de contexto; atualmente `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | `{ width, height }` em células do terminal. |
| `ctx.session` | `{ id, title, cwd, busy }` da tarefa ativa. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` contém `{ enabled, readOnly, network }`. |
| `ctx.tokens` | contadores de token `{ input, output }`. |
| `ctx.context` | `{ used, window, usedPercent }`; a porcentagem pode ser `null`. |
| `ctx.data` | Resultados pertencentes apenas às fontes de dados registradas por este plugin. |

Seções mais ricas são omitidas a menos que o plugin solicite o escopo correspondente em `permissions.context`:

| Escopo | Campo exposto | Estrutura e limites |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` começa em 1. |
| `workspace` | `ctx.workspace.modifiedFiles` | Até 50 entradas Git `{ path, added, removed }`. |
| `tools` | `ctx.tools.available` | Nomes ordenados das ferramentas habilitadas para a tarefa ativa. |
| `tools` | `ctx.tools.active` | Nomes das ferramentas em execução no momento. |
| `tools` | `ctx.tools.recent` | Até 20 registros `{ name, status, durationMs }`. |
| `tasks` | `ctx.tasks.todoStatuses` | Apenas strings de status de todo, sem o texto do todo. |
| `tasks` | `ctx.tasks.subagents` | Registros `{ type, status }`, sem prompts ou transcripts. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, sem o texto do goal. |
| `services` | `ctx.services.mcp` | Registros `{ name, connected }`. |
| `services` | `ctx.services.lsp` | Registros `{ language, running }`. |

Por exemplo:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` é uma API de observação: ela informa a um renderizador quais ferramentas existem e quais estão ou estiveram em execução. Plugins de UI não podem invocar uma ferramenta. Entradas de ferramentas, saídas de ferramentas, prompts, conteúdo de transcript, texto de todo/goal, valores de ambiente e credenciais não são incluídos, e a API não pode contornar o sistema de aprovação do Zode.

#### Dados HTTP em segundo plano

Plugins de UI também podem registrar fontes de dados HTTP em segundo plano. Acesso à rede e a segredos deve ser declarado no manifest:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

A requisição é declarativa e roda fora do caminho de renderização. Variáveis de ambiente secretas são montadas em headers pelo Zode e nunca são expostas ao JavaScript:

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

`zode.data.define(key, config)` aceita uma key alfanumérica de 1–64 caracteres, underscore ou hífen. `request` suporta `url`, `method`, `headers`, `body` JSON opcional e `timeoutMs`. Os padrões são `GET`, timeout de 3 segundos e refresh de 60 segundos. Apenas HTTPS `GET` e `POST` são aceitos. Headers literais são strings; um header secreto usa `{ "env": "NAME", "prefix": "Bearer " }`. A variável de ambiente também deve aparecer em `permissions.env`, é lida apenas pelo Rust ao montar a requisição, e nunca é retornada ao JavaScript.

O Zode desativa redirects e proxies, valida e fixa endereços DNS públicos, rejeita localhost/redes privadas, limita respostas a 256 KiB, restringe timeouts de requisição a 500 ms–10 segundos e restringe intervalos de refresh a 10 segundos–1 hora. Um wildcard como `*.example.com` casa subdomínios, mas não o host nu `example.com`.

Cada plugin vê apenas seus próprios dados. `ctx.data.<key>` contém `{ ok, status, data, updatedAt }` ou `{ ok: false, error, updatedAt }`. Respostas JSON viram objetos/arrays; respostas não-JSON viram strings. Um status HTTP de erro ainda inclui `status` e `data`, com `ok: false`.

Inicie o zode com o segredo necessário em seu ambiente ao usar uma quota privada ou API de coding-plan:

```bash
CODING_PLAN_TOKEN=... zode
```

O [exemplo executável completo](../../examples/plugins/zode-ui-demo/) exibe atividade de modelo/contexto/ferramentas na sidebar e na barra de status e usa `zode.data.define` para uma quota pública da API do GitHub.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# Um marketplace é um índice estático local/Git, não um serviço hospedado pelo Zode.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # desambiguar se necessário
zode plugin marketplace update
```

### ACP, dashboard, telemetria e testes de regressão da TUI

`zode acp` implementa initialize/new/load/fork/prompt/cancel do ACP sobre stdio, transmite atualizações de mensagem/thought/tool, solicita permissões através do cliente e aceita servidores MCP stdio, HTTP e SSE fornecidos pelo cliente. Os dados de sessão usam o mesmo store compatível com V1 da TUI e da CLI headless.

```bash
zode acp
zode dashboard
zode dashboard --json
```

A exportação OTLP está desligada por padrão e requer opt-in explícito. Ela exporta apenas atributos de ciclo de vida/nome-de-ferramenta/status/uso sem conteúdo: prompts, texto gerado, entradas/saídas de ferramentas, caminhos de arquivo e mensagens de erro nunca são enviados.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

Para cenários de regressão da TUI em terminal real, o workspace inclui um harness PTY + VT100 que registra diagnósticos brutos e snapshots de tela virtual:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` dirige o terminal real com waits ordenados, entrada de teclas, resizes e snapshots (a notação de teclas suporta `<Enter>`, `<Esc>`, `<Tab>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>` e `<C-l>`):

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

Esta implementação local/aberta deliberadamente não inclui contas, billing específicos da xAI, nem um serviço de marketplace na nuvem operado pelo Zode.

### Chaves de config de topo opcionais

Todas têm padrões razoáveis:

```jsonc
{
  "maxOutputTokens": 16384,      // teto de saída por turno (aumente para escritas grandes)
  "contextWindow": 1000000,      // janela de contexto do modelo — defina 1000000 para um modelo de 1M
  "temperature": 0,              // menor = mais determinístico
  "language": "pt",              // idioma da UI (15 locales); também via /language
  "effort": "medium",            // esforço de raciocínio; no Anthropic, medium/high mapeiam para thinking budgets reais
  "autonomousOrchestration": true, // orquestração de sub-agents + workflows (padrão ligado)
  "subagentMaxIterations": 0,      // guarda opcional para filhos; omitido/0 = ilimitado
  "tools": {
    "deferNonCore": false        // true: mantém ~20 ferramentas do dia a dia visíveis e adia o resto atrás do ToolSearch
  },
  "webSearch": {
    "tavilyApiKey": null         // habilita a ferramenta WebSearch (ou defina $TAVILY_API_KEY)
  },
  "sandbox": {
    "enabled": true,             // sandbox do OS para comandos shell (padrão ligado)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // permite rede de saída dentro do sandbox
    "writableRoots": []          // diretórios graváveis extras (workspace-write)
  },
  "browser": {
    "enabled": true,             // ferramentas browser_* e painel /browser (padrão ligado)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // modo de launch do Chromium gerenciado
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // vigia turnos /loop e /schedule autônomos
    "inactivityTimeoutSecs": 900, // aborta após 15 min sem atividade de provider/ferramenta
    "maxRuntimeSecs": 3600,      // teto absoluto de uma hora por turno em segundo plano
    "abortGraceSecs": 10,        // espera cancelamento cooperativo antes do hard-stop
    "maxRetries": 3,             // tentativas consecutivas de recuperação antes de esgotar
    "initialBackoffSecs": 5,     // atraso do primeiro retry
    "maxBackoffSecs": 300        // teto do backoff exponencial de retry
  }
}
```

> `contextWindow` dirige a auto-compactação — defina-o para a janela real do seu modelo (por exemplo `1000000`). Prefira o valor **por modelo** em `providers.<name>.models.<id>.contextWindow` (ele tem precedência); a chave de topo acima é um fallback global. Não o defina acima da janela real: superestimar faz as requisições transbordarem e o provider rejeita o turno.

## Server mode e SDKs

`zode server` inicia um servidor JSON-RPC delimitado por novas linhas em stdin/stdout. Destina-se a integrações com editores, automação local, testes e clientes SDK que querem as capacidades existentes do zode sem iniciar a TUI.

```bash
zode server                      # stdio (padrão) — o que os SDKs iniciam
zode server --listen stdio://    # o mesmo, explicitado
zode server --listen ws://127.0.0.1:0   # WebSocket loopback + auth Bearer
zode server --listen off         # não inicia nada e sai
```

O server mode expõe o comportamento apoiado no zode:

- inicialização + descoberta de capacidades (com um `approvalPolicy` de `readOnly` (padrão) / `auto` / `prompt`)
- ciclo de vida de metadados de thread e **turnos em streaming** — saída do modelo e chamadas de ferramenta chegam como notificações JSON-RPC; `turn/interrupt` cancela um turno
- **aprovações interativas** — a política `prompt` dirige frames `approval/request` servidor→cliente respondidos com `allow` / `allowAlways` / `deny`
- read/write/create/stat/list/remove/copy de filesystem e `command/exec` de execução única
- list/set de modelos, read/list/write de config, e status somente leitura de skills, hooks, servidores MCP e listas de plugins

O transporte WebSocket faz bind apenas em loopback e grava um arquivo de credenciais `0600` em `<config-dir>/server.json` (`{port, pid, token}`); clientes autenticam com `Authorization: Bearer <token>`. Veja [`sdk/README.md`](../../sdk/README.md) para o protocolo completo, nomes de campos de notificação e exemplos por linguagem.

SDKs vivem em [`sdk/`](../../sdk/):

| SDK | Diretório | Teste local |
|-----|-----------|-------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

Cada SDK expõe um conjunto nativo de enum/constant `ProtocolMethod` para os nomes de métodos estáveis atuais, para que as integrações evitem strings JSON-RPC hardcoded. Todos os params, formato de result e nome de enum/constant do SDK de cada método suportado estão documentados na [referência de métodos de `sdk/`](../../sdk/README.md#method-reference).

## Teammates de CLI externas

Zode pode usar uma CLI de agente de terceiros instalada como worker Task único ou como teammate persistente ou stateless. O registro é deliberadamente manual: instalar uma CLI ou colocá-la no `PATH` **não** a expõe ao modelo. Adicione um profile em `externalAgents.agents` e depois inicie o Zode no projeto. Ou rode `/external-agents` para inspecionar as CLIs suportadas atualmente no `PATH` e depois `/external-agents discover` para adicionar explicitamente cada preset detectado à config global. Este comando é acionado pelo usuário; a inicialização nunca escaneia nem registra CLIs externas automaticamente.

| Profile de agente | Executável | Worker Task | Modo team | Sandbox da CLI externa |
|---|---|---:|---:|---|
| `claude-code` | `claude` | sim | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | sim | persistent | workspace-write |
| `opencode` | `opencode` | sim | stateless | unknown |
| `cline` | `cline` | sim | stateless | unrestricted |
| `antigravity` | `agy` | sim | stateless | unknown |
| `cursor` | `cursor-agent` | sim | persistent | unrestricted |
| `kiro` | `kiro-cli` | sim | stateless | unrestricted |
| `pi` | `pi` | sim | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | sim | persistent | unrestricted |

Todo profile registrado pode entrar em uma equipe. Profiles resumíveis preservam o session ID e a conversa da CLI entre atribuições; outras CLIs são teammates stateless que iniciam um processo novo a cada atribuição. Outras ferramentas, incluindo CLIs Grok alternativas, podem usar um profile custom.

### Adicionar um profile de CLI manualmente

Coloque `externalAgents` em `~/.zode/config.json` para todos os projetos, ou em `<project>/.zode/config.json` para um projeto. Um objeto vazio ativa explicitamente um preset conhecido e resolve seu executável no `PATH` higienizado:

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

Adicione apenas os profiles que pretende expor. Um `command` nu como `cline` é resolvido no `PATH`; caminhos como `./tools/my-agent` ou `/opt/agents/my-agent` também são aceitos. Presets conhecidos honram `enabled`, `command`, `extraArgs`, `envAllow` e `trusted`; `extraArgs` é anexado à invocação do preset.

Processos de CLI iniciam com um ambiente limpo contendo apenas `PATH`, `HOME` e `TERM` (mais as variáveis Windows necessárias), então adicione explicitamente API keys ou outras variáveis necessárias em `envAllow`. O estado de login existente sob `HOME` continua funcionando. Uma entrada de projeto com o mesmo nome de profile substitui a entrada global inteira, então repita todos os overrides que o projeto ainda precisa.

Um profile custom declara a invocação e o protocolo completos:

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

`promptTransport` é `stdin`, `argv` ou `file`; `argv` requer um argumento `{prompt}` isolado e `file` requer `{prompt_file}`. `output` é `text`, `jsonl` genérico, `jsonl-claude` ou `jsonl-codex`. Profiles JSONL genéricos usam pointers RFC 6901 `textSource` e `sessionIdSource` para extrair texto em streaming e um session ID resumível de qualquer evento. `resumeArgs` deve conter um token `{session_id}` isolado e é anexado nos turnos posteriores; `resumeFlag` é mantido como a forma abreviada `<flag> <session-id>`.

Se uma CLI aceita um session ID selecionado pelo chamador, `newSessionArgs` pode conter um token `{session_id}` isolado. O Zode gera um UUID, anexa os argumentos expandidos na primeira execução e usa `resumeArgs` nas atribuições posteriores. Isso também torna uma CLI de texto puro resumível sem parsear um ID de sua saída.

`effectiveSandbox` aceita `none`, `readOnly`, `workspaceWrite`, `unrestricted` ou `unknown` e é exibido no prompt de confiança.

### Contratar e trabalhar com o teammate

Peça ao leader em linguagem natural; `team_hire` e `team_send` são ferramentas do modelo, não slash commands:

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

O primeiro hire mostra o executável e os argumentos resolvidos, o diretório de trabalho e o sandbox efetivo da CLI. Aprová-lo delega trabalho a esse processo no projeto atual: o Zode controla o launch do processo, mas **não** controla cada edição de arquivo ou comando shell executado pela CLI externa. Grants de confiança duram a sessão Zode atual; o roster persistente é recuperado de `<cwd>/.zode/team/`, mas um teammate externo precisa ser confiado novamente após um restart ou mudança do executável.

Em execuções não interativas/bypass (incluindo `--yolo`), o Zode não pode mostrar o prompt de confiança e falha de forma fechada. Defina `externalAgents.agents.<profile>.trusted` como `true` apenas quando você deliberadamente quiser que esse profile rode sem o prompt.

## Agent team

Sobre a camada de agentes externos, o Zode pode montar uma equipe colaborativa de agentes internos e externos. Teammates são `Internal` (um QueryLoop in-process persistente sobre um MessageStore compartilhado, com seu próprio provider/model) ou `External` (uma CLI de agente registrada manualmente). O leader é o modelo raiz.

- **Ferramentas** (grupo `team`, `tools:team` para desabilitar): `team_hire`, `team_send` (verificação de ocupado → claim atômico → dispatch, retorna a resposta mais os relays `@ask`), `team_dismiss`, `team_list`, e as ferramentas de board/claim (`team_board_read/update/append`, `team_claim/release`).
- **Board** é gerenciado pelo host em `<cwd>/.zode/team/`: `board.json` escrito atomicamente sob um `board.lock` estável, atualizações de seção via CAS num contador de revisão. Claims são leases TTL cientes de subárvore, com identidade do holder injetada pelo host (nunca a partir do input da ferramenta), confinadas à cwd canônica.
- **Colaboração** é mediada pelo leader (no fim do turno, não ao vivo): teammates terminam uma linha de resposta com `@ask <name>: <question>`; o leader repassa. Plays (pipeline / debate / swarm) são orientações de prompt injetadas apenas quando o grupo team está ativo.

Use `/team` para inspecionar o roster e o board após contratar:

```text
/team                         # painel de roster + board
/team status                  # roster em texto
/team board                   # objetivo compartilhado, notas, atribuições e claims
/team dismiss implementer     # remove o teammate
```

## Slash commands

| Comando | O que faz |
|---|---|
| `/help` | Overlay de comandos + atalhos |
| `/clear` | Limpa a conversa (e o contexto) |
| `/model [id]` | Mostra / anota o modelo ativo |
| `/config` | Mostra modelo + diretório de trabalho |
| `/compact` | Status da auto-compactação de contexto |
| `/cost` | Uso de tokens e custo até agora (incl. sub-agents) |
| `/theme [id]` | Alterna tema (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Seletor de sessão — retoma numa nova aba com histórico |
| `/connect` | Conecta e alterna o provider ativo |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Mostra/oculta a barra lateral direita; dobra as seções MCP / modified-files / todo |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Painel e comandos de controle do navegador; pareia a extensão Chrome bridge ou alterna entre Chromium gerenciado e seu perfil Chrome |
| `/loop <interval> [--max N] <prompt>` | Roda um prompt recorrente na aba atual; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Persiste um prompt agendado; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Mostra a configuração, saúde e retries pendentes do watchdog de turnos em segundo plano |
| `/tasks` | Painel de shells em segundo plano, turnos em execução e saúde do watchdog |
| `/undo`, `/redo` | Desfaz / refaz a última edição de arquivo |
| `/mcp` | Gerencia servidores MCP — habilitar / desabilitar num diálogo |
| `/skills` | Lista as skills disponíveis |
| `/agents` | Gerencia sub-agents — criar (com IA ou manual) / deletar |
| `/external-agents [list\|discover]` | Lista CLIs externas suportadas no `PATH`, ou registra explicitamente cada preset detectado |
| `/team [status\|board\|dismiss <name>]` | Inspeciona o roster persistente de teammates e o board compartilhado, ou remove um teammate |
| `/workflows` | Gerencia e roda workflows scriptados em JS (orquestração `agent()`/`parallel()`/`pipeline()`, executada deterministicamente pelo zode) |
| `/effort` | Escolhe o nível de esforço de raciocínio |
| `/thinking`, `/tool-details` | Alterna a exibição de raciocínio / detalhe de chamadas de ferramenta |
| `/orchestration` | Alterna a orquestração autônoma de sub-agents + workflows |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | Mostra / controla o sandbox do OS em runtime |
| `/language` | Alterna o idioma da UI (15 locales) |
| `/export [path]` | Exporta o transcript para Markdown (um dir recebe um nome padrão) |
| `/yolo` | Modo bypass de aprovação |
| `/exit` | Sai |

Agents e skills criados, e ferramentas MCP conectadas, também aparecem como slash commands dinâmicos (por exemplo `/<name>`) e podem ser invocados diretamente.

## Atalhos de teclado

> No macOS os chords do app abaixo usam **`Cmd`** (⌘); no Windows/Linux usam `Ctrl`. `Ctrl+C/D/L/V` permanecem `Ctrl` em todos os lugares (convenções de terminal).

| Tecla | Ação |
|---|---|
| `Enter` | Envia a mensagem (enfileira se um turno está rodando) |
| `Shift`/`Alt`+`Enter` | Nova linha |
| `Up` / `Down` | Recupera o prompt enviado anterior / próximo (ou move a seleção do autocomplete) |
| `Ctrl+C` | Interrompe o turno (sai quando ocioso) |
| `Ctrl+D` | Sai |
| `Ctrl+L` | Redesenha a conversa a partir do store (recupera uma view apagada; use `/clear` para descartar) |
| `Ctrl+V` | Cola (texto ou caminhos de imagem) |
| `Cmd/Ctrl+O` | Settings |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | Nova aba / fecha aba |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Salta para / cicla abas |
| `Cmd/Ctrl+B` | Painel de tarefas em segundo plano |
| `Cmd/Ctrl+G` | Alterna a barra lateral |
| `F1` | Ajuda |
| `PgUp` / `PgDn` | Rola a conversa |
| `Home` / `End` | Salta para o topo / o mais recente da conversa |
| `Esc` | Fecha o overlay atual (ou interrompe um turno em execução) |

## Instruções do projeto

Zode lê instruções de uma hierarquia de três níveis (o mais recente ganha atenção): global `~/.zode/AGENTS.md` (ou `instructions.md`) → raiz do projeto → cwd. Em cada diretório ele prefere `AGENTS.md` a `CLAUDE.md`. Skills ficam em `.zode/skills/**/SKILL.md`; servidores MCP em `~/.zode/mcp.json` ⊕ `.mcp.json`; hooks em `~/.zode/hooks.json` ⊕ `.zode/hooks.json`.

**Configuração cross-agent.** Zode lê skills diretas e configuração MCP de Claude Code, Codex, Cursor, opencode, Gemini e agentes locais relacionados. Árvores de plugins instaladas e caches de plugins pertencentes a esses produtos nunca são escaneados. Para reutilizar um plugin, instale sua fonte explicitamente com `zode plugin install ... --trust`; os formatos de pacote do Codex e do Claude Code continuam suportados para plugins instalados através do Zode.

## Configurar servidores MCP

Servidores MCP vivem na mesma config de precedência aninhada de tudo o mais — `~/.zode/mcp.json` para todos os projetos, `.mcp.json` ou `.zode/mcp.json` na raiz do projeto para escopar um a um repositório. Sem registro, sem restart-and-pray: edite o arquivo, depois `/mcp` (ou relance) para carregá-lo.

### stdio (inicia um servidor local)

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

`command`/`args` iniciam o servidor como subprocesso via stdio. Os valores de `env` suportam substituição `$NAME` / `${NAME}` contra o próprio ambiente do processo zode (expandida logo antes de conectar, não escrita em disco) — útil para manter tokens fora do próprio arquivo de config.

### Streamable HTTP (servidor remoto)

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

`"transport": "http"` conecta com o transporte Streamable HTTP da spec MCP atual — uma única `url`, sem endpoint SSE separado para configurar. `"sse"` é aceito como grafia equivalente (algumas configs — e os próprios docs de setup de servidores MCP — ainda o chamam assim); ambos resolvem para o mesmo conector. Os `headers` são encaminhados verbatim (incluindo `Authorization`, então esquemas Bearer/Basic/custom funcionam) e suportam a mesma substituição `$VAR` de `env`. Adicione `"enabled": false` a qualquer servidor para manter sua definição sem conectá-lo — `/mcp` também alterna isso por servidor sem editar o arquivo à mão.

### Usando

Toda ferramenta que um servidor conectado expõe aparece como `mcp__<server>__<tool>`, chamável pelo agente como qualquer ferramenta integrada (e `@`-mencionável na caixa de input). `/mcp` abre um diálogo listando cada servidor descoberto — conectado / desconectado / desabilitado — com Space para alternar um; a seção `mcp` recolhível da barra lateral (clique no cabeçalho ▼, ou `/sidebar mcp`) espelha o mesmo estado de conexão ao vivo.

Zode também lê configuração MCP direta de Claude Code, Codex, Cursor, opencode e Gemini. A configuração da home é tratada como o setup do usuário; definições MCP externas locais ao projeto são descobertas desabilitadas e podem ser habilitadas via `/mcp`. Declarações MCP enterradas na árvore de plugins instalada de outro produto não são escaneadas. `openpencil` é reservado — o op-bridge o dirige nativamente, então qualquer servidor declarado com esse nome é ignorado.

## Instalar skills e commands em Markdown

Ambos são Markdown simples em disco — sem registro, sem passo de build. Solte um arquivo e ele fica ativo no próximo launch (ou `/skills` para checar o que carregou).

### Instalar uma skill

Uma skill é uma pasta com um `SKILL.md` dentro. Coloque-a sob o projeto (`.zode/skills/`) ou seu diretório home (`~/.zode/skills/`):

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

A skill agora aparece em `/skills`, o agente pode invocá-la por conta própria via a ferramenta Skill, e ela também vira um slash command dinâmico — digitar `/code-review look at src/lib.rs` expande para um prompt que roda a skill. Arquivos extras ao lado do `SKILL.md` (referências, scripts) acompanham a skill. Diretórios de skills diretas pertencentes a Claude Code, Codex, opencode, Cursor e agentes relacionados são escaneados. Skills enterradas nas árvores de plugins ou caches instalados desses produtos não são; instale o plugin explicitamente através do Zode se quiser usá-lo aqui.

### Instalar um command (prompt em Markdown)

Um slash command customizado é um único arquivo `.md` cujo **nome do arquivo é o nome do comando** e cujo corpo é o prompt que ele submete. Qualquer coisa que você digitar após o comando é anexada ao corpo:

```bash
mkdir -p .zode/commands            # ou ~/.zode/commands para todos os projetos
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

Agora `/changelog` submete esse prompt, e `/changelog only the sidebar work` anexa seus argumentos após ele. Comandos em `~/.claude/commands` e `~/.codex/commands` (e seus equivalentes em nível de projeto) também são carregados; comandos dentro de uma *árvore de plugin externa* ficam desligados por padrão — copie o `.md` para um diretório `.zode/commands/` para optar por eles.

## Ecossistema ZSeven-W

Zode faz parte do stack ZSeven-W de ferramentas AI-native para desenvolvimento:

| Produto | O que é |
|---------|---------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Runtime async em Rust puro para LLM agents, com streaming multi-provider, tool dispatch, permissões, MCP, cost tracking, attachments, sessions e coding tools opcionais. |
| [`jian`](https://github.com/ZSeven-W/jian) | Framework UI cross-platform nativo de Rust em que um arquivo `.op` é um app, ligando artefatos de design estilo OpenPencil a software executável. |
| [`noema`](https://github.com/ZSeven-W/noema) | Sistema de memória local-first e non-vector para coding agents, com lexical recall, review queues, MCP, S3 offload e enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Ferramenta open-source AI-native de design vetorial para workflows design-as-code, transformando prompts em UI no live canvas com concurrent agent teams. |

## Benchmark

Os benchmarks de Zode cobrem code generation one-shot, fluxo agentic de ler/rodar/editar/corrigir, tarefas multiarquivo, bugs difíceis, MCP/Skills/constraint following e o Noema LOCOMO runner. Em todas as dimensões, **Zode + DeepSeek-v4-pro empata com o Claude**, com cada tarefa avaliada por um grader oculto. A metodologia completa, os comandos de reprodução e as tabelas de resultados estão na seção [Benchmark do README em inglês](../../README.md#benchmark), e as suítes vivem em [`benchmarks/`](../../benchmarks/).

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

Contribuições são bem-vindas. Use [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`, com scopes comuns como `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`.

## Licença

[MIT](../../LICENSE) &copy; ZSeven-W
