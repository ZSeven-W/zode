//! macOS computer-use backend: accessibility tree read/write (`ax.rs`),
//! input injection (`input.rs`), screen capture (`screenshot.rs`), and TCC
//! permission checks (`permissions.rs`), wired into the [`ComputerBackend`]
//! trait. Not exercised by CI — see `tests/computer_it.rs` for the
//! `#[ignore]`d, `ZODE_COMPUTER_IT=1`-gated real-desktop integration test.

mod ax;
mod input;
mod permissions;
mod screenshot;

// Re-exported for the crate-level `permission_status()` query the desktop
// Settings page (`SettingsCategory::ComputerUse`) uses to render live grant
// state — kept separate from `ComputerBackend`'s own inline checks, which
// need the `PermissionPending` error path, not a plain bool.
pub use permissions::{accessibility_trusted, screen_recording_trusted};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex as StdMutex;

use async_trait::async_trait;

use ax::CachedElement;

use super::backend::{ActTarget, AppInfo, AppState, ComputerBackend, ComputerError, Screenshot};

const ACCESSIBILITY_PENDING: &str =
    "Accessibility permission not granted. Open System Settings → Privacy & Security → \
     Accessibility and enable it for zode.";
const SCREEN_RECORDING_PENDING: &str =
    "Screen Recording permission not granted. Open System Settings → Privacy & Security → \
     Screen Recording and enable it for zode.";

/// The most recently read AX tree: which app it belongs to and the flat,
/// ref-ordered element list (index `i` in `elements` is ref `i + 1`).
#[derive(Debug)]
struct TreeCache {
    generation: u64,
    pid: i32,
    elements: Vec<CachedElement>,
}

#[derive(Debug, Default)]
pub struct MacosBackend {
    generation: AtomicU64,
    tree: StdMutex<Option<TreeCache>>,
}

impl MacosBackend {
    pub fn new() -> Self {
        Self::default()
    }

    fn resolve_pid(&self, app: Option<&str>) -> Result<(i32, String), ComputerError> {
        match app {
            Some(name) => ax::pid_for_app_name(name)
                .map(|pid| (pid, name.to_string()))
                .ok_or_else(|| ComputerError::NotFound(format!("application not running: {name}"))),
            None => ax::frontmost_app()
                .map(|(name, pid)| (pid, name))
                .ok_or_else(|| ComputerError::NotFound("no frontmost application".into())),
        }
    }

    /// Validate `generation` against the cached tree and return the cache.
    /// A poisoned mutex (a prior panic mid-hold) is treated as "no cache" —
    /// safe to fall through to StaleGeneration, since we can't trust a
    /// partially-written cache anyway.
    fn cached(
        &self,
        generation: u64,
    ) -> Result<std::sync::MutexGuard<'_, Option<TreeCache>>, ComputerError> {
        let guard = self.tree.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(cache) if cache.generation == generation => Ok(guard),
            Some(cache) => Err(ComputerError::StaleGeneration {
                current: cache.generation,
            }),
            None => Err(ComputerError::StaleGeneration { current: 0 }),
        }
    }

    /// Resolve a click/drag-endpoint target to a screen point, using the
    /// cached tree for `Element` refs.
    fn resolve_point(cache: &TreeCache, target: &ActTarget) -> Result<(f64, f64), ComputerError> {
        match target {
            ActTarget::Coords { x, y } => Ok((*x, *y)),
            ActTarget::Element(r) => cache
                .elements
                .get(*r as usize - 1)
                .map(|e| e.point)
                .ok_or_else(|| ComputerError::NotFound(format!("no element with ref {r}"))),
        }
    }

    /// Resolve an `Element` target to its live `AXUIElement`, re-derived
    /// from the cached path (see `ax::resolve_path`). Coordinate targets
    /// have no AX element to resolve.
    fn resolve_element(
        cache: &TreeCache,
        target: &ActTarget,
    ) -> Result<
        objc2_core_foundation::CFRetained<objc2_application_services::AXUIElement>,
        ComputerError,
    > {
        match target {
            ActTarget::Coords { .. } => Err(ComputerError::NotFound(
                "this action requires an element ref, not coordinates".into(),
            )),
            ActTarget::Element(r) => {
                let cached = cache
                    .elements
                    .get(*r as usize - 1)
                    .ok_or_else(|| ComputerError::NotFound(format!("no element with ref {r}")))?;
                ax::resolve_path(cache.pid, &cached.path)
            }
        }
    }
}

