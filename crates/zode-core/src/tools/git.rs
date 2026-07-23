//! Git tools. Each wraps the system `git` binary via tokio::process.
//! Read-only tools (status/diff/log/blame) are SafetyClass::ReadOnly;
//! mutating ones (commit/checkout/stash/branch) are Mutating and get
//! wrapped by PermissionGatedTool at assembly time. `git pr` is post-v1.

use std::time::Duration;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::process_supervision::{run_captured, CaptureError};

const GIT_TIMEOUT: Duration = Duration::from_secs(120);
const GIT_STREAM_CAP: usize = 1024 * 1024;

/// Run `git <args>` in `cwd`; return a JSON object with the result.
async fn run_git(ctx: &ToolUseContext, args: &[String]) -> Result<Value, AgentError> {
    let mut command = tokio::process::Command::new("git");
    command.args(args).current_dir(&ctx.cwd);
    let output = match run_captured(command, &ctx.abort, GIT_TIMEOUT, GIT_STREAM_CAP).await {
        Ok(output) => output,
        Err(CaptureError::Aborted(reason)) => return Err(AgentError::Aborted(reason)),
        Err(CaptureError::TimedOut) => {
            return Err(AgentError::other(format!(
                "git command timed out after {}s",
                GIT_TIMEOUT.as_secs()
            )))
        }
        Err(error) => return Err(AgentError::other(format!("git execution failed: {error}"))),
    };
    Ok(json!({
        "ok": output.status.success(),
        "exit_code": output.status.code().unwrap_or(-1),
        "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
        "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
        "stdout_truncated": output.stdout_truncated,
        "stderr_truncated": output.stderr_truncated,
    }))
}

/// Pull a string array out of an optional input field, defaulting to [].
fn str_array(input: &Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn str_field<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(|v| v.as_str())
}

// ---------------------------------------------------------------------
// Read-only tools
// ---------------------------------------------------------------------

#[derive(Debug)]
pub struct GitStatus;
#[async_trait]
impl Tool for GitStatus {
    fn name(&self) -> &str {
        "GitStatus"
    }
    fn description(&self) -> &str {
        "Show working tree status (porcelain)."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }
    async fn call(&self, ctx: &ToolUseContext, _input: Value) -> Result<Value, AgentError> {
        run_git(
            ctx,
            &["status".into(), "--porcelain=v1".into(), "--branch".into()],
        )
        .await
    }
}

#[derive(Debug)]
pub struct GitDiff;
#[async_trait]
impl Tool for GitDiff {
    fn name(&self) -> &str {
        "GitDiff"
    }
    fn description(&self) -> &str {
        "Show changes. Pass `staged: true` for the index, or `paths: [..]` to scope."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "staged": {"type": "boolean"},
            "paths": {"type": "array", "items": {"type": "string"}}
        }})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let mut args = vec!["diff".to_string()];
        if input
            .get("staged")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            args.push("--staged".into());
        }
        let paths = str_array(&input, "paths");
        if !paths.is_empty() {
            args.push("--".into());
            args.extend(paths);
        }
        run_git(ctx, &args).await
    }
}

#[derive(Debug)]
pub struct GitLog;
#[async_trait]
impl Tool for GitLog {
    fn name(&self) -> &str {
        "GitLog"
    }
    fn description(&self) -> &str {
        "Show recent commits. `max_count` limits how many (default 20)."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"max_count": {"type": "integer"}}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let n = input
            .get("max_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(20);
        run_git(
            ctx,
            &[
                "log".into(),
                format!("--max-count={n}"),
                "--oneline".into(),
                "--decorate".into(),
            ],
        )
        .await
    }
}

#[derive(Debug)]
pub struct GitBlame;
#[async_trait]
impl Tool for GitBlame {
    fn name(&self) -> &str {
        "GitBlame"
    }
    fn description(&self) -> &str {
        "Show line-by-line authorship for a file. Requires `path`."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let path = str_field(&input, "path")
            .ok_or_else(|| AgentError::other("GitBlame requires `path`"))?;
        run_git(ctx, &["blame".into(), "--".into(), path.into()]).await
    }
}

