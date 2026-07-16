//! DesktopBackend trait plus the identity and error model. Identity tokens
//! are session-local and opaque: they carry pid/generation but NEVER a native
//! handle — native objects live only on the actor thread (see spec §线程模型).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Session-local opaque app identity. Never a bare PID: pid is paired with the
/// process start time, an executable identity string, and a generation that
/// bumps when the app is relaunched or self-updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppId {
    pid: i32,
    process_start_time: u64,
    executable_identity: String,
    generation: u64,
}

impl AppId {
    pub fn new(
        pid: i32,
        process_start_time: u64,
        executable_identity: String,
        generation: u64,
    ) -> Self {
        Self {
            pid,
            process_start_time,
            executable_identity,
            generation,
        }
    }
    pub fn pid(&self) -> i32 {
        self.pid
    }
    pub fn process_start_time(&self) -> u64 {
        self.process_start_time
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn executable_identity(&self) -> &str {
        &self.executable_identity
    }
}

/// Session-local opaque window identity. Stores only the actor generation and
/// an actor-local numeric key; the native AXUIElement/HWND lives in the actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowId {
    app: AppId,
    actor_local_key: u64,
    window_generation: u64,
    actor_generation: u64,
}

impl WindowId {
    pub fn new(
        app: AppId,
        actor_local_key: u64,
        window_generation: u64,
        actor_generation: u64,
    ) -> Self {
        Self {
            app,
            actor_local_key,
            window_generation,
            actor_generation,
        }
    }
    pub fn app(&self) -> &AppId {
        &self.app
    }
    pub fn actor_local_key(&self) -> u64 {
        self.actor_local_key
    }
    pub fn window_generation(&self) -> u64 {
        self.window_generation
    }
    pub fn actor_generation(&self) -> u64 {
        self.actor_generation
    }
}

/// Model-visible form is `e<N>` in the outline; the tool input must also carry
/// `window`, and the session resolves both into this generation-bound ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementRef {
    window: WindowId,
    snapshot_generation: u64,
    local_id: u64,
}

impl ElementRef {
    pub fn new(window: WindowId, snapshot_generation: u64, local_id: u64) -> Self {
        Self {
            window,
            snapshot_generation,
            local_id,
        }
    }
    pub fn window(&self) -> &WindowId {
        &self.window
    }
    pub fn snapshot_generation(&self) -> u64 {
        self.snapshot_generation
    }
    pub fn local_id(&self) -> u64 {
        self.local_id
    }
}

/// Identity of an installed app to launch (bundle id / registered app). Never
/// an arbitrary command line — that belongs to the shell tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLaunchId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub executable_identity: String,
    pub is_electron: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    /// Opaque token string the model uses to reference a window.
    pub token: String,
    /// Present only when the owning app is allowlisted (spec: title is content).
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotResult {
    /// ref-annotated text outline, same format as browser snapshot_js.
    pub outline: String,
    pub snapshot_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementActionKind {
    Click,
    Toggle,
    Expand,
    Scroll,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Screenshot {
    pub bytes: Vec<u8>,
    pub media_type: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopError {
    NotFound(String),
    PermissionDenied(String),
    UnsupportedAction(String),
    Protocol(String),
    Timeout(String),
    Dead(String),
    StaleRef { reason: String },
    StaleTarget { reason: String },
    PartialInput { characters_sent: usize, reason: String },
    PartialKeyInput {
        combos_sent: usize,
        cleanup_ok: bool,
        reason: String,
    },
    Ambiguous { candidates: Vec<String> },
}

impl fmt::Display for DesktopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DesktopError::NotFound(m) => write!(f, "desktop target not found: {m}"),
            DesktopError::PermissionDenied(m) => write!(f, "desktop permission denied: {m}"),
            DesktopError::UnsupportedAction(m) => write!(f, "unsupported desktop action: {m}"),
            DesktopError::Protocol(m) => write!(f, "desktop protocol error: {m}"),
            DesktopError::Timeout(m) => write!(f, "desktop operation timed out: {m}"),
            DesktopError::Dead(m) => write!(f, "desktop backend not running: {m}"),
            DesktopError::StaleRef { reason } => {
                write!(f, "stale desktop ref ({reason}); re-run snapshot")
            }
            DesktopError::StaleTarget { reason } => {
                write!(f, "stale desktop target ({reason}); re-approve")
            }
            DesktopError::PartialInput {
                characters_sent,
                reason,
            } => write!(
                f,
                "partial text input ({characters_sent} chars sent): {reason}"
            ),
            DesktopError::PartialKeyInput {
                combos_sent,
                cleanup_ok,
                reason,
            } => write!(
                f,
                "partial key input ({combos_sent} combos sent, cleanup_ok={cleanup_ok}): {reason}"
            ),
            DesktopError::Ambiguous { candidates } => {
                write!(f, "ambiguous desktop target; candidates: {}", candidates.join(", "))
            }
        }
    }
}

