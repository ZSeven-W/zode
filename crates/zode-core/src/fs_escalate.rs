//! Escalation for sandbox-blocked FILE mutations.
//!
//! `SandboxedBashTool` lets a blocked shell command ask the user to re-run
//! outside the sandbox. The file tools (`FileWrite`, `FileEdit`, …) had no such
//! path: a write outside the workspace just failed, so the model would retry
//! the same write, then improvise a `Bash` heredoc with `require_escalated` to
//! work around it. That is both noisy and backwards — the shell became the
//! privileged path for something the file tool should ask about directly.
//!
//! This decorator closes the gap: when a mutation is blocked BY THE SANDBOX,
//! the user is asked ONCE whether to perform it outside the workspace
//! confinement. On consent the very same call is replayed against an
//! unconfined tool (plain `WorkspacePolicy` + direct fs sink). On refusal —
//! or with a non-interactive gate (yolo / headless bypass), where there is
//! nobody to ask — the original error stands.
//!
//! Layering: this wraps the RAW tool, and `wrap_mutating_tools` then wraps
//! this. So the user first approves the write itself, and only a
//! sandbox-blocked write raises the second, distinct "escalate?" question.

use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolRegistry, ToolUseContext};
use async_trait::async_trait;

use crate::approval::{Approval, ApprovalGate};

/// Mutating file tools from `agent_tools_code::register_default`. Read tools
/// are never confined by the write policy, so they need no escalation.
pub const MUTATING_FS_TOOLS: &[&str] = &[
    "FileWrite",
    "FileEdit",
    "MultiEdit",
    "Mkdir",
    "Move",
    "Remove",
    "NotebookEdit",
];

/// Did this error come from the sandbox refusing the write (as opposed to a
/// missing parent dir, a too-large payload, a bad path)? Only those are worth
/// offering an escape for.
///
/// Two shapes, one per enforcement layer:
/// - `policy: path '…' is outside the configured workspace roots` — the
///   in-process `WorkspacePolicy` check (also what read-only mode produces,
///   since it clears every writable root);
/// - `… sandboxed fs op failed …` — the kernel denying the sandboxed sink's
///   `cat`/`mkdir`/`mv`/`rm`.
///
/// `.git` / `.zode` (`PolicyError::Protected`) are deliberately NOT
/// escalatable: they exist so a sandboxed agent cannot rewrite git history or
/// edit its own `.zode/state.json` to widen its permissions. Offering a
/// one-click escape from that guard would defeat it. Shell (`Bash`) remains
/// available for a user who genuinely means to touch those.
pub fn is_escalatable_denial(err: &AgentError) -> bool {
    let text = err.to_string();
    if text.contains("protected, read-only location") {
        return false;
    }
    text.contains("outside the configured workspace roots")
        || text.contains("sandboxed fs op failed")
}

/// Wraps one mutating file tool: on a sandbox denial, ask the user, and on
/// consent replay the call against the unconfined twin.
#[derive(Debug)]
pub struct EscalatingFsTool {
    /// Confined tool (workspace policy + sandboxed fs sink).
    inner: Arc<dyn Tool>,
    /// Same tool built on an unconfined policy + direct fs sink.
    escalated: Arc<dyn Tool>,
    gate: Arc<dyn ApprovalGate>,
    /// Inner description + the no-retry note, built once (`description`
    /// returns `&str`).
    description: String,
}

impl EscalatingFsTool {
    pub fn new(
        inner: Arc<dyn Tool>,
        escalated: Arc<dyn Tool>,
        gate: Arc<dyn ApprovalGate>,
    ) -> Self {
        let description = format!(
            "{}\n\nWrites are confined to the sandbox workspace. If this call is \
             blocked by the sandbox, zode asks the user whether to perform it \
             outside the workspace and, on consent, completes it for you. Do NOT \
             retry the identical call, and do NOT work around the block with a \
             shell command — a refusal means the user declined, so report that \
             instead.",
            inner.description()
        );
        Self {
            inner,
            escalated,
            gate,
            description,
        }
    }
}

#[async_trait]
impl Tool for EscalatingFsTool {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn description(&self) -> &str {
        &self.description
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
        let err = match self.inner.call(ctx, input.clone()).await {
            Ok(v) => return Ok(v),
            Err(e) => e,
        };
        // Only a sandbox denial is escalatable, and only a human can authorize
        // it — an auto-answering gate (yolo / bypass) must never "consent" on
        // the user's behalf to leaving the workspace.
        if !is_escalatable_denial(&err) || !self.gate.interactive() {
            return Err(err);
        }
        let label = format!(
            "{} — escalate: perform this write OUTSIDE the sandbox workspace",
            self.inner.name()
        );
        match self.gate.approve(&label, &input).await {
            Approval::AllowOnce | Approval::AllowAlways => self.escalated.call(ctx, input).await,
            Approval::Deny => Err(err),
        }
    }
}

