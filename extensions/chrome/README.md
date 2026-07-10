# zode Chrome bridge

This MV3 extension lets zode drive the Chrome window you are already using, including logged-in sites. It connects only to a local `ws://127.0.0.1:<port>` bridge started by zode.

Stable extension ID: `hcabdgpfhoclfgnknddadgfhhdnlkloc`
Default bridge port: `17657`

## Load unpacked

1. Open `chrome://extensions`.
2. Enable Developer mode.
3. Click Load unpacked and choose this `extensions/chrome` directory.
4. In zode, run `/browser pair`; zode opens the extension page with the WS port and pairing code pre-filled and auto-connects it.
5. Run `/browser target bridge` before using browser tools against this Chrome profile.

After the first pairing, the extension stores a token. It stays idle while zode is not running; when zode is running with bridge target selected, zode opens the extension page to reconnect with the stored token. Tabs opened through zode's bridge are grouped into a Chrome tab group named `zode`.

## Tab behavior

The bridge drives one sticky zode-owned tab instead of whatever tab you are
viewing:

- When zode needs a tab it always creates a fresh background `about:blank`
  tab inside the blue `zode` tab group. It never takes over a tab you opened
  and never steals focus — click into the zode group to watch it work.
- All actions keep targeting that tab even when you switch tabs.
- If you navigate the zode tab yourself via the address bar, a bookmark, or
  an omnibox search — or click "Cancel" on Chrome's debugging bar — zode
  abandons it and creates a fresh tab for its next action. Clicking links
  inside the zode tab is indistinguishable from zode's own clicks and does
  not hand off.
- Screenshots briefly activate the zode tab (hidden tabs do not render) and
  then restore the tab you were on.

## Update

After changing files in this directory, open `chrome://extensions` and click the reload button on the zode browser bridge card. The manifest embeds a public key so unpacked and packed installs keep the same extension ID, which zode uses for the WebSocket Origin check.

## Pack CRX

Run:

```bash
extensions/chrome/pack.sh
```

The script creates or reuses `extensions/chrome/zode-bridge.pem`, or reuses an existing legacy `extensions/zode-bridge.pem`, prints the manifest public key and extension ID, and writes `extensions/chrome/zode-bridge.crx`. Keep the `.pem` private; it is ignored by git. Reusing the same `.pem` preserves the extension ID across future CRX builds.

Modern Chrome restricts off-store CRX installs. Developer builds usually need Load unpacked, while managed environments can allow CRX installs with enterprise policy such as `ExtensionInstallAllowlist` for the fixed extension ID above.

## Smoke test

Run `/browser pair`, confirm the extension page opens and closes after connecting, confirm the `/browser` panel shows `Extension: Paired`, switch to `/browser target bridge`, then ask the agent to open a new tab and take a browser screenshot. The new tab should appear in the `zode` Chrome tab group.
