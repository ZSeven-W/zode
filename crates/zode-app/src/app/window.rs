use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use jian_widgets::Point2D;
use winit::event_loop::ActiveEventLoop;
use zode_app_model::ThemePreference;
use zode_app_ui::{
    accessibility_tree, Composer, ComposerContextMenu, GlobalSearch,
    PrimarySidebarPreview as PrimarySidebarPreviewWidget, WidgetId, WorkspaceShell,
    WorkspaceSnapshot, COMPOSER_BRANCH_SEARCH_ID, COMPOSER_ID, GLOBAL_SEARCH_INPUT_ID,
};

#[cfg(not(target_os = "macos"))]
use crate::event_map::resize_direction;
use crate::{
    accessibility_host::{AccessibilityBridge, AccessibilityHost},
    event_map::{is_drag_region, map_system_theme, window_theme_override},
    presenter::{LivePresenter, PresentationFrame},
    render::{FramePainter, NativeBackend, RasterSurface},
    window_bootstrap::hidden_window_attributes_for_placement,
    window_state::{update_window_geometry, AppWake, WindowGeometry, WindowState},
    work_area::{resolve_startup_placement, PlatformWorkAreaProvider},
};

use super::DesktopApp;

/// How long to keep suppressing the AccessKit tree push / raster shrink after
/// the last live-resize tick before treating the window as settled.
const RESIZE_SETTLE_DURATION: Duration = Duration::from_millis(150);

/// The raster surface is allocated to a multiple of this many physical
/// pixels per axis so a live resize drag does not force a full Skia
/// reallocation on every intermediate size.
const RASTER_ALLOCATION_GRANULARITY: u32 = 256;

