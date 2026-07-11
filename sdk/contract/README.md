# Zode SDK Contract

`basic_run.json` documents the shared request sequence every SDK sends over
stdio against a real `zode server`.

`turn/start` streams model output as notifications (`turn/started`, then
`item/agentMessage/delta` and tool `item/*` frames, ending in exactly one of
`turn/completed` / `turn/failed` / `turn/interrupted`). Without provider
credentials the turn ends in `turn/failed`, which the e2e tests assert.

## Use

Each SDK's e2e test drives this flow against the binary named by the `ZODE_BIN`
environment variable (skipped when unset):

1. Send `initialize` (with `approvalPolicy: "auto"` so `command/exec` is
   allowed without a prompt).
2. Send `thread/start`.
3. Substitute the returned thread id into `turn/start`, collect the streamed
   notifications through the turn's terminal frame.
4. Send `command/exec`.

## Enforcement

The contract is no longer advisory — three mechanisms keep the five SDKs and
the server in lockstep:

- **Fixtures are derived, not hand-listed.** `sdk/fixtures/jsonrpc/*.json` is
  regenerated from the protocol crate's request enum
  (`cargo run -p zode-app-server-protocol --bin export`); a Rust test
  regenerates into a temp dir and fails if the committed fixtures drift
  (extra, missing, or changed files).
- **Every SDK asserts semantic parity.** Each SDK's test suite builds each
  request through its own serialization path and deep-compares the parsed JSON
  against the fixture (key-order independent), and validates that the strict
  JSON-RPC 2.0 envelope (`jsonrpc: "2.0"`) is present.
- **CI runs all five suites strictly.** `scripts/test-sdks.sh --strict` (the
  `sdk` job in `.github/workflows/ci.yml`) builds `zode`, points `ZODE_BIN` at
  it, and runs every language's suite including the live e2e; a missing
  toolchain fails the job instead of silently skipping.

The flow intentionally avoids account, marketplace, remote-control, Realtime,
and background-process APIs because they are not part of the current
zode-backed surface. WebSocket transport is available (`--listen ws://…`) and
exercised by the TypeScript SDK's WebSocket client.
