//! Windows UI Automation (UIA) desktop backend. A dedicated STA thread
//! initializes COM (`CoInitializeEx` apartment-threaded) and owns every UIA
//! COM object (they must not cross apartments); the async `UiaBackend` forwards
//! commands over a channel and awaits results (spec §线程与执行模型).
//!
//! NOTE: This backend is written against the `windows` crate UIA API but is
//! built and verified only on Windows (it is `#[cfg(windows)]`). It could not
//! be compiled on the macOS development host — verify on real hardware.

#![cfg(windows)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use windows::core::{BOOL, BSTR};
use windows::Win32::Foundation::{HWND, LPARAM, TRUE};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationValuePattern, TreeScope_Children, UIA_InvokePatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
};

use crate::desktop::backend::{
    AppId, AppInfo, AppLaunchId, DesktopBackend, DesktopBackendFactory, DesktopError,
    ElementActionKind, ElementRef, Screenshot, SnapshotResult, WindowId, WindowInfo,
};

/// Whether UIA can be instantiated in this process.
pub fn uia_available() -> bool {
    // Probing requires an initialized COM apartment; done on the actor thread.
    true
}

#[derive(Debug, Default)]
pub struct UiaFactory;

#[async_trait]
impl DesktopBackendFactory for UiaFactory {
    async fn create(&self) -> Result<Arc<dyn DesktopBackend>, DesktopError> {
        Ok(UiaBackend::spawn())
    }
}

enum UiaCommand {
    ListApps(oneshot::Sender<Result<Vec<AppInfo>, DesktopError>>),
    ListWindows {
        pid: u32,
        resp: oneshot::Sender<Result<Vec<WindowInfo>, DesktopError>>,
    },
    Snapshot {
        pid: u32,
        index: usize,
        max_nodes: usize,
        resp: oneshot::Sender<Result<SnapshotResult, DesktopError>>,
    },
    ElementAction {
        pid: u32,
        index: usize,
        local_id: u64,
        kind: ElementActionKind,
        resp: oneshot::Sender<Result<String, DesktopError>>,
    },
    SetValue {
        pid: u32,
        index: usize,
        local_id: u64,
        text: String,
        resp: oneshot::Sender<Result<(), DesktopError>>,
    },
    TypeText {
        text: String,
        resp: oneshot::Sender<Result<(), DesktopError>>,
    },
    Focus {
        pid: u32,
        index: usize,
        resp: oneshot::Sender<Result<(), DesktopError>>,
    },
    Ping(oneshot::Sender<()>),
}

impl UiaCommand {
    /// Skip queued UIA work when cancellation has already dropped the local
    /// response receiver. An operation already executing on the STA thread is
    /// still conservatively reported as unresolved by the desktop tool layer.
    fn response_closed(&self) -> bool {
        match self {
            Self::ListApps(resp) => resp.is_closed(),
            Self::ListWindows { resp, .. } => resp.is_closed(),
            Self::Snapshot { resp, .. } => resp.is_closed(),
            Self::ElementAction { resp, .. } => resp.is_closed(),
            Self::SetValue { resp, .. } => resp.is_closed(),
            Self::TypeText { resp, .. } => resp.is_closed(),
            Self::Focus { resp, .. } => resp.is_closed(),
            Self::Ping(resp) => resp.is_closed(),
        }
    }
}

#[derive(Debug)]
pub struct UiaBackend {
    tx: mpsc::UnboundedSender<UiaCommand>,
}

impl UiaBackend {
    pub fn spawn() -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("zode-uia-actor".into())
            .spawn(move || actor_loop(rx))
            .expect("spawn uia actor thread");
        Arc::new(Self { tx })
    }

    async fn send<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T, DesktopError>>) -> UiaCommand,
    ) -> Result<T, DesktopError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(make(resp_tx))
            .map_err(|_| DesktopError::Dead("uia actor thread gone".into()))?;
        match tokio::time::timeout(Duration::from_secs(10), resp_rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => Err(DesktopError::Dead("uia actor dropped the response".into())),
            Err(_) => Err(DesktopError::Timeout("uia command timed out".into())),
        }
    }
}

/// Parse the pid encoded in an executable identity string ("Name#<pid>").
fn parse_pid(exe: &str) -> Option<u32> {
    exe.rsplit('#').next()?.parse().ok()
}

#[async_trait]
impl DesktopBackend for UiaBackend {
    async fn list_apps(&self) -> Result<Vec<AppInfo>, DesktopError> {
        self.send(UiaCommand::ListApps).await
    }

