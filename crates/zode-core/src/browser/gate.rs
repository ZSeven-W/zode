//! Browser-aware gate view: approval prompts for browser tools carry
//! the live target, page URL and origin, which are session state the
//! model's input cannot be trusted to report — plus the site-scoping
//! wrapper that turns "allow always" into either a session-wide flag or
//! a persistent per-origin grant.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use agent::tool::Tool;
use async_trait::async_trait;

use crate::approval::{Approval, ApprovalGate};
use crate::gated_tool::{GateView, PermissionGatedTool};

use super::backend::BrowserTarget;
use super::session::BrowserSession;
use super::site_auth::{AlwaysScope, GateDecision, Origin, SiteAuthStore};

#[derive(Debug)]
pub struct BrowserGateView {
    session: Arc<BrowserSession>,
    /// Mirrors the tools' `BrowserToolDeps::target_override`: the target the
    /// gated tool will actually drive, so approval prompts never claim
    /// "managed" for a turn pinned to the bridge (or vice versa).
    target_override: Option<BrowserTarget>,
}

impl BrowserGateView {
    pub(crate) fn with_target_override(
        session: Arc<BrowserSession>,
        target_override: Option<BrowserTarget>,
    ) -> Self {
        Self {
            session,
            target_override,
        }
    }

    fn effective_target(&self) -> BrowserTarget {
        self.target_override
            .clone()
            .unwrap_or_else(|| self.session.target())
    }
}

/// Origin the call should be authorized against.
///
/// The current page wins: a click or an eval runs against whatever is
/// loaded, whatever the model claims. Only when there is no resolvable
/// page origin (nothing loaded yet, `about:blank`, a `data:`/`file:` URL)
/// does a navigation-like call fall back to where it is *going* — that is
/// the first-navigate case. Anything else has no origin and stays on the
/// session-wide flag.
fn call_origin(input: &serde_json::Value, page_url: Option<&str>) -> Option<Origin> {
    if let Some(origin) = page_url.and_then(Origin::parse) {
        return Some(origin);
    }
    let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("");
    if !matches!(action, "navigate" | "new") {
        return None;
    }
    input
        .get("url")
        .and_then(|v| v.as_str())
        .and_then(Origin::parse)
}

#[async_trait]
impl GateView for BrowserGateView {
    async fn view(&self, input: &serde_json::Value) -> serde_json::Value {
        let mut shown = input.clone();
        if let Some(obj) = shown.as_object_mut() {
            obj.insert(
                "_target".into(),
                serde_json::json!(match self.effective_target() {
                    BrowserTarget::Managed => "managed",
                    BrowserTarget::Bridge => "bridge",
                }),
            );
            let page_url = self.session.current_url_hint().await;
            if let Some(url) = &page_url {
                obj.insert("_page_url".into(), serde_json::json!(url));
            }
            if let Some(origin) = call_origin(input, page_url.as_deref()) {
                obj.insert("_origin".into(), serde_json::json!(origin.as_str()));
            }
        }
        shown
    }
}

/// Approval gate wrapper that scopes "allow always" to a site when the
/// user asks for that.
///
/// Decision order for one call (see [`GateDecision::resolve`]):
/// 1. a stored grant for (tool, origin) — allowed, no prompt;
/// 2. the tool's session-wide "always" flag — allowed, no prompt (in
///    practice [`PermissionGatedTool`] short-circuits on it before we are
///    even called);
/// 3. otherwise prompt.
///
/// The prompt payload carries `_scope_request` (an id) and
/// `_scope_options`. A UI that understands scopes calls
/// [`SiteAuthStore::choose_scope`] with that id before answering
/// `AllowAlways`; when it answers `Site`, the grant is stored for the
/// origin and this gate downgrades the answer to `AllowOnce` so the
/// tool's global flag stays off. A UI that knows nothing about scopes —
/// today's TUI dialog — answers plain `AllowAlways` and gets exactly the
/// previous, session-wide behavior.
#[derive(Debug)]
pub struct SiteScopedGate {
    inner: Arc<dyn ApprovalGate>,
    store: Arc<SiteAuthStore>,
    /// The wrapped tool's "always" flag, injected right after the
    /// [`PermissionGatedTool`] that owns it is built.
    always: OnceLock<Arc<AtomicBool>>,
}

impl SiteScopedGate {
    pub fn new(inner: Arc<dyn ApprovalGate>, store: Arc<SiteAuthStore>) -> Self {
        Self {
            inner,
            store,
            always: OnceLock::new(),
        }
    }

    fn bind_always_flag(&self, flag: Arc<AtomicBool>) {
        let _ = self.always.set(flag);
    }

