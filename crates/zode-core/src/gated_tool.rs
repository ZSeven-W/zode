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

/// Optional hook that lets a decorator show the approval gate a different
/// (usually context-enriched) copy of the input than what the inner tool
/// actually receives. The inner tool always gets the original, unmodified
/// input — only the human-facing prompt sees the `view()` output.
#[async_trait]
pub trait GateView: Send + Sync + std::fmt::Debug {
    /// Produce the input variant shown to the approval gate. The inner
    /// tool always receives the original input.
    async fn view(&self, input: &serde_json::Value) -> serde_json::Value;
}

#[derive(Debug)]
pub struct PermissionGatedTool {
    inner: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    always: Arc<AtomicBool>,
    /// When `false`, an `AllowAlways` answer is honored for THAT call only
    /// (never stored). Set for tools the user force-listed in
    /// `permissions.ask`: they must prompt on every call, and stacking a
    /// second context-blind gate outside a context-aware one (the browser /
    /// desktop pre-wrapped tools) is not an acceptable way to get that.
    persist_always: AtomicBool,
    /// Serializes the check-approve-store sequence so concurrent calls to
    /// the same tool in one turn don't double-prompt; the second waiter
    /// re-checks `always` inside the lock and skips the prompt.
    approve_lock: tokio::sync::Mutex<()>,
    /// Optional context-injection hook for the approval prompt only (see
    /// [`GateView`]). `None` for the plain `new()` path — identical
    /// behavior to before this hook existed.
    view: Option<Arc<dyn GateView>>,
}

impl PermissionGatedTool {
    pub fn new(inner: Arc<dyn Tool>, gate: Arc<dyn ApprovalGate>) -> Self {
        Self {
            inner,
            gate,
            always: Arc::new(AtomicBool::new(false)),
            persist_always: AtomicBool::new(true),
            approve_lock: tokio::sync::Mutex::new(()),
            view: None,
        }
    }

    /// Like [`Self::new`], but the approval gate is shown `view.view(input)`
    /// instead of the raw input. The inner tool still receives the raw input.
    pub fn with_view(
        inner: Arc<dyn Tool>,
        gate: Arc<dyn ApprovalGate>,
        view: Arc<dyn GateView>,
    ) -> Self {
        Self {
            inner,
            gate,
            always: Arc::new(AtomicBool::new(false)),
            persist_always: AtomicBool::new(true),
            approve_lock: tokio::sync::Mutex::new(()),
            view: Some(view),
        }
    }

    /// Force a prompt on EVERY call: an `AllowAlways` answer is applied to
    /// that call only and never cached (`permissions.ask` semantics for
    /// pre-wrapped, context-aware gates).
    pub fn set_always_persist(&self, persist: bool) {
        self.persist_always.store(persist, Ordering::Relaxed);
    }

    pub fn is_always_allowed(&self) -> bool {
        self.always.load(Ordering::Relaxed)
    }

    /// Clone of the "allow always" flag, for callers (e.g. browser tool
    /// assembly) that need to register it in an external flag registry.
    pub fn always_flag(&self) -> Arc<AtomicBool> {
        self.always.clone()
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
            // Serialize per tool; re-check inside the lock so a concurrent
            // call that already selected "always" doesn't re-prompt.
            let _guard = self.approve_lock.lock().await;
            if !self.always.load(Ordering::Relaxed) {
                let shown = match &self.view {
                    Some(v) => v.view(&input).await,
                    None => input.clone(),
                };
                match self.gate.approve(self.inner.name(), &shown).await {
                    Approval::AllowOnce => {}
                    Approval::AllowAlways => {
                        if self.persist_always.load(Ordering::Relaxed) {
                            self.always.store(true, Ordering::Relaxed);
                        }
                    }
                    Approval::Deny => {
                        return Err(AgentError::other(format!(
                            "Tool '{}' denied by user",
                            self.inner.name()
                        )));
                    }
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
