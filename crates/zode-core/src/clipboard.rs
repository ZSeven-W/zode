//! System clipboard via the platform CLI — no native dependency (avoids
//! pulling X11/Wayland crates into the build). Used by `/copy`.
//!
//! macOS: `pbcopy`. Windows: `clip`. Linux: `wl-copy` (Wayland), then
//! `xclip` / `xsel` (X11), whichever is installed.

use std::io::Write;
use std::process::{Command, Stdio};

use tokio::io::AsyncReadExt;

const ASYNC_CLIPBOARD_TEXT_BYTE_CAP: usize = 262_144;

/// Copy `text` to the system clipboard. On success returns the helper used
/// (e.g. `"pbcopy"`); on failure returns a human-readable error.
pub fn copy_to_clipboard(text: &str) -> Result<&'static str, String> {
    let mut last_err = "no clipboard helper found".to_string();
    for (bin, args) in clipboard_candidates() {
        match try_copy(bin, args, text) {
            Ok(()) => return Ok(bin),
            Err(e) => last_err = format!("{bin}: {e}"),
        }
    }
    Err(last_err)
}

/// Read UTF-8 text from the system clipboard. Used as a Ctrl+V fallback when
/// the terminal doesn't emit a bracketed paste event.
pub fn read_from_clipboard() -> Result<String, String> {
    let mut last_err = "no clipboard paste helper found".to_string();
    for (bin, args) in paste_candidates() {
        match try_paste(bin, args) {
            Ok(text) => return Ok(text),
            Err(e) => last_err = format!("{bin}: {e}"),
        }
    }
    Err(last_err)
}

/// Read UTF-8 clipboard text without allowing a platform helper to block
/// longer than `timeout` per candidate. Stdout is bounded to 262,144 bytes.
pub async fn read_from_clipboard_with_timeout(
    timeout: std::time::Duration,
) -> Result<String, String> {
    read_from_clipboard_candidates_with_timeout(paste_candidates(), timeout).await
}

/// Read raw IMAGE bytes from the system clipboard (a screenshot or a copied
/// image — not a file path). `Ok(None)` means the clipboard holds no image, so
/// the caller falls back to a text paste. Returns PNG bytes on macOS/Linux;
/// other platforms return `Ok(None)`.
///
/// Terminals deliver pastes as text and never hand image data to a TUI, so this
/// queries the OS clipboard directly (the same reason Ctrl+V reads it for text).
pub fn read_image_from_clipboard() -> Result<Option<Vec<u8>>, String> {
    if cfg!(target_os = "macos") {
        macos_clipboard_image()
    } else if cfg!(target_os = "linux") {
        linux_clipboard_image()
    } else {
        Ok(None)
    }
}

/// macOS: AppleScript reads the pasteboard as PNG and writes it to a temp file
/// (osascript can't emit binary on stdout), returning the path — or "" when the
/// clipboard holds no image.
fn macos_clipboard_image() -> Result<Option<Vec<u8>>, String> {
    let script = "try\n\
            set png to (the clipboard as «class PNGf»)\n\
        on error\n\
            return \"\"\n\
        end try\n\
        set tmp to (POSIX path of (path to temporary items)) & \"zode_clipboard.png\"\n\
        set fh to open for access (POSIX file tmp) with write permission\n\
        set eof fh to 0\n\
        write png to fh\n\
        close access fh\n\
        return tmp";
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("osascript clipboard read failed".into());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    Ok((!bytes.is_empty()).then_some(bytes))
}

/// Linux: ask Wayland (`wl-paste`) then X11 (`xclip`) for `image/png` bytes.
/// A missing helper or absent image yields `Ok(None)` so text paste can run.
fn linux_clipboard_image() -> Result<Option<Vec<u8>>, String> {
    let candidates: &[(&str, &[&str])] = &[
        ("wl-paste", &["--type", "image/png"]),
        (
            "xclip",
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        ),
    ];
    for (bin, args) in candidates {
        if let Ok(out) = Command::new(bin)
            .args(*args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
        {
            if out.status.success() && !out.stdout.is_empty() {
                return Ok(Some(out.stdout));
            }
        }
    }
    Ok(None)
}

/// Clipboard CLIs to try, in order, for the current platform.
fn clipboard_candidates() -> &'static [(&'static str, &'static [&'static str])] {
    if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(target_os = "windows") {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    }
}

