use super::*;

/// Path to a shared empty, read-only directory used to mask not-yet-existing
/// protected subdirs under bwrap (so the sandbox can't create them). Created
/// once on demand; `None` if it can't be created (then a missing dir simply
/// isn't masked — degrades to the prior behavior).
pub(super) fn empty_ro_mask_dir() -> Option<String> {
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
/// mutating syscall is delegated here. Unix runs the matching coreutil under
/// `sandbox-exec` / `bwrap`; Windows performs the Win32 call synchronously while
/// impersonating its restricted token. The kernel — not zode — blocks an
/// out-of-policy write, so a path-resolution bug can't escape the sandbox.
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
        #[cfg(windows)]
        if self.config.is_windows_tier_one() {
            return super::windows::run_fs_operation(
                &self.config,
                super::windows::FsOperation::Write {
                    path: path.to_path_buf(),
                    bytes: bytes.to_vec(),
                },
            )
            .await;
        }
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
        #[cfg(windows)]
        if self.config.is_windows_tier_one() {
            return super::windows::run_fs_operation(
                &self.config,
                super::windows::FsOperation::CreateDir {
                    path: path.to_path_buf(),
                    recursive,
                },
            )
            .await;
        }
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
        #[cfg(windows)]
        if self.config.is_windows_tier_one() {
            return super::windows::run_fs_operation(
                &self.config,
                super::windows::FsOperation::Rename {
                    from: from.to_path_buf(),
                    to: to.to_path_buf(),
                },
            )
            .await;
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
        #[cfg(windows)]
        if self.config.is_windows_tier_one() {
            return super::windows::run_fs_operation(
                &self.config,
                super::windows::FsOperation::Remove {
                    path: path.to_path_buf(),
                    recursive,
                    is_dir,
                },
            )
            .await;
        }
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
    Ok(config
        .with_restrict_reads(settings.restrict_reads.unwrap_or(false))
        .with_windows_tier(settings.windows_tier.as_deref()))
}

/// Whether the OS sandbox backend is actually present — a PURE function so the
/// fail-closed decision is testable without the host's real backend. Linux
/// needs `bwrap` (bubblewrap); macOS has Seatbelt built in.
pub(super) fn backend_available(os: SandboxOs, bwrap_present: bool) -> Result<(), CoreError> {
    if os == SandboxOs::Linux && !bwrap_present {
        return Err(sandbox_unavailable(
            "`bwrap` (bubblewrap) was not found on PATH",
        ));
    }
    Ok(())
}

/// The fail-closed error for a requested-but-unavailable sandbox. Actionable:
/// it names how to install a backend (with distro commands) and how to opt out.
pub(super) fn sandbox_unavailable(reason: &str) -> CoreError {
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
pub(super) fn binary_on_path(name: &str) -> bool {
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
pub(super) fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Escape a string for a Scheme string literal (sandbox profile syntax):
/// backslash and double-quote.
pub(super) fn scheme_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Quote argv into a single /bin/sh-safe command string.
pub(super) fn shell_join(args: &[String]) -> String {
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
