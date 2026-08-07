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
  <strong>Asistente de programación open-source y AI-native para tu terminal.</strong><br/>
  Lee tu código, ejecuta comandos, busca archivos y gestiona git desde una TUI rápida en Rust.
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

> Este README localizado cubre la visión general y el inicio rápido. El [README en inglés](../../README.md) sigue siendo la referencia para los detalles completos de benchmarks y notas largas actualizadas.

## Puntos destacados

- **Múltiples proveedores**: Anthropic, OpenAI y cualquier API compatible con OpenAI (dialectos de DeepSeek, Moonshot, OpenRouter), además de Ollama local. Admite modelos de gran salida y de **contexto de 1M** (`contextWindow` / `maxOutputTokens` son configurables).
- **Amplia superficie de herramientas**: lectura/escritura/edición de archivos (incluido `MultiEdit` atómico multi-hunk), búsqueda de código y contenido, shells en primer y segundo plano, git, web fetch (más `WebSearch` opcional con una clave de Tavily), notebooks y seguimiento de TODOs.
- **Control del navegador**: las herramientas `browser_*` integradas manejan un Chromium gestionado o tu perfil real de Chrome mediante la extensión Chrome bridge de zode: navegar, hacer clic/escribir, inspeccionar el DOM, capturar pantallas, leer logs de console/network y agrupar las pestañas que abre zode. El emparejamiento se hace una sola vez: la extensión se reconecta automáticamente entre reinicios de zode.
- **Control del escritorio**: las herramientas `desktop_*` conducen aplicaciones nativas a través de las APIs de accesibilidad del sistema (AX en macOS, UIA en Windows, AT-SPI en Linux, CDP en Electron), con un cursor fantasma y parada global con **Esc**.
- **Permisos sin bloqueo**: toda herramienta con efectos secundarios pasa por aprobación (allow once / always / deny), pero el prompt se acopla en línea y nunca te bloquea: puedes seguir escribiendo para encolar lo siguiente mientras una herramienta espera, con reglas de denegación estricta.
- **Sandbox del sistema operativo, activado por defecto**: los comandos shell se ejecutan bajo sandbox-exec (macOS) / bwrap (Linux) en modo `read-only` o `workspace-write`, con **red saliente denegada por defecto**. Alterna en vivo con `/sandbox`; el modelo puede solicitar una excepción para un solo comando (`dangerouslyDisableSandbox`) que **tú autorizas** en el prompt.
- **TUI de pantalla completa**: Markdown en streaming con resaltado de sintaxis, vistas de diff, autocompletado de slash commands, historial de prompts (Up/Down), 11 temas integrados, paneles de settings/help, secciones resilientes en la barra lateral derecha y UI en **15 idiomas** (`/language`).
- **Sesiones duraderas y compatibles con V1**: mantiene el contrato de transcript `<id>.jsonl` existente mientras añade journals, checkpoints, rewind, fork y Git worktrees aislados como datos laterales. La compactación de contexto nunca pierde la conversación visible: las sesiones reanudadas reproducen todo el historial previo a la compactación mientras el contexto del modelo se mantiene compacto.
- **Superficies de automatización**: salida headless estable en JSON/JSONL, direccionamiento exacto de sesiones, filtros de herramientas, códigos de salida deterministas, ACP sobre stdio y un panel de operaciones local.
- **Pestañas multi-sesión**: ejecuta varias conversaciones en paralelo (`Ctrl+T`), cada una un agente aislado; reanuda sesiones anteriores con reproducción completa del historial.
- **Sub-agentes, equipos y workflows**: delega trabajo puntual mediante la herramienta Task, contrata teammates internos o de CLI externas persistentes, coordínalos con un board compartido y claims de archivos, y administra todo con `/agents`, `/team` y `/workflows`.
- **Configuración local portable**: lee la configuración directa de skills y MCP de Claude Code, Codex, Cursor, opencode y Gemini, sin importar nunca sus árboles de plugins instalados ni sus cachés.
- **Skills y MCP**: carga paquetes de instrucciones `SKILL.md` bajo demanda y conecta servidores MCP (`mcp__<server>__<tool>`); los agentes, skills y herramientas MCP creados aparecen como slash commands.
- **Hooks**: ejecuta scripts externos en eventos de herramientas (p. ej. bloquear comandos peligrosos, ejecutar linters tras editar).
- **Instrucciones de tres niveles**: global (`~/.zode/`) → raíz del proyecto → cwd (`AGENTS.md` / `CLAUDE.md`).

## Instalación

### Una línea (binarios precompilados)

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

