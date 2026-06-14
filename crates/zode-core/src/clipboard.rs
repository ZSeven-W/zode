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

fn paste_candidates() -> &'static [(&'static str, &'static [&'static str])] {
    if cfg!(target_os = "macos") {
        &[("pbpaste", &[])]
    } else if cfg!(target_os = "windows") {
        &[("powershell", &["-NoProfile", "-Command", "Get-Clipboard"])]
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
    fn candidates_are_non_empty_for_this_platform() {
        // Every supported target has at least one helper to try.
        assert!(!clipboard_candidates().is_empty());
        assert!(!paste_candidates().is_empty());
    }
}
