//! External agent process runner. Trust boundary notes (spec v2.3 §3.6-3.7):
//! env starts from `env_clear()` and only an allowlist is restored; prompts
//! prefer stdin (argv leaks via process lists); stdout/stderr are drained
//! concurrently (pipe deadlock); termination closes the owned Unix process
//! group or Windows `taskkill /T` tree. OS-escaped descendants are not claimed
//! as proven dead; aftermath (changed files, cache clear) stays best-effort.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent::abort::AbortController;
use agent::file_cache::FileStateCache;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::capability::PromptTransport;
use super::parser::{ExtEvent, FinalResult, StreamParser};
use super::profiles::ExternalAgentDef;
use crate::process_supervision::ProcessTreeGuard;

const STDERR_TAIL_BYTES: usize = 8 * 1024;
const MAX_LINE_BYTES: usize = 64 * 1024;
const IO_CHUNK_BYTES: usize = 8 * 1024;
const GIT_STATUS_MAX_BYTES: usize = 4 * 1024 * 1024;
const GIT_STATUS_TIMEOUT: Duration = Duration::from_secs(3);
const PIPE_DRAIN_GRACE: Duration = Duration::from_secs(1);
const TRUNCATION_MARKER: &str = " …[truncated]";

/// Env vars restored from the parent environment on every platform.
const BASE_ENV: &[&str] = &["PATH", "HOME", "TERM"];
#[cfg(windows)]
const WINDOWS_ENV: &[&str] = &["SystemRoot", "USERPROFILE", "TEMP", "PATHEXT"];

/// Loader-injection vars are refused even when explicitly allowlisted.
fn is_forbidden_env(name: &str) -> bool {
    name == "LD_PRELOAD" || name == "LD_LIBRARY_PATH" || name.starts_with("DYLD_")
}

#[cfg(unix)]
fn create_process_group(command: &mut tokio::process::Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn create_process_group(_command: &mut tokio::process::Command) {}

/// Reads logical lines without ever accumulating an unbounded unterminated
/// line. `BufReader` and `line` both have fixed upper bounds.
struct BoundedLineReader<R> {
    inner: BufReader<R>,
    line: Vec<u8>,
    truncated: bool,
}

impl<R: AsyncRead + Unpin> BoundedLineReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner: BufReader::with_capacity(IO_CHUNK_BYTES, inner),
            line: Vec::with_capacity(MAX_LINE_BYTES),
            truncated: false,
        }
    }

    async fn next_line(&mut self, abort: &AbortController) -> std::io::Result<Option<String>> {
        loop {
            let inner = &mut self.inner;
            let line = &mut self.line;
            let truncated = &mut self.truncated;
            let available = inner.fill_buf().await?;
            if available.is_empty() {
                if line.is_empty() && !*truncated {
                    return Ok(None);
                }
                return Ok(Some(Self::finish_line(line, truncated)));
            }

            abort.pulse();
            let newline = available.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(available.len(), |index| index + 1);
            let payload_len = newline.unwrap_or(available.len());
            let remaining = MAX_LINE_BYTES.saturating_sub(line.len());
            let kept = payload_len.min(remaining);
            line.extend_from_slice(&available[..kept]);
            if kept < payload_len {
                *truncated = true;
            }
            inner.consume(consumed);

            if newline.is_some() {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(Self::finish_line(line, truncated)));
            }
        }
    }

    fn finish_line(line: &mut Vec<u8>, truncated: &mut bool) -> String {
        let mut output = String::from_utf8_lossy(line).into_owned();
        if *truncated {
            output.push_str(TRUNCATION_MARKER);
        }
        line.clear();
        *truncated = false;
        output
    }
}

async fn stderr_tail<R: AsyncRead + Unpin>(
    mut reader: R,
    abort: AbortController,
) -> std::io::Result<String> {
    let mut tail = Vec::with_capacity(STDERR_TAIL_BYTES);
    let mut chunk = [0_u8; IO_CHUNK_BYTES];
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            break;
        }
        abort.pulse();
        let bytes = &chunk[..count];
        if bytes.len() >= STDERR_TAIL_BYTES {
            tail.clear();
            tail.extend_from_slice(&bytes[bytes.len() - STDERR_TAIL_BYTES..]);
        } else {
            let overflow = tail
                .len()
                .saturating_add(bytes.len())
                .saturating_sub(STDERR_TAIL_BYTES);
            if overflow > 0 {
                tail.drain(..overflow);
            }
            tail.extend_from_slice(bytes);
        }
    }
    Ok(String::from_utf8_lossy(&tail).into_owned())
}