    fn global_always(&self) -> bool {
        self.always.get().is_some_and(|f| f.load(Ordering::Relaxed))
    }
}

#[async_trait]
impl ApprovalGate for SiteScopedGate {
    async fn approve(&self, tool: &str, input: &serde_json::Value) -> Approval {
        let origin = input
            .get("_origin")
            .and_then(|v| v.as_str())
            .and_then(Origin::parse);
        let granted = origin
            .as_ref()
            .is_some_and(|o| self.store.is_granted(tool, o));
        if !GateDecision::resolve(self.global_always(), granted).is_prompt() {
            return Approval::AllowOnce;
        }

        let request_id = self.store.next_request_id();
        let mut shown = input.clone();
        if let Some(obj) = shown.as_object_mut() {
            obj.insert("_scope_request".into(), serde_json::json!(request_id));
            obj.insert(
                "_scope_options".into(),
                serde_json::json!(match origin {
                    Some(_) => vec!["global", "site"],
                    None => vec!["global"],
                }),
            );
        }

        let answer = self.inner.approve(tool, &shown).await;
        let scope = self.store.take_scope(request_id);
        match (answer, scope, origin) {
            (Approval::AllowAlways, AlwaysScope::Site, Some(origin)) => {
                self.store.grant(tool, &origin);
                // Deliberately not AllowAlways: the caller's global flag
                // must stay off so other sites keep prompting.
                Approval::AllowOnce
            }
            (answer, _, _) => answer,
        }
    }
}

/// Wrap `inner` in a [`PermissionGatedTool`] whose approval prompts are
/// enriched with the session's live target/URL/origin (see
/// [`BrowserGateView`]) and whose "allow always" answers can be scoped to
/// one site (see [`SiteScopedGate`]), and register its global "allow
/// always" flag with the session's perm-flag registry so `/browser`
/// status can report it.
pub fn browser_gated(
    inner: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    session: Arc<BrowserSession>,
) -> Arc<PermissionGatedTool> {
    let store = session.site_auth();
    browser_gated_with(inner, gate, session, None, store)
}

/// [`browser_gated`] with an explicit target override so the approval
/// prompt's `_target` matches the backend the tool will actually drive
/// (extension-task engines pin this to `Bridge`).
pub fn browser_gated_as(
    inner: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    session: Arc<BrowserSession>,
    target_override: Option<BrowserTarget>,
) -> Arc<PermissionGatedTool> {
    let store = session.site_auth();
    browser_gated_with(inner, gate, session, target_override, store)
}

/// [`browser_gated`] against an explicit grant store (tests, and any
/// caller that wants a store other than the session's).
pub fn browser_gated_with_store(
    inner: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    session: Arc<BrowserSession>,
    store: Arc<SiteAuthStore>,
) -> Arc<PermissionGatedTool> {
    browser_gated_with(inner, gate, session, None, store)
}

