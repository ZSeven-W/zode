//! Agent-facing browser tools: `browser_read` (screenshot / DOM snapshot /
//! console / network / tabs / downloads), `browser_act` (navigate / click / type / key /
//! scroll), `browser_eval` (arbitrary JS), and `browser_tabs` (open / close /
//! select). All four share a `BrowserSession` lease per call, following the
//! `op_read`/`op_write` pattern in `openpencil/tools.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use agent::abort::AbortController;
use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::backend::{BrowserError, BrowserTarget, ClickTarget};
use super::session::{BackendLease, BrowserSession};

/// Shared deps for all four `browser_*` tools.
#[derive(Debug, Clone)]
pub struct BrowserToolDeps {
    pub session: Arc<BrowserSession>,
    pub shots_dir: PathBuf,
    /// When set, every lease taken by these tools targets this backend
    /// instead of the session-wide `/browser target` selection. Extension
    /// task engines pin this to `Bridge` so side-panel turns always act on
    /// the page beside the panel, never a managed Chrome.
    pub target_override: Option<BrowserTarget>,
}

impl BrowserToolDeps {
    /// The target these tools actually drive: the override when pinned,
    /// otherwise the session-wide selection.
    pub fn effective_target(&self) -> BrowserTarget {
        self.target_override
            .clone()
            .unwrap_or_else(|| self.session.target())
    }

    pub async fn lease(&self) -> Result<BackendLease<'_>, BrowserError> {
        self.session.lease_as(self.effective_target()).await
    }
}

fn to_agent_err(e: BrowserError) -> AgentError {
    AgentError::other(e.to_string())
}

/// A browser request can outlive its local future (the bridge, CDP handler,
/// or a freshly launched browser may still be working). Keep the turn fenced
/// until the adapter receives a response that proves a terminal state.
struct UnresolvedBrowserCall {
    abort: AbortController,
    armed: bool,
}

impl UnresolvedBrowserCall {
    fn new(abort: AbortController) -> Self {
        Self { abort, armed: true }
    }

    fn resolve(&mut self) {
        self.armed = false;
    }
}

impl Drop for UnresolvedBrowserCall {
    fn drop(&mut self) {
        if self.armed {
            self.abort.mark_unresolved_external_work();
        }
    }
}

fn aborted(ctx: &ToolUseContext) -> AgentError {
    AgentError::Aborted(
        ctx.abort
            .reason()
            .unwrap_or_else(|| "browser operation aborted".into()),
    )
}

fn browser_error_is_terminal(error: &BrowserError) -> bool {
    // NotFound is a definite local/pre-dispatch outcome. Launch, protocol,
    // timeout, and dead-channel errors cannot prove that an external browser
    // did not start or partially apply the request.
    matches!(error, BrowserError::NotFound(_))
}

/// Await one browser boundary while observing the root turn abort. This is
/// also used for read operations because lazy backend creation and bridge/CDP
/// reads can start work whose completion is unknown after a dropped future.
pub(super) async fn await_browser_response<T>(
    ctx: &ToolUseContext,
    request: impl std::future::Future<Output = Result<T, BrowserError>>,
) -> Result<T, AgentError> {
    if ctx.abort.is_aborted() {
        return Err(aborted(ctx));
    }
    ctx.abort.pulse();
    let mut unresolved = UnresolvedBrowserCall::new(ctx.abort.clone());
    tokio::pin!(request);
    tokio::select! {
        biased;
        _ = ctx.abort.cancelled() => Err(aborted(ctx)),
        result = &mut request => {
            ctx.abort.pulse();
            match result {
                Ok(value) => {
                    unresolved.resolve();
                    Ok(value)
                }
                Err(error) => {
                    if browser_error_is_terminal(&error) {
                        unresolved.resolve();
                    }
                    Err(to_agent_err(error))
                }
            }
        }
    }
}

/// Resolve a click/type target from `selector` / `ref` / `x`+`y` input
/// fields, in that priority order.
fn click_target(input: &Value) -> Result<ClickTarget, AgentError> {
    if let Some(sel) = input.get("selector").and_then(|v| v.as_str()) {
        return Ok(ClickTarget::Selector(sel.to_string()));
    }
    if let Some(r) = input.get("ref").and_then(|v| v.as_u64()) {
        return Ok(ClickTarget::Ref(r as u32));
    }
    if let (Some(x), Some(y)) = (
        input.get("x").and_then(|v| v.as_f64()),
        input.get("y").and_then(|v| v.as_f64()),
    ) {
        return Ok(ClickTarget::Coords { x, y });
    }
    Err(AgentError::other(
        "target required: selector, ref, or x/y coordinates",
    ))
}

