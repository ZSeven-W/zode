//! Agent-facing desktop tools: `DesktopRead` (apps / windows / snapshot),
//! `DesktopAct` (semantic + keyboard actions), and `DesktopScreenshot`.
//! Reads are permission-laddered (subsystem consent → app allowlist), not
//! un-gated ReadOnly — desktop reads span the whole login session (spec §权限).

use std::path::PathBuf;
use std::sync::Arc;

use agent::abort::AbortController;
use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::approval::{Approval, ApprovalGate};

use super::backend::{DesktopBackend, DesktopError, ElementActionKind};
use super::screenshot::save_screenshot_artifact;
use super::session::{ActionFamily, DesktopSession};

/// Shared deps for the desktop tools.
#[derive(Clone)]
pub struct DesktopToolDeps {
    pub session: Arc<DesktopSession>,
    pub shots_dir: PathBuf,
    /// Approval gate used to obtain the one-time subsystem consent and per-app
    /// allowlist grants that the permission ladder requires (spec §权限). The
    /// grants are cached in the session scopes, so each prompt fires at most once.
    pub gate: Arc<dyn ApprovalGate>,
}

impl std::fmt::Debug for DesktopToolDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopToolDeps")
            .field("shots_dir", &self.shots_dir)
            .finish()
    }
}

fn to_agent_err(e: DesktopError) -> AgentError {
    AgentError::other(e.to_string())
}

/// Accessibility and CDP actors may keep running after their local receiver
/// is dropped. This guard makes every dispatched desktop operation fail closed
/// unless a response proves that the operation reached a terminal state.
struct UnresolvedDesktopCall {
    abort: AbortController,
    armed: bool,
}

impl UnresolvedDesktopCall {
    fn new(abort: AbortController) -> Self {
        Self { abort, armed: true }
    }

    fn resolve(&mut self) {
        self.armed = false;
    }
}

impl Drop for UnresolvedDesktopCall {
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
            .unwrap_or_else(|| "desktop operation aborted".into()),
    )
}

fn desktop_error_is_terminal(error: &DesktopError) -> bool {
    !matches!(
        error,
        DesktopError::Protocol(_)
            | DesktopError::Timeout(_)
            | DesktopError::Dead(_)
            | DesktopError::PartialInput { .. }
            | DesktopError::PartialKeyInput { .. }
    )
}

async fn await_desktop_response<T>(
    ctx: &ToolUseContext,
    request: impl std::future::Future<Output = Result<T, DesktopError>>,
) -> Result<T, AgentError> {
    if ctx.abort.is_aborted() {
        return Err(aborted(ctx));
    }
    ctx.abort.pulse();
    let mut unresolved = UnresolvedDesktopCall::new(ctx.abort.clone());
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
                    if desktop_error_is_terminal(&error) {
                        unresolved.resolve();
                    }
                    Err(to_agent_err(error))
                }
            }
        }
    }
}

/// Map a `DesktopAct` action to its approval family. Total over the schema's
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