    async fn list_windows(&self, app: &AppId) -> Result<Vec<WindowInfo>, DesktopError> {
        let pid = parse_pid(app.executable_identity())
            .ok_or_else(|| DesktopError::NotFound("app identity has no pid".into()))?;
        self.send(|resp| UiaCommand::ListWindows { pid, resp })
            .await
    }

    async fn snapshot(
        &self,
        win: &WindowId,
        _scope: Option<ElementRef>,
    ) -> Result<SnapshotResult, DesktopError> {
        let pid = parse_pid(win.app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("window app has no pid".into()))?;
        let index = win.actor_local_key() as usize;
        self.send(|resp| UiaCommand::Snapshot {
            pid,
            index,
            max_nodes: 500,
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
        self.send(|resp| UiaCommand::ElementAction {
            pid,
            index: r.window().actor_local_key() as usize,
            local_id: r.local_id(),
            kind,
            resp,
        })
        .await
    }

    async fn set_value(&self, r: &ElementRef, text: &str) -> Result<(), DesktopError> {
        let pid = parse_pid(r.window().app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("ref app has no pid".into()))?;
        let text = text.to_string();
        self.send(|resp| UiaCommand::SetValue {
            pid,
            index: r.window().actor_local_key() as usize,
            local_id: r.local_id(),
            text,
            resp,
        })
        .await
    }

    async fn type_text(&self, _win: &WindowId, text: &str) -> Result<(), DesktopError> {
        let text = text.to_string();
        self.send(|resp| UiaCommand::TypeText { text, resp }).await
    }

    async fn key(&self, _win: &WindowId, _combo: &str) -> Result<(), DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "key combos on Windows UIA are not implemented yet".into(),
        ))
    }

    async fn focus_window(&self, win: &WindowId) -> Result<(), DesktopError> {
        let pid = parse_pid(win.app().executable_identity())
            .ok_or_else(|| DesktopError::NotFound("window app has no pid".into()))?;
        self.send(|resp| UiaCommand::Focus {
            pid,
            index: win.actor_local_key() as usize,
            resp,
        })
        .await
    }

    async fn launch_app(&self, _ident: &AppLaunchId) -> Result<AppInfo, DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "launch is not implemented for the UIA backend".into(),
        ))
    }

    async fn screenshot(&self, _win: &WindowId) -> Result<Screenshot, DesktopError> {
        Err(DesktopError::UnsupportedAction(
            "UIA screenshot (PrintWindow capture) is deferred".into(),
        ))
    }

    async fn is_alive(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(UiaCommand::Ping(tx)).is_err() {
            return false;
        }
        matches!(
            tokio::time::timeout(Duration::from_secs(2), rx).await,
            Ok(Ok(()))
        )
    }

    async fn close(&self) -> Result<(), DesktopError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Actor thread: owns COM + UIA objects.
// ---------------------------------------------------------------------------

struct UiaState {
    automation: IUIAutomation,
    /// Latest snapshot's element map per (pid, window index).
    snapshots: HashMap<(u32, usize), Vec<IUIAutomationElement>>,
}

fn actor_loop(mut rx: mpsc::UnboundedReceiver<UiaCommand>) {
    // Initialize COM as an STA and create the automation object once.
    let automation = unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        match CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
            Ok(a) => a,
            Err(_) => {
                // Drain commands with a permission/protocol error.
                while let Some(cmd) = rx.blocking_recv() {
                    if !cmd.response_closed() {
                        fail_command(cmd, "failed to create UIAutomation (COM init)");
                    }
                }
                return;
            }
        }
    };
    let mut state = UiaState {
        automation,
        snapshots: HashMap::new(),
    };
    while let Some(cmd) = rx.blocking_recv() {
        if cmd.response_closed() {
            continue;
        }
        handle(cmd, &mut state);
    }
}

fn fail_command(cmd: UiaCommand, msg: &str) {
    let e = || DesktopError::Protocol(msg.to_string());
    match cmd {
        UiaCommand::ListApps(r) => {
            let _ = r.send(Err(e()));
        }
        UiaCommand::ListWindows { resp, .. } => {
            let _ = resp.send(Err(e()));
        }
        UiaCommand::Snapshot { resp, .. } => {
            let _ = resp.send(Err(e()));
        }
        UiaCommand::ElementAction { resp, .. } => {
            let _ = resp.send(Err(e()));
        }
        UiaCommand::SetValue { resp, .. } => {
            let _ = resp.send(Err(e()));
        }
        UiaCommand::TypeText { resp, .. } => {
            let _ = resp.send(Err(e()));
        }
        UiaCommand::Focus { resp, .. } => {
            let _ = resp.send(Err(e()));
        }
        UiaCommand::Ping(r) => {
            let _ = r.send(());
        }
    }
}

