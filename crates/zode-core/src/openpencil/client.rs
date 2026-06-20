//! JSON-RPC-over-HTTP client for a live OpenPencil instance. `Transport` is a
//! seam so tests inject a fake. NOTE: tools/call carry NO token (localhost
//! trust); the token is only validated via `ping`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::OpError;

/// Abstraction over the HTTP wire so unit tests never open a socket.
#[async_trait]
pub trait Transport: Send + Sync + std::fmt::Debug {
    async fn post_json(&self, url: &str, body: Value) -> Result<Value, OpError>;
}

/// Production transport backed by `reqwest`.
#[derive(Debug)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl Default for ReqwestTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl ReqwestTransport {
    /// Bounded HTTP: a fixed request timeout so a hung instance can't stall a turn.
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self { client }
    }
}

#[async_trait]
impl Transport for ReqwestTransport {
    async fn post_json(&self, url: &str, body: Value) -> Result<Value, OpError> {
        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| OpError::Http(e.to_string()))?;
        resp.json::<Value>()
            .await
            .map_err(|e| OpError::Http(e.to_string()))
    }
}

/// High-level client that wraps a `Transport` and speaks the OpenPencil MCP
/// JSON-RPC dialect.
#[derive(Debug, Clone)]
pub struct OpClient {
    base_url: String,
    transport: Arc<dyn Transport>,
}

impl OpClient {
    pub fn new(base_url: String, transport: Arc<dyn Transport>) -> Self {
        Self {
            base_url,
            transport,
        }
    }

    fn mcp_url(&self) -> String {
        format!("{}/mcp", self.base_url)
    }

    /// Fire a raw JSON-RPC request; returns the full envelope (caller unwraps).
    async fn rpc(&self, method: &str, params: Value) -> Result<Value, OpError> {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        self.transport.post_json(&self.mcp_url(), body).await
    }

    /// JSON-RPC `ping`; returns the `result` object for the caller to validate
    /// (`server` / `mode` / `token`).
    pub async fn ping(&self) -> Result<Value, OpError> {
        let resp = self.rpc("ping", Value::Null).await?;
        resp.get("result")
            .cloned()
            .ok_or_else(|| OpError::Parse("ping: no result".into()))
    }

    /// Invoke a named MCP tool and unwrap the envelope into the tool's JSON output.
    /// tools/call carries NO token — localhost trust.
    pub async fn call(&self, tool: &str, args: Value) -> Result<Value, OpError> {
        let resp = self
            .rpc("tools/call", json!({"name": tool, "arguments": args}))
            .await?;
        unwrap_envelope(resp)
    }

    /// Return the list of tool names exposed by the live instance.
    pub async fn list_tools(&self) -> Result<Vec<String>, OpError> {
        let resp = self.rpc("tools/list", json!({})).await?;
        Ok(resp
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.get("name").and_then(|n| n.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// Unwrap a JSON-RPC tools/call envelope.
///
/// Precedence:
/// 1. `error` key present → `OpError::Rpc`.
/// 2. `result.isError == true` → `OpError::Rpc` with concatenated text.
/// 3. Otherwise → text blocks joined and parsed as JSON (falls back to a plain
///    `Value::String` if not valid JSON).
pub fn unwrap_envelope(resp: Value) -> Result<Value, OpError> {
    // Surface JSON-RPC-level errors first.
    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("rpc error");
        return Err(OpError::Rpc(msg.to_string()));
    }

    let result = resp
        .get("result")
        .ok_or_else(|| OpError::Parse("no result".into()))?;

    // Concatenate text blocks from the content array.
    let text = result
        .get("content")
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    // Tool-level error (isError flag).
    if result
        .get("isError")
        .and_then(|e| e.as_bool())
        .unwrap_or(false)
    {
        return Err(OpError::Rpc(text));
    }

    // Try to parse as JSON; fall back to a plain string.
    Ok(serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug)]
    struct FakeTransport(Value);

    #[async_trait::async_trait]
    impl Transport for FakeTransport {
        async fn post_json(&self, _u: &str, _b: Value) -> Result<Value, OpError> {
            Ok(self.0.clone())
        }
    }

    fn client_with(v: Value) -> OpClient {
        OpClient::new(
            "http://127.0.0.1:1".into(),
            std::sync::Arc::new(FakeTransport(v)),
        )
    }

    #[tokio::test]
    async fn call_unwraps_text_envelope() {
        let r = json!({"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"{\"ok\":true}"}]}});
        assert_eq!(
            client_with(r)
                .call("get_document_info", json!({}))
                .await
                .unwrap(),
            json!({"ok":true})
        );
    }

    #[tokio::test]
    async fn call_surfaces_is_error() {
        let r = json!({"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"boom"}],"isError":true}});
        assert!(
            matches!(client_with(r).call("delete_node", json!({})).await.unwrap_err(), OpError::Rpc(m) if m.contains("boom"))
        );
    }

    #[tokio::test]
    async fn ping_returns_result_object() {
        let r = json!({"jsonrpc":"2.0","id":0,"result":{"server":"openpencil-mcp","mode":"live","token":"t"}});
        let res = client_with(r).ping().await.unwrap();
        assert_eq!(res["server"], "openpencil-mcp");
        assert_eq!(res["token"], "t");
    }
}
