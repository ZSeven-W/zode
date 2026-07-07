# Zode JSON-RPC Fixtures

This directory contains language-independent fixtures generated from
`zode-app-server-protocol`.

The fixtures cover the stable methods implemented by the current zode server
mode. Codex-only product surfaces that zode does not implement are intentionally
absent.

## Generate

From the repository root:

```sh
cargo run -p zode-app-server-protocol --bin export -- sdk/fixtures/jsonrpc
```

## Use

SDK tests can load the files under [`jsonrpc/`](jsonrpc/) to assert that their
request serialization matches the Rust protocol source of truth.
