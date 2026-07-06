//! Harness-level verification state and the `run_check` tool.
//!
//! `run_check` turns "I think it works" into structured evidence. It runs a
//! command, evaluates simple assertions over exit code / stdout / stderr, and
//! records whether the latest verification is still fresh. Mutating tools mark
//! that evidence stale, so `goal_complete` cannot stop an autonomous loop after
//! edits that have not been re-checked.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent::error::AgentError;
use agent::hook::{HookEvent, HookOutcome, RustHookHandler};
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 600;
const MODEL_OUTPUT_CAP: usize = 16 * 1024;

#[derive(Debug, Clone, Default)]
pub struct VerificationState {
    inner: Arc<Mutex<State>>,
}

#[derive(Debug, Clone, Default)]
struct State {
    generation: u64,
    last: Option<VerificationRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationRecord {
    pub generation: u64,
    pub passed: bool,
    pub tool: String,
    pub command: String,
    pub failures: Vec<String>,
}

impl VerificationState {
    pub fn record_pass(&self, tool: &str, command: &str) {
        self.record(tool, command, true, Vec::new());
    }

    pub fn record(&self, tool: &str, command: &str, passed: bool, failures: Vec<String>) {
        if let Ok(mut state) = self.inner.lock() {
            state.last = Some(VerificationRecord {
                generation: state.generation,
                passed,
                tool: tool.to_string(),
                command: command.to_string(),
                failures,
            });
        }
    }

    pub fn mark_mutation(&self, tool: &str) {
        if !tool_invalidates_verification(tool) {
            return;
        }
        if let Ok(mut state) = self.inner.lock() {
            state.generation = state.generation.saturating_add(1);
        }
    }

    pub fn completion_evidence(&self) -> Result<VerificationRecord, String> {
        let state = self
            .inner
            .lock()
            .map_err(|_| "verification state poisoned".to_string())?;
        let Some(record) = state.last.clone() else {
            return Err("run_check must pass before goal_complete can stop the loop".into());
        };
        if !record.passed {
            return Err(format!(
                "latest run_check did not pass: {}",
                record.failures.join("; ")
            ));
        }
        if record.generation != state.generation {
            return Err(
                "run_check evidence is stale because files, commands, git state, or another mutating tool changed after it ran".into(),
            );
        }
        Ok(record)
    }
}

pub fn verification_hook(state: VerificationState) -> RustHookHandler {
    RustHookHandler::new("verification-state", move |event| {
        match event {
            HookEvent::AfterToolUse { tool, .. } | HookEvent::PostToolUseFailure { tool, .. } => {
                state.mark_mutation(tool);
            }
            _ => {}
        }
        HookOutcome::Ok
    })
}

fn tool_invalidates_verification(tool: &str) -> bool {
    matches!(
        tool,
        "Bash"
            | "BashRun"
            | "KillShell"
            | "FileWrite"
            | "FileEdit"
            | "Mkdir"
            | "Move"
            | "Remove"
            | "GitCommit"
            | "GitBranch"
            | "GitCheckout"
            | "GitStash"
            | "GitWorktree"
            | "Task"
            | "define_agent"
            | "define_workflow"
            | "run_workflow"
            | "browser_act"
            | "browser_eval"
            | "browser_tabs"
            | "op_write"
            | "op_design"
            | "lsp_rename"
            | "lsp_format"
    )
}

#[derive(Debug)]
pub struct RunCheckTool {
    state: VerificationState,
}

impl RunCheckTool {
    pub fn new(state: VerificationState) -> Self {
        Self { state }
    }
}

#[derive(Debug, Deserialize)]
struct RunCheckInput {
    command: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default = "default_expect_exit_code")]
    expect_exit_code: i32,
    #[serde(default)]
    stdout_contains: Vec<String>,
    #[serde(default)]
    stderr_contains: Vec<String>,
    #[serde(default)]
    stdout_not_contains: Vec<String>,
    #[serde(default)]
    stderr_not_contains: Vec<String>,
}

fn default_expect_exit_code() -> i32 {
    0
}

#[async_trait]
impl Tool for RunCheckTool {
    fn name(&self) -> &str {
        "run_check"
    }