fn handle(cmd: UiaCommand, state: &mut UiaState) {
    match cmd {
        UiaCommand::ListApps(resp) => {
            let _ = resp.send(list_apps());
        }
        UiaCommand::ListWindows { pid, resp } => {
            let _ = resp.send(Ok(list_windows(pid)
                .into_iter()
                .enumerate()
                .map(|(i, (_hwnd, title))| WindowInfo {
                    token: i.to_string(),
                    title: (!title.is_empty()).then_some(title),
                })
                .collect()));
        }
        UiaCommand::Snapshot {
            pid,
            index,
            max_nodes,
            resp,
        } => {
            let _ = resp.send(snapshot(state, pid, index, max_nodes));
        }
        UiaCommand::ElementAction {
            pid,
            index,
            local_id,
            kind,
            resp,
        } => {
            let _ = resp.send(element_action(state, pid, index, local_id, kind));
        }
        UiaCommand::SetValue {
            pid,
            index,
            local_id,
            text,
            resp,
        } => {
            let _ = resp.send(set_value(state, pid, index, local_id, &text));
        }
        UiaCommand::TypeText { text, resp } => {
            let _ = resp.send(type_text(&text));
        }
        UiaCommand::Focus { pid, index, resp } => {
            let out = match nth_window(pid, index) {
                Some(hwnd) => {
                    unsafe {
                        let _ = SetForegroundWindow(hwnd);
                    };
                    Ok(())
                }
                None => Err(DesktopError::NotFound("no such window".into())),
            };
            let _ = resp.send(out);
        }
        UiaCommand::Ping(r) => {
            let _ = r.send(());
        }
    }
}

// ---- Win32 window enumeration -------------------------------------------

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    if unsafe { IsWindowVisible(hwnd) }.as_bool() && !window_title(hwnd).is_empty() {
        out.push(hwnd);
    }
    TRUE
}

