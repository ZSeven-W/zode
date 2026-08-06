# zode v0.2.0-beta.1

**zode** is an open-source, AI-native coding assistant for your terminal: it
reads your code, runs commands, searches files, and manages git from a fast
Rust TUI with non-blocking permissions and an on-by-default OS sandbox.

> ⚠️ **Beta.** APIs, configuration, and behavior may change before 0.2.0 is
> stable. Please file issues with a minimal reproduction when something goes
> wrong.

This beta sharpens the two places where an agentic coding session spends most
of its time: pairing with the browser and understanding what a tool is about to
change. It also keeps runtime-only prompts out of the user's transcript and
recall history.

## Chrome Web Store pairing by default

- **The production extension is the default.** zode now accepts the Chrome Web
  Store extension ID out of the box and opens its pairing page by default.
- **Local extension builds remain first-class.** Put a development extension ID
  first in `browser.extensionIds`; the bridge and automatically opened pairing
  page then use that same ID.
- **Store listing assets are included.** The extension documentation now ships
  the screenshots and source artwork used for the Chrome Web Store listing.

## File edits you can understand before approving

- **Inline edit previews.** FileWrite and FileEdit tool calls show a numbered,
  bounded diff while the on-disk pre-image is still available, so the approval
  dialog and transcript describe the same pending change.
- **More useful tool folds.** Multi-row tool activity keeps an eight-row
  preview and folds only the tail. Long commands and diffs stay visible;
  thinking blocks remain compact.

## A cleaner transcript and prompt history

Reminder blocks, post-compaction restore messages, and scheduler/goal-loop
driver prompts are runtime context rather than user-authored text. This release
keeps them out of chat bubbles, Up/Down prompt recall, and newly loaded
persisted history.

## Release and test reliability

- The Rust, TypeScript, Python, Go, and Kotlin SDK metadata, protocol fixtures,
  and Chrome extension are synchronized at `0.2.0-beta.1`.
- The external-agent process-group cleanup test now has a longer, load-tolerant
  reaping window; normal successful cleanup still returns immediately.

## Install

### One line (recommended)

**macOS / Linux:**

```sh
curl -fsSL https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ZSeven-W/zode/main/scripts/install.ps1 | iex
```

The installers auto-detect your OS and CPU, download the matching binary, and
put `zode` on your PATH. To pin this beta, use `--version v0.2.0-beta.1` on
macOS/Linux or `$env:ZODE_VERSION='v0.2.0-beta.1'` on Windows.

Already installed? Run `zode update` (or `zode upgrade`) to fetch this beta;
the updater includes prereleases when selecting the newest available version.

> Because this is a pre-release, GitHub's “latest” endpoint may exclude it.
> zode's installers resolve the newest release including betas.

### Manual download

| OS | Architecture | Asset |
|----|--------------|-------|
| macOS | Apple Silicon (M1+) | `zode-0.2.0-beta.1-arm64-mac.tar.gz` |
| macOS | Intel | `zode-0.2.0-beta.1-x64-mac.tar.gz` |
| Linux | x86_64 | `zode-0.2.0-beta.1-x64-linux.tar.gz` |
| Linux | ARM64 (aarch64) | `zode-0.2.0-beta.1-arm64-linux.tar.gz` |
| Windows | x64 | `zode-0.2.0-beta.1-x64-windows.zip` |
| Windows | ARM64 | `zode-0.2.0-beta.1-arm64-windows.zip` |

### From source

```sh
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode && cargo build --release -p zode
```

## Supported platforms

Prebuilt binaries are provided for macOS (arm64 and x64), Linux (x86_64 and
arm64, glibc), and Windows (x64 and arm64). Every architecture is built and
packaged independently in CI.

## Notes and caveats

- Linux builds are glibc-linked; a static musl build is not included.
- The OS sandbox uses `sandbox-exec` on macOS and `bwrap` on Linux. Windows
  defaults to a restricted-token sandbox; opt into AppContainer Tier 2 with
  `sandbox.windowsTier: "elevated"` when network denial is required.
- macOS binaries are ad-hoc signed but not notarized. A manually downloaded
  archive may need `xattr -dr com.apple.quarantine ./zode` once.

---

**Full changelog:** https://github.com/ZSeven-W/zode/compare/v0.1.0...v0.2.0-beta.1