async fn bounded_bytes<R: AsyncRead + Unpin>(
    mut reader: R,
    max_bytes: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::with_capacity(max_bytes.min(IO_CHUNK_BYTES));
    let mut overflowed = false;
    let mut chunk = [0_u8; IO_CHUNK_BYTES];
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            break;
        }
        let remaining = max_bytes.saturating_sub(output.len());
        let kept = remaining.min(count);
        output.extend_from_slice(&chunk[..kept]);
        overflowed |= kept < count;
    }
    Ok((output, overflowed))
}

fn spawn_tracked<F>(abort: &AbortController, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let work = abort.activity().track_worker();
    tokio::spawn(async move {
        let _work = work;
        future.await
    })
}

async fn join_bounded<T: Send + 'static>(mut task: JoinHandle<T>) -> Option<T> {
    match tokio::time::timeout(PIPE_DRAIN_GRACE, &mut task).await {
        Ok(result) => result.ok(),
        Err(_) => {
            task.abort();
            let _ = task.await;
            None
        }
    }
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
pub fn git_status_snapshot(cwd: &Path) -> Option<HashMap<String, String>> {
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
    Some(parse_git_status(&out.stdout))
}

fn parse_git_status(bytes: &[u8]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let text = String::from_utf8_lossy(bytes);
    for entry in text.split('\0').filter(|e| e.len() > 3) {
        let (status, path) = entry.split_at(3);
        map.insert(path.to_string(), status.trim().to_string());
    }
    map
}

#[derive(Debug)]
struct SnapshotAborted;

/// Async counterpart used by `run_external`. The subprocess and its output
/// are time/size bounded, and dropping this future invokes the platform's
/// process-tree cleanup through [`ProcessTreeGuard`].
async fn git_status_snapshot_async(
    cwd: &Path,
    abort: &AbortController,
    observe_abort: bool,
) -> Result<Option<HashMap<String, String>>, SnapshotAborted> {
    if observe_abort && abort.is_aborted() {
        return Err(SnapshotAborted);
    }

    let mut command = tokio::process::Command::new("git");
    command
        .arg("status")
        .arg("--porcelain=v1")
        .arg("-z")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    create_process_group(&mut command);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Ok(None),
    };
    let pid = child.id();
    let mut process_guard = ProcessTreeGuard::new(pid, abort.clone());
    let Some(stdout) = child.stdout.take() else {
        return Ok(None);
    };
    let stdout_task = spawn_tracked(abort, bounded_bytes(stdout, GIT_STATUS_MAX_BYTES));
    let deadline = tokio::time::sleep(GIT_STATUS_TIMEOUT);
    tokio::pin!(deadline);

    let mut was_aborted = false;
    let mut timed_out = false;
    let status = tokio::select! {
        result = child.wait() => result.ok(),
        _ = &mut deadline => {
            timed_out = true;
            process_guard.terminate_and_reap(&mut child).await.ok()
        }
        _ = abort.cancelled(), if observe_abort => {
            was_aborted = true;
            process_guard.terminate_and_reap(&mut child).await.ok()
        }
    };
    // A successful leader may still have background descendants. Closing the
    // group also guarantees the bounded stdout reader can reach EOF.
    process_guard.cleanup_after_leader_exit().await;

    let output_join = join_bounded(stdout_task).await;
    if output_join.is_none() {
        // A reader that cannot finish after group cleanup may still be held by
        // a descendant that escaped the observable process-group boundary.
        abort.mark_unresolved_external_work();
    }
    let output = output_join.and_then(Result::ok);

    if was_aborted {
        return Err(SnapshotAborted);
    }
    if timed_out || status.is_none() || !status.is_some_and(|status| status.success()) {
        return Ok(None);
    }
    let Some((bytes, overflowed)) = output else {
        return Ok(None);
    };
    if overflowed {
        return Ok(None);
    }
    Ok(Some(parse_git_status(&bytes)))
}

