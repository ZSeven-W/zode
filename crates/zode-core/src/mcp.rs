//! MCP integration. agent-rs provides Lifecycle + registry but no Tool
//! adapter, so each discovered MCP tool is wrapped in a ZodeMcpTool that
//! routes through Lifecycle::call_tool (master §4.6②). Connection happens
//! at assembly (blocking); tools are registered under `mcp__<server>__<tool>`.

use std::path::Path;
use std::sync::Arc;

use agent::error::AgentError;
use agent::mcp::{Lifecycle, McpConfig, McpRegistry, RmcpConnector};
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::ConfigManager;

pub fn prefixed_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{server}__{tool}")
}

/// Load + merge MCP config: global ~/.zode/mcp.json ⊕ project .mcp.json
/// (project servers override same-name). None if neither exists/parses.
pub fn discover_mcp_config(cwd: &Path) -> Option<McpConfig> {
    let mut merged: Option<McpConfig> = None;
    let candidates = [
        ConfigManager::config_dir().ok().map(|d| d.join("mcp.json")),
        Some(cwd.join(".mcp.json")),
    ];
    for path in candidates.into_iter().flatten() {
        let Ok(s) = std::fs::read_to_string(&path) else {
            continue;
        };
        match agent::mcp::parse_json_str(&s) {
            Ok(cfg) => {
                merged = Some(match merged {
                    None => cfg,
                    Some(mut base) => {
                        base.servers.extend(cfg.servers); // project overrides
                        base
                    }
                });
            }
            Err(e) => tracing::warn!("skip mcp config {}: {e}", path.display()),
        }
    }
    merged.filter(|c| !c.servers.is_empty())
}

/// Connect all servers in `config` and return the live Lifecycle. Failures
/// are logged; the lifecycle is returned regardless so /mcp can report state.
pub async fn connect(config: McpConfig) -> Arc<Lifecycle> {
    let registry = McpRegistry::new();
    for (name, server_cfg) in config.servers {
        registry.upsert(name, server_cfg);
    }
    let lifecycle = Arc::new(Lifecycle::new(registry, Arc::new(RmcpConnector::new())));
    for (server, res) in lifecycle.connect_all().await {
        if let Err(e) = res {
            tracing::warn!("mcp server {server} failed to connect: {e}");
        }
    }
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
    #[serial_test::serial]
    fn discover_merges_project_over_global() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path());
        // agent's McpConfig uses {"servers": {name: {transport, ...}}}.
        std::fs::write(
            dir.path().join("mcp.json"),
            r#"{"servers":{"a":{"transport":"stdio","command":"echo"}}}"#,
        )
        .unwrap();
        let proj = tempfile::tempdir().unwrap();
        std::fs::write(
            proj.path().join(".mcp.json"),
            r#"{"servers":{"b":{"transport":"stdio","command":"cat"}}}"#,
        )
        .unwrap();
        let cfg = discover_mcp_config(proj.path()).unwrap();
        std::env::remove_var("ZODE_CONFIG_DIR");
        // Global `a` + project `b` are both present after the merge.
        assert!(
            cfg.servers.contains_key("a"),
            "servers: {:?}",
            cfg.servers.keys().collect::<Vec<_>>()
        );
        assert!(cfg.servers.contains_key("b"));
    }

    #[test]
    #[serial_test::serial]
    fn discover_none_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZODE_CONFIG_DIR", dir.path().join("none"));
        let cfg = discover_mcp_config(&dir.path().join("noproj"));
        std::env::remove_var("ZODE_CONFIG_DIR");
        assert!(cfg.is_none());
    }
}
