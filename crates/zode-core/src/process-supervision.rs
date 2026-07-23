//! Small, cancellation-safe subprocess capture primitive used by product tools.
//!
//! The actual process owner runs in a tracked Tokio worker. Dropping a tool
//! future therefore cannot detach an unowned child: the worker keeps the
//! process guard until exit, timeout, or root-turn cancellation.

use std::io;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use agent::abort::{AbortController, TurnActivity};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};

const TREE_EXIT_POLL: Duration = Duration::from_millis(20);
const TREE_CLEANUP_GRACE: Duration = Duration::from_secs(5);

/// Run blocking turn work without letting cancellation hide an in-flight
/// operation from the host's quiescence barrier.
pub(crate) async fn spawn_blocking_tracked<F, T>(
    activity: &TurnActivity,
    operation: F,
) -> Result<T, tokio::task::JoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let work = activity.track_worker();
    tokio::task::spawn_blocking(move || {
        let _work = work;
        operation()
    })
    .await
}

#[derive(Debug)]
pub(crate) struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug)]
pub(crate) enum CaptureError {
    Aborted(String),
    Io(io::Error),
    TimedOut,
    Worker(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Aborted(reason) => write!(f, "aborted: {reason}"),
            Self::Io(error) => error.fmt(f),
            Self::TimedOut => f.write_str("timed out"),
            Self::Worker(error) => write!(f, "process supervisor failed: {error}"),
        }
    }
}

/// Capture a command with bounded memory, root-turn cancellation, and a hard
/// deadline. On Unix every command gets a distinct process group, which is
/// closed even after a successful leader exit. Processes that escape that OS
/// boundary are not claimed as locally terminated; observable cleanup gaps
/// latch `unresolved_external_work` instead.
pub(crate) async fn run_captured(
    command: Command,
    abort: &AbortController,
    deadline: Duration,
    stream_cap: usize,
) -> Result<CapturedOutput, CaptureError> {
    run_captured_with_input(command, None, abort, deadline, stream_cap).await
}

/// [`run_captured`] with an optional, owned stdin payload. Ownership keeps the
/// bytes alive in the tracked supervisor if the calling tool future is dropped.
pub(crate) async fn run_captured_with_input(
    command: Command,
    stdin_bytes: Option<Vec<u8>>,
    abort: &AbortController,
    deadline: Duration,
    stream_cap: usize,
) -> Result<CapturedOutput, CaptureError> {
    if abort.is_aborted() {
        return Err(CaptureError::Aborted(abort_reason(abort)));
    }

    let worker_abort = abort.clone();
    let worker_guard = abort.activity().track_worker();
    let worker = tokio::spawn(async move {
        let _worker_guard = worker_guard;
        spawn_and_supervise(command, stdin_bytes, worker_abort, deadline, stream_cap).await
    });

    worker
        .await
        .map_err(|error| CaptureError::Worker(error.to_string()))?
}

