//! Permission gate decorator. Wraps a tool so each call is checked against
//! an `ApprovalGate` before delegating. "Allow always" is cached for the
//! lifetime of this wrapper (the session). See master plan §4.6① for why
//! gating lives here, not in QueryLoop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;

use crate::approval::{Approval, ApprovalGate};

#[derive(Debug)]
pub struct PermissionGatedTool {
    inner: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    always: AtomicBool,
}

impl PermissionGatedTool {
    pub fn new(inner: Arc<dyn Tool>, gate: Arc<dyn ApprovalGate>) -> Self {
        Self {
            inner,
            gate,
            always: AtomicBool::new(false),
        }
    }

    pub fn is_always_allowed(&self) -> bool {
        self.always.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl Tool for PermissionGatedTool {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn description(&self) -> &str {
        self.inner.description()
    }
    fn input_schema(&self) -> serde_json::Value {
        self.inner.input_schema()
    }
    fn safety_class(&self) -> SafetyClass {
        self.inner.safety_class()
    }

    async fn call(
        &self,
        ctx: &ToolUseContext,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, AgentError> {
        if !self.always.load(Ordering::Relaxed) {
            match self.gate.approve(self.inner.name(), &input).await {
                Approval::AllowOnce => {}
                Approval::AllowAlways => self.always.store(true, Ordering::Relaxed),
                Approval::Deny => {
                    return Err(AgentError::other(format!(
                        "Tool '{}' denied by user",
                        self.inner.name()
                    )));
                }
            }
        }
        self.inner.call(ctx, input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::tool::{SafetyClass, Tool, ToolUseContext};
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Arc;

    #[derive(Debug)]
    struct EchoTool;
    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "Echo"
        }
        fn description(&self) -> &str {
            "echo"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object"})
        }
        async fn call(
            &self,
            _c: &ToolUseContext,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, agent::error::AgentError> {
            Ok(input)
        }
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::Mutating
        }
    }

    #[derive(Debug)]
    struct FixedGate(crate::approval::Approval);
    #[async_trait]
    impl crate::approval::ApprovalGate for FixedGate {
        async fn approve(&self, _t: &str, _i: &serde_json::Value) -> crate::approval::Approval {
            self.0
        }
    }

    fn ctx() -> ToolUseContext {
        ToolUseContext::new(std::env::temp_dir())
    }

    #[tokio::test]
    async fn allow_once_delegates_but_does_not_cache() {
        use crate::approval::Approval;
        let gate = Arc::new(FixedGate(Approval::AllowOnce));
        let gated = PermissionGatedTool::new(Arc::new(EchoTool), gate);
        let r = gated.call(&ctx(), json!({"x": 1})).await.unwrap();
        assert_eq!(r, json!({"x": 1}));
        assert!(!gated.is_always_allowed());
    }

    #[tokio::test]
    async fn allow_always_caches() {
        use crate::approval::Approval;
        let gate = Arc::new(FixedGate(Approval::AllowAlways));
        let gated = PermissionGatedTool::new(Arc::new(EchoTool), gate);
        gated.call(&ctx(), json!({})).await.unwrap();
        assert!(gated.is_always_allowed());
    }

    #[tokio::test]
    async fn deny_returns_err() {
        use crate::approval::Approval;
        let gate = Arc::new(FixedGate(Approval::Deny));
        let gated = PermissionGatedTool::new(Arc::new(EchoTool), gate);
        let r = gated.call(&ctx(), json!({})).await;
        assert!(r.is_err());
    }
}
