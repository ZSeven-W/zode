//! Agent-facing desktop tools: `desktop_read` (apps / windows / snapshot),
//! `desktop_act` (semantic + keyboard actions), and `desktop_screenshot`.
//! Reads are permission-laddered (subsystem consent → app allowlist), not
//! un-gated ReadOnly — desktop reads span the whole login session (spec §权限).

use std::path::PathBuf;
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::backend::{DesktopError, ElementActionKind};
use super::screenshot::save_screenshot_artifact;
use super::session::{ActionFamily, DesktopSession};

/// Shared deps for the desktop tools.
#[derive(Debug, Clone)]
pub struct DesktopToolDeps {
    pub session: Arc<DesktopSession>,
    pub shots_dir: PathBuf,
}

fn to_agent_err(e: DesktopError) -> AgentError {
    AgentError::other(e.to_string())
}

/// Map a `desktop_act` action to its approval family. Total over the schema's
/// action enum; `None` for anything unknown. The registration-invariant test
/// enumerates the schema and asserts every action maps to exactly one family.
pub fn action_family(action: &str) -> Option<ActionFamily> {
    Some(match action {
        "launch" => ActionFamily::Launch,
        "focus" | "type" | "key" => ActionFamily::RawInput,
        "click" | "toggle" | "expand" | "scroll" | "set_value" => ActionFamily::Element,
        _ => return None,
    })
}

fn require_consent(deps: &DesktopToolDeps) -> Result<(), AgentError> {
    if !deps.session.scopes().subsystem_consented() {
        return Err(AgentError::other(
            "desktop subsystem consent required for this session before any desktop tool can run",
        ));
    }
    Ok(())
}

fn require_app_allowed(deps: &DesktopToolDeps, exe: &str) -> Result<(), AgentError> {
    if !deps.session.scopes().is_app_allowed(exe) {
        return Err(to_agent_err(DesktopError::PermissionDenied(format!(
            "app {exe:?} is not in the session allowlist; approve it before reading or acting on it"
        ))));
    }
    Ok(())
}

fn arg_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, AgentError> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| AgentError::other(format!("'{key}' required")))
}

/// Reads the desktop: app list, window list, accessibility snapshot. Not
/// un-gated ReadOnly — enforces the permission ladder (spec §权限).
#[derive(Debug)]
pub struct DesktopReadTool {
    deps: DesktopToolDeps,
}

impl DesktopReadTool {
    pub fn new(deps: DesktopToolDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for DesktopReadTool {
    fn name(&self) -> &str {
        "desktop_read"
    }

    fn description(&self) -> &str {
        "Read the desktop via the accessibility tree: list apps, list an app's windows, or take a \
         ref-annotated snapshot of a window. Requires desktop subsystem consent; window/snapshot \
         require the app to be allowlisted."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["apps", "windows", "snapshot"],
                    "description": "Which read to perform"
                },
                "app": { "type": "string", "description": "App executable identity (required for windows/snapshot)" },
                "window": { "type": "string", "description": "Window token (required for snapshot)" },
                "scope": { "type": "integer", "description": "Optional ref to snapshot a subtree" }
            }
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        require_consent(&self.deps)?;
        let lease = self.deps.session.lease().await.map_err(to_agent_err)?;
        match input.get("action").and_then(|a| a.as_str()).unwrap_or("") {
            "apps" => {
                let apps = lease.backend().list_apps().await.map_err(to_agent_err)?;
                Ok(json!({ "apps": apps }))
            }
            "windows" => {
                let exe = arg_str(&input, "app")?;
                let app = self.deps.session.resolve_app(exe);
                let windows = lease
                    .backend()
                    .list_windows(&app)
                    .await
                    .map_err(to_agent_err)?;
                if self.deps.session.scopes().is_app_allowed(exe) {
                    Ok(json!({ "windows": windows }))
                } else {
                    // Not allowlisted: return a redacted summary only — window
                    // titles are content, not harmless metadata (spec §权限).
                    let tokens: Vec<&str> = windows.iter().map(|w| w.token.as_str()).collect();
                    Ok(json!({
                        "count": windows.len(),
                        "tokens": tokens,
                        "note": "app not allowlisted; titles withheld — approve the app for full window info"
                    }))
                }
            }
            "snapshot" => {
                let exe = arg_str(&input, "app")?;
                require_app_allowed(&self.deps, exe)?;
                let app = self.deps.session.resolve_app(exe);
                let token = arg_str(&input, "window")?;
                let win = self
                    .deps
                    .session
                    .resolve_window(app, token)
                    .map_err(to_agent_err)?;
                let snap = lease
                    .backend()
                    .snapshot(&win, None)
                    .await
                    .map_err(to_agent_err)?;
                Ok(json!({ "outline": snap.outline }))
            }
            other => Err(AgentError::other(format!("unknown action: {other:?}"))),
        }
    }
}