fn browser_gated_with(
    inner: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    session: Arc<BrowserSession>,
    target_override: Option<BrowserTarget>,
    store: Arc<SiteAuthStore>,
) -> Arc<PermissionGatedTool> {
    let name = inner.name().to_string();
    let view = Arc::new(BrowserGateView::with_target_override(
        session.clone(),
        target_override,
    ));
    let scoped = Arc::new(SiteScopedGate::new(gate, store));
    let gated = Arc::new(PermissionGatedTool::with_view(inner, scoped.clone(), view));
    scoped.bind_always_flag(gated.always_flag());
    session.register_perm_flag(&name, gated.always_flag());
    gated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::{Approval, ApprovalGate};
    use serde_json::json;

    #[derive(Debug)]
    struct CapturingGate {
        seen: std::sync::Mutex<Vec<serde_json::Value>>,
        answer: Approval,
        /// Scope this gate announces before answering `AllowAlways`,
        /// standing in for a scope-aware UI.
        scope: Option<(AlwaysScope, Arc<SiteAuthStore>)>,
    }

    impl CapturingGate {
        fn answering(answer: Approval) -> Arc<Self> {
            Arc::new(Self {
                seen: Default::default(),
                answer,
                scope: None,
            })
        }

        fn scoped(answer: Approval, scope: AlwaysScope, store: Arc<SiteAuthStore>) -> Arc<Self> {
            Arc::new(Self {
                seen: Default::default(),
                answer,
                scope: Some((scope, store)),
            })
        }

        fn prompts(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
    }

    #[async_trait::async_trait]
    impl ApprovalGate for CapturingGate {
        async fn approve(&self, _tool: &str, input: &serde_json::Value) -> Approval {
            self.seen.lock().unwrap().push(input.clone());
            if let Some((scope, store)) = &self.scope {
                let id = input.get("_scope_request").and_then(|v| v.as_u64());
                if let Some(id) = id {
                    store.choose_scope(id, *scope);
                }
            }
            self.answer
        }
    }

    #[derive(Debug)]
    struct EchoInput(&'static str);
    #[async_trait::async_trait]
    impl agent::tool::Tool for EchoInput {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            "t"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object"})
        }
        fn safety_class(&self) -> agent::tool::SafetyClass {
            agent::tool::SafetyClass::Mutating
        }
        async fn call(
            &self,
            _ctx: &agent::tool::ToolUseContext,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, agent::error::AgentError> {
            Ok(input)
        }
    }

    fn session() -> std::sync::Arc<crate::browser::BrowserSession> {
        crate::browser::BrowserSession::new(
            crate::config::BrowserConfig::default(),
            crate::browser::backend::mock::mock_factory(),
        )
    }

    fn ctx() -> agent::tool::ToolUseContext {
        agent::tool::ToolUseContext::new(std::env::temp_dir())
    }

    #[tokio::test]
    async fn gate_sees_context_inner_does_not() {
        let gate = CapturingGate::answering(Approval::AllowOnce);
        let s = session();
        {
            s.lease().await.unwrap();
        } // boot backend so url hint resolves
        let t = browser_gated_with_store(
            Arc::new(EchoInput("browser_act")),
            gate.clone(),
            s,
            SiteAuthStore::in_memory(),
        );
        let out = t
            .call(
                &ctx(),
                json!({"action": "navigate", "url": "https://x.test"}),
            )
            .await
            .unwrap();
        assert_eq!(out, json!({"action": "navigate", "url": "https://x.test"})); // untouched
        let seen = gate.seen.lock().unwrap();
        assert_eq!(seen[0]["_target"], "managed");
        assert_eq!(seen[0]["_page_url"], "https://example.test/");
        assert_eq!(seen[0]["_origin"], "https://example.test");
        assert_eq!(seen[0]["_scope_options"], json!(["global", "site"]));
        assert!(seen[0]["_scope_request"].is_u64());
    }

    #[tokio::test]
    async fn gate_reports_effective_target_when_overridden() {
        // Deny so the inner tool never leases.
        let gate = CapturingGate::answering(Approval::Deny);
        let s = session(); // session-wide target stays managed
        let t = browser_gated_as(
            Arc::new(EchoInput("browser_act")),
            gate.clone(),
            s,
            Some(crate::browser::BrowserTarget::Bridge),
        );
        let _ = t.call(&ctx(), json!({})).await;
        let seen = gate.seen.lock().unwrap();
        assert_eq!(seen[0]["_target"], "bridge");
    }

    #[tokio::test]
    async fn always_allow_skips_second_prompt_and_registers_flag() {
        let gate = CapturingGate::answering(Approval::AllowAlways);
        let s = session();
        let t = browser_gated_with_store(
            Arc::new(EchoInput("browser_act")),
            gate.clone(),
            s.clone(),
            SiteAuthStore::in_memory(),
        );
        t.call(&ctx(), json!({})).await.unwrap();
        t.call(&ctx(), json!({})).await.unwrap();
        assert_eq!(gate.prompts(), 1);
        let flags = s.perm_flags();
        assert_eq!(flags[0].0, "browser_act");
        assert!(flags[0].1.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn deny_is_an_error() {
        let gate = CapturingGate::answering(Approval::Deny);
        let t = browser_gated_with_store(
            Arc::new(EchoInput("browser_act")),
            gate,
            session(),
            SiteAuthStore::in_memory(),
        );
        let err = t.call(&ctx(), json!({})).await.unwrap_err();
        assert!(err.to_string().contains("denied"));
    }

    #[tokio::test]
    async fn site_scoped_always_grants_only_that_origin() {
        let store = SiteAuthStore::in_memory();
        let gate = CapturingGate::scoped(Approval::AllowAlways, AlwaysScope::Site, store.clone());
        let s = session();
        {
            s.lease().await.unwrap();
        } // current page is https://example.test/
        let t = browser_gated_with_store(
            Arc::new(EchoInput("browser_act")),
            gate.clone(),
            s.clone(),
            store.clone(),
        );

        t.call(&ctx(), json!({"action": "click", "ref": 1}))
            .await
            .unwrap();
        assert_eq!(gate.prompts(), 1);
        assert!(store.is_granted(
            "browser_act",
            &Origin::parse("https://example.test").unwrap()
        ));
        assert!(
            !t.is_always_allowed(),
            "a site grant must not set the global flag"
        );

        // Same origin: no second prompt.
        t.call(&ctx(), json!({"action": "click", "ref": 2}))
            .await
            .unwrap();
        assert_eq!(gate.prompts(), 1);
    }

    #[tokio::test]
    async fn site_grant_does_not_cover_another_origin_or_another_tool() {
        let store = SiteAuthStore::in_memory();
        store.grant(
            "browser_act",
            &Origin::parse("https://example.test").unwrap(),
        );
        let s = session();
        {
            s.lease().await.unwrap();
        }

        // Granted origin + granted tool: allowed without a prompt.
        let act_gate = CapturingGate::answering(Approval::Deny);
        let act = browser_gated_with_store(
            Arc::new(EchoInput("browser_act")),
            act_gate.clone(),
            s.clone(),
            store.clone(),
        );
        act.call(&ctx(), json!({"action": "click", "ref": 1}))
            .await
            .unwrap();
        assert_eq!(act_gate.prompts(), 0);

        // Same origin, different tool: browser_eval is gated separately.
        let eval_gate = CapturingGate::answering(Approval::Deny);
        let eval = browser_gated_with_store(
            Arc::new(EchoInput("browser_eval")),
            eval_gate.clone(),
            s.clone(),
            store.clone(),
        );
        assert!(eval.call(&ctx(), json!({"expression": "1"})).await.is_err());
        assert_eq!(eval_gate.prompts(), 1);

        // Different destination origin on a page-less session: prompts.
        let fresh = session();
        let nav_gate = CapturingGate::answering(Approval::Deny);
        let nav = browser_gated_with_store(
            Arc::new(EchoInput("browser_act")),
            nav_gate.clone(),
            fresh,
            store.clone(),
        );
        assert!(nav
            .call(
                &ctx(),
                json!({"action": "navigate", "url": "https://other.test/"})
            )
            .await
            .is_err());
        assert_eq!(nav_gate.prompts(), 1);
    }

    #[tokio::test]
    async fn plain_always_from_a_scope_blind_ui_stays_global() {
        let store = SiteAuthStore::in_memory();
        let gate = CapturingGate::answering(Approval::AllowAlways);
        let s = session();
        {
            s.lease().await.unwrap();
        }
        let t = browser_gated_with_store(
            Arc::new(EchoInput("browser_act")),
            gate.clone(),
            s,
            store.clone(),
        );
        t.call(&ctx(), json!({"action": "click", "ref": 1}))
            .await
            .unwrap();
        assert!(t.is_always_allowed(), "unscoped always stays session-wide");
        assert!(store.all_grants().is_empty(), "nothing persisted");
    }

    #[tokio::test]
    async fn originless_call_offers_only_the_global_scope() {
        let store = SiteAuthStore::in_memory();
        let gate = CapturingGate::scoped(Approval::AllowAlways, AlwaysScope::Site, store.clone());
        // No backend booted: no page URL, and a non-navigate action has no
        // destination either.
        let t = browser_gated_with_store(
            Arc::new(EchoInput("browser_act")),
            gate.clone(),
            session(),
            store.clone(),
        );
        t.call(&ctx(), json!({"action": "scroll", "dy": 100}))
            .await
            .unwrap();
        let seen = gate.seen.lock().unwrap();
        assert!(seen[0].get("_origin").is_none());
        assert_eq!(seen[0]["_scope_options"], json!(["global"]));
        drop(seen);
        // A "site" answer with no site falls back to the global flag
        // rather than silently allowing nothing.
        assert!(t.is_always_allowed());
        assert!(store.all_grants().is_empty());
    }

    #[test]
    fn origin_for_call_prefers_the_current_page() {
        let nav = json!({"action": "navigate", "url": "https://dest.test/x"});
        assert_eq!(
            call_origin(&nav, Some("https://page.test/a"))
                .unwrap()
                .as_str(),
            "https://page.test"
        );
        // Nothing loaded yet: navigate keys on its destination.
        assert_eq!(
            call_origin(&nav, None).unwrap().as_str(),
            "https://dest.test"
        );
        // about:blank has no origin, so the destination wins again.
        assert_eq!(
            call_origin(&nav, Some("about:blank")).unwrap().as_str(),
            "https://dest.test"
        );
        // A new tab pointed somewhere counts as a destination too.
        assert_eq!(
            call_origin(&json!({"action": "new", "url": "https://tab.test/"}), None)
                .unwrap()
                .as_str(),
            "https://tab.test"
        );
        // Non-navigating action with no page: no origin at all.
        assert!(call_origin(&json!({"action": "click", "ref": 1}), None).is_none());
    }
}
