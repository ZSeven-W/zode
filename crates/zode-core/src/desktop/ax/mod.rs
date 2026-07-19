//! macOS AXUIElement backend. A dedicated OS thread owns every native handle
//! (AXUIElement pointers never cross threads); the async `AxBackend` forwards
//! commands over a channel and awaits results with a bounded timeout, so a
//! wedged provider surfaces as `Timeout` (the actor handle then replaces this
//! backend). See spec §线程与执行模型.

#![cfg(target_os = "macos")]

mod element;
mod input;
mod paste;
mod screenshot;
mod tree;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use accessibility_sys::AXIsProcessTrusted;

use crate::desktop::backend::{
    AppId, AppInfo, AppLaunchId, DesktopBackend, DesktopBackendFactory, DesktopError,
    ElementActionKind, ElementRef, Screenshot, SnapshotResult, WindowId, WindowInfo,
};

use element::{attr_string, AxElem};

/// Whether this process is trusted for the Accessibility API. When false, every
/// backend operation should surface `PermissionDenied` with runtime guidance.
pub fn ax_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Runtime-probed guidance for granting Accessibility to the *actual* running
/// executable (which may be a terminal, IDE, or the zode binary itself).
pub fn permission_guidance() -> String {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "the running process".to_string());
    format!(
        "Accessibility (AX) permission is required. Grant it to {exe} in System Settings → \
         Privacy & Security → Accessibility, then restart zode. Note: the authorized app is the \
         one that launched zode (terminal/IDE), not necessarily zode itself."
    )
}

/// Factory that spins up an `AxBackend` (its own actor thread).
#[derive(Debug, Default)]
pub struct AxFactory {
    /// Ghost-cursor sink; None when visualization is disabled.
    pub overlay: Option<crate::desktop::overlay::OverlaySink>,
}

#[async_trait]
impl DesktopBackendFactory for AxFactory {
    async fn create(&self) -> Result<Arc<dyn DesktopBackend>, DesktopError> {
        Ok(AxBackend::spawn(self.overlay.clone()))
    }
}

/// Commands sent to the actor thread. Every payload is `Send`; native handles
/// are created and dropped entirely on the actor thread.
enum AxCommand {
    ListApps(oneshot::Sender<Result<Vec<AppInfo>, DesktopError>>),
    ListWindows {
        pid: i32,
        resp: oneshot::Sender<Result<Vec<WindowInfo>, DesktopError>>,
    },
    Snapshot {
        pid: i32,
        index: usize,
        max_nodes: usize,
        resp: oneshot::Sender<Result<SnapshotResult, DesktopError>>,
    },
    ElementAction {
        pid: i32,
        index: usize,
        local_id: u64,
        kind: ElementActionKind,
        resp: oneshot::Sender<Result<String, DesktopError>>,
    },
    SetValue {
        pid: i32,
        index: usize,
        local_id: u64,
        text: String,
        resp: oneshot::Sender<Result<(), DesktopError>>,
    },
    TypeText {
        pid: i32,
        text: String,
        resp: oneshot::Sender<Result<(), DesktopError>>,
    },
    Key {
        pid: i32,
        combo: String,
        resp: oneshot::Sender<Result<(), DesktopError>>,
    },
    Focus {
        pid: i32,
        index: usize,
        resp: oneshot::Sender<Result<(), DesktopError>>,
    },
    Launch {
        exe: String,
        resp: oneshot::Sender<Result<AppInfo, DesktopError>>,
    },
    Screenshot {
        pid: i32,
        index: usize,
        resp: oneshot::Sender<Result<Vec<u8>, DesktopError>>,
    },
    Ping(oneshot::Sender<()>),
}

#[derive(Debug)]
pub struct AxBackend {
    tx: mpsc::UnboundedSender<AxCommand>,
}

impl AxBackend {
    pub fn spawn(overlay: Option<crate::desktop::overlay::OverlaySink>) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("zode-ax-actor".into())
            .spawn(move || actor_loop(rx, overlay))
            .expect("spawn ax actor thread");
        Arc::new(Self { tx })
    }

    async fn send<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T, DesktopError>>) -> AxCommand,
    ) -> Result<T, DesktopError> {
        if !ax_trusted() {
            return Err(DesktopError::PermissionDenied(permission_guidance()));
        }
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(make(resp_tx))
            .map_err(|_| DesktopError::Dead("ax actor thread gone".into()))?;
        match tokio::time::timeout(Duration::from_secs(10), resp_rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => Err(DesktopError::Dead("ax actor dropped the response".into())),
            Err(_) => Err(DesktopError::Timeout("ax command timed out".into())),
        }
    }
}

fn parse_pid(exe: &str) -> Option<i32> {
    exe.rsplit('#').next()?.parse().ok()
}

#[async_trait]
impl DesktopBackend for AxBackend {
    async fn list_apps(&self) -> Result<Vec<AppInfo>, DesktopError> {
        self.send(AxCommand::ListApps).await
    }

