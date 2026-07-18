//! Real-helper integration test. Opt-in: needs a logged-in macOS session
//! (WindowServer). Run with:
//!   ZODE_DESKTOP_IT=1 cargo test -p zode-overlay --test overlay-it -- --ignored

#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
#[ignore]
fn overlay_helper_end_to_end() {
    if std::env::var("ZODE_DESKTOP_IT").as_deref() != Ok("1") {
        eprintln!("ZODE_DESKTOP_IT!=1 — skipping");
        return;
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_zode-overlay"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn helper");
    let mut stdin = child.stdin.take().unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), r#"{"ready":true}"#);

    for line in [
        r#"{"cmd":"show","banner":"it","esc_hint":"esc"}"#,
        r#"{"cmd":"move","x":400.0,"y":300.0,"pulse":"click"}"#,
        r#"{"cmd":"chip","text":"⌨ Cmd+F"}"#,
        r#"{"cmd":"hide"}"#,
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    stdin.flush().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        child.try_wait().unwrap().is_none(),
        "helper died mid-session"
    );

    writeln!(stdin, r#"{{"cmd":"quit"}}"#).unwrap();
    stdin.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "helper exit status: {status:?}");
            break;
        }
        assert!(Instant::now() < deadline, "helper did not exit on quit");
        std::thread::sleep(Duration::from_millis(50));
    }
}
