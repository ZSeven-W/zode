//! External agent process runner. Trust boundary notes (spec v2.3 §3.6-3.7):
//! env starts from `env_clear()` and only an allowlist is restored; prompts
//! prefer stdin (argv leaks via process lists); stdout/stderr are drained
//! concurrently (pipe deadlock); termination kills the whole process group;
//! aftermath (changed files, cache clear) is best-effort and honest about it.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent::abort::AbortController;
use agent::file_cache::FileStateCache;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::capability::PromptTransport;
use super::parser::{ExtEvent, FinalResult, StreamParser};
use super::profiles::ExternalAgentDef;

const STDERR_TAIL_BYTES: usize = 8 * 1024;
const MAX_LINE_BYTES: usize = 64 * 1024;
const KILL_GRACE: Duration = Duration::from_secs(5);

/// Env vars restored from the parent environment on every platform.
const BASE_ENV: &[&str] = &["PATH", "HOME", "TERM"];
#[cfg(windows)]
const WINDOWS_ENV: &[&str] = &["SystemRoot", "USERPROFILE", "TEMP", "PATHEXT"];

/// Loader-injection vars are refused even when explicitly allowlisted.
fn is_forbidden_env(name: &str) -> bool {
    name == "LD_PRELOAD" || name == "LD_LIBRARY_PATH" || name.starts_with("DYLD_")
}

#[derive(Debug, Clone)]
pub struct RunSpec {
    pub def: ExternalAgentDef,
    pub prompt: String,
    pub cwd: PathBuf,
    pub timeout: Duration,
    /// Extra args appended after the template (e.g. resume flags).
    pub extra_args: Vec<String>,
    /// Cleared wholesale after the run — a partial per-path invalidation
    /// would be less safe than a full drop (spec §3.7).
    pub file_cache: Option<Arc<FileStateCache>>,
}

#[derive(Debug)]
pub struct RunOutcome {
    pub result: FinalResult,
    pub exit_code: i32,
    pub duration_ms: u64,
    /// Best-effort: files whose `git status --porcelain` entry appeared or
    /// changed across the run. Misses files already dirty before the run,
    /// ignored files, and writes outside a git cwd.
    pub changed_files: Vec<String>,
}

/// Snapshot `git status --porcelain=v1 -z` as (path -> status). `None` when
/// cwd is not a git repository.
pub fn git_status_snapshot(cwd: &Path) -> Option<std::collections::HashMap<String, String>> {
    let out = std::process::Command::new("git")
        .arg("status")
        .arg("--porcelain=v1")
        .arg("-z")
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut map = std::collections::HashMap::new();
    let text = String::from_utf8_lossy(&out.stdout);
    for entry in text.split('\0').filter(|e| e.len() > 3) {
        let (status, path) = entry.split_at(3);
        map.insert(path.to_string(), status.trim().to_string());
    }
    Some(map)
}

fn diff_snapshots(
    before: &Option<std::collections::HashMap<String, String>>,
    after: &Option<std::collections::HashMap<String, String>>,
) -> Vec<String> {
    let (Some(before), Some(after)) = (before, after) else {
        return Vec::new();
    };
    let mut changed: Vec<String> = after
        .iter()
        .filter(|(path, status)| before.get(*path) != Some(status))
        .map(|(path, _)| path.clone())
        .collect();
    changed.sort();
    changed
}

