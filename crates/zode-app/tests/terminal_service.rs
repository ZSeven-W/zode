use std::time::Duration;

#[cfg(unix)]
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    thread,
    time::{Instant, SystemTime},
};

use futures_util::StreamExt;
use zode_app::services::{LocalTerminalService, TerminalService};

#[test]
fn terminal_service_resize_reaches_the_native_pty_master() {
    let cwd = std::env::current_dir().unwrap();
    let service = LocalTerminalService::new();
    let terminal = service.spawn(&cwd).unwrap();

    service.resize(terminal, 120, 40).unwrap();

    assert_eq!(service.size(terminal).unwrap(), (120, 40));
    service.close(terminal).unwrap();
}

#[tokio::test]
async fn terminal_service_shell_output_streams_and_close_joins_the_session() {
    let cwd = std::env::current_dir().unwrap();
    let service = LocalTerminalService::new();
    let terminal = service.spawn(&cwd).unwrap();
    let mut output = service.subscribe(terminal).unwrap();
    service
        .write(terminal, b"echo zode-terminal-ready\r\n".to_vec())
        .unwrap();

    let received = tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        while let Some(chunk) = output.next().await {
            bytes.extend(chunk.unwrap());
            if String::from_utf8_lossy(&bytes).contains("zode-terminal-ready") {
                return bytes;
            }
        }
        bytes
    })
    .await
    .expect("terminal output timed out");

    assert!(String::from_utf8_lossy(&received).contains("zode-terminal-ready"));
    service.close(terminal).unwrap();
    assert!(service.size(terminal).is_err());
}

#[tokio::test]
async fn terminal_service_process_exit_ends_output_without_an_io_error() {
    let cwd = std::env::current_dir().unwrap();
    let service = LocalTerminalService::new();
    let terminal = service.spawn(&cwd).unwrap();
    let mut output = service.subscribe(terminal).unwrap();
    service.write(terminal, b"exit\r\n".to_vec()).unwrap();

    let ended_cleanly = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = output.next().await {
            chunk?;
        }
        Ok::<_, zode_app::services::TerminalError>(())
    })
    .await
    .expect("terminal output did not end after process exit");

    ended_cleanly.unwrap();
    service.close(terminal).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_service_process_exit_cancels_reader_when_a_descendant_retains_the_pty() {
    let cwd = std::env::current_dir().unwrap();
    let service = LocalTerminalService::new();
    let terminal = service.spawn(&cwd).unwrap();
    let mut output = service.subscribe(terminal).unwrap();
    service
        .write(
            terminal,
            b"sh -c 'trap \"\" HUP TERM; exec sleep 30' & exit\r\n".to_vec(),
        )
        .unwrap();

    tokio::time::timeout(Duration::from_secs(5), async {
        while output.next().await.is_some() {}
    })
    .await
    .expect("terminal reader survived after its shell exited");

    service.close(terminal).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_service_natural_exit_drains_final_pty_bytes() {
    let cwd = std::env::current_dir().unwrap();
    let service = LocalTerminalService::new();
    let terminal = service.spawn(&cwd).unwrap();
    let mut output = service.subscribe(terminal).unwrap();
    service
        .write(
            terminal,
            b"stty -echo; printf '\\146\\151\\156\\141\\154\\055\\160\\164\\171\\055\\142\\171\\164\\145\\163'; exit\r\n"
                .to_vec(),
        )
        .unwrap();

    let bytes = tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        while let Some(chunk) = output.next().await {
            bytes.extend(chunk.unwrap());
        }
        bytes
    })
    .await
    .expect("terminal output did not reach EOF after natural exit");

    assert!(String::from_utf8_lossy(&bytes).contains("final-pty-bytes"));
    service.close(terminal).unwrap();
}

#[cfg(windows)]
#[test]
fn terminal_service_windows_close_watchdog_interrupts_conpty_io() {
    let cwd = std::env::current_dir().unwrap();
    let service = std::sync::Arc::new(LocalTerminalService::new());
    let terminal = service.spawn(&cwd).unwrap();
    service
        .write(terminal, b"ping -n 31 127.0.0.1 >nul\r\n".to_vec())
        .unwrap();

    let started = std::time::Instant::now();
    service
        .write(terminal, vec![b'x'; 32 * 1024 * 1024])
        .unwrap();
    assert!(started.elapsed() < Duration::from_millis(250));
    std::thread::sleep(Duration::from_millis(250));

    let (close_tx, close_rx) = std::sync::mpsc::channel();
    let closer = std::thread::spawn(move || {
        let _ = close_tx.send(service.close(terminal));
    });
    let close_result = close_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("ConPTY close timed out while terminal IO was active");
    closer.join().unwrap();
    close_result.unwrap();
}

#[cfg(unix)]
#[test]
fn terminal_service_close_interrupts_a_large_write_to_a_child_that_is_not_reading() {
    let cwd = std::env::current_dir().unwrap();
    let service = Arc::new(LocalTerminalService::new());
    let terminal = service.spawn(&cwd).unwrap();
    let shell_pid_file = unique_pid_file("blocked-write-shell");
    service
        .write(
            terminal,
            format!("echo $$ > {} ; sleep 30\r\n", shell_pid_file.display()).into_bytes(),
        )
        .unwrap();
    let shell_pid = wait_for_pid_file(&shell_pid_file);

    let large_input = vec![b'x'; 32 * 1024 * 1024];
    let started = Instant::now();
    service.write(terminal, large_input).unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "large PTY write blocked instead of returning after enqueue"
    );
    thread::sleep(Duration::from_millis(250));

    let (close_tx, close_rx) = mpsc::channel();
    let close_service = Arc::clone(&service);
    let closer = thread::spawn(move || {
        let _ = close_tx.send(close_service.close(terminal));
    });
    let close_result = close_rx.recv_timeout(Duration::from_secs(2));

    if close_result.is_err() {
        kill_process_group(shell_pid);
    }
    let _ = closer.join();
    let _ = std::fs::remove_file(shell_pid_file);
    close_result
        .expect("terminal close timed out behind a blocked write")
        .unwrap();
}

