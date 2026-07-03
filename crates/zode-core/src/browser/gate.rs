//! Browser-aware gate view: approval prompts for browser tools carry
//! the live target and page URL, which are session state the model's
//! input cannot be trusted to report.

use std::sync::Arc;

use agent::tool::Tool;
use async_trait::async_trait;

use crate::approval::ApprovalGate;
use crate::gated_tool::{GateView, PermissionGatedTool};

use super::backend::BrowserTarget;
use super::session::BrowserSession;

#[derive(Debug)]
pub struct BrowserGateView {
    session: Arc<BrowserSession>,
}

#[async_trait]
impl GateView for BrowserGateView {
    async fn view(&self, input: &serde_json::Value) -> serde_json::Value {
        let mut shown = input.clone();
        if let Some(obj) = shown.as_object_mut() {
            obj.insert(
                "_target".into(),
                serde_json::json!(match self.session.target() {
                    BrowserTarget::Managed => "managed",
                    BrowserTarget::Bridge => "bridge",
                }),
            );
            if let Some(url) = self.session.current_url_hint().await {
                obj.insert("_page_url".into(), serde_json::json!(url));
            }
        }
        shown
    }
}

/// Wrap `inner` in a [`PermissionGatedTool`] whose approval prompts are
/// enriched with the session's live target/URL (see [`BrowserGateView`]),
/// and register its "allow always" flag with the session's perm-flag
/// registry so `/browser` status can report it.
pub fn browser_gated(
    inner: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    session: Arc<BrowserSession>,
) -> Arc<PermissionGatedTool> {
    let name = inner.name().to_string();
    let view = Arc::new(BrowserGateView {
        session: session.clone(),
    });
    let gated = Arc::new(PermissionGatedTool::with_view(inner, gate, view));
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
    }
    #[async_trait::async_trait]
    impl ApprovalGate for CapturingGate {
        async fn approve(&self, _tool: &str, input: &serde_json::Value) -> Approval {
            self.seen.lock().unwrap().push(input.clone());
            self.answer
        }
    }

    #[derive(Debug)]
    struct EchoInput;
    #[async_trait::async_trait]
    impl agent::tool::Tool for EchoInput {
        fn name(&self) -> &str {
            "browser_act"
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

    #[tokio::test]
    async fn gate_sees_context_inner_does_not() {
        let gate = std::sync::Arc::new(CapturingGate {
            seen: Default::default(),
            answer: Approval::AllowOnce,
        });
        let s = session();
        {
            s.lease().await.unwrap();
        } // boot backend so url hint resolves
        let t = browser_gated(std::sync::Arc::new(EchoInput), gate.clone(), s);
        let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());
        let out = t
            .call(&ctx, json!({"action": "navigate", "url": "https://x.test"}))
            .await
            .unwrap();
        assert_eq!(out, json!({"action": "navigate", "url": "https://x.test"})); // untouched
        let seen = gate.seen.lock().unwrap();
        assert_eq!(seen[0]["_target"], "managed");
        assert_eq!(seen[0]["_page_url"], "https://example.test/");
    }

    #[tokio::test]
    async fn always_allow_skips_second_prompt_and_registers_flag() {
        let gate = std::sync::Arc::new(CapturingGate {
            seen: Default::default(),
            answer: Approval::AllowAlways,
        });
        let s = session();
        let t = browser_gated(std::sync::Arc::new(EchoInput), gate.clone(), s.clone());
        let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());
        t.call(&ctx, json!({})).await.unwrap();
        t.call(&ctx, json!({})).await.unwrap();
        assert_eq!(gate.seen.lock().unwrap().len(), 1);
        let flags = s.perm_flags();
        assert_eq!(flags[0].0, "browser_act");
        assert!(flags[0].1.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[tokio::test]
    async fn deny_is_an_error() {
        let gate = std::sync::Arc::new(CapturingGate {
            seen: Default::default(),
            answer: Approval::Deny,
        });
        let t = browser_gated(std::sync::Arc::new(EchoInput), gate, session());
        let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());
        let err = t.call(&ctx, json!({})).await.unwrap_err();
        assert!(err.to_string().contains("denied"));
    }
}
