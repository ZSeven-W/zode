# zode Chrome bridge

This MV3 extension lets zode drive the Chrome window you are already using, including logged-in sites. It connects only to a local `ws://127.0.0.1:<port>` bridge started by `/browser pair`.

Stable extension ID: `hcabdgpfhoclfgnknddadgfhhdnlkloc`

## Load unpacked

1. Open `chrome://extensions`.
2. Enable Developer mode.
3. Click Load unpacked and choose this `extensions/chrome` directory.
4. In zode, run `/browser pair`.
5. Open the zode bridge extension popup, enter the WS port and 6-digit pairing code, then click Pair / Reconnect.
6. Run `/browser target bridge` before using browser tools against this Chrome profile.

## Update

After changing files in this directory, open `chrome://extensions` and click the reload button on the zode browser bridge card. The manifest embeds a public key so unpacked and packed installs keep the same extension ID, which zode uses for the WebSocket Origin check.

## Pack CRX

Run:

```bash
extensions/chrome/pack.sh
```

The script creates or reuses `extensions/chrome/zode-bridge.pem`, prints the manifest public key and extension ID, and writes `extensions/chrome/zode-bridge.crx`. Keep the `.pem` private; it is ignored by git. Reusing the same `.pem` preserves the extension ID across future CRX builds.

Modern Chrome restricts off-store CRX installs. Developer builds usually need Load unpacked, while managed environments can allow CRX installs with enterprise policy such as `ExtensionInstallAllowlist` for the fixed extension ID above.

## Smoke test

Run `/browser pair`, enter the shown port and code in the popup, confirm the `/browser` panel shows `Extension: Paired`, switch to `/browser target bridge`, then ask the agent to take a browser screenshot.