const WINDOWS_PASTE_ARGS: &[&str] = &[
    "-NoProfile",
    "-NonInteractive",
    "-Command",
    "$text = [string](Get-Clipboard -Raw); $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($text); [Console]::OpenStandardOutput().Write($bytes, 0, $bytes.Length)",
];

fn paste_candidates() -> &'static [(&'static str, &'static [&'static str])] {
    if cfg!(target_os = "macos") {
        &[("pbpaste", &[])]
    } else if cfg!(target_os = "windows") {
        &[("powershell", WINDOWS_PASTE_ARGS)]
    } else {
        &[
            ("wl-paste", &["-n"]),
            ("xclip", &["-selection", "clipboard", "-out"]),
            ("xsel", &["--clipboard", "--output"]),
        ]
    }
}

fn try_copy(bin: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .as_mut()
        .ok_or("no stdin handle")?
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("helper exited with {status}"))
    }
}

fn try_paste(bin: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("helper exited with {}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

async fn read_from_clipboard_candidates_with_timeout(
    candidates: &[(&str, &[&str])],
    timeout: std::time::Duration,
) -> Result<String, String> {
    read_from_clipboard_candidates_with_timeout_and_cap(
        candidates,
        timeout,
        ASYNC_CLIPBOARD_TEXT_BYTE_CAP,
    )
    .await
}

enum BoundedClipboardOutput {
    Complete {
        status: std::process::ExitStatus,
        bytes: Vec<u8>,
    },
    Oversized,
}

async fn read_bounded_clipboard_output(
    child: &mut tokio::process::Child,
    stdout: tokio::process::ChildStdout,
    max_stdout_bytes: usize,
) -> std::io::Result<BoundedClipboardOutput> {
    let read_limit = max_stdout_bytes
        .checked_add(1)
        .and_then(|limit| u64::try_from(limit).ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "clipboard stdout byte cap is too large",
            )
        })?;
    let mut bytes = Vec::new();
    stdout.take(read_limit).read_to_end(&mut bytes).await?;
    if bytes.len() > max_stdout_bytes {
        return Ok(BoundedClipboardOutput::Oversized);
    }
    let status = child.wait().await?;
    Ok(BoundedClipboardOutput::Complete { status, bytes })
}

