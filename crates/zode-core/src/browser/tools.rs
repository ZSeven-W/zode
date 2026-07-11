//! Agent-facing browser tools: `browser_read` (screenshot / DOM snapshot /
//! console / network / tabs / downloads), `browser_act` (navigate / click / type / key /
//! scroll), `browser_eval` (arbitrary JS), and `browser_tabs` (open / close /
//! select). All four share a `BrowserSession` lease per call, following the
//! `op_read`/`op_write` pattern in `openpencil/tools.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::backend::{BrowserError, ClickTarget};
use super::session::BrowserSession;

/// Shared deps for all four `browser_*` tools.
#[derive(Debug, Clone)]
pub struct BrowserToolDeps {
    pub session: Arc<BrowserSession>,
    pub shots_dir: PathBuf,
}

fn to_agent_err(e: BrowserError) -> AgentError {
    AgentError::other(e.to_string())
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

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let lease = self.deps.session.lease().await.map_err(to_agent_err)?;
        match input.get("action").and_then(|a| a.as_str()).unwrap_or("") {
            "screenshot" => {
                let shot = lease.backend().screenshot().await.map_err(to_agent_err)?;
                drop(lease); // release the browser before disk I/O
                std::fs::create_dir_all(&self.deps.shots_dir)
                    .map_err(|e| AgentError::other(format!("shots dir: {e}")))?;
                let name = format!(
                    "shot-{}.jpg",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                );
                let path = self.deps.shots_dir.join(name);
                std::fs::write(&path, &shot.bytes)
                    .map_err(|e| AgentError::other(format!("save screenshot: {e}")))?;
                let block = agent::attachments::image_from_bytes(&shot.bytes)
                    .map_err(|e| AgentError::other(format!("screenshot encode: {e}")))?;
                Ok(json!({
                    "__agent_content_blocks__": [serde_json::to_value(&block)
                        .map_err(|e| AgentError::other(e.to_string()))?],
                    "text": format!("screenshot saved: {}", path.display()),
                }))
            }
            "snapshot" => Ok(json!({
                "outline": lease.backend().snapshot().await.map_err(to_agent_err)?
            })),
            "console" => {
                let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                Ok(json!({
                    "entries": lease.backend().console_logs(limit).await.map_err(to_agent_err)?
                }))
            }
            "network" => {
                let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
                Ok(json!({
                    "entries": lease.backend().network_log(limit).await.map_err(to_agent_err)?
                }))
            }
            "downloads" => {
                let limit = input
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100)
                    .clamp(1, 100) as usize;
                Ok(json!({
                    "entries": lease.backend().downloads(limit).await.map_err(to_agent_err)?
                }))
            }
            "tabs" => Ok(json!({
                "tabs": lease.backend().tabs().await.map_err(to_agent_err)?
            })),
            other => Err(AgentError::other(format!("unknown action: {other:?}"))),
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
         targets: CSS selector, snapshot ref, or x/y."
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

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let lease = self.deps.session.lease().await.map_err(to_agent_err)?;
        match input.get("action").and_then(|a| a.as_str()).unwrap_or("") {
            "navigate" => {
                let url = input
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("navigate: 'url' required"))?;
                let final_url = lease.backend().navigate(url).await.map_err(to_agent_err)?;
                Ok(json!({ "url": final_url }))
            }
            "click" => {
                let target = click_target(&input)?;
                lease.backend().click(&target).await.map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            "type" => {
                let target = click_target(&input)?;
                let text = input
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("type: 'text' required"))?;
                lease
                    .backend()
                    .type_text(&target, text)
                    .await
                    .map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            "key" => {
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("key: 'key' required"))?;
                lease.backend().press_key(key).await.map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            "scroll" => {
                let dx = input.get("dx").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let dy = input.get("dy").and_then(|v| v.as_f64()).unwrap_or(600.0);
                lease.backend().scroll(dx, dy).await.map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            other => Err(AgentError::other(format!("unknown action: {other:?}"))),
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

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let expression = input
            .get("expression")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::other("eval: 'expression' required"))?;
        let lease = self.deps.session.lease().await.map_err(to_agent_err)?;
        let value = lease
            .backend()
            .evaluate(expression)
            .await
            .map_err(to_agent_err)?;
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

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let lease = self.deps.session.lease().await.map_err(to_agent_err)?;
        match input.get("action").and_then(|a| a.as_str()).unwrap_or("") {
            "new" => {
                let url = input.get("url").and_then(|v| v.as_str());
                let tab = lease.backend().tab_new(url).await.map_err(to_agent_err)?;
                Ok(json!({ "tab": tab }))
            }
            "close" => {
                let id = input
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("close: 'id' required"))?;
                lease.backend().tab_close(id).await.map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            "select" => {
                let id = input
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AgentError::other("select: 'id' required"))?;
                lease.backend().tab_select(id).await.map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            other => Err(AgentError::other(format!("unknown action: {other:?}"))),
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
            },
            dir,
        )
    }

    fn ctx() -> ToolUseContext {
        ToolUseContext::new(std::env::temp_dir())
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
}
