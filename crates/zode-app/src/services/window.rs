use std::sync::{Arc, Weak};

use winit::{
    event_loop::EventLoopProxy,
    window::{Fullscreen, Theme, Window},
};
use zode_app_model::SystemTheme;

use crate::window_state::AppWake;

use super::{ServiceError, WindowService};

pub struct NativeWindowService {
    window: Weak<Window>,
    proxy: EventLoopProxy<AppWake>,
}

impl NativeWindowService {
    pub fn new(window: &Arc<Window>, proxy: EventLoopProxy<AppWake>) -> Self {
        Self {
            window: Arc::downgrade(window),
            proxy,
        }
    }

    fn window(&self) -> Option<Arc<Window>> {
        self.window.upgrade()
    }
}

impl WindowService for NativeWindowService {
    fn begin_drag(&self) -> Result<(), ServiceError> {
        self.window()
            .ok_or_else(|| ServiceError::Platform("window is closed".into()))?
            .drag_window()
            .map_err(|error| ServiceError::Platform(error.to_string()))
    }

    fn minimize(&self) {
        if let Some(window) = self.window() {
            window.set_minimized(true);
        }
    }

    fn toggle_maximize(&self) {
        if let Some(window) = self.window() {
            window.set_maximized(!window.is_maximized());
        }
    }

    fn close(&self) {
        let _ = self.proxy.send_event(AppWake::Close);
    }

    fn toggle_fullscreen(&self) {
        if let Some(window) = self.window() {
            let next = if window.fullscreen().is_some() {
                None
            } else {
                Some(Fullscreen::Borderless(None))
            };
            window.set_fullscreen(next);
        }
    }

    fn system_theme(&self) -> SystemTheme {
        match self.window().and_then(|window| window.theme()) {
            Some(Theme::Dark) => SystemTheme::Dark,
            _ => SystemTheme::Light,
        }
    }
}
