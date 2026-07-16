//! Desktop-aware gate view + gating helper. The view enriches approval prompts
//! with session state the model's input can't be trusted to report, and does
//! so WITHOUT blocking on or booting a backend (spec: fail-closed, non-blocking).

use std::sync::Arc;

use agent::tool::Tool;
use async_trait::async_trait;

use crate::approval::ApprovalGate;
use crate::gated_tool::{GateView, PermissionGatedTool};

use super::session::DesktopSession;

#[derive(Debug)]
pub struct DesktopGateView {
    _session: Arc<DesktopSession>,
}

#[async_trait]
impl GateView for DesktopGateView {
    async fn view(&self, input: &serde_json::Value) -> serde_json::Value {
        let mut shown = input.clone();
        if let Some(obj) = shown.as_object_mut() {
            // Non-blocking, cache-only enrichment. M1 marks the backend family;
            // richer _app/_window_title come from cached descriptors in M2+.
            obj.insert("_backend".into(), serde_json::json!("desktop"));
        }
        shown
    }
}

/// Wrap a mutating desktop tool in a context-aware `PermissionGatedTool` and
/// register its always-allow flag with the session for `/desktop status`.
pub fn desktop_gated(
    inner: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    session: Arc<DesktopSession>,
) -> Arc<PermissionGatedTool> {
    let name = inner.name().to_string();
    let view = Arc::new(DesktopGateView {
        _session: session.clone(),
    });
    let gated = Arc::new(PermissionGatedTool::with_view(inner, gate, view));
    session.register_perm_flag(&name, gated.always_flag());
    gated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::{Approval, ApprovalGate};
    use crate::config::DesktopConfig;
    use serde_json::json;

    #[derive(Debug)]
    struct YesGate;
    #[async_trait::async_trait]
    impl ApprovalGate for YesGate {
        async fn approve(&self, _t: &str, input: &serde_json::Value) -> Approval {
            // gate view must have injected _backend without blocking
            assert_eq!(input["_backend"], "desktop");
            Approval::AllowOnce
        }
    }

    #[derive(Debug)]
    struct Echo;
    #[async_trait::async_trait]
    impl agent::tool::Tool for Echo {
        fn name(&self) -> &str {
            "desktop_act"
        }
        fn description(&self) -> &str {
            "t"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type":"object"})
        }
        fn safety_class(&self) -> agent::tool::SafetyClass {
            agent::tool::SafetyClass::Mutating
        }
        async fn call(
            &self,
            _c: &agent::tool::ToolUseContext,
            i: serde_json::Value,
        ) -> Result<serde_json::Value, agent::error::AgentError> {
            Ok(i)
        }
    }

    #[tokio::test]
    async fn gate_injects_backend_and_registers_flag() {
        let s = DesktopSession::new(
            DesktopConfig::default(),
            crate::desktop::mock::mock_factory(),
        );
        let t = desktop_gated(Arc::new(Echo), Arc::new(YesGate), s.clone());
        let ctx = agent::tool::ToolUseContext::new(std::env::temp_dir());
        let out = t.call(&ctx, json!({"action":"click"})).await.unwrap();
        assert_eq!(out, json!({"action":"click"})); // inner sees raw input
        assert_eq!(s.perm_flags()[0].0, "desktop_act");
    }
}
