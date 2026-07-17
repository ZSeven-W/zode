use std::{num::NonZeroU32, sync::Arc};

use winit::event_loop::ActiveEventLoop;
use zode_app_ui::{accessibility_tree, Composer, WorkspaceShell, WorkspaceSnapshot, COMPOSER_ID};

#[cfg(not(target_os = "macos"))]
use crate::event_map::resize_direction;
use crate::{
    accessibility_host::{AccessibilityBridge, AccessibilityHost},
    event_map::{is_drag_region, map_system_theme},
    render::{FramePainter, NativeBackend, RasterSurface},
    window_bootstrap::hidden_window_attributes_for_placement,
    window_state::{update_window_geometry, AppWake, WindowGeometry, WindowState},
    work_area::{resolve_startup_placement, PlatformWorkAreaProvider},
};

use super::DesktopApp;

impl DesktopApp {
    pub(super) fn open_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.window.is_some() {
            return Ok(());
        }
        let provider = PlatformWorkAreaProvider::from_event_loop(event_loop);
        let (placement, work_area_warning) =
            resolve_startup_placement(self.window_geometry, &provider);
        if let Some(error) = work_area_warning {
            eprintln!(
                "zode-app: saved window position was not restored because work-area discovery is unavailable: {error}"
            );
        }
        let attributes = hidden_window_attributes_for_placement(placement);

        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| error.to_string())?,
        );
        window.set_ime_allowed(false);
        self.app_state.host.system_theme = map_system_theme(window.theme());
        let size = window.inner_size();
        self.window_state = WindowState::new(size.width, size.height, window.scale_factor());
        self.renderer = NativeBackend::new(self.window_state.scale_factor as f32);
        self.rebuild_frame_snapshot();
        self.resize_terminal_grid();

        let wake_proxy = self.proxy.clone();
        let wake = Arc::new(move || {
            let _ = wake_proxy.send_event(AppWake::Redraw);
        });
        let a11y = AccessibilityHost::install_before_show(
            &window,
            &self.frame_snapshot,
            self.window_state.scale_factor,
            wake,
            placement.maximized(),
        )
        .map_err(|error| error.to_string())?;
        let context =
            softbuffer::Context::new(window.clone()).map_err(|error| error.to_string())?;
        let presenter = softbuffer::Surface::new(&context, window.clone())
            .map_err(|error| error.to_string())?;
        self.presenter = Some(presenter);
        self.a11y = Some(a11y);
        self.window = Some(window.clone());
        self.record_window_geometry();
        window.request_redraw();
        Ok(())
    }

    pub(super) fn redraw(&mut self) -> Result<(), String> {
        // Most state-changing input paths prepare the next immutable snapshot
        // before requesting a frame. Rebuild only as a fallback for paint-only
        // wakes (focus, hover, expose), never twice for the same frame.
        if !self.frame_snapshot_prepared {
            self.rebuild_frame_snapshot();
        }
        let (physical_width, physical_height) = (
            self.window_state.physical_width.max(1),
            self.window_state.physical_height.max(1),
        );
        let required = (physical_width, physical_height);
        if self.raster.as_ref().map(RasterSurface::size) != Some(required) {
            self.raster = Some(
                RasterSurface::new(physical_width, physical_height)
                    .map_err(|error| error.to_string())?,
            );
        }
        let project_picker = self.project_picker_view_state();
        let raster = self.raster.as_mut().expect("raster initialized");
        let canvas = raster.canvas();
        canvas.reset_matrix();
        canvas.clear(skia_safe::Color::WHITE);
        let scale = self.window_state.scale_factor as f32;
        if self.accessibility_tree_dirty {
            if let Some(a11y) = self.a11y.as_mut() {
                a11y.push(accessibility_tree(
                    &self.frame_snapshot,
                    self.window_state.scale_factor,
                ));
                self.accessibility_tree_dirty = false;
            }
        }
        canvas.scale((scale, scale));
        let theme = crate::preferences::theme_for_state(&self.app_state);
        let ime_cursor_area = {
            let mut painter = FramePainter::new(&mut self.renderer, canvas);
            WorkspaceShell::paint_snapshot_with_project_picker(
                &mut painter,
                &self.frame_snapshot,
                &self.app_state,
                self.composer.input_state(),
                &self.terminal_grid,
                self.terminal_controller.selection(),
                &project_picker,
                self.project_picker_controller.input_state(),
                self.hovered_widget,
                &theme,
            );
            // Focus sync already anchors IME to the input bounds. Probe the
            // exact caret only while a candidate/preedit session is active,
            // avoiding a second text-layout pass for ordinary redraws.
            (self.window_focused
                && self.focused_widget == Some(COMPOSER_ID)
                && self.composer.input_state().composition().is_some())
            .then(|| {
                Composer::ime_cursor_area(
                    &mut painter,
                    self.frame_snapshot.layout.composer,
                    self.composer.input_state(),
                    &self.app_state,
                    &theme,
                )
            })
            .flatten()
        };
        if let (Some(window), Some(area)) = (self.window.as_deref(), ime_cursor_area) {
            crate::ime::set_cursor_area(window, area);
        }

        let presenter = self
            .presenter
            .as_mut()
            .ok_or_else(|| "presenter is not initialized".to_owned())?;
        if self.presenter_size != Some(required) {
            presenter
                .resize(
                    NonZeroU32::new(physical_width).expect("positive width"),
                    NonZeroU32::new(physical_height).expect("positive height"),
                )
                .map_err(|error| error.to_string())?;
            self.presenter_size = Some(required);
        }
        let mut buffer = presenter.buffer_mut().map_err(|error| error.to_string())?;
        if !raster.copy_rgb_to(&mut buffer) {
            return Err("Skia framebuffer copy failed".into());
        }
        if let Some(window) = self.window.as_ref() {
            window.pre_present_notify();
        }
        buffer.present().map_err(|error| error.to_string())?;
        self.frame_snapshot_prepared = false;
        self.window_state.dirty = false;
        Ok(())
    }

    pub(super) fn begin_window_gesture(&self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        #[cfg(not(target_os = "macos"))]
        {
            let (width, height) = self.window_state.logical_size();
            if let Some(direction) = resize_direction(
                self.window_state.cursor_logical.x,
                self.window_state.cursor_logical.y,
                width,
                height,
            ) {
                let _ = window.drag_resize_window(direction);
                return;
            }
        }
        let geometry = self.frame_snapshot.layout;
        if is_drag_region(self.window_state.cursor_logical, &geometry) {
            let _ = window.drag_window();
        }
    }

    pub(super) fn rebuild_frame_snapshot(&mut self) {
        let (width, height) = self.window_state.logical_size();
        let project_picker = self.project_picker_view_state();
        let mut snapshot = WorkspaceSnapshot::build_with_project_picker(
            &self.app_state,
            width,
            height,
            self.window_state.safe_area_insets,
            &project_picker,
        );
        snapshot.focused = self
            .focused_widget
            .filter(|focused| snapshot.node(*focused).is_some())
            .or(snapshot.focused);
        self.focused_widget = snapshot.focused;
        self.hovered_widget = snapshot.hit_test(self.window_state.cursor_logical);
        self.frame_snapshot = snapshot;
        self.frame_snapshot_prepared = true;
        self.accessibility_tree_dirty = true;
    }

    pub(super) fn record_window_geometry(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let size = window.inner_size();
        let minimized = window.is_minimized().unwrap_or(false);
        if minimized || size.width == 0 || size.height == 0 {
            return;
        }
        let fallback = self.window_geometry.unwrap_or(WindowGeometry {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
            maximized: false,
        });
        let position = window
            .outer_position()
            .unwrap_or(winit::dpi::PhysicalPosition::new(fallback.x, fallback.y));
        let maximized = window.is_maximized();
        let reported = WindowGeometry {
            x: position.x,
            y: position.y,
            width: size.width.max(1),
            height: size.height.max(1),
            maximized,
        };
        if let Some(saved) = self.window_geometry.as_mut() {
            update_window_geometry(saved, reported, maximized, minimized);
        } else {
            self.window_geometry = Some(reported);
        }
    }

    pub(super) fn request_redraw(&mut self) {
        self.window_state.dirty = true;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    pub(super) fn update_accessibility_window_bounds(&mut self) {
        if let (Some(a11y), Some(window)) = (self.a11y.as_mut(), self.window.as_ref()) {
            a11y.update_window_bounds(window);
        }
    }
}