/// Re-register `src`, wrapping each mutating file tool with its unconfined
/// twin from `escalated`. Tools absent from `escalated` pass through unchanged.
pub fn apply_fs_escalation(
    src: ToolRegistry,
    escalated: &ToolRegistry,
    gate: &Arc<dyn ApprovalGate>,
) -> ToolRegistry {
    let mut out = ToolRegistry::new();
    for tool in src.list() {
        let name = tool.name().to_string();
        match escalated.get(&name) {
            Some(twin) if MUTATING_FS_TOOLS.contains(&name.as_str()) => {
                out.register(Arc::new(EscalatingFsTool::new(tool, twin, gate.clone())));
            }
            _ => {
                out.register(tool);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy_err() -> AgentError {
        AgentError::other("policy: path '/etc/x' is outside the configured workspace roots")
    }

    #[test]
    fn escalatable_only_for_sandbox_denials() {
        assert!(is_escalatable_denial(&policy_err()));
        assert!(is_escalatable_denial(&AgentError::other(
            "write '/x' failed: sandboxed fs op failed (exit status: 1): Operation not permitted"
        )));
        // Not a sandbox denial — a plain IO / usage error must not offer an escape.
        assert!(!is_escalatable_denial(&AgentError::other(
            "write '/x' failed: No such file or directory"
        )));
        assert!(!is_escalatable_denial(&AgentError::other(
            "payload too large: 99 bytes (limit 8)"
        )));
        // The .git / .zode guard is NOT escalatable (self-escalation defense).
        assert!(!is_escalatable_denial(&AgentError::other(
            "policy: path '/w/.zode/state.json' is a protected, read-only location"
        )));
    }

    /// Inner tool: always fails with `err`. Counts calls.
    #[derive(Debug)]
    struct FailingTool {
        err: fn() -> AgentError,
        calls: std::sync::Mutex<usize>,
    }
    impl FailingTool {
        fn new(err: fn() -> AgentError) -> Arc<Self> {
            Arc::new(Self {
                err,
                calls: std::sync::Mutex::new(0),
            })
        }
    }
    #[async_trait]
    impl Tool for FailingTool {
        fn name(&self) -> &str {
            "FileWrite"
        }
        fn description(&self) -> &str {
            "write a file"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {"path": {"type": "string"}}})
        }
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::Mutating
        }
        async fn call(
            &self,
            _ctx: &ToolUseContext,
            _input: serde_json::Value,
        ) -> Result<serde_json::Value, AgentError> {
            *self.calls.lock().unwrap() += 1;
            Err((self.err)())
        }
    }

    /// Escalated twin: always succeeds, records the input it replayed.
    #[derive(Debug, Default)]
    struct SucceedingTool {
        seen: std::sync::Mutex<Option<serde_json::Value>>,
    }
    #[async_trait]
    impl Tool for SucceedingTool {
        fn name(&self) -> &str {
            "FileWrite"
        }
        fn description(&self) -> &str {
            "write a file"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object"})
        }
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::Mutating
        }
        async fn call(
            &self,
            _ctx: &ToolUseContext,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, AgentError> {
            *self.seen.lock().unwrap() = Some(input);
            Ok(json!({"ok": true}))
        }
    }

    #[derive(Debug)]
    struct PromptGate {
        answer: Approval,
        asked: std::sync::Mutex<Vec<String>>,
    }
    impl PromptGate {
        fn new(answer: Approval) -> Arc<Self> {
            Arc::new(Self {
                answer,
                asked: std::sync::Mutex::new(Vec::new()),
            })
        }
    }
    #[async_trait]
    impl ApprovalGate for PromptGate {
        fn interactive(&self) -> bool {
            true
        }
        async fn approve(&self, tool: &str, _input: &serde_json::Value) -> Approval {
            self.asked.lock().unwrap().push(tool.to_string());
            self.answer
        }
    }

    fn ctx() -> ToolUseContext {
        ToolUseContext::new(std::env::temp_dir())
    }

    #[tokio::test]
    async fn blocked_write_asks_once_then_replays_unconfined() {
        let inner = FailingTool::new(policy_err);
        let esc = Arc::new(SucceedingTool::default());
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = EscalatingFsTool::new(inner.clone(), esc.clone(), gate.clone());

        let input = json!({"path": "/etc/x", "content": "hi"});
        let out = tool.call(&ctx(), input.clone()).await.unwrap();

        assert_eq!(out["ok"], true, "escalated write succeeded");
        assert_eq!(*inner.calls.lock().unwrap(), 1, "inner tried exactly once");
        assert_eq!(
            gate.asked.lock().unwrap().len(),
            1,
            "user asked exactly once"
        );
        assert!(gate.asked.lock().unwrap()[0].contains("FileWrite"));
        assert_eq!(
            esc.seen.lock().unwrap().clone().unwrap(),
            input,
            "the escalated twin replays the ORIGINAL input verbatim"
        );
    }

    #[tokio::test]
    async fn user_refusal_surfaces_the_original_error() {
        let inner = FailingTool::new(policy_err);
        let esc = Arc::new(SucceedingTool::default());
        let gate = PromptGate::new(Approval::Deny);
        let tool = EscalatingFsTool::new(inner, esc.clone(), gate);

        let err = tool
            .call(&ctx(), json!({"path": "/etc/x"}))
            .await
            .expect_err("refusal must not silently succeed");
        assert!(err.to_string().contains("outside the configured workspace"));
        assert!(
            esc.seen.lock().unwrap().is_none(),
            "the unconfined twin must never run without consent"
        );
    }

    #[tokio::test]
    async fn non_interactive_gate_never_escalates() {
        // yolo / headless bypass: nobody to ask, so the write stays blocked.
        let inner = FailingTool::new(policy_err);
        let esc = Arc::new(SucceedingTool::default());
        let tool = EscalatingFsTool::new(inner, esc.clone(), Arc::new(crate::approval::BypassGate));

        let err = tool
            .call(&ctx(), json!({"path": "/etc/x"}))
            .await
            .expect_err("a bypass gate must not auto-consent to leaving the workspace");
        assert!(err.to_string().contains("outside the configured workspace"));
        assert!(esc.seen.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn protected_paths_are_never_escalatable() {
        let inner = FailingTool::new(|| {
            AgentError::other(
                "policy: path '/w/.zode/state.json' is a protected, read-only location",
            )
        });
        let esc = Arc::new(SucceedingTool::default());
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = EscalatingFsTool::new(inner, esc.clone(), gate.clone());

        tool.call(&ctx(), json!({"path": "/w/.zode/state.json"}))
            .await
            .expect_err(".git/.zode must stay protected");
        assert!(
            gate.asked.lock().unwrap().is_empty(),
            "no escape offered for the self-escalation guard"
        );
        assert!(esc.seen.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn non_sandbox_errors_do_not_prompt() {
        let inner = FailingTool::new(|| AgentError::other("write '/x' failed: Is a directory"));
        let esc = Arc::new(SucceedingTool::default());
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = EscalatingFsTool::new(inner, esc, gate.clone());

        tool.call(&ctx(), json!({"path": "/x"})).await.unwrap_err();
        assert!(
            gate.asked.lock().unwrap().is_empty(),
            "a plain IO failure must not raise an escalation prompt"
        );
    }

    /// End-to-end against the REAL FileWrite tool + a real OS sandbox: a write
    /// outside the workspace is blocked by the sandboxed policy, the user is
    /// asked once, and on consent the file actually lands on disk.
    #[tokio::test]
    async fn real_filewrite_outside_workspace_escalates_and_writes() {
        use agent::tool::ToolRegistry;

        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let Ok(sb) = crate::sandbox::SandboxConfig::new(
            workspace.path(),
            crate::sandbox::SandboxMode::WorkspaceWrite,
            false,
            &[],
        ) else {
            return; // unsupported OS
        };
        // The escalation target must be outside BOTH the workspace and /tmp-ish
        // writable roots, or the write would have been allowed anyway.
        let target = outside.path().join("escalated.txt");
        if sb.writable_roots().iter().any(|r| target.starts_with(r))
            || target.starts_with("/tmp")
            || target.starts_with("/private/tmp")
        {
            return; // tempdir landed inside a writable root — nothing to prove
        }

        let confined = crate::engine::build_workspace_policy_for_test(workspace.path(), &Some(sb))
            .unwrap()
            .into_arc();
        let unconfined = crate::engine::build_workspace_policy_for_test(workspace.path(), &None)
            .unwrap()
            .into_arc();
        let mut base = ToolRegistry::new();
        agent_tools_code::register_default(&mut base, confined);
        let mut esc = ToolRegistry::new();
        agent_tools_code::register_default(&mut esc, unconfined);

        let gate = PromptGate::new(Approval::AllowOnce);
        let gate_dyn: Arc<dyn ApprovalGate> = gate.clone();
        let out = apply_fs_escalation(base, &esc, &gate_dyn);
        let file_write = out.get("FileWrite").unwrap();

        file_write
            .call(
                &ctx(),
                json!({"path": target.display().to_string(), "content": "escalated!"}),
            )
            .await
            .expect("consent must complete the blocked write");

        assert_eq!(gate.asked.lock().unwrap().len(), 1, "asked exactly once");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "escalated!",
            "the file really landed outside the workspace"
        );
    }

    #[test]
    fn apply_wraps_only_mutating_fs_tools() {
        let gate: Arc<dyn ApprovalGate> = Arc::new(crate::approval::BypassGate);
        let mut base = ToolRegistry::new();
        base.register(FailingTool::new(policy_err)); // named FileWrite
        let mut esc = ToolRegistry::new();
        esc.register(Arc::new(SucceedingTool::default()));

        let out = apply_fs_escalation(base, &esc, &gate);
        let tool = out.get("FileWrite").expect("FileWrite still registered");
        assert!(
            tool.description().contains("Do NOT"),
            "wrapped tool carries the no-retry guidance"
        );
    }
}