fn diff_snapshots(
    before: &Option<HashMap<String, String>>,
    after: &Option<HashMap<String, String>>,
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
    struct CacheClearOnDrop(Option<Arc<FileStateCache>>);
    impl Drop for CacheClearOnDrop {
        fn drop(&mut self) {
            if let Some(cache) = &self.0 {
                cache.clear();
            }
        }
    }
    let _cache_clear = CacheClearOnDrop(spec.file_cache.clone());
    let started = Instant::now();
    let before = git_status_snapshot_async(&spec.cwd, &abort, true)
        .await
        .map_err(|_| "external agent aborted before launch".to_string())?;

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
            let path = dir.join(format!(
                "prompt-{}-{}.txt",
                std::process::id(),
                Uuid::new_v4()
            ));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&path).map_err(|e| e.to_string())?;
            _prompt_file = Some(TempPrompt(path.clone()));
            std::io::Write::write_all(&mut file, spec.prompt.as_bytes())
                .map_err(|e| e.to_string())?;
            for a in &mut args {
                if a == "{prompt_file}" {
                    *a = path.to_string_lossy().to_string();
                }
            }
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
        })
        .kill_on_drop(true);

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

    // On Unix, create a new process group. The guard also supplies Windows'
    // tree-kill fallback so timeout, abort, and hard-drop cover descendants.
    create_process_group(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", spec.def.command.display()))?;
    let child_pid = child.id();
    let mut process_guard = ProcessTreeGuard::new(child_pid, abort.clone());

    let stdin_task = if let (Some(payload), Some(mut stdin)) = (stdin_payload, child.stdin.take()) {
        Some(spawn_tracked(&abort, async move {
            let _ = stdin.write_all(payload.as_bytes()).await;
            // dropping stdin closes the pipe (EOF)
        }))
    } else {
        None
    };

    // Concurrent drain: stderr on its own task, stdout parsed inline.
    let stderr = child.stderr.take().expect("stderr piped");
    let stderr_task = spawn_tracked(&abort, stderr_tail(stderr, abort.clone()));

    let stdout = child.stdout.take().expect("stdout piped");
    let mut parser = StreamParser::with_sources(
        &spec.def.capability.output_protocol,
        spec.def.capability.text_source.as_deref(),
        spec.def.capability.session_id_source.as_deref(),
    );
    let mut reader = BoundedLineReader::new(stdout);

    let deadline = tokio::time::sleep(spec.timeout);
    tokio::pin!(deadline);
    let mut timed_out = false;
    let mut aborted = false;
    let mut stdout_error = None;

    loop {
        tokio::select! {
            line = reader.next_line(&abort) => match line {
                Ok(Some(l)) => {
                    for ev in parser.feed(&l) {
                        on_event(ev);
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    on_event(ExtEvent::Log(format!("stdout read error: {e}")));
                    stdout_error = Some(e.to_string());
                    break;
                }
            },
            _ = &mut deadline => { timed_out = true; break; }
            _ = abort.cancelled() => { aborted = true; break; }
        }
    }

    let status = if timed_out || aborted || stdout_error.is_some() {
        process_guard.terminate_and_reap(&mut child).await
    } else {
        tokio::select! {
            result = child.wait() => result,
            _ = &mut deadline => {
                timed_out = true;
                process_guard.terminate_and_reap(&mut child).await
            }
            _ = abort.cancelled() => {
                aborted = true;
                process_guard.terminate_and_reap(&mut child).await
            }
        }
    };
    // A successful CLI leader may still have group-bound descendants. Close
    // the group before joining readers, and keep the guard armed meanwhile.
    process_guard.cleanup_after_leader_exit().await;
    drop(reader);
    let stderr_join = join_bounded(stderr_task).await;
    if stderr_join.is_none() {
        abort.mark_unresolved_external_work();
    }
    let stderr_tail = stderr_join.and_then(Result::ok).unwrap_or_default();
    if let Some(stdin_task) = stdin_task {
        if join_bounded(stdin_task).await.is_none() {
            abort.mark_unresolved_external_work();
        }
    }
    // Pipes are now at EOF, closed, or their reader task has been joined.
    let status = status.map_err(|e| e.to_string())?;

    // Cache clearing is owned by `_cache_clear`, including hard-drop paths.
    let after = match git_status_snapshot_async(&spec.cwd, &abort, false).await {
        Ok(snapshot) => snapshot,
        Err(_) => {
            aborted = true;
            None
        }
    };
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
    if let Some(error) = stdout_error {
        return Err(format!("external agent stdout read failed: {error}"));
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

#[cfg(test)]
#[path = "runner-tests.rs"]
mod tests;