    async fn list_windows(&self, app: &AppId) -> Result<Vec<WindowInfo>, DesktopError> {
        let pid = parse_pid(app.executable_identity())
            .ok_or_else(|| DesktopError::NotFound("app identity has no pid".into()))?;
        self.send(|resp| AxCommand::ListWindows { pid, resp }).await
    }

    async fn snapshot(
        &self,
        win: &WindowId,
        _scope: Option<ElementRef>,
    ) -> Result<SnapshotResult, DesktopError> {
        let pid = parse_pid(win.app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("window app has no pid".into()))?;
        let index = win.actor_local_key() as usize;
        // 500 default; the session owns the config value but the trait doesn't
        // carry it, so the actor uses a fixed M1 cap (matches DesktopConfig).
        let max_nodes = 500;
        self.send(|resp| AxCommand::Snapshot {
            pid,
            index,
            max_nodes,
            resp,
        })
        .await
    }

    async fn element_action(
        &self,
        r: &ElementRef,
        kind: ElementActionKind,
    ) -> Result<String, DesktopError> {
        let pid = parse_pid(r.window().app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("ref app has no pid".into()))?;
        let index = r.window().actor_local_key() as usize;
        let local_id = r.local_id();
        self.send(|resp| AxCommand::ElementAction {
            pid,
            index,
            local_id,
            kind,
            resp,
        })
        .await
    }

    async fn set_value(&self, r: &ElementRef, text: &str) -> Result<(), DesktopError> {
        let pid = parse_pid(r.window().app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("ref app has no pid".into()))?;
        let index = r.window().actor_local_key() as usize;
        let local_id = r.local_id();
        let text = text.to_string();
        self.send(|resp| AxCommand::SetValue {
            pid,
            index,
            local_id,
            text,
            resp,
        })
        .await
    }

    async fn type_text(&self, win: &WindowId, text: &str) -> Result<(), DesktopError> {
        let pid = parse_pid(win.app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("window app has no pid".into()))?;
        let text = text.to_string();
        self.send(|resp| AxCommand::TypeText { pid, text, resp })
            .await
    }

    async fn key(&self, win: &WindowId, combo: &str) -> Result<(), DesktopError> {
        let pid = parse_pid(win.app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("window app has no pid".into()))?;
        let combo = combo.to_string();
        self.send(|resp| AxCommand::Key { pid, combo, resp }).await
    }

    async fn focus_window(&self, win: &WindowId) -> Result<(), DesktopError> {
        let pid = parse_pid(win.app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("window app has no pid".into()))?;
        let index = win.actor_local_key() as usize;
        self.send(|resp| AxCommand::Focus { pid, index, resp })
            .await
    }

    async fn launch_app(&self, ident: &AppLaunchId) -> Result<AppInfo, DesktopError> {
        let exe = ident.0.clone();
        self.send(|resp| AxCommand::Launch { exe, resp }).await
    }

    async fn screenshot(&self, win: &WindowId) -> Result<Screenshot, DesktopError> {
        let pid = parse_pid(win.app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("window app has no pid".into()))?;
        let index = win.actor_local_key() as usize;
        let bytes = self
            .send(|resp| AxCommand::Screenshot { pid, index, resp })
            .await?;
        Ok(Screenshot {
            bytes,
            media_type: "image/png",
        })
    }

    async fn is_alive(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(AxCommand::Ping(tx)).is_err() {
            return false;
        }
        matches!(
            tokio::time::timeout(Duration::from_secs(2), rx).await,
            Ok(Ok(()))
        )
    }

    async fn close(&self) -> Result<(), DesktopError> {
        // Dropping the sender ends the actor loop; nothing else to release.
        Ok(())
    }
}

/// Per-thread state: the latest snapshot's element map per (pid, window index).
#[derive(Default)]
struct AxState {
    snapshots: HashMap<(i32, usize), Vec<AxElem>>,
}

fn actor_loop(
    mut rx: mpsc::UnboundedReceiver<AxCommand>,
    overlay: Option<crate::desktop::overlay::OverlaySink>,
) {
    let mut state = AxState::default();
    let mut snap_gen: u64 = 0;
    while let Some(cmd) = rx.blocking_recv() {
        handle(cmd, &mut state, &mut snap_gen, overlay.as_ref());
    }
}

