# Zode Rust SDK

Rust SDK for `zode server` stdio JSON-RPC.

## Install

Inside this repository it is a workspace crate:

```toml
zode-sdk-rust = { path = "sdk/rust" }
```

The library name is `zode_sdk`.

## Usage

`zode` must be on `PATH`, or pass an absolute binary path in `ClientOptions`.

```rust
use serde_json::json;
use zode_sdk::{ClientOptions, ProtocolMethod, ZodeClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ZodeClient::new(ClientOptions::default());
    let mut server = client.spawn_stdio().await?;

    let init = server.initialize("example", "0.1.0").await?;
    println!("server: {}", init.server_info.name);

    let command: serde_json::Value = server
        .request_method(
            ProtocolMethod::CommandExec,
            json!({"command": ["sh", "-c", "printf hi"]}),
        )
        .await?;
    println!("stdout: {}", command["stdout"]);

    server.close().await?;
    Ok(())
}
```

Use `request_method(ProtocolMethod::..., params)` for stable zode methods, or
`request(method, params)` when you intentionally need raw JSON-RPC.
Every supported method's params, result shape, and enum name are documented in
the [SDK method reference](../README.md#method-reference).

## Test

From the repository root:

```sh
cargo test -p zode-sdk-rust
```
