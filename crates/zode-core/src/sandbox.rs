//! Sandbox mode (`--sandbox`). Wraps shell commands so they run under an OS
//! sandbox. Modeled on Codex (`SandboxPolicy`: read-only / workspace-write /
//! full-access) and Claude Code (network denied by default, configurable
//! writable roots): fs tools are already cwd-confined by WorkspacePolicy; this
//! covers Bash / BashRun. macOS uses sandbox-exec (Seatbelt); Linux uses
//! bwrap; Windows is unsupported (assemble errors out).
//!
//! Two modes:
//! - **read-only** — deny ALL filesystem writes (safe exploration).
//! - **workspace-write** — writes confined to cwd + tmp + extra writable roots.
//!
//! In BOTH modes outbound **network is denied by default** (the biggest reason
//! to sandbox an agent's shell) and re-enabled only with `allow_network`. The
//! standard character devices (`/dev/null`, `/dev/tty`, …) stay writable so
//! ordinary commands (`cmd 2>/dev/null`, terminal output) still work.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::approval::{Approval, ApprovalGate};
use crate::error::CoreError;

/// What the sandbox lets the command write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SandboxMode {
    /// Deny every filesystem write.
    ReadOnly,
    /// Allow writes to the workspace cwd, tmp, and configured writable roots.
    #[default]
    WorkspaceWrite,
}

impl SandboxMode {
    /// Parse a config / flag string; unknown values fall back to the default
    /// (workspace-write) so a typo never silently disables the sandbox.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "read-only" | "readonly" | "ro" => SandboxMode::ReadOnly,
            _ => SandboxMode::WorkspaceWrite,
        }
    }
}

/// Host sandbox backend, chosen from the target OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SandboxOs {
    MacOs,
    Linux,
}

/// Standard character devices a command legitimately writes to. Keeping these
/// writable in every mode is what lets `2>/dev/null`, tty output, etc. work —
/// otherwise even a read-only command fails (matches Codex's seatbelt allows).
const WRITABLE_DEVICES: &[&str] = &[
    "/dev/null",
    "/dev/zero",
    "/dev/stdout",
    "/dev/stderr",
    "/dev/tty",
    "/dev/random",
    "/dev/urandom",
    "/dev/dtracehelper",
];

/// Project metadata dirs kept READ-ONLY even inside a writable root, mirroring
/// Codex's protected `.git` / `.codex` carveouts (codex-rs permissions.rs:
/// `default_read_only_subpaths_for_writable_root`). `.git` stops a sandboxed
/// command from rewriting history; `.zode` stops it from editing its own
/// sandbox / permission state (`state.json`) to silently self-escalate. Like
/// Codex's `protect_missing_dot_codex`, these are denied even when the dir does
/// not exist yet (so first-time creation is blocked) — on macOS, where a deny
/// rule needs no existing inode; the Linux backend can only mask paths that
/// already exist.
const PROTECTED_SUBDIRS: &[&str] = &[".git", ".zode"];

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    os: SandboxOs,
    /// Canonical workspace dir (symlinks resolved — the OS sandbox matches the
    /// real path, e.g. macOS /tmp -> /private/tmp).
    cwd: PathBuf,
    mode: SandboxMode,
    /// Allow outbound network. Off by default (deny network / unshare-net).
    allow_network: bool,
    /// Extra writable roots (workspace-write only), already canonicalized.
    writable_roots: Vec<PathBuf>,
    /// Drop `/tmp` from the default writable roots (Codex `exclude_slash_tmp`).
    exclude_slash_tmp: bool,
    /// Drop `$TMPDIR` from the default writable roots (Codex
    /// `exclude_tmpdir_env_var`). macOS gives each user a private `$TMPDIR`
    /// under `/var/folders`; Linux usually leaves it unset.
    exclude_tmpdir_env_var: bool,
}

impl SandboxConfig {
    /// Backward-compatible constructor: workspace-write, network denied, no
    /// extra writable roots. (`--sandbox` with no further configuration.)
    pub fn for_current_os(cwd: &Path) -> Result<Self, CoreError> {
        Self::new(cwd, SandboxMode::WorkspaceWrite, false, &[])
    }

    /// Full constructor. `writable_roots` are extra dirs the command may write
    /// to in workspace-write mode (ignored in read-only mode).
    pub fn new(
        cwd: &Path,
        mode: SandboxMode,
        allow_network: bool,
        writable_roots: &[PathBuf],
    ) -> Result<Self, CoreError> {
        let os = if cfg!(target_os = "macos") {
            SandboxOs::MacOs
        } else if cfg!(target_os = "linux") {
            SandboxOs::Linux
        } else {
            return Err(CoreError::Other(
                "--sandbox is only supported on macOS and Linux".into(),
            ));
        };
        Ok(Self {
            os,
            cwd: canonical(cwd),
            mode,
            allow_network,
            writable_roots: writable_roots.iter().map(|p| canonical(p)).collect(),
            // Codex defaults: /tmp and $TMPDIR ARE writable in workspace-write.
            exclude_slash_tmp: false,
            exclude_tmpdir_env_var: false,
        })
    }

