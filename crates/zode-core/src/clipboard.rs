//! System clipboard via the platform CLI — no native dependency (avoids
//! pulling X11/Wayland crates into the build). Used by `/copy`.
//!
//! macOS: `pbcopy`. Windows: `clip`. Linux: `wl-copy` (Wayland), then
//! `xclip` / `xsel` (X11), whichever is installed.

use std::io::Write;
use std::process::{Command, Stdio};

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
}
