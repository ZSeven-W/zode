#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};

use super::super::capability::{
    EffectiveSandbox, OutputProtocol, ProfileCapability, PromptTransport,
};
use super::*;

fn fixture(script: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/extagent")
        .join(script)
}

fn test_spec_path(
    command: PathBuf,
    transport: PromptTransport,
    protocol: OutputProtocol,
) -> RunSpec {
    RunSpec {
        def: ExternalAgentDef {
            name: "fake".to_string(),
            command,
            args: vec![],
            capability: ProfileCapability {
                prompt_transport: transport,
                output_protocol: protocol,
                resume_flag: None,
                resume_args: None,
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::Unknown,
                version_requirement: None,
                session_id_source: None,
                text_source: None,
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

fn test_spec(script: &str, transport: PromptTransport, protocol: OutputProtocol) -> RunSpec {
    test_spec_path(fixture(script), transport, protocol)
}

#[cfg(unix)]
fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
async fn wait_for_pid(path: &Path) -> i32 {
    for _ in 0..500 {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse() {
                return pid;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("process did not publish a pid at {}", path.display());
}

#[cfg(unix)]
async fn assert_process_group_gone(pgid: i32) {
    // Generous window: the kill itself is prompt, but under a fully loaded
    // machine (workspace-wide `cargo test` saturating every core) the reaper
    // thread and the timer wheel both lag — 3s flaked twice in real runs.
    // Returns as soon as the group is gone, so the happy path stays fast.
    for _ in 0..1500 {
        let result = unsafe { libc::killpg(pgid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("process group {pgid} survived cleanup");
}

#[cfg(unix)]
fn process_tree_spec(dir: &Path, timeout: Duration) -> (RunSpec, PathBuf) {
    let leader_path = dir.join("leader.pid");
    let script = write_script(
        dir,
        "process-tree.sh",
        "#!/bin/sh\necho \"$$\" > \"$1\"\necho \"$$\"\nsleep 300 &\nwait\n",
    );
    let mut spec = test_spec_path(script, PromptTransport::Stdin, OutputProtocol::Text);
    spec.def
        .args
        .push(leader_path.to_string_lossy().into_owned());
    spec.cwd = dir.to_path_buf();
    spec.timeout = timeout;
    (spec, leader_path)
}

#[cfg(unix)]
struct EnvGuard {
    name: &'static str,
    previous: Option<OsString>,
}

#[cfg(unix)]
impl EnvGuard {
    fn set(name: &'static str, value: &Path) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

#[cfg(unix)]
impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

#[tokio::test]
#[cfg(unix)]
#[serial_test::serial]
async fn runner_happy_path_streams_events_and_returns_result() {
    let spec = test_spec(
        "fake-claude.sh",
        PromptTransport::Stdin,
        OutputProtocol::JsonlClaude,
    );
    let mut seen = vec![];
    let abort = AbortController::new();
    let out = run_external(spec, |event| seen.push(event), abort.clone())
        .await
        .unwrap();
    assert_eq!(out.result.session_id.as_deref(), Some("sess-0001"));
    assert_eq!(out.exit_code, 0);
    assert!(seen
        .iter()
        .any(|event| matches!(event, ExtEvent::ToolUse { .. })));
    assert!(!abort.activity().unresolved_external_work());
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
#[serial_test::serial]
async fn runner_timeout_kills_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let (spec, _) = process_tree_spec(dir.path(), Duration::from_secs(3));
    let leader = Arc::new(AtomicI32::new(0));
    let callback_leader = leader.clone();
    let result = run_external(
        spec,
        move |event| {
            if let ExtEvent::Text(text) = event {
                if let Ok(pid) = text.parse() {
                    callback_leader.store(pid, Ordering::Release);
                }
            }
        },
        AbortController::new(),
    )
    .await;
    assert!(result.unwrap_err().contains("timed out"));
    let leader = leader.load(Ordering::Acquire);
    assert!(leader > 0, "runner never streamed its process-group id");
    assert_process_group_gone(leader).await;
}

#[tokio::test]
#[cfg(unix)]
#[serial_test::serial]
async fn runner_abort_kills_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let (spec, leader_path) = process_tree_spec(dir.path(), Duration::from_secs(30));
    let abort = AbortController::new();
    let task_abort = abort.clone();
    let task = tokio::spawn(async move { run_external(spec, |_| {}, task_abort).await });
    let leader = wait_for_pid(&leader_path).await;
    abort.abort();
    assert!(task.await.unwrap().unwrap_err().contains("aborted"));
    assert_process_group_gone(leader).await;
    assert_eq!(abort.activity().active_workers(), 0);
    assert!(!abort.activity().unresolved_external_work());
}

#[tokio::test]
#[cfg(unix)]
#[serial_test::serial]
async fn runner_hard_drop_kills_process_group() {
    let dir = tempfile::tempdir().unwrap();
    let (spec, leader_path) = process_tree_spec(dir.path(), Duration::from_secs(30));
    let abort = AbortController::new();
    let activity = abort.activity();
    let task = tokio::spawn(run_external(spec, |_| {}, abort));
    let leader = wait_for_pid(&leader_path).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_process_group_gone(leader).await;
    tokio::time::timeout(Duration::from_secs(2), activity.wait_for_quiescence())
        .await
        .unwrap();
    assert_eq!(activity.active_workers(), 0);
    assert!(activity.unresolved_external_work());
}

#[tokio::test]
#[cfg(unix)]
#[serial_test::serial]
async fn runner_success_kills_detached_group_descendant() {
    let dir = tempfile::tempdir().unwrap();
    let leader_path = dir.path().join("leader.pid");
    let script = write_script(
        dir.path(),
        "background-child.sh",
        "#!/bin/sh\necho \"$$\" > \"$1\"\nsleep 300 </dev/null >/dev/null 2>&1 &\n",
    );
    let mut spec = test_spec_path(script, PromptTransport::Stdin, OutputProtocol::Text);
    spec.def
        .args
        .push(leader_path.to_string_lossy().into_owned());
    spec.cwd = dir.path().to_path_buf();
    run_external(spec, |_| {}, AbortController::new())
        .await
        .unwrap();
    let leader = wait_for_pid(&leader_path).await;
    assert_process_group_gone(leader).await;
}

#[tokio::test]
#[cfg(unix)]
#[serial_test::serial]
async fn runner_bounds_unterminated_stdout_and_pulses_activity() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("output.txt");
    std::fs::write(&output_path, vec![b'x'; MAX_LINE_BYTES * 4]).unwrap();
    let script = write_script(dir.path(), "stdout.sh", "#!/bin/sh\ncat \"$1\"\n");
    let mut spec = test_spec_path(script, PromptTransport::Stdin, OutputProtocol::Text);
    spec.def
        .args
        .push(output_path.to_string_lossy().into_owned());
    spec.cwd = dir.path().to_path_buf();
    let abort = AbortController::new();
    let before = abort.activity().last_activity_at();
    let out = run_external(spec, |_| {}, abort.clone()).await.unwrap();
    assert!(out.result.text.ends_with(TRUNCATION_MARKER));
    assert!(out.result.text.len() <= MAX_LINE_BYTES + TRUNCATION_MARKER.len());
    assert!(abort.activity().last_activity_at() > before);
}

#[tokio::test]
#[cfg(unix)]
#[serial_test::serial]
async fn runner_preserves_normal_unterminated_text() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("output.txt");
    std::fs::write(&output_path, "unterminated text").unwrap();
    let script = write_script(dir.path(), "stdout.sh", "#!/bin/sh\ncat \"$1\"\n");
    let mut spec = test_spec_path(script, PromptTransport::Stdin, OutputProtocol::Text);
    spec.def
        .args
        .push(output_path.to_string_lossy().into_owned());
    spec.cwd = dir.path().to_path_buf();
    let out = run_external(spec, |_| {}, AbortController::new())
        .await
        .unwrap();
    assert_eq!(out.result.text, "unterminated text");
}

#[tokio::test]
#[cfg(unix)]
#[serial_test::serial]
async fn runner_bounds_unterminated_stderr_and_pulses_activity() {
    let dir = tempfile::tempdir().unwrap();
    let error_path = dir.path().join("error.txt");
    std::fs::write(&error_path, vec![b'e'; STDERR_TAIL_BYTES * 4]).unwrap();
    let script = write_script(
        dir.path(),
        "stderr.sh",
        "#!/bin/sh\ncat \"$1\" >&2\nexit 7\n",
    );
    let mut spec = test_spec_path(script, PromptTransport::Stdin, OutputProtocol::Text);
    spec.def
        .args
        .push(error_path.to_string_lossy().into_owned());
    spec.cwd = dir.path().to_path_buf();
    let abort = AbortController::new();
    let before = abort.activity().last_activity_at();
    let error = run_external(spec, |_| {}, abort.clone()).await.unwrap_err();
    assert!(
        error.len() <= STDERR_TAIL_BYTES + 128,
        "error length: {}",
        error.len()
    );
    assert!(abort.activity().last_activity_at() > before);
}

#[tokio::test]
#[cfg(unix)]
#[serial_test::serial]
async fn file_prompt_paths_are_unique_and_cleaned_up() {
    let dir = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let _env = EnvGuard::set("ZODE_CONFIG_DIR", config.path());
    let script = write_script(dir.path(), "echo-arg.sh", "#!/bin/sh\nprintf '%s' \"$1\"\n");
    let make_spec = || {
        let mut spec = test_spec_path(script.clone(), PromptTransport::File, OutputProtocol::Text);
        spec.def.args.push("{prompt_file}".to_string());
        spec.cwd = dir.path().to_path_buf();
        spec
    };
    let (first, second) = tokio::join!(
        run_external(make_spec(), |_| {}, AbortController::new()),
        run_external(make_spec(), |_| {}, AbortController::new())
    );
    let first = first.unwrap().result.text;
    let second = second.unwrap().result.text;
    assert_ne!(first, second);
    assert!(!Path::new(&first).exists());
    assert!(!Path::new(&second).exists());
}

#[tokio::test]
#[serial_test::serial]
async fn async_git_snapshot_matches_public_sync_snapshot() {
    let repo = tempfile::tempdir().unwrap();
    std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(repo.path())
        .status()
        .unwrap();
    std::fs::write(repo.path().join("new.txt"), "new").unwrap();
    let sync = git_status_snapshot(repo.path());
    let asynchronous = git_status_snapshot_async(repo.path(), &AbortController::new(), true)
        .await
        .unwrap();
    assert_eq!(asynchronous, sync);
}

#[tokio::test]
#[cfg(unix)]
#[serial_test::serial]
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
            .any(|path| path.contains("written-by-agent.txt")),
        "changed: {:?}",
        out.changed_files
    );
}