    pub fn mode(&self) -> SandboxMode {
        self.mode
    }
    pub fn allow_network(&self) -> bool {
        self.allow_network
    }
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
    /// Extra writable roots (workspace-write mode). Used to widen the file-tool
    /// `WorkspacePolicy` to match the shell sandbox.
    pub fn writable_roots(&self) -> &[PathBuf] {
        &self.writable_roots
    }
    /// Return a copy rebased on a different workspace cwd, preserving mode /
    /// network / writable roots / temp policy. Used when a tab resumes a
    /// session in another directory, so the sandbox confines to THAT repo.
    pub fn with_cwd(mut self, cwd: &Path) -> Self {
        self.cwd = canonical(cwd);
        self
    }

    /// Return a copy with a different mode (for runtime `/sandbox` toggles).
    pub fn with_mode(mut self, mode: SandboxMode) -> Self {
        self.mode = mode;
        self
    }
    /// Return a copy with network allowed/denied.
    pub fn with_network(mut self, allow: bool) -> Self {
        self.allow_network = allow;
        self
    }
    /// Return a copy with the default-temp-root policy set (Codex
    /// `exclude_slash_tmp` / `exclude_tmpdir_env_var`).
    pub fn with_temp_policy(
        mut self,
        exclude_slash_tmp: bool,
        exclude_tmpdir_env_var: bool,
    ) -> Self {
        self.exclude_slash_tmp = exclude_slash_tmp;
        self.exclude_tmpdir_env_var = exclude_tmpdir_env_var;
        self
    }

    /// All dirs that should be writable in workspace-write mode, mirroring
    /// Codex's `get_writable_roots_with_cwd`: cwd + the configured extra roots +
    /// (unless excluded) `/tmp` and `$TMPDIR`. Every path is canonicalized so it
    /// matches the real inode the sandbox sees (macOS `/tmp` → `/private/tmp`,
    /// `$TMPDIR` → `/private/var/folders/...`).
    fn writable_dirs(&self) -> Vec<String> {
        let mut roots: Vec<String> = vec![self.cwd.display().to_string()];
        for r in &self.writable_roots {
            roots.push(r.display().to_string());
        }
        if !self.exclude_slash_tmp {
            // macOS resolves /tmp to /private/tmp; cover both forms.
            roots.push(canonical(Path::new("/tmp")).display().to_string());
            roots.push("/tmp".into());
        }
        if !self.exclude_tmpdir_env_var {
            if let Some(tmp) = std::env::var_os("TMPDIR") {
                let tmp = Path::new(&tmp);
                if tmp.is_absolute() {
                    roots.push(canonical(tmp).display().to_string());
                }
            }
        }
        roots.sort();
        roots.dedup();
        roots
    }

    /// Roots whose `.git` / `.zode` metadata must stay read-only: cwd plus the
    /// configured extra writable roots (NOT the shared temp dirs). Mirrors
    /// Codex applying `read_only_subpaths` to every writable workspace root.
    fn protected_bases(&self) -> Vec<String> {
        let mut bases = vec![self.cwd.display().to_string()];
        for r in &self.writable_roots {
            bases.push(r.display().to_string());
        }
        bases.sort();
        bases.dedup();
        bases
    }

    /// Absolute `.git` / `.zode` paths kept read-only inside every writable
    /// root (workspace-write only). The file-tool `WorkspacePolicy` denies
    /// writes to these, mirroring the shell-sandbox carveout so the agent can't
    /// rewrite git history or edit its own `.zode/state.json` via FileWrite/Edit
    /// either. Empty in read-only mode (writes already denied everywhere).
    pub fn protected_paths(&self) -> Vec<PathBuf> {
        if self.mode != SandboxMode::WorkspaceWrite {
            return Vec::new();
        }
        let mut out = Vec::new();
        for base in self.protected_bases() {
            for sub in PROTECTED_SUBDIRS {
                out.push(PathBuf::from(&base).join(sub));
            }
        }
        out
    }

    /// Build the wrapped argv that runs a `/bin/sh -c <command>` under the
    /// sandbox (used by the shell tool).
    pub fn wrap(&self, command: &str) -> Vec<String> {
        self.wrap_argv(&["/bin/sh".to_string(), "-c".to_string(), command.to_string()])
    }

    /// Wrap an arbitrary `argv` (program + args) so it runs under the sandbox.
    /// Used by the shell tool (via [`Self::wrap`]) and by the sandboxed
    /// filesystem sink, which runs the mutating coreutil (`cat`/`mkdir`/`mv`/
    /// `rm`) under the same kernel sandbox so file writes are kernel-enforced.
    pub fn wrap_argv(&self, argv: &[String]) -> Vec<String> {
        match self.os {
            SandboxOs::MacOs => {
                let mut out = vec!["sandbox-exec".into(), "-p".into(), self.macos_profile()];
                out.extend(argv.iter().cloned());
                out
            }
            SandboxOs::Linux => {
                let mut out = self.linux_bwrap_prefix();
                out.extend(argv.iter().cloned());
                out
            }
        }
    }

