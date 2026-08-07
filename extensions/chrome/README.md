# zode Chrome bridge

This MV3 extension lets zode drive the Chrome window you are already using, including logged-in sites. It connects only to a local `ws://127.0.0.1:<port>` bridge started by zode.

**Chrome Web Store:** <https://chromewebstore.google.com/detail/zode/hmnlhofbekmkhmifkfkkmmpigijlkcca>

Extension ID: `hmnlhofbekmkhmifkfkkmmpigijlkcca`
Default bridge port: `17657`

## Install

1. Add it from the [Chrome Web Store](https://chromewebstore.google.com/detail/zode/hmnlhofbekmkhmifkfkkmmpigijlkcca).
2. In zode, run `/browser pair` — zode opens the extension page with the WS port and pairing code pre-filled and auto-connects it.
3. Run `/browser target bridge` before using browser tools against this Chrome profile.

A locally built extension gets a different ID (the manifest's own `key` for an
unpacked/CRX load), which zode does not accept by default. Point it at yours —
this also steers the popup `/browser pair` auto-opens:

```json
{ "browser": { "extensionIds": ["<your-local-id>"] } }
```

## Load unpacked

1. Build the React side panel with `npm --prefix extensions/chrome install` and `npm --prefix extensions/chrome run build`.
2. Open `chrome://extensions`.
3. Enable Developer mode.
4. Click Load unpacked and choose this `extensions/chrome` directory.
5. In zode, run `/browser pair`; zode opens the extension page with the WS port and pairing code pre-filled and auto-connects it.
6. Run `/browser target bridge` before using browser tools against this Chrome profile.

## Task side panel

Run the updated zode CLI once and execute `/browser pair` to register the local
Native Messaging host and open the lightweight pairing page. After this one-time
setup, opening the side panel auto-starts zode automatically when no CLI process
is running, then reconnects with the stored token. Clicking the toolbar icon
opens the side panel where tasks run.

The stable `sidepanel.html` entry is implemented with React, TypeScript,
Tailwind CSS, and local shadcn/ui components. Source files live in
`extensions/chrome/src/`; Vite writes the CSP-safe extension assets to
`extensions/chrome/dist/`. The existing task controller and protocol state
machine remain framework-independent and continue to be covered by the Node
test suite. The extension ships with zode and carries the same version: the
manifest's `version_name` is the full workspace version (for example
`0.2.0-beta.1`) and `version` is its numeric core, because Chrome only accepts
dotted integers there. Native Messaging auto-start (the daemon uses the last
workspace registered by a normal zode launch) and the element picker described
below both arrived with this numbering; extensions built before it used an
independent 0.x line that ran ahead of zode's.

Side-panel tasks are shared with the TUI sessions: creating or selecting a task
does not switch the terminal's focused tab, and the same history remains
available from either surface. The side panel supports text tasks, model
selection, access modes `readOnly`, `prompt`, and `auto`, streaming output, and
Stop for the active turn. `prompt` displays approval choices; `readOnly` does
not expose mutating tools, while `auto` allows eligible interactions without a
prompt but does not bypass hard deny or sandbox rules.

When a turn is submitted from the side panel, bridge browser tools target the
page currently shown beside it. This lets `browser_read` inspect that page
without opening another tab. The page stays in its existing tab group; only an
explicit `browser_tabs` action creates or selects a different tab.
Side-panel prompts also treat that page as their primary context: ambiguous
phrases such as “this” or “current page” trigger a page read before an answer,
while the local workspace is used only when the prompt explicitly asks about
the project, code, or files.

The composer's element button starts a DevTools-style picker on that page:
hover highlights elements, a click captures the one under the cursor, and the
picked element rides along with the next turn as a chip above the input. The
picker uses the debugger access the bridge already holds, so it needs no extra
permission and no content script; it runs on `http`, `https`, and `file` pages
only. A pick is cancelled by pressing the button again, pressing Esc in the
panel, navigating or closing the page, or after two minutes. zode receives the
element's unique CSS selector, tag, visible text, bounding box, attributes, and
a capped `outerHTML` snippet, plus the page title and URL, and can act on it
with `browser_read`, `browser_act`, and `browser_eval`. The value of a password
input is never captured. Sending a picked element requires a zode build that
understands it; an older zode reports that the selection is unsupported and the
chip is kept so the turn can be retried without it.

Images can be pasted straight into the composer: a clipboard screenshot
becomes an attachment named `pasted-<stamp>.<ext>` and is previewed as a
thumbnail above the input, next to any element chip. Pasting text still fills
the draft as usual — only an actual image on the clipboard is intercepted.
After the turn is sent, the same picture is shown inside its user message in
the transcript. Those previews are panel-local `blob:` URLs held for the last
40 turns, so they disappear when the panel reloads or reloads its history from
an authoritative snapshot; the image itself still reached the model.

Each turn accepts at most 8 files and 20 MiB total. Supported images are PNG,
JPEG, GIF, and WebP, up to 5 MiB each. UTF-8 text and code files are supported
up to 1 MiB each, including Markdown, JSON, Rust, JavaScript/TypeScript, Python,
Go, Java/Kotlin, shell, CSS, HTML, TOML, and YAML. PDF, Office documents,
archives, executables, and non-UTF-8 text are rejected.