    fn description(&self) -> &str {
        "Run a verification command and evaluate explicit assertions. Returns structured {passed, failures, stdout, stderr, exit_code}. Use before claiming success or calling goal_complete."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell command to run as verification."},
                "cwd": {"type": "string", "description": "Working directory; relative paths resolve against current cwd."},
                "timeout_secs": {"type": "integer", "minimum": 1, "maximum": MAX_TIMEOUT_SECS},
                "expect_exit_code": {"type": "integer", "default": 0},
                "stdout_contains": {"type": "array", "items": {"type": "string"}},
                "stderr_contains": {"type": "array", "items": {"type": "string"}},
                "stdout_not_contains": {"type": "array", "items": {"type": "string"}},
                "stderr_not_contains": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["command"]
        })
    }

    fn safety_class(&self) -> SafetyClass {
        // This runs a shell command. Even verification commands may have side
        // effects, so keep normal approval semantics.
        SafetyClass::Mutating
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let parsed: RunCheckInput = serde_json::from_value(input)
            .map_err(|e| AgentError::other(format!("run_check invalid input: {e}")))?;
        if parsed.command.trim().is_empty() {
            return Err(AgentError::other("run_check command must be non-empty"));
        }
        let timeout_secs = parsed
            .timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, MAX_TIMEOUT_SECS);
        let cwd = resolve_cwd(&ctx.cwd, parsed.cwd.as_deref());

        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = tokio::process::Command::new("cmd");
            c.arg("/C").arg(&parsed.command);
            c
        } else {
            let mut c = tokio::process::Command::new("/bin/sh");
            c.arg("-c").arg(&parsed.command);
            c
        };
        cmd.current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let output =
            match tokio::time::timeout(Duration::from_secs(timeout_secs), cmd.output()).await {
                Ok(Ok(output)) => output,
                Ok(Err(err)) => {
                    return Err(AgentError::other(format!("run_check spawn failed: {err}")));
                }
                Err(_) => {
                    let failures = vec![format!("command timed out after {timeout_secs}s")];
                    self.state
                        .record("run_check", &parsed.command, false, failures.clone());
                    return Ok(json!({
                        "passed": false,
                        "command": parsed.command,
                        "cwd": cwd.display().to_string(),
                        "exit_code": null,
                        "stdout": "",
                        "stderr": "",
                        "stdout_truncated": false,
                        "stderr_truncated": false,
                        "failures": failures,
                    }));
                }
            };

        let exit_code = output.status.code().unwrap_or(-1);
        let (stdout, stdout_truncated) = cap_output(&String::from_utf8_lossy(&output.stdout));
        let (stderr, stderr_truncated) = cap_output(&String::from_utf8_lossy(&output.stderr));
        let failures = evaluate_assertions(&parsed, exit_code, &stdout, &stderr);
        let passed = failures.is_empty();
        self.state
            .record("run_check", &parsed.command, passed, failures.clone());

        Ok(json!({
            "passed": passed,
            "command": parsed.command,
            "cwd": cwd.display().to_string(),
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
            "stdout_truncated": stdout_truncated,
            "stderr_truncated": stderr_truncated,
            "failures": failures,
        }))
    }
}

fn resolve_cwd(base: &std::path::Path, cwd: Option<&str>) -> PathBuf {
    match cwd {
        Some(path) => {
            let p = PathBuf::from(path);
            if p.is_absolute() {
                p
            } else {
                base.join(p)
            }
        }
        None => base.to_path_buf(),
    }
}

fn evaluate_assertions(
    parsed: &RunCheckInput,
    exit_code: i32,
    stdout: &str,
    stderr: &str,
) -> Vec<String> {
    let mut failures = Vec::new();
    if exit_code != parsed.expect_exit_code {
        failures.push(format!(
            "expected exit_code {}, got {exit_code}",
            parsed.expect_exit_code
        ));
    }
    for needle in &parsed.stdout_contains {
        if !stdout.contains(needle) {
            failures.push(format!("stdout did not contain {needle:?}"));
        }
    }
    for needle in &parsed.stderr_contains {
        if !stderr.contains(needle) {
            failures.push(format!("stderr did not contain {needle:?}"));
        }
    }
    for needle in &parsed.stdout_not_contains {
        if stdout.contains(needle) {
            failures.push(format!("stdout unexpectedly contained {needle:?}"));
        }
    }
    for needle in &parsed.stderr_not_contains {
        if stderr.contains(needle) {
            failures.push(format!("stderr unexpectedly contained {needle:?}"));
        }
    }
    failures
}

fn cap_output(raw: &str) -> (String, bool) {
    if raw.len() <= MODEL_OUTPUT_CAP {
        return (raw.to_string(), false);
    }
    let head_end = raw
        .char_indices()
        .map(|(i, _)| i)
        .take_while(|&i| i <= MODEL_OUTPUT_CAP / 2)
        .last()
        .unwrap_or(0);
    let tail_start = raw
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| i >= raw.len().saturating_sub(MODEL_OUTPUT_CAP / 2))
        .unwrap_or(raw.len());
    (
        format!(
            "{}\n... run_check output truncated: {} bytes elided ...\n{}",
            &raw[..head_end],
            tail_start.saturating_sub(head_end),
            &raw[tail_start..]
        ),
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &std::path::Path) -> ToolUseContext {
        ToolUseContext::new(dir.to_path_buf())
    }

    #[tokio::test]
    async fn run_check_records_fresh_passed_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let state = VerificationState::default();
        let tool = RunCheckTool::new(state.clone());
        let out = tool
            .call(
                &ctx(dir.path()),
                json!({
                    "command": "printf ok",
                    "stdout_contains": ["ok"]
                }),
            )
            .await
            .unwrap();
        assert_eq!(out["passed"], true);
        let evidence = state.completion_evidence().unwrap();
        assert_eq!(evidence.command, "printf ok");
    }

    #[tokio::test]
    async fn failed_check_blocks_completion_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let state = VerificationState::default();
        let tool = RunCheckTool::new(state.clone());
        let out = tool
            .call(
                &ctx(dir.path()),
                json!({
                    "command": "printf nope",
                    "stdout_contains": ["ok"]
                }),
            )
            .await
            .unwrap();
        assert_eq!(out["passed"], false);
        assert!(state.completion_evidence().is_err());
    }

    #[test]
    fn mutating_tool_makes_prior_evidence_stale() {
        let state = VerificationState::default();
        state.record_pass("run_check", "cargo test");
        assert!(state.completion_evidence().is_ok());
        state.mark_mutation("FileWrite");
        assert!(state.completion_evidence().unwrap_err().contains("stale"));
    }
}
