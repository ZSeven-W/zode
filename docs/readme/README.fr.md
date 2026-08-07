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

- **Multi-provider** : Anthropic, OpenAI et toute API compatible OpenAI (dialectes DeepSeek, Moonshot, OpenRouter), plus Ollama local. Prise en charge des modèles à sortie longue et à **contexte 1M** (`contextWindow` / `maxOutputTokens` sont configurables).
- **Large surface d'outils** : lecture/écriture/édition de fichiers (y compris `MultiEdit` atomique multi-hunk), recherche de code et contenu, shells foreground/background, git, web fetch (plus `WebSearch` optionnel avec une clé Tavily), notebooks et suivi des TODO.
- **Contrôle du navigateur** : les outils `browser_*` intégrés pilotent un Chromium géré ou votre vrai profil Chrome via l'extension Chrome bridge de zode : naviguer, cliquer/saisir, inspecter le DOM, capturer des screenshots, lire les logs console/réseau et grouper les onglets ouverts par zode. L'appairage ne se fait qu'une fois : l'extension se reconnecte automatiquement d'un redémarrage de zode à l'autre.
- **Permissions non bloquantes** : chaque outil mutating passe par une approbation (allow once / always / deny), mais la demande s'ancre en ligne sans jamais vous bloquer : continuez à taper pour mettre en file une suite pendant qu'un outil attend, avec des règles de hard-deny.
- **Sandbox OS par défaut** : les commandes shell tournent sous sandbox-exec (macOS) / bwrap (Linux) en mode `read-only` ou `workspace-write`, avec le **réseau sortant refusé par défaut**. Basculez à chaud avec `/sandbox` ; le modèle peut demander une échappée pour une seule commande (`dangerouslyDisableSandbox`) que **vous autorisez** à l'invite.
- **TUI plein écran** : Markdown en streaming avec coloration syntaxique, diff previews, autocomplétion des slash commands, historique des prompts (Haut/Bas), 11 thèmes intégrés, overlays settings et help, sections de barre latérale résilientes, et **UI en 15 langues** (`/language`).
- **Sessions durables et compatibles V1** : conserve le contrat de transcript `<id>.jsonl` existant tout en ajoutant journals, checkpoints, rewind, fork et Git worktrees isolés en données sidecar. La compaction de contexte ne perd jamais la conversation visible : les sessions reprises rejouent tout l'historique d'avant compaction tandis que le contexte du modèle reste compact.
- **Surfaces d'automatisation** : sortie headless JSON/JSONL stable, ciblage exact des sessions, filtres d'outils, codes de sortie déterministes, ACP sur stdio, et un dashboard d'opérations local.
- **Onglets multi-session** : lancez plusieurs conversations côte à côte (`Ctrl+T`), chacune un agent isolé ; reprenez les sessions passées avec rejeu complet de l'historique.
- **Sous-agents, équipes et workflows** : déléguez des travaux ponctuels via l'outil Task, engagez des teammates internes ou issus de CLI externes persistants, coordonnez-les avec un board partagé et des file claims, et gérez le tout avec `/agents`, `/team` et `/workflows`.
- **Configuration locale portable** : lit directement les skills et la configuration MCP de Claude Code, Codex, Cursor, opencode et Gemini, sans jamais importer leurs arbres de plugins installés ni leurs caches.
- **Skills et MCP** : chargez des paquets d'instructions `SKILL.md` à la demande et connectez des serveurs MCP (`mcp__<server>__<tool>`) ; les agents, skills et outils MCP créés apparaissent comme des slash commands.
- **Hooks** : exécutez des scripts externes sur les événements d'outils (par ex. bloquer des commandes dangereuses, linter après édition).
- **Instructions à trois niveaux** : global (`~/.zode/`) → racine du projet → cwd (`AGENTS.md` / `CLAUDE.md`).

## Installation

### En une ligne (binaires précompilés)

**macOS / Linux :**

```bash
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell) :**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

L'installeur détecte automatiquement l'OS et le CPU, télécharge le binaire adapté depuis le dernier [release](https://github.com/ZSeven-W/zode/releases), puis place `zode` dans le `PATH`. Pour épingler une version ou changer l'emplacement :

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh -s -- --version v0.1.0-beta.1
ZODE_BIN_DIR="$HOME/.local/bin" curl -fsSL .../install.sh | sh
```

```powershell
# Windows
$env:ZODE_VERSION = 'v0.1.0-beta.1'; irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

### Téléchargement manuel

Téléchargez l'archive correspondant à votre plateforme depuis la [page des releases](https://github.com/ZSeven-W/zode/releases) :

| OS | Arch | Asset |
|----|------|-------|
| macOS | Apple Silicon | `zode-<version>-arm64-mac.tar.gz` |
| macOS | Intel | `zode-<version>-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-<version>-x64-linux.tar.gz` |
| Linux | ARM64 | `zode-<version>-arm64-linux.tar.gz` |
| Windows | x64 | `zode-<version>-x64-windows.zip` |
| Windows | ARM64 | `zode-<version>-arm64-windows.zip` |

Décompressez puis déplacez `zode` dans votre `PATH` (`sudo mv zode /usr/local/bin/`). Les builds Linux utilisent glibc ; les binaires macOS ne sont pas signés (`xattr -dr com.apple.quarantine ./zode` si Gatekeeper se plaint).

### Depuis les sources

Nécessite Rust 1.88 ou plus récent :

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo build --release -p zode
# binaire dans target/release/zode
```

> Le runtime agent vit dans le submodule git `vendor/agent` — clonez toujours avec `--recurse-submodules` (ou exécutez `git submodule update --init`).

## Démarrage rapide

Le plus simple est de lancer `zode` puis d'exécuter **`/connect`** — un sélecteur interactif, adossé à models.dev, qui écrit la configuration pour vous.

Pour écrire `~/.zode/config.json` à la main : **`providers`** est la source de vérité — une entrée par provider (identifiants partagés) contenant un ou plusieurs **models** — et le **`provider`** de premier niveau enregistre le modèle *actif* :

```jsonc
{
  "providers": {
    "anthropic": {
      "type": "anthropic",               // protocole : "anthropic" | "openai" | "ollama"
      "apiKey": "sk-...",
      "models": { "claude-sonnet-4-6": {} }
    }
  },
  "provider": { "model": "claude-sonnet-4-6" }   // le modèle actif
}
```

Les providers compatibles OpenAI (DeepSeek, Moonshot, OpenRouter, …) ajoutent un `baseUrl` + `dialect`, et les réglages par modèle vivent dans l'entrée de chaque modèle :

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

Une entrée provider peut contenir plusieurs modèles — basculez entre eux à chaud avec `/model`.

Puis lancez :

```bash
zode                       # TUI plein écran
zode -p "explain main.rs"  # headless : un prompt, streamé vers stdout, sortie
zode --no-tui              # REPL readline simple
zode -c                    # continuer la session la plus récente
zode -r <id>               # reprendre une session par préfixe d'id
zode --yolo                # contourner les approbations (les règles de deny s'appliquent toujours)
zode --no-sandbox          # désactiver la sandbox OS (activée par défaut)
zode --sandbox-read-only   # sandbox en lecture seule (refuse toute écriture)
zode --sandbox-allow-network  # autoriser le réseau sortant dans la sandbox
zode --browser             # forcer l'activation des outils navigateur pour ce run
zode --no-browser          # désactiver les outils navigateur pour ce run
zode --model <id>          # surcharger le modèle
zode --provider <name>     # choisir un provider nommé dans config.providers
zode server                # mode app-server JSON-RPC sur stdio
zode acp                   # agent Agent Client Protocol sur stdio
zode dashboard             # vue locale sessions/checkpoints/worktrees
```

