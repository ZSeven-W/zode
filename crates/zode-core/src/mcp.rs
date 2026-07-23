//! MCP integration. agent-rs provides Lifecycle + registry but no Tool
//! adapter, so each discovered MCP tool is wrapped in a ZodeMcpTool that
//! routes through Lifecycle::call_tool (master §4.6②). Connection happens at
//! assembly, bounded by a per-server timeout so a hung server can't block the
//! launch; tools are registered under `mcp__<server>__<tool>`.

use std::sync::Arc;
use std::time::Duration;

use agent::abort::AbortController;
use agent::error::AgentError;
use agent::mcp::{Lifecycle, LifecycleError, McpConfig, McpRegistry, RmcpConnector};
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

/// A remote tool must not hold a scheduler turn forever. Timing out locally
/// does not prove the server stopped, so the call is fenced as unresolved.
const TOOL_TIMEOUT_SECS: u64 = 60;

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

struct UnresolvedMcpCall {
    abort: AbortController,
    armed: bool,
}

impl UnresolvedMcpCall {
    fn new(abort: AbortController) -> Self {
        Self { abort, armed: true }
    }

    fn resolve(&mut self) {
        self.armed = false;
    }
}

impl Drop for UnresolvedMcpCall {
    fn drop(&mut self) {
        if self.armed {
            self.abort.mark_unresolved_external_work();
        }
    }
}

fn mcp_aborted(ctx: &ToolUseContext) -> AgentError {
    AgentError::Aborted(
        ctx.abort
            .reason()
            .unwrap_or_else(|| "MCP tool call aborted".into()),
    )
}

async fn await_mcp_response(
    ctx: &ToolUseContext,
    server: &str,
    timeout: Duration,
    request: impl std::future::Future<Output = Result<Value, LifecycleError>>,
) -> Result<Value, AgentError> {
    if ctx.abort.is_aborted() {
        return Err(mcp_aborted(ctx));
    }
    ctx.abort.pulse();
    let mut unresolved = UnresolvedMcpCall::new(ctx.abort.clone());
    tokio::pin!(request);
    tokio::select! {
        biased;
        _ = ctx.abort.cancelled() => Err(mcp_aborted(ctx)),
        result = &mut request => {
            ctx.abort.pulse();
            match result {
                Ok(value) => {
                    unresolved.resolve();
                    Ok(value)
                }
                Err(error) => {
                    // Unknown/disabled servers are pre-dispatch outcomes, and
                    // Tool is a server-declared terminal response. Connector
                    // errors include transport/protocol loss and stay armed.
                    if !matches!(error, LifecycleError::Connector(_)) {
                        unresolved.resolve();
                    }
                    Err(AgentError::other(format!("mcp {server}: {error}")))
                }
            }
        }
        _ = tokio::time::sleep(timeout) => {
            Err(AgentError::other(format!(
                "mcp {server}: tool call timed out after {}s",
                timeout.as_secs()
            )))
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
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        await_mcp_response(
            ctx,
            &self.server,
            Duration::from_secs(TOOL_TIMEOUT_SECS),
            self.lifecycle.call_tool(&self.server, &self.tool, input),
        )
        .await
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

    fn ctx() -> ToolUseContext {
        ToolUseContext::new(std::env::temp_dir())
    }

    #[tokio::test]
    async fn mcp_transport_error_and_timeout_latch_unresolved_work() {
        let transport = ctx();
        let result = await_mcp_response(&transport, "test", Duration::from_secs(1), async {
            Err(LifecycleError::Connector("connection lost".into()))
        })
        .await;
        assert!(result.is_err());
        assert!(transport.abort.activity().unresolved_external_work());

        let timed_out = ctx();
        let result = await_mcp_response(
            &timed_out,
            "test",
            Duration::from_millis(1),
            std::future::pending(),
        )
        .await;
        assert!(result.is_err());
        assert!(timed_out.abort.activity().unresolved_external_work());
    }

    #[tokio::test]
    async fn mcp_explicit_terminal_response_does_not_latch_unresolved_work() {
        let success = ctx();
        let value = await_mcp_response(&success, "test", Duration::from_secs(1), async {
            Ok(json!({"ok": true}))
        })
        .await
        .unwrap();
        assert_eq!(value, json!({"ok": true}));
        assert!(!success.abort.activity().unresolved_external_work());

        let tool_error = ctx();
        let result = await_mcp_response(&tool_error, "test", Duration::from_secs(1), async {
            Err(LifecycleError::Tool("rejected".into()))
        })
        .await;
        assert!(result.is_err());
        assert!(!tool_error.abort.activity().unresolved_external_work());
    }

    #[tokio::test]
    async fn abort_after_mcp_dispatch_latches_unresolved_work() {
        let context = ctx();
        let abort = context.abort.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = started_rx.await;
            abort.abort_with_reason("test stop");
        });
        let request = async move {
            let _ = started_tx.send(());
            std::future::pending::<Result<Value, LifecycleError>>().await
        };

        let result = await_mcp_response(&context, "test", Duration::from_secs(1), request).await;
        assert!(matches!(result, Err(AgentError::Aborted(_))));
        assert!(context.abort.activity().unresolved_external_work());
    }
}
