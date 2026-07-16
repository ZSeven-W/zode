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

- **Múltiples proveedores**: Anthropic, OpenAI, APIs compatibles con OpenAI como DeepSeek, Moonshot y OpenRouter, además de Ollama local.
- **Amplia superficie de herramientas**: lectura/escritura/edición de archivos, búsqueda de código y contenido, shells en primer y segundo plano, git, web fetch, notebooks y seguimiento de TODOs.
- **Control del navegador**: las herramientas `browser_*` manejan un Chromium gestionado o tu perfil real de Chrome mediante la extensión Chrome bridge.
- **Permisos sin bloqueo**: toda herramienta con efectos secundarios pasa por allow once / always / deny, con el prompt de aprobación integrado en la interfaz.
- **Sandbox del sistema operativo por defecto**: los comandos shell se ejecutan bajo `sandbox-exec` en macOS o `bwrap` en Linux, con red saliente denegada por defecto.
- **TUI de pantalla completa**: Markdown en streaming, resaltado de sintaxis, vistas de diff, autocompletado de slash commands, historial, 11 temas integrados, paneles de settings/help y UI en 15 idiomas (`/language`).
- **Pestañas multi-sesión**: ejecuta conversaciones aisladas en paralelo con `Ctrl+T` y reanuda sesiones anteriores.
- **Sub-agentes y workflows**: delega trabajo acotado con la herramienta Task y adminístralo con `/agents` y `/workflows`.
- **Skills, MCP y hooks**: carga paquetes `SKILL.md`, conecta servidores MCP y ejecuta scripts externos en eventos de herramientas.

## Instalación

### Una línea

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

El instalador detecta OS y CPU, descarga el binario correcto desde el último [release](https://github.com/ZSeven-W/zode/releases) y coloca `zode` en tu `PATH`.

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

Después descomprime y mueve `zode` a tu `PATH`, por ejemplo `sudo mv zode /usr/local/bin/`. Los builds de Linux usan glibc; los binarios de macOS no están firmados, así que usa `xattr -dr com.apple.quarantine ./zode` si Gatekeeper avisa.

### Desde código fuente

Requiere una toolchain estable reciente de Rust:

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

El binario queda en `target/release/zode`. El runtime del agente vive en el submodule `vendor/agent`; clona con `--recurse-submodules` o ejecuta `git submodule update --init`.

## Inicio rápido

La forma más sencilla es iniciar `zode` y ejecutar **`/connect`**. Abre un selector interactivo respaldado por models.dev y escribe la configuración.

También puedes crear `~/.zode/config.json` manualmente:

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

Comandos habituales:

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

## Configuración

`providers` es la fuente de verdad de los proveedores; `provider` en el nivel superior indica el modelo activo. Los proveedores compatibles con OpenAI suelen usar `baseUrl` y `dialect`:

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
  "language": "es"
}
```

Un provider puede contener varios modelos; cambia entre ellos con `/model`. El idioma se puede cambiar con `/language`.

## Server mode y SDKs

`zode server` inicia un servidor JSON-RPC delimitado por saltos de línea sobre stdin/stdout, útil para integraciones con editores, automatización local, tests y clientes SDK.

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDKs disponibles:

| SDK | Directorio | Test local |
|-----|------------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## Control del navegador

Zode incluye el grupo `tools:browser` para leer capturas/DOM/logs, navegar, hacer clic, escribir, ejecutar JavaScript y administrar pestañas. Puede usar un Chromium gestionado o tu Chrome real mediante la extensión en [`extensions/chrome/`](../../extensions/chrome/).

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

## Slash commands comunes

| Comando | Acción |
|---|---|
| `/help` | Ayuda de comandos y teclas |
| `/connect` | Conectar y cambiar provider |
| `/model [id]` | Mostrar o fijar modelo activo |
| `/theme [id]` | Cambiar tema (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Reanudar sesiones |
| `/browser ...` | Control del navegador |
| `/tasks` | Tareas en segundo plano |
| `/mcp` | Administrar servidores MCP |
| `/skills` | Listar skills |
| `/agents` | Administrar sub-agentes |
| `/workflows` | Administrar workflows |
| `/sandbox ...` | Controlar sandbox |
| `/language` | Cambiar idioma de la UI |
| `/export [path]` | Exportar Markdown |
| `/exit` | Salir |

La tabla completa está en el [README en inglés](../../README.md#slash-commands).

## Instrucciones, MCP y skills

Zode lee instrucciones desde `~/.zode/`, la raíz del proyecto y el directorio actual; en cada nivel prefiere `AGENTS.md` y luego `CLAUDE.md`. Las skills viven en `.zode/skills/**/SKILL.md`; los servidores MCP en `~/.zode/mcp.json`, `.mcp.json` o `.zode/mcp.json`.

También descubre skills, comandos y MCP existentes de Claude, Codex, opencode, Cursor y otros agentes. Los MCP externos encontrados dentro de un proyecto quedan desactivados por defecto.

## Ecosistema ZSeven-W

Zode forma parte del stack de herramientas AI-native de ZSeven-W:

| Producto | Qué es |
|----------|--------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Runtime async en Rust puro para LLM agents, con streaming multi-provider, tool dispatch, permisos, MCP, cost tracking, attachments, sessions y herramientas de coding opcionales. |
| [`jian`](https://github.com/ZSeven-W/jian) | Framework UI cross-platform nativo de Rust donde un archivo `.op` es una app, conectando artefactos de diseño estilo OpenPencil con software ejecutable. |
| [`noema`](https://github.com/ZSeven-W/noema) | Sistema de memoria local-first y non-vector para coding agents, con lexical recall, review queues, MCP, S3 offload y enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Herramienta open-source AI-native de diseño vectorial para workflows design-as-code, que convierte prompts en UI sobre un live canvas con concurrent agent teams. |

## Benchmark

Los benchmarks de Zode cubren generación one-shot, trabajo agentic de leer/ejecutar/editar/arreglar, tareas multiarchivo, bugs difíciles, seguimiento de instrucciones MCP/Skills y Noema LOCOMO. La metodología y los resultados completos están en la sección [Benchmark del README en inglés](../../README.md#benchmark).

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

Las contribuciones son bienvenidas. Usa [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <subject>`.

## Licencia

[MIT](../../LICENSE) &copy; ZSeven-W
