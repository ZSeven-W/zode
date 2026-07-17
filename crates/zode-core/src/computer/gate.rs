//! Computer-use-aware gate view: approval prompts for `computer_act` carry
//! the target app and a description of the resolved element, both derived
//! by the backend — never trusted from the model's own tool-call input
//! (mirrors `BrowserGateView`, see `browser/gate.rs`).

use std::sync::Arc;

use agent::tool::Tool;
use async_trait::async_trait;

use crate::approval::ApprovalGate;
use crate::gated_tool::{GateView, PermissionGatedTool};

use super::session::ComputerSession;
use super::tools::{parse_generation, parse_target_opt};

#[derive(Debug)]
pub struct ComputerGateView {
    session: Arc<ComputerSession>,
}

#[async_trait]
impl GateView for ComputerGateView {
    async fn view(&self, input: &serde_json::Value) -> serde_json::Value {
        let mut shown = input.clone();
        if let Some(obj) = shown.as_object_mut() {
            let backend = self.session.backend();
            if let Some(app) = backend.frontmost_app_name().await {
                obj.insert("_app".into(), serde_json::json!(app));
            }
            if let Ok(generation) = parse_generation(input) {
                // click/set_value use the bare prefix; drag uses "from_" —
                // show whichever resolves so the human sees what's about to
                // be acted on, preferring the primary target.
                let target =
                    parse_target_opt(input, "").or_else(|| parse_target_opt(input, "from_"));
                if let Some(target) = target {
                    if let Some(desc) = backend.describe_target(generation, &target).await {
                        obj.insert("_element".into(), serde_json::json!(desc));
                    }
                }
            }
        }
        shown
    }
}

/// Wrap `inner` in a [`PermissionGatedTool`] whose approval prompts are
/// enriched with the resolved app/element (see [`ComputerGateView`]), and
/// register its "allow always" flag with the session's perm-flag registry.
pub fn computer_gated(
    inner: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    session: Arc<ComputerSession>,
) -> Arc<PermissionGatedTool> {
    let name = inner.name().to_string();
    let view = Arc::new(ComputerGateView {
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
    use crate::computer::backend::mock::MockBackend;
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
            "computer_act"
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

    fn session() -> Arc<ComputerSession> {
        ComputerSession::new(Arc::new(MockBackend::default()))
    }

    #[tokio::test]
    async fn gate_sees_app_and_element_inner_does_not() {
        let gate = Arc::new(CapturingGate {
            seen: Default::default(),
            answer: Approval::AllowOnce,
        });
        let s = session();
        // Establish generation 1 so the element ref resolves.
        s.backend().app_state(None).await.unwrap();
        let t = computer_gated(Arc::new(EchoInput), gate.clone(), s);
        let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());
        let input = json!({"action": "click", "element": 1, "generation": 1});
        let out = t.call(&ctx, input.clone()).await.unwrap();
        assert_eq!(out, input); // untouched
        let seen = gate.seen.lock().unwrap();
        assert_eq!(seen[0]["_app"], "TestApp");
        assert_eq!(seen[0]["_element"], "role=AXButton label=\"OK\"");
    }

    #[tokio::test]
    async fn always_allow_skips_second_prompt_and_registers_flag() {
        let gate = Arc::new(CapturingGate {
            seen: Default::default(),
            answer: Approval::AllowAlways,
        });
        let s = session();
        let t = computer_gated(Arc::new(EchoInput), gate.clone(), s.clone());
        let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());
        t.call(&ctx, json!({})).await.unwrap();
        t.call(&ctx, json!({})).await.unwrap();
        assert_eq!(gate.seen.lock().unwrap().len(), 1);
        let flags = s.perm_flags();
        assert_eq!(flags[0].0, "computer_act");
        assert!(flags[0].1.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[tokio::test]
    async fn deny_is_an_error() {
        let gate = Arc::new(CapturingGate {
            seen: Default::default(),
            answer: Approval::Deny,
        });
        let t = computer_gated(Arc::new(EchoInput), gate, session());
        let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());
        let err = t.call(&ctx, json!({})).await.unwrap_err();
        assert!(err.to_string().contains("denied"));
    }
}