fn all_windows() -> Vec<HWND> {
    let mut out: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

fn list_apps() -> Result<Vec<AppInfo>, DesktopError> {
    let mut seen: Vec<AppInfo> = Vec::new();
    for hwnd in all_windows() {
        let pid = window_pid(hwnd);
        if pid == 0
            || seen
                .iter()
                .any(|a| a.executable_identity.ends_with(&format!("#{pid}")))
        {
            continue;
        }
        let title = window_title(hwnd);
        seen.push(AppInfo {
            name: title.clone(),
            executable_identity: format!("{title}#{pid}"),
            is_electron: false,
        });
    }
    Ok(seen)
}

fn list_windows(pid: u32) -> Vec<(HWND, String)> {
    all_windows()
        .into_iter()
        .filter(|h| window_pid(*h) == pid)
        .map(|h| (h, window_title(h)))
        .collect()
}

fn nth_window(pid: u32, index: usize) -> Option<HWND> {
    list_windows(pid).into_iter().nth(index).map(|(h, _)| h)
}

// ---- UIA tree walk + actions --------------------------------------------

fn element_from_window(state: &UiaState, hwnd: HWND) -> Result<IUIAutomationElement, DesktopError> {
    unsafe {
        state
            .automation
            .ElementFromHandle(hwnd)
            .map_err(|e| DesktopError::Protocol(format!("ElementFromHandle: {e}")))
    }
}

fn elem_name(elem: &IUIAutomationElement) -> String {
    unsafe {
        elem.CurrentName()
            .map(|b| b.to_string())
            .unwrap_or_default()
    }
}

fn elem_control_type(elem: &IUIAutomationElement) -> String {
    unsafe {
        elem.CurrentLocalizedControlType()
            .map(|b| b.to_string())
            .unwrap_or_else(|_| "Control".into())
    }
}

fn snapshot(
    state: &mut UiaState,
    pid: u32,
    index: usize,
    max_nodes: usize,
) -> Result<SnapshotResult, DesktopError> {
    let hwnd = nth_window(pid, index).ok_or_else(|| {
        DesktopError::NotFound(format!("no window at index {index} for pid {pid}"))
    })?;
    let root = element_from_window(state, hwnd)?;
    let mut nodes: Vec<IUIAutomationElement> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut truncated = false;
    walk(
        state,
        &root,
        0,
        max_nodes,
        &mut nodes,
        &mut lines,
        &mut truncated,
    );
    if truncated {
        lines.push("  …truncated, use snapshot with scope to expand".into());
    }
    state.snapshots.insert((pid, index), nodes);
    Ok(SnapshotResult {
        outline: lines.join("\n"),
        snapshot_generation: 1,
    })
}

fn walk(
    state: &UiaState,
    elem: &IUIAutomationElement,
    depth: usize,
    max_nodes: usize,
    nodes: &mut Vec<IUIAutomationElement>,
    lines: &mut Vec<String>,
    truncated: &mut bool,
) {
    if nodes.len() >= max_nodes {
        *truncated = true;
        return;
    }
    let role = elem_control_type(elem);
    let name = elem_name(elem);
    nodes.push(elem.clone());
    let n = nodes.len();
    let indent = "  ".repeat(depth);
    let line = if name.is_empty() {
        format!("{indent}[e{n}] {role}")
    } else {
        format!("{indent}[e{n}] {role} {name:?}")
    };
    lines.push(line);

    // Walk children via a TrueCondition + FindAll on the immediate children.
    let children = unsafe {
        state
            .automation
            .CreateTrueCondition()
            .and_then(|cond| elem.FindAll(TreeScope_Children, &cond))
    };
    if let Ok(arr) = children {
        let count = unsafe { arr.Length().unwrap_or(0) };
        for i in 0..count {
            if nodes.len() >= max_nodes {
                *truncated = true;
                break;
            }
            if let Ok(child) = unsafe { arr.GetElement(i) } {
                walk(state, &child, depth + 1, max_nodes, nodes, lines, truncated);
            }
        }
    }
}

fn nth_node(
    state: &UiaState,
    pid: u32,
    index: usize,
    local_id: u64,
) -> Result<IUIAutomationElement, DesktopError> {
    let nodes = state
        .snapshots
        .get(&(pid, index))
        .ok_or_else(|| DesktopError::StaleRef {
            reason: "no snapshot for this window; run snapshot first".into(),
        })?;
    (local_id as usize)
        .checked_sub(1)
        .and_then(|i| nodes.get(i))
        .cloned()
        .ok_or_else(|| DesktopError::StaleRef {
            reason: format!("ref e{local_id} not in current snapshot"),
        })
}

fn element_action(
    state: &UiaState,
    pid: u32,
    index: usize,
    local_id: u64,
    _kind: ElementActionKind,
) -> Result<String, DesktopError> {
    let elem = nth_node(state, pid, index, local_id)?;
    // Try the Invoke pattern (buttons, links, menu items).
    let invoke: Result<IUIAutomationInvokePattern, _> =
        unsafe { elem.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) };
    match invoke {
        Ok(p) => {
            unsafe { p.Invoke() }.map_err(|e| DesktopError::Protocol(format!("Invoke: {e}")))?;
            Ok("ok:invoke".into())
        }
        Err(_) => Err(DesktopError::UnsupportedAction(
            "element exposes no Invoke pattern (pointer synthesis on Windows is deferred)".into(),
        )),
    }
}

fn set_value(
    state: &UiaState,
    pid: u32,
    index: usize,
    local_id: u64,
    text: &str,
) -> Result<(), DesktopError> {
    let elem = nth_node(state, pid, index, local_id)?;
    let vp: IUIAutomationValuePattern =
        unsafe { elem.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
            .map_err(|_| DesktopError::UnsupportedAction("element has no Value pattern".into()))?;
    // Reject read-only / password-like fields.
    if unsafe { vp.CurrentIsReadOnly() }.unwrap_or(TRUE).as_bool() {
        return Err(DesktopError::PermissionDenied(
            "value is read-only or protected".into(),
        ));
    }
    unsafe { vp.SetValue(&BSTR::from(text)) }
        .map_err(|e| DesktopError::Protocol(format!("SetValue: {e}")))
}

/// Type text by synthesizing Unicode key events into the focused control.
fn type_text(text: &str) -> Result<(), DesktopError> {
    let mut inputs: Vec<INPUT> = Vec::new();
    for ch in text.encode_utf16() {
        for &up in &[false, true] {
            let mut flags = KEYEVENTF_UNICODE;
            if up {
                flags |= KEYEVENTF_KEYUP;
            }
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: ch,
                        dwFlags: KEYBD_EVENT_FLAGS(flags.0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(DesktopError::PartialInput {
            characters_sent: (sent as usize) / 2,
            reason: "SendInput delivered fewer events than requested".into(),
        })
    }
}
