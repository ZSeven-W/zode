//! Sandbox mode (`--sandbox`). Wraps shell commands so writes are confined
//! to the workspace cwd (+ /tmp). fs tools are already cwd-confined by
//! WorkspacePolicy; this covers Bash / BashRun. macOS uses sandbox-exec;
//! Linux uses bwrap; Windows is unsupported (assemble errors out).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;

use crate::error::CoreError;

#[derive(Debug, Clone)]
pub enum SandboxConfig {
    MacOs { cwd: PathBuf },
    Linux { cwd: PathBuf },
}

impl SandboxConfig {
    pub fn for_current_os(cwd: &Path) -> Result<Self, CoreError> {
        // Canonicalize: the OS sandbox resolves symlinks before matching, so
        // an allowed subpath must be the real path (e.g. macOS /tmp ->
        // /private/tmp). Without this, writes *inside* cwd get denied.
        let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        if cfg!(target_os = "macos") {
            Ok(SandboxConfig::MacOs { cwd })
        } else if cfg!(target_os = "linux") {
            Ok(SandboxConfig::Linux { cwd })
        } else {
            Err(CoreError::Other(
                "--sandbox is only supported on macOS and Linux".into(),
            ))
        }
    }

    /// Build the wrapped argv that runs `command` under the sandbox.
    pub fn wrap(&self, command: &str) -> Vec<String> {
        match self {
            SandboxConfig::MacOs { cwd } => vec![
                "sandbox-exec".into(),
                "-p".into(),
                macos_profile(cwd),
                "/bin/sh".into(),
                "-c".into(),
                command.to_string(),
            ],
            SandboxConfig::Linux { cwd } => linux_bwrap_args(cwd, command),
        }
    }
}

/// Escape a string for a Scheme string literal (sandbox profile syntax):
/// backslash and double-quote.
fn scheme_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// sandbox-exec profile: read anywhere, write only cwd + /tmp.
fn macos_profile(cwd: &Path) -> String {
    let cwd = scheme_escape(&cwd.display().to_string());
    format!(
        "(version 1)\
         (allow default)\
         (deny file-write*)\
         (allow file-write* (subpath \"{cwd}\"))\
         (allow file-write* (subpath \"/tmp\"))\
         (allow file-write* (subpath \"/private/tmp\"))\
         (allow file-write* (subpath \"/private/var/folders\"))"
    )
}

/// bwrap argv: ro-bind the whole fs, rw-bind cwd + /tmp, then sh -c.
/// No `--chdir`: bwrap inherits the caller's working directory, which the
/// inner Bash tool already set to its resolved `cwd` input (a forced
/// --chdir would override that and run from the wrong directory).
fn linux_bwrap_args(cwd: &Path, command: &str) -> Vec<String> {
    let cwd = cwd.display().to_string();
    vec![
        "bwrap".into(),
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
        "--bind".into(),
        cwd.clone(),
        cwd,
        "--bind".into(),
        "/tmp".into(),
        "/tmp".into(),
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
        "/bin/sh".into(),
        "-c".into(),
        command.to_string(),
    ]
}

/// Quote argv into a single /bin/sh-safe command string.
fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if !a.is_empty()
                && a.chars()
                    .all(|c| c.is_alphanumeric() || "-_/.:=".contains(c))
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Wraps Bash / BashRun: rewrites the `command` input to run inside the
/// sandbox before delegating. The tool name is preserved.
#[derive(Debug)]
pub struct SandboxedBashTool {
    inner: Arc<dyn Tool>,
    config: SandboxConfig,
}

impl SandboxedBashTool {
    pub fn new(inner: Arc<dyn Tool>, config: SandboxConfig) -> Self {
        Self { inner, config }
    }
}

#[async_trait]
impl Tool for SandboxedBashTool {
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
        mut input: serde_json::Value,
    ) -> Result<serde_json::Value, AgentError> {
        if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
            let wrapped = shell_join(&self.config.wrap(cmd));
            input["command"] = serde_json::Value::String(wrapped);
        }
        self.inner.call(ctx, input).await
    }
}

/// Re-register, wrapping Bash/BashRun with the sandbox. Other tools pass
/// through unchanged.
pub fn apply_sandbox(
    src: agent::tool::ToolRegistry,
    config: &SandboxConfig,
) -> agent::tool::ToolRegistry {
    let mut out = agent::tool::ToolRegistry::new();
    for tool in src.list() {
        if matches!(tool.name(), "Bash" | "BashRun") {
            out.register(Arc::new(SandboxedBashTool::new(tool, config.clone())));
        } else {
            out.register(tool);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_profile_confines_writes_to_cwd() {
        let p = macos_profile(Path::new("/work/proj"));
        assert!(p.contains("(deny file-write*)"));
        assert!(p.contains("/work/proj"));
        assert!(p.contains("/tmp"));
    }

    #[test]
    fn linux_bwrap_binds_cwd_and_runs_command() {
        let args = linux_bwrap_args(Path::new("/work/proj"), "echo hi");
        assert_eq!(args[0], "bwrap");
        assert!(args.iter().any(|a| a == "--bind"));
        assert!(args.iter().any(|a| a == "/work/proj"));
        assert_eq!(args.last().unwrap(), "echo hi");
    }

    #[test]
    fn scheme_escape_handles_quotes_and_backslashes() {
        assert_eq!(scheme_escape(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(scheme_escape("/work/proj"), "/work/proj");
        // A malicious path can't break out of the string literal.
        let p = macos_profile(Path::new(r#"/x") (allow file-write*) ;"#));
        assert!(p.contains(r#"\""#), "quote must be escaped in profile");
    }

    #[test]
    fn shell_join_quotes_unsafe_args() {
        assert_eq!(shell_join(&["echo".into(), "hi".into()]), "echo hi");
        let joined = shell_join(&["sh".into(), "-c".into(), "rm -rf x".into()]);
        assert!(joined.contains("'rm -rf x'"));
    }

    #[test]
    fn supported_platform_check() {
        let r = SandboxConfig::for_current_os(Path::new("/x"));
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert!(r.is_ok());
        } else {
            assert!(r.is_err());
        }
    }

    #[test]
    fn for_current_os_canonicalizes_cwd() {
        // The stored cwd must be the canonical (symlink-resolved) path, or
        // the OS sandbox would deny writes inside cwd (e.g. /tmp ->
        // /private/tmp on macOS).
        let dir = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap();
        if let Ok(cfg) = SandboxConfig::for_current_os(dir.path()) {
            let stored = match cfg {
                SandboxConfig::MacOs { cwd } | SandboxConfig::Linux { cwd } => cwd,
            };
            assert_eq!(stored, real);
        }
    }
}