// ---------------------------------------------------------------------
// Mutating tools
// ---------------------------------------------------------------------

#[derive(Debug)]
pub struct GitCommit;
#[async_trait]
impl Tool for GitCommit {
    fn name(&self) -> &str {
        "GitCommit"
    }
    fn description(&self) -> &str {
        "Create a commit. `message` required. `all: true` stages tracked changes first; `paths: [..]` stages specific files."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "message": {"type": "string"},
            "all": {"type": "boolean"},
            "paths": {"type": "array", "items": {"type": "string"}}
        }, "required": ["message"]})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let message = str_field(&input, "message")
            .ok_or_else(|| AgentError::other("GitCommit requires `message`"))?;
        let paths = str_array(&input, "paths");
        let stage_all = input.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
        if !paths.is_empty() {
            // `--` fences user paths so a leading-dash name isn't a flag.
            let mut add = vec!["add".to_string(), "--".to_string()];
            add.extend(paths.clone());
            let r = run_git(ctx, &add).await?;
            if !r["ok"].as_bool().unwrap_or(false) {
                return Ok(r);
            }
        }
        let mut args = vec!["commit".to_string(), "-m".into(), message.to_string()];
        // `-a` only when no explicit paths: combining them would stage ALL
        // tracked changes, not just the selected paths. Paths take precedence.
        if stage_all && paths.is_empty() {
            args.insert(1, "-a".into());
        }
        run_git(ctx, &args).await
    }
}

#[derive(Debug)]
pub struct GitBranch;
#[async_trait]
impl Tool for GitBranch {
    fn name(&self) -> &str {
        "GitBranch"
    }
    fn description(&self) -> &str {
        "List branches, or create one with `name`. `delete: name` removes a branch."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "name": {"type": "string"}, "delete": {"type": "string"}
        }})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        // `--` fences the branch name so a leading-dash value isn't a flag.
        let args = if let Some(d) = str_field(&input, "delete") {
            vec!["branch".into(), "-D".into(), "--".into(), d.to_string()]
        } else if let Some(n) = str_field(&input, "name") {
            vec!["branch".into(), "--".into(), n.to_string()]
        } else {
            vec!["branch".into(), "--list".into()]
        };
        run_git(ctx, &args).await
    }
}

#[derive(Debug)]
pub struct GitCheckout;
#[async_trait]
impl Tool for GitCheckout {
    fn name(&self) -> &str {
        "GitCheckout"
    }
    fn description(&self) -> &str {
        "Switch branches or restore files. `target` is a branch/ref; `create: true` makes a new branch."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "target": {"type": "string"}, "create": {"type": "boolean"}
        }, "required": ["target"]})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let target = str_field(&input, "target")
            .ok_or_else(|| AgentError::other("GitCheckout requires `target`"))?;
        // A branch switch can't use `--` fencing (that means "restore
        // pathspec"), so reject leading-dash targets git would read as flags.
        if target.starts_with('-') {
            return Err(AgentError::other(
                "GitCheckout target must not start with '-'",
            ));
        }
        let mut args = vec!["checkout".to_string()];
        if input
            .get("create")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            args.push("-b".into());
        }
        args.push(target.to_string());
        run_git(ctx, &args).await
    }
}

#[derive(Debug)]
pub struct GitStash;
#[async_trait]
impl Tool for GitStash {
    fn name(&self) -> &str {
        "GitStash"
    }
    fn description(&self) -> &str {
        "Stash changes. `op` is one of push (default) | pop | list."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"op": {"type": "string", "enum": ["push", "pop", "list"]}}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        // Enforce the enum at runtime (the schema is advisory only) so an
        // arbitrary `git stash <subcommand>` can't be smuggled in.
        let op = match str_field(&input, "op").unwrap_or("push") {
            valid @ ("push" | "pop" | "list") => valid,
            other => {
                return Err(AgentError::other(format!(
                    "GitStash op must be push | pop | list, got {other:?}"
                )))
            }
        };
        run_git(ctx, &["stash".to_string(), op.to_string()]).await
    }
}

