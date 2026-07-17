use super::*;

/// Wraps Bash / BashRun: rewrites the `command` input to run inside the
/// sandbox before delegating. The tool name is preserved.
#[derive(Debug)]
pub struct SandboxedBashTool {
    inner: Arc<dyn Tool>,
    config: SandboxConfig,
    /// Inner description + a note about the sandbox and the escape flag, built
    /// once (the `Tool::description` signature returns `&str`).
    description: String,
    /// Approval gate used to ask the user whether to ESCALATE (re-run outside
    /// the sandbox) when a sandboxed command fails on a sandbox restriction.
    gate: Arc<dyn ApprovalGate>,
}

impl SandboxedBashTool {
    pub fn new(inner: Arc<dyn Tool>, config: SandboxConfig, gate: Arc<dyn ApprovalGate>) -> Self {
        let mode = if config.is_windows_tier_one() {
            if config.is_windows_tier_two() {
                super::windows_policy::tier_two_summary(config.mode == SandboxMode::ReadOnly)
            } else {
                super::windows_policy::tier_one_summary(config.mode == SandboxMode::ReadOnly)
            }
        } else {
            match config.mode {
                SandboxMode::ReadOnly => "read-only (no filesystem writes)".to_string(),
                SandboxMode::WorkspaceWrite => {
                    format!("writes confined to {}", config.write_scope_summary())
                }
            }
        };
        let net = if config.is_windows_tier_one() {
            if config.is_windows_tier_two() {
                "network denied (AppContainer; loopback included)"
            } else {
                "network unenforced"
            }
        } else if config.allow_network {
            "network allowed"
        } else {
            "network denied"
        };
        let description = format!(
            "{}\n\nThis command runs in an OS sandbox ({mode}; {net}). If it \
             genuinely needs network or to write outside the workspace, set \
             `{SANDBOX_PERMISSIONS_FLAG}: \"require_escalated\"` (with a short \
             `{JUSTIFICATION_FLAG}`) to request running it outside the sandbox — \
             the user is asked to authorize the escape. In a non-interactive \
             session (yolo / headless bypass) there is no one to ask, so the \
             request is not honored and the command runs sandboxed.",
            inner.description()
        );
        Self {
            inner,
            config,
            description,
            gate,
        }
    }
}

/// Heuristic: did a (failed) command result look like it was blocked by the
/// sandbox (write outside cwd, read-only fs, network denied)? Used to decide
/// whether to OFFER an escalation rather than prompt on every failure.
pub(super) fn looks_like_sandbox_denial(result: &serde_json::Value) -> bool {
    let exit_code = result.get("exit_code").and_then(|v| v.as_i64());
    // exit 0 → success, never a denial. `null` (killed by signal) stays a
    // candidate, matching Codex treating a missing/non-zero code as failed.
    if exit_code == Some(0) {
        return false;
    }
    // Quick reject the well-known shell / exec failure codes — they are never
    // caused by the sandbox, so they must not trigger an escape prompt (Codex
    // `QUICK_REJECT_EXIT_CODES`). 126 ("permission denied" running a
    // non-executable) would otherwise false-match the keyword below.
    //   2  → misuse of a shell builtin
    //   126 → command found but not executable
    //   127 → command not found
    if matches!(exit_code, Some(2) | Some(126) | Some(127)) {
        return false;
    }
    let mut text = String::new();
    for k in ["stderr", "stdout", "error"] {
        if let Some(s) = result.get(k).and_then(|v| v.as_str()) {
            text.push_str(&s.to_ascii_lowercase());
            text.push('\n');
        }
    }
    // Codex's keyword set (core/src/exec.rs `SANDBOX_DENIED_KEYWORDS`) plus the
    // network-denial signs that surface under bwrap's `--unshare-net` / a denied
    // seatbelt `network*` rule (Codex catches those via seccomp SIGSYS, which
    // zode's no-seccomp backends never emit).
    const SIGNS: &[&str] = &[
        "operation not permitted",
        "not permitted",
        "permission denied",
        "read-only file system",
        "seccomp",
        "landlock",
        "sandbox",
        "failed to write file",
        "network is unreachable",
        "could not resolve host",
        "name resolution",
    ];
    SIGNS.iter().any(|s| text.contains(s))
}

/// Codex's per-command sandbox override (codex-rs `SandboxPermissions`). The
/// model sets it on the shell tool call to REQUEST leaving the sandbox; it's
/// only a request — the command still flows through the approval gate (which
/// wraps this tool), so the user authorizes the escape ("如果用户授权运行时可以
/// 离开沙箱"). Values: `use_default` (stay sandboxed), `require_escalated` (run
/// outside the sandbox), `with_additional_permissions` (Codex widens the
/// sandbox for this one command; zode lacks per-command root widening, so it
/// escalates the same as `require_escalated`).
pub const SANDBOX_PERMISSIONS_FLAG: &str = "sandbox_permissions";
/// User-facing reason for the escalation, shown in the approval card (Codex's
/// `justification`). Stripped before the command reaches the real Bash.
pub const JUSTIFICATION_FLAG: &str = "justification";
/// Legacy boolean escape flag, still honored if a caller sets it, but no longer
/// advertised — `sandbox_permissions` is the canonical, Codex-aligned input.
pub const ESCAPE_FLAG: &str = "dangerouslyDisableSandbox";

