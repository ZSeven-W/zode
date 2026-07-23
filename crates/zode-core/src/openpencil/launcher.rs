//! Launch the OpenPencil GUI via `op start`. `op start` itself detects the
//! desktop binary (env, colocated, target dirs, macOS .app, Linux/AppImage)
//! and errors clearly if none is found — we surface that error rather than
//! gating on desktop detection here.

use std::path::Path;
use std::process::Command;

use agent::abort::AbortController;

use super::OpError;
use crate::config::OpenPencilConfig;

/// Launch the GUI (detached). `cfg.launch_command()` is split on whitespace;
/// the first token (conventionally "op") is dropped and replaced by the
/// resolved binary path so the command works regardless of whether the binary
/// is named `op`, `op.exe`, or is at a custom path. stdio is nulled so
/// `op start`'s JSON/log output cannot bleed into the TUI.
pub fn launch_gui(
    op: &Path,
    cfg: &OpenPencilConfig,
    abort: &AbortController,
) -> Result<(), OpError> {
    use std::process::Stdio;

    if abort.is_aborted() {
        return Err(OpError::Aborted(
            abort.reason().unwrap_or_else(|| "aborted".to_string()),
        ));
    }

    // Split launch_command; skip the first token ("op" / "op.exe") and pass
    // the remaining args (e.g. ["start"] or ["start", "--headless"]) to the
    // resolved binary.
    let mut parts = cfg.launch_command().split_whitespace();
    let _ = parts.next(); // drop the placeholder binary name
    let mut cmd = Command::new(op);
    for arg in parts {
        cmd.arg(arg);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // The GUI is intentionally detached product behavior. Latch the root turn
    // before spawn, then record unresolved external work after success so a
    // scheduler watchdog cannot treat the turn as safe to replay.
    abort.mark_side_effect_risk();
    cmd.spawn()
        .map(|_| abort.mark_unresolved_external_work())
        .map_err(|e| OpError::NoInstance(format!("op start: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn launch_command_drops_first_token() {
        // Verify that splitting "op start" drops "op" leaving ["start"]; if the
        // resolved path is used as the binary, args must only be the tail.
        let cmd = "op start";
        let mut parts = cmd.split_whitespace();
        let _ = parts.next();
        let args: Vec<&str> = parts.collect();
        assert_eq!(args, vec!["start"]);
    }

    #[test]
    fn launch_command_custom_args_preserved() {
        let cmd = "op start --headless";
        let mut parts = cmd.split_whitespace();
        let _ = parts.next();
        let args: Vec<&str> = parts.collect();
        assert_eq!(args, vec!["start", "--headless"]);
    }

    #[test]
    fn launch_gui_nonexistent_binary_returns_error() {
        let cfg = crate::config::OpenPencilConfig::default();
        let abort = AbortController::new();
        // Use a definitely-nonexistent path.
        let res = launch_gui(Path::new("/nonexistent-binary-zode-test"), &cfg, &abort);
        assert!(res.is_err(), "expected Err for nonexistent binary");
        assert!(abort.activity().side_effect_risk());
        assert!(!abort.activity().unresolved_external_work());
        match res.unwrap_err() {
            OpError::NoInstance(m) => assert!(m.contains("op start"), "got: {m}"),
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn successful_detached_launch_is_visible_to_scheduler_policy() {
        let cfg = crate::config::OpenPencilConfig {
            launch_command: Some("op".to_string()),
            ..Default::default()
        };
        let abort = AbortController::new();
        launch_gui(Path::new("/usr/bin/true"), &cfg, &abort).unwrap();
        assert!(abort.activity().side_effect_risk());
        assert!(abort.activity().unresolved_external_work());
    }

    #[test]
    fn aborted_launch_never_spawns_or_marks_side_effects() {
        let cfg = crate::config::OpenPencilConfig::default();
        let abort = AbortController::new();
        abort.abort_with_reason("cancelled");
        let result = launch_gui(Path::new("/nonexistent-binary-zode-test"), &cfg, &abort);
        assert!(matches!(result, Err(OpError::Aborted(reason)) if reason == "cancelled"));
        assert!(!abort.activity().side_effect_risk());
    }
}