/// Reads the controlled browser: screenshot, DOM snapshot, console/network
/// logs, tab list. Read-only — never requires approval.
#[derive(Debug)]
pub struct BrowserReadTool {
    deps: BrowserToolDeps,
}

impl BrowserReadTool {
    pub fn new(deps: BrowserToolDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for BrowserReadTool {
    fn name(&self) -> &str {
        "browser_read"
    }

    fn description(&self) -> &str {
        "Read the controlled browser: screenshot, DOM snapshot (with clickable refs), console \
         logs, network log, tab list, recent downloads. No approval needed."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["screenshot", "snapshot", "console", "network", "tabs", "downloads"],
                    "description": "Which read to perform"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "description": "Max entries for console/network/downloads (default 100)"
                }
            }
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let action = input.get("action").and_then(|a| a.as_str()).unwrap_or("");
        if !matches!(
            action,
            "screenshot" | "snapshot" | "console" | "network" | "tabs" | "downloads"
        ) {
            return Err(AgentError::other(format!("unknown action: {action:?}")));
        }
        let lease = await_browser_response(ctx, self.deps.lease()).await?;
        let backend = lease.backend();
        match action {
            "screenshot" => {
                let shot = await_browser_response(ctx, backend.screenshot()).await?;
                drop(lease); // release the browser before disk I/O
                crate::desktop::screenshot::save_screenshot_artifact(
                    &self.deps.shots_dir,
                    &shot.bytes,
                )
            }
            "snapshot" => Ok(json!({
                "outline": await_browser_response(ctx, backend.snapshot()).await?
            })),
            "console" => {
                let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                Ok(json!({
                    "entries": await_browser_response(ctx, backend.console_logs(limit)).await?
                }))
            }
            "network" => {
                let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                Ok(json!({
                    "entries": await_browser_response(ctx, backend.network_log(limit)).await?
                }))
            }
            "downloads" => {
                let limit = input
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100)
                    .clamp(1, 100) as usize;
                Ok(json!({
                    "entries": await_browser_response(ctx, backend.downloads(limit)).await?
                }))
            }
            "tabs" => Ok(json!({
                "tabs": await_browser_response(ctx, backend.tabs()).await?
            })),
            _ => unreachable!("validated browser_read action"),
        }
    }
}

/// Acts in the controlled browser: navigate, click, type, press keys, scroll.
/// Mutating — gated by the standard `ApprovalGate`.
#[derive(Debug)]
pub struct BrowserActTool {
    deps: BrowserToolDeps,
}

impl BrowserActTool {
    pub fn new(deps: BrowserToolDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for BrowserActTool {
    fn name(&self) -> &str {
        "browser_act"
    }

    fn description(&self) -> &str {
        "Act in the controlled browser: navigate, click, type, press keys, scroll. Click \
         targets: CSS selector, snapshot ref, or x/y. navigate returns {url, ok, class, \
         loaded, message}; class is one of ok, dns_failure, connection_refused, \
         connection_failed, offline, tls_error, proxy_error, http_error (with http_status), \
         timeout, aborted, blocked, invalid_url, unknown. A timeout with loaded=true means \
         the page committed and is still loading — retry a read rather than the navigation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["navigate", "click", "type", "key", "scroll"]
                },
                "url": { "type": "string", "description": "URL for navigate" },
                "selector": { "type": "string", "description": "CSS selector target" },
                "ref": { "type": "integer", "description": "Snapshot ref target" },
                "x": { "type": "number", "description": "X coordinate target" },
                "y": { "type": "number", "description": "Y coordinate target" },
                "text": { "type": "string", "description": "Text to type" },
                "key": { "type": "string", "description": "Key to press (e.g. Enter, Escape, Tab)" },
                "dx": { "type": "number", "description": "Horizontal scroll delta (default 0)" },
                "dy": { "type": "number", "description": "Vertical scroll delta (default 600)" }
            }
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let action = input.get("action").and_then(|a| a.as_str()).unwrap_or("");
        if !matches!(action, "navigate" | "click" | "type" | "key" | "scroll") {
            return Err(AgentError::other(format!("unknown action: {action:?}")));
        }
        let lease = await_browser_response(ctx, self.deps.lease()).await?;
        let backend = lease.backend();
        match action {
            "navigate" => {
                let url = input
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("navigate: 'url' required"))?;
                let outcome = await_browser_response(ctx, backend.navigate(url)).await?;
                Ok(outcome.to_json())
            }
            "click" => {
                let target = click_target(&input)?;
                await_browser_response(ctx, backend.click(&target)).await?;
                Ok(json!({ "ok": true }))
            }
            "type" => {
                let target = click_target(&input)?;
                let text = input
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("type: 'text' required"))?;
                await_browser_response(ctx, backend.type_text(&target, text)).await?;
                Ok(json!({ "ok": true }))
            }
            "key" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("key: 'key' required"))?;
                await_browser_response(ctx, backend.press_key(key)).await?;
                Ok(json!({ "ok": true }))
            }
            "scroll" => {
                let dx = input.get("dx").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let dy = input.get("dy").and_then(|v| v.as_f64()).unwrap_or(600.0);
                await_browser_response(ctx, backend.scroll(dx, dy)).await?;
                Ok(json!({ "ok": true }))
            }
            _ => unreachable!("validated browser_act action"),
        }
    }
}