async fn kill_and_wait_for_clipboard_helper(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

async fn read_from_clipboard_candidates_with_timeout_and_cap(
    candidates: &[(&str, &[&str])],
    timeout: std::time::Duration,
    max_stdout_bytes: usize,
) -> Result<String, String> {
    let mut last_err = "no clipboard paste helper found".to_string();
    for &(bin, args) in candidates {
        let mut command = tokio::process::Command::new(bin);
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                last_err = format!("{bin}: {error}");
                continue;
            }
        };
        let Some(stdout) = child.stdout.take() else {
            kill_and_wait_for_clipboard_helper(&mut child).await;
            last_err = format!("{bin}: no stdout handle");
            continue;
        };

        let outcome = {
            let read = read_bounded_clipboard_output(&mut child, stdout, max_stdout_bytes);
            tokio::time::timeout(timeout, read).await
        };
        let (status, bytes) = match outcome {
            Err(_) => {
                kill_and_wait_for_clipboard_helper(&mut child).await;
                last_err = format!("{bin}: helper timed out after {timeout:?}");
                continue;
            }
            Ok(Err(error)) => {
                kill_and_wait_for_clipboard_helper(&mut child).await;
                last_err = format!("{bin}: {error}");
                continue;
            }
            Ok(Ok(BoundedClipboardOutput::Oversized)) => {
                kill_and_wait_for_clipboard_helper(&mut child).await;
                last_err = format!("{bin}: helper output exceeded {max_stdout_bytes} bytes");
                continue;
            }
            Ok(Ok(BoundedClipboardOutput::Complete { status, bytes })) => (status, bytes),
        };

        if !status.success() {
            last_err = format!("{bin}: helper exited with {status}");
            continue;
        }
        match String::from_utf8(bytes) {
            Ok(text) => return Ok(text),
            Err(error) => last_err = format!("{bin}: {error}"),
        }
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paste_helper_writes_raw_clipboard_text() {
        assert_eq!(
            WINDOWS_PASTE_ARGS,
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$text = [string](Get-Clipboard -Raw); $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($text); [Console]::OpenStandardOutput().Write($bytes, 0, $bytes.Length)",
            ]
        );
    }

    #[test]
    fn candidates_are_non_empty_for_this_platform() {
        // Every supported target has at least one helper to try.
        assert!(!clipboard_candidates().is_empty());
        assert!(!paste_candidates().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn async_paste_helper_preserves_exact_utf8_output() {
        let candidates: &[(&str, &[&str])] = &[("sh", &["-c", "printf 'first\\n第二行\\n'"])];

        let text = read_from_clipboard_candidates_with_timeout(
            candidates,
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap();

        assert_eq!(text, "first\n第二行\n");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn async_paste_helper_times_out_and_names_the_helper() {
        let timeout = std::time::Duration::from_millis(20);
        let candidates: &[(&str, &[&str])] = &[("sh", &["-c", "exec sleep 5"])];

        let error = read_from_clipboard_candidates_with_timeout(candidates, timeout)
            .await
            .unwrap_err();

        assert_eq!(error, format!("sh: helper timed out after {timeout:?}"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn async_paste_helper_reports_nonzero_exit() {
        let candidates: &[(&str, &[&str])] = &[("sh", &["-c", "exit 7"])];

        let error = read_from_clipboard_candidates_with_timeout(
            candidates,
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap_err();

        assert!(error.starts_with("sh: helper exited with "));
        assert!(error.contains('7'));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn async_paste_helper_reports_invalid_utf8() {
        let candidates: &[(&str, &[&str])] = &[("sh", &["-c", "printf '\\377'"])];

        let error = read_from_clipboard_candidates_with_timeout(
            candidates,
            std::time::Duration::from_secs(1),
        )
        .await
        .unwrap_err();

        assert!(error.starts_with("sh: "));
        assert!(error.contains("invalid utf-8"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn async_paste_helper_accepts_output_at_the_byte_cap() {
        let candidates: &[(&str, &[&str])] = &[("sh", &["-c", "printf 1234"])];

        let text = read_from_clipboard_candidates_with_timeout_and_cap(
            candidates,
            std::time::Duration::from_secs(1),
            4,
        )
        .await
        .unwrap();

        assert_eq!(text, "1234");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn async_paste_helper_rejects_cap_plus_one_without_waiting_for_exit() {
        let candidates: &[(&str, &[&str])] = &[("sh", &["-c", "printf 12345; exec sleep 5"])];

        let error = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            read_from_clipboard_candidates_with_timeout_and_cap(
                candidates,
                std::time::Duration::from_secs(5),
                4,
            ),
        )
        .await
        .expect("cap+1 output should be rejected before the helper exits")
        .unwrap_err();

        assert_eq!(error, "sh: helper output exceeded 4 bytes");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn async_paste_helper_reaps_the_process_after_timeout() {
        let temp = tempfile::tempdir().unwrap();
        let pid_path = temp.path().join("helper.pid");
        let pid_path = pid_path.to_string_lossy().into_owned();
        let args = [
            "-c",
            "printf '%s' \"$$\" > \"$1\"; exec sleep 5",
            "sh",
            pid_path.as_str(),
        ];
        let candidates: &[(&str, &[&str])] = &[("sh", &args)];
        let timeout = std::time::Duration::from_millis(500);

        let error = read_from_clipboard_candidates_with_timeout_and_cap(candidates, timeout, 4)
            .await
            .unwrap_err();

        assert_eq!(error, format!("sh: helper timed out after {timeout:?}"));
        let pid = std::fs::read_to_string(pid_path).unwrap();
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("kill -0 {pid} 2>/dev/null"))
            .status()
            .unwrap();
        assert!(!status.success(), "timed-out helper {pid} is still alive");
    }
}
