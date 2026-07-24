# zode-hook-demo

A minimal Zode plugin that ships a **hook** — specifically a **sandboxed
JavaScript** `before_tool_use` hook that blocks destructive Bash commands.

```text
zode-hook-demo/
├── plugin.json
└── hooks/
    ├── hooks.json     # maps before_tool_use (Bash) → guard.js
    └── guard.js       # the hook, runs in Zode's QuickJS sandbox
```

## How it works

`hooks/hooks.json` binds the hook to the event and tool:

```json
{
  "hooks": [
    { "event": "before_tool_use", "tool": "Bash", "script": "${ZODE_PLUGIN_ROOT}/hooks/guard.js" }
  ]
}
```

- `${ZODE_PLUGIN_ROOT}` resolves to the plugin's install directory, so the
  `script` path works wherever the plugin is installed.
- Because `script` ends in `.js`, Zode runs it **in-process** in the QuickJS
  sandbox — no shell, no Node, no filesystem/network/process access. (A
  non-`.js` script is instead executed as an external process: event JSON on
  stdin, exit code `0`/`2`/other → proceed/block/warn.)

`hooks/guard.js` registers a synchronous handler with `zode.hook(fn)`. It
receives the event `{ event, tool, input }` and returns:

- `{ ok: true }` / nothing / `true` → allow the tool call
- `{ block: true, reason }` or a non-empty string → block it
- `{ warn: <code> }` → log a warning, then proceed

## Try it

```bash
# Executable hooks (JS included) require --trust.
zode plugin validate ./examples/plugins/zode-hook-demo
zode plugin install ./examples/plugins/zode-hook-demo --trust
# Restart a running Zode so it loads the new snapshot, then ask the agent to run
# e.g. `rm -rf /tmp/whatever` — the hook blocks the Bash call.
zode plugin disable zode-hook-demo   # turn it off
```

## Customize

Edit the `DANGEROUS` patterns in `guard.js`, or change the `event` in
`hooks.json` to hook a different point in the agent loop (e.g.
`after_tool_use`, `on_user_message`, `on_session_start`). A hook may also read
its own per-plugin data directory via `${ZODE_PLUGIN_DATA}` in the `script`
path.