Vous pouvez aussi pointer vers n'importe quel provider sans éditer la config en exportant la clé correspondante (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, …) ; pour Ollama, le `baseUrl` est pris dans l'environnement s'il n'est pas défini.

## Contrôle du navigateur

Zode inclut un groupe `tools:browser` pour l'automatisation du navigateur. L'agent peut utiliser `browser_read` pour les screenshots, les snapshots DOM, les logs console et réseau et la lecture des onglets ; `browser_act` pour la navigation, les clics, la saisie, les appuis de touches et le défilement ; `browser_eval` pour du JavaScript ; et `browser_tabs` pour la gestion des onglets. L'inspection en lecture seule n'est pas gated ; les actions mutating passent par le même flux allow-once / always / deny que les autres outils à effet de bord.

Deux cibles navigateur existent :

- **managed** — zode lance et contrôle un profil Chromium dédié.
- **bridge** — zode contrôle le profil Chrome que vous utilisez déjà via l'extension MV3 fournie dans [`extensions/chrome/`](../../extensions/chrome/).

Pour la cible bridge, chargez l'extension une fois depuis `extensions/chrome`, puis exécutez `/browser pair`. Chrome bloque les URL `chrome-extension://` ouvertes par des programmes externes (ERR_BLOCKED_BY_CLIENT, sur macOS, Windows et Linux indifféremment), la tentative de zode d'ouvrir la page lui-même peut donc échouer — à la place, l'extension ouvre elle-même sa page d'appairage dans les ~30 secondes qui suivent `/browser pair`, avec le port prérempli ; saisissez-y le code d'appairage à 6 chiffres affiché dans le chat. En secours manuel, tapez vous-même l'URL `chrome-extension://…/popup.html?port=…` dans la barre d'adresse (une navigation saisie à la main est initiée par le navigateur et donc autorisée). **L'appairage ne se fait qu'une fois** : l'extension stocke un token de longue durée et se reconnecte automatiquement — au démarrage du navigateur, lors des mises à jour de l'extension, et avec une nouvelle tentative environ toutes les 30 secondes tant qu'elle est déconnectée — redémarrer zode ne redemande donc jamais d'appairage. Elle se reconnecte à une CLI en cours d'exécution ou démarre automatiquement un daemon zode dédié à l'extension au besoin. Les onglets ouverts par zode sont placés dans un groupe d'onglets Chrome nommé `zode`.

### Panneau latéral de tâches Chrome

Lancez la CLI zode mise à jour et `/browser pair` une fois. Cliquer sur l'icône de la barre d'outils ouvre le panneau latéral ; ensuite il démarre zode automatiquement quand aucun processus CLI ne tourne. La page d'appairage reste un petit flux code/token, et les tâches restent partagées avec les sessions TUI sans changer le focus du terminal.

Les tours du panneau latéral lient les outils navigateur bridge à la page actuellement affichée à côté du panneau, de sorte que des requêtes comme « analyse cette page » utilisent `browser_read` sur l'onglet existant au lieu d'en ouvrir un nouveau. L'automatisation navigateur autonome en TUI/CLI continue d'utiliser des onglets détenus par zode dans le groupe `zode`. La page active est aussi le contexte par défaut pour les prompts ambigus du panneau ; les fichiers locaux du projet ne sont inspectés que si l'utilisateur le demande explicitement.

Le panneau peut envoyer du texte, choisir un modèle, sélectionner les modes d'accès `readOnly`, `prompt` et `auto`, streamer la réponse et arrêter un tour en cours. Un tour peut joindre au plus 8 fichiers et 20 Mio au total : images PNG, JPEG, GIF et WebP jusqu'à 5 Mio chacune, plus des fichiers texte et code UTF-8 jusqu'à 1 Mio chacun. Les entrées PDF, Office, archive, exécutable et non-UTF-8 sont rejetées.

Après une mise à jour de l'extension, cliquez sur Recharger dans `chrome://extensions`. Les anciennes versions restent compatibles avec l'automatisation navigateur mais n'ont pas le panneau latéral de tâches. Sur Windows, zode localise et lance Chrome directement pour les URL d'extension au lieu d'invoquer le shell du navigateur par défaut, évitant la redirection vers le Microsoft Store lorsque Chrome est déjà installé.

Commandes utiles :

```bash
/browser                         # ouvrir le panneau de contrôle navigateur
/browser status                  # afficher l'état target/running/paired
/browser launch                  # lancer le navigateur géré
/browser close                   # fermer le navigateur géré
/browser pair                    # appairer ou reconnecter l'extension Chrome bridge
/browser target managed          # utiliser le Chromium géré de zode
/browser target bridge           # utiliser l'extension et l'enregistrer comme cible par défaut au prochain lancement
/browser screenshot [path]       # capturer un screenshot du navigateur
```

Voir [`extensions/chrome/README.md`](../../extensions/chrome/README.md) pour le chargement de l'extension, la mise à jour, le packaging CRX et les étapes de smoke test.

### Drapeaux `--browser` / `--no-browser`

Ce sont des surcharges valables uniquement pour la session, jamais persistées dans `config.json` : `--browser` force l'activation du groupe `browser` pour ce run ; `--no-browser` le force à désactivé. Sans drapeau, `browser.enabled` (config, `true` par défaut) décide.

## Contrôle du bureau

Zode peut piloter les applications de bureau natives via les API d'accessibilité de l'OS, pas seulement le navigateur. L'agent utilise `desktop_read` pour lire l'arbre d'accessibilité (fenêtres, éléments et leurs refs), `desktop_act` pour cliquer, saisir, faire défiler et définir des valeurs par élément, et `desktop_screenshot` pour capturer l'écran. Les lectures en lecture seule ne sont pas gated ; les actions bureau mutating passent par le même flux allow-once / always / deny que les autres outils à effet de bord.

Les backends sont choisis par plateforme :

- **macOS** — l'API Accessibility (AX).
- **Windows** — UI Automation (UIA).
- **Linux** — AT-SPI.
- **Applications Electron** — attachement via le Chrome DevTools Protocol.

**Curseur fantôme et arrêt par Esc.** Zode ne déplace jamais votre vraie souris. Sur macOS, une couche de superposition sans permission (`zode-overlay`) dessine un *faux* curseur qui vole le long d'un chemin de Dubins lisse jusqu'à la cible de chaque action, pour que vous puissiez suivre ce que fait l'agent ; le texte saisi n'est jamais affiché dans la superposition. Pendant l'automatisation du bureau, un **Esc** global interrompt tous les tours en cours et masque la superposition (le même chemin d'arrêt que l'Esc de la TUI). Les autres plateformes exécutent les actions bureau sans la visualisation.

Les caractères CJK et autres sans keycode de disposition US sont livrés via le presse-papiers du système (écrire → synthétiser le coller → restaurer le presse-papiers précédent) afin que les applications avec une gestion de touches personnalisée reçoivent les vrais caractères.

```bash
/desktop            # afficher la cible bureau et l'état des permissions
/desktop status     # idem, explicite
```

La configuration vit sous `desktop.*` dans `~/.zode/config.json` :

```json
{
  "desktop": {
    "ghostCursor": true,
    "escCancel": true,
    "overlayHelperPath": null
  }
}
```