    /// sandbox-exec profile. `(allow default)` baseline, then deny network
    /// (unless allowed) and confine writes per the mode, re-allowing the
    /// standard devices so ordinary commands keep working.
    fn macos_profile(&self) -> String {
        let mut p = String::from("(version 1)(allow default)");
        if !self.allow_network {
            // Block outbound + inbound network so a sandboxed command can't
            // fetch or exfiltrate. Opt back in with allow_network.
            p.push_str("(deny network*)");
        }
        // Deny all writes, then re-allow the devices + (workspace-write) roots.
        p.push_str("(deny file-write*)");
        for dev in WRITABLE_DEVICES {
            p.push_str(&format!("(allow file-write* (literal \"{dev}\"))"));
        }
        p.push_str("(allow file-write* (subpath \"/dev/fd\"))");
        if self.mode == SandboxMode::WorkspaceWrite {
            for root in self.writable_dirs() {
                let root = scheme_escape(&root);
                p.push_str(&format!("(allow file-write* (subpath \"{root}\"))"));
            }
            // Carve the protected metadata dirs back out. Seatbelt is
            // last-match-wins, so these denies (emitted AFTER the allows above)
            // win inside an otherwise-writable root — and they apply even to
            // dirs that don't exist yet, blocking first-time creation.
            for base in self.protected_bases() {
                for sub in PROTECTED_SUBDIRS {
                    let path = scheme_escape(&format!("{base}/{sub}"));
                    p.push_str(&format!("(deny file-write* (subpath \"{path}\"))"));
                }
            }
        }
        p
    }

    /// bwrap argv: ro-bind the whole fs, rw-bind the writable roots (none in
    /// read-only mode), a fresh /dev (provides /dev/null etc.) and /proc, and
    /// `--unshare-net` to drop the network unless it's explicitly allowed.
    /// No `--chdir`: bwrap inherits the caller's cwd, which the inner Bash tool
    /// already set to its resolved `cwd` input.
    fn linux_bwrap_prefix(&self) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "bwrap".into(),
            "--ro-bind".into(),
            "/".into(),
            "/".into(),
            "--dev".into(),
            "/dev".into(),
            "--proc".into(),
            "/proc".into(),
        ];
        if !self.allow_network {
            args.push("--unshare-net".into());
        }
        if self.mode == SandboxMode::WorkspaceWrite {
            for root in self.writable_dirs() {
                // Only bind dirs that exist; bwrap errors on a missing source.
                if Path::new(&root).exists() {
                    args.push("--bind".into());
                    args.push(root.clone());
                    args.push(root);
                }
            }
            // Re-mask protected metadata read-only ON TOP of the rw binds.
            // bwrap applies binds in order and a later bind wins for an
            // overlapping path, so the nested ro-bind makes `.git` / `.zode`
            // read-only inside the writable root. For a dir that does NOT exist
            // yet, mask it with an empty read-only dir so a sandboxed command
            // can't CREATE it (e.g. write `.zode/state.json` to self-escalate) —
            // matching the macOS deny rule, which covers missing paths too.
            let empty_ro = empty_ro_mask_dir();
            for base in self.protected_bases() {
                for sub in PROTECTED_SUBDIRS {
                    let path = format!("{base}/{sub}");
                    // Existing dir → ro-bind itself. Missing dir → mask with an
                    // empty ro dir so it can't be created. If that mask can't be
                    // built, ro-bind the (nonexistent) path to ITSELF: bwrap then
                    // errors on the missing source and the command fails — fail
                    // CLOSED, never running with the protected path writable.
                    let source = if Path::new(&path).exists() {
                        path.clone()
                    } else {
                        empty_ro.clone().unwrap_or_else(|| path.clone())
                    };
                    args.push("--ro-bind".into());
                    args.push(source);
                    args.push(path);
                }
            }
        }
        args
    }
}

/// Path to a shared empty, read-only directory used to mask not-yet-existing
/// protected subdirs under bwrap (so the sandbox can't create them). Created
/// once on demand; `None` if it can't be created (then a missing dir simply
/// isn't masked — degrades to the prior behavior).
fn empty_ro_mask_dir() -> Option<String> {
    // Per-process path so another user can't pre-plant the mount source. It is
    // ro-bound regardless of its contents, so even a hijacked dir stays
    // read-only inside the sandbox (writes to the protected path still fail).
    let dir = std::env::temp_dir().join(format!("zode-sandbox-empty-ro-{}", std::process::id()));
    match std::fs::create_dir_all(&dir) {
        Ok(()) => Some(dir.display().to_string()),
        Err(_) => None,
    }
}