#[async_trait]
impl ComputerBackend for MacosBackend {
    async fn app_state(&self, app: Option<&str>) -> Result<AppState, ComputerError> {
        if !permissions::accessibility_trusted() {
            return Err(ComputerError::PermissionPending(
                ACCESSIBILITY_PENDING.into(),
            ));
        }
        let (pid, name) = self.resolve_pid(app)?;
        let result = ax::walk_app(pid);
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let element_count = result.elements.len();
        *self.tree.lock().unwrap_or_else(|e| e.into_inner()) = Some(TreeCache {
            generation,
            pid,
            elements: result.elements,
        });
        Ok(AppState {
            generation,
            app: name,
            outline: result.outline,
            element_count,
        })
    }

    async fn list_apps(&self) -> Result<Vec<AppInfo>, ComputerError> {
        if !permissions::accessibility_trusted() {
            return Err(ComputerError::PermissionPending(
                ACCESSIBILITY_PENDING.into(),
            ));
        }
        Ok(ax::list_running_apps()
            .into_iter()
            .map(|(name, pid, frontmost)| AppInfo {
                name,
                pid,
                frontmost,
            })
            .collect())
    }

    async fn screenshot(&self) -> Result<Screenshot, ComputerError> {
        if !permissions::screen_recording_trusted() {
            return Err(ComputerError::PermissionPending(
                SCREEN_RECORDING_PENDING.into(),
            ));
        }
        let bytes = screenshot::capture_main_display().await?;
        Ok(Screenshot {
            bytes,
            media_type: "image/jpeg",
        })
    }

    async fn click(&self, generation: u64, target: &ActTarget) -> Result<(), ComputerError> {
        if !permissions::accessibility_trusted() {
            return Err(ComputerError::PermissionPending(
                ACCESSIBILITY_PENDING.into(),
            ));
        }
        let cache_guard = self.cached(generation)?;
        let cache = cache_guard.as_ref().expect("checked Some above");
        // Prefer AXPress on a resolvable element — it doesn't depend on
        // window focus/z-order the way a synthetic coordinate click does.
        if let Ok(element) = Self::resolve_element(cache, target) {
            if ax::perform_action(&element, "AXPress").is_ok() {
                return Ok(());
            }
        }
        let point = Self::resolve_point(cache, target)?;
        drop(cache_guard);
        input::click_at(point.0, point.1)
    }

    async fn type_text(&self, generation: u64, text: &str) -> Result<(), ComputerError> {
        if !permissions::accessibility_trusted() {
            return Err(ComputerError::PermissionPending(
                ACCESSIBILITY_PENDING.into(),
            ));
        }
        drop(self.cached(generation)?);
        input::type_text(text)
    }

    async fn set_value(
        &self,
        generation: u64,
        target: &ActTarget,
        value: &str,
    ) -> Result<(), ComputerError> {
        if !permissions::accessibility_trusted() {
            return Err(ComputerError::PermissionPending(
                ACCESSIBILITY_PENDING.into(),
            ));
        }
        let cache_guard = self.cached(generation)?;
        let cache = cache_guard.as_ref().expect("checked Some above");
        let element = Self::resolve_element(cache, target)?;
        drop(cache_guard);
        ax::set_value_string(&element, value)
    }

    async fn key(&self, generation: u64, key: &str) -> Result<(), ComputerError> {
        if !permissions::accessibility_trusted() {
            return Err(ComputerError::PermissionPending(
                ACCESSIBILITY_PENDING.into(),
            ));
        }
        drop(self.cached(generation)?);
        input::key_press(key)
    }

    async fn scroll(&self, generation: u64, dx: f64, dy: f64) -> Result<(), ComputerError> {
        if !permissions::accessibility_trusted() {
            return Err(ComputerError::PermissionPending(
                ACCESSIBILITY_PENDING.into(),
            ));
        }
        drop(self.cached(generation)?);
        input::scroll(dx, dy)
    }

    async fn drag(
        &self,
        generation: u64,
        from: &ActTarget,
        to: &ActTarget,
    ) -> Result<(), ComputerError> {
        if !permissions::accessibility_trusted() {
            return Err(ComputerError::PermissionPending(
                ACCESSIBILITY_PENDING.into(),
            ));
        }
        let cache_guard = self.cached(generation)?;
        let cache = cache_guard.as_ref().expect("checked Some above");
        let from_point = Self::resolve_point(cache, from)?;
        let to_point = Self::resolve_point(cache, to)?;
        drop(cache_guard);
        input::drag(from_point, to_point)
    }

    async fn describe_target(&self, generation: u64, target: &ActTarget) -> Option<String> {
        let cache_guard = self.cached(generation).ok()?;
        let cache = cache_guard.as_ref()?;
        match target {
            ActTarget::Coords { x, y } => Some(format!("coords=({x},{y})")),
            ActTarget::Element(r) => cache
                .elements
                .get(*r as usize - 1)
                .map(|e| format!("role={} label=\"{}\"", e.role, e.label)),
        }
    }

    async fn frontmost_app_name(&self) -> Option<String> {
        ax::frontmost_app().map(|(name, _)| name)
    }
}
