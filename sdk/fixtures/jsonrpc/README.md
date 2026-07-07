# JSON-RPC Fixture Files

These files are generated from `zode-app-server-protocol`.

Do not edit fixture JSON by hand. Regenerate from the repository root:

```sh
cargo run -p zode-app-server-protocol --bin export -- sdk/fixtures/jsonrpc
```

Useful fixtures:

- `protocol.schema.json` lists the current stable method names.
- `initialize.request.json` and `initialize.response.json` show handshake shape.
- `thread-start.request.json` shows thread creation params.
- `fs-read-file.request.json` shows base filesystem params.
- `command-exec.request.json` shows one-shot command execution params.