/// Run an external agent CLI to completion. `on_event` receives display-plane
/// events as they stream; the control-plane outcome is returned.
pub async fn run_external(
    spec: RunSpec,
    mut on_event: impl FnMut(ExtEvent) + Send,
    abort: AbortController,
) -> Result<RunOutcome, String> {
    let started = Instant::now();
    let before = git_status_snapshot(&spec.cwd);

    let mut args: Vec<String> = spec
        .def
        .args
        .iter()
        .chain(spec.extra_args.iter())
        .cloned()
        .collect();

    // Prompt transport. The temp file (if any) is cleaned up on every exit
    // path via the `_prompt_file` RAII guard.
    struct TempPrompt(PathBuf);
    impl Drop for TempPrompt {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let mut _prompt_file: Option<TempPrompt> = None;
    let mut stdin_payload: Option<String> = None;
    match spec.def.capability.prompt_transport {
        PromptTransport::Stdin => stdin_payload = Some(spec.prompt.clone()),
        PromptTransport::Argv => {
            for a in &mut args {
                if a == "{prompt}" {
                    *a = spec.prompt.clone();
                }
            }
        }
        PromptTransport::File => {
            let dir = crate::config::ConfigManager::config_dir()
                .map_err(|e| format!("config dir unavailable: {e}"))?
                .join("tmp/extagent");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            // Sweep stale files from crashed runs (older than a day).
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let stale = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .map(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(86_400))
                        .unwrap_or(false);
                    if stale {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
            let path = dir.join(format!("prompt-{}.txt", std::process::id()));
            std::fs::write(&path, &spec.prompt).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            for a in &mut args {
                if a == "{prompt_file}" {
                    *a = path.to_string_lossy().to_string();
                }
            }
            _prompt_file = Some(TempPrompt(path));
        }
    }

    let mut cmd = tokio::process::Command::new(&spec.def.command);
    cmd.args(&args)
        .current_dir(&spec.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin_payload.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });

    // env: clear, then restore the allowlist. Forbidden loader vars are
    // dropped even if a profile tries to allowlist them.
    cmd.env_clear();
    let mut allowed: Vec<&str> = BASE_ENV.to_vec();
    #[cfg(windows)]
    allowed.extend_from_slice(WINDOWS_ENV);
    let profile_env: Vec<&String> = spec
        .def
        .auth_env
        .iter()
        .chain(spec.def.env_allow.iter())
        .collect();
    for name in profile_env {
        if !is_forbidden_env(name) {
            allowed.push(name.as_str());
        }
    }
    for name in allowed {
        if let Ok(v) = std::env::var(name) {
            cmd.env(name, v);
        }
    }

    #[cfg(unix)]
    {
        // New process group so timeout/abort can kill the whole tree.
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", spec.def.command.display()))?;
    let child_pid = child.id();

    if let (Some(payload), Some(mut stdin)) = (stdin_payload, child.stdin.take()) {
        tokio::spawn(async move {
            let _ = stdin.write_all(payload.as_bytes()).await;
            // dropping stdin closes the pipe (EOF)
        });
    }

    // Concurrent drain: stderr on its own task, stdout parsed inline.
    let stderr = child.stderr.take().expect("stderr piped");
    let stderr_task = tokio::spawn(async move {
        let mut tail: Vec<u8> = Vec::new();
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    tail.extend_from_slice(line.as_bytes());
                    if tail.len() > STDERR_TAIL_BYTES {
                        let cut = tail.len() - STDERR_TAIL_BYTES;
                        tail.drain(..cut);
                    }
                }
            }
        }
        String::from_utf8_lossy(&tail).to_string()
    });

    let stdout = child.stdout.take().expect("stdout piped");
    let mut parser = StreamParser::new(&spec.def.capability.output_protocol);
    let mut reader = BufReader::new(stdout).lines();

    let deadline = tokio::time::sleep(spec.timeout);
    tokio::pin!(deadline);
    let mut timed_out = false;
    let mut aborted = false;

    loop {
        tokio::select! {
            line = reader.next_line() => match line {
                Ok(Some(mut l)) => {
                    if l.len() > MAX_LINE_BYTES {
                        l.truncate(MAX_LINE_BYTES);
                        l.push_str(" …[truncated]");
                    }
                    for ev in parser.feed(&l) {
                        on_event(ev);
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    on_event(ExtEvent::Log(format!("stdout read error: {e}")));
                    break;
                }
            },
            _ = &mut deadline => { timed_out = true; break; }
            _ = abort.cancelled() => { aborted = true; break; }
        }
    }

    if timed_out || aborted {
        terminate(child_pid, &mut child).await;
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let stderr_tail = stderr_task.await.unwrap_or_default();

    // Aftermath, on every completed spawn: clearing the WHOLE cache beats an
    // incomplete per-path invalidation (external writes bypass fs tools).
    if let Some(cache) = &spec.file_cache {
        cache.clear();
    }
    let after = git_status_snapshot(&spec.cwd);
    let changed_files = diff_snapshots(&before, &after);
    let duration_ms = started.elapsed().as_millis() as u64;

    if timed_out {
        return Err(format!(
            "external agent timed out after {}s; partial changes are NOT rolled back (changed files: {:?})",
            spec.timeout.as_secs(),
            changed_files
        ));
    }
    if aborted {
        return Err(format!(
            "external agent aborted; partial changes are NOT rolled back (changed files: {:?})",
            changed_files
        ));
    }
    if !status.success() {
        return Err(format!(
            "external agent exited with {}: {}",
            status.code().unwrap_or(-1),
            stderr_tail
        ));
    }

    let result = parser.finish()?;
    Ok(RunOutcome {
        result,
        exit_code: status.code().unwrap_or(0),
        duration_ms,
        changed_files,
    })
}

async fn terminate(pid: Option<u32>, child: &mut tokio::process::Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = pid {
            unsafe {
                libc::killpg(pid as i32, libc::SIGTERM);
            }
            if tokio::time::timeout(KILL_GRACE, child.wait()).await.is_ok() {
                return;
            }
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
            return;
        }
    }
    let _ = pid;
    let _ = child.kill().await;
}

