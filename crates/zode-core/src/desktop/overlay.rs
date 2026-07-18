//! Ghost-cursor overlay: process-wide handle to the `zode-overlay` helper.
//! Strictly best-effort — the forwarder thread lazily spawns the helper on
//! the first visualized command, and any failure (missing binary, dead pipe)
//! disables visualization for the session without surfacing an error.
//! Wire format: JSON lines; the parse side lives in
//! crates/zode-overlay/src/proto.rs (golden tests pin both copies).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::OnceLock;

use serde::Serialize;

use crate::config::DesktopConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Pulse {
    Click,
    Scroll,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum OverlayCmd {
    Show {
        banner: String,
        esc_hint: String,
    },
    Move {
        x: f64,
        y: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        window_id: Option<u32>,
        pulse: Pulse,
    },
    Chip {
        text: String,
    },
    Hide,
    Quit,
}

pub type OverlaySink = Sender<OverlayCmd>;

/// Owns the forwarder thread. The process-wide instance never drops; direct
/// construction (tests) shuts the helper down on drop.
pub struct OverlayHandle {
    tx: Sender<OverlayCmd>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl OverlayHandle {
    pub fn start(helper: PathBuf, banner: String, esc_hint: String) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let join = std::thread::Builder::new()
            .name("zode-overlay-fwd".into())
            .spawn(move || forward(rx, helper, banner, esc_hint))
            .ok();
        Self { tx, join }
    }

    pub fn sink(&self) -> OverlaySink {
        self.tx.clone()
    }

    pub fn hide(&self) {
        let _ = self.tx.send(OverlayCmd::Hide);
    }
}

impl Drop for OverlayHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(OverlayCmd::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn forward(rx: Receiver<OverlayCmd>, helper: PathBuf, banner: String, esc_hint: String) {
    let mut child: Option<Child> = None;
    let mut stdin: Option<ChildStdin> = None;
    let mut disabled = false;
    let mut needs_show = true;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            OverlayCmd::Quit => break,
            OverlayCmd::Hide => {
                if let Some(s) = stdin.as_mut() {
                    let _ = write_line(s, &OverlayCmd::Hide);
                }
                needs_show = true;
            }
            other => {
                if disabled {
                    continue;
                }
                if stdin.is_none() {
                    match spawn_helper(&helper) {
                        Ok((c, i)) => {
                            child = Some(c);
                            stdin = Some(i);
                        }
                        Err(e) => {
                            tracing::debug!("zode-overlay spawn failed ({helper:?}): {e}");
                            disabled = true;
                            continue;
                        }
                    }
                }
                let s = stdin.as_mut().expect("spawned above");
                let mut ok = true;
                if needs_show {
                    ok = write_line(
                        s,
                        &OverlayCmd::Show {
                            banner: banner.clone(),
                            esc_hint: esc_hint.clone(),
                        },
                    )
                    .is_ok();
                    needs_show = false;
                }
                if ok {
                    ok = write_line(s, &other).is_ok();
                }
                if !ok {
                    tracing::debug!("zode-overlay pipe broke; visualization disabled");
                    disabled = true;
                    stdin = None;
                    if let Some(mut c) = child.take() {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                }
            }
        }
    }

    // Shutdown: best-effort quit line, then make sure the child is gone.
    if let Some(s) = stdin.as_mut() {
        let _ = write_line(s, &OverlayCmd::Quit);
    }
    drop(stdin); // EOF is the helper's second exit signal
    if let Some(child) = child.take() {
        wait_or_kill(child);
    }
}

/// Let the helper exit on the `quit` line / stdin EOF, polling for up to ~2s so
/// a well-behaved child is reaped cleanly (never SIGKILLed mid-flush). Only a
/// helper that ignores both signals is force-killed as a last resort.
fn wait_or_kill(mut child: Child) {
    for _ in 0..100 {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_helper(helper: &PathBuf) -> std::io::Result<(Child, ChildStdin)> {
    let mut c = Command::new(helper)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let stdin = c
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "no child stdin"))?;
    Ok((c, stdin))
}

fn write_line(s: &mut ChildStdin, cmd: &OverlayCmd) -> std::io::Result<()> {
    let line = serde_json::to_string(cmd)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    writeln!(s, "{line}")?;
    s.flush()
}

// ── Process-wide instance ──

static GLOBAL: OnceLock<Option<OverlayHandle>> = OnceLock::new();

/// Initialize (once) and return a sink, or None when visualization is off:
/// non-macOS, `desktop.ghostCursor=false`, or no helper binary found.
pub fn global(cfg: &DesktopConfig) -> Option<OverlaySink> {
    GLOBAL
        .get_or_init(|| {
            if !cfg!(target_os = "macos") || !cfg.ghost_cursor() {
                return None;
            }
            let helper = helper_path(cfg)?;
            Some(OverlayHandle::start(
                helper,
                crate::i18n::t("zode is controlling your computer").to_string(),
                crate::i18n::t("press Esc to stop").to_string(),
            ))
        })
        .as_ref()
        .map(|h| h.sink())
}

/// Hide the overlay if the process-wide instance exists (turn end, Esc).
pub fn hide_global() {
    if let Some(h) = GLOBAL.get().and_then(|o| o.as_ref()) {
        h.hide();
    }
}

/// Explicit config path, else `zode-overlay` next to the running executable.
/// Must exist on disk — a missing helper means visualization stays off.
fn helper_path(cfg: &DesktopConfig) -> Option<PathBuf> {
    let explicit = cfg.overlay_helper_path().map(PathBuf::from);
    let candidate = explicit.or_else(|| {
        let exe = std::env::current_exe().ok()?;
        let name = if cfg!(windows) {
            "zode-overlay.exe"
        } else {
            "zode-overlay"
        };
        Some(exe.parent()?.join(name))
    })?;
    candidate.exists().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Golden wire fixtures — keep byte-identical with
    // crates/zode-overlay/src/proto.rs. ──
    const G_SHOW: &str = r#"{"cmd":"show","banner":"b","esc_hint":"e"}"#;
    const G_MOVE: &str = r#"{"cmd":"move","x":10.0,"y":20.5,"window_id":42,"pulse":"click"}"#;
    const G_MOVE_NOWIN: &str = r#"{"cmd":"move","x":1.0,"y":2.0,"pulse":"none"}"#;
    const G_CHIP: &str = r#"{"cmd":"chip","text":"⌨ Cmd+F"}"#;
    const G_HIDE: &str = r#"{"cmd":"hide"}"#;
    const G_QUIT: &str = r#"{"cmd":"quit"}"#;

    #[test]
    fn golden_serialize_matches_helper_parser_fixtures() {
        let cases: Vec<(OverlayCmd, &str)> = vec![
            (
                OverlayCmd::Show {
                    banner: "b".into(),
                    esc_hint: "e".into(),
                },
                G_SHOW,
            ),
            (
                OverlayCmd::Move {
                    x: 10.0,
                    y: 20.5,
                    window_id: Some(42),
                    pulse: Pulse::Click,
                },
                G_MOVE,
            ),
            (
                OverlayCmd::Move {
                    x: 1.0,
                    y: 2.0,
                    window_id: None,
                    pulse: Pulse::None,
                },
                G_MOVE_NOWIN,
            ),
            (
                OverlayCmd::Chip {
                    text: "⌨ Cmd+F".into(),
                },
                G_CHIP,
            ),
            (OverlayCmd::Hide, G_HIDE),
            (OverlayCmd::Quit, G_QUIT),
        ];
        for (cmd, want) in cases {
            assert_eq!(serde_json::to_string(&cmd).unwrap(), want);
        }
    }

    /// End-to-end through a fake helper: a shell script that copies stdin to a
    /// file. Verifies lazy spawn, the auto-`show` prefix, ordered writes, and
    /// quit-on-drop.
    #[cfg(unix)]
    #[test]
    fn forwarder_writes_lines_through_fake_helper() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.jsonl");
        let script = dir.path().join("fake-overlay.sh");
        std::fs::write(&script, format!("#!/bin/sh\ncat > {}\n", out.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let h = OverlayHandle::start(script, "B".into(), "E".into());
        h.sink()
            .send(OverlayCmd::Move {
                x: 1.0,
                y: 2.0,
                window_id: None,
                pulse: Pulse::Click,
            })
            .unwrap();
        h.hide();
        drop(h); // sends quit, joins the forwarder

        let text = std::fs::read_to_string(&out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "show, move, hide, quit — got: {text}");
        assert!(lines[0].contains("\"cmd\":\"show\"") && lines[0].contains("\"banner\":\"B\""));
        assert!(lines[1].contains("\"cmd\":\"move\""));
        assert!(lines[2].contains("\"cmd\":\"hide\""));
        assert!(lines[3].contains("\"cmd\":\"quit\""));
    }

    /// A missing helper binary must disable quietly — sends still succeed.
    #[test]
    fn missing_helper_disables_without_error() {
        let h = OverlayHandle::start(
            std::path::PathBuf::from("/nonexistent/zode-overlay-nope"),
            "B".into(),
            "E".into(),
        );
        h.sink()
            .send(OverlayCmd::Chip { text: "x".into() })
            .unwrap();
        drop(h); // must not hang or panic
    }
}
