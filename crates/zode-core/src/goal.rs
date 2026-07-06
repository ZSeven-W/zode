//! Goal auto-loop support: the `goal_complete` tool and its shared signal.
//!
//! When the user sets a persistent goal (`/goal <text>`), the TUI runs agent
//! turns autonomously until the agent decides the goal is fully achieved. The
//! agent signals completion by calling the `goal_complete` tool, which flips a
//! shared [`AtomicBool`] the TUI polls after each turn to stop the loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::verification::VerificationState;

/// The `goal_complete` tool. Read-only in effect: it only flips a flag the host
/// reads to end the autonomous goal loop — it mutates nothing outside the agent.
#[derive(Debug)]
pub struct GoalCompleteTool {
    /// Shared with the owning [`ZodeEngine`](crate::engine::ZodeEngine); the
    /// same `Arc` is handed to the engine so the host can poll it after a turn.
    completed: Arc<AtomicBool>,
    verification: VerificationState,
}

impl GoalCompleteTool {
    pub fn new(completed: Arc<AtomicBool>, verification: VerificationState) -> Self {
        Self {
            completed,
            verification,
        }
    }
}

#[async_trait]
impl Tool for GoalCompleteTool {
    fn name(&self) -> &str {
        "goal_complete"
    }

    fn description(&self) -> &str {
        "Call this ONLY when the current goal is fully achieved; it ends the \
         autonomous goal loop. Provide a short summary of what was accomplished. \
         Do not call it prematurely — if more work remains, keep going instead. \
         A fresh passing run_check is required before this tool will succeed."
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
        let tool = GoalCompleteTool::new(flag, VerificationState::default());
        assert_eq!(tool.name(), "goal_complete");
        assert_eq!(tool.safety_class(), SafetyClass::ReadOnly);
        let schema = tool.input_schema();
        assert_eq!(schema["required"][0], "summary");
    }

    #[tokio::test]
    async fn call_sets_the_shared_flag_only_after_a_fresh_passed_verification() {
        let verification = crate::verification::VerificationState::default();
        let flag = Arc::new(AtomicBool::new(false));
        let tool = GoalCompleteTool::new(flag.clone(), verification.clone());
        assert!(!flag.load(Ordering::SeqCst));
        let ctx = ToolUseContext::new(".");
        let premature = tool
            .call(&ctx, json!({ "summary": "did the thing" }))
            .await
            .expect_err("goal_complete must reject completion before verification");
        assert!(premature.to_string().contains("run_check"));
        assert!(!flag.load(Ordering::SeqCst));

        verification.record_pass("run_check", "cargo test");
        let out = tool
            .call(&ctx, json!({ "summary": "did the thing" }))
            .await
            .expect("goal_complete should succeed");
        assert!(flag.load(Ordering::SeqCst), "flag must be set after call");
        assert_eq!(out["ok"], true);
        assert_eq!(out["summary"], "did the thing");
    }
}