/// Acts on the desktop: semantic element actions, focus, keyboard input, launch.
/// Mutating — gated by `desktop_gated`; the tool body enforces consent+allowlist.
#[derive(Debug)]
pub struct DesktopActTool {
    deps: DesktopToolDeps,
}

impl DesktopActTool {
    pub fn new(deps: DesktopToolDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for DesktopActTool {
    fn name(&self) -> &str {
        "desktop_act"
    }

    fn description(&self) -> &str {
        "Act on the desktop: click/toggle/expand/scroll/set_value a snapshot ref, focus a window, \
         type text, press a key combo, or launch an app. Requires consent + an allowlisted app. \
         M1 has no pointer synthesis: elements without a semantic action are refused."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action", "app"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["launch", "focus", "click", "toggle", "expand", "scroll", "set_value", "type", "key"]
                },
                "app": { "type": "string", "description": "App executable identity" },
                "window": { "type": "string", "description": "Window token (focus/click/.../type/key)" },
                "ref": { "type": "integer", "description": "Snapshot element ref (element actions)" },
                "text": { "type": "string", "description": "Text for set_value / type" },
                "combo": { "type": "string", "description": "Key combo for key (e.g. Cmd+S)" }
            }
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        require_consent(&self.deps)?;
        let action = input.get("action").and_then(|a| a.as_str()).unwrap_or("");
        if action_family(action).is_none() {
            return Err(AgentError::other(format!("unknown action: {action:?}")));
        }
        let exe = arg_str(&input, "app")?;
        require_app_allowed(&self.deps, exe)?;
        let app = self.deps.session.resolve_app(exe);
        let lease = self.deps.session.lease().await.map_err(to_agent_err)?;
        let backend = lease.backend();

        match action {
            "launch" => {
                let info = backend
                    .launch_app(&super::backend::AppLaunchId(exe.to_string()))
                    .await
                    .map_err(to_agent_err)?;
                Ok(json!({ "launched": info }))
            }
            "focus" => {
                let win = self
                    .deps
                    .session
                    .resolve_window(app, arg_str(&input, "window")?)
                    .map_err(to_agent_err)?;
                backend.focus_window(&win).await.map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            "click" | "toggle" | "expand" | "scroll" => {
                let win = self
                    .deps
                    .session
                    .resolve_window(app, arg_str(&input, "window")?)
                    .map_err(to_agent_err)?;
                let local = input
                    .get("ref")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| AgentError::other("'ref' required for element actions"))?;
                let er = super::backend::ElementRef::new(win, 0, local);
                let kind = match action {
                    "click" => ElementActionKind::Click,
                    "toggle" => ElementActionKind::Toggle,
                    "expand" => ElementActionKind::Expand,
                    _ => ElementActionKind::Scroll,
                };
                let out = backend
                    .element_action(&er, kind)
                    .await
                    .map_err(to_agent_err)?;
                Ok(json!({ "result": out }))
            }
            "set_value" => {
                let win = self
                    .deps
                    .session
                    .resolve_window(app, arg_str(&input, "window")?)
                    .map_err(to_agent_err)?;
                let local = input
                    .get("ref")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| AgentError::other("'ref' required for set_value"))?;
                let er = super::backend::ElementRef::new(win, 0, local);
                backend
                    .set_value(&er, arg_str(&input, "text")?)
                    .await
                    .map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            "type" => {
                let win = self
                    .deps
                    .session
                    .resolve_window(app, arg_str(&input, "window")?)
                    .map_err(to_agent_err)?;
                backend
                    .type_text(&win, arg_str(&input, "text")?)
                    .await
                    .map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            "key" => {
                let win = self
                    .deps
                    .session
                    .resolve_window(app, arg_str(&input, "window")?)
                    .map_err(to_agent_err)?;
                backend
                    .key(&win, arg_str(&input, "combo")?)
                    .await
                    .map_err(to_agent_err)?;
                Ok(json!({ "ok": true }))
            }
            other => Err(AgentError::other(format!("unknown action: {other:?}"))),
        }
    }
}

/// Captures a window screenshot. Mutating (per-call approval via `desktop_gated`);
/// saves via the shared owner-only artifact helper and returns the sentinel.
#[derive(Debug)]
pub struct DesktopScreenshotTool {
    deps: DesktopToolDeps,
}

impl DesktopScreenshotTool {
    pub fn new(deps: DesktopToolDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for DesktopScreenshotTool {
    fn name(&self) -> &str {
        "desktop_screenshot"
    }

    fn description(&self) -> &str {
        "Capture a window screenshot (window-level, never full-screen fallback). Requires consent \
         and an allowlisted app; approved per call."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["app", "window"],
            "properties": {
                "app": { "type": "string", "description": "App executable identity" },
                "window": { "type": "string", "description": "Window token" }
            }
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        require_consent(&self.deps)?;
        let exe = arg_str(&input, "app")?;
        require_app_allowed(&self.deps, exe)?;
        let app = self.deps.session.resolve_app(exe);
        let win = self
            .deps
            .session
            .resolve_window(app, arg_str(&input, "window")?)
            .map_err(to_agent_err)?;
        let lease = self.deps.session.lease().await.map_err(to_agent_err)?;
        let shot = lease
            .backend()
            .screenshot(&win)
            .await
            .map_err(to_agent_err)?;
        drop(lease); // release the input lock before disk I/O
        save_screenshot_artifact(&self.deps.shots_dir, &shot.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DesktopConfig;
    use agent::tool::{SafetyClass, Tool, ToolUseContext};
    use serde_json::json;

    fn deps() -> (DesktopToolDeps, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let session = DesktopSession::new(
            DesktopConfig::default(),
            crate::desktop::mock::mock_factory(),
        );
        session.scopes().grant_subsystem();
        session.scopes().allow_app("com.apple.TextEdit");
        (
            DesktopToolDeps {
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
    fn safety_classes_and_family_map_is_total() {
        let (d, _g) = deps();
        assert_eq!(
            DesktopReadTool::new(d.clone()).safety_class(),
            SafetyClass::ReadOnly
        );
        assert_eq!(
            DesktopActTool::new(d.clone()).safety_class(),
            SafetyClass::Mutating
        );
        assert_eq!(
            DesktopScreenshotTool::new(d).safety_class(),
            SafetyClass::Mutating
        );
        for a in [
            "launch",
            "focus",
            "click",
            "toggle",
            "expand",
            "scroll",
            "set_value",
            "type",
            "key",
        ] {
            assert!(action_family(a).is_some(), "unmapped action {a}");
        }
        assert!(action_family("fly").is_none());
    }

    #[tokio::test]
    async fn read_dispatch_and_subsystem_gate() {
        let (d, _g) = deps();
        let t = DesktopReadTool::new(d.clone());
        let apps = t.call(&ctx(), json!({"action":"apps"})).await.unwrap();
        assert_eq!(apps["apps"][0]["name"], "TextEdit");
        let snap = t
            .call(
                &ctx(),
                json!({"action":"snapshot","app":"com.apple.TextEdit","window":"w1"}),
            )
            .await
            .unwrap();
        assert!(snap["outline"].as_str().unwrap().contains("[e1]"));

        // subsystem consent required: a fresh session without consent errors
        let s2 = DesktopSession::new(
            DesktopConfig::default(),
            crate::desktop::mock::mock_factory(),
        );
        let d2 = DesktopToolDeps {
            session: s2,
            shots_dir: d.shots_dir.clone(),
        };
        let err = DesktopReadTool::new(d2)
            .call(&ctx(), json!({"action":"apps"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("consent"));
    }

    #[tokio::test]
    async fn windows_redacts_titles_for_unallowlisted_app() {
        let (d, _g) = deps();
        let t = DesktopReadTool::new(d);
        // "com.other" is NOT allowlisted → redacted summary, no titles
        let out = t
            .call(&ctx(), json!({"action":"windows","app":"com.other"}))
            .await
            .unwrap();
        assert!(out.get("count").is_some());
        assert!(out.get("windows").is_none());
        assert!(out.to_string().find("Untitled").is_none());
    }

    #[tokio::test]
    async fn snapshot_requires_allowlist() {
        let (d, _g) = deps();
        let err = DesktopReadTool::new(d)
            .call(
                &ctx(),
                json!({"action":"snapshot","app":"com.other","window":"w1"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("allowlist"));
    }

    #[tokio::test]
    async fn act_unknown_action_errors_and_element_needs_ref() {
        let (d, _g) = deps();
        let t = DesktopActTool::new(d);
        let err = t
            .call(&ctx(), json!({"action":"fly","app":"com.apple.TextEdit"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown action"));
        let err = t
            .call(
                &ctx(),
                json!({"action":"click","app":"com.apple.TextEdit","window":"w1"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("'ref' required"));
        // a well-formed click succeeds against the mock
        let ok = t
            .call(
                &ctx(),
                json!({"action":"click","app":"com.apple.TextEdit","window":"w1","ref":1}),
            )
            .await
            .unwrap();
        assert_eq!(ok["result"], "ok");
    }

    #[tokio::test]
    async fn screenshot_emits_sentinel() {
        let (d, _g) = deps();
        let out = DesktopScreenshotTool::new(d)
            .call(&ctx(), json!({"app":"com.apple.TextEdit","window":"w1"}))
            .await
            .unwrap();
        assert!(out
            .as_object()
            .unwrap()
            .contains_key("__agent_content_blocks__"));
    }
}
