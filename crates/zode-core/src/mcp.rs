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

/// Default per-call MCP tool timeout when config leaves it unset. A remote tool
/// must not hold a scheduler turn forever; timing out locally does not prove the
/// server stopped, so the call is fenced as unresolved. Overridable (including
/// disabling) via `mcpToolTimeoutSecs` — see [`ConfigManager`]-loaded config.
pub const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 60;

/// Connect all servers in `config` (concurrently, each bounded by a timeout)
/// and return the live Lifecycle. Failures/timeouts are logged and skipped;
/// the lifecycle is returned regardless so /mcp can report state and the
/// session starts without the unreachable server.
pub async fn connect(config: McpConfig) -> Arc<Lifecycle> {
    let registry = McpRegistry::new();
    // Register every server so `/mcp` can list disabled definitions, but only
    // connect the ones the user or installed plugin enabled.
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
            // The `/mcp` dialog shows live connection state, so keep connection
            // failures at debug level instead of spamming every launch.
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
///
/// `tool_timeout` bounds each call locally: `None` disables the local timeout
/// (relying on the turn's own cancellation), `Some(d)` fences a stuck server.
pub fn mcp_tools(lifecycle: &Arc<Lifecycle>, tool_timeout: Option<Duration>) -> Vec<Arc<dyn Tool>> {
    let mut out: Vec<Arc<dyn Tool>> = Vec::new();
    for server in lifecycle.registry.snapshot() {
        for tool in server.state.tool_names() {
            out.push(Arc::new(ZodeMcpTool::new(
                lifecycle.clone(),
                server.name.clone(),
                tool.clone(),
                tool_timeout,
            )));
        }
    }
    out
}

/// A single MCP tool surfaced as an agent Tool. The tool's input schema is
/// the SERVER-DECLARED parameter contract when available — a permissive
/// catch-all object made models guess argument names (e.g. passing
/// `instruction` where the server declares `text`) and fail the first call
/// every time.
#[derive(Debug)]
pub struct ZodeMcpTool {
    lifecycle: Arc<Lifecycle>,
    server: String,
    tool: String,
    display_name: String,
    /// The server-declared input schema; a permissive object fallback when
    /// the server omitted one.
    schema: serde_json::Value,
    /// Per-call local timeout; `None` disables it.
    timeout: Option<Duration>,
}

impl ZodeMcpTool {
    pub fn new(
        lifecycle: Arc<Lifecycle>,
        server: String,
        tool: String,
        timeout: Option<Duration>,
    ) -> Self {
        let display_name = prefixed_tool_name(&server, &tool);
        let schema = lifecycle
            .tool_schema(&server, &tool)
            .unwrap_or_else(|| serde_json::json!({"type": "object", "additionalProperties": true}));
        Self {
            lifecycle,
            server,
            tool,
            display_name,
            schema,
            timeout,
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
    timeout: Option<Duration>,
    request: impl std::future::Future<Output = Result<Value, LifecycleError>>,
) -> Result<Value, AgentError> {
    if ctx.abort.is_aborted() {
        return Err(mcp_aborted(ctx));
    }
    ctx.abort.pulse();
    let mut unresolved = UnresolvedMcpCall::new(ctx.abort.clone());
    // A disabled (`None`) timeout waits forever, so far in the future the arm
    // never fires; the turn's own cancellation is then the only bound.
    let timeout_sleep = async {
        match timeout {
            Some(duration) => tokio::time::sleep(duration).await,
            None => std::future::pending::<()>().await,
        }
    };
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
        _ = timeout_sleep => {
            Err(AgentError::other(format!(
                "mcp {server}: tool call timed out after {}s",
                timeout.map(|d| d.as_secs()).unwrap_or_default()
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
        self.schema.clone()
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Unknown // gated by default (unknown side effects)
    }
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        await_mcp_response(
            ctx,
            &self.server,
            self.timeout,
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
        let result = await_mcp_response(&transport, "test", Some(Duration::from_secs(1)), async {
            Err(LifecycleError::Connector("connection lost".into()))
        })
        .await;
        assert!(result.is_err());
        assert!(transport.abort.activity().unresolved_external_work());

        let timed_out = ctx();
        let result = await_mcp_response(
            &timed_out,
            "test",
            Some(Duration::from_millis(1)),
            std::future::pending(),
        )
        .await;
        assert!(result.is_err());
        assert!(timed_out.abort.activity().unresolved_external_work());
    }

    #[tokio::test]
    async fn mcp_explicit_terminal_response_does_not_latch_unresolved_work() {
        let success = ctx();
        let value = await_mcp_response(&success, "test", Some(Duration::from_secs(1)), async {
            Ok(json!({"ok": true}))
        })
        .await
        .unwrap();
        assert_eq!(value, json!({"ok": true}));
        assert!(!success.abort.activity().unresolved_external_work());

        let tool_error = ctx();
        let result = await_mcp_response(&tool_error, "test", Some(Duration::from_secs(1)), async {
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

        let result =
            await_mcp_response(&context, "test", Some(Duration::from_secs(1)), request).await;
        assert!(matches!(result, Err(AgentError::Aborted(_))));
        assert!(context.abort.activity().unresolved_external_work());
    }
}