fn handle(
    cmd: AxCommand,
    state: &mut AxState,
    snap_gen: &mut u64,
    overlay: Option<&crate::desktop::overlay::OverlaySink>,
) {
    match cmd {
        AxCommand::ListApps(resp) => {
            let apps = tree::list_apps()
                .into_iter()
                .map(|a| AppInfo {
                    name: a.name.clone(),
                    executable_identity: format!("{}#{}", a.name, a.pid),
                    is_electron: false,
                })
                .collect();
            let _ = resp.send(Ok(apps));
        }
        AxCommand::ListWindows { pid, resp } => {
            let windows = tree::list_windows(pid)
                .into_iter()
                .enumerate()
                .map(|(i, title)| WindowInfo {
                    token: i.to_string(),
                    title,
                })
                .collect();
            let _ = resp.send(Ok(windows));
        }
        AxCommand::Snapshot {
            pid,
            index,
            max_nodes,
            resp,
        } => {
            let out = match tree::window_at(pid, index) {
                Some(win) => {
                    let (outline, nodes) = tree::snapshot(&win, max_nodes);
                    *snap_gen += 1;
                    state.snapshots.insert((pid, index), nodes);
                    Ok(SnapshotResult {
                        outline,
                        snapshot_generation: *snap_gen,
                    })
                }
                None => Err(DesktopError::NotFound(format!(
                    "no window at index {index} for pid {pid}"
                ))),
            };
            let _ = resp.send(out);
        }
        AxCommand::ElementAction {
            pid,
            index,
            local_id,
            kind,
            resp,
        } => {
            if let Some(sink) = overlay {
                vis_element(
                    sink,
                    state,
                    pid,
                    index,
                    local_id,
                    matches!(kind, ElementActionKind::Scroll),
                );
            }
            let out = with_node(state, pid, index, local_id, |elem| {
                input::element_action(elem, kind)
            });
            let _ = resp.send(out);
        }
        AxCommand::SetValue {
            pid,
            index,
            local_id,
            text,
            resp,
        } => {
            if let Some(sink) = overlay {
                vis_element(sink, state, pid, index, local_id, false);
            }
            let out = with_node(state, pid, index, local_id, |elem| {
                // Reject secure text fields (spec §输入安全 hard rule).
                if is_secure(elem) {
                    return Err(DesktopError::PermissionDenied(
                        "refusing to set a secure text field".into(),
                    ));
                }
                input::set_value(elem, &text)
            });
            let _ = resp.send(out);
        }
        AxCommand::TypeText { pid, text, resp } => {
            // Never surface the typed text — it may contain secrets. A generic
            // keyboard chip is enough to show that input is happening.
            if let Some(sink) = overlay {
                let _ = sink.send(crate::desktop::overlay::OverlayCmd::Chip {
                    text: format!("⌨ {}", crate::i18n::t("typing…")),
                });
            }
            let _ = resp.send(input::type_text(pid, &text));
        }
        AxCommand::Key { pid, combo, resp } => {
            if let Some(sink) = overlay {
                let _ = sink.send(crate::desktop::overlay::OverlayCmd::Chip {
                    text: format!("⌨ {combo}"),
                });
            }
            let _ = resp.send(input::key_combo(pid, &combo));
        }
        AxCommand::Focus { pid, index, resp } => {
            let out = match tree::window_at(pid, index) {
                Some(win) => input::focus_window(&win),
                None => Err(DesktopError::NotFound("no such window".into())),
            };
            let _ = resp.send(out);
        }
        AxCommand::Launch { exe, resp } => {
            // M1: launch is only supported for an already-running identity we
            // can resolve; real bundle launch is deferred. Report clearly.
            let _ = resp.send(Err(DesktopError::UnsupportedAction(format!(
                "launching {exe:?} is not implemented in M1"
            ))));
        }
        AxCommand::Screenshot { pid, index, resp } => {
            let out = match tree::window_at(pid, index) {
                Some(win) => input::screenshot(pid, &win),
                None => Err(DesktopError::NotFound("no such window".into())),
            };
            let _ = resp.send(out);
        }
        AxCommand::Ping(resp) => {
            let _ = resp.send(());
        }
    }
}

/// Best-effort ghost-cursor targeting: element center + owning CGWindowID.
/// Any lookup failure silently skips visualization — never the action.
fn vis_element(
    sink: &crate::desktop::overlay::OverlaySink,
    state: &AxState,
    pid: i32,
    index: usize,
    local_id: u64,
    scroll: bool,
) {
    use crate::desktop::overlay::{OverlayCmd, Pulse};
    let Some(nodes) = state.snapshots.get(&(pid, index)) else {
        return;
    };
    let Some(elem) = (local_id as usize)
        .checked_sub(1)
        .and_then(|i| nodes.get(i))
    else {
        return;
    };
    let Ok((x, y)) = input::element_center(elem) else {
        return;
    };
    let window_id = input::cg_window_id_for_element(pid, elem);
    let _ = sink.send(OverlayCmd::Move {
        x,
        y,
        window_id,
        pulse: if scroll { Pulse::Scroll } else { Pulse::Click },
    });
}

fn with_node<T>(
    state: &AxState,
    pid: i32,
    index: usize,
    local_id: u64,
    f: impl FnOnce(&AxElem) -> Result<T, DesktopError>,
) -> Result<T, DesktopError> {
    let nodes = state
        .snapshots
        .get(&(pid, index))
        .ok_or_else(|| DesktopError::StaleRef {
            reason: "no snapshot for this window; run snapshot first".into(),
        })?;
    let idx = (local_id as usize).checked_sub(1);
    match idx.and_then(|i| nodes.get(i)) {
        Some(elem) => f(elem),
        None => Err(DesktopError::StaleRef {
            reason: format!("ref e{local_id} not in current snapshot"),
        }),
    }
}

fn is_secure(elem: &AxElem) -> bool {
    unsafe { attr_string(elem.as_ref(), "AXSubrole") }
        .map(|s| s == "AXSecureTextField")
        .unwrap_or(false)
}
