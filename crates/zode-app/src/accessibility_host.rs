use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[cfg(target_os = "linux")]
use accesskit::DeactivationHandler;
use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use zode_app_ui::{accessibility_tree, WorkspaceSnapshot};

type SharedTree = Arc<Mutex<Option<TreeUpdate>>>;
type ActionQueue = Arc<Mutex<VecDeque<ActionRequest>>>;
pub type AccessibilityWake = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostInstallWindowAction {
    Maximize,
    Show,
}

pub fn post_install_window_actions(maximized: bool) -> &'static [PostInstallWindowAction] {
    const NORMAL: &[PostInstallWindowAction] = &[PostInstallWindowAction::Show];
    const MAXIMIZED: &[PostInstallWindowAction] = &[
        PostInstallWindowAction::Maximize,
        PostInstallWindowAction::Show,
    ];
    if maximized {
        MAXIMIZED
    } else {
        NORMAL
    }
}

pub trait AccessibilityBridge {
    fn push(&mut self, update: TreeUpdate);
    fn drain_actions(&mut self) -> Vec<ActionRequest>;
    fn set_window_focused(&mut self, focused: bool);
    fn update_window_bounds(&mut self, window: &winit::window::Window);
}

#[derive(Debug, thiserror::Error)]
pub enum AccessibilityHostError {
    #[error("the platform accessibility adapter could not attach to this window")]
    AdapterUnavailable,
}

struct CachedTreeActivation {
    tree: SharedTree,
}

impl ActivationHandler for CachedTreeActivation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.tree.lock().ok().and_then(|tree| tree.clone())
    }
}

struct QueueingActionHandler {
    queue: ActionQueue,
    wake: AccessibilityWake,
}

impl ActionHandler for QueueingActionHandler {
    fn do_action(&mut self, request: ActionRequest) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(request);
        }
        (self.wake)();
    }
}

#[cfg(target_os = "linux")]
struct NoopDeactivation;

#[cfg(target_os = "linux")]
impl DeactivationHandler for NoopDeactivation {
    fn deactivate_accessibility(&mut self) {}
}

pub struct AccessibilityHost {
    #[cfg(target_os = "macos")]
    adapter: Option<accesskit_macos::SubclassingAdapter>,
    #[cfg(target_os = "windows")]
    adapter: Option<accesskit_windows::SubclassingAdapter>,
    #[cfg(target_os = "linux")]
    adapter: Option<accesskit_unix::Adapter>,
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    adapter: Option<()>,
    tree: SharedTree,
    queue: ActionQueue,
}

impl AccessibilityHost {
    pub fn install(
        window: &winit::window::Window,
        initial_tree: TreeUpdate,
        wake: AccessibilityWake,
    ) -> Result<Self, AccessibilityHostError> {
        let tree = Arc::new(Mutex::new(Some(initial_tree.clone())));
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let adapter = Self::build_adapter(window, tree.clone(), queue.clone(), wake)
            .ok_or(AccessibilityHostError::AdapterUnavailable)?;
        let mut host = Self {
            adapter: Some(adapter),
            tree,
            queue,
        };
        host.raise(initial_tree);
        Ok(host)
    }

    /// Install and seed the real adapter while the native window is hidden, then reveal it.
    pub fn install_before_show(
        window: &winit::window::Window,
        snapshot: &WorkspaceSnapshot,
        physical_scale: f64,
        wake: AccessibilityWake,
        maximized: bool,
    ) -> Result<Self, AccessibilityHostError> {
        let mut host = Self::install(window, accessibility_tree(snapshot, physical_scale), wake)?;
        host.update_window_bounds(window);
        for action in post_install_window_actions(maximized) {
            match action {
                PostInstallWindowAction::Maximize => window.set_maximized(true),
                PostInstallWindowAction::Show => window.set_visible(true),
            }
        }
        Ok(host)
    }

