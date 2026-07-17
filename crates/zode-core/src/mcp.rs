//! MCP integration. agent-rs provides Lifecycle + registry but no Tool
//! adapter, so each discovered MCP tool is wrapped in a ZodeMcpTool that
//! routes through Lifecycle::call_tool (master §4.6②). Connection happens at
//! assembly, bounded by a per-server timeout so a hung server can't block the
//! launch; tools are registered under `mcp__<server>__<tool>`.

use std::sync::Arc;

use agent::error::AgentError;
use agent::mcp::{Lifecycle, McpConfig, McpRegistry, RmcpConnector};
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Merge a caller-provided MCP config over discovered local configuration.
/// Explicit session transports win same-name collisions without ever being
/// persisted (ACP may include short-lived credentials in headers/env).
pub fn merge_config(base: Option<McpConfig>, overlay: Option<McpConfig>) -> Option<McpConfig> {
    match (base, overlay) {
        (None, None) => None,
        (Some(config), None) | (None, Some(config)) => Some(config),
        (Some(mut base), Some(overlay)) => {
            base.servers.extend(overlay.servers);
            Some(base)
        }
    }
}

/// Parse already-normalized server definitions without touching disk.
pub fn config_from_servers(servers: serde_json::Map<String, Value>) -> Result<McpConfig, String> {
    agent::mcp::parse_json_str(&json!({"servers": servers}).to_string())
        .map_err(|error| error.to_string())
}

pub fn prefixed_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{server}__{tool}")
}

mod discovery;

pub use discovery::discover_mcp_config;

/// Per-server connect timeout — a hung/unreachable server must never block
/// the whole launch.
const CONNECT_TIMEOUT_SECS: u64 = 10;

/// Connect all servers in `config` (concurrently, each bounded by a timeout)
/// and return the live Lifecycle. Failures/timeouts are logged and skipped;
/// the lifecycle is returned regardless so /mcp can report state and the
/// session starts without the unreachable server.
pub async fn connect(config: McpConfig) -> Arc<Lifecycle> {
    let registry = McpRegistry::new();
    // Register every server (so `/mcp` can list disabled ones), but only
    // CONNECT the enabled ones — foreign/cross-agent servers default to
    // disabled, so a machine full of them doesn't auto-connect dozens.
    let mut names: Vec<String> = Vec::new();
    for (name, server_cfg) in config.servers {
        if server_cfg.enabled() {
            names.push(name.clone());
        }
        registry.upsert(name, server_cfg);
    }
    let lifecycle = Arc::new(Lifecycle::new(registry, Arc::new(RmcpConnector::new())));

    let timeout = std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS);
    let connects = names.into_iter().map(|name| {
        let lc = lifecycle.clone();
        async move {
            // A failed connect is expected for cross-agent-discovered servers
            // (not installed / need another env). The `/mcp` dialog shows live
            // connection state, so log at debug to avoid spamming every launch.
            match tokio::time::timeout(timeout, lc.connect(&name)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::debug!("mcp server {name} failed to connect: {e}"),
                Err(_) => {
                    tracing::debug!(
                        "mcp server {name} connect timed out after {CONNECT_TIMEOUT_SECS}s"
                    )
                }
            }
        }
    });
    futures::future::join_all(connects).await;
    lifecycle
}

/// Build a ZodeMcpTool for every tool discovered on a connected server.
pub fn mcp_tools(lifecycle: &Arc<Lifecycle>) -> Vec<Arc<dyn Tool>> {
    let mut out: Vec<Arc<dyn Tool>> = Vec::new();
    for server in lifecycle.registry.snapshot() {
        for tool in server.state.tool_names() {
            out.push(Arc::new(ZodeMcpTool::new(
                lifecycle.clone(),
                server.name.clone(),
                tool.clone(),
            )));
        }
    }
    out
}

/// A single MCP tool surfaced as an agent Tool. The Connection trait does
/// not expose a per-tool JSON schema, so we advertise a permissive object
/// schema (agent-rs gap) — the server validates the real shape.
#[derive(Debug)]
pub struct ZodeMcpTool {
    lifecycle: Arc<Lifecycle>,
    server: String,
    tool: String,
    display_name: String,
}

impl ZodeMcpTool {
    pub fn new(lifecycle: Arc<Lifecycle>, server: String, tool: String) -> Self {
        let display_name = prefixed_tool_name(&server, &tool);
        Self {
            lifecycle,
            server,
            tool,
            display_name,
        }
    }
}

#[async_trait]
impl Tool for ZodeMcpTool {
    fn name(&self) -> &str {
        &self.display_name
    }
    fn description(&self) -> &str {
        "MCP server tool."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": true})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Unknown // gated by default (unknown side effects)
    }
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        self.lifecycle
            .call_tool(&self.server, &self.tool, input)
            .await
            .map_err(|e| AgentError::other(format!("mcp {}: {e}", self.server)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixed_name() {
        assert_eq!(prefixed_tool_name("deepwiki", "ask"), "mcp__deepwiki__ask");
    }

    #[test]
    fn explicit_config_overrides_a_discovered_server_with_the_same_name() {
        let base = config_from_servers(serde_json::Map::from_iter([(
            "shared".into(),
            json!({"transport":"stdio", "command":"old", "args":[], "env":{}}),
        )]))
        .unwrap();
        let overlay = config_from_servers(serde_json::Map::from_iter([(
            "shared".into(),
            json!({"transport":"stdio", "command":"new", "args":[], "env":{}}),
        )]))
        .unwrap();
        let merged = merge_config(Some(base), Some(overlay)).unwrap();
        assert_eq!(merged.servers.len(), 1);
        assert!(merged.servers.contains_key("shared"));
    }
}
