//! Real-Windows Tier 1 integration tests. Opt in with:
//! ZODE_SANDBOX_IT=1 cargo test -p zode-core --features sandbox-test-runner
//!   --test windows-sandbox-it -- --ignored

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Stdio;

use agent_tools_code::FsSink;
use serial_test::serial;
use zode_core::sandbox::{SandboxConfig, SandboxMode, SandboxedFsSink};

fn enabled() -> bool {
    std::env::var("ZODE_SANDBOX_IT").as_deref() == Ok("1")
}

fn zode_exe() -> PathBuf {
    let test = std::env::current_exe().expect("test exe");
    test.parent()
        .and_then(Path::parent)
        .expect("target debug")
        .join("zode.exe")
}

fn configure_runner() {
    std::env::set_var("ZODE_SANDBOX_RUNNER", zode_exe());
}

fn run(config: &SandboxConfig, argv: &[String], stdin: Option<&[u8]>) -> std::process::Output {
    let wrapped = config.wrap_argv(argv);
    let mut child = std::process::Command::new(&wrapped[0])
        .args(&wrapped[1..])
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sandbox wrapper");
    if let Some(bytes) = stdin {
        use std::io::Write;
        child.stdin.take().unwrap().write_all(bytes).unwrap();
    }
    let output = child.wait_with_output().expect("sandbox output");
    // Surface the sandboxed child's diagnostics in the CI log so a runtime
    // Win32 failure (restricted-token launch, ACL, fs-op) is visible.
    eprintln!(
        "[sandbox-it] argv={:?}\n  status={:?}\n  stdout={}\n  stderr={}",
        argv,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

fn log_fs_result(operation: &str, result: &std::io::Result<()>) {
    eprintln!("[sandbox-it] in-process fs-op={operation} result={result:?}");
}

struct TierTwoGuard;

impl Drop for TierTwoGuard {
    fn drop(&mut self) {
        if let Err(error) = zode_core::sandbox::windows::cleanup_acl_journal() {
            eprintln!("[sandbox-it] AppContainer cleanup during Drop failed: {error}");
        }
    }
}

#[tokio::test]
#[ignore]
#[serial]
async fn writes_inside_but_denies_outside_and_read_only() {
    if !enabled() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let config = SandboxConfig::new(root.path(), SandboxMode::WorkspaceWrite, false, &[]).unwrap();
    let sink = SandboxedFsSink::new(config.clone());
    let inside = root.path().join("inside.txt");
    let result = sink.write_file(&inside, b"inside").await;
    log_fs_result("write inside", &result);
    assert!(result.is_ok());
    assert_eq!(std::fs::read(&inside).unwrap(), b"inside");

    // Windows workspace-write intentionally includes the process temp dir, so
    // a second tempfile is not outside policy. USERPROFILE is outside both the
    // workspace and temp roots on windows-latest.
    let escaped = PathBuf::from(std::env::var_os("USERPROFILE").expect("USERPROFILE"))
        .join(format!("zode-sandbox-outside-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&escaped);
    let result = sink.write_file(&escaped, b"escape").await;
    log_fs_result("write outside", &result);
    assert!(result.is_err());
    assert!(!escaped.exists());

    let read_only = config.with_mode(SandboxMode::ReadOnly);
    let read_only_sink = SandboxedFsSink::new(read_only);
    let denied = root.path().join("read-only.txt");
    let result = read_only_sink.write_file(&denied, b"denied").await;
    log_fs_result("write read-only", &result);
    assert!(result.is_err());
    assert!(!denied.exists());
}

#[tokio::test]
#[ignore]
#[serial]
async fn fs_ops_round_trip_and_parent_delete_child_tradeoff_is_explicit() {
    if !enabled() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let config = SandboxConfig::new(root.path(), SandboxMode::WorkspaceWrite, false, &[]).unwrap();
    let sink = SandboxedFsSink::new(config);
    let dir = root.path().join("dir");
    let result = sink.create_dir(&dir, false).await;
    log_fs_result("mkdir", &result);
    assert!(result.is_ok());
    let file = dir.join("a.txt");
    let result = sink.write_file(&file, b"roundtrip").await;
    log_fs_result("write", &result);
    assert!(result.is_ok());
    let renamed = dir.join("b.txt");
    let result = sink.rename(&file, &renamed).await;
    log_fs_result("rename", &result);
    assert!(result.is_ok());
    let result = sink.remove(&renamed, false, false).await;
    log_fs_result("remove", &result);
    assert!(result.is_ok());

    // Tier 1 retains FILE_DELETE_CHILD for atomic-rename compatibility. This
    // proves the documented limitation: kernel ACLs cannot protect .git from
    // a rename performed through the writable parent.
    let git = root.path().join(".git");
    std::fs::create_dir(&git).unwrap();
    let moved = root.path().join("git-moved");
    let result = sink.rename(&git, &moved).await;
    log_fs_result("rename .git through parent", &result);
    assert!(result.is_ok());
}

#[test]
#[ignore]
#[serial]
fn tier_one_sets_advisory_network_environment() {
    if !enabled() {
        return;
    }
    configure_runner();
    let root = tempfile::tempdir().unwrap();
    let config = SandboxConfig::new(root.path(), SandboxMode::WorkspaceWrite, false, &[]).unwrap();
    let output = run(
        &config,
        &[
            "cmd.exe".into(),
            "/D".into(),
            "/C".into(),
            "echo %ZODE_SANDBOX_NETWORK%".into(),
        ],
        None,
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("unenforced"));
}

#[tokio::test]
#[ignore]
#[serial]
async fn tier_two_appcontainer_confines_files_and_denies_network() {
    if !enabled() {
        return;
    }
    configure_runner();
    let guard = TierTwoGuard;

    let root = tempfile::tempdir().unwrap();
    let config = SandboxConfig::new(root.path(), SandboxMode::WorkspaceWrite, false, &[])
        .unwrap()
        .with_windows_tier(Some("elevated"));

    // Diagnose the lowbox child's effective CWD independently from an
    // absolute redirect. If this succeeds, retained user read/traverse access
    // reaches the workspace and the launch CWD is a valid Win32 name.
    let cwd_output = run(
        &config,
        &["cmd.exe".into(), "/D".into(), "/C".into(), "cd".into()],
        None,
    );
    eprintln!(
        "[sandbox-it] Tier 2 child cwd={:?}",
        String::from_utf8_lossy(&cwd_output.stdout).trim()
    );
    assert!(cwd_output.status.success());

    // cmd.exe applies special /C quote parsing to the raw Windows command
    // line, whereas the launcher must quote argv for ordinary programs. A
    // quoted absolute redirect is consequently not a reliable access-control
    // oracle here. Use a relative redirect from the verified CWD instead.
    let relative = root.path().join("tier-two-relative.txt");
    let relative_output = run(
        &config,
        &[
            "cmd.exe".into(),
            "/D".into(),
            "/C".into(),
            "echo relative>tier-two-relative.txt".into(),
        ],
        None,
    );
    assert!(relative_output.status.success());
    assert_eq!(std::fs::read(&relative).unwrap(), b"relative\r\n");

    // Tier 2 file tools deliberately reuse Tier 1 restricted-token
    // impersonation. Prove their positive path independently from cmd.exe.
    let sink = SandboxedFsSink::new(config.clone());
    let inside = root.path().join("tier-two-fs-sink-inside.txt");
    let inside_result = sink.write_file(&inside, b"inside").await;
    log_fs_result("Tier 2 write inside", &inside_result);
    assert!(inside_result.is_ok());
    assert_eq!(std::fs::read(&inside).unwrap(), b"inside");

    // Use the direct CreateFileW-based fs-op path for the negative assertion.
    // PermissionDenied is therefore a real ERROR_ACCESS_DENIED result from the
    // restricted token, not cmd.exe's ERROR_INVALID_NAME absolute-path quirk.
    let outside = PathBuf::from(std::env::var_os("USERPROFILE").unwrap())
        .join(format!("zode-tier-two-outside-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&outside);
    let outside_result = sink.write_file(&outside, b"escape").await;
    log_fs_result("Tier 2 write outside", &outside_result);
    assert_eq!(
        outside_result
            .expect_err("outside write must be denied")
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert!(!outside.exists());

    let read_only_sink = SandboxedFsSink::new(config.clone().with_mode(SandboxMode::ReadOnly));
    let read_only_path = root.path().join("tier-two-read-only.txt");
    let read_only_result = read_only_sink.write_file(&read_only_path, b"denied").await;
    log_fs_result("Tier 2 read-only write", &read_only_result);
    assert_eq!(
        read_only_result
            .expect_err("read-only write must be denied")
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert!(!read_only_path.exists());

    let verification = config.verify().await;
    eprintln!("[sandbox-it] Tier 2 AppContainer verification={verification:?}");
    assert!(verification.is_ok());
    zode_core::sandbox::windows::cleanup_acl_journal()
        .expect("remove AppContainer profile and ACEs");
    std::mem::forget(guard);
}