    #[cfg(target_os = "macos")]
    fn build_adapter(
        window: &winit::window::Window,
        tree: SharedTree,
        queue: ActionQueue,
        wake: AccessibilityWake,
    ) -> Option<accesskit_macos::SubclassingAdapter> {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        let activation = CachedTreeActivation { tree };
        let action = QueueingActionHandler { queue, wake };
        // SAFETY: the NSView comes from `window` and the host is dropped before the window.
        Some(unsafe {
            accesskit_macos::SubclassingAdapter::new(handle.ns_view.as_ptr(), activation, action)
        })
    }

    #[cfg(target_os = "macos")]
    fn raise(&mut self, update: TreeUpdate) {
        if let Some(adapter) = self.adapter.as_mut() {
            if let Some(events) = adapter.update_if_active(|| update) {
                events.raise();
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn build_adapter(
        window: &winit::window::Window,
        tree: SharedTree,
        queue: ActionQueue,
        wake: AccessibilityWake,
    ) -> Option<accesskit_windows::SubclassingAdapter> {
        use accesskit_windows::HWND;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let RawWindowHandle::Win32(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        Some(accesskit_windows::SubclassingAdapter::new(
            HWND(handle.hwnd.get() as *mut core::ffi::c_void),
            CachedTreeActivation { tree },
            QueueingActionHandler { queue, wake },
        ))
    }

    #[cfg(target_os = "windows")]
    fn raise(&mut self, update: TreeUpdate) {
        if let Some(adapter) = self.adapter.as_mut() {
            if let Some(events) = adapter.update_if_active(|| update) {
                events.raise();
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn build_adapter(
        _window: &winit::window::Window,
        tree: SharedTree,
        queue: ActionQueue,
        wake: AccessibilityWake,
    ) -> Option<accesskit_unix::Adapter> {
        Some(accesskit_unix::Adapter::new(
            CachedTreeActivation { tree },
            QueueingActionHandler { queue, wake },
            NoopDeactivation,
        ))
    }

    #[cfg(target_os = "linux")]
    fn raise(&mut self, update: TreeUpdate) {
        if let Some(adapter) = self.adapter.as_mut() {
            adapter.update_if_active(|| update);
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn build_adapter(
        _window: &winit::window::Window,
        _tree: SharedTree,
        _queue: ActionQueue,
        _wake: AccessibilityWake,
    ) -> Option<()> {
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn raise(&mut self, _update: TreeUpdate) {}
}

impl AccessibilityBridge for AccessibilityHost {
    fn push(&mut self, update: TreeUpdate) {
        if let Ok(mut tree) = self.tree.lock() {
            *tree = Some(update.clone());
        }
        self.raise(update);
    }

    fn drain_actions(&mut self) -> Vec<ActionRequest> {
        self.queue
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default()
    }

    fn set_window_focused(&mut self, focused: bool) {
        #[cfg(target_os = "macos")]
        if let Some(adapter) = self.adapter.as_mut() {
            if let Some(events) = adapter.update_view_focus_state(focused) {
                events.raise();
            }
        }
        #[cfg(target_os = "linux")]
        if let Some(adapter) = self.adapter.as_mut() {
            adapter.update_window_focus_state(focused);
        }
        #[cfg(target_os = "windows")]
        {
            // The subclass adapter receives WM_SETFOCUS/WM_KILLFOCUS directly.
            let _ = focused;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            let _ = focused;
        }
    }

    fn update_window_bounds(&mut self, window: &winit::window::Window) {
        #[cfg(target_os = "linux")]
        if let Some(adapter) = self.adapter.as_mut() {
            let outer_position: (_, _) = window
                .outer_position()
                .unwrap_or_default()
                .cast::<f64>()
                .into();
            let outer_size: (_, _) = window.outer_size().cast::<f64>().into();
            let inner_position: (_, _) = window
                .inner_position()
                .unwrap_or_default()
                .cast::<f64>()
                .into();
            let inner_size: (_, _) = window.inner_size().cast::<f64>().into();
            adapter.set_root_window_bounds(
                accesskit::Rect::from_origin_size(outer_position, outer_size),
                accesskit::Rect::from_origin_size(inner_position, inner_size),
            );
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = window;
        }
    }
}