`ghostCursor` (`true` par défaut) dessine le curseur de superposition macOS ; `escCancel` (`true` par défaut) arme l'interruption Esc globale pendant l'automatisation ; `overlayHelperPath` (`null` par défaut) surcharge l'emplacement du helper `zode-overlay` — un helper absent désactive simplement la visualisation. L'automatisation du bureau peut demander une permission OS (par ex. l'Accessibilité macOS) à la première utilisation.

## Watchdog des tours en arrière-plan, `/loop` et `/schedule`

Les tours `/loop` et `/schedule` détenus par le scheduler s'exécutent sous un watchdog de liveness en process. L'activité provider, outils et sous-agents rafraîchit un heartbeat côté source partagé, tandis que `maxRuntimeSecs` reste un plafond absolu. À l'un ou l'autre timeout, zode demande une annulation coopérative, attend `abortGraceSecs`, puis stoppe durement la tâche de tour locale si elle n'a toujours pas vidé. Arrêter la tâche ne suffit pas à libérer son créneau scheduler : zode attend aussi que chaque provider, outil, hook, lecteur de sous-processus et worker de sous-agent suivis se mettent au repos. Si cette seconde limite n'est pas atteinte en cinq secondes, l'onglet/store est mis en quarantaine, le job est désactivé, et son bail d'attempt-live reste tenu jusqu'à la sortie effective des workers.

Les tentatives échouées utilisent un backoff exponentiel borné de `initialBackoffSecs` à `maxBackoffSecs`. Un tour réussi remet à zéro son compteur d'échecs consécutifs ; une fois `maxRetries` épuisé, zode arrête la boucle ou désactive le planning persisté. Une interruption manuelle, une suppression de job et une désactivation explicite annulent la récupération en attente au lieu de créer une autre tentative quand aucune mutation n'a démarré. La récupération est volontairement prudente face aux effets de bord : zode ne réessaie automatiquement que s'il n'a observé aucun effet de bord ; si une mutation a pu déjà se produire, y compris une annulation manuelle en cours de mutation, il arrête/désactive le job et attend une revue humaine. Les outils qui détachent volontairement du travail (`BashRun` ou un GUI détaché) stoppent aussi la récurrence après ce tour. La même limite d'inactivité borne la mise en file claim-to-start : si un onglet occupé ou un préflight de tour empêche une occurrence détenue de démarrer, cela devient un échec de watchdog sans effet de bord et entre dans la même politique de retry bornée au lieu de tenir son bail cross-process indéfiniment.

La mise au repos est une garantie locale. Un travail déjà accepté par un serveur MCP distant, une extension navigateur, un acteur bureau ou tout autre système externe peut ne pas supporter la révocation. Si un tel appel est interrompu, zode marque son résultat comme non résolu, désactive le job du scheduler, et exige que vous vérifiiez l'état externe avant de le réactiver.

Utilisez `/watchdog status` pour la configuration et la santé par tour/retry. Le même état apparaît dans `/tasks` aux côtés des shells d'arrière-plan et des tours en cours ; l'âge de la file claimée et les barrières de persistance terminale y sont aussi affichés.

C'est un watchdog pour les tours du scheduler à l'intérieur du process zode courant. Ce n'est pas un superviseur de process OS et il ne peut pas redémarrer zode après un crash ou un redémarrage machine ; utilisez le gestionnaire de services de votre plateforme quand des redémarrages au niveau process sont requis. Les plannings persistés enregistrent un token d'attempt-live adossé à un verrou de fichier OS par planning. Au démarrage, un verrou en contention est laissé tranquille car un autre process zode le détient encore ; un verrou libre portant le token persisté exact est un orphelin d'une sortie non propre, donc zode désactive ce planning comme état d'exécution inconnu au lieu de le rejouer silencieusement.

### `/loop` et `/schedule`

- **`/loop <30s|5m|1h> [--max N] <prompt>`** — tours récurrents valables pour la session sur l'onglet courant ; `list` / `stop [id]`. Intervalle minimum 30s. Un prompt dû est mis en file via le même chemin `queued_input` que la goal loop (n'interrompt jamais un tour en cours ; saute un déclenchement tant que son prompt est encore en file).
- **`/schedule add <hh:mm|mon hh:mm|every 2h> <prompt>`** — persisté dans `~/.zode/schedules.json` (tmp+rename atomique ; les fichiers corrompus sont mis en quarantaine en `.corrupt`). Les déclenchements manqués pendant que zode ne tourne pas sont sautés, jamais rejoués. La dédup cross-process est first-writer-wins. `list` / `rm <id>` / `enable|disable <id>`.

Les prompts de job sont des prompts simples : `parse_loop` / `parse_schedule` rejettent un `/` ou `!` en tête. Le timing rend les intervalles en tokens compacts round-trippables (par ex. `every 2h`).

Configuration du watchdog de premier niveau, `backgroundWatchdog` en camelCase : `enabled` (`true` par défaut), `inactivityTimeoutSecs` (`900`), `maxRuntimeSecs` (`3600`), `abortGraceSecs` (`10`), `maxRetries` (`3`), `initialBackoffSecs` (`5`) et `maxBackoffSecs` (`300`).

### Timing des tâches

`TurnRecorder` estampille `durationMs` sur les événements `tool.completed` et `turn.completed` (journalisé ; les anciens journaux se parsent comme `None`). La TUI affiche des suffixes `· 1.2s` par outil, un pied de tour `✓ done · 34s · 3 tools`, et un temps écoulé humanisé dans `/tasks`.

## Enregistrer manuellement des teammates CLI externes

Zode peut utiliser une CLI d'agent tierce installée comme worker Task ponctuel, ou comme teammate persistant ou stateless. L'enregistrement est volontairement manuel : installer une CLI ou la mettre dans `PATH` ne l'expose **pas** au modèle. Ajoutez un profile sous `externalAgents.agents`, puis démarrez Zode dans le projet. Ou exécutez `/external-agents` pour inspecter les CLI supportées actuellement dans `PATH`, puis `/external-agents discover` pour ajouter explicitement chaque preset détecté à la configuration globale. Cette commande est déclenchée par l'utilisateur ; le démarrage ne scanne ni n'enregistre jamais de CLI externes automatiquement.

| Profile d'agent | Exécutable | Worker Task | Mode team | Sandbox de la CLI externe |
|---|---|---:|---:|---|
| `claude-code` | `claude` | oui | persistent | unrestricted (`--dangerously-skip-permissions`) |
| `codex` | `codex` | oui | persistent | workspace-write |
| `opencode` | `opencode` | oui | stateless | unknown |
| `cline` | `cline` | oui | stateless | unrestricted |
| `antigravity` | `agy` | oui | stateless | unknown |
| `cursor` | `cursor-agent` | oui | persistent | unrestricted |
| `kiro` | `kiro-cli` | oui | stateless | unrestricted |
| `pi` | `pi` | oui | persistent | unrestricted |
| `grok` (Grok Build) | `grok` | oui | persistent | unrestricted |

Tout profile enregistré peut rejoindre une équipe. Les profiles resumables préservent l'ID de session et la conversation de la CLI entre les assignations ; les autres CLI sont des teammates stateless qui démarrent un nouveau processus à chaque assignation. Les presets utilisent les interfaces headless documentées de [Cline](https://docs.cline.bot/usage/cli-overview), [Antigravity](https://antigravity.google/docs/cli-best-practices), [Cursor](https://cursor.com/docs/cli/headless), [Kiro](https://kiro.dev/docs/cli/headless/), [Pi](https://pi.dev/docs/latest) et de [Grok Build](https://docs.x.ai/build/cli/headless-scripting) de xAI. D'autres outils, y compris d'autres CLI Grok, peuvent utiliser un profile custom.

### Ajouter un profile de CLI manuellement

Placez `externalAgents` dans `~/.zode/config.json` pour tous les projets, ou dans `<project>/.zode/config.json` pour un projet. Un objet vide active explicitement un preset connu et résout son exécutable sur le `PATH` assaini :

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

N'ajoutez que les profiles que vous comptez exposer. Un `command` nu comme `cline` est résolu sur `PATH` ; des chemins comme `./tools/my-agent` ou `/opt/agents/my-agent` sont aussi acceptés. Les presets connus honorent `enabled`, `command`, `extraArgs`, `envAllow` et `trusted` ; `extraArgs` est ajouté à l'invocation preset de Zode.

