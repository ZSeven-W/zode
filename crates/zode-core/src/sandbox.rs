//! Sandbox mode (`--sandbox`). Wraps shell commands so they run under an OS
//! sandbox. Modeled on Codex (`SandboxPolicy`: read-only / workspace-write /
//! full-access) and Claude Code (network denied by default, configurable
//! writable roots): fs tools are already cwd-confined by WorkspacePolicy; this
//! covers Bash / BashRun. macOS uses sandbox-exec (Seatbelt); Linux uses
//! bwrap; Windows Tier 1 uses restricted tokens and capability ACLs, while
//! optional Tier 2 launches commands in an AppContainer without network capabilities.
//!
//! Two modes:
//! - **read-only** — deny ALL filesystem writes (safe exploration).
//! - **workspace-write** — writes confined to cwd + tmp + extra writable roots.
//!
//! On macOS/Linux and Windows Tier 2, outbound **network is denied by default**
//! and re-enabled only with `allow_network`; Windows Tier 1 reports network as
//! unenforced. The
//! standard character devices (`/dev/null`, `/dev/tty`, …) stay writable so
//! ordinary commands (`cmd 2>/dev/null`, terminal output) still work.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use tokio::process::Command;

use crate::approval::{Approval, ApprovalGate};
use crate::error::CoreError;

#[cfg(windows)]
pub mod windows;
#[path = "sandbox/windows-policy.rs"]
pub mod windows_policy;

#[path = "sandbox/fs.rs"]
mod fs;
#[path = "sandbox/tool.rs"]
mod tool;

#[cfg(all(test, unix))]
use fs::backend_available;
pub(crate) use fs::read_denied_dirs;
use fs::{
    binary_on_path, canonical, empty_ro_mask_dir, sandbox_unavailable, scheme_escape, shell_join,
};
pub use fs::{
    overlay_profile, resolve, resolve_with_overrides, resolve_with_settings, select_profile,
    SandboxOverrides, SandboxedFsSink,
};
#[cfg(all(test, unix))]
use tool::looks_like_sandbox_denial;
pub use tool::{
    apply_sandbox, SandboxedBashTool, ESCAPE_FLAG, JUSTIFICATION_FLAG, SANDBOX_PERMISSIONS_FLAG,
};

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
    Windows,
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
    windows_tier: windows_policy::ResolvedWindowsTier,
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
        } else if cfg!(windows) {
            SandboxOs::Windows
        } else {
            return Err(CoreError::Other(
                "--sandbox is only supported on macOS, Linux, and Windows".into(),
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
            windows_tier: windows_policy::parse_windows_tier(None),
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
    pub fn with_windows_tier(mut self, tier: Option<&str>) -> Self {
        self.windows_tier = windows_policy::parse_windows_tier(tier);
        self
    }
    pub fn windows_tier_notice(&self) -> Option<&'static str> {
        self.windows_tier.notice
    }
    pub fn is_windows_tier_one(&self) -> bool {
        self.os == SandboxOs::Windows
    }
    pub fn is_windows_tier_two(&self) -> bool {
        #[cfg(windows)]
        {
            return self.windows_network_enforced().unwrap_or(false);
        }
        #[cfg(not(windows))]
        false
    }
    #[cfg(windows)]
    fn windows_network_enforced(&self) -> Result<bool, String> {
        if self.os != SandboxOs::Windows || self.allow_network {
            return Ok(false);
        }
        Ok(windows_policy::resolve_network_enforcement(
            self.windows_tier.tier,
        ))
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

    #[cfg(windows)]
    fn windows_writable_roots(&self) -> Vec<PathBuf> {
        if self.mode == SandboxMode::ReadOnly {
            return Vec::new();
        }
        let mut roots = vec![self.cwd.clone()];
        roots.extend(self.writable_roots.iter().cloned());
        roots.push(std::env::temp_dir());
        roots.sort();
        roots.dedup();
        roots
    }

    #[cfg(windows)]
    fn windows_canary_path(&self) -> Option<PathBuf> {
        let candidates = [
            std::env::var_os("USERPROFILE").map(PathBuf::from),
            std::env::var_os("SystemRoot").map(PathBuf::from),
        ];
        candidates.into_iter().flatten().find_map(|root| {
            let path = root.join(format!("zode-sandbox-canary-{}", std::process::id()));
            (!self
                .windows_writable_roots()
                .iter()
                .any(|allowed| path.starts_with(allowed)))
            .then_some(path)
        })
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
        let argv = if self.os == SandboxOs::Windows {
            vec![
                "cmd.exe".to_string(),
                "/D".to_string(),
                "/S".to_string(),
                "/C".to_string(),
                command.to_string(),
            ]
        } else {
            vec!["/bin/sh".to_string(), "-c".to_string(), command.to_string()]
        };
        self.wrap_argv(&argv)
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
            SandboxOs::Windows => {
                #[cfg(windows)]
                {
                    windows::prepare_argv(self, argv).unwrap_or_else(|error| {
                        vec![
                            "cmd.exe".into(),
                            "/D".into(),
                            "/C".into(),
                            format!("echo zode sandbox setup failed: {error} 1>&2 & exit /b 1"),
                        ]
                    })
                }
                #[cfg(not(windows))]
                unreachable!("Windows backend cannot run on a non-Windows target")
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
        #[cfg(windows)]
        if self.os == SandboxOs::Windows {
            return windows::verify(self).await;
        }
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

// These exercise the Unix backends (Seatbelt profile / bwrap prefix) and their
// assertions are inherently Unix-shaped, so they are gated to `unix`. The
// Windows Tier 1 backend is covered by the `windows` / `windows_policy` module
// tests and the opt-in `windows-sandbox-it` integration test instead.
#[cfg(all(test, unix))]
mod tests {
    include!("sandbox/fs-supervision-tests.rs");
    include!("sandbox/tests-one.rs");
    include!("sandbox/tests-two.rs");
}