After updating the extension, use the Reload button on `chrome://extensions`
before testing the side panel. An older extension version remains compatible
with existing browser automation, but a previous version may not contain the
task side panel; reload it from this directory for task dispatch. Because the
version now tracks zode, it is numerically lower than the extension's old
independent line — Chrome refuses to *update* a packed install to a lower
version, so remove that install and load this directory unpacked instead.

Chrome blocks `chrome-extension://` URLs opened by external programs
(ERR_BLOCKED_BY_CLIENT on every OS), so zode cannot reliably open the pairing
page for you. Instead the extension polls the local bridge (a pre-auth probe
that reveals only "is a pairing window open") and opens its own pairing page
within ~30 seconds of `/browser pair`. Manual fallback: type the
`chrome-extension://…/popup.html?port=…` URL into the address bar yourself —
typed navigation is allowed.

After the first pairing, the extension stores a token — pairing is one-time.
The service worker reconnects with that token automatically: on browser
startup, on extension install/update, and (while disconnected) on a one-minute
`chrome.alarms` cadence, so restarting zode never requires re-pairing. The
automatic path only ever *reconnects* to an already-listening zode; it never
launches one. Separately, when the side panel is opened while zode is not
running, the extension starts an extension-only background daemon through
Chrome Native Messaging and reconnects to its local WebSocket. Closing Chrome
or unloading the extension closes the native port and stops that daemon. Tabs
opened through zode's bridge are grouped into a Chrome tab group named `zode`.

The extension also reports downloads created after the current bridge WebSocket
connection was established. It never searches or returns earlier Chrome profile
download history. Downloads that cannot be tied to the controlled zode tab are
reported with `profile` or `unknown` attribution.

## Opening the extension on Windows

On Windows, zode launches Chrome directly for extension pairing and reconnect
URLs instead of delegating them to the default-browser shell.

On Windows, zode locates Chrome in this exact order:

1. The `browser.executable` configuration value.
2. Chrome under `%LOCALAPPDATA%`.
3. Chrome under `%ProgramFiles%`.
4. Chrome under `%ProgramFiles(x86)%`.
5. `chrome.exe`, then `google-chrome.exe`, from `PATH`.

Zode executes Chrome directly instead of using `cmd` or the default browser.
This avoids Microsoft Store and default-browser indirection for the
`chrome-extension://` URL, which is tied to zode's stable extension ID. If no
Chrome executable is found, the error lists every attempted location and keeps
the full extension URL so you can open it manually.

## Tab behavior

The bridge chooses its initial tab based on where the task starts:

- A turn submitted from the side panel targets the active page beside the
  panel, so it can analyze that page without creating a tab.
- Standalone TUI or CLI bridge automation creates a background `about:blank`
  tab inside the blue `zode` tab group instead of taking over a human tab.
- After acquisition, all actions keep targeting the chosen tab even when you
  switch tabs.
- If you navigate the zode tab yourself via the address bar, a bookmark, or
  an omnibox search — or click "Cancel" on Chrome's debugging bar — zode
  abandons it and creates a fresh tab for its next action. Clicking links
  inside the zode tab is indistinguishable from zode's own clicks and does
  not hand off.
- Screenshots of a background zode tab briefly activate it (hidden tabs do not
  render) and then restore the tab you were on.

## Theme-adaptive icon

The toolbar icon follows the browser's light/dark appearance: an offscreen
document watches `prefers-color-scheme` and the service worker swaps between
`icons/zode-*.png` (dark appearance) and `icons/zode-light-*.png` (light
appearance) at runtime. The icon on `chrome://extensions` stays the static
manifest one — Chrome offers no way to switch it. Light icons are generated
from `assets/logo-light.png` at the repo root with `sips -Z <size>`.

## Update

After changing files in this directory, open `chrome://extensions` and click
the Reload button on the zode card (the extension is named "zode"; it was
previously listed as "zode browser bridge"). The side panel and its permission
are not in every older build, so a stale extension must be reloaded before
clicking the toolbar icon can open it. The manifest embeds a
public key so unpacked and packed installs keep the same extension ID, which
zode uses for the WebSocket Origin check.

## Pack CRX (enterprise and automated testing only)

Do not drag the generated CRX into a regular Chrome installation. Current
Chrome releases can reject off-store packages with
`CRX_REQUIRED_PROOF_MISSING` because they do not carry Chrome Web Store proof.
For local use, follow **Load unpacked** above and select this
`extensions/chrome` directory. The manifest public key keeps the unpacked
extension ID stable.

Run:

```bash
extensions/chrome/pack.sh
```

The script creates or reuses `extensions/chrome/zode-bridge.pem`, or reuses an existing legacy `extensions/zode-bridge.pem`, prints the manifest public key and extension ID, and writes `extensions/chrome/zode-bridge.crx`. Keep the `.pem` private; it is ignored by git. Reusing the same `.pem` preserves the extension ID across future CRX builds.

The CRX remains useful for ChromeDriver and for managed environments that allow
off-store installation through enterprise policy. Public distribution should
use the Chrome Web Store.

## Smoke test

Run `/browser pair`, confirm the extension page opens and closes after connecting, confirm the `/browser` panel shows `Extension: Paired`, switch to `/browser target bridge`, then ask the agent to open a new tab and take a browser screenshot. The new tab should appear in the `zode` Chrome tab group.