Les processus CLI démarrent avec un environnement vidé ne contenant que `PATH`, `HOME` et `TERM` (plus les variables Windows requises), donc ajoutez explicitement les API keys ou autres variables nécessaires à `envAllow`. L'état de connexion existant sous `HOME` continue de fonctionner. Une entrée projet portant le même nom de profile remplace toute l'entrée globale, alors répétez toute surcharge dont le projet a encore besoin.

Un profile custom déclare l'invocation et le protocole complets :

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

`promptTransport` vaut `stdin`, `argv` ou `file` ; `argv` requiert un argument `{prompt}` autonome et `file` requiert `{prompt_file}`. `output` vaut `text`, `jsonl` générique, `jsonl-claude` ou `jsonl-codex`. Les profiles JSONL génériques utilisent des pointeurs RFC 6901 `textSource` et `sessionIdSource` pour extraire le texte streamé et un ID de session resumable de n'importe quel événement. `resumeArgs` doit contenir un token `{session_id}` autonome et est ajouté aux tours ultérieurs ; `resumeFlag` est conservé comme la forme raccourcie `<flag> <session-id>`.

Si une CLI accepte un ID de session choisi par l'appelant, `newSessionArgs` peut contenir un token `{session_id}` autonome. Zode génère un UUID, ajoute les arguments étendus au premier run, et utilise `resumeArgs` aux assignations suivantes. Cela rend aussi resumable une CLI en texte simple sans parser un ID depuis sa sortie.

Cela permet à toute CLI headless de devenir un worker Task ou un teammate stateless. Pour préserver le contexte de conversation entre assignations d'équipe, elle doit en plus exposer un ID de session, ou en accepter un via `newSessionArgs`, plus une invocation de reprise non interactive.

`effectiveSandbox` accepte `none`, `readOnly`, `workspaceWrite`, `unrestricted` ou `unknown` et s'affiche dans l'invite de confiance.

### Engager et travailler avec le teammate

Demandez au leader en langage naturel ; `team_hire` et `team_send` sont des tools du modèle, pas des slash commands :

```text
Hire the `codex` external agent as a teammate named `implementer`.
Its role is to implement the authentication refactor and run the focused tests.

Send `implementer` the task now and claim `src/auth/` for it before editing.

Ask `implementer` to address the review findings while preserving its session context.
```

Le premier hire montre l'exécutable et les arguments résolus, le répertoire de travail et le sandbox effectif de la CLI. L'approuver délègue le travail à ce processus dans le projet courant : Zode gate le lancement du processus, mais ne gate **pas** chaque édition de fichier ou commande shell effectuée par la CLI externe. Les trust grants durent pendant la session Zode courante ; le roster persistant est récupéré depuis `<cwd>/.zode/team/`, mais un teammate externe doit être approuvé à nouveau après un redémarrage ou un changement d'exécutable.

Dans les runs non interactifs/bypass (y compris `--yolo`), Zode ne peut pas montrer l'invite de confiance et échoue fermé. Ne mettez `externalAgents.agents.<profile>.trusted` à `true` que si vous voulez délibérément que ce profile tourne sans l'invite.

Utilisez `/team` pour inspecter le roster et le board après un hire :

```text
/team                         # panneau roster + board
/team status                  # roster en texte
/team board                   # objectif partagé, notes, assignations et claims
/team dismiss implementer     # retirer le teammate
```