async fn spawn_and_supervise(
    mut command: Command,
    stdin_bytes: Option<Vec<u8>>,
    abort: AbortController,
    deadline: Duration,
    stream_cap: usize,
) -> Result<CapturedOutput, CaptureError> {
    if abort.is_aborted() {
        return Err(CaptureError::Aborted(abort_reason(&abort)));
    }

    command
        .stdin(if stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_process_group(&mut command);

    let mut child = command.spawn().map_err(CaptureError::Io)?;
    let pid = child.id();
    let mut process_guard = ProcessTreeGuard::new(pid, abort.clone());
    let Some(stdout) = child.stdout.take() else {
        cleanup_failed_spawn(&mut child, &mut process_guard).await;
        return Err(CaptureError::Io(io::Error::other(
            "spawned process has no stdout pipe",
        )));
    };
    let Some(stderr) = child.stderr.take() else {
        cleanup_failed_spawn(&mut child, &mut process_guard).await;
        return Err(CaptureError::Io(io::Error::other(
            "spawned process has no stderr pipe",
        )));
    };
    let stdin = if stdin_bytes.is_some() {
        let Some(stdin) = child.stdin.take() else {
            cleanup_failed_spawn(&mut child, &mut process_guard).await;
            return Err(CaptureError::Io(io::Error::other(
                "spawned process has no stdin pipe",
            )));
        };
        Some(stdin)
    } else {
        None
    };

    abort.pulse();
    supervise_child(
        child,
        ChildPipes {
            stdin,
            stdin_bytes,
            stdout,
            stderr,
        },
        process_guard,
        abort,
        deadline,
        stream_cap,
    )
    .await
}

async fn cleanup_failed_spawn(child: &mut Child, process_guard: &mut ProcessTreeGuard) {
    let _ = process_guard.terminate_and_reap(child).await;
}

struct ChildPipes<R1, R2> {
    stdin: Option<tokio::process::ChildStdin>,
    stdin_bytes: Option<Vec<u8>>,
    stdout: R1,
    stderr: R2,
}

async fn supervise_child<R1, R2>(
    mut child: Child,
    pipes: ChildPipes<R1, R2>,
    mut process_guard: ProcessTreeGuard,
    abort: AbortController,
    deadline: Duration,
    stream_cap: usize,
) -> Result<CapturedOutput, CaptureError>
where
    R1: AsyncRead + Unpin,
    R2: AsyncRead + Unpin,
{
    let ChildPipes {
        stdin,
        stdin_bytes,
        mut stdout,
        mut stderr,
    } = pipes;

    enum Completion {
        Finished(io::Result<(ExitStatus, CappedBytes, CappedBytes)>),
        Aborted,
        TimedOut,
    }

    let completion = {
        let stdin_write = write_input(stdin, stdin_bytes, &abort);
        let stdout_read = read_capped(&mut stdout, stream_cap, &abort);
        let stderr_read = read_capped(&mut stderr, stream_cap, &abort);
        let execution = async {
            let (stdin, stdout, stderr, status) =
                tokio::join!(stdin_write, stdout_read, stderr_read, child.wait());
            let status = status?;
            stdin?;
            Ok((status, stdout?, stderr?))
        };
        tokio::pin!(execution);
        tokio::select! {
            biased;
            _ = abort.cancelled() => Completion::Aborted,
            _ = tokio::time::sleep(deadline) => Completion::TimedOut,
            result = &mut execution => Completion::Finished(result),
        }
    };

    match completion {
        Completion::Finished(result) => {
            // A successful leader can still leave group-bound descendants.
            process_guard.cleanup_after_leader_exit().await;
            let (status, stdout, stderr) = result.map_err(CaptureError::Io)?;
            abort.pulse();
            Ok(CapturedOutput {
                status,
                stdout: stdout.bytes,
                stderr: stderr.bytes,
                stdout_truncated: stdout.truncated,
                stderr_truncated: stderr.truncated,
            })
        }
        Completion::Aborted => {
            let _ = process_guard.terminate_and_reap(&mut child).await;
            Err(CaptureError::Aborted(abort_reason(&abort)))
        }
        Completion::TimedOut => {
            let _ = process_guard.terminate_and_reap(&mut child).await;
            Err(CaptureError::TimedOut)
        }
    }
}

async fn write_input(
    stdin: Option<tokio::process::ChildStdin>,
    bytes: Option<Vec<u8>>,
    abort: &AbortController,
) -> io::Result<()> {
    let (Some(mut stdin), Some(bytes)) = (stdin, bytes) else {
        return Ok(());
    };
    for chunk in bytes.chunks(16 * 1024) {
        stdin.write_all(chunk).await?;
        abort.pulse();
    }
    stdin.shutdown().await
}

#[derive(Debug)]
struct CappedBytes {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn read_capped<R: AsyncRead + Unpin>(
    reader: &mut R,
    cap: usize,
    abort: &AbortController,
) -> io::Result<CappedBytes> {
    let mut bytes = Vec::with_capacity(cap.min(16 * 1024));
    let mut truncated = false;
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        abort.pulse();
        let available = cap.saturating_sub(bytes.len());
        let retained = available.min(read);
        bytes.extend_from_slice(&chunk[..retained]);
        truncated |= retained < read;
    }
    Ok(CappedBytes { bytes, truncated })
}

fn abort_reason(abort: &AbortController) -> String {
    abort.reason().unwrap_or_else(|| "aborted".to_string())
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[derive(Debug)]
pub(crate) struct ProcessTreeGuard {
    pid: Option<u32>,
    abort: AbortController,
    armed: bool,
}

impl ProcessTreeGuard {
    pub(crate) fn new(pid: Option<u32>, abort: AbortController) -> Self {
        Self {
            pid,
            abort,
            armed: true,
        }
    }

    /// Returns whether the platform reported that a termination request was
    /// delivered to the owned process group/tree.
    fn kill_tree(&self) -> bool {
        #[cfg(unix)]
        {
            let Some(pid) = self.pid.and_then(|pid| i32::try_from(pid).ok()) else {
                return false;
            };
            let result = unsafe { libc::killpg(pid, libc::SIGKILL) };
            if result == 0 {
                return true;
            }
            false
        }

        #[cfg(windows)]
        {
            let Some(pid) = self.pid else {
                return false;
            };
            return std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
        }

        #[cfg(not(any(unix, windows)))]
        {
            let _ = self.pid;
            false
        }
    }

    async fn wait_for_tree_exit(&self, tree_signal_succeeded: bool) -> bool {
        #[cfg(unix)]
        {
            let _ = tree_signal_succeeded;
            let deadline = tokio::time::Instant::now() + TREE_CLEANUP_GRACE;
            loop {
                match self.tree_state() {
                    ProcessTreeState::Gone => return true,
                    ProcessTreeState::Unknown => return false,
                    ProcessTreeState::Alive if tokio::time::Instant::now() >= deadline => {
                        return false;
                    }
                    ProcessTreeState::Alive => {
                        // A group leader can fork after the first SIGKILL was
                        // delivered but before it actually stops. Re-signal the
                        // group so those late descendants cannot escape cleanup.
                        let _ = self.kill_tree();
                        tokio::time::sleep(TREE_EXIT_POLL).await;
                    }
                }
            }
        }

        #[cfg(windows)]
        return tree_signal_succeeded;

        #[cfg(not(any(unix, windows)))]
        {
            let _ = tree_signal_succeeded;
            false
        }
    }

    #[cfg(unix)]
    fn tree_state(&self) -> ProcessTreeState {
        let Some(pid) = self.pid.and_then(|pid| i32::try_from(pid).ok()) else {
            return ProcessTreeState::Unknown;
        };
        if unsafe { libc::killpg(pid, 0) } == 0 {
            return ProcessTreeState::Alive;
        }
        match io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => ProcessTreeState::Gone,
            Some(libc::EPERM) => ProcessTreeState::Alive,
            _ => ProcessTreeState::Unknown,
        }
    }

    pub(crate) async fn cleanup_after_leader_exit(&mut self) {
        if !self.armed {
            return;
        }
        let signal_succeeded = self.kill_tree();
        let proven = self.wait_for_tree_exit(signal_succeeded).await;
        self.complete_cleanup(proven);
    }

    pub(crate) async fn terminate_and_reap(&mut self, child: &mut Child) -> io::Result<ExitStatus> {
        let tree_signal_succeeded = self.kill_tree();
        let _ = child.start_kill();
        let direct_child_result = match tokio::time::timeout(TREE_CLEANUP_GRACE, child.wait()).await
        {
            Ok(result) => result,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "direct child did not exit after process-tree termination",
            )),
        };
        let tree_exit_proven = self.wait_for_tree_exit(tree_signal_succeeded).await;
        self.complete_cleanup(
            direct_child_result.is_ok() && tree_signal_succeeded && tree_exit_proven,
        );
        direct_child_result
    }

    fn complete_cleanup(&mut self, proven: bool) {
        if !proven {
            self.abort.mark_unresolved_external_work();
        }
        self.armed = false;
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessTreeState {
    Alive,
    Gone,
    Unknown,
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.kill_tree();
            // Drop cannot wait for an OS-level proof. Keep this monotonic latch
            // set even when the synchronous best-effort signal succeeded.
            self.abort.mark_unresolved_external_work();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropped_blocking_caller_stays_active_until_closure_returns() {
        let activity = TurnActivity::new();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker_activity = activity.clone();
        let task = tokio::spawn(async move {
            spawn_blocking_tracked(&worker_activity, move || {
                let _ = started_tx.send(());
                release_rx.recv().unwrap();
            })
            .await
        });

        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .unwrap()
            .unwrap();
        task.abort();
        let _ = task.await;
        assert_eq!(activity.active_workers(), 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(25), activity.wait_for_quiescence())
                .await
                .is_err()
        );

        release_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), activity.wait_for_quiescence())
            .await
            .unwrap();
    }

    #[cfg(unix)]
    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script);
        command
    }

    #[cfg(unix)]
    async fn wait_for_pid(path: &std::path::Path) -> u32 {
        for _ in 0..100 {
            if let Ok(raw) = tokio::fs::read_to_string(path).await {
                if let Ok(pid) = raw.trim().parse() {
                    return pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("child pid was not written to {}", path.display());
    }

    #[cfg(unix)]
    async fn assert_process_stopped(pid: u32) {
        for _ in 0..100 {
            let alive = std::process::Command::new("/bin/kill")
                .args(["-0", &pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !alive {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("process {pid} survived supervised cleanup");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn capture_drains_but_retains_only_the_configured_limit() {
        let abort = AbortController::new();
        let output = run_captured(
            shell("head -c 8192 /dev/zero; head -c 4096 /dev/zero >&2"),
            &abort,
            Duration::from_secs(2),
            1024,
        )
        .await
        .unwrap();

        assert_eq!(output.stdout.len(), 1024);
        assert_eq!(output.stderr.len(), 1024);
        assert!(output.stdout_truncated);
        assert!(output.stderr_truncated);
        assert!(!abort.activity().unresolved_external_work());
    }

    #[tokio::test]
    async fn missing_process_identity_marks_cleanup_unresolved() {
        let abort = AbortController::new();
        let mut guard = ProcessTreeGuard::new(None, abort.clone());

        guard.cleanup_after_leader_exit().await;

        assert!(abort.activity().unresolved_external_work());
    }

    #[test]
    fn dropped_unverified_process_guard_marks_cleanup_unresolved() {
        let abort = AbortController::new();
        drop(ProcessTreeGuard::new(None, abort.clone()));

        assert!(abort.activity().unresolved_external_work());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn captured_input_is_owned_and_output_stays_bounded() {
        let abort = AbortController::new();
        let output = run_captured_with_input(
            shell("cat"),
            Some(vec![b'x'; 8192]),
            &abort,
            Duration::from_secs(2),
            1024,
        )
        .await
        .unwrap();

        assert_eq!(output.stdout, vec![b'x'; 1024]);
        assert!(output.stdout_truncated);
        assert!(output.stderr.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_the_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("descendant.pid");
        let script = format!(
            "sleep 30 & child=$!; printf %s \"$child\" > '{}'; wait",
            pid_file.display()
        );
        let abort = AbortController::new();
        let result = run_captured(shell(&script), &abort, Duration::from_millis(150), 1024).await;

        assert!(matches!(result, Err(CaptureError::TimedOut)));
        let pid = wait_for_pid(&pid_file).await;
        assert_process_stopped(pid).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hard_dropped_caller_leaves_a_tracked_supervisor_until_abort() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("descendant.pid");
        let script = format!(
            "sleep 30 & child=$!; printf %s \"$child\" > '{}'; wait",
            pid_file.display()
        );
        let abort = AbortController::new();
        let task_abort = abort.clone();
        let task = tokio::spawn(async move {
            run_captured(shell(&script), &task_abort, Duration::from_secs(30), 1024).await
        });
        let pid = wait_for_pid(&pid_file).await;

        task.abort();
        let _ = task.await;
        assert_eq!(abort.activity().active_workers(), 1);
        abort.abort_with_reason("test hard stop");
        abort.activity().wait_for_quiescence().await;

        assert_process_stopped(pid).await;
    }
}