#[cfg(unix)]
#[test]
fn terminal_service_close_stops_a_descendant_that_retains_the_pty() {
    let cwd = std::env::current_dir().unwrap();
    let service = LocalTerminalService::new();
    let terminal = service.spawn(&cwd).unwrap();
    let shell_pid_file = unique_pid_file("descendant-shell");
    let descendant_pid_file = unique_pid_file("descendant-child");
    service
        .write(
            terminal,
            format!(
                "echo $$ > {} ; sh -c 'trap \"\" HUP TERM; echo $$ > {}; exec sleep 30' & exit\r\n",
                shell_pid_file.display(),
                descendant_pid_file.display()
            )
            .into_bytes(),
        )
        .unwrap();
    let shell_pid = wait_for_pid_file(&shell_pid_file);
    let descendant_pid = wait_for_pid_file(&descendant_pid_file);

    let (close_tx, close_rx) = mpsc::channel();
    let closer = thread::spawn(move || {
        let _ = close_tx.send(service.close(terminal));
    });
    let close_result = close_rx.recv_timeout(Duration::from_secs(2));

    if close_result.is_err() {
        kill_process_group(shell_pid);
    }
    let _ = closer.join();
    let stopped = wait_until(Duration::from_secs(1), || !process_is_alive(descendant_pid));
    if !stopped {
        kill_process(descendant_pid);
    }
    let _ = std::fs::remove_file(shell_pid_file);
    let _ = std::fs::remove_file(descendant_pid_file);
    close_result
        .expect("terminal close timed out while a descendant retained the PTY")
        .unwrap();
    assert!(stopped, "terminal descendant survived close");
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_service_caps_output_when_the_subscriber_is_stalled() {
    const OUTPUT_BYTES: usize = 4 * 1024 * 1024;
    const MAX_BUFFERED_BYTES: usize = 1024 * 1024;

    let cwd = std::env::current_dir().unwrap();
    let service = LocalTerminalService::new();
    let terminal = service.spawn(&cwd).unwrap();
    let mut output = service.subscribe(terminal).unwrap();
    let shell_pid_file = unique_pid_file("bounded-output-shell");
    service
        .write(
            terminal,
            format!(
                "echo $$ > {} ; yes z | head -c {OUTPUT_BYTES}\r\n",
                shell_pid_file.display()
            )
            .into_bytes(),
        )
        .unwrap();
    let shell_pid = wait_for_pid_file(&shell_pid_file);

    tokio::time::sleep(Duration::from_secs(1)).await;
    let (close_tx, close_rx) = mpsc::channel();
    let closer = thread::spawn(move || {
        let _ = close_tx.send(service.close(terminal));
    });
    let close_result = match close_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(result) => result,
        Err(error) => {
            kill_process_group(shell_pid);
            #[cfg(target_os = "macos")]
            dump_process_sample();
            let _ = std::fs::remove_file(shell_pid_file);
            drop(closer);
            panic!("terminal close timed out with a stalled output subscriber: {error}");
        }
    };
    closer.join().unwrap();
    let _ = std::fs::remove_file(shell_pid_file);
    close_result.unwrap();

    let buffered = tokio::time::timeout(Duration::from_secs(2), async {
        let mut bytes = 0;
        while let Some(chunk) = output.next().await {
            bytes += chunk.unwrap().len();
        }
        bytes
    })
    .await
    .expect("terminal output stream did not close");
    assert!(buffered > 0, "terminal produced no buffered output");
    assert!(
        buffered <= MAX_BUFFERED_BYTES,
        "stalled terminal subscriber buffered {buffered} bytes"
    );
}

#[cfg(unix)]
fn unique_pid_file(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "zode-terminal-{label}-{}-{nonce}.pid",
        std::process::id()
    ))
}

#[cfg(unix)]
fn wait_for_pid_file(path: &Path) -> i32 {
    let found = wait_until(Duration::from_secs(2), || {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|value| value.trim().parse::<i32>().ok())
            .is_some()
    });
    assert!(found, "terminal did not create pid file {}", path.display());
    std::fs::read_to_string(path)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

#[cfg(unix)]
fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    predicate()
}

#[cfg(unix)]
fn process_is_alive(pid: i32) -> bool {
    let output = std::process::Command::new("/bin/ps")
        .args(["-o", "state=", "-p", &pid.to_string()])
        .output()
        .expect("failed to inspect terminal descendant state");
    output.status.success()
        && output
            .stdout
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            .is_some_and(|state| state != b'Z')
}

#[cfg(unix)]
fn kill_process(pid: i32) {
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status();
}

#[cfg(unix)]
fn kill_process_group(group: i32) {
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &format!("-{group}")])
        .status();
}

#[cfg(target_os = "macos")]
fn dump_process_sample() {
    let output = std::process::Command::new("/usr/bin/sample")
        .args([
            &std::process::id().to_string(),
            "1",
            "10",
            "-mayDie",
            "-file",
            "/dev/stdout",
        ])
        .output()
        .expect("failed to sample stalled terminal test");
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.status.success() {
        eprintln!("sample failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}