/// An [`agent_tools_code::FsSink`] that performs each filesystem mutation
/// INSIDE the OS sandbox, so the file tools' writes are kernel-enforced just
/// like shell commands — mirroring Codex's `SandboxedFileSystem`. The fs tool
/// computes the new bytes in-process (no read-before-write staleness); only the
/// mutating syscall is delegated here, where it runs the matching coreutil
/// (`cat`/`mkdir`/`mv`/`rm`/`rmdir`) under `sandbox-exec` / `bwrap` via
/// [`SandboxConfig::wrap_argv`]. The kernel — not zode — blocks an out-of-policy
/// write, so a path-resolution bug can't escape the sandbox.
#[derive(Debug)]
pub struct SandboxedFsSink {
    config: SandboxConfig,
}

impl SandboxedFsSink {
    pub fn new(config: SandboxConfig) -> Self {
        Self { config }
    }

    fn path_str(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    /// Run `argv` under the sandbox, optionally feeding `stdin_bytes`. A
    /// non-zero exit (e.g. the kernel denying the write) becomes an io::Error
    /// carrying stderr, which the fs tool surfaces like any write failure.
    async fn run(&self, argv: &[String], stdin_bytes: Option<&[u8]>) -> std::io::Result<()> {
        let wrapped = self.config.wrap_argv(argv);
        let (program, rest) = wrapped
            .split_first()
            .ok_or_else(|| std::io::Error::other("empty sandbox argv"))?;
        let mut cmd = Command::new(program);
        cmd.args(rest);
        cmd.stdin(if stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        if let Some(bytes) = stdin_bytes {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(bytes).await?;
                stdin.shutdown().await?;
            }
        }
        let output = child.wait_with_output().await?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "sandboxed fs op failed ({}): {}",
                    output.status,
                    stderr.trim()
                ),
            ))
        }
    }
}

#[async_trait]
impl agent_tools_code::FsSink for SandboxedFsSink {
    async fn write_file(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        // `sh -c 'exec cat > "$0"' <path>`: $0 is the path (no shell
        // interpolation/injection), and the bytes arrive on stdin (binary-safe).
        let argv = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "exec cat > \"$0\"".to_string(),
            Self::path_str(path),
        ];
        self.run(&argv, Some(bytes)).await
    }
    async fn create_dir(&self, path: &Path, recursive: bool) -> std::io::Result<()> {
        let mut argv = vec!["mkdir".to_string()];
        if recursive {
            argv.push("-p".to_string());
        }
        argv.push("--".to_string());
        argv.push(Self::path_str(path));
        self.run(&argv, None).await
    }
    async fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        // Renaming a path onto itself is a no-op for `fs::rename`, but `mv -f`
        // errors ("same file"); short-circuit to match.
        if from == to {
            return Ok(());
        }
        // `mv` would move INTO an existing directory target (foo → bar/foo),
        // whereas `fs::rename(from, to)` treats `to` as the literal new path.
        // Reject a directory target so the sandboxed path matches direct-rename
        // semantics (the host can stat `to`; only the mutation is sandboxed).
        if to.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "rename target is an existing directory",
            ));
        }
        let argv = vec![
            "mv".to_string(),
            "-f".to_string(),
            "--".to_string(),
            Self::path_str(from),
            Self::path_str(to),
        ];
        self.run(&argv, None).await
    }
    async fn remove(&self, path: &Path, recursive: bool, is_dir: bool) -> std::io::Result<()> {
        // Match tokio::fs semantics: empty-dir removal = rmdir, tree = rm -rf,
        // file = rm -f (the tool already stat'd the target, so it exists).
        let argv = if is_dir && !recursive {
            vec!["rmdir".to_string(), "--".to_string(), Self::path_str(path)]
        } else if recursive {
            vec![
                "rm".to_string(),
                "-rf".to_string(),
                "--".to_string(),
                Self::path_str(path),
            ]
        } else {
            vec![
                "rm".to_string(),
                "-f".to_string(),
                "--".to_string(),
                Self::path_str(path),
            ]
        };
        self.run(&argv, None).await
    }
}

/// Resolve effective sandbox settings into a config, or `None` when the
/// sandbox is disabled or can't run here. Degrades gracefully (logs + returns
/// `None`) on an unsupported OS or a missing backend, so a default-on sandbox
/// never bricks a host: on Linux that means `bwrap` must be installed.
pub fn resolve(
    cwd: &Path,
    enabled: bool,
    mode: SandboxMode,
    allow_network: bool,
    writable_roots: &[PathBuf],
    exclude_slash_tmp: bool,
    exclude_tmpdir_env_var: bool,
) -> Option<SandboxConfig> {
    if !enabled {
        return None;
    }
    let config = match SandboxConfig::new(cwd, mode, allow_network, writable_roots) {
        Ok(c) => c.with_temp_policy(exclude_slash_tmp, exclude_tmpdir_env_var),
        Err(e) => {
            tracing::warn!("sandbox disabled: {e}");
            return None;
        }
    };
    if config.os == SandboxOs::Linux && !binary_on_path("bwrap") {
        tracing::warn!(
            "sandbox disabled: `bwrap` (bubblewrap) not found on PATH — install it to sandbox shell commands"
        );
        return None;
    }
    Some(config)
}

