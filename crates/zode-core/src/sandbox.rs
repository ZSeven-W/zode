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
    /// Opt-in "strict read": hide a curated set of credential/secret dirs from
    /// READS too (a coding agent reads almost everything, so this is OFF by
    /// default — turning it on protects `~/.ssh`, `~/.aws`, the zode config, …).
    restrict_reads: bool,
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
            restrict_reads: false,
        })
    }

    pub fn mode(&self) -> SandboxMode {
        self.mode
    }
    pub fn allow_network(&self) -> bool {
        self.allow_network
    }
    pub fn restrict_reads(&self) -> bool {
        self.restrict_reads
    }
    /// Return a copy with strict-read on/off (opt-in credential read hiding).
    pub fn with_restrict_reads(mut self, restrict: bool) -> Self {
        self.restrict_reads = restrict;
        self
    }
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
    /// Extra writable roots (workspace-write mode). Used to widen the file-tool
    /// `WorkspacePolicy` to match the shell sandbox.
    pub fn writable_roots(&self) -> &[PathBuf] {
        &self.writable_roots
    }
    /// Human-readable summary of WHERE workspace-write allows writes, e.g.
    /// "the workspace + /tmp + $TMPDIR". Used for user-facing text (TUI status
    /// line, tool description, system prompt). Naming /tmp explicitly matters:
    /// the default policy keeps it writable (Codex-aligned), and a message
    /// claiming "confined to the workspace" reads as a broken sandbox the
    /// moment a user tests it with /tmp.
    pub fn write_scope_summary(&self) -> String {
        let mut s = String::from("the workspace");
        for r in &self.writable_roots {
            s.push_str(&format!(" + {}", r.display()));
        }
        if !self.exclude_slash_tmp {
            s.push_str(" + /tmp");
        }
        if !self.exclude_tmpdir_env_var
            && std::env::var_os("TMPDIR").is_some_and(|t| Path::new(&t).is_absolute())
        {
            s.push_str(" + $TMPDIR");
        }
        s
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

    /// Runtime effectiveness probe, run when the sandbox is (re-)enabled.
    /// Static checks (OS type, `bwrap` on PATH) can't prove the sandbox
    /// actually confines on THIS host — e.g. a kernel with unprivileged user
    /// namespaces disabled runs `bwrap` from PATH but can't sandbox, and an OS
    /// update could regress Seatbelt semantics. Two probes, FAIL-CLOSED:
    ///
    /// 1. a trivial command must SUCCEED under the wrap (backend can run),
    /// 2. a canary write OUTSIDE every writable root must be DENIED, and
    /// 3. with network denied, a sandboxed client must FAIL to reach an
    ///    in-process localhost listener (no external traffic involved) —
    ///    catching hosts/OS versions where the write rules enforce but the
    ///    network rules do not.
    ///
    /// If (2) or (3) succeeds the sandbox is silently ineffective; refuse to
    /// run with a false sense of isolation instead. Returns `Ok(())` when
    /// enforcement is proven, or when a probe has nothing to work with (no
    /// probe location / no client tool).
    pub async fn verify(&self) -> Result<(), CoreError> {
        if !self.probe("true").await? {
            return Err(sandbox_unavailable(
                "the backend failed to run a probe command (on Linux this \
                 usually means the kernel disallows unprivileged user \
                 namespaces, so `bwrap` cannot sandbox)",
            ));
        }
        if let Some(canary) = self.canary_path() {
            let touch = shell_join(&["touch".to_string(), canary.display().to_string()]);
            if self.probe(&touch).await? {
                let _ = std::fs::remove_file(&canary);
                return Err(CoreError::Other(format!(
                    "the sandbox is INEFFECTIVE on this host: a probe write outside \
                     the writable roots ({}) was NOT blocked by the OS backend. \
                     Refusing to run with a false sense of isolation — fix the \
                     backend, or pass `--no-sandbox` (or set `\"sandbox\": {{ \
                     \"enabled\": false }}` in config) to explicitly run WITHOUT \
                     isolation.",
                    canary.display()
                )));
            }
        }
        if !self.allow_network && self.network_canary_leaked().await? == Some(true) {
            return Err(CoreError::Other(
                "the sandbox does NOT block network on this host even though \
                 network is denied (a sandboxed probe reached a local listener). \
                 Refusing to run with a false sense of isolation — either allow \
                 network honestly (`/sandbox network on` or `\"sandbox\": {{ \
                 \"network\": true }}` in config), or pass `--no-sandbox` to \
                 explicitly run WITHOUT isolation."
                    .into(),
            ));
        }
        Ok(())
    }

    /// Network canary: bind an in-process listener on 127.0.0.1 and run a tiny
    /// client under the wrap. The client's exit code is irrelevant — what
    /// counts is whether a TCP handshake reached the listener's accept queue
    /// (a completed handshake stays queued after the client exits). No traffic
    /// ever leaves the machine. `Ok(None)` when there is no client tool to
    /// probe with.
    async fn network_canary_leaked(&self) -> Result<Option<bool>, CoreError> {
        if !binary_on_path("curl") {
            return Ok(None);
        }
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| CoreError::Other(format!("sandbox network probe: bind failed: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| CoreError::Other(format!("sandbox network probe: no local addr: {e}")))?
            .port();
        let _ = self
            .probe(&format!(
                "curl -s -m 2 -o /dev/null http://127.0.0.1:{port}/"
            ))
            .await?;
        let leaked = tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept())
            .await
            .is_ok();
        Ok(Some(leaked))
    }

    /// Run `sh -c <command>` under the sandbox wrap, discarding output.
    /// `Ok(true)` = exit 0. Spawn failures and a 10s timeout are hard errors
    /// (a probe that can't run proves nothing — fail closed at the caller).
    async fn probe(&self, command: &str) -> Result<bool, CoreError> {
        let argv = self.wrap(command);
        let (program, rest) = argv
            .split_first()
            .ok_or_else(|| CoreError::Other("empty sandbox probe argv".into()))?;
        let mut cmd = Command::new(program);
        cmd.args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let status = tokio::time::timeout(std::time::Duration::from_secs(10), cmd.status())
            .await
            .map_err(|_| CoreError::Other("the sandbox probe timed out".into()))?
            .map_err(|e| CoreError::Other(format!("the sandbox probe failed to spawn: {e}")))?;
        Ok(status.success())
    }

    /// Where the canary probe tries to write: a per-process file in $HOME,
    /// which is user-writable WITHOUT the sandbox but outside every writable
    /// root — so its success proves non-enforcement, not a permission quirk.
    /// `None` when no such location exists (no home dir, or the workspace IS
    /// the home dir, making it a writable root).
    fn canary_path(&self) -> Option<PathBuf> {
        let home = canonical(&dirs::home_dir()?);
        let path = home.join(format!(".zode-sandbox-canary-{}", std::process::id()));
        if self.mode == SandboxMode::WorkspaceWrite
            && self
                .writable_dirs()
                .iter()
                .any(|d| path.starts_with(Path::new(d)))
        {
            return None;
        }
        Some(path)
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
        // Strict-read: hide credential dirs from READS too (any mode). Emitted
        // last so these denies win over the `(allow default)` baseline.
        if self.restrict_reads {
            for dir in read_denied_dirs() {
                let path = scheme_escape(&dir.to_string_lossy());
                p.push_str(&format!("(deny file-read* (subpath \"{path}\"))"));
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
        // Strict-read (any mode): overlay an empty tmpfs over each credential
        // dir so its real contents are hidden from reads. Only mask dirs that
        // EXIST — `--tmpfs` on an absent path makes bwrap fail to build the
        // mountpoint inside the sealed ro-root (breaking every shell command),
        // and a missing dir has nothing to hide anyway.
        if self.restrict_reads {
            for dir in read_denied_dirs() {
                if dir.is_dir() {
                    args.push("--tmpfs".into());
                    args.push(dir.to_string_lossy().into_owned());
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

/// Resolve effective sandbox settings into a config. `Ok(None)` ONLY when the
/// sandbox is explicitly disabled (`enabled == false`).
///
/// FAIL-CLOSED: when the sandbox is requested but can't be established here (an
/// unsupported OS, or a missing backend like `bwrap` on Linux), this returns
/// `Err` instead of silently running the agent UNCONFINED. The caller surfaces
/// the error and stops; the user installs the backend or opts out explicitly
/// with `--no-sandbox`. (Previously this degraded to `None` = no isolation,
/// which is a silent security hole when the sandbox was supposed to be on.)
pub fn resolve(
    cwd: &Path,
    enabled: bool,
    mode: SandboxMode,
    allow_network: bool,
    writable_roots: &[PathBuf],
    exclude_slash_tmp: bool,
    exclude_tmpdir_env_var: bool,
) -> Result<Option<SandboxConfig>, CoreError> {
    if !enabled {
        return Ok(None);
    }
    let config = SandboxConfig::new(cwd, mode, allow_network, writable_roots)
        .map_err(|e| sandbox_unavailable(&e.to_string()))?
        .with_temp_policy(exclude_slash_tmp, exclude_tmpdir_env_var);
    backend_available(config.os, binary_on_path("bwrap"))?;
    Ok(Some(config))
}

/// Resolve a config for ENABLING the sandbox at runtime (`/sandbox on` and the
/// mode / network toggles) from the persisted `sandbox` config section, so a
/// runtime toggle honors the same `writableRoots` / `excludeSlashTmp` /
/// `excludeTmpdirEnvVar` / `restrictReads` the startup path applies.
/// Previously the toggle rebuilt from bare defaults, silently re-widening /tmp
/// (and dropping extra roots) for a user who had configured otherwise. `mode`
/// and `allow_network` come from the caller — they are session state owned by
/// the toggle itself, not the config.
pub fn resolve_with_settings(
    cwd: &Path,
    settings: &crate::config::SandboxSettings,
    mode: SandboxMode,
    allow_network: bool,
) -> Result<SandboxConfig, CoreError> {
    let roots: Vec<PathBuf> = settings.writable_roots.iter().map(PathBuf::from).collect();
    let config = resolve(
        cwd,
        true,
        mode,
        allow_network,
        &roots,
        settings.exclude_slash_tmp.unwrap_or(false),
        settings.exclude_tmpdir_env_var.unwrap_or(false),
    )?
    // `resolve` returns `Ok(None)` only when `enabled == false`.
    .expect("resolve(enabled=true) always yields a config");
    Ok(config.with_restrict_reads(settings.restrict_reads.unwrap_or(false)))
}

/// Whether the OS sandbox backend is actually present — a PURE function so the
/// fail-closed decision is testable without the host's real backend. Linux
/// needs `bwrap` (bubblewrap); macOS has Seatbelt built in.
fn backend_available(os: SandboxOs, bwrap_present: bool) -> Result<(), CoreError> {
    if os == SandboxOs::Linux && !bwrap_present {
        return Err(sandbox_unavailable(
            "`bwrap` (bubblewrap) was not found on PATH",
        ));
    }
    Ok(())
}

/// The fail-closed error for a requested-but-unavailable sandbox. Actionable:
/// it names how to install a backend (with distro commands) and how to opt out.
fn sandbox_unavailable(reason: &str) -> CoreError {
    CoreError::Other(format!(
        "the sandbox is enabled but can't be established here: {reason}. \
         Install a backend — Linux: bubblewrap (`apt install bubblewrap` / \
         `dnf install bubblewrap` / `pacman -S bubblewrap`); macOS: Seatbelt is built in. \
         Or pass `--no-sandbox` (or set `\"sandbox\": {{ \"enabled\": false }}` in config) \
         to run WITHOUT isolation — shell commands then run unconfined."
    ))
}

/// Credential / secret directories hidden from READS in strict-read mode.
/// Deliberately curated and short — a coding agent legitimately reads almost
/// everything, so this stays high-signal (well-known credential stores) to keep
/// false positives near zero. Canonicalized to match the inode the sandbox sees.
/// A nonexistent entry is harmless (nothing to hide / mountpoint just created).
pub(crate) fn read_denied_dirs() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    [
        ".ssh",
        ".aws",
        ".gnupg",
        ".azure",
        ".kube",
        ".docker",
        ".config/gcloud",
        ".config/gh",
        // zode's own config dir holds provider API keys.
        ".zode",
    ]
    .iter()
    .map(|rel| canonical(&home.join(rel)))
    .collect()
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
            SandboxMode::ReadOnly => "read-only (no filesystem writes)".to_string(),
            SandboxMode::WorkspaceWrite => {
                format!("writes confined to {}", config.write_scope_summary())
            }
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
            restrict_reads: false,
        }
    }

    #[test]
    fn strict_read_off_by_default_allows_reads() {
        let p = cfg(SandboxMode::WorkspaceWrite, false).macos_profile();
        assert!(
            !p.contains("(deny file-read*"),
            "reads must be unrestricted by default: {p}"
        );
    }

    #[test]
    fn strict_read_denies_credential_dir_reads() {
        // Only meaningful when there's a home dir to derive credential paths.
        if dirs::home_dir().is_none() {
            return;
        }
        let macos = cfg(SandboxMode::WorkspaceWrite, false)
            .with_restrict_reads(true)
            .macos_profile();
        assert!(
            macos.contains("(deny file-read*"),
            "strict-read must emit read denies: {macos}"
        );
        assert!(macos.contains(".ssh"), "must hide ~/.ssh: {macos}");

        let mut linux = cfg(SandboxMode::WorkspaceWrite, false).with_restrict_reads(true);
        linux.os = SandboxOs::Linux;
        let args = linux.linux_bwrap_prefix();
        // Collect the dirs masked with --tmpfs.
        let mut masked = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == "--tmpfs" {
                if let Some(p) = it.next() {
                    masked.push(p.clone());
                }
            }
        }
        // B3 invariant: only EXISTING dirs are masked (--tmpfs on an absent dir
        // would make bwrap fail and break every shell command).
        for m in &masked {
            assert!(
                std::path::Path::new(m).is_dir(),
                "must not --tmpfs a nonexistent dir: {m}"
            );
        }
        // If a known credential dir exists on this host, it must be masked.
        if let Some(home) = dirs::home_dir() {
            if home.join(".ssh").is_dir() {
                assert!(
                    masked.iter().any(|m| m.ends_with(".ssh")),
                    "existing ~/.ssh must be masked: {masked:?}"
                );
            }
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
            restrict_reads: false,
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
        Arc::new(crate::approval::BypassGate) // auto-approves, NOT interactive
    }

    /// Interactive mock gate: records every approve() label, returns a fixed
    /// answer — stands in for a human at the TUI/stdin prompt.
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

    #[tokio::test]
    async fn escape_flag_runs_raw_and_is_stripped() {
        let rec = Arc::new(RecordingTool::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = SandboxedBashTool::new(rec.clone(), cfg, gate.clone());
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

        // With the escape flag (and the user approving the dedicated escape
        // prompt) → command runs raw and the flag is stripped.
        tool.call(
            &ctx,
            serde_json::json!({"command": "echo hi", ESCAPE_FLAG: true}),
        )
        .await
        .unwrap();
        let raw = rec.seen.lock().unwrap().clone().unwrap();
        assert_eq!(raw["command"].as_str().unwrap(), "echo hi", "ran raw");
        assert!(raw.get(ESCAPE_FLAG).is_none(), "escape flag stripped");
        assert_eq!(
            gate.asked.lock().unwrap().len(),
            1,
            "the escape must have its own approval prompt"
        );
    }

    #[tokio::test]
    async fn escape_is_not_honored_without_an_interactive_gate() {
        // yolo / bypass: no human to ask, so a model-requested escape must NOT
        // run raw — the sandbox config is the user's only contract there.
        let rec = Arc::new(RecordingTool::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(rec.clone(), cfg, allow_gate());
        let ctx = ToolUseContext::new(std::env::temp_dir());
        tool.call(
            &ctx,
            json!({"command": "curl example.com", SANDBOX_PERMISSIONS_FLAG: "require_escalated"}),
        )
        .await
        .unwrap();
        let seen = rec.seen.lock().unwrap().clone().unwrap();
        let cmd = seen["command"].as_str().unwrap();
        assert!(
            cmd.contains("sandbox-exec") || cmd.contains("bwrap"),
            "escape under a bypass gate must stay sandboxed: {cmd}"
        );
    }

    #[tokio::test]
    async fn escape_denied_by_user_runs_sandboxed_without_second_offer() {
        // The user says no to the escape → run sandboxed; and even if that
        // fails on a sandbox denial, do NOT immediately re-ask to escalate.
        let inner = Arc::new(DenyThenRecord::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let gate = PromptGate::new(Approval::Deny);
        let tool = SandboxedBashTool::new(inner.clone(), cfg, gate.clone());
        let ctx = ToolUseContext::new(std::env::temp_dir());
        tool.call(
            &ctx,
            json!({"command": "curl example.com", SANDBOX_PERMISSIONS_FLAG: "require_escalated"}),
        )
        .await
        .unwrap();
        let calls = inner.calls.lock().unwrap();
        assert_eq!(
            calls.len(),
            1,
            "sandboxed run only, no raw re-run: {calls:?}"
        );
        assert!(
            calls[0].contains("sandbox-exec") || calls[0].contains("bwrap"),
            "the one run was sandboxed"
        );
        assert_eq!(
            gate.asked.lock().unwrap().len(),
            1,
            "asked once for the escape, not again for escalation"
        );
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
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = SandboxedBashTool::new(rec.clone(), cfg, gate.clone());
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
        assert_eq!(
            gate.asked.lock().unwrap().len(),
            1,
            "escape authorized via its own prompt"
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
    fn write_scope_summary_names_tmp_only_when_writable() {
        // Default policy: /tmp IS writable, so user-facing text must say so —
        // claiming "confined to the workspace" reads as a broken sandbox the
        // moment a user tests it with /tmp.
        let s = cfg(SandboxMode::WorkspaceWrite, false).write_scope_summary();
        assert!(s.contains("workspace"), "{s}");
        assert!(
            s.contains("/tmp"),
            "default /tmp writability must be named: {s}"
        );
        // Excluded → not advertised.
        let s = cfg(SandboxMode::WorkspaceWrite, false)
            .with_temp_policy(true, true)
            .write_scope_summary();
        assert!(
            !s.contains("/tmp"),
            "excluded /tmp must not be advertised: {s}"
        );
        // Extra writable roots are surfaced too.
        let mut c = cfg(SandboxMode::WorkspaceWrite, false);
        c.writable_roots = vec![PathBuf::from("/data/cache")];
        assert!(c.write_scope_summary().contains("/data/cache"));
    }

    #[test]
    fn bash_tool_description_names_the_real_write_scope() {
        let rec = Arc::new(RecordingTool::default());
        let dir = tempfile::tempdir().unwrap();
        let Ok(config) = SandboxConfig::for_current_os(dir.path()) else {
            return; // unsupported OS
        };
        let tool = SandboxedBashTool::new(rec, config, allow_gate());
        assert!(
            tool.description().contains("/tmp"),
            "description must not claim workspace-only while /tmp is writable: {}",
            tool.description()
        );
    }

    #[test]
    fn resolve_with_settings_honors_config_roots_and_temp_policy() {
        let dir = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        let settings = crate::config::SandboxSettings {
            writable_roots: vec![extra.path().display().to_string()],
            exclude_slash_tmp: Some(true),
            exclude_tmpdir_env_var: Some(true),
            restrict_reads: Some(true),
            ..Default::default()
        };
        let c = match resolve_with_settings(
            dir.path(),
            &settings,
            SandboxMode::WorkspaceWrite,
            false,
        ) {
            Ok(c) => c,
            Err(_) => return, // unsupported host / missing backend
        };
        let canon_extra = canonical(extra.path());
        assert!(
            c.writable_roots().iter().any(|r| r == &canon_extra),
            "config writableRoots must survive a runtime toggle: {:?}",
            c.writable_roots()
        );
        assert!(
            !c.writable_dirs().iter().any(|d| d == "/tmp"),
            "excludeSlashTmp must survive a runtime toggle: {:?}",
            c.writable_dirs()
        );
        assert!(c.restrict_reads(), "restrictReads must be applied");
        assert_eq!(c.mode(), SandboxMode::WorkspaceWrite);
        assert!(!c.allow_network());
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
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = SandboxedBashTool::new(inner.clone(), cfg, gate.clone());
        let ctx = ToolUseContext::new(std::env::temp_dir());

        let out = tool
            .call(&ctx, json!({"command": "echo hi"}))
            .await
            .unwrap();
        // The escalation (user-approved) re-ran the raw command, which succeeded.
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
        assert_eq!(gate.asked.lock().unwrap().len(), 1, "user was asked");
    }

    #[tokio::test]
    async fn sandbox_denial_does_not_escalate_under_a_bypass_gate() {
        // yolo / bypass: an auto-answering gate must NOT "approve" the
        // escalation — the failed command stays failed and stays sandboxed.
        let inner = Arc::new(DenyThenRecord::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(inner.clone(), cfg, allow_gate());
        let ctx = ToolUseContext::new(std::env::temp_dir());

        let out = tool
            .call(&ctx, json!({"command": "echo hi"}))
            .await
            .unwrap();
        assert_eq!(out["exit_code"], 1, "sandboxed failure surfaces as-is");
        let calls = inner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "no silent unsandboxed re-run: {calls:?}");
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
        // Interactive gate, so a false escalation offer WOULD be visible here.
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = SandboxedBashTool::new(inner.clone(), cfg, gate.clone());
        let ctx = ToolUseContext::new(std::env::temp_dir());
        tool.call(&ctx, json!({"command": "frobnicate"}))
            .await
            .unwrap();
        assert_eq!(
            *inner.0.lock().unwrap(),
            1,
            "ran once, no escalation re-run"
        );
        assert!(
            gate.asked.lock().unwrap().is_empty(),
            "no escalation prompt for a non-sandbox failure"
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
    fn canary_path_is_outside_writable_roots_or_absent() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        // Workspace == home → every home path is writable; nothing provable.
        let mut at_home = cfg(SandboxMode::WorkspaceWrite, false);
        at_home.cwd = canonical(&home);
        assert!(
            at_home.canary_path().is_none(),
            "home-as-workspace must skip the canary"
        );
        // Normal workspace → canary lives in home, outside the writable roots.
        let c = cfg(SandboxMode::WorkspaceWrite, false); // cwd /work/proj
        if let Some(p) = c.canary_path() {
            assert!(p.starts_with(canonical(&home)), "{}", p.display());
            assert!(
                !c.writable_dirs()
                    .iter()
                    .any(|d| p.starts_with(Path::new(d))),
                "canary must be outside every writable root: {}",
                p.display()
            );
        }
    }

    #[tokio::test]
    async fn verify_proves_enforcement_on_a_working_host() {
        let dir = tempfile::tempdir().unwrap();
        let Ok(config) = SandboxConfig::new(dir.path(), SandboxMode::WorkspaceWrite, false, &[])
        else {
            return; // unsupported OS
        };
        if config.os == SandboxOs::Linux && !binary_on_path("bwrap") {
            return; // backend unavailable
        }
        match config.verify().await {
            // Working host: enforcement proven.
            Ok(()) => {}
            // A CI container may run bwrap without user namespaces — verify
            // must then FAIL (closed) with the actionable backend message,
            // never claim enforcement.
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("sandbox"), "{msg}");
            }
        }
    }

    #[tokio::test]
    async fn network_canary_detects_traffic_when_network_allowed() {
        // True-positive path: with network ALLOWED, the sandboxed client must
        // reach the in-process listener — proving the canary detects traffic
        // whenever the OS lets it through (the leak case verify() rejects).
        let dir = tempfile::tempdir().unwrap();
        let Ok(config) = SandboxConfig::new(dir.path(), SandboxMode::WorkspaceWrite, true, &[])
        else {
            return; // unsupported OS
        };
        if config.os == SandboxOs::Linux && !binary_on_path("bwrap") {
            return; // backend unavailable
        }
        if !config.probe("true").await.unwrap_or(false) {
            return; // backend present but can't run (e.g. CI without userns)
        }
        match config.network_canary_leaked().await {
            Ok(Some(leaked)) => assert!(leaked, "allowed network must reach the listener"),
            Ok(None) => {} // no curl on this host — nothing to prove
            Err(e) => panic!("network canary errored: {e}"),
        }
    }

    #[test]
    fn resolve_disabled_is_ok_none() {
        // Explicitly disabled → Ok(None), the ONLY way to get no sandbox.
        assert!(matches!(
            resolve(
                Path::new("/tmp"),
                false,
                SandboxMode::WorkspaceWrite,
                false,
                &[],
                false,
                false
            ),
            Ok(None)
        ));
    }

    #[test]
    fn backend_available_fails_closed_on_linux_without_bwrap() {
        // Deterministic (no dependence on the host's real backend): Linux with
        // no bwrap is the fail-closed Err path; with bwrap, or macOS, it's Ok.
        assert!(backend_available(SandboxOs::Linux, true).is_ok());
        assert!(
            backend_available(SandboxOs::Linux, false).is_err(),
            "Linux without bwrap must fail closed, not run unconfined"
        );
        assert!(
            backend_available(SandboxOs::MacOs, false).is_ok(),
            "macOS has Seatbelt built in"
        );
    }

    #[test]
    fn resolve_enabled_never_resolves_to_silent_none() {
        // Whatever the host, a REQUESTED sandbox is never a silent Ok(None):
        // Ok(Some) where a backend exists, else a fail-closed Err.
        let r = resolve(
            Path::new("/tmp"),
            true,
            SandboxMode::WorkspaceWrite,
            false,
            &[],
            false,
            false,
        );
        assert!(
            !matches!(r, Ok(None)),
            "a requested sandbox must never resolve to silent no-isolation"
        );
    }

    #[test]
    fn sandbox_unavailable_message_is_actionable() {
        let s = sandbox_unavailable("`bwrap` was not found").to_string();
        assert!(s.contains("--no-sandbox"), "{s}");
        assert!(s.contains("bubblewrap") || s.contains("bwrap"), "{s}");
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