/// True if `sandbox_permissions` (or the legacy boolean) asks to leave the
/// sandbox. `use_default` / absent → false.
pub(crate) fn wants_escape(input: &serde_json::Value) -> bool {
    if let Some(p) = input.get(SANDBOX_PERMISSIONS_FLAG).and_then(|v| v.as_str()) {
        return matches!(
            p.trim().to_ascii_lowercase().as_str(),
            "require_escalated" | "with_additional_permissions"
        );
    }
    input
        .get(ESCAPE_FLAG)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[async_trait]
impl Tool for SandboxedBashTool {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn description(&self) -> &str {
        // Cached: `description` returns `&str`, so we can't build per-call.
        &self.description
    }
    fn input_schema(&self) -> serde_json::Value {
        let mut schema = self.inner.input_schema();
        if let Some(props) = schema.get_mut("properties").and_then(|p| p.as_object_mut()) {
            props.insert(
                SANDBOX_PERMISSIONS_FLAG.to_string(),
                serde_json::json!({
                    "type": "string",
                    "enum": ["use_default", "require_escalated", "with_additional_permissions"],
                    "description": "Per-command sandbox override. Defaults to `use_default` \
                        (run inside the OS sandbox). Use `require_escalated` to run OUTSIDE \
                        the sandbox when the command genuinely needs network or to write \
                        outside the workspace; the user is asked to authorize the escape."
                }),
            );
            props.insert(
                JUSTIFICATION_FLAG.to_string(),
                serde_json::json!({
                    "type": "string",
                    "description": "One-sentence, user-facing reason for `require_escalated`; \
                        shown in the approval prompt. Omit otherwise."
                }),
            );
        }
        schema
    }
    fn safety_class(&self) -> SafetyClass {
        self.inner.safety_class()
    }
    async fn call(
        &self,
        ctx: &ToolUseContext,
        mut input: serde_json::Value,
    ) -> Result<serde_json::Value, AgentError> {
        let escape = wants_escape(&input);
        if let Some(obj) = input.as_object_mut() {
            obj.remove(SANDBOX_PERMISSIONS_FLAG);
            obj.remove(JUSTIFICATION_FLAG);
            obj.remove(ESCAPE_FLAG);
        }
        // Keep the raw input so an authorized escape can run unsandboxed.
        let raw_input = input.clone();

        // A model-requested escape needs its OWN human authorization here.
        // The outer approval (PermissionGatedTool) cannot double as consent:
        // it is skipped entirely under always-allow / yolo, which would let
        // the model silently disable the sandbox by setting a flag on its own
        // tool call. Non-interactive gates auto-answer, so with them the
        // request is not honored and the command runs sandboxed like any
        // other — in yolo the sandbox config is the user's only contract.
        let mut escape_declined = false;
        if escape {
            if self.gate.interactive() {
                let label = "Bash — run OUTSIDE the sandbox (model-requested escalation)";
                match self.gate.approve(label, &raw_input).await {
                    Approval::AllowOnce | Approval::AllowAlways => {
                        return self.inner.call(ctx, raw_input).await;
                    }
                    Approval::Deny => escape_declined = true,
                }
            } else {
                escape_declined = true;
            }
        }

        let mut wrapped = input;
        if let Some(cmd) = wrapped.get("command").and_then(|v| v.as_str()) {
            wrapped["command"] = serde_json::Value::String(shell_join(&self.config.wrap(cmd)));
        }
        let result = self.inner.call(ctx, wrapped).await?;

        // The command ran (1st prompt already approved it). If it failed on
        // what looks like a sandbox restriction, ASK the user whether to
        // ESCALATE — re-run the command OUTSIDE the sandbox (提权). This is a
        // second, distinct authorization, so it too is interactive-only (a
        // bypass gate would silently "approve" the escape) and is skipped
        // when an escape was already declined for this very call.
        if !escape_declined && self.gate.interactive() && looks_like_sandbox_denial(&result) {
            let label = "Bash — escalate: re-run this command OUTSIDE the sandbox";
            match self.gate.approve(label, &raw_input).await {
                Approval::AllowOnce | Approval::AllowAlways => {
                    return self.inner.call(ctx, raw_input).await;
                }
                Approval::Deny => {}
            }
        }
        Ok(result)
    }
}

/// Re-register, wrapping Bash/BashRun with the sandbox. Other tools pass
/// through unchanged.
pub fn apply_sandbox(
    src: agent::tool::ToolRegistry,
    config: &SandboxConfig,
    gate: &Arc<dyn ApprovalGate>,
) -> agent::tool::ToolRegistry {
    let mut out = agent::tool::ToolRegistry::new();
    for tool in src.list() {
        if matches!(tool.name(), "Bash" | "BashRun") {
            out.register(Arc::new(SandboxedBashTool::new(
                tool,
                config.clone(),
                gate.clone(),
            )));
        } else {
            out.register(tool);
        }
    }
    out
}