impl std::error::Error for DesktopError {}

/// Factory for platform (or mock) backends. Lives here — not in the test-only
/// `mock` module — so production code (engine) can construct a session with a
/// real platform factory.
#[async_trait]
pub trait DesktopBackendFactory: Send + Sync + std::fmt::Debug {
    async fn create(&self) -> Result<std::sync::Arc<dyn DesktopBackend>, DesktopError>;
}

/// Fallback factory for platforms/builds without a real backend yet. Creation
/// fails with a clear message rather than panicking, so enabling the subsystem
/// on an unsupported platform degrades gracefully.
#[derive(Debug, Default)]
pub struct UnsupportedDesktopFactory;

#[async_trait]
impl DesktopBackendFactory for UnsupportedDesktopFactory {
    async fn create(&self) -> Result<std::sync::Arc<dyn DesktopBackend>, DesktopError> {
        Err(DesktopError::Dead(
            "desktop control is not available on this platform in this build".into(),
        ))
    }
}

/// Actor-client backend: `Send + Sync + Debug` so `Arc<dyn DesktopBackend>`
/// can be shared; the implementation forwards commands to a platform thread.
#[async_trait]
pub trait DesktopBackend: Send + Sync + std::fmt::Debug {
    async fn list_apps(&self) -> Result<Vec<AppInfo>, DesktopError>;
    async fn list_windows(&self, app: &AppId) -> Result<Vec<WindowInfo>, DesktopError>;
    async fn snapshot(
        &self,
        win: &WindowId,
        scope: Option<ElementRef>,
    ) -> Result<SnapshotResult, DesktopError>;
    async fn element_action(
        &self,
        r: &ElementRef,
        kind: ElementActionKind,
    ) -> Result<String, DesktopError>;
    async fn set_value(&self, r: &ElementRef, text: &str) -> Result<(), DesktopError>;
    async fn type_text(&self, win: &WindowId, text: &str) -> Result<(), DesktopError>;
    async fn key(&self, win: &WindowId, combo: &str) -> Result<(), DesktopError>;
    async fn focus_window(&self, win: &WindowId) -> Result<(), DesktopError>;
    async fn launch_app(&self, ident: &AppLaunchId) -> Result<AppInfo, DesktopError>;
    async fn screenshot(&self, win: &WindowId) -> Result<Screenshot, DesktopError>;
    async fn is_alive(&self) -> bool;
    async fn close(&self) -> Result<(), DesktopError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_carries_context() {
        let e = DesktopError::StaleRef {
            reason: "window closed".into(),
        };
        assert!(e.to_string().contains("window closed"));
        let p = DesktopError::PartialKeyInput {
            combos_sent: 2,
            cleanup_ok: true,
            reason: "focus lost".into(),
        };
        assert!(p.to_string().contains("focus lost"));
        assert!(p.to_string().contains('2'));
    }

    #[test]
    fn tokens_are_opaque_and_carry_generation() {
        let a = AppId::new(1, 100, "com.apple.TextEdit".into(), 7);
        assert_eq!(a.generation(), 7);
        let w = WindowId::new(a.clone(), 3, 42, 5);
        assert_eq!(w.app().generation(), 7);
        assert_eq!(w.actor_local_key(), 3);
        assert_eq!(w.window_generation(), 42);
        assert_eq!(w.actor_generation(), 5);
        let r = ElementRef::new(w.clone(), 9, 12);
        assert_eq!(r.window().window_generation(), 42);
        assert_eq!(r.local_id(), 12);
        assert_eq!(r.snapshot_generation(), 9);
    }
}