/// Evaluates arbitrary JavaScript in the current page. Mutating — the page
/// may have side effects — so it is gated by the standard `ApprovalGate`.
#[derive(Debug)]
pub struct BrowserEvalTool {
    deps: BrowserToolDeps,
}

impl BrowserEvalTool {
    pub fn new(deps: BrowserToolDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for BrowserEvalTool {
    fn name(&self) -> &str {
        "browser_eval"
    }

    fn description(&self) -> &str {
        "Evaluate arbitrary JavaScript in the current page and return the JSON result. \
         Requires approval."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["expression"],
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "JavaScript expression to evaluate"
                }
            }
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let expression = input
            .get("expression")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::other("eval: 'expression' required"))?;
        let lease = await_browser_response(ctx, self.deps.lease()).await?;
        let backend = lease.backend();
        let value = await_browser_response(ctx, backend.evaluate(expression)).await?;
        Ok(json!({ "value": value }))
    }
}

/// Manages browser tabs: open, close, or select. Mutating — gated by the
/// standard `ApprovalGate`.
#[derive(Debug)]
pub struct BrowserTabsTool {
    deps: BrowserToolDeps,
}

impl BrowserTabsTool {
    pub fn new(deps: BrowserToolDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for BrowserTabsTool {
    fn name(&self) -> &str {
        "browser_tabs"
    }

    fn description(&self) -> &str {
        "Manage browser tabs: open, close, or select."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["new", "close", "select"]
                },
                "url": { "type": "string", "description": "URL to open in a new tab (optional)" },
                "id": { "type": "string", "description": "Tab id (required for close/select)" }
            }
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let action = input.get("action").and_then(|a| a.as_str()).unwrap_or("");
        if !matches!(action, "new" | "close" | "select") {
            return Err(AgentError::other(format!("unknown action: {action:?}")));
        }
        let lease = await_browser_response(ctx, self.deps.lease()).await?;
        let backend = lease.backend();
        match action {
            "new" => {
                let url = input.get("url").and_then(|v| v.as_str());
                let tab = await_browser_response(ctx, backend.tab_new(url)).await?;
                Ok(json!({ "tab": tab }))
            }
            "close" => {
                let id = input
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("close: 'id' required"))?;
                await_browser_response(ctx, backend.tab_close(id)).await?;
                Ok(json!({ "ok": true }))
            }
            "select" => {
                let id = input
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("select: 'id' required"))?;
                await_browser_response(ctx, backend.tab_select(id)).await?;
                Ok(json!({ "ok": true }))
            }
            _ => unreachable!("validated browser_tabs action"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::tool::{SafetyClass, Tool, ToolUseContext};
    use serde_json::json;

    fn deps() -> (BrowserToolDeps, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let session = crate::browser::BrowserSession::new(
            crate::config::BrowserConfig::default(),
            crate::browser::backend::mock::mock_factory(),
        );
        (
            BrowserToolDeps {
                session,
                shots_dir: dir.path().to_path_buf(),
                target_override: None,
            },
            dir,
        )
    }

    fn ctx() -> ToolUseContext {
        ToolUseContext::new(std::env::temp_dir())
    }

    #[tokio::test]
    async fn target_override_bridge_beats_managed_session_default() {
        let (mut d, _g) = deps();
        d.target_override = Some(crate::browser::BrowserTarget::Bridge);
        // Session default is managed (mock factory would succeed); the
        // override must force the unpaired bridge path and fail with the
        // pairing hint instead of touching the managed factory.
        let err = BrowserReadTool::new(d)
            .call(&ctx(), json!({"action": "tabs"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("pair"));
    }

    #[test]
    fn safety_classes() {
        let (d, _g) = deps();
        assert_eq!(
            BrowserReadTool::new(d.clone()).safety_class(),
            SafetyClass::ReadOnly
        );
        assert_eq!(
            BrowserActTool::new(d.clone()).safety_class(),
            SafetyClass::Mutating
        );
        assert_eq!(
            BrowserEvalTool::new(d.clone()).safety_class(),
            SafetyClass::Mutating
        );
        assert_eq!(
            BrowserTabsTool::new(d).safety_class(),
            SafetyClass::Mutating
        );
    }

    #[tokio::test]
    async fn screenshot_emits_sentinel_and_saves_file() {
        let (d, _g) = deps();
        let shots = d.shots_dir.clone();
        let out = BrowserReadTool::new(d)
            .call(&ctx(), json!({"action": "screenshot"}))
            .await
            .unwrap();
        let obj = out.as_object().unwrap();
        assert!(obj.contains_key("__agent_content_blocks__"));
        assert!(obj
            .keys()
            .all(|k| k == "__agent_content_blocks__" || k == "text"));
        let text = obj["text"].as_str().unwrap();
        assert!(text.contains("screenshot saved: "));
        assert_eq!(std::fs::read_dir(&shots).unwrap().count(), 1);
        let blocks = obj["__agent_content_blocks__"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "image");
    }

    #[tokio::test]
    async fn read_actions_dispatch() {
        let (d, _g) = deps();
        let t = BrowserReadTool::new(d);
        let s = t.call(&ctx(), json!({"action": "snapshot"})).await.unwrap();
        assert!(s["outline"].as_str().unwrap().contains("[1]"));
        let c = t.call(&ctx(), json!({"action": "console"})).await.unwrap();
        assert_eq!(c["entries"][0]["text"], "hi");
        let tabs = t.call(&ctx(), json!({"action": "tabs"})).await.unwrap();
        assert_eq!(tabs["tabs"][0]["id"], "t1");
        let downloads = t
            .call(&ctx(), json!({"action": "downloads", "limit": 500}))
            .await
            .unwrap();
        assert_eq!(
            downloads["entries"][0]["url"],
            "https://example.test/file-100.txt"
        );
        assert_eq!(downloads["entries"][0]["status"], "complete");
        let downloads = t
            .call(&ctx(), json!({"action": "downloads", "limit": 0}))
            .await
            .unwrap();
        assert_eq!(
            downloads["entries"][0]["url"],
            "https://example.test/file-1.txt"
        );
        let err = t.call(&ctx(), json!({"action": "fly"})).await.unwrap_err();
        assert!(err.to_string().contains("unknown action"));
    }

    #[tokio::test]
    async fn act_navigate_and_click_by_ref() {
        let (d, _g) = deps();
        let t = BrowserActTool::new(d);
        let nav = t
            .call(
                &ctx(),
                json!({"action": "navigate", "url": "https://example.test"}),
            )
            .await
            .unwrap();
        assert_eq!(nav["url"], "https://example.test");
        assert_eq!(nav["ok"], true);
        assert_eq!(nav["class"], "ok");
        assert_eq!(nav["loaded"], true);
        t.call(&ctx(), json!({"action": "click", "ref": 1}))
            .await
            .unwrap();
        t.call(
            &ctx(),
            json!({"action": "type", "selector": "#q", "text": "hi"}),
        )
        .await
        .unwrap();
        let err = t
            .call(&ctx(), json!({"action": "click"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("selector, ref, or x/y"));
    }

    #[tokio::test]
    async fn eval_returns_value() {
        let (d, _g) = deps();
        let out = BrowserEvalTool::new(d)
            .call(&ctx(), json!({"expression": "1+1"}))
            .await
            .unwrap();
        assert_eq!(out["value"], json!(2));
    }

    #[tokio::test]
    async fn uncertain_browser_error_latches_unresolved_work() {
        for error in [
            BrowserError::Protocol("response lost".into()),
            BrowserError::Timeout("request".into()),
        ] {
            let context = ctx();
            let result: Result<(), AgentError> =
                await_browser_response(&context, async { Err(error) }).await;
            assert!(result.is_err());
            assert!(context.abort.activity().unresolved_external_work());
        }
    }

    #[tokio::test]
    async fn explicit_browser_response_does_not_latch_unresolved_work() {
        let context = ctx();
        assert_eq!(
            await_browser_response(&context, async { Ok::<_, BrowserError>(42) })
                .await
                .unwrap(),
            42
        );
        assert!(!context.abort.activity().unresolved_external_work());

        let terminal = ctx();
        let _: Result<(), AgentError> = await_browser_response(&terminal, async {
            Err(BrowserError::NotFound("no target".into()))
        })
        .await;
        assert!(!terminal.abort.activity().unresolved_external_work());
    }

    #[tokio::test]
    async fn abort_after_browser_dispatch_latches_unresolved_work() {
        let context = ctx();
        let abort = context.abort.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = started_rx.await;
            abort.abort_with_reason("test stop");
        });
        let request = async move {
            let _ = started_tx.send(());
            std::future::pending::<Result<(), BrowserError>>().await
        };

        let result = await_browser_response(&context, request).await;
        assert!(matches!(result, Err(AgentError::Aborted(_))));
        assert!(context.abort.activity().unresolved_external_work());
    }
}