/// Ensure the ladder is satisfied for a tool call: obtain one-time subsystem
/// consent, and (when `exe` is given) allowlist that app. Prompts go through the
/// approval gate; results are cached in the session scopes so a granted app or
/// consent is never re-prompted. A denial fails the call.
async fn authorize(deps: &DesktopToolDeps, exe: Option<&str>) -> Result<(), AgentError> {
    let scopes = deps.session.scopes();
    if !scopes.subsystem_consented() {
        let allowed = deps
            .gate
            .approve(
                "desktop",
                &json!({
                    "_desktop_request": "subsystem-consent",
                    "note": "Allow zode to read and control desktop apps via accessibility for this session?"
                }),
            )
            .await;
        if matches!(allowed, Approval::Deny) {
            return Err(AgentError::other("desktop subsystem consent denied"));
        }
        scopes.grant_subsystem();
    }
    if let Some(exe) = exe {
        if !scopes.is_app_allowed(exe) {
            let allowed = deps
                .gate
                .approve(
                    "desktop",
                    &json!({ "_desktop_request": "allow-app", "app": exe }),
                )
                .await;
            if matches!(allowed, Approval::Deny) {
                return Err(to_agent_err(DesktopError::PermissionDenied(format!(
                    "app {exe:?} was not approved"
                ))));
            }
            scopes.allow_app(exe);
        }
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
        "DesktopRead"
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

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let action = input.get("action").and_then(|a| a.as_str()).unwrap_or("");
        if !matches!(action, "apps" | "windows" | "snapshot") {
            return Err(AgentError::other(format!("unknown action: {action:?}")));
        }
        match action {
            "apps" => {
                authorize(&self.deps, None).await?;
                // Platform apps (AX/UIA/AT-SPI2) plus any CDP-attached instance,
                // so the model can address an attached Electron app by identity.
                let lease = await_desktop_response(ctx, self.deps.session.lease()).await?;
                let backend = lease.backend();
                let mut apps = await_desktop_response(ctx, backend.list_apps()).await?;
                drop(lease);
                if let Some(cdp) = self.deps.session.cdp().await {
                    if let Ok(mut cdp_apps) = await_desktop_response(ctx, cdp.list_apps()).await {
                        apps.append(&mut cdp_apps);
                    }
                }
                Ok(json!({ "apps": apps }))
            }
            "windows" => {
                let exe = arg_str(&input, "app")?;
                authorize(&self.deps, Some(exe)).await?;
                let rb =
                    await_desktop_response(ctx, self.deps.session.resolve_backend(exe)).await?;
                let app = self.deps.session.resolve_app(exe);
                let backend = rb.backend();
                let windows = await_desktop_response(ctx, backend.list_windows(&app)).await?;
                Ok(json!({ "windows": windows }))
            }
            "snapshot" => {
                let exe = arg_str(&input, "app")?;
                authorize(&self.deps, Some(exe)).await?;
                let rb =
                    await_desktop_response(ctx, self.deps.session.resolve_backend(exe)).await?;
                let app = self.deps.session.resolve_app(exe);
                let token = arg_str(&input, "window")?;
                let win = self
                    .deps
                    .session
                    .resolve_window(app, token)
                    .map_err(to_agent_err)?;
                let backend = rb.backend();
                let snap = await_desktop_response(ctx, backend.snapshot(&win, None)).await?;
                Ok(json!({ "outline": snap.outline }))
            }
            _ => unreachable!("validated desktop_read action"),
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
        "DesktopAct"
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

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let action = input.get("action").and_then(|a| a.as_str()).unwrap_or("");
        if action_family(action).is_none() {
            return Err(AgentError::other(format!("unknown action: {action:?}")));
        }
        let exe = arg_str(&input, "app")?;
        authorize(&self.deps, Some(exe)).await?;
        let app = self.deps.session.resolve_app(exe);
        let rb = await_desktop_response(ctx, self.deps.session.resolve_backend(exe)).await?;
        let backend = rb.backend();

        match action {
            "launch" => {
                let ident = super::backend::AppLaunchId(exe.to_string());
                let info = await_desktop_response(ctx, backend.launch_app(&ident)).await?;
                Ok(json!({ "launched": info }))
            }
            "focus" => {
                let win = self
                    .deps
                    .session
                    .resolve_window(app, arg_str(&input, "window")?)
                    .map_err(to_agent_err)?;
                await_desktop_response(ctx, backend.focus_window(&win)).await?;
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
                let out = await_desktop_response(ctx, backend.element_action(&er, kind)).await?;
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
                await_desktop_response(ctx, backend.set_value(&er, arg_str(&input, "text")?))
                    .await?;
                Ok(json!({ "ok": true }))
            }
            "type" => {
                let win = self
                    .deps
                    .session
                    .resolve_window(app, arg_str(&input, "window")?)
                    .map_err(to_agent_err)?;
                await_desktop_response(ctx, backend.type_text(&win, arg_str(&input, "text")?))
                    .await?;
                Ok(json!({ "ok": true }))
            }
            "key" => {
                let win = self
                    .deps
                    .session
                    .resolve_window(app, arg_str(&input, "window")?)
                    .map_err(to_agent_err)?;
                await_desktop_response(ctx, backend.key(&win, arg_str(&input, "combo")?)).await?;
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
        "DesktopScreenshot"
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

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let exe = arg_str(&input, "app")?;
        authorize(&self.deps, Some(exe)).await?;
        let app = self.deps.session.resolve_app(exe);
        let win = self
            .deps
            .session
            .resolve_window(app, arg_str(&input, "window")?)
            .map_err(to_agent_err)?;
        let rb = await_desktop_response(ctx, self.deps.session.resolve_backend(exe)).await?;
        let backend = rb.backend();
        let shot = await_desktop_response(ctx, backend.screenshot(&win)).await?;
        drop(rb); // release the input lease before disk I/O
        save_screenshot_artifact(&self.deps.shots_dir, &shot.bytes)
    }
}

/// Evaluates arbitrary JS in a CDP-attached Electron/Chromium page. Mutating —
/// gated independently (its own always-allow flag), and only usable when a CDP
/// backend is attached (`/desktop attach`). Higher risk than browser_eval:
/// Electron pages can reach preload/IPC/Node APIs (spec §desktop_eval).
#[derive(Debug)]
pub struct DesktopEvalTool {
    deps: DesktopToolDeps,
}

impl DesktopEvalTool {
    pub fn new(deps: DesktopToolDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for DesktopEvalTool {
    fn name(&self) -> &str {
        "DesktopEval"
    }

    fn description(&self) -> &str {
        "Evaluate JavaScript in an attached Electron/Chromium page (requires a CDP attachment via \
         /desktop attach). Returns the JSON result. Higher risk than browser_eval."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["window", "expression"],
            "properties": {
                "window": { "type": "string", "description": "Page window token (from desktop_read windows)" },
                "expression": { "type": "string", "description": "JavaScript expression to evaluate" }
            }
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        authorize(&self.deps, None).await?;
        let expr = arg_str(&input, "expression")?;
        let token = arg_str(&input, "window")?;
        let cdp = self.deps.session.cdp().await.ok_or_else(|| {
            AgentError::other(
                "no CDP attachment; run `/desktop attach <port>` on a running Electron app first",
            )
        })?;
        // CDP windows are addressed by the same numeric token model.
        let app = self.deps.session.resolve_app("cdp");
        let win = self
            .deps
            .session
            .resolve_window(app, token)
            .map_err(to_agent_err)?;
        let value = await_desktop_response(ctx, cdp.evaluate(&win, expr)).await?;
        Ok(json!({ "value": value }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DesktopConfig;
    use agent::tool::{SafetyClass, Tool, ToolUseContext};
    use serde_json::json;

    #[derive(Debug)]
    struct DenyGate;
    #[async_trait::async_trait]
    impl crate::approval::ApprovalGate for DenyGate {
        async fn approve(&self, _t: &str, _i: &Value) -> crate::approval::Approval {
            crate::approval::Approval::Deny
        }
    }

    fn deps_with(
        gate: Arc<dyn crate::approval::ApprovalGate>,
    ) -> (DesktopToolDeps, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let session = DesktopSession::new(
            DesktopConfig::default(),
            crate::desktop::mock::mock_factory(),
        );
        (
            DesktopToolDeps {
                session,
                shots_dir: dir.path().to_path_buf(),
                gate,
            },
            dir,
        )
    }
    /// Deps whose gate auto-approves — consent + allowlist grants succeed.
    fn deps() -> (DesktopToolDeps, tempfile::TempDir) {
        deps_with(Arc::new(crate::approval::BypassGate))
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
    async fn read_dispatch_with_granted_consent() {
        let (d, _g) = deps(); // BypassGate → consent + allowlist auto-granted
        let t = DesktopReadTool::new(d.clone());
        let apps = t.call(&ctx(), json!({"action":"apps"})).await.unwrap();
        assert_eq!(apps["apps"][0]["name"], "TextEdit");
        // consent got recorded through the gate, not pre-seeded
        assert!(d.session.scopes().subsystem_consented());
        let snap = t
            .call(
                &ctx(),
                json!({"action":"snapshot","app":"com.apple.TextEdit","window":"w1"}),
            )
            .await
            .unwrap();
        assert!(snap["outline"].as_str().unwrap().contains("[e1]"));
        assert!(d.session.scopes().is_app_allowed("com.apple.TextEdit"));
    }

    #[tokio::test]
    async fn denied_consent_blocks_reads() {
        let (d, _g) = deps_with(Arc::new(DenyGate));
        let err = DesktopReadTool::new(d)
            .call(&ctx(), json!({"action":"apps"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("consent"));
    }

    #[tokio::test]
    async fn denied_app_blocks_snapshot() {
        // Consent granted, but the per-app approval is denied.
        let (d, _g) = deps_with(Arc::new(DenyGate));
        d.session.scopes().grant_subsystem();
        let err = DesktopReadTool::new(d)
            .call(
                &ctx(),
                json!({"action":"snapshot","app":"com.other","window":"w1"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not approved"));
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

    #[tokio::test]
    async fn uncertain_desktop_error_latches_unresolved_work() {
        for error in [
            DesktopError::Protocol("actor response lost".into()),
            DesktopError::Timeout("actor request".into()),
        ] {
            let context = ctx();
            let result: Result<(), AgentError> =
                await_desktop_response(&context, async { Err(error) }).await;
            assert!(result.is_err());
            assert!(context.abort.activity().unresolved_external_work());
        }
    }

    #[tokio::test]
    async fn explicit_desktop_response_does_not_latch_unresolved_work() {
        let context = ctx();
        assert_eq!(
            await_desktop_response(&context, async { Ok::<_, DesktopError>(42) })
                .await
                .unwrap(),
            42
        );
        assert!(!context.abort.activity().unresolved_external_work());

        let terminal = ctx();
        let _: Result<(), AgentError> = await_desktop_response(&terminal, async {
            Err(DesktopError::NotFound("no window".into()))
        })
        .await;
        assert!(!terminal.abort.activity().unresolved_external_work());
    }

    #[tokio::test]
    async fn abort_after_desktop_dispatch_latches_unresolved_work() {
        let context = ctx();
        let abort = context.abort.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _ = started_rx.await;
            abort.abort_with_reason("test stop");
        });
        let request = async move {
            let _ = started_tx.send(());
            std::future::pending::<Result<(), DesktopError>>().await
        };

        let result = await_desktop_response(&context, request).await;
        assert!(matches!(result, Err(AgentError::Aborted(_))));
        assert!(context.abort.activity().unresolved_external_work());
    }
}