#[cfg(test)]
mod tests {
    use super::super::capability::{
        EffectiveSandbox, OutputProtocol, ProfileCapability, PromptTransport,
    };
    use super::*;

    fn fixture(script: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/extagent")
            .join(script)
    }

    fn test_spec(script: &str, transport: PromptTransport, protocol: OutputProtocol) -> RunSpec {
        RunSpec {
            def: ExternalAgentDef {
                name: "fake".to_string(),
                command: fixture(script),
                args: vec![],
                capability: ProfileCapability {
                    prompt_transport: transport,
                    output_protocol: protocol,
                    resume_flag: None,
                    effective_sandbox: EffectiveSandbox::Unknown,
                    version_requirement: None,
                    session_id_source: None,
                },
                auth_env: vec![],
                env_allow: vec![],
                trusted: false,
            },
            prompt: "do the thing".to_string(),
            cwd: std::env::temp_dir(),
            timeout: Duration::from_secs(30),
            extra_args: vec![],
            file_cache: None,
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn runner_happy_path_streams_events_and_returns_result() {
        let spec = test_spec(
            "fake-claude.sh",
            PromptTransport::Stdin,
            OutputProtocol::JsonlClaude,
        );
        let mut seen = vec![];
        let out = run_external(spec, |e| seen.push(e), AbortController::new())
            .await
            .unwrap();
        assert_eq!(out.result.session_id.as_deref(), Some("sess-0001"));
        assert_eq!(out.exit_code, 0);
        assert!(seen.iter().any(|e| matches!(e, ExtEvent::ToolUse { .. })));
    }

    #[tokio::test]
    #[cfg(unix)]
    #[serial_test::serial]
    async fn runner_env_is_cleared_to_allowlist() {
        std::env::set_var("ZODE_SECRET_PROBE", "must-not-leak");
        let spec = test_spec(
            "fake-env-dump.sh",
            PromptTransport::Stdin,
            OutputProtocol::Text,
        );
        let out = run_external(spec, |_| {}, AbortController::new())
            .await
            .unwrap();
        std::env::remove_var("ZODE_SECRET_PROBE");
        assert!(!out.result.text.contains("ZODE_SECRET_PROBE"));
        assert!(out.result.text.contains("PATH="));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn runner_timeout_kills_process_group() {
        let mut spec = test_spec("fake-hang.sh", PromptTransport::Stdin, OutputProtocol::Text);
        spec.timeout = Duration::from_millis(300);
        let started = Instant::now();
        assert!(run_external(spec, |_| {}, AbortController::new())
            .await
            .is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn runner_reports_changed_files_in_git_cwd() {
        let repo = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo.path())
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        std::fs::write(repo.path().join("clean.txt"), "old").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);
        let mut spec = test_spec(
            "fake-writer.sh",
            PromptTransport::Stdin,
            OutputProtocol::Text,
        );
        spec.cwd = repo.path().to_path_buf();
        let out = run_external(spec, |_| {}, AbortController::new())
            .await
            .unwrap();
        assert!(
            out.changed_files
                .iter()
                .any(|p| p.contains("written-by-agent.txt")),
            "changed: {:?}",
            out.changed_files
        );
    }
}
