# zode Chrome bridge

This MV3 extension lets zode drive the Chrome window you are already using, including logged-in sites. It connects only to a local `ws://127.0.0.1:<port>` bridge started by `/browser pair`.

## Load unpacked

1. Open `chrome://extensions`.
2. Enable Developer mode.
3. Click Load unpacked and choose this `extensions/chrome` directory.
4. In zode, run `/browser pair`.
5. Open the zode bridge extension popup, enter the WS port and 6-digit pairing code, then click Pair / Reconnect.
6. Run `/browser target bridge` before using browser tools against this Chrome profile.

## Update

After changing files in this directory, open `chrome://extensions` and click the reload button on the zode browser bridge card.

## Smoke test

Run `/browser pair`, enter the shown port and code in the popup, confirm the `/browser` panel shows `Extension: Paired`, switch to `/browser target bridge`, then ask the agent to take a browser screenshot.
