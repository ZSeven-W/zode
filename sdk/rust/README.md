# Zode Rust SDK

Rust SDK for `zode server` stdio JSON-RPC.

## Install

From the tagged Git repository:

```toml
zode-sdk-rust = { git = "https://github.com/ZSeven-W/zode.git", tag = "v0.2.0-beta.1" }
```

Inside this repository it is also a workspace crate:

```toml
zode-sdk-rust = { path = "sdk/rust" }
```

The library name is `zode_sdk`.

Each GitHub Release also includes a standalone Rust SDK source bundle containing
both `zode-sdk-rust` and its `zode-app-server-protocol` dependency.

## Usage

`zode` must be on `PATH`, or pass an absolute binary path in `ClientOptions`.

```rust
use serde_json::json;
use zode_app_server_protocol::ApprovalPolicy;
use zode_sdk::{ClientOptions, ProtocolMethod, ZodeClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ZodeClient::new(ClientOptions::default());
    let server = client.spawn_stdio().await?;

    let init = server
        .initialize("example", "0.1.0", ApprovalPolicy::ReadOnly)
        .await?;
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

## Streaming turns and approvals

A turn streams as notifications; register handlers **before** starting the turn.
Use `ApprovalPolicy::Auto` (or `Prompt` with an approval handler) so
side-effecting work actually runs — the default `ReadOnly` denies it.

```rust
use serde_json::{json, Value};
use zode_app_server_protocol::server_requests::{ApprovalDecision, ApprovalRequestParams};
use zode_app_server_protocol::types::ApprovalPolicy;
use zode_sdk::{ClientOptions, ProtocolMethod, ZodeClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = ZodeClient::new(ClientOptions::default()).spawn_stdio().await?;

    server.on_notification(|method: String, params: Value| {
        if method == "item/agentMessage/delta" {
            print!("{}", params["delta"].as_str().unwrap_or_default());
        }
    });
    server.on_approval_request(|params: ApprovalRequestParams| {
        eprintln!("approve {:?}: {}", params.kind, params.summary);
        ApprovalDecision::Allow
    });

    server.initialize("example", "0.1.0", ApprovalPolicy::Auto).await?;
    let thread: Value = server
        .request_method(ProtocolMethod::ThreadStart, json!({}))
        .await?;
    let thread_id = thread["thread"]["id"].as_str().unwrap();
    server
        .request_method::<_, Value>(
            ProtocolMethod::TurnStart,
            json!({"threadId": thread_id, "input": "list the repo files"}),
        )
        .await?;
    // turn/completed arrives on the notification handler.
    server.close().await?;
    Ok(())
}
```

`on_notification` receives `(method, params)`; `on_approval_request` returns an
`ApprovalDecision` (`Allow` / `AllowAlways` / `Deny`). An unregistered approval
handler denies.

## Version

This crate is versioned with the zode workspace (currently `0.2.0-beta.1`).

## Test

From the repository root:

```sh
cargo test -p zode-sdk-rust
```