#[derive(Debug)]
pub struct GitWorktree;
#[async_trait]
impl Tool for GitWorktree {
    fn name(&self) -> &str {
        "GitWorktree"
    }
    fn description(&self) -> &str {
        "Manage git worktrees. `op`: list (default) | add | remove. `add` needs `path` (+ optional `branch` to create & check out); `remove` needs `path`."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "op": {"type": "string", "enum": ["list", "add", "remove"]},
            "path": {"type": "string", "description": "Worktree path (add/remove)."},
            "branch": {"type": "string", "description": "Branch to create & check out in the new worktree (add only)."}
        }})
    }
    fn safety_class(&self) -> SafetyClass {
        // add/remove create or delete a working tree → gated. (list is benign,
        // but one class per tool; the conservative choice is to confirm.)
        SafetyClass::Mutating
    }
    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let op = match str_field(&input, "op").unwrap_or("list") {
            valid @ ("list" | "add" | "remove") => valid,
            other => {
                return Err(AgentError::other(format!(
                    "GitWorktree op must be list | add | remove, got {other:?}"
                )))
            }
        };
        let mut args = vec!["worktree".to_string(), op.to_string()];
        match op {
            "list" => {}
            "add" => {
                let path = str_field(&input, "path")
                    .ok_or_else(|| AgentError::other("GitWorktree add requires 'path'"))?;
                if let Some(branch) = str_field(&input, "branch") {
                    args.push("-b".to_string());
                    args.push(branch.to_string());
                }
                args.push(path.to_string());
            }
            "remove" => {
                let path = str_field(&input, "path")
                    .ok_or_else(|| AgentError::other("GitWorktree remove requires 'path'"))?;
                args.push(path.to_string());
            }
            _ => unreachable!(),
        }
        run_git(ctx, &args).await
    }
}

