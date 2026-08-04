# zode bridge — Privacy Policy

_Last updated: 2026-08-04_

**zode bridge** is a Chrome extension that connects your browser to the
**zode** command-line tool running on your own computer, so its coding agent
can read and control the browser tab you explicitly pair with it.

## What the extension does with data

- The extension communicates **only** with the zode CLI on your own machine, at
  `ws://127.0.0.1` (localhost), over a WebSocket secured by a 6-digit pairing
  code and a long-term token stored locally on your device.
- To carry out the actions you request, the extension reads the content of the
  browser tab you paired (page text, DOM, screenshots, console and network
  output) and sends it to that local zode process. This data does **not** leave
  your device via the extension, and the extension does **not** transmit any
  data to the developer or to any third-party server.
- The extension does **not** read the value of password fields.

## What we collect

**Nothing.** zode bridge does not collect, sell, or transfer your personal
information, browsing data, or page content to us or to any third party. All
data handled by the extension stays on your device (exchanged only with your
local zode CLI).

## Local storage

The extension stores a pairing token and minimal connection state locally in
your browser so it can reconnect to your local zode CLI without re-pairing each
session. This data never leaves your device.

## The zode CLI

The zode CLI is separate software that you install and run yourself. When you
ask its agent to work with a page, zode may send the relevant content to the AI
model **you** configured in zode. That processing is governed by zode's own
documentation and by the terms of whichever model provider you chose — not by
this extension. The extension itself only bridges your browser to your local
zode.

## Contact

Source code and issues: https://github.com/ZSeven-W/zode