Les teammates internes exécutent une QueryLoop en process qui partage le gate de permissions / hooks / file cache du leader (même sandbox et historique d'édition), héritent d'un jeu d'outils filtré par rôle (reviewer/researcher en lecture seule par défaut), tirent leur modèle/système d'un AgentDef correspondant, et rapportent leur usage de tokens par teammate. Leur historique persiste par teammate sous `~/.zode/agent/sessions/team/`. Le board est géré par l'hôte sous `<cwd>/.zode/team/` (`board.json` écrit atomiquement sous un `board.lock` stable) ; les claims sont des baux TTL conscients de la sous-arborescence, avec l'identité du détenteur injectée par l'hôte.

## Automatisation, sessions durables et opérations

### Runs headless structurés

`-p`, `--prompt-file` et `--prompt-json` utilisent tous le même moteur headless. `json` émet un objet de résultat final ; `stream-json` émet un objet JSON `zode.run-event.v1` par ligne. Les modes structurés réservent stdout à la sortie machine et utilisent des codes de sortie stables : `0` succès, `10` erreur provider, `11` permission refusée, `12` limite de tour/atteinte, `13` interrompu (Ctrl-C), `14` résultat partiel, `15` erreur de ciblage de session.

```bash
zode -p "fix the failing tests" --output-format json --max-turns 12
zode -p "review this repo" --output-format stream-json \
  --tools 'File*,ContentSearch,Git' --disallowed-tools FileWrite
zode --prompt-file prompt.txt --permission-mode accept-edits
zode --prompt-json '{"prompt":"summarize the workspace"}'

# Les IDs exacts ne font pas de préfixe. Un fork ne mute jamais sa session source.
zode -p "continue the work" --session-id my-session
zode -p "try another approach" --fork-session my-session --fork-worktree
```

Les patterns de deny d'outils l'emportent sur les patterns d'allow et sont hérités par les sous-agents Task. `--permission-mode` accepte `default`, `dont-ask`, `accept-edits` et `bypass` ; `--yolo` reste un raccourci pour bypass, les règles de hard deny s'appliquant toujours.

### Sessions, checkpoints et worktrees compatibles V1

Le transcript reste le fichier V1 original à `~/.zode/sessions/<id>.jsonl`. C'est la **seule** copie du transcript, donc les clients Zode plus anciens peuvent continuer à le lire et l'écrire. Les nouvelles métadonnées sont additives et vivent dans `~/.zode/sessions/<id>/` (`meta.json`, journal, checkpoints et snapshots). Aucun nouveau format de session ni migration de transcript n'est requis.

```bash
zode session list
zode session list --json
zode session show <id>                         # métadonnées + IDs de checkpoint
zode session fork <id> --target-id experiment
zode session fork <id> --checkpoint <cp> --worktree
zode session rewind <id> <cp>                  # prévisualisation consciente des conflits
zode session rewind <id> <cp> --apply
zode session apply-back <id> --target /path/to/checkout
zode session delete <id> --remove-worktree
```

Un checkpoint est capturé avant un tour mutating. Rewind restaure le contenu des fichiers suivis et le préfixe du transcript, rapporte les conflits au lieu d'écraser des changements plus récents, et enregistre une nouvelle branche logique de journal plutôt que de supprimer l'historique. Les forks worktree peuvent être réappliqués explicitement quand l'expérience est prête.

**La compaction ne perd jamais la conversation visible.** Quand la compaction de contexte remplace d'anciens messages par un résumé, les originaux sont préservés dans un sidecar additif (`~/.zode/sessions/<id>/compacted.jsonl`). Reprendre une session, appuyer sur `Ctrl+L`, `/export` et le panneau latéral Chrome affichent tous l'historique complet d'avant compaction, tandis que le modèle continue de ne recevoir que le contexte compacté. Les forks emportent l'archive (filtrée sur leur propre transcript), `/clear` la supprime, et supprimer une session supprime tout le sidecar.

### Règles de permissions et profiles de sandbox

Les règles peuvent vivre sous `permissions.rules` dans `config.json`, ou dans un fichier JSON autonome passé avec `--rules`. Un matcher de champ utilise un pointeur JSON RFC 6901 ; deny prime sur ask, qui prime sur allow. Le fichier autonome doit être soit un tableau de règles, soit `{ "rules": [...] }` ; il n'est pas enveloppé dans un objet `permissions` de premier niveau.

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

Les profiles intégrés sont `read-only`, `workspace`, `workspace-network` et `unconfined`. Les profiles définis en config utilisent les mêmes champs sandbox montrés ci-dessus. Sur Windows, la sandbox suit une approche à paliers (tiers) analogue.

### Plugins et marketplaces statiques

Un plugin géré peut fournir skills, commandes, agents, hooks, serveurs MCP, serveurs LSP et rendus UI JavaScript sandboxés. Zode accepte `plugin.json`, `.zode-plugin/plugin.json`, `.codex-plugin/plugin.json`, `.grok-plugin/plugin.json` et `.claude-plugin/plugin.json`. Les tableaux de chemins de composants Codex et Claude Code sont supportés, et le `defaultEnabled` de Claude Code est honoré à la première installation. Les composants réservés à l'hôte, comme les apps/connectors Codex et les themes, monitors ou output styles de Claude Code, sont ignorés ; un plugin uniquement app est refusé car il n'a aucun composant compatible Zode. Les installations sont des snapshots immuables avec provenance et un tree hash SHA-256. Le contenu de plugin exécutable n'est jamais activé sans le drapeau `--trust` explicite.

#### Démarrage rapide d'un plugin UI JavaScript

Le plus petit plugin UI contient un manifest et un fichier JavaScript :

```text
my-plugin/
├── plugin.json
└── scripts/
    └── ui.js
```

`plugin.json` :

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

Installez un répertoire local ou un dépôt/sous-répertoire GitHub, puis redémarrez un process Zode en cours pour qu'il charge le nouveau snapshot :

```bash
zode plugin validate ./my-plugin
zode plugin install ./my-plugin --trust
zode plugin install owner/repo@main#plugins/my-plugin --trust
zode plugin list
```

Utilisez `zode plugin update my-plugin` après avoir modifié la source. `--trust` est requis car JavaScript, hooks, serveurs MCP et accès réseau déclaré sont des capacités exécutables. Install et update impriment le grant de permissions déclaré par le plugin (hôtes réseau, variables d'env, scopes de contexte). Une mise à jour dont le manifest demande des permissions *plus larges* que le snapshot installé est refusée sauf si vous la relancez avec `--trust` — une source Git mouvante ne peut pas élargir silencieusement son propre grant.

#### API de rendu UI

Les plugins UI peuvent contribuer des lignes déclaratives juste au-dessus de la version dans la barre latérale — au plus six lignes au total, partagées entre tous les plugins dans l'ordre de chargement. Déclarez un point d'entrée JavaScript dans le manifest :

```json
{
  "name": "my-sidebar",
  "ui": {
    "sidebar": "./ui/sidebar.js",
    "statusLine": "./ui/status-line.js"
  }
}
```

Enregistrez un renderer synchrone avec `zode.ui.sidebar`. Le contexte est un snapshot JSON en lecture seule contenant les champs terminal, session, model, status, token et fenêtre de contexte. Le résultat est rendu par Zode ; les scripts ne reçoivent aucun pont filesystem, réseau, terminal ou Ratatui.

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

Les tones supportés sont `default`, `muted`, `accent`, `success`, `warning` et `danger` ; les spans acceptent aussi `bold` et `italic`. Un renderer doit être synchrone. Chaque script est limité à 256 KiB, 8 MiB de mémoire JS et 25 ms par évaluation, et les renderers sont réévalués au plus toutes les 250 ms (la sortie en cache est réutilisée entre évaluations). La sortie sidebar est limitée à 6 lignes par renderer (6 au total tous plugins confondus), chaque ligne à 16 spans et 2 048 octets de texte. Les caractères de contrôle sont assainis par l'hôte.

La barre d'état est aussi extensible. Elle reste une ligne quand aucun plugin ne renvoie de contenu et croît dynamiquement à deux lignes quand un renderer synchrone `zode.ui.statusLine` renvoie des spans. Zode garde son état core et ses indicateurs de sécurité sur la première ligne ; la sortie du plugin est composée sur la seconde.

```js
zode.ui.statusLine((ctx) => ({
  spans: [
    { text: ctx.session.title, tone: "accent", bold: true },
    { text: `  ↑${ctx.tokens.input} ↓${ctx.tokens.output}`, tone: "muted" }
  ]
}));
```

#### Contexte de rendu et permissions

Chaque renderer reçoit les champs de base suivants sans demander de permission de contexte supplémentaire :

| Champ | Forme et signification |
| --- | --- |
| `ctx.apiVersion` | Version de l'API de contexte ; actuellement `1`. |
| `ctx.app` | `{ version, effort }`. |
| `ctx.terminal` | `{ width, height }` en cellules terminal. |
| `ctx.session` | `{ id, title, cwd, busy }` pour la tâche active. |
| `ctx.model` | `{ id, provider }`. |
| `ctx.status` | `{ mode, planMode, selectionMode, yolo, sandbox }` ; `sandbox` contient `{ enabled, readOnly, network }`. |
| `ctx.tokens` | compteurs de tokens `{ input, output }`. |
| `ctx.context` | `{ used, window, usedPercent }` ; le pourcentage peut être `null`. |
| `ctx.data` | Résultats appartenant uniquement aux data sources enregistrées par ce plugin. |

Les sections plus riches sont omises sauf si le plugin demande le scope correspondant dans `permissions.context` :

| Scope | Champ exposé | Forme et limites |
| --- | --- | --- |
| `tabs` | `ctx.tabs` | `{ active, count }` ; `active` commence à 1. |
| `workspace` | `ctx.workspace.modifiedFiles` | Jusqu'à 50 entrées Git `{ path, added, removed }`. |
| `tools` | `ctx.tools.available` | Noms triés des outils activés pour la tâche active. |
| `tools` | `ctx.tools.active` | Noms des outils en cours d'exécution. |
| `tools` | `ctx.tools.recent` | Jusqu'à 20 enregistrements `{ name, status, durationMs }`. |
| `tasks` | `ctx.tasks.todoStatuses` | Chaînes de statut des todos uniquement, sans le texte du todo. |
| `tasks` | `ctx.tasks.subagents` | Enregistrements `{ type, status }`, sans prompts ni transcripts. |
| `tasks` | `ctx.tasks.goal` | `{ active, turn }`, sans le texte du goal. |
| `services` | `ctx.services.mcp` | Enregistrements `{ name, connected }`. |
| `services` | `ctx.services.lsp` | Enregistrements `{ language, running }`. |

Par exemple :

```json
{
  "permissions": {
    "context": ["tabs", "workspace", "tools", "tasks", "services"]
  }
}
```

`ctx.tools` est une API d'observation : elle indique à un renderer quels outils existent et lesquels sont ou ont été en cours d'exécution. Les plugins UI ne peuvent pas invoquer un outil. Les entrées et sorties d'outils, les prompts, le contenu des transcripts, le texte des todos/goals, les valeurs d'environnement et les identifiants ne sont pas inclus, et l'API ne peut pas contourner le système d'approbation de Zode.

#### Données HTTP en arrière-plan

Les plugins UI peuvent aussi enregistrer des data sources HTTP en arrière-plan. L'accès réseau et aux secrets doit être déclaré dans le manifest :

```json
{
  "permissions": {
    "network": ["quota.example.com"],
    "env": ["CODING_PLAN_TOKEN"]
  }
}
```

La requête est déclarative et s'exécute hors du chemin de rendu. Les variables d'environnement secrètes sont assemblées en headers par Zode et ne sont jamais exposées à JavaScript :

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

`zode.data.define(key, config)` accepte une clé de 1 à 64 caractères alphanumériques, underscore ou tiret. `request` supporte `url`, `method`, `headers`, un `body` JSON optionnel et `timeoutMs`. Les valeurs par défaut sont `GET`, un timeout de 3 secondes et un refresh de 60 secondes. Seuls HTTPS `GET` et `POST` sont acceptés. Les headers littéraux sont des chaînes ; un header secret utilise `{ "env": "NAME", "prefix": "Bearer " }`. La variable d'environnement doit aussi figurer dans `permissions.env`, n'est lue que par Rust lors de la construction de la requête, et n'est jamais renvoyée à JavaScript.

Zode désactive les redirections et proxies, valide et épingle les adresses DNS publiques, rejette localhost/réseaux privés, plafonne les réponses à 256 KiB, borne les timeouts de requête entre 500 ms et 10 secondes, et borne les intervalles de refresh entre 10 secondes et 1 heure. Un wildcard comme `*.example.com` matche les sous-domaines mais pas l'hôte nu `example.com`.

Chaque plugin ne voit que ses propres données. `ctx.data.<key>` contient `{ ok, status, data, updatedAt }` ou `{ ok: false, error, updatedAt }`. Les réponses JSON deviennent des objets/tableaux ; les réponses non-JSON deviennent des chaînes. Un statut d'erreur HTTP inclut toujours `status` et `data`, avec `ok: false`.

Démarrez Zode avec le secret requis dans son environnement quand vous utilisez un quota privé ou une API de coding plan :

```bash
CODING_PLAN_TOKEN=... zode
```

L'[exemple exécutable complet](../../examples/plugins/zode-ui-demo/) affiche l'activité modèle/contexte/outils dans la barre latérale et la barre d'état et utilise `zode.data.define` pour un quota d'API GitHub public.

```bash
zode plugin list --json
zode plugin details my-plugin
zode plugin disable my-plugin
zode plugin enable my-plugin
zode plugin update my-plugin
zode plugin uninstall my-plugin

# Un marketplace est un index statique local/Git, pas un service hébergé par Zode.
zode plugin marketplace add owner/plugin-index --trust
zode plugin marketplace list --json
zode plugin install my-plugin@MARKETPLACE_NAME --trust  # désambiguïser si besoin
zode plugin marketplace update
```

### ACP, dashboard, télémétrie et tests de régression TUI

`zode acp` implémente ACP initialize/new/load/fork/prompt/cancel sur stdio, streame les mises à jour message/thought/tool, demande les permissions via le client, et accepte des serveurs MCP stdio, HTTP et SSE fournis par le client. Les données de session utilisent le même store compatible V1 que la TUI et la CLI headless.

```bash
zode acp
zode dashboard
zode dashboard --json
```

L'export OTLP est désactivé par défaut et nécessite un opt-in explicite. Il n'exporte que des attributs de cycle de vie/nom d'outil/statut/usage sans contenu : prompts, texte généré, entrées/sorties d'outils, chemins de fichiers et messages d'erreur ne sont jamais envoyés.

```bash
ZODE_OTEL=1 \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
zode -p "run the test suite" --output-format json
```

Pour les scénarios de régression TUI en terminal réel, le workspace inclut un harnais PTY + VT100 qui enregistre des diagnostics bruts et des snapshots d'écran virtuel :

```bash
cargo test -p zode-pty-harness
cargo run -p zode-pty-harness --bin zode-pty-scenario -- scenario.json
```

`scenario.json` pilote le vrai terminal avec des waits ordonnés, des entrées clavier, des redimensionnements et des snapshots (la notation des touches supporte `<Enter>`, `<Esc>`, `<Tab>`, `<Up>`, `<Down>`, `<Left>`, `<Right>`, `<Backspace>`, `<C-c>`, `<C-d>` et `<C-l>`) :

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

Cette implémentation locale/open n'inclut délibérément pas de comptes, facturation spécifiques xAI ni de service marketplace cloud opéré par Zode.

Clés de config de premier niveau optionnelles (toutes avec des valeurs par défaut raisonnables) :

```jsonc
{
  "maxOutputTokens": 16384,      // plafond de sortie par tour (à monter pour les gros writes)
  "contextWindow": 1000000,      // fenêtre de contexte du modèle — mettre 1000000 pour un modèle 1M
  "temperature": 0,              // plus bas = plus déterministe
  "language": "zh-CN",           // langue UI (15 locales) ; aussi via /language
  "effort": "medium",            // effort de raisonnement ; sur Anthropic, medium/high correspondent à de vrais budgets de thinking
  "autonomousOrchestration": true, // orchestration sous-agents + workflows (activé par défaut)
  "subagentMaxIterations": 0,      // garde-fou enfant optionnel ; omis/0 = illimité
  "tools": {
    "deferNonCore": false        // true : garder ~20 outils du quotidien visibles, différer le reste derrière ToolSearch
  },
  "webSearch": {
    "tavilyApiKey": null         // active l'outil WebSearch (ou définissez $TAVILY_API_KEY)
  },
  "sandbox": {
    "enabled": true,             // sandbox OS pour les commandes shell (activé par défaut)
    "mode": "workspace-write",   // "workspace-write" | "read-only"
    "network": false,            // autoriser le réseau sortant dans la sandbox
    "writableRoots": []          // dossiers writables supplémentaires (workspace-write)
  },
  "browser": {
    "enabled": true,             // outils browser_* et panneau /browser (activé par défaut)
    "defaultTarget": "managed",  // "managed" | "bridge"
    "headless": false,           // mode de lancement du Chromium géré
    "viewport": { "width": 1440, "height": 900 }
  },
  "backgroundWatchdog": {
    "enabled": true,             // surveiller les tours /loop et /schedule non surveillés
    "inactivityTimeoutSecs": 900, // abandonner après 15 minutes sans activité provider/outil
    "maxRuntimeSecs": 3600,      // plafond absolu d'une heure par tour d'arrière-plan
    "abortGraceSecs": 10,        // attendre l'annulation coopérative avant l'arrêt dur
    "maxRetries": 3,             // tentatives de récupération consécutives avant épuisement
    "initialBackoffSecs": 5,     // délai de la première retry
    "maxBackoffSecs": 300        // plafond du backoff exponentiel des retries
  }
}
```

> La sandbox confine les commandes shell (macOS : sandbox-exec ; Linux : `bwrap`, qui doit être installé). Le démarrage échoue fermé si la sandbox configurée ne peut pas être vérifiée ; utilisez le drapeau explicite `--no-sandbox` pour tourner sans. Le réseau est refusé par défaut. Si une commande a réellement besoin de s'échapper, le modèle met `dangerouslyDisableSandbox: true` et **vous** l'autorisez à l'invite d'approbation — ou basculez toute la sandbox à chaud avec `/sandbox`.

> `contextWindow` pilote l'auto-compaction — réglez-le sur la fenêtre réelle de votre modèle (par ex. `1000000`). Préférez la valeur **par modèle** sous `providers.<name>.models.<id>.contextWindow` (elle prime) ; la clé de premier niveau ci-dessus est un fallback global, et zode la remplit aussi depuis le catalogue models.dev embarqué quand aucune n'est définie. Ne la réglez **pas** au-dessus de la fenêtre réelle : surestimer fait déborder les requêtes et le provider rejette le tour.

## Mode serveur et SDKs

`zode server` démarre un serveur JSON-RPC newline-delimited sur stdin/stdout. Il est prévu pour les intégrations d'éditeurs, l'automatisation locale, les tests et les clients SDK qui veulent les capacités existantes de zode sans lancer la TUI.

```bash
zode server                      # stdio (par défaut) — ce que les SDKs lancent
zode server --listen stdio://    # la même chose, explicité
zode server --listen ws://127.0.0.1:0   # WebSocket loopback + auth Bearer
zode server --listen off         # ne rien démarrer et sortir
```

Le mode serveur expose des comportements adossés à zode :

- initialisation + découverte de capacités (avec une `approvalPolicy` `readOnly` (défaut) / `auto` / `prompt`)
- cycle de vie des métadonnées de thread et **tours en streaming** — la sortie du modèle et les appels d'outils arrivent en notifications JSON-RPC ; `turn/interrupt` annule un tour
- **approbations interactives** — la politique `prompt` pilote des frames `approval/request` serveur→client répondues par `allow` / `allowAlways` / `deny`
- read/write/create/stat/list/remove/copy de filesystem et un `command/exec` one-shot
- model list/set, config read/list/write, et skills, hooks, statut des serveurs MCP et listes de plugins en lecture seule

Le transport WebSocket se lie uniquement en loopback et écrit un fichier d'identifiants `0600` `<config-dir>/server.json` (`{port, pid, token}`) ; les clients s'authentifient avec `Authorization: Bearer <token>`. Voir [`sdk/README.md`](../../sdk/README.md) pour le protocole complet, les noms de champs de notification et des exemples par langage.

Pour ce protocole app-server spécifiquement, la gestion de marketplace hébergée, le remote-control, Realtime, le spawn de process autonome, les terminaux d'arrière-plan, l'archivage/fork de thread, les goals et les app connectors restent hors périmètre. Les commandes locales de session et de marketplace de plugins statiques documentées ci-dessus sont des surfaces CLI distinctes.

Les SDKs vivent sous [`sdk/`](../../sdk/) :

| SDK | Dossier | Test local |
|-----|---------|------------|
| Rust | [`sdk/rust`](../../sdk/rust/) | `cargo test -p zode-sdk-rust` |
| TypeScript | [`sdk/typescript`](../../sdk/typescript/) | `pnpm --dir sdk/typescript test` |
| Python | [`sdk/python`](../../sdk/python/) | `PYTHONPATH=sdk/python/src python3 -m unittest discover -s sdk/python/tests` |
| Go | [`sdk/go`](../../sdk/go/) | `(cd sdk/go && go test ./...)` |
| Kotlin/JVM | [`sdk/kotlin`](../../sdk/kotlin/) | `(cd sdk/kotlin && gradle test)` |

Chaque SDK expose un ensemble natif d'enum/constant `ProtocolMethod` pour les noms de méthodes stables actuels, afin que les intégrations évitent les chaînes JSON-RPC codées en dur. Les params, la forme de résultat et le nom d'enum/constant SDK de chaque méthode supportée sont documentés dans la [référence des méthodes de `sdk/`](../../sdk/README.md#method-reference).

Lancez les checks SDK disponibles sur votre machine avec :

```bash
scripts/test-sdks.sh
```

Les fixtures de protocole sont générées depuis `zode-app-server-protocol` :

```bash
cargo run -p zode-app-server-protocol --bin export -- sdk/fixtures/jsonrpc
```

## Slash commands

| Commande | Ce qu'elle fait |
|---|---|
| `/help` | Overlay commandes + raccourcis |
| `/clear` | Effacer la conversation (et le contexte) |
| `/model [id]` | Afficher / noter le modèle actif |
| `/config` | Afficher le modèle + le répertoire de travail |
| `/compact` | Statut d'auto-compaction du contexte |
| `/cost` | Usage des tokens et coût jusqu'ici (sous-agents inclus) |
| `/theme [id]` | Changer de thème (`catppuccin-mocha`, `aurora-forge`, `ember-atelier`, `sakura-paper`, `arctic-day`, `lavender-mist`, `citrus-grove`, `verdant-signal`, `cyberpunk`, `minimal`, `hacker`) |
| `/sessions`, `/resume` | Sélecteur de session — reprendre dans un nouvel onglet avec l'historique |
| `/connect` | Connecter et changer le provider actif |
| `/sidebar [on\|off\|toggle\|auto\|mcp\|files\|todo]` | Afficher/masquer la barre latérale ; replier les sections MCP / fichiers modifiés / todo (ou cliquer sur leurs en-têtes ▼) |
| `/browser [status\|launch\|close\|pair\|target <managed\|bridge>\|screenshot [path]]` | Panneau et commandes de contrôle navigateur ; appairer l'extension Chrome bridge ou basculer entre Chromium géré et votre profil Chrome |
| `/desktop [status]` | Afficher la cible bureau et l'état des permissions |
| `/loop <interval> [--max N] <prompt>` | Lancer un prompt récurrent dans l'onglet courant ; `list` / `stop [id]` |
| `/schedule add <when> <prompt>` | Persister un prompt planifié ; `list` / `rm <id>` / `enable\|disable <id>` |
| `/watchdog [status]` | Afficher la config du watchdog de tours d'arrière-plan, la santé et les retries en attente |
| `/tasks` | Panneau des shells d'arrière-plan, tours en cours et santé du watchdog |
| `/undo`, `/redo` | Annuler / rétablir la dernière édition de fichier |
| `/mcp` | Gérer les serveurs MCP — activer / désactiver dans un dialogue |
| `/skills` | Lister les skills disponibles |
| `/agents` | Gérer les sous-agents — créer (assisté par IA ou manuel) / supprimer |
| `/external-agents [list\|discover]` | Lister les CLI externes supportées dans `PATH`, ou enregistrer explicitement chaque preset détecté |
| `/team [status\|board\|dismiss <name>]` | Inspecter le roster de teammates persistants et le board partagé, ou retirer un teammate |
| `/workflows` | Gérer et lancer des workflows scriptés en JS (orchestration `agent()`/`parallel()`/`pipeline()`, exécutée déterministe par zode) |
| `/effort` | Choisir le niveau d'effort de raisonnement |
| `/thinking`, `/tool-details` | Basculer l'affichage du raisonnement / du détail des appels d'outils |
| `/orchestration` | Basculer l'orchestration autonome sous-agents + workflows |
| `/sandbox [on\|off\|read-only\|workspace-write\|network on\|network off]` | Afficher / contrôler la sandbox OS à l'exécution |
| `/language` | Changer la langue de l'UI (15 locales) |
| `/export [path]` | Exporter le transcript en Markdown (un dossier reçoit un nom par défaut) |
| `/yolo` | Mode contournement d'approbation |
| `/exit` | Quitter |

Les agents et skills créés, et les outils MCP connectés, apparaissent aussi comme des slash commands dynamiques (par ex. `/<name>`) et peuvent être invoqués directement.

## Raccourcis clavier

> Sur macOS, les chords d'application ci-dessous utilisent **`Cmd`** (⌘) ; sur Windows/Linux ils utilisent `Ctrl`. `Ctrl+C/D/L/V` restent `Ctrl` partout (conventions terminal).

| Touche | Action |
|---|---|
| `Enter` | Envoyer le message (met en file si un tour tourne) |
| `Shift`/`Alt`+`Enter` | Nouvelle ligne |
| `Up` / `Down` | Rappeler le prompt précédent / suivant (ou déplacer la sélection d'autocomplétion) |
| `Ctrl+C` | Interrompre le tour (quitte quand inactif) |
| `Ctrl+D` | Quitter |
| `Ctrl+L` | Redessiner la conversation depuis le store (récupère une vue vidée ; utilisez `/clear` pour jeter) |
| `Ctrl+V` | Coller (texte ou chemins d'image) |
| `Cmd/Ctrl+O` | Settings |
| `Cmd/Ctrl+T` / `Cmd/Ctrl+W` | Nouvel onglet / fermer l'onglet |
| `Cmd/Ctrl+1`–`9` / `Cmd/Ctrl+Tab` | Aller à / cycler les onglets |
| `Cmd/Ctrl+B` | Panneau des tâches d'arrière-plan |
| `Cmd/Ctrl+G` | Basculer la barre latérale |
| `F1` | Aide |
| `PgUp` / `PgDn` | Faire défiler la conversation |
| `Home` / `End` | Aller en haut / au plus récent de la conversation |
| `Esc` | Fermer l'overlay courant (ou interrompre un tour en cours) |

## Instructions de projet

Zode lit les instructions depuis une hiérarchie à trois niveaux (le plus tardif gagne l'attention) : global `~/.zode/AGENTS.md` (ou `instructions.md`) → racine du projet → cwd. Dans chaque répertoire, il préfère `AGENTS.md` à `CLAUDE.md`. Les skills vivent sous `.zode/skills/**/SKILL.md` ; les serveurs MCP dans `~/.zode/mcp.json` ⊕ `.mcp.json` ; les hooks dans `~/.zode/hooks.json` ⊕ `.zode/hooks.json`.

**Configuration cross-agent.** Zode lit directement les skills et la configuration MCP de Claude Code, Codex, Cursor, opencode, Gemini et agents locaux apparentés. Les arbres de plugins installés et les caches de plugins de ces produits ne sont jamais scannés. Pour réutiliser un plugin, installez sa source explicitement avec `zode plugin install ... --trust` ; les formats de paquet Codex et Claude Code restent supportés pour les plugins installés via Zode.

## Configurer les serveurs MCP

Les serveurs MCP vivent dans la même config à précédence imbriquée que le reste — `~/.zode/mcp.json` pour tous les projets, `.mcp.json` ou `.zode/mcp.json` à la racine du projet pour en cadrer un à un dépôt. Pas de registre, pas de restart-and-pray : éditez le fichier, puis `/mcp` (ou relancez) pour le prendre en compte.

### stdio (lancer un serveur local)

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

`command`/`args` lancent le serveur comme sous-processus piped sur stdio. Les valeurs `env` supportent la substitution `$NAME` / `${NAME}` contre l'environnement du process de zode (étendue juste avant la connexion, pas écrite sur disque) — pratique pour garder les tokens hors du fichier de config lui-même.

### Streamable HTTP (serveur distant)

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

`"transport": "http"` se connecte avec le transport Streamable HTTP de la spec MCP actuelle — un seul `url`, aucun endpoint SSE séparé à configurer. `"sse"` est accepté comme orthographe équivalente (certaines configs — et les docs de setup des serveurs MCP eux-mêmes — l'appellent encore ainsi) ; les deux résolvent vers le même connecteur. Les `headers` sont transférés verbatim (y compris `Authorization`, donc les schémas Bearer/Basic/custom fonctionnent tous) et supportent la même substitution `$VAR` que `env`. Ajoutez `"enabled": false` à un serveur pour garder sa définition sans le connecter — `/mcp` bascule aussi cela par serveur sans éditer le fichier à la main.

### L'utiliser

Chaque outil exposé par un serveur connecté apparaît comme `mcp__<server>__<tool>`, appelable par l'agent comme tout outil intégré (et `@`-mentionnable dans la boîte de saisie). `/mcp` ouvre un dialogue listant chaque serveur découvert — connecté / déconnecté / désactivé — avec Espace pour en basculer un ; la section `mcp` repliable de la barre latérale (cliquez son en-tête ▼, ou `/sidebar mcp`) reflète le même état de connexion en direct d'un coup d'œil.

Zode lit aussi la configuration MCP directe de Claude Code, Codex, Cursor, opencode et Gemini. La config du home est traitée comme le setup de l'utilisateur ; les définitions MCP étrangères locales au projet sont découvertes désactivées et peuvent être activées via `/mcp`. Les déclarations MCP enfouies dans l'arbre de plugins installé d'un autre produit ne sont pas scannées. `openpencil` est réservé — op-bridge le pilote nativement, donc tout serveur déclaré sous ce nom est ignoré.

## Installer skills et commandes Markdown

Les deux sont du Markdown simple sur disque — pas de registre, pas d'étape de build. Déposez un fichier, et il est actif au prochain lancement (ou `/skills` pour vérifier ce qui a chargé).

### Installer un skill

Un skill est un dossier contenant un `SKILL.md`. Placez-le sous le projet (`.zode/skills/`) ou votre home (`~/.zode/skills/`) :

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

Le skill apparaît maintenant dans `/skills`, l'agent peut l'invoquer lui-même via l'outil Skill, et il devient aussi une slash command dynamique — taper `/code-review look at src/lib.rs` s'étend en un prompt qui lance le skill. Les fichiers supplémentaires à côté de `SKILL.md` (références, scripts) sont livrés avec le skill. Les répertoires de skills directs de Claude Code, Codex, opencode, Cursor et agents apparentés sont scannés. Les skills enfouis dans les arbres de plugins installés ou caches de ces produits ne le sont pas ; installez le plugin explicitement via Zode si vous voulez l'utiliser ici.

### Installer une commande (prompt Markdown)

Une slash command custom est un unique fichier `.md` dont le **nom de fichier est le nom de la commande** et dont le corps est le prompt qu'elle soumet. Tout ce que vous tapez après la commande est ajouté au corps :

```bash
mkdir -p .zode/commands            # ou ~/.zode/commands pour tous les projets
cat > .zode/commands/changelog.md <<'EOF'
Update CHANGELOG.md for the changes in the current working tree.
Follow Keep-a-Changelog headings and write entries in imperative mood.
EOF
```

Maintenant `/changelog` soumet ce prompt, et `/changelog only the sidebar work` ajoute vos arguments après. Les commandes dans `~/.claude/commands` et `~/.codex/commands` (et leurs équivalents au niveau projet) sont chargées aussi ; les commandes dans un *arbre de plugin étranger* sont désactivées par défaut — copiez le `.md` dans un dossier `.zode/commands/` pour opter dedans.

## Écosystème ZSeven-W

Zode fait partie d'un stack ZSeven-W plus large d'outils de développement AI-native :

| Produit | Ce que c'est |
|---------|------|
| [`agent-rs`](https://github.com/ZSeven-W/agent-rs) | Runtime async en Rust pur pour shipper des LLM agents : streaming multi-provider, tool dispatch, permissions, MCP, suivi des coûts, attachments, sessions et coding tools optionnels. |
| [`jian`](https://github.com/ZSeven-W/jian) | Framework UI cross-platform natif Rust où un fichier `.op` est une app, reliant les artefacts de design façon OpenPencil à un logiciel exécutable. |
| [`noema`](https://github.com/ZSeven-W/noema) | Système mémoire local-first et non-vectoriel pour coding agents, avec lexical recall, review queues, accès MCP, S3 offload et enterprise policy controls. |
| [`openpencil`](https://github.com/ZSeven-W/openpencil) | Outil open-source AI-native de design vectoriel pour workflows design-as-code, transformant les prompts en UI directement sur un live canvas avec concurrent agent teams. |

## Benchmark

Les benchmarks de Zode couvrent la génération de code one-shot, le travail agentic lire/exécuter/éditer/corriger, les tâches multi-fichiers, les bugs difficiles, le suivi d'instructions MCP/Skills/contraintes, ainsi que le runner Noema LOCOMO. Sur cinq dimensions, **Zode + DeepSeek-v4-pro rivalise avec Claude**, chaque tâche étant notée par un grader *caché*. La méthodologie complète, les commandes de reproduction et les tables de résultats sont dans la [section Benchmark du README anglais](../../README.md#benchmark), et les suites vivent dans [`benchmarks/`](../../benchmarks/).

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

Les contributions sont bienvenues. Suivez [Conventional Commits](https://www.conventionalcommits.org/) : `<type>(<scope>): <subject>`, avec des scopes courants comme `core`, `tui`, `cli`, `tools`, `config`, `build`, `ci`, `docs`.

## Licence

[MIT](../../LICENSE) &copy; ZSeven-W