El instalador detecta tu OS y CPU, descarga el binario correcto desde el último [release](https://github.com/ZSeven-W/zode/releases) y coloca `zode` en tu `PATH`. Para fijar una versión o cambiar la ubicación:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

### Descarga manual

Descarga el archivo para tu plataforma desde la [página de releases](https://github.com/ZSeven-W/zode/releases):

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Después descomprime y mueve `zode` a tu `PATH` (`sudo mv zode /usr/local/bin/`). Los builds de Linux usan glibc; los binarios de macOS no están firmados (`xattr -dr com.apple.quarantine ./zode` si Gatekeeper avisa).

### Desde código fuente

Requiere Rust 1.88 o más reciente:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binario en target/release/zode
```

> El runtime del agente vive en el git submodule `vendor/agent`; clona siempre con `--recurse-submodules` (o ejecuta `git submodule update --init`).

## Inicio rápido

La forma más sencilla es iniciar `zode` y ejecutar **`/connect`**: un selector interactivo respaldado por models.dev que escribe la configuración por ti.

Para escribir `~/.zode/config.json` a mano: **`providers`** es la fuente de verdad —una entrada por proveedor (credenciales compartidas) que contiene uno o varios **models**— y el **`provider`** de nivel superior registra el modelo *activo*:

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // protocolo: "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // el modelo activo
}
```

Los proveedores compatibles con OpenAI (DeepSeek, Moonshot, OpenRouter, …) añaden `baseUrl` + `dialect`, y los ajustes por modelo viven en la entrada de cada modelo:

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

Una sola entrada de proveedor puede contener varios modelos; cambia entre ellos en vivo con `/model`.

Después ejecuta:

```bash
zode                       # TUI de pantalla completa
zode -p "explain main.rs"  # headless: un prompt, stream a stdout, salir
zode --no-tui              # REPL readline simple
zode -c                    # continuar la sesión más reciente
zode -r <id>               # reanudar una sesión por prefijo de id
zode --yolo                # omitir prompts de aprobación (las reglas de denegación siguen vigentes)
zode --no-sandbox          # desactivar el sandbox del OS (está activado por defecto)
zode --sandbox-read-only   # sandbox en modo solo lectura (denegar toda escritura)
zode --sandbox-allow-network  # permitir red saliente dentro del sandbox
zode --browser             # forzar la activación de las herramientas de navegador en esta ejecución
zode --no-browser          # desactivar las herramientas de navegador en esta ejecución
zode --model <id>          # sobrescribir el modelo
zode --provider <name>     # elegir un proveedor con nombre de config.providers
zode server                # modo app-server JSON-RPC sobre stdio
zode acp                   # agente Agent Client Protocol sobre stdio
zode dashboard             # resumen local de sesiones/checkpoints/worktrees
```

También puedes apuntar a cualquier proveedor sin editar la configuración exportando la clave correspondiente (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …); para Ollama la `baseUrl` se toma del entorno cuando no se define.

## Teammates de CLI externas

Zode puede usar una CLI de agente de terceros instalada como worker Task de una sola ejecución o como teammate persistente o sin estado. El registro es deliberadamente manual: instalar una CLI o ponerla en `PATH` **no** la expone al modelo. Añade un profile bajo `externalAgents.agents` y luego inicia Zode en el proyecto. También puedes ejecutar `/external-agents` para inspeccionar las CLI compatibles que hay ahora en `PATH`, y luego `/external-agents discover` para añadir explícitamente cada preset detectado a la configuración global. Este comando lo lanza el usuario; el inicio nunca escanea ni registra CLIs externas automáticamente.

| Profile del agente | Ejecutable | Task worker | Modo team | Sandbox de la CLI externa |
|---|---|---:|---:|---|
| `claude-code` | `claude` | sí | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | sí | persistent | workspace-write |
| `opencode` | `opencode` | sí | stateless | unknown |
| `cline` | `cline` | sí | stateless | unrestricted |
| `antigravity` | `agy` | sí | stateless | unknown |
| `cursor` | `cursor-agent` | sí | persistent | unrestricted |
| `kiro` | `kiro-cli` | sí | stateless | unrestricted |
| `pi` | `pi` | sí | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | sí | persistent | unrestricted |

Todo profile registrado puede unirse a un team. Los profiles resumibles preservan el session ID de la CLI y la conversación entre asignaciones; las demás CLIs son teammates sin estado que arrancan un proceso nuevo en cada asignación. Los presets usan las interfaces headless documentadas de [Cline](https://docs.cline.bot/usage/cli-overview), [Antigravity](https://antigravity.google/docs/cli-best-practices), [Cursor](https://cursor.com/docs/cli/headless), [Kiro](https://kiro.dev/docs/cli/headless/), [Pi](https://pi.dev/docs/latest) y el [Grok Build](https://docs.x.ai/build/cli/headless-scripting) de xAI. Otras herramientas, incluidas CLIs alternativas de Grok, pueden usar un profile custom.

### Añadir un profile de CLI manualmente

Pon `externalAgents` en `~/.zode/config.json` para todos los proyectos, o en `<project>/.zode/config.json` para un solo proyecto. Un objeto vacío habilita explícitamente un preset conocido y resuelve su ejecutable en el `PATH` saneado:

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

Añade solo los profiles que tengas intención de exponer. Un `command` simple como `cline` se resuelve en `PATH`; también se aceptan rutas como `./tools/my-agent` o `/opt/agents/my-agent`. Los presets conocidos respetan `enabled`, `command`, `extraArgs`, `envAllow` y `trusted`; `extraArgs` se añade a la invocación del preset de Zode.

Los procesos de CLI arrancan con un entorno limpio que contiene solo `PATH`, `HOME` y `TERM` (más las variables requeridas por Windows), así que añade explícitamente a `envAllow` las API keys u otras variables necesarias. El estado de login existente bajo `HOME` sigue funcionando. Una entrada de proyecto con el mismo nombre de profile reemplaza la entrada global completa, así que repite cada override que el proyecto siga necesitando.

Un profile custom declara la invocación y el protocolo completos:

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

`promptTransport` es `stdin`, `argv` o `file`; `argv` requiere un argumento `{prompt}` independiente y `file` requiere `{prompt_file}`. `output` es `text`, `jsonl` genérico, `jsonl-claude` o `jsonl-codex`. Los profiles JSONL genéricos usan punteros RFC 6901 `textSource` y `sessionIdSource` para extraer el texto en streaming y un session ID resumible de cualquier evento. `resumeArgs` debe contener un token `{session_id}` independiente y se añade en los turnos posteriores; `resumeFlag` se mantiene como forma abreviada `<flag> <session-id>`.

Si una CLI acepta un session ID elegido por el llamador, `newSessionArgs` puede contener un token `{session_id}` independiente. Zode genera un UUID, añade los argumentos expandidos en la primera ejecución y usa `resumeArgs` en las asignaciones posteriores. Esto también hace resumible a una CLI de texto plano sin necesidad de parsear un ID de su salida.

Esto permite que cualquier CLI headless se convierta en un Task worker o teammate sin estado. Para preservar el contexto de conversación entre asignaciones de team, además debe exponer un session ID, o aceptarlo mediante `newSessionArgs`, más una invocación de resume no interactiva.

`effectiveSandbox` acepta `none`, `readOnly`, `workspaceWrite`, `unrestricted` o `unknown` y se muestra en el prompt de confianza.

### Contratar y trabajar con el teammate

Pídeselo al leader en lenguaje normal; `team_hire` y `team_send` son tools del modelo, no slash commands:

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

El primer hire muestra el ejecutable y los argumentos resueltos, el directorio de trabajo y el sandbox efectivo de la CLI. Aprobarlo delega el trabajo a ese proceso en el proyecto actual: Zode controla el lanzamiento del proceso, pero **no** controla cada edición de archivo o comando shell que ejecuta la CLI externa. Los trust grants duran la sesión Zode actual; el roster persistente se recupera de `<cwd>/.zode/team/`, pero un teammate externo debe volver a recibir confianza tras un reinicio o un cambio de ejecutable.

En ejecuciones no interactivas o de bypass (incluido `--yolo`), Zode no puede mostrar el prompt de confianza y falla de forma cerrada. Establece `externalAgents.agents.<profile>.trusted` en `true` solo cuando quieras deliberadamente que ese profile se ejecute sin el prompt.

Usa `/team` para inspeccionar el roster y el board tras contratar:

```text
/team                         # panel de roster + board
/team status                  # roster en texto
/team board                   # objetivo compartido, notas, asignaciones y claims
/team dismiss implementer     # eliminar el teammate
```

## Automatización, sesiones duraderas y operaciones

### Ejecuciones headless estructuradas

`-p`, `--prompt-file` y `--prompt-json` usan el mismo motor headless. `json` emite un único objeto de resultado final; `stream-json` emite un objeto JSON `zode.run-event.v1` por línea. Los modos estructurados reservan stdout para la salida legible por máquina y usan códigos de salida estables: `0` éxito, `10` error de provider, `11` permiso denegado, `12` límite de turnos alcanzado, `13` interrumpido (Ctrl-C), `14` resultado parcial, `15` error de direccionamiento de sesión.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# Los IDs exactos no hacen prefix-match. Un fork nunca muta su sesión de origen.
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Los patrones de deny de herramientas ganan sobre los de allow y los heredan los sub-agentes Task. `--permission-mode` acepta `default`, `dont-ask`, `accept-edits` y `bypass`; `--yolo` sigue siendo un atajo para bypass, mientras que las reglas de denegación estricta siguen vigentes.

### Sesiones compatibles con V1, checkpoints y worktrees

El transcript sigue siendo el archivo V1 original en `~/.zode/sessions/<id>.jsonl`. Es la **única** copia del transcript, de modo que clientes Zode antiguos pueden seguir leyéndolo y escribiéndolo. Los metadatos nuevos son aditivos y viven en `~/.zode/sessions/<id>/` (`meta.json`, journal, checkpoints y snapshots). No se requiere un nuevo formato de sesión ni migración del transcript.

```bash
zode session list
zode session list --json
zode session show <id>                         # metadatos + IDs de checkpoint
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # vista previa consciente de conflictos
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

Se captura un checkpoint antes de un turno con efectos secundarios. Rewind restaura el contenido de los archivos rastreados y el prefijo del transcript, informa de conflictos en lugar de sobrescribir cambios más recientes, y registra una nueva rama lógica del journal en vez de borrar historial. Los forks de worktree se pueden aplicar de vuelta explícitamente cuando el experimento está listo.

**La compactación nunca pierde la conversación visible.** Cuando la compactación de contexto reemplaza mensajes antiguos por un resumen, los originales se conservan en un sidecar aditivo (`~/.zode/sessions/<id>/compacted.jsonl`). Reanudar una sesión, pulsar `Ctrl+L`, `/export` y el panel lateral de Chrome muestran todo el historial previo a la compactación, mientras el modelo sigue recibiendo solo el contexto compactado. Los forks llevan el archivo (filtrado a su propio transcript), `/clear` lo elimina, y borrar una sesión elimina el sidecar completo.

### Reglas de permisos y profiles de sandbox

Las reglas pueden vivir bajo `permissions.rules` en `config.json`, o en un archivo JSON independiente pasado con `--rules`. Un matcher de campo usa un JSON pointer RFC 6901; deny tiene precedencia sobre ask, que tiene precedencia sobre allow. El archivo independiente debe ser un array de reglas o `{ "rules": [...] }`; no se envuelve en un objeto `permissions` de nivel superior.

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

Los profiles integrados son `read-only`, `workspace`, `workspace-network` y `unconfined`. Los profiles definidos en configuración usan los mismos campos de sandbox mostrados arriba.

En Windows, el sandbox del OS aplica niveles equivalentes usando la contención nativa de la plataforma; cuando un nivel no puede verificarse, el inicio falla de forma cerrada igual que en macOS/Linux, y `--no-sandbox` es el override explícito.

### Plugins y marketplaces estáticos

Un plugin gestionado puede aportar skills, commands, agents, hooks, servidores MCP, servidores LSP y renderizadores de UI en JavaScript en sandbox. Zode acepta `plugin.json`, `.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.grok-plugin/plugin.json` y `.claude-plugin/plugin.json`. Se admiten los arrays de rutas de componentes de Codex y Claude Code, y se respeta el `defaultEnabled` de Claude Code en la primera instalación. Los componentes exclusivos del host, como las apps/connectors de Codex y los themes, monitors u output styles de Claude Code, se ignoran; un plugin solo-app se rechaza porque no tiene ningún componente compatible con Zode. Las instalaciones son snapshots inmutables con procedencia y un tree hash SHA-256. El contenido ejecutable de un plugin nunca se activa sin el flag explícito `--trust`.

#### Inicio rápido de un plugin de UI en JavaScript

El plugin de UI más pequeño contiene un manifest y un archivo JavaScript:

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

Instala un directorio local o un repositorio/subdirectorio de GitHub, luego reinicia un proceso Zode en ejecución para que cargue el nuevo snapshot:

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Usa `zode plugin update my-plugin` tras cambiar el código fuente. `--trust` es obligatorio porque JavaScript, hooks, servidores MCP y el acceso de red declarado son capacidades ejecutables. La instalación y la actualización imprimen el grant de permisos declarado por el plugin (hosts de red, variables de entorno, scopes de contexto). Una actualización cuyo manifest solicite permisos *más amplios* que el snapshot instalado se rechaza a menos que la vuelvas a ejecutar con `--trust` — una fuente Git móvil no puede ampliar silenciosamente su propio grant.

#### API de renderizado de UI

Los plugins de UI pueden aportar filas declarativas justo encima de la versión en la barra lateral — como máximo seis filas en total, compartidas entre todos los plugins por orden de carga. Declara un entrypoint JavaScript en el manifest:

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

Registra un renderizador síncrono con `zode.ui.sidebar`. El contexto es un snapshot JSON de solo lectura con campos de terminal, sesión, modelo, estado, token y ventana de contexto. El resultado lo renderiza Zode; los scripts no reciben acceso al sistema de archivos, la red, la terminal ni un puente Ratatui.

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

Los tonos admitidos son `default`, `muted`, `accent`, `success`, `warning` y `danger`; los spans también aceptan `bold` e `italic`. Un renderizador debe ser síncrono. Cada script está limitado a 256 KiB, 8 MiB de memoria JS y 25 ms por evaluación, y los renderizadores se reevalúan como máximo cada 250 ms (la salida en caché se reutiliza entre evaluaciones). La salida de la barra lateral está limitada a 6 líneas por renderizador (6 en total entre todos los plugins), cada línea a 16 spans y 2.048 bytes de texto. El host sanea los caracteres de control.

La barra de estado también es extensible. Permanece en una fila cuando ningún plugin devuelve contenido y crece a dos filas dinámicamente cuando un renderizador síncrono `zode.ui.statusLine` devuelve spans. Zode mantiene su estado central y sus indicadores de seguridad en la primera fila; la salida del plugin se compone en la segunda.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Contexto de renderizado y permisos

Cada renderizador recibe los siguientes campos base sin solicitar permiso de contexto adicional:

| Campo | Forma y significado |
| --- | --- |
| `ctx.apiVersion` | Versión de la API de contexto; actualmente `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | `{ width, height }` en celdas de terminal. |
| `ctx.session` | `{ id, title, cwd, busy }` de la tarea activa. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }`; `sandbox` contiene `{ enabled, readOnly, network }`. |
| `ctx.tokens` | Contadores de token `{ input, output }`. |
| `ctx.context` | `{ used, window, usedPercent }`; el porcentaje puede ser `null`. |
| `ctx.data` | Resultados que pertenecen solo a las fuentes de datos registradas por este plugin. |

Las secciones más ricas se omiten a menos que el plugin solicite el scope correspondiente en `permissions.context`:

| Scope | Campo expuesto | Forma y límites |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }`; `active` empieza en 1. |
| `workspace` | `ctx.workspace.modifiedFiles` | Hasta 50 entradas Git `{ path, added, removed }`. |
| `tools` | `ctx.tools.available` | Nombres ordenados de las herramientas habilitadas para la tarea activa. |
| `tools` | `ctx.tools.active` | Nombres de las herramientas que se están ejecutando. |
| `tools` | `ctx.tools.recent` | Hasta 20 registros `{ name, status, durationMs }`. |
| `tasks` | `ctx.tasks.todoStatuses` | Solo los estados de los todos, sin el texto. |
| `tasks` | `ctx.tasks.subagents` | Registros `{ type, status }`, sin prompts ni transcripts. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, sin el texto del goal. |
| `services` | `ctx.services.mcp` | Registros `{ name, connected }`. |
| `services` | `ctx.services.lsp` | Registros `{ language, running }`. |

Por ejemplo:

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` es una API de observación: le indica a un renderizador qué herramientas existen y cuáles están o estuvieron en ejecución. Los plugins de UI no pueden invocar una herramienta. Las entradas y salidas de herramientas, los prompts, el contenido del transcript, el texto de todos/goals, los valores de entorno y las credenciales no se incluyen, y la API no puede eludir el sistema de aprobación de Zode.

#### Datos HTTP en segundo plano

Los plugins de UI también pueden registrar fuentes de datos HTTP en segundo plano. El acceso a red y a secretos debe declararse en el manifest:

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

La petición es declarativa y se ejecuta fuera de la ruta de renderizado. Las variables de entorno secretas las ensambla Zode en los headers y nunca se exponen a JavaScript:

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

`zode.data.define(key, config)` acepta una clave alfanumérica de 1–64 caracteres, con guiones bajos o guiones. `request` admite `url`, `method`, `headers`, un `body` JSON opcional y `timeoutMs`. Los valores por defecto son `GET`, 3 segundos de timeout y 60 segundos de refresco. Solo se aceptan `GET` y `POST` sobre HTTPS. Los headers literales son cadenas; un header secreto usa `{ "env": "NAME", "prefix": "Bearer " }`. La variable de entorno también debe aparecer en `permissions.env`, la lee solo Rust al construir la petición y nunca se devuelve a JavaScript.

Zode desactiva redirecciones y proxies, valida y fija direcciones DNS públicas, rechaza localhost/redes privadas, limita las respuestas a 256 KiB, restringe los timeouts de petición a 500 ms–10 segundos y los intervalos de refresco a 10 segundos–1 hora. Un comodín como `*.example.com` coincide con subdominios pero no con el host desnudo `example.com`.

Cada plugin ve solo sus propios datos. `ctx.data.<key>` contiene `{ ok, status, data, updatedAt }` o `{ ok: false, error, updatedAt }`. Las respuestas JSON se convierten en objetos/arrays; las no JSON en cadenas. Un status HTTP de error sigue incluyendo `status` y `data`, con `ok: false`.

Inicia Zode con el secreto requerido en su entorno cuando uses una API privada de cuota o coding-plan:

```bash
CODING_PLAN_TOKEN=... zode
```

El [ejemplo completo y ejecutable](../../examples/plugins/zode-ui-demo/) muestra actividad de modelo/contexto/herramientas en la barra lateral y la barra de estado, y usa `zode.data.define` para una cuota pública de la API de GitHub.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# Un marketplace es un índice estático local/Git, no un servicio hospedado por Zode.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # desambigua si hace falta
zode plugin marketplace update
```

### ACP, dashboard, telemetría y tests de regresión de TUI

`zode acp` implementa ACP initialize/new/load/fork/prompt/cancel sobre stdio, transmite actualizaciones de mensaje/thought/tool, solicita permisos a través del cliente y acepta servidores MCP stdio, HTTP y SSE proporcionados por el cliente. Los datos de sesión usan el mismo store compatible con V1 que la TUI y la CLI headless.

```bash
zode acp
zode dashboard
zode dashboard --json
```

La exportación OTLP está desactivada por defecto y requiere un opt-in explícito. Exporta solo atributos de ciclo de vida/nombre de herramienta/estado/uso sin contenido: los prompts, el texto generado, las entradas/salidas de herramientas, las rutas de archivo y los mensajes de error nunca se envían.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

Para escenarios de regresión de TUI en terminal real, el workspace incluye un harness PTY + VT100 que registra diagnósticos crudos y snapshots de pantalla virtual:

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` conduce la terminal real con esperas ordenadas, entrada de teclas, resizes y snapshots (la notación de teclas admite `<Enter>`, `<Esc>`, `<Tab>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>` y `<C-l>`):

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

Esta implementación local/abierta deliberadamente no incluye cuentas, facturación específicas de xAI ni un servicio de marketplace en la nube operado por Zode.

Claves de configuración opcionales de nivel superior (todas con valores por defecto sensatos):

```jsonc
{
  "maxOutputTokens": 16384,      // tope de salida por turno (súbelo para escrituras grandes)
  "contextWindow": 1000000,      // ventana de contexto del modelo — pon 1000000 para un modelo de 1M
  "temperature": 0,              // más bajo = más determinista
  "language": "zh-CN",           // idioma de la UI (15 locales); también con /language
  "effort": "medium",            // esfuerzo de razonamiento; en Anthropic, medium/high se asignan a presupuestos de thinking reales
  "autonomousOrchestration": true, // orquestación de sub-agentes + workflows (activado por defecto)
  "subagentMaxIterations": 0,      // guard opcional de hijos; omitido/0 = sin límite
  "tools": {
    "deferNonCore": false        // true: mantener visibles ~20 herramientas cotidianas y diferir el resto tras ToolSearch
  },
  "webSearch": {
    "tavilyApiKey": null         // habilita la herramienta WebSearch (o define $TAVILY_API_KEY)
  },
  "sandbox": {
    "enabled": true,             // sandbox del OS para comandos shell (activado por defecto)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // permitir red saliente dentro del sandbox
    "writableRoots": []          // directorios escribibles extra (workspace-write)
  },
  "browser": {
    "enabled": true,             // herramientas browser_* y panel /browser (activado por defecto)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // modo de lanzamiento del Chromium gestionado
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // vigilar turnos /loop y /schedule desatendidos
    "inactivityTimeoutSecs": 900, // abortar tras 15 minutos sin actividad de provider/tool
    "maxRuntimeSecs": 3600,      // tope absoluto de una hora por turno en segundo plano
    "abortGraceSecs": 10,        // esperar la cancelación cooperativa antes del hard-stop
    "maxRetries": 3,             // intentos de recuperación consecutivos antes de agotarse
    "initialBackoffSecs": 5,     // primer retardo de reintento
    "maxBackoffSecs": 300        // tope del backoff exponencial de reintentos
  }
}
```

> El sandbox confina los comandos shell (macOS: sandbox-exec; Linux: `bwrap`, que debe estar instalado). El inicio falla de forma cerrada si el sandbox configurado no puede verificarse; usa el flag explícito `--no-sandbox` para ejecutar sin él. La red está denegada por defecto. Si un comando realmente necesita salir, el modelo pone `dangerouslyDisableSandbox: true` y **tú** lo autorizas en el prompt de aprobación — o alternas todo el sandbox en vivo con `/sandbox`.

> `contextWindow` gobierna la auto-compactación — ajústalo a la ventana real de tu modelo (p. ej. `1000000`). Prefiere el valor **por modelo** en `providers.<name>.models.<id>.contextWindow` (tiene precedencia); la clave de nivel superior anterior es un fallback global, y zode también la rellena desde el catálogo empaquetado de models.dev cuando ninguna está definida. **No** lo pongas por encima de la ventana real: sobreestimarlo hace que las peticiones desborden y el provider rechace el turno.

## Server mode y SDKs

`zode server` inicia un servidor JSON-RPC delimitado por saltos de línea sobre stdin/stdout. Está pensado para integraciones con editores, automatización local, tests y clientes SDK que quieran las capacidades existentes de zode sin lanzar la TUI.

```bash
zode server                      # stdio (por defecto) — lo que lanzan los SDKs
zode server --listen stdio://    # lo mismo, escrito por extenso
zode server --listen ws://127.0.0.1:0   # WebSocket loopback + auth Bearer
zode server --listen off         # no iniciar nada y salir
```

El modo servidor expone el comportamiento respaldado por zode:

- inicialización + descubrimiento de capacidades (con un `approvalPolicy` de `readOnly` (por defecto) / `auto` / `prompt`)
- ciclo de vida de metadatos de thread y **turnos en streaming** — la salida del modelo y las llamadas a herramientas llegan como notificaciones JSON-RPC; `turn/interrupt` cancela un turno
- **aprobaciones interactivas** — la policy `prompt` conduce frames server→client `approval/request` que se responden con `allow` / `allowAlways` / `deny`
- lectura/escritura/creación/stat/list/remove/copy del sistema de archivos y `command/exec` de una sola vez
- list/set de modelo, read/list/write de configuración, y skills, hooks, estado de servidores MCP y listas de plugins de solo lectura

El transporte WebSocket enlaza solo a loopback y escribe un archivo de credenciales `0600` `<config-dir>/server.json` (`{port, pid, token}`); los clientes se autentican con `Authorization: Bearer <token>`. Consulta [`sdk/README.md`](../../sdk/README.md) para el protocolo completo, los nombres de campos de notificación y ejemplos por lenguaje.

Específicamente para este protocolo app-server, quedan fuera de alcance la gestión de marketplaces hospedados, el control remoto, Realtime, el spawn de procesos independientes, las terminales en segundo plano, el archive/fork de threads, los goals y los app connectors. Los comandos locales de sesión y de marketplace de plugins estáticos documentados arriba son superficies CLI separadas.

Los SDKs viven bajo [`sdk/`](../../sdk/):

| SDK | Directorio | Test local |
|-----|-----------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

Cada SDK expone un conjunto nativo de enum/constantes `ProtocolMethod` para los nombres de método estables actuales, de modo que las integraciones eviten cadenas JSON-RPC codificadas a mano. Los params, la forma del resultado y el nombre del enum/constante de cada método soportado están documentados en la [referencia de métodos de `sdk/`](../../sdk/README.md#method-reference).

Ejecuta las comprobaciones de SDK disponibles en tu máquina con:

```bash
scripts/test-sdks.sh
```

Las fixtures del protocolo se generan desde `zode-app-server-protocol`:

```bash
cargo run -p zode-app-server-protocol --bin export -- sdk/fixtures/jsonrpc
```

## Control del navegador

Zode incluye un grupo `tools:browser` para automatización del navegador. El agente puede usar `browser_read` para capturas de pantalla, snapshots del DOM, logs de console, logs de network y lecturas de pestañas; `browser_act` para navegación, clics, escritura, pulsaciones de teclas y scroll; `browser_eval` para JavaScript; y `browser_tabs` para gestión de pestañas. La inspección de solo lectura del navegador no requiere aprobación; las acciones que modifican el navegador usan el mismo flujo de aprobación allow-once / always / deny que las demás herramientas con efectos secundarios.

Hay dos targets de navegador:

- **managed** — zode lanza y controla un perfil de Chromium dedicado.
- **bridge** — zode controla el perfil de Chrome que ya estás usando mediante la extensión MV3 incluida en [`extensions/chrome/`](../../extensions/chrome/).

Para el target bridge, carga la extensión una vez desde `extensions/chrome`, luego ejecuta `/browser pair`. Zode abre la página de la extensión con el puerto WebSocket local y el código de emparejamiento ya rellenados; si esa pestaña aparece en blanco (Chrome a veces rechaza URLs `chrome-extension://` desde la línea de comandos), haz clic en el icono de zode en la barra de herramientas e introduce el puerto y el código de emparejamiento manualmente. **El emparejamiento se hace una sola vez**: la extensión almacena un token de larga duración y se reconecta automáticamente — al arrancar el navegador, al actualizarse la extensión y con un reintento cada minuto mientras esté desconectada — así que reiniciar zode nunca vuelve a pedirte emparejar. Se reconecta a una CLI en ejecución o auto-inicia un daemon zode exclusivo de la extensión cuando hace falta. Las pestañas que abre zode se colocan en un grupo de pestañas de Chrome llamado `zode`.

### Panel lateral de tareas en Chrome

Ejecuta la CLI zode actualizada y `/browser pair` una vez. Al hacer clic en el icono de la barra de herramientas se abre el panel lateral; después auto-inicia zode automáticamente cuando no hay ningún proceso CLI en ejecución. La página de emparejamiento sigue siendo un pequeño flujo de código/token, y las tareas se comparten con las sesiones de la TUI sin cambiar el foco de la terminal.

Los turnos del panel lateral vinculan las herramientas de navegador bridge a la página mostrada junto al panel, de modo que peticiones como "analiza esta página" usan `browser_read` sobre la pestaña existente en lugar de abrir una nueva. La automatización de navegador de la TUI y la CLI independientes sigue usando pestañas propias de zode en el grupo `zode`. La página activa es también el contexto por defecto para prompts ambiguos del panel lateral; los archivos locales del proyecto solo se inspeccionan cuando el usuario pregunta explícitamente por ellos.

El panel puede enviar texto, seleccionar un modelo, elegir los modos de acceso `readOnly`, `prompt` y `auto`, transmitir la respuesta y detener (Stop) un turno en ejecución. Un turno puede adjuntar como máximo 8 archivos y 20 MiB en total: imágenes PNG, JPEG, GIF y WebP de hasta 5 MiB cada una, más archivos de texto y código UTF-8 de hasta 1 MiB cada uno. Se rechazan las entradas PDF, de Office, archivos comprimidos, ejecutables y no UTF-8.

Tras actualizar la extensión, haz clic en Reload en `chrome://extensions`. Las versiones antiguas de la extensión siguen siendo compatibles con la automatización del navegador pero no tienen el panel lateral de tareas. En Windows, zode localiza y lanza Chrome directamente para las URLs de extensión en lugar de invocar el shell del navegador por defecto, evitando la redirección a Microsoft Store cuando Chrome ya está instalado.

Comandos útiles:

```bash
/browser                         # abrir el panel de control del navegador
/browser status                  # mostrar estado de target/running/paired
/browser launch                  # lanzar el navegador gestionado
/browser close                   # cerrar el navegador gestionado
/browser pair                    # emparejar o reconectar la extensión Chrome bridge
/browser target managed          # usar el Chromium gestionado de zode
/browser target bridge           # usar la extensión y guardarla como target por defecto en el próximo inicio
/browser screenshot [path]       # capturar una pantalla del navegador
```

Consulta [`extensions/chrome/README.md`](../../extensions/chrome/README.md) para la carga, actualización, empaquetado CRX y pasos de smoke-test de la extensión.

## Control del escritorio

Zode también puede conducir aplicaciones de escritorio nativas a través de las APIs de accesibilidad del sistema, no solo el navegador. El agente usa `desktop_read` para leer el árbol de accesibilidad (ventanas, elementos y sus refs), `desktop_act` para hacer clic, escribir, hacer scroll y fijar valores por elemento, y `desktop_screenshot` para capturar la pantalla. Las lecturas de solo lectura no requieren aprobación; las acciones de escritorio con efectos secundarios usan el mismo flujo de aprobación allow-once / always / deny que las demás herramientas con efectos secundarios.

Los backends se seleccionan por plataforma:

- **macOS** — la API de Accessibility (AX).
- **Windows** — UI Automation (UIA).
- **Linux** — AT-SPI.
- **Apps Electron** — se adjunta mediante el Chrome DevTools Protocol.

**Cursor fantasma y parada con Esc.** Zode nunca mueve tu ratón real. En macOS un overlay de cero permisos (`zode-overlay`) dibuja un cursor *falso* que vuela por una trayectoria Dubins suave hasta el objetivo de cada acción, para que puedas seguir lo que hace el agente; el texto escrito nunca se muestra en el overlay. Mientras la automatización de escritorio está activa, un **Esc** global interrumpe todos los turnos en ejecución y oculta el overlay (la misma ruta de parada que el Esc de la TUI). Las demás plataformas ejecutan las acciones de escritorio sin la visualización.

El texto CJK y otros caracteres sin keycode del layout US se entregan a través del portapapeles del sistema (escribir → sintetizar pegado → restaurar el portapapeles anterior) para que las apps con manejo de teclas personalizado reciban los caracteres reales.

```bash
/desktop            # mostrar el target de escritorio y el estado de permisos
/desktop status     # lo mismo, explícito
```

La configuración vive bajo `desktop.*` en `~/.zode/config.json`:

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor` (por defecto `true`) dibuja el cursor de overlay de macOS; `escCancel` (por defecto `true`) arma la interrupción con Esc global durante la automatización; `overlayHelperPath` (por defecto `null`) sobrescribe la ubicación del helper `zode-overlay` — un helper ausente simplemente desactiva la visualización. La automatización de escritorio puede pedir permiso del OS (p. ej. Accessibility de macOS) en el primer uso.

## Watchdog de turnos en segundo plano

Los turnos `/loop` y `/schedule` propiedad del scheduler se ejecutan bajo un watchdog de liveness en proceso. La actividad de provider, herramientas y agentes anidados refresca un heartbeat compartido del lado de la fuente, mientras `maxRuntimeSecs` permanece como tope absoluto. En cualquiera de los dos timeouts, zode solicita la cancelación cooperativa, espera `abortGraceSecs` y hace un hard-stop de la tarea del turno local si aún no se ha drenado. Detener la tarea no basta para liberar su slot del scheduler: zode también espera a que se aquieten todos los workers rastreados de provider, herramientas, hooks, lectores de subprocesos y agentes anidados. Si esa segunda frontera no se alcanza en cinco segundos, la pestaña/store se pone en cuarentena, el job se desactiva y su lease de intento en vivo permanece retenido hasta que los workers salgan realmente.

Los intentos fallidos usan un backoff exponencial acotado desde `initialBackoffSecs` hasta `maxBackoffSecs`. Un turno exitoso limpia su cuenta de fallos consecutivos; una vez agotado `maxRetries`, zode detiene el loop o desactiva el schedule persistido. La interrupción manual, la eliminación del job y la desactivación explícita cancelan la recuperación pendiente en lugar de crear otro reintento cuando no ha empezado ninguna mutación. La recuperación es intencionadamente conservadora con los efectos secundarios: zode reintenta automáticamente solo cuando no ha observado un efecto secundario; si una mutación pudo haber ocurrido ya, incluida una cancelación manual a mitad de mutación, detiene/desactiva el job y espera revisión humana. Las herramientas que desacoplan trabajo deliberadamente (`BashRun` o una GUI desacoplada) también detienen la recurrencia tras ese turno. El mismo límite de inactividad acota el encolado claim-to-start: si una pestaña o un preflight de turno ocupados impiden que una ocurrencia propia arranque, se convierte en un fallo de watchdog normal sin efectos secundarios y entra en la misma política de reintentos acotados en lugar de retener su lease entre procesos para siempre.

La quiescencia es una garantía local. El trabajo ya aceptado por un servidor MCP remoto, una extensión de navegador, un actor de escritorio u otro sistema externo puede no admitir revocación. Si una llamada así se interrumpe, zode marca su resultado como no resuelto, desactiva el job del scheduler y te exige verificar el estado externo antes de reactivarlo.

Usa `/watchdog status` para ver la configuración y la salud por turno/reintento. El mismo estado aparece en `/tasks` junto a los shells en segundo plano y los turnos en ejecución; también se muestran allí la antigüedad de la cola reclamada y las barreras de persistencia terminal.

Este es un watchdog para los turnos del scheduler dentro del proceso zode actual. No es un supervisor de procesos del OS y no puede reiniciar zode tras un crash o reinicio de la máquina; usa el gestor de servicios de tu plataforma cuando se requieran reinicios a nivel de proceso. Los schedules persistidos registran un token de intento activo respaldado por un lock de archivo del OS por schedule. Al inicio, un lock en disputa se deja en paz porque otro proceso zode todavía lo posee; un lock libre con el token persistido exacto es un huérfano de una salida no limpia, así que zode desactiva ese schedule como de estado-de-ejecución-desconocido en lugar de reproducirlo silenciosamente. Este contrato de recuperación cubre los crashes de proceso. No afirma durabilidad a nivel de almacenamiento ante un corte de energía súbito o hardware fallido, y no sustituye a un gestor de servicios del OS.

El timestamp de disparo y el token de intento activo se reclaman atómicamente antes de que un prompt persistido entre en la cola de una pestaña, de modo que el trabajo encolado ya es exclusivo entre procesos zode. Ese mismo lease se mueve con el prompt al turno y permanece retenido durante la persistencia final de transcript/índice. Editar, eliminar o desactivar una ocurrencia encolada es una cancelación explícita y limpia solo su token activo correspondiente. Una salida ordenada de la aplicación, en cambio, restaura la marca de disparo exacta no iniciada o el token de reintento, de modo que no puede consumir trabajo que nunca se ejecutó. Una escritura del roster terminal que falla mantiene el lease en un finalizador con reintentos; un token en conflicto se desactiva de forma duradera para revisión antes de liberarlo. Los turnos del scheduler omiten la extracción de memoria post-turno desacoplada, y la salida ordenada drena la quiescencia de workers más la persistencia terminal antes de destruir sus pestañas. La fase de recurrencia es canónica: los slots de intervalo usan aritmética de epoch absoluta desde el anchor persistido (incluso durante el retroceso de DST), los schedules de calendario mantienen su fase de reloj de pared, y el backlog perdido se coalesce al slot debido más reciente. Un proceso en ejecución también refresca el roster para que los cambios remotos de disable/remove, reintento y propiedad de huérfanos surtan efecto sin reiniciar.

## /loop, /schedule y temporización

- **`/loop <30s|5m|1h> [--max N] <prompt>`** — turnos recurrentes solo de la sesión en la pestaña actual; `list` / `stop [id]`. Intervalo mínimo 30s. Un prompt debido se encola por la misma ruta `queued_input` que el goal loop (nunca interrumpe un turno en ejecución; salta un disparo mientras su prompt sigue encolado).
- **`/schedule add <hh:mm|mon hh:mm|every 2h> <prompt>`** — persistido en `~/.zode/schedules.json`. Los disparos perdidos mientras zode no estaba en ejecución se saltan, nunca se reproducen. La dedup entre procesos es first-writer-wins sobre `lastFiredMs`. `list` / `rm <id>` / `enable|disable <id>`.
- **Alcance del watchdog en segundo plano** — solo los turnos `/loop` y `/schedule` propiedad del scheduler se registran con el watchdog; los turnos interactivos ordinarios no. Es un watchdog de turnos en proceso, no un supervisor del OS: no reinicia zode tras un crash del proceso o de la máquina.
- **Temporización** — `TurnRecorder` estampa `durationMs` en los eventos `tool.completed` y `turn.completed`. La TUI muestra sufijos por herramienta `· 1.2s`, un footer de turno `✓ done · 34s · 3 tools`, y el tiempo transcurrido humanizado en `/tasks`.

## Slash commands

| Comando | Qué hace |
|---|---|
| `/help` | Panel de comandos + teclas |
| `/clear` | Limpiar la conversación (y el contexto) |
| `/model [id]` | Mostrar / anotar el modelo activo |
| `/config` | Mostrar modelo + directorio de trabajo |
| `/compact` | Estado de la auto-compactación de contexto |
| `/cost` | Uso de tokens y coste hasta ahora (incl. sub-agentes) |
| `/theme [id]` | Cambiar tema (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Selector de sesiones — reanudar en una pestaña nueva con historial |
| `/connect` | Conectar y cambiar el provider activo |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Mostrar/ocultar la barra lateral derecha; plegar las secciones MCP / archivos modificados / todo |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Panel y comandos de control del navegador; emparejar la extensión Chrome bridge o cambiar entre Chromium gestionado y tu perfil de Chrome |
| `/desktop [status]` | Mostrar el target de escritorio y el estado de permisos |
| `/loop <interval> [--max N] <prompt>` | Ejecutar un prompt recurrente en la pestaña actual; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Persistir un prompt programado; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Mostrar la configuración, salud y reintentos pendientes del watchdog de turnos en segundo plano |
| `/tasks` | Panel de shells en segundo plano, turnos en ejecución y salud del watchdog |
| `/undo`, `/redo` | Deshacer / rehacer la última edición de archivo |
| `/mcp` | Administrar servidores MCP — habilitar / deshabilitar en un diálogo |
| `/skills` | Listar skills disponibles |
| `/agents` | Administrar sub-agentes — crear (asistido por IA o manual) / eliminar |
| `/external-agents [list\|discover]` | Listar las CLI externas compatibles en `PATH`, o registrar explícitamente cada preset detectado |
| `/team [status\|board\|dismiss <name>]` | Inspeccionar el roster de teammates persistentes y el board compartido, o eliminar un teammate |
| `/workflows` | Administrar y ejecutar workflows con scripts JS (orquestación `agent()`/`parallel()`/`pipeline()`, ejecutada de forma determinista por zode) |
| `/effort` | Elegir el nivel de esfuerzo de razonamiento |
| `/thinking`, `/tool-details` | Alternar la muestra del razonamiento / detalle de llamadas a herramientas |
| `/orchestration` | Alternar la orquestación autónoma de sub-agentes + workflows |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | Mostrar / controlar el sandbox del OS en tiempo de ejecución |
| `/language` | Cambiar el idioma de la UI (15 locales) |
| `/export [path]` | Exportar el transcript a Markdown (un directorio recibe un nombre por defecto) |
| `/yolo` | Modo de omisión de aprobaciones |
| `/exit` | Salir |

Los agentes y skills creados, y las herramientas MCP conectadas, también aparecen como slash commands dinámicos (p. ej. `/<name>`) y se pueden invocar directamente.

## Atajos de teclado

> En macOS los chords de la app de abajo usan **`Cmd`** (⌘); en Windows/Linux usan `Ctrl`. `Ctrl+C/D/L/V` se quedan como `Ctrl` en todas partes (convenciones de terminal).

| Tecla | Acción |
|---|---|
| `Enter` | Enviar mensaje (encola si hay un turno en ejecución) |
| `Shift`/`Alt`+`Enter` | Nueva línea |
| `Up` / `Down` | Recuperar el prompt anterior / siguiente enviado (o mover la selección del autocompletado) |
| `Ctrl+C` | Interrumpir el turno (salir cuando está inactivo) |
| `Ctrl+D` | Salir |
| `Ctrl+L` | Redibujar la conversación desde el store (recupera una vista en blanco; usa `/clear` para descartar) |
| `Ctrl+V` | Pegar (texto o rutas de imagen) |
| `Cmd/Ctrl+O` | Settings |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | Nueva pestaña / cerrar pestaña |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Saltar a / ciclar pestañas |
| `Cmd/Ctrl+B` | Panel de tareas en segundo plano |
| `Cmd/Ctrl+G` | Alternar la barra lateral |
| `F1` | Ayuda |
| `PgUp` / `PgDn` | Desplazar la conversación |
| `Home` / `End` | Saltar al inicio / al final de la conversación |
| `Esc` | Cerrar el overlay actual (o interrumpir un turno en ejecución) |

## Instrucciones del proyecto

Zode lee instrucciones de una jerarquía de tres niveles (los posteriores ganan atención): global `~/.zode/AGENTS.md` (o `instructions.md`) → raíz del proyecto → cwd. En cada directorio prefiere `AGENTS.md` sobre `CLAUDE.md`. Las skills viven bajo `.zode/skills/**/SKILL.md`; los servidores MCP en `~/.zode/mcp.json` ⊕ `.mcp.json`; los hooks en `~/.zode/hooks.json` ⊕ `.zode/hooks.json`.

**Configuración cross-agent.** Zode lee la configuración directa de skills y MCP de Claude Code, Codex, Cursor, opencode, Gemini y agentes locales relacionados. Los árboles de plugins instalados y las cachés de plugins de esos productos nunca se escanean. Para reutilizar un plugin, instala su fuente explícitamente con `zode plugin install ... --trust`; los formatos de paquete de Codex y Claude Code siguen siendo compatibles para los plugins instalados a través de Zode.

## Configurar servidores MCP

Los servidores MCP viven en la misma configuración de precedencia anidada que todo lo demás — `~/.zode/mcp.json` para todos los proyectos, `.mcp.json` o `.zode/mcp.json` en la raíz del proyecto para acotar uno a un repo. Sin registro, sin reiniciar-y-rezar: edita el archivo, luego `/mcp` (o relanza) para recogerlo.

### stdio (lanzar un servidor local)

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

## Ecosistema ZSeven-W

Zode forma parte del stack de herramientas AI-native de ZSeven-W:

| Producto | Qué es |
|----------|--------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Runtime async en Rust puro para LLM agents, con streaming multi-provider, tool dispatch, permisos, MCP, cost tracking, attachments, sessions y herramientas de coding opcionales. |
| [`jian`](https://github.com/ZSeven-W/jian) | Framework UI cross-platform nativo de Rust donde un archivo `.op` es una app, conectando artefactos de diseño estilo OpenPencil con software ejecutable. |
| [`noema`](https://github.com/ZSeven-W/noema) | Sistema de memoria local-first y non-vector para coding agents, con lexical recall, review queues, MCP, S3 offload y enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Herramienta open-source AI-native de diseño vectorial para workflows design-as-code, que convierte prompts en UI sobre un live canvas con concurrent agent teams. |

## Benchmark

Los benchmarks de Zode cubren generación one-shot de código, trabajo agentic de leer/ejecutar/editar/arreglar, tareas multiarchivo, bugs difíciles, seguimiento de instrucciones MCP/Skills y el runner Noema LOCOMO. La metodología completa, los comandos de reproducción y las tablas de resultados están en la [sección Benchmark del README en inglés](../../README.md#benchmark); la suite vive en [`benchmarks/`](../../benchmarks/).

## Desarrollo

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## Contribuir

Las contribuciones son bienvenidas. Usa [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`, con scopes habituales como `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`.

## Licencia

[MIT](../../LICENSE) &copy; ZSeven-W