/// Once settled, an allocated surface is only shrunk back down when its area
/// is more than this factor larger than what the window actually needs.
const RASTER_SHRINK_AREA_FACTOR: u64 = 2;

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
        self.cursor.invalidate();
        self.app_state.host.system_theme = map_system_theme(window.theme());
        window.set_theme(window_theme_override(self.app_state.ui_preferences.theme));
        let size = window.inner_size();
        self.window_state = WindowState::new(size.width, size.height, window.scale_factor());
        self.renderer = NativeBackend::new(self.window_state.scale_factor as f32);
        self.sync_right_panel_transition_for_viewport_change();
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

    pub(super) fn sync_window_theme(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let preference = self.app_state.ui_preferences.theme;
        window.set_theme(window_theme_override(preference));
        if preference == ThemePreference::System {
            self.app_state.host.system_theme = map_system_theme(window.theme());
        }
    }

    pub(super) fn redraw(&mut self) -> Result<(), String> {
        let now = Instant::now();
        // A live resize delivers a `Resized` event for every intermediate
        // size; settle_resize_if_due() decides whether this tick is still
        // "mid drag" (suppress the AccessKit push, keep the raster surface's
        // current allocation) or the first tick past the settle window (do
        // one final full flush).
        self.settle_resize_if_due(now);
        self.maybe_poll_pull_request_status(now);
        // Layout-affecting paths explicitly invalidate the immutable snapshot.
        // Paint-only wakes (hover, focus, expose) keep using the last valid tree.
        self.ensure_frame_snapshot();
        let primary_sidebar_transition = self.advance_primary_sidebar_transition(now);
        let right_panel_transition = self.advance_right_panel_transition(now);
        if primary_sidebar_transition.layout_changed || right_panel_transition.layout_changed {
            let motion_continues =
                primary_sidebar_transition.needs_frame || right_panel_transition.needs_frame;
            self.rebuild_frame_snapshot_with_accessibility(!motion_continues);
        }
        let preview_needs_redraw = self.advance_primary_sidebar_preview(now);
        if self.terminal_grid_resize_pending {
            self.terminal_grid_resize_pending = false;
            self.resize_terminal_grid();
            // Same settle point: a resized browser panel re-requests its
            // screencast at the new physical width, avoiding an upscaled
            // (blurry) or oversized (wasteful) stream for the old rect.
            self.resize_browser_frame_stream();
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
        let resizing = self.resize_settle_deadline.is_some();
        if raster_needs_reallocation(
            self.raster.as_ref().map(RasterSurface::size),
            required,
            resizing,
        ) {
            let allocated_width =
                round_up_to_granularity(required.0, RASTER_ALLOCATION_GRANULARITY);
            let allocated_height =
                round_up_to_granularity(required.1, RASTER_ALLOCATION_GRANULARITY);
            self.raster = Some(
                RasterSurface::new(allocated_width, allocated_height)
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
        let global_search = self.global_search_view_state();
        // Decoupled `Arc` clone (not a borrow of `self`) so it can outlive
        // the mutable borrows the paint path takes below (raster, renderer,
        // accessibility tree) - the actual `BrowserFrameView` is built from
        // it right at the paint call.
        let browser_frame_data = self.browser_frame_snapshot();
        let primary_sidebar_preview_progress = self.primary_sidebar_preview_progress();
        let primary_sidebar_preview_focus = self.primary_sidebar_preview_paint_focus();
        let workspace_hovered = self
            .workspace_hover_excluding_primary_sidebar_preview(self.window_state.cursor_logical);
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
        let (
            composer_ime_cursor_area,
            branch_search_ime_cursor_area,
            global_search_ime_cursor_area,
        ) = {
            let mut painter = FramePainter::new(&mut self.renderer, canvas);
            let browser_frame = browser_frame_data.as_ref().map(|(image_id, bytes)| {
                zode_app_ui::BrowserFrameView {
                    image_id: *image_id,
                    encoded: bytes.as_slice(),
                }
            });
            let image_source: Option<&dyn zode_app_ui::TranscriptImageSource> =
                Some(&self.transcript_images);
            WorkspaceShell::paint_snapshot_with_overlays(
                &mut painter,
                &self.frame_snapshot,
                &self.app_state,
                self.composer.input_state(),
                &self.terminal_grid,
                self.terminal_controller.selection(),
                browser_frame,
                image_source,
                &project_picker,
                self.project_picker_controller.input_state(),
                &global_search,
                self.global_search_controller.input_state(),
                self.branch_picker_controller.input_state(),
                self.session_rename_controller.input_state(),
                workspace_hovered,
                self.modifiers.super_key(),
                Some(self.transcript_find_controller.input_state()),
                &theme,
            );
            PrimarySidebarPreviewWidget::paint(
                &mut painter,
                self.frame_snapshot.layout.viewport,
                &self.app_state,
                primary_sidebar_preview_focus,
                self.hovered_widget,
                self.modifiers.super_key(),
                primary_sidebar_preview_progress,
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
            let global_search = (self.window_focused
                && self.focused_widget == Some(GLOBAL_SEARCH_INPUT_ID))
            .then(|| {
                let search = self.frame_snapshot.node(GLOBAL_SEARCH_INPUT_ID)?.rect;
                GlobalSearch::ime_cursor_area(
                    &mut painter,
                    search,
                    self.global_search_controller.input_state(),
                    &theme,
                )
            })
            .flatten();
            (composer, branch_search, global_search)
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
        } else if self.window_focused && self.focused_widget == Some(GLOBAL_SEARCH_INPUT_ID) {
            if let (Some(window), Some(area)) =
                (self.window.as_deref(), global_search_ime_cursor_area)
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
        self.window_state.dirty = preview_needs_redraw
            || primary_sidebar_transition.needs_frame
            || right_panel_transition.needs_frame;
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
        self.rebuild_frame_snapshot_with_accessibility(true);
    }

    /// Rebuild the live paint and hit-test snapshot for an interpolated frame.
    /// AccessKit only needs the command/start state and the settled end state;
    /// pushing the full tree on every 16 ms step makes long conversations drop
    /// animation frames. IME geometry remains live because the composer moves
    /// with the primary surface during the transition.
    fn rebuild_frame_snapshot_with_accessibility(&mut self, invalidate_accessibility: bool) {
        let (width, height) = self.window_state.logical_size();
        let project_picker = self.project_picker_view_state();
        let global_search = self.global_search_view_state();
        let mut snapshot = WorkspaceSnapshot::build_with_overlays_and_sidebar_options(
            &self.app_state,
            width,
            height,
            self.window_state.safe_area_insets,
            &project_picker,
            &global_search,
            self.animated_sidebar_layout_options(),
        );
        snapshot.focused = self
            .focused_widget
            .filter(|focused| snapshot.node(*focused).is_some())
            .or(snapshot.focused);
        self.focused_widget = snapshot.focused;
        self.frame_snapshot = snapshot;
        self.hovered_widget = self.primary_sidebar_aware_hover(self.window_state.cursor_logical);
        self.frame_snapshot_valid = true;
        self.window_state.snapshot_scroll_only_invalid = false;
        if invalidate_accessibility {
            self.accessibility_tree_dirty = true;
        }
        self.ime.mark_area_dirty();
    }

    pub(super) fn ensure_frame_snapshot(&mut self) {
        if snapshot_requires_rebuild(
            self.frame_snapshot_valid,
            self.window_state.snapshot_scroll_only_invalid,
            SnapshotUse::Current,
        ) {
            // Mirrors the sidebar/right-panel transition pattern above: the
            // Rust-side snapshot (layout + hit-testing) always has to catch
            // up with the new size, but while a live resize is still
            // in-flight the AccessKit tree push is deferred to the settled
            // frame so a resize drag does not push a full tree every tick.
            let resizing = self.resize_settle_deadline.is_some();
            self.rebuild_frame_snapshot_with_accessibility(!resizing);
        }
    }

    /// Record that a `Resized`/`ScaleFactorChanged` event just arrived. Marks
    /// the layout snapshot stale (without forcing an AccessKit push - see
    /// `ensure_frame_snapshot`) and (re)arms the settle deadline consumed by
    /// `settle_resize_if_due`.
    pub(super) fn begin_resize_settle(&mut self) {
        self.resize_settle_deadline = Some(Instant::now() + RESIZE_SETTLE_DURATION);
        self.invalidate_frame_snapshot_for_resize();
    }

    /// Clears the settle deadline and forces a final AccessKit flush once
    /// the window has been still for `RESIZE_SETTLE_DURATION`; otherwise
    /// keeps the window dirty so another redraw is scheduled even if the
    /// user stops resizing and no further OS event arrives - mirroring how
    /// sidebar/right-panel motion keeps requesting frames via
    /// `about_to_wait` until it settles.
    fn settle_resize_if_due(&mut self, now: Instant) {
        let Some(deadline) = self.resize_settle_deadline else {
            return;
        };
        if now >= deadline {
            self.resize_settle_deadline = None;
            self.accessibility_tree_dirty = true;
        } else {
            self.window_state.dirty = true;
        }
    }

    pub(super) fn ensure_scroll_geometry_snapshot(&mut self) {
        if snapshot_requires_rebuild(
            self.frame_snapshot_valid,
            self.window_state.snapshot_scroll_only_invalid,
            SnapshotUse::ScrollGeometry,
        ) {
            self.rebuild_frame_snapshot();
        }
    }

    pub(super) fn invalidate_frame_snapshot(&mut self) {
        self.frame_snapshot_valid = false;
        self.window_state.snapshot_scroll_only_invalid = false;
        self.accessibility_tree_dirty = true;
        self.ime.mark_area_dirty();
        self.ime.invalidate_native();
    }

    /// Same as `invalidate_frame_snapshot`, but leaves `accessibility_tree_dirty`
    /// untouched: `ensure_frame_snapshot`/`settle_resize_if_due` alone decide
    /// when a live resize should push the AccessKit tree.
    fn invalidate_frame_snapshot_for_resize(&mut self) {
        self.frame_snapshot_valid = false;
        self.window_state.snapshot_scroll_only_invalid = false;
        self.ime.mark_area_dirty();
        self.ime.invalidate_native();
    }

    /// Scroll offsets move interaction nodes within an otherwise stable shell
    /// viewport. Keep the last viewport geometry until the next redraw so a
    /// burst of trackpad events results in one snapshot rebuild.
    pub(super) fn invalidate_frame_snapshot_for_scroll(&mut self) {
        if !self.frame_snapshot_valid {
            return;
        }
        self.frame_snapshot_valid = false;
        self.window_state.snapshot_scroll_only_invalid = true;
        self.accessibility_tree_dirty = true;
        self.ime.mark_area_dirty();
        self.ime.invalidate_native();
    }

    /// Composer typing within the same submit-capability state does not change
    /// shell geometry or hit testing. Keep the existing snapshot and update
    /// only the value exposed to accessibility.
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

    pub(super) fn update_native_cursor(&mut self, hint: crate::cursor::CursorHint) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if let Some(icon) = self.cursor.changed_icon(hint) {
            window.set_cursor(icon);
        }
    }

    pub(super) fn update_accessibility_window_bounds(&mut self) {
        if let (Some(a11y), Some(window)) = (self.a11y.as_mut(), self.window.as_ref()) {
            a11y.update_window_bounds(window);
        }
    }

    pub(super) fn primary_sidebar_aware_hover(&self, point: Point2D) -> Option<WidgetId> {
        resolve_primary_sidebar_hover(
            self.primary_sidebar_preview_hit(point),
            self.primary_sidebar_preview_contains(point),
            self.frame_snapshot.hit_test(point),
        )
    }

    fn workspace_hover_excluding_primary_sidebar_preview(
        &self,
        point: Point2D,
    ) -> Option<WidgetId> {
        resolve_primary_sidebar_hover(
            None,
            self.primary_sidebar_preview_contains(point),
            self.frame_snapshot.hit_test(point),
        )
    }
}

fn resolve_primary_sidebar_hover(
    preview_hit: Option<WidgetId>,
    preview_contains_point: bool,
    workspace_hit: Option<WidgetId>,
) -> Option<WidgetId> {
    if preview_contains_point {
        preview_hit
    } else {
        preview_hit.or(workspace_hit)
    }
}

/// Rounds `value` up to the next multiple of `granularity` (both clamped to
/// at least 1), giving the raster surface headroom so small size changes
/// during a live resize drag reuse the existing allocation.
fn round_up_to_granularity(value: u32, granularity: u32) -> u32 {
    let value = value.max(1);
    let granularity = granularity.max(1);
    value.div_ceil(granularity) * granularity
}

/// Decides whether the CPU raster surface must be (re)allocated. The surface
/// is kept sticky (never reallocated) while `resizing` is true as long as it
/// still fits `required`; once settled, it is only shrunk back down if it is
/// oversized by more than `RASTER_SHRINK_AREA_FACTOR`.
fn raster_needs_reallocation(
    allocated: Option<(u32, u32)>,
    required: (u32, u32),
    resizing: bool,
) -> bool {
    let Some((allocated_width, allocated_height)) = allocated else {
        return true;
    };
    let fits = allocated_width >= required.0 && allocated_height >= required.1;
    if !fits {
        return true;
    }
    if resizing {
        return false;
    }
    let allocated_area = u64::from(allocated_width) * u64::from(allocated_height);
    let required_area = u64::from(required.0) * u64::from(required.1);
    allocated_area > required_area.saturating_mul(RASTER_SHRINK_AREA_FACTOR)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotUse {
    Current,
    ScrollGeometry,
}

fn snapshot_requires_rebuild(valid: bool, scroll_only_invalid: bool, use_: SnapshotUse) -> bool {
    !(valid || scroll_only_invalid && use_ == SnapshotUse::ScrollGeometry)
}

#[cfg(test)]
mod tests {
    use zode_app_ui::{Insets, WidgetId, WorkspaceSnapshot, COMPOSER_ID};

    use super::{
        raster_needs_reallocation, resolve_primary_sidebar_hover, round_up_to_granularity,
        snapshot_requires_rebuild, update_composer_snapshot_value, SnapshotUse,
    };

    #[test]
    fn granularity_rounds_up_and_never_returns_zero() {
        assert_eq!(round_up_to_granularity(1, 256), 256);
        assert_eq!(round_up_to_granularity(256, 256), 256);
        assert_eq!(round_up_to_granularity(257, 256), 512);
        assert_eq!(round_up_to_granularity(0, 256), 256);
        assert_eq!(round_up_to_granularity(100, 0), 100);
    }

    #[test]
    fn raster_reallocates_when_absent_or_too_small() {
        assert!(raster_needs_reallocation(None, (800, 600), false));
        assert!(raster_needs_reallocation(
            Some((800, 600)),
            (801, 600),
            true
        ));
        assert!(raster_needs_reallocation(
            Some((800, 600)),
            (800, 601),
            false
        ));
    }

    #[test]
    fn raster_stays_sticky_mid_resize_even_when_oversized() {
        // A surface much larger than required must be kept as-is while a
        // live resize is still in flight, so every intermediate size during
        // a drag reuses the same allocation instead of reallocating.
        assert!(!raster_needs_reallocation(
            Some((4096, 4096)),
            (800, 600),
            true
        ));
    }

    #[test]
    fn raster_only_shrinks_once_settled_and_far_oversized() {
        // Settled and only mildly oversized (< 2x area): keep the surface.
        assert!(!raster_needs_reallocation(
            Some((900, 900)),
            (800, 600),
            false
        ));
        // Settled and far oversized (> 2x area): shrink back down.
        assert!(raster_needs_reallocation(
            Some((4096, 4096)),
            (800, 600),
            false
        ));
    }

    #[test]
    fn primary_sidebar_surface_blocks_workspace_hover_without_a_preview_hit() {
        let workspace = WidgetId(41);

        assert_eq!(
            resolve_primary_sidebar_hover(None, true, Some(workspace)),
            None
        );
    }

    #[test]
    fn primary_sidebar_hit_wins_and_workspace_hover_survives_outside() {
        let preview = WidgetId(42);
        let workspace = WidgetId(43);

        assert_eq!(
            resolve_primary_sidebar_hover(Some(preview), true, Some(workspace)),
            Some(preview)
        );
        assert_eq!(
            resolve_primary_sidebar_hover(None, false, Some(workspace)),
            Some(workspace)
        );
    }

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

    #[test]
    fn scroll_geometry_can_coalesce_but_interaction_requires_current_nodes() {
        assert!(!snapshot_requires_rebuild(
            false,
            true,
            SnapshotUse::ScrollGeometry
        ));
        assert!(snapshot_requires_rebuild(false, true, SnapshotUse::Current));
        assert!(snapshot_requires_rebuild(
            false,
            false,
            SnapshotUse::ScrollGeometry
        ));
        assert!(!snapshot_requires_rebuild(
            true,
            false,
            SnapshotUse::Current
        ));
    }
}
