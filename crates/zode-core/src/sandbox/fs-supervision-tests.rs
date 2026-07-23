async fn supervised_fs_sink() -> Option<(tempfile::TempDir, SandboxedFsSink)> {
    let dir = tempfile::tempdir().ok()?;
    let config = SandboxConfig::new(dir.path(), SandboxMode::WorkspaceWrite, false, &[]).ok()?;
    if config.os == SandboxOs::Linux && !binary_on_path("bwrap") {
        return None;
    }
    if !config.probe("true").await.ok()? {
        return None;
    }
    Some((dir, SandboxedFsSink::new(config)))
}

async fn wait_for_supervised_pid(path: &Path) -> u32 {
    for _ in 0..100 {
        if let Ok(raw) = tokio::fs::read_to_string(path).await {
            if let Ok(pid) = raw.trim().parse() {
                return pid;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("child pid was not written to {}", path.display());
}

async fn assert_supervised_process_stopped(pid: u32) {
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
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("sandboxed fs descendant {pid} survived cancellation");
}

#[tokio::test]
async fn dropped_fs_call_stays_tracked_and_abort_kills_descendants() {
    let Some((dir, sink)) = supervised_fs_sink().await else {
        return;
    };
    let pid_file = dir.path().join("fs-descendant.pid");
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "sleep 30 & child=$!; printf %s \"$child\" > \"$0\"; wait".to_string(),
        pid_file.display().to_string(),
    ];
    let abort = agent::abort::AbortController::new();
    let task_abort = abort.clone();
    let task = tokio::spawn(async move { sink.run(&argv, None, &task_abort).await });
    let pid = wait_for_supervised_pid(&pid_file).await;

    task.abort();
    let _ = task.await;
    assert_eq!(abort.activity().active_workers(), 1);
    abort.abort_with_reason("watchdog hard stop");
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        abort.activity().wait_for_quiescence(),
    )
    .await
    .expect("sandboxed fs supervisor must quiesce after abort");

    assert_supervised_process_stopped(pid).await;
}

#[tokio::test]
async fn sandboxed_fs_error_output_is_bounded() {
    let Some((_dir, sink)) = supervised_fs_sink().await else {
        return;
    };
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "yes x | head -c 131072 >&2; exit 7".to_string(),
    ];
    let abort = agent::abort::AbortController::new();
    let error = sink.run(&argv, None, &abort).await.unwrap_err();
    let message = error.to_string();

    assert!(message.contains("stderr truncated"), "{message}");
    assert!(
        message.len() <= fs::FS_OP_OUTPUT_CAP + 256,
        "{}",
        message.len()
    );
}
