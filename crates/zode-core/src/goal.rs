//! Goal auto-loop support: the `GoalComplete` tool and its shared signal.
//!
//! When the user sets a persistent goal (`/goal <text>`), the TUI runs agent
//! turns autonomously until the agent decides the goal is fully achieved. The
//! agent signals completion by calling the `GoalComplete` tool, which flips a
//! shared [`AtomicBool`] the TUI polls after each turn to stop the loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::verification::VerificationState;

/// The `GoalComplete` tool. Read-only in effect: it only flips a flag the host
/// reads to end the autonomous goal loop — it mutates nothing outside the agent.
#[derive(Debug)]
pub struct GoalCompleteTool {
    /// Shared with the owning [`ZodeEngine`](crate::engine::ZodeEngine); the
    /// same `Arc` is handed to the engine so the host can poll it after a turn.
    completed: Arc<AtomicBool>,
    verification: VerificationState,
    /// Whether a fresh passing `RunCheck` is required before this tool will
    /// succeed. `RunCheck` is `Mutating`, so plan-mode / read-only sessions
    /// never register it — requiring its evidence there would make
    /// `GoalComplete` permanently unreachable. Callers in that shape of
    /// session pass `false` so the tool can still legitimately end the loop.
    evidence_required: bool,
}

impl GoalCompleteTool {
    pub fn new(
        completed: Arc<AtomicBool>,
        verification: VerificationState,
        evidence_required: bool,
    ) -> Self {
        Self {
            completed,
            verification,
            evidence_required,
        }
    }
}

#[async_trait]
impl Tool for GoalCompleteTool {
    fn name(&self) -> &str {
        "GoalComplete"
    }

    fn description(&self) -> &str {
        if self.evidence_required {
            "Call this ONLY when the current goal is fully achieved; it ends the \
             autonomous goal loop. Provide a short summary of what was accomplished. \
             Do not call it prematurely — if more work remains, keep going instead. \
             A fresh passing RunCheck is required before this tool will succeed."
        } else {
            "Call this ONLY when the current goal is fully achieved; it ends the \
             autonomous goal loop. Provide a short summary of what was accomplished. \
             Do not call it prematurely — if more work remains, keep going instead. \
             A fresh passing RunCheck is required before this tool will succeed when \
             verification tooling is available in this session."
        }
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "A short summary of how the goal was achieved."
                }
            },
            "required": ["summary"]
        })
    }

    fn safety_class(&self) -> SafetyClass {
        // Signalling only — no side effects outside the agent, so it never needs
        // to pass through the approval gate.
        SafetyClass::ReadOnly
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let summary = input
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if !self.evidence_required {
            self.completed.store(true, Ordering::SeqCst);
            return Ok(json!({
                "ok": true,
                "summary": summary,
                "verification": {
                    "tool": Value::Null,
                    "command": Value::Null,
                },
                "note": "goal marked complete; the autonomous goal loop will stop \
                         (verification tooling unavailable in this session; \
                         completed without RunCheck evidence)"
            }));
        }
        let evidence = self
            .verification
            .completion_evidence()
            .map_err(AgentError::other)?;
        self.completed.store(true, Ordering::SeqCst);
        Ok(json!({
            "ok": true,
            "summary": summary,
            "verification": {
                "tool": evidence.tool,
                "command": evidence.command,
            },
            "note": "goal marked complete; the autonomous goal loop will stop"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata_is_stable() {
        let flag = Arc::new(AtomicBool::new(false));
        let tool = GoalCompleteTool::new(flag, VerificationState::default(), true);
        assert_eq!(tool.name(), "GoalComplete");
        assert_eq!(tool.safety_class(), SafetyClass::ReadOnly);
        let schema = tool.input_schema();
        assert_eq!(schema["required"][0], "summary");
    }

    #[tokio::test]
    async fn call_sets_the_shared_flag_only_after_a_fresh_passed_verification() {
        let verification = crate::verification::VerificationState::default();
        let flag = Arc::new(AtomicBool::new(false));
        let tool = GoalCompleteTool::new(flag.clone(), verification.clone(), true);
        assert!(!flag.load(Ordering::SeqCst));
        let ctx = ToolUseContext::new(".");
        let premature = tool
            .call(&ctx, json!({ "summary": "did the thing" }))
            .await
            .expect_err("GoalComplete must reject completion before verification");
        assert!(premature.to_string().contains("RunCheck"));
        assert!(!flag.load(Ordering::SeqCst));

        verification.record_pass("RunCheck", "cargo test");
        let out = tool
            .call(&ctx, json!({ "summary": "did the thing" }))
            .await
            .expect("GoalComplete should succeed");
        assert!(flag.load(Ordering::SeqCst), "flag must be set after call");
        assert_eq!(out["ok"], true);
        assert_eq!(out["summary"], "did the thing");
    }

    #[tokio::test]
    async fn call_completes_without_evidence_when_not_required() {
        let verification = crate::verification::VerificationState::default();
        let flag = Arc::new(AtomicBool::new(false));
        let tool = GoalCompleteTool::new(flag.clone(), verification, false);
        assert!(!flag.load(Ordering::SeqCst));
        let ctx = ToolUseContext::new(".");
        let out = tool
            .call(&ctx, json!({ "summary": "did the thing" }))
            .await
            .expect("GoalComplete should succeed without RunCheck evidence");
        assert!(flag.load(Ordering::SeqCst), "flag must be set after call");
        assert_eq!(out["ok"], true);
        assert_eq!(out["summary"], "did the thing");
        assert!(out["verification"]["tool"].is_null());
        assert!(out["verification"]["command"].is_null());
    }
}