/// All nine git tools, ready to register.
pub fn all_git_tools() -> Vec<std::sync::Arc<dyn Tool>> {
    use std::sync::Arc;
    vec![
        Arc::new(GitStatus),
        Arc::new(GitDiff),
        Arc::new(GitLog),
        Arc::new(GitBlame),
        Arc::new(GitCommit),
        Arc::new(GitBranch),
        Arc::new(GitCheckout),
        Arc::new(GitStash),
        Arc::new(GitWorktree),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap();
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);
        dir
    }

    fn ctx(p: &std::path::Path) -> ToolUseContext {
        ToolUseContext::new(p.to_path_buf())
    }

    #[tokio::test]
    async fn git_status_clean_repo() {
        let dir = init_repo();
        let out = GitStatus.call(&ctx(dir.path()), json!({})).await.unwrap();
        assert!(out["ok"].as_bool().unwrap(), "stderr: {}", out["stderr"]);
        // porcelain body (excluding the ## branch line) is empty on a clean tree
        let body: Vec<&str> = out["stdout"]
            .as_str()
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with("##"))
            .collect();
        assert!(body.is_empty(), "unexpected: {body:?}");
    }

    #[tokio::test]
    async fn git_log_shows_init_commit() {
        let dir = init_repo();
        let out = GitLog
            .call(&ctx(dir.path()), json!({"max_count": 5}))
            .await
            .unwrap();
        assert!(out["stdout"].as_str().unwrap().contains("init"));
    }

    #[tokio::test]
    async fn git_tools_honor_root_abort_before_spawn() {
        let dir = init_repo();
        let context = ctx(dir.path());
        context.abort.abort_with_reason("watchdog stop");

        let error = GitStatus.call(&context, json!({})).await.unwrap_err();

        assert!(matches!(error, AgentError::Aborted(reason) if reason == "watchdog stop"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn aborting_git_commit_kills_hook_descendants() {
        use std::os::unix::fs::PermissionsExt;

        let dir = init_repo();
        let pid_file = dir.path().join("hook-child.pid");
        let hook = dir.path().join(".git/hooks/pre-commit");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\nsleep 30 &\nchild=$!\nprintf %s \"$child\" > '{}'\nwait\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&hook, permissions).unwrap();
        std::process::Command::new("git")
            .args([
                "config",
                "core.hooksPath",
                hook.parent().unwrap().to_str().unwrap(),
            ])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();

        let context = ctx(dir.path());
        let abort = context.abort.clone();
        let mut call = tokio::spawn(async move {
            GitCommit
                .call(&context, json!({"message": "blocked hook", "all": true}))
                .await
        });
        let pid = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if call.is_finished() {
                    let result = (&mut call).await.unwrap();
                    panic!("git commit exited before hook pid was written: {result:?}");
                }
                if let Ok(raw) = tokio::fs::read_to_string(&pid_file).await {
                    if let Ok(pid) = raw.trim().parse::<u32>() {
                        break pid;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("git hook did not write its child pid");

        abort.abort_with_reason("watchdog stop");
        let error = call.await.unwrap().unwrap_err();
        assert!(matches!(error, AgentError::Aborted(reason) if reason == "watchdog stop"));

        for _ in 0..100 {
            let alive = std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .output()
                .is_ok_and(|output| output.status.success());
            if !alive {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("git hook descendant {pid} survived abort");
    }

    #[tokio::test]
    async fn git_commit_records_change() {
        let dir = init_repo();
        // New untracked file: `-a` would NOT stage it (that's git's
        // behavior), so stage it explicitly via `paths`.
        std::fs::write(dir.path().join("b.txt"), "world\n").unwrap();
        let out = GitCommit
            .call(
                &ctx(dir.path()),
                json!({"message": "add b", "paths": ["b.txt"]}),
            )
            .await
            .unwrap();
        assert!(out["ok"].as_bool().unwrap(), "stderr: {}", out["stderr"]);
        let log = GitLog.call(&ctx(dir.path()), json!({})).await.unwrap();
        assert!(log["stdout"].as_str().unwrap().contains("add b"));
    }

    #[tokio::test]
    async fn git_branch_creates_and_lists() {
        let dir = init_repo();
        GitBranch
            .call(&ctx(dir.path()), json!({"name": "feature"}))
            .await
            .unwrap();
        let out = GitBranch.call(&ctx(dir.path()), json!({})).await.unwrap();
        assert!(out["stdout"].as_str().unwrap().contains("feature"));
    }

    #[test]
    fn safety_classes() {
        assert_eq!(GitStatus.safety_class(), SafetyClass::ReadOnly);
        assert_eq!(GitDiff.safety_class(), SafetyClass::ReadOnly);
        assert_eq!(GitLog.safety_class(), SafetyClass::ReadOnly);
        assert_eq!(GitBlame.safety_class(), SafetyClass::ReadOnly);
        assert_eq!(GitCommit.safety_class(), SafetyClass::Mutating);
        assert_eq!(GitCheckout.safety_class(), SafetyClass::Mutating);
        assert_eq!(GitStash.safety_class(), SafetyClass::Mutating);
        assert_eq!(GitBranch.safety_class(), SafetyClass::Mutating);
    }

    #[test]
    fn all_eight_tools() {
        assert_eq!(all_git_tools().len(), 9);
    }

    #[tokio::test]
    async fn checkout_rejects_leading_dash_target() {
        let dir = init_repo();
        let r = GitCheckout
            .call(&ctx(dir.path()), json!({"target": "-f"}))
            .await;
        assert!(r.is_err(), "leading-dash target must be rejected");
    }

    #[tokio::test]
    async fn stash_rejects_unknown_op() {
        let dir = init_repo();
        let r = GitStash
            .call(&ctx(dir.path()), json!({"op": "clear"}))
            .await;
        assert!(r.is_err(), "unknown stash op must be rejected");
    }

    #[tokio::test]
    async fn commit_with_dash_named_path_is_fenced() {
        // A path literally starting with '-' must be treated as a path, not
        // a git flag (the `add --` fence). We just assert it doesn't error
        // out as an unknown-flag parse and the tool runs.
        let dir = init_repo();
        std::fs::write(dir.path().join("-weird.txt"), "x\n").unwrap();
        let out = GitCommit
            .call(
                &ctx(dir.path()),
                json!({"message": "add weird", "paths": ["-weird.txt"]}),
            )
            .await
            .unwrap();
        assert!(out["ok"].as_bool().unwrap(), "stderr: {}", out["stderr"]);
    }
}
