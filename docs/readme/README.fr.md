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
  <strong>Assistant de développement open-source et AI-native pour le terminal.</strong><br/>
  Il lit votre code, exécute des commandes, recherche des fichiers et gère git depuis une TUI Rust rapide.
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

> Ce README localisé couvre la vue d'ensemble et le démarrage rapide. Le [README anglais](../../README.md) reste la source de référence pour les détails complets des benchmarks et les longues notes à jour.

## Points forts

- **Multi-provider** : Anthropic, OpenAI, API compatibles OpenAI comme DeepSeek, Moonshot et OpenRouter, plus Ollama local.
- **Large surface d'outils** : lecture/écriture/édition de fichiers, recherche de code et contenu, shells foreground/background, git, web fetch, notebooks et suivi des TODO.
- **Contrôle du navigateur** : les outils `browser_*` pilotent un Chromium géré ou votre vrai profil Chrome via l'extension Chrome bridge.
- **Permissions non bloquantes** : chaque outil mutating passe par allow once / always / deny, avec une demande d'approbation intégrée.
- **Sandbox OS par défaut** : les commandes shell tournent sous `sandbox-exec` sur macOS ou `bwrap` sur Linux, avec le réseau sortant refusé par défaut.
- **TUI plein écran** : Markdown en streaming, coloration syntaxique, diff preview, autocomplétion des slash commands, historique, 11 thèmes intégrés, overlays settings/help et UI en 15 langues (`/language`).
- **Onglets multi-session** : lancez plusieurs conversations isolées avec `Ctrl+T` et reprenez les sessions passées.
- **Sous-agents et workflows** : déléguez des tâches bornées avec l'outil Task, puis gérez-les via `/agents` et `/workflows`.
- **Skills, MCP et hooks** : chargez des paquets `SKILL.md`, connectez des serveurs MCP et exécutez des scripts sur les événements d'outils.

## Installation

### En une ligne

**macOS / Linux :**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell) :**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

L'installeur détecte l'OS et le CPU, télécharge le binaire adapté depuis le dernier [release](https://github.com/ZSeven-W/zode/releases), puis place `zode` dans le `PATH`.

### Téléchargement manuel

Téléchargez l'archive correspondant à votre plateforme depuis la [page des releases](https://github.com/ZSeven-W/zode/releases).

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Décompressez puis déplacez `zode` dans votre `PATH`, par exemple `sudo mv zode /usr/local/bin/`. Les builds Linux utilisent glibc ; les binaires macOS ne sont pas signés, donc utilisez `xattr -dr com.apple.quarantine ./zode` si Gatekeeper signale un problème.

### Depuis les sources

Une toolchain Rust stable récente est nécessaire :

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
```

Le binaire se trouve dans `target/release/zode`. Le runtime agent est le submodule git `vendor/agent`; clonez avec `--recurse-submodules` ou exécutez `git submodule update --init`.

## Démarrage rapide

Le plus simple est de lancer `zode` puis **`/connect`**. Cela ouvre un sélecteur interactif de modèles et écrit la configuration.

Vous pouvez aussi écrire `~/.zode/config.json` à la main :

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

Commandes courantes :

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

## Configuration

`providers` est la source de vérité des providers ; le champ top-level `provider` pointe vers le modèle actif. Les providers compatibles OpenAI utilisent généralement `baseUrl` et `dialect` :

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
  "language": "fr"
}
```

Un provider peut contenir plusieurs modèles et `/model` permet de changer à chaud. La langue se change aussi avec `/language`.

## Mode serveur et SDKs

`zode server` démarre un serveur JSON-RPC newline-delimited sur stdin/stdout, prévu pour les intégrations d'éditeurs, l'automatisation locale, les tests et les clients SDK.

```bash
zode server
zode server --listen stdio://
zode server --listen off
```

SDKs :

| SDK | Dossier | Test local |
|-----|---------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

## Contrôle du navigateur

Zode inclut le groupe `tools:browser` pour lire captures/DOM/logs, naviguer, cliquer, saisir du texte, exécuter JavaScript et gérer les onglets. Il peut utiliser un Chromium géré ou votre Chrome réel via l'extension [`extensions/chrome/`](../../extensions/chrome/).

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

## Slash commands courantes

| Commande | Action |
|---|---|
| `/help` | Aide commandes et raccourcis |
| `/connect` | Connecter et changer de provider |
| `/model [id]` | Afficher ou fixer le modèle actif |
| `/theme [id]` | Changer de thème (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Reprendre des sessions |
| `/browser ...` | Contrôle navigateur |
| `/tasks` | Tâches en arrière-plan |
| `/mcp` | Gérer les serveurs MCP |
| `/skills` | Lister les skills |
| `/agents` | Gérer les sous-agents |
| `/workflows` | Gérer les workflows |
| `/sandbox ...` | Contrôler le sandbox |
| `/language` | Changer la langue de l'UI |
| `/export [path]` | Exporter en Markdown |
| `/exit` | Quitter |

La table complète se trouve dans le [README anglais](../../README.md#slash-commands).

## Instructions, MCP et skills

Zode lit les instructions depuis `~/.zode/`, la racine du projet puis le dossier courant ; à chaque niveau, `AGENTS.md` est préféré à `CLAUDE.md`. Les skills vivent dans `.zode/skills/**/SKILL.md` ; les serveurs MCP dans `~/.zode/mcp.json`, `.mcp.json` ou `.zode/mcp.json`.

Zode découvre aussi les skills, commandes et configurations MCP déjà installées pour Claude, Codex, opencode, Cursor et d'autres agents. Les MCP externes trouvés dans un projet sont désactivés par défaut.

## Écosystème ZSeven-W

Zode fait partie du stack ZSeven-W d'outils de développement AI-native :

| Produit | Rôle |
|---------|------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Runtime async en Rust pur pour LLM agents, avec streaming multi-provider, tool dispatch, permissions, MCP, suivi des coûts, attachments, sessions et coding tools optionnels. |
| [`jian`](https://github.com/ZSeven-W/jian) | Framework UI cross-platform natif Rust où un fichier `.op` est une app, reliant les artefacts de design façon OpenPencil à un logiciel exécutable. |
| [`noema`](https://github.com/ZSeven-W/noema) | Système mémoire local-first et non-vectoriel pour coding agents, avec lexical recall, review queues, MCP, S3 offload et enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Outil open-source AI-native de design vectoriel pour workflows design-as-code, transformant les prompts en UI sur un live canvas avec concurrent agent teams. |

## Benchmark

Les benchmarks de Zode couvrent la génération one-shot, le travail agentic lire/exécuter/éditer/corriger, les tâches multi-fichiers, les bugs difficiles, le suivi d'instructions MCP/Skills et Noema LOCOMO. La méthodologie et les résultats complets sont dans la section [Benchmark du README anglais](../../README.md#benchmark).

## Développement

```bash
cargo build --workspace
cargo run -p zode
cargo run -p zode -- -p "<prompt>"
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo deny check
```

## Contribution

Les contributions sont bienvenues. Utilisez [Conventional Commits](https://www.conventionalcommits.org/) : `<type>(<scope>): <subject>`.

## Licence

[MIT](../../LICENSE) &copy; ZSeven-W
