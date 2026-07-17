use std::sync::Arc;

use winit::event_loop::ActiveEventLoop;
use zode_app_ui::{
    accessibility_tree, Composer, ComposerContextMenu, WorkspaceShell, WorkspaceSnapshot,
    COMPOSER_BRANCH_SEARCH_ID, COMPOSER_ID,
};

#[cfg(not(target_os = "macos"))]
use crate::event_map::resize_direction;
use crate::{
    accessibility_host::{AccessibilityBridge, AccessibilityHost},
    event_map::{is_drag_region, map_system_theme},
    presenter::{LivePresenter, PresentationFrame},
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
        self.ime.invalidate_native();
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
        let presenter = LivePresenter::new(window.clone())?;
        self.presenter = Some(presenter);
        self.a11y = Some(a11y);
        self.window = Some(window.clone());
        self.record_window_geometry();
        window.request_redraw();
        Ok(())
    }

    pub(super) fn redraw(&mut self) -> Result<(), String> {
        // Layout-affecting paths explicitly invalidate the immutable snapshot.
        // Paint-only wakes (hover, focus, expose) keep using the last valid tree.
        if !self.frame_snapshot_valid {
            self.rebuild_frame_snapshot();
        }
        if self.terminal_grid_resize_pending {
            self.terminal_grid_resize_pending = false;
            self.resize_terminal_grid();
            if !self.frame_snapshot_valid {
                self.rebuild_frame_snapshot();
            }
        }
        if self.window_metrics_update_pending {
            self.window_metrics_update_pending = false;
            self.record_window_geometry();
            self.update_accessibility_window_bounds();
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
        let base_theme = crate::preferences::theme_for_state(&self.app_state);
        let native_sidebar_material = self
            .presenter
            .as_ref()
            .is_some_and(LivePresenter::supports_native_sidebar_material)
            && base_theme.uses_sidebar_material();
        let theme = if native_sidebar_material {
            base_theme.with_native_sidebar_material()
        } else {
            base_theme
        };
        let project_picker = self.project_picker_view_state();
        let raster = self.raster.as_mut().expect("raster initialized");
        let canvas = raster.canvas();
        canvas.reset_matrix();
        canvas.clear(if native_sidebar_material {
            skia_safe::Color::TRANSPARENT
        } else {
            skia_safe::Color::WHITE
        });
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
        let (composer_ime_cursor_area, branch_search_ime_cursor_area) = {
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
                self.branch_picker_controller.input_state(),
                self.session_rename_controller.input_state(),
                self.hovered_widget,
                self.modifiers.super_key(),
                &theme,
            );
            // Focus sync installs a caret-shaped fallback before IME is
            // enabled. Resolve the exact caret on the first focused frame and
            // only after input/layout changes thereafter.
            let composer = (self.window_focused
                && self.focused_widget == Some(COMPOSER_ID)
                && self.ime.needs_area_measurement())
            .then(|| {
                Composer::ime_cursor_area(
                    &mut painter,
                    self.frame_snapshot.layout.composer,
                    self.composer.input_state(),
                    &self.app_state,
                    &theme,
                )
            })
            .flatten();
            let branch_search = (self.window_focused
                && self.focused_widget == Some(COMPOSER_BRANCH_SEARCH_ID))
            .then(|| {
                let search = self.frame_snapshot.node(COMPOSER_BRANCH_SEARCH_ID)?.rect;
                ComposerContextMenu::branch_search_ime_cursor_area(
                    &mut painter,
                    search,
                    self.branch_picker_controller.input_state(),
                    &theme,
                )
            })
            .flatten();
            (composer, branch_search)
        };
        if let Some(area) = composer_ime_cursor_area {
            self.ime.set_area(area);
        }
        if self.window_focused && self.focused_widget == Some(COMPOSER_ID) {
            if let (Some(window), Some(area)) = (self.window.as_deref(), self.ime.area()) {
                self.ime.update_native(window, area);
            }
        } else if self.window_focused && self.focused_widget == Some(COMPOSER_BRANCH_SEARCH_ID) {
            if let (Some(window), Some(area)) =
                (self.window.as_deref(), branch_search_ime_cursor_area)
            {
                self.ime.update_native(window, area);
            }
        }

        let presenter = self
            .presenter
            .as_mut()
            .ok_or_else(|| "presenter is not initialized".to_owned())?;
        let sidebar = self.frame_snapshot.layout.sidebar;
        presenter.present(
            raster,
            PresentationFrame {
                physical_width,
                physical_height,
                scale_factor: self.window_state.scale_factor,
                sidebar_width: sidebar.origin.x + sidebar.size.x,
                native_sidebar_material,
            },
        )?;
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
        self.frame_snapshot_valid = true;
        self.accessibility_tree_dirty = true;
        self.ime.mark_area_dirty();
    }

    pub(super) fn invalidate_frame_snapshot(&mut self) {
        self.frame_snapshot_valid = false;
        self.accessibility_tree_dirty = true;
        self.ime.mark_area_dirty();
        self.ime.invalidate_native();
    }

    /// Composer typing does not change shell geometry or hit testing. Keep the
    /// existing snapshot and update only the value exposed to accessibility.
    pub(super) fn refresh_composer_snapshot_value(&mut self, value_changed: bool) {
        if !self.frame_snapshot_valid {
            return;
        }
        if value_changed
            && !update_composer_snapshot_value(
                &mut self.frame_snapshot,
                &self.app_state.composer.draft,
            )
        {
            self.invalidate_frame_snapshot();
            return;
        }
        if value_changed {
            self.accessibility_tree_dirty = true;
        }
    }

    pub(super) fn refresh_snapshot_focus(&mut self, focused: Option<zode_app_ui::WidgetId>) {
        if !self.frame_snapshot_valid {
            return;
        }
        if self.frame_snapshot.focused != focused {
            self.frame_snapshot.focused = focused;
            self.accessibility_tree_dirty = true;
        }
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

fn update_composer_snapshot_value(snapshot: &mut WorkspaceSnapshot, value: &str) -> bool {
    let Some(node) = snapshot
        .nodes
        .iter_mut()
        .find(|node| node.id == COMPOSER_ID)
    else {
        return false;
    };
    node.value = Some(value.to_owned());
    true
}

#[cfg(test)]
mod tests {
    use zode_app_ui::{Insets, WorkspaceSnapshot, COMPOSER_ID};

    use super::update_composer_snapshot_value;

    #[test]
    fn composer_value_refresh_preserves_layout_and_hit_geometry() {
        let state = zode_app_model::demo_state();
        let mut snapshot = WorkspaceSnapshot::build(&state, 1_221.0, 992.0, Insets::ZERO);
        let layout = snapshot.layout;
        let node_count = snapshot.nodes.len();
        let composer_rect = snapshot.node(COMPOSER_ID).unwrap().rect;

        assert!(update_composer_snapshot_value(
            &mut snapshot,
            "incremental draft"
        ));

        assert_eq!(snapshot.layout, layout);
        assert_eq!(snapshot.nodes.len(), node_count);
        let composer = snapshot.node(COMPOSER_ID).unwrap();
        assert_eq!(composer.rect, composer_rect);
        assert_eq!(composer.value.as_deref(), Some("incremental draft"));
    }
}
