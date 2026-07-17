//! Platform-agnostic computer-use backend trait + shared types. Mirrors
//! `browser::backend`: one trait, one error taxonomy, one mock for tests.
//!
//! Generation binding (doc §1): every mutating call carries the `generation`
//! returned by the most recent `app_state` read. A backend whose live state
//! has moved past that generation (a fresh `app_state` happened, or internal
//! bookkeeping otherwise invalidated the cached tree) must reject the call
//! with [`ComputerError::StaleGeneration`] instead of acting on stale
//! coordinates/elements — this is the "read, then act on what you just read"
//! discipline the doc calls out as the reason both mature prior-art
//! implementations converged on AX-tree-first grounding.

use async_trait::async_trait;
use std::fmt;

/// Which running application to read/act on.
#[derive(Debug, Clone, PartialEq)]
pub struct AppInfo {
    pub name: String,
    pub pid: i32,
    pub frontmost: bool,
}

/// Result of a `computer_read` `app_state` call: a ref-annotated outline of
/// the target app's accessibility tree plus the generation token that
/// subsequent `computer_act` calls must echo back.
#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub generation: u64,
    pub app: String,
    /// Ref-annotated outline text, e.g. `[1] <AXButton> "OK"`, one element
    /// per line, indented by tree depth. Bounded depth/node count — see
    /// the backend implementation for the actual caps.
    pub outline: String,
    pub element_count: usize,
}

/// A captured screenshot, ready to hand to `agent::attachments::image_from_bytes`.
#[derive(Debug, Clone, PartialEq)]
pub struct Screenshot {
    pub bytes: Vec<u8>,
    pub media_type: &'static str,
}

/// Target of a click / set_value / drag endpoint: either a stable element
/// ref from the most recent `app_state` outline, or raw screen coordinates
/// (the doc's documented fallback for AX-poor UIs, e.g. Electron canvases).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActTarget {
    Element(u32),
    Coords { x: f64, y: f64 },
}

/// Computer-use operation errors.
#[derive(Debug, Clone, PartialEq)]
pub enum ComputerError {
    /// Accessibility or Screen Recording permission is not yet granted.
    /// NOT a hard failure — per the doc, tools surface this as a retryable
    /// result telling the model to call again, not as a terminal error.
    PermissionPending(String),
    /// `computer_act` was called with a generation older than the backend's
    /// current one: the read that produced the ref/coords is stale.
    StaleGeneration { current: u64 },
    /// Requested app or element could not be found.
    NotFound(String),
    /// The underlying OS call failed.
    Protocol(String),
    /// This platform has no computer-use backend (M1 is macOS-only).
    Unsupported(String),
}

impl fmt::Display for ComputerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComputerError::PermissionPending(msg) => write!(f, "permission pending: {msg}"),
            ComputerError::StaleGeneration { current } => write!(
                f,
                "stale generation: current state is generation {current}; call computer_read \
                 app_state again before acting"
            ),
            ComputerError::NotFound(msg) => write!(f, "not found: {msg}"),
            ComputerError::Protocol(msg) => write!(f, "computer-use protocol error: {msg}"),
            ComputerError::Unsupported(msg) => write!(f, "computer-use unsupported: {msg}"),
        }
    }
}

impl std::error::Error for ComputerError {}

/// Backend for computer-use (native OS UI automation) operations.
///
/// Every mutating method takes the caller's `generation` so the backend can
/// reject stale act calls uniformly (see module docs). Implementers must be
/// object-safe (`Send + Sync + Debug`).
#[async_trait]
pub trait ComputerBackend: Send + Sync + fmt::Debug {
    /// Read the accessibility tree for `app` (frontmost app if `None`).
    /// Bumps and returns a new generation.
    async fn app_state(&self, app: Option<&str>) -> Result<AppState, ComputerError>;

    /// List running applications.
    async fn list_apps(&self) -> Result<Vec<AppInfo>, ComputerError>;

    /// Screenshot the main display.
    async fn screenshot(&self) -> Result<Screenshot, ComputerError>;

    /// Click `target`. `generation` must match the backend's current one.
    async fn click(&self, generation: u64, target: &ActTarget) -> Result<(), ComputerError>;

    /// Type `text` into the currently focused control (keystroke simulation).
    async fn type_text(&self, generation: u64, text: &str) -> Result<(), ComputerError>;

    /// Write `value` directly into `target` via the AX value attribute,
    /// bypassing keystroke simulation (more reliable for form fields that
    /// intercept/reformat keyboard input).
    async fn set_value(
        &self,
        generation: u64,
        target: &ActTarget,
        value: &str,
    ) -> Result<(), ComputerError>;

    /// Press a named key (e.g. "Enter", "Escape", "Tab", "ArrowDown").
    async fn key(&self, generation: u64, key: &str) -> Result<(), ComputerError>;

    /// Scroll by `dx`, `dy` pixels at the current pointer location.
    async fn scroll(&self, generation: u64, dx: f64, dy: f64) -> Result<(), ComputerError>;

    /// Drag from one target to another (mouse down at `from`, up at `to`).
    async fn drag(
        &self,
        generation: u64,
        from: &ActTarget,
        to: &ActTarget,
    ) -> Result<(), ComputerError>;