/// True if `name` is an executable on `$PATH`. Used to avoid enabling a
/// sandbox backend that isn't installed.
fn binary_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let p = dir.join(name);
                p.is_file() || p.is_symlink()
            })
        })
        .unwrap_or(false)
}

/// Canonicalize a path, falling back to the input if it doesn't resolve (the
/// OS sandbox matches real paths after symlink resolution).
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Escape a string for a Scheme string literal (sandbox profile syntax):
/// backslash and double-quote.
fn scheme_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
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
    /// Inner description + a note about the sandbox and the escape flag, built
    /// once (the `Tool::description` signature returns `&str`).
    description: String,
    /// Approval gate used to ask the user whether to ESCALATE (re-run outside
    /// the sandbox) when a sandboxed command fails on a sandbox restriction.
    gate: Arc<dyn ApprovalGate>,
}

impl SandboxedBashTool {
    pub fn new(inner: Arc<dyn Tool>, config: SandboxConfig, gate: Arc<dyn ApprovalGate>) -> Self {
        let mode = match config.mode {
            SandboxMode::ReadOnly => "read-only (no filesystem writes)",
            SandboxMode::WorkspaceWrite => "writes confined to the workspace",
        };
        let net = if config.allow_network {
            "network allowed"
        } else {
            "network denied"
        };
        let description = format!(
            "{}\n\nThis command runs in an OS sandbox ({mode}; {net}). If it \
             genuinely needs network or to write outside the workspace, set \
             `{SANDBOX_PERMISSIONS_FLAG}: \"require_escalated\"` (with a short \
             `{JUSTIFICATION_FLAG}`) to request running it outside the sandbox — \
             the user is asked to authorize the escape.",
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
fn looks_like_sandbox_denial(result: &serde_json::Value) -> bool {
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
fn wants_escape(input: &serde_json::Value) -> bool {
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
        // If the model requested (and the gate authorized) a sandbox escape,
        // strip the control keys and run the command raw — no sandbox wrap.
        let escape = wants_escape(&input);
        if let Some(obj) = input.as_object_mut() {
            obj.remove(SANDBOX_PERMISSIONS_FLAG);
            obj.remove(JUSTIFICATION_FLAG);
            obj.remove(ESCAPE_FLAG);
        }
        if escape {
            return self.inner.call(ctx, input).await;
        }

        // Keep the raw input so we can re-run unsandboxed if the user escalates.
        let raw_input = input.clone();
        let mut wrapped = input;
        if let Some(cmd) = wrapped.get("command").and_then(|v| v.as_str()) {
            wrapped["command"] = serde_json::Value::String(shell_join(&self.config.wrap(cmd)));
        }
        let result = self.inner.call(ctx, wrapped).await?;

        // The command ran (1st prompt already approved it). If it failed on
        // what looks like a sandbox restriction, ASK the user whether to
        // ESCALATE — re-run the command OUTSIDE the sandbox (提权). This is a
        // second, distinct authorization.
        if looks_like_sandbox_denial(&result) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(mode: SandboxMode, net: bool) -> SandboxConfig {
        SandboxConfig {
            os: SandboxOs::MacOs,
            cwd: PathBuf::from("/work/proj"),
            mode,
            allow_network: net,
            writable_roots: vec![],
            exclude_slash_tmp: false,
            exclude_tmpdir_env_var: false,
        }
    }

    #[test]
    fn workspace_write_confines_writes_to_cwd_and_denies_network() {
        let p = cfg(SandboxMode::WorkspaceWrite, false).macos_profile();
        assert!(p.contains("(deny file-write*)"));
        assert!(p.contains("/work/proj"));
        assert!(p.contains("/tmp"));
        assert!(p.contains("(deny network*)"), "network denied by default");
        // standard devices stay writable so commands work
        assert!(p.contains("/dev/null"));
    }

    #[test]
    fn read_only_denies_all_writes_except_devices() {
        let p = cfg(SandboxMode::ReadOnly, false).macos_profile();
        assert!(p.contains("(deny file-write*)"));
        assert!(p.contains("/dev/null"), "devices still writable");
        // no workspace write-allow in read-only mode
        assert!(
            !p.contains("(allow file-write* (subpath \"/work/proj\"))"),
            "read-only must not allow writing the workspace: {p}"
        );
    }

    #[test]
    fn allow_network_omits_the_deny() {
        let p = cfg(SandboxMode::WorkspaceWrite, true).macos_profile();
        assert!(!p.contains("(deny network*)"), "network allowed");
    }

    #[test]
    fn extra_writable_roots_are_allowed() {
        let mut c = cfg(SandboxMode::WorkspaceWrite, false);
        c.writable_roots = vec![PathBuf::from("/data/cache")];
        let p = c.macos_profile();
        assert!(
            p.contains("/data/cache"),
            "extra writable root allowed: {p}"
        );
    }

    fn linux_cfg(mode: SandboxMode, net: bool) -> SandboxConfig {
        SandboxConfig {
            os: SandboxOs::Linux,
            cwd: PathBuf::from("/tmp"), // exists, so it binds
            mode,
            allow_network: net,
            writable_roots: vec![],
            exclude_slash_tmp: false,
            exclude_tmpdir_env_var: false,
        }
    }

    #[test]
    fn linux_unshares_net_by_default_and_runs_command() {
        let args = linux_cfg(SandboxMode::WorkspaceWrite, false).wrap("echo hi");
        assert_eq!(args[0], "bwrap");
        assert!(args.iter().any(|a| a == "--unshare-net"), "network dropped");
        assert!(args.iter().any(|a| a == "--bind"), "workspace bound rw");
        assert_eq!(args.last().unwrap(), "echo hi");
    }

    #[test]
    fn linux_read_only_has_no_rw_bind() {
        let args = linux_cfg(SandboxMode::ReadOnly, false).wrap("ls");
        assert!(!args.iter().any(|a| a == "--bind"), "read-only: no rw bind");
        assert!(args.iter().any(|a| a == "--ro-bind"));
    }

    #[test]
    fn linux_allow_network_keeps_net() {
        let args = linux_cfg(SandboxMode::WorkspaceWrite, true).wrap("curl x");
        assert!(!args.iter().any(|a| a == "--unshare-net"), "net kept");
    }

    #[test]
    fn mode_parse() {
        assert_eq!(SandboxMode::parse("read-only"), SandboxMode::ReadOnly);
        assert_eq!(SandboxMode::parse("readonly"), SandboxMode::ReadOnly);
        assert_eq!(
            SandboxMode::parse("workspace-write"),
            SandboxMode::WorkspaceWrite
        );
        assert_eq!(SandboxMode::parse("nonsense"), SandboxMode::WorkspaceWrite);
    }

    #[test]
    fn scheme_escape_handles_quotes_and_backslashes() {
        assert_eq!(scheme_escape(r#"a"b\c"#), r#"a\"b\\c"#);
        // A malicious cwd can't break out of the string literal.
        let mut c = cfg(SandboxMode::WorkspaceWrite, false);
        c.cwd = PathBuf::from(r#"/x") (allow file-write*) ;"#);
        let p = c.macos_profile();
        assert!(p.contains(r#"\""#), "quote must be escaped in profile");
    }

    /// Mock tool that records the `command` input it was called with.
    #[derive(Debug, Default)]
    struct RecordingTool {
        seen: std::sync::Mutex<Option<serde_json::Value>>,
    }
    #[async_trait]
    impl Tool for RecordingTool {
        fn name(&self) -> &str {
            "Bash"
        }
        fn description(&self) -> &str {
            "run a shell command"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}})
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
            Ok(serde_json::json!({}))
        }
    }

    fn allow_gate() -> Arc<dyn ApprovalGate> {
        Arc::new(crate::approval::BypassGate) // auto-approves (AllowOnce)
    }

    #[tokio::test]
    async fn escape_flag_runs_raw_and_is_stripped() {
        let rec = Arc::new(RecordingTool::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(rec.clone(), cfg, allow_gate());
        let ctx = ToolUseContext::new(std::env::temp_dir());

        // Without the escape flag → command is wrapped under the sandbox.
        tool.call(&ctx, serde_json::json!({"command": "echo hi"}))
            .await
            .unwrap();
        let wrapped = rec.seen.lock().unwrap().clone().unwrap();
        let cmd = wrapped["command"].as_str().unwrap();
        assert!(
            cmd.contains("sandbox-exec") || cmd.contains("bwrap"),
            "command should be sandbox-wrapped: {cmd}"
        );

        // With the escape flag → command runs raw and the flag is stripped.
        tool.call(
            &ctx,
            serde_json::json!({"command": "echo hi", ESCAPE_FLAG: true}),
        )
        .await
        .unwrap();
        let raw = rec.seen.lock().unwrap().clone().unwrap();
        assert_eq!(raw["command"].as_str().unwrap(), "echo hi", "ran raw");
        assert!(raw.get(ESCAPE_FLAG).is_none(), "escape flag stripped");
    }

    #[test]
    fn input_schema_advertises_sandbox_permissions_and_justification() {
        let rec = Arc::new(RecordingTool::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(rec, cfg, allow_gate());
        let schema = tool.input_schema();
        let perms = &schema["properties"][SANDBOX_PERMISSIONS_FLAG];
        assert!(
            perms.is_object(),
            "sandbox_permissions advertised: {schema}"
        );
        let variants: Vec<&str> = perms["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(variants.contains(&"use_default"));
        assert!(variants.contains(&"require_escalated"));
        assert!(schema["properties"].get(JUSTIFICATION_FLAG).is_some());
        // The legacy boolean is no longer advertised.
        assert!(schema["properties"].get(ESCAPE_FLAG).is_none());
    }

    #[tokio::test]
    async fn sandbox_permissions_require_escalated_runs_raw() {
        let rec = Arc::new(RecordingTool::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(rec.clone(), cfg, allow_gate());
        let ctx = ToolUseContext::new(std::env::temp_dir());
        tool.call(
            &ctx,
            json!({
                "command": "curl example.com",
                SANDBOX_PERMISSIONS_FLAG: "require_escalated",
                JUSTIFICATION_FLAG: "needs the network",
            }),
        )
        .await
        .unwrap();
        let seen = rec.seen.lock().unwrap().clone().unwrap();
        assert_eq!(
            seen["command"].as_str().unwrap(),
            "curl example.com",
            "ran raw"
        );
        assert!(
            seen.get(SANDBOX_PERMISSIONS_FLAG).is_none(),
            "control key stripped"
        );
        assert!(
            seen.get(JUSTIFICATION_FLAG).is_none(),
            "justification stripped"
        );
    }

    #[test]
    fn protected_metadata_dirs_stay_read_only_in_workspace_write() {
        let p = cfg(SandboxMode::WorkspaceWrite, false).macos_profile();
        // cwd is writable...
        assert!(p.contains("(allow file-write* (subpath \"/work/proj\"))"));
        // ...but .git and .zode under it are carved back out (deny AFTER allow).
        assert!(
            p.contains("(deny file-write* (subpath \"/work/proj/.git\"))"),
            ".git protected: {p}"
        );
        assert!(
            p.contains("(deny file-write* (subpath \"/work/proj/.zode\"))"),
            ".zode protected: {p}"
        );
        let allow_at = p
            .find("(allow file-write* (subpath \"/work/proj\"))")
            .unwrap();
        let deny_at = p
            .find("(deny file-write* (subpath \"/work/proj/.git\"))")
            .unwrap();
        assert!(
            deny_at > allow_at,
            "deny must follow allow for last-match-wins"
        );
    }

    #[test]
    fn with_cwd_rebases_protected_paths_to_the_new_dir() {
        let c = cfg(SandboxMode::WorkspaceWrite, false); // cwd /work/proj
        assert!(c
            .protected_paths()
            .iter()
            .any(|p| p == Path::new("/work/proj/.git")));
        // A resumed tab in another repo must protect THAT repo's metadata.
        let rebased = c.with_cwd(Path::new("/elsewhere/repo"));
        assert!(rebased
            .protected_paths()
            .iter()
            .any(|p| p == Path::new("/elsewhere/repo/.zode")));
        assert!(
            !rebased
                .protected_paths()
                .iter()
                .any(|p| p.starts_with("/work/proj")),
            "old cwd no longer protected after rebase"
        );
    }

    #[test]
    fn exclude_slash_tmp_drops_tmp_from_writable_roots() {
        let with_tmp = cfg(SandboxMode::WorkspaceWrite, false);
        assert!(with_tmp.writable_dirs().iter().any(|d| d == "/tmp"));
        let without = with_tmp.with_temp_policy(true, true);
        assert!(
            !without.writable_dirs().iter().any(|d| d == "/tmp"),
            "exclude_slash_tmp drops /tmp: {:?}",
            without.writable_dirs()
        );
    }

    /// Tool that fails (sandbox-denied) for the wrapped command but succeeds
    /// for the raw one; records every command it received.
    #[derive(Debug, Default)]
    struct DenyThenRecord {
        calls: std::sync::Mutex<Vec<String>>,
    }
    #[async_trait]
    impl Tool for DenyThenRecord {
        fn name(&self) -> &str {
            "Bash"
        }
        fn description(&self) -> &str {
            "x"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {"command": {"type": "string"}}})
        }
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::Mutating
        }
        async fn call(
            &self,
            _ctx: &ToolUseContext,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, AgentError> {
            let cmd = input["command"].as_str().unwrap_or("").to_string();
            let sandboxed = cmd.contains("sandbox-exec") || cmd.contains("bwrap");
            self.calls.lock().unwrap().push(cmd);
            if sandboxed {
                Ok(json!({"exit_code": 1, "stderr": "Operation not permitted"}))
            } else {
                Ok(json!({"exit_code": 0, "stdout": "ok"}))
            }
        }
    }

    #[tokio::test]
    async fn sandbox_denial_offers_escalation_and_reruns_raw() {
        let inner = Arc::new(DenyThenRecord::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(inner.clone(), cfg, allow_gate());
        let ctx = ToolUseContext::new(std::env::temp_dir());

        let out = tool
            .call(&ctx, json!({"command": "echo hi"}))
            .await
            .unwrap();
        // The escalation (auto-approved) re-ran the raw command, which succeeded.
        assert_eq!(out["exit_code"], 0, "escalated raw run returned success");
        let calls = inner.calls.lock().unwrap();
        assert_eq!(
            calls.len(),
            2,
            "ran sandboxed, then escalated raw: {calls:?}"
        );
        assert!(
            calls[0].contains("sandbox-exec") || calls[0].contains("bwrap"),
            "first run was sandboxed"
        );
        assert_eq!(calls[1], "echo hi", "re-run uses the original command");
    }

    #[tokio::test]
    async fn non_sandbox_failure_does_not_escalate() {
        // A plain failure (no sandbox signature) must not trigger escalation.
        #[derive(Debug, Default)]
        struct AlwaysFail(std::sync::Mutex<usize>);
        #[async_trait]
        impl Tool for AlwaysFail {
            fn name(&self) -> &str {
                "Bash"
            }
            fn description(&self) -> &str {
                "x"
            }
            fn input_schema(&self) -> serde_json::Value {
                json!({"type": "object", "properties": {"command": {"type": "string"}}})
            }
            fn safety_class(&self) -> SafetyClass {
                SafetyClass::Mutating
            }
            async fn call(
                &self,
                _ctx: &ToolUseContext,
                _input: serde_json::Value,
            ) -> Result<serde_json::Value, AgentError> {
                *self.0.lock().unwrap() += 1;
                Ok(json!({"exit_code": 1, "stderr": "command not found: frobnicate"}))
            }
        }
        let inner = Arc::new(AlwaysFail::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(inner.clone(), cfg, allow_gate());
        let ctx = ToolUseContext::new(std::env::temp_dir());
        tool.call(&ctx, json!({"command": "frobnicate"}))
            .await
            .unwrap();
        assert_eq!(
            *inner.0.lock().unwrap(),
            1,
            "ran once, no escalation re-run"
        );
    }

    #[test]
    fn denial_detection_matches_sandbox_signs_but_rejects_exec_errors() {
        // Real sandbox denials.
        assert!(looks_like_sandbox_denial(
            &json!({"exit_code": 1, "stderr": "touch: /x: Read-only file system"})
        ));
        assert!(looks_like_sandbox_denial(
            &json!({"exit_code": 1, "stderr": "curl: (6) Could not resolve host: example.com"})
        ));
        // exit 0 is never a denial.
        assert!(!looks_like_sandbox_denial(
            &json!({"exit_code": 0, "stderr": "sandbox"})
        ));
        // 126/127 are exec failures — NOT sandbox denials — even though 126
        // prints "permission denied" (which would otherwise keyword-match).
        assert!(!looks_like_sandbox_denial(
            &json!({"exit_code": 126, "stderr": "bash: ./run: Permission denied"})
        ));
        assert!(!looks_like_sandbox_denial(
            &json!({"exit_code": 127, "stderr": "bash: frobnicate: command not found"})
        ));
    }

    #[tokio::test]
    async fn sandboxed_fs_sink_writes_inside_but_kernel_blocks_protected_git() {
        use agent_tools_code::FsSink;
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let config = match SandboxConfig::new(cwd, SandboxMode::WorkspaceWrite, false, &[]) {
            Ok(c) => c,
            Err(_) => return, // unsupported OS — nothing to prove here
        };
        if config.os == SandboxOs::Linux && !binary_on_path("bwrap") {
            return; // backend unavailable
        }
        // .git must already exist so the denied write fails on the SANDBOX, not
        // on a missing parent dir.
        std::fs::create_dir_all(cwd.join(".git")).unwrap();
        let sink = SandboxedFsSink::new(config);

        // A normal write inside the workspace goes through the sandboxed `cat`.
        let inside = canonical(cwd).join("ok.txt");
        sink.write_file(&inside, b"hello")
            .await
            .expect("write inside the workspace must succeed");
        assert_eq!(std::fs::read_to_string(&inside).unwrap(), "hello");

        // A write into .git is blocked by the KERNEL (deny-after-allow in the
        // profile), not by zode — proving the file-tool write is OS-enforced.
        let protected = canonical(cwd).join(".git").join("HACK");
        let err = sink
            .write_file(&protected, b"x")
            .await
            .expect_err("kernel must deny the write into .git");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{err}");
        assert!(
            !protected.exists(),
            "the protected file must not have been created"
        );
    }

    #[test]
    fn resolve_disabled_is_none() {
        assert!(resolve(
            Path::new("/tmp"),
            false,
            SandboxMode::WorkspaceWrite,
            false,
            &[],
            false,
            false
        )
        .is_none());
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
        let dir = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap();
        if let Ok(cfg) = SandboxConfig::for_current_os(dir.path()) {
            assert_eq!(cfg.cwd, real);
        }
    }
}