    /// Best-effort human-readable description ("role=AXButton label=\"OK\"")
    /// of `target` at `generation`, for approval-prompt enrichment. `None`
    /// when the generation is stale or the target can't be resolved — this
    /// is advisory only, never a hard error.
    async fn describe_target(&self, generation: u64, target: &ActTarget) -> Option<String>;

    /// Best-effort name of the frontmost application, for approval-prompt
    /// enrichment. `None` when it can't be determined without blocking.
    async fn frontmost_app_name(&self) -> Option<String>;
}

#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    /// Records calls; every op succeeds unless `permission_pending` is set.
    /// `generation` starts at 0 (no state read yet — a stale-generation
    /// check against 0 fails until the first `app_state` call).
    #[derive(Debug, Default)]
    pub struct MockBackend {
        pub generation: AtomicU64,
        pub calls: AtomicUsize,
        pub permission_pending: AtomicBool,
        pub last_click: std::sync::Mutex<Option<ActTarget>>,
        pub last_typed: std::sync::Mutex<Option<String>>,
        pub last_set_value: std::sync::Mutex<Option<(ActTarget, String)>>,
        pub last_key: std::sync::Mutex<Option<String>>,
        pub last_scroll: std::sync::Mutex<Option<(f64, f64)>>,
        pub last_drag: std::sync::Mutex<Option<(ActTarget, ActTarget)>>,
    }

    impl MockBackend {
        fn check_pending(&self) -> Result<(), ComputerError> {
            if self.permission_pending.load(Ordering::SeqCst) {
                return Err(ComputerError::PermissionPending(
                    "Accessibility permission not granted for zode".into(),
                ));
            }
            Ok(())
        }

        fn check_generation(&self, generation: u64) -> Result<(), ComputerError> {
            let current = self.generation.load(Ordering::SeqCst);
            if generation != current {
                return Err(ComputerError::StaleGeneration { current });
            }
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl ComputerBackend for MockBackend {
        async fn app_state(&self, app: Option<&str>) -> Result<AppState, ComputerError> {
            self.check_pending()?;
            self.calls.fetch_add(1, Ordering::SeqCst);
            let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(AppState {
                generation,
                app: app.unwrap_or("TestApp").to_string(),
                outline: "[1] <AXButton> \"OK\"".into(),
                element_count: 1,
            })
        }

        async fn list_apps(&self) -> Result<Vec<AppInfo>, ComputerError> {
            self.check_pending()?;
            Ok(vec![AppInfo {
                name: "TestApp".into(),
                pid: 1234,
                frontmost: true,
            }])
        }

        async fn screenshot(&self) -> Result<Screenshot, ComputerError> {
            self.check_pending()?;
            Ok(Screenshot {
                bytes: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1],
                media_type: "image/png",
            })
        }

        async fn click(&self, generation: u64, target: &ActTarget) -> Result<(), ComputerError> {
            self.check_pending()?;
            self.check_generation(generation)?;
            *self.last_click.lock().unwrap() = Some(*target);
            Ok(())
        }

        async fn type_text(&self, generation: u64, text: &str) -> Result<(), ComputerError> {
            self.check_pending()?;
            self.check_generation(generation)?;
            *self.last_typed.lock().unwrap() = Some(text.to_string());
            Ok(())
        }

        async fn set_value(
            &self,
            generation: u64,
            target: &ActTarget,
            value: &str,
        ) -> Result<(), ComputerError> {
            self.check_pending()?;
            self.check_generation(generation)?;
            *self.last_set_value.lock().unwrap() = Some((*target, value.to_string()));
            Ok(())
        }

        async fn key(&self, generation: u64, key: &str) -> Result<(), ComputerError> {
            self.check_pending()?;
            self.check_generation(generation)?;
            *self.last_key.lock().unwrap() = Some(key.to_string());
            Ok(())
        }

        async fn scroll(&self, generation: u64, dx: f64, dy: f64) -> Result<(), ComputerError> {
            self.check_pending()?;
            self.check_generation(generation)?;
            *self.last_scroll.lock().unwrap() = Some((dx, dy));
            Ok(())
        }

        async fn drag(
            &self,
            generation: u64,
            from: &ActTarget,
            to: &ActTarget,
        ) -> Result<(), ComputerError> {
            self.check_pending()?;
            self.check_generation(generation)?;
            *self.last_drag.lock().unwrap() = Some((*from, *to));
            Ok(())
        }

        async fn describe_target(&self, generation: u64, target: &ActTarget) -> Option<String> {
            if self.check_generation(generation).is_err() {
                return None;
            }
            match target {
                ActTarget::Element(1) => Some("role=AXButton label=\"OK\"".into()),
                ActTarget::Element(n) => Some(format!("role=AXUnknown label=\"elem-{n}\"")),
                ActTarget::Coords { x, y } => Some(format!("coords=({x},{y})")),
            }
        }

        async fn frontmost_app_name(&self) -> Option<String> {
            Some("TestApp".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_prefixed() {
        assert_eq!(
            ComputerError::NotFound("Finder".into()).to_string(),
            "not found: Finder"
        );
        assert!(ComputerError::StaleGeneration { current: 3 }
            .to_string()
            .contains("generation 3"));
    }

    #[tokio::test]
    async fn mock_backend_is_object_safe() {
        let b: std::sync::Arc<dyn ComputerBackend> =
            std::sync::Arc::new(mock::MockBackend::default());
        let state = b.app_state(None).await.unwrap();
        assert_eq!(state.generation, 1);
    }
}
