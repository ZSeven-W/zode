use std::time::Instant;

use jian_widgets::{Point2D, Rect};
use zode_app_model::ShellRoute;
use zode_app_ui::{
    CollapsedSidebarChrome, PrimarySidebarPreview as PrimarySidebarPreviewWidget, ProjectSidebar,
    RectExt, WidgetId,
};

use super::{
    primary_sidebar_preview::{PrimarySidebarPreviewContext, PrimarySidebarPreviewRegions},
    DesktopApp,
};

impl DesktopApp {
    pub(super) fn primary_sidebar_preview_progress(&self) -> f32 {
        if self.primary_sidebar_transition_is_present() {
            0.0
        } else {
            self.primary_sidebar_preview.progress()
        }
    }

    pub(super) fn primary_sidebar_preview_is_visible(&self) -> bool {
        preview_has_painted_surface(
            self.primary_sidebar_transition_is_present(),
            self.primary_sidebar_preview.progress(),
        )
    }

    pub(super) fn active_primary_sidebar_rect(&self) -> Rect {
        if self.primary_sidebar_transition_is_present() {
            self.frame_snapshot.layout.primary_sidebar_content
        } else {
            let sidebar = ProjectSidebar::transient_rect(
                self.frame_snapshot.layout.viewport,
                &self.app_state,
            );
            PrimarySidebarPreviewWidget::translated_rect(
                sidebar,
                self.primary_sidebar_preview.progress(),
            )
        }
    }

    pub(super) fn primary_sidebar_preview_hit(&self, point: Point2D) -> Option<WidgetId> {
        if !self.primary_sidebar_preview_contains(point) {
            return None;
        }
        ProjectSidebar::transient_hit_test(
            self.active_primary_sidebar_rect(),
            &self.app_state,
            point,
        )
    }

    pub(super) fn primary_sidebar_preview_contains(&self, point: Point2D) -> bool {
        if !self.primary_sidebar_preview_is_visible() {
            return false;
        }
        let viewport = self.frame_snapshot.layout.viewport;
        if !viewport.contains(point) {
            return false;
        }
        let sidebar = self.active_primary_sidebar_rect();
        let menu = ProjectSidebar::menu_layout(sidebar, &self.app_state).map(|menu| menu.rect);
        preview_surface_contains(sidebar, menu, point)
    }

    pub(super) fn update_primary_sidebar_preview_hover(
        &mut self,
        pointer: Option<Point2D>,
    ) -> bool {
        let regions = self.primary_sidebar_preview_regions();
        let context = self.primary_sidebar_preview_context();
        self.primary_sidebar_preview
            .update_hover(pointer, regions, context)
    }

    pub(super) fn dismiss_primary_sidebar_preview(&mut self) -> bool {
        let preview_changed = self.primary_sidebar_preview.dismiss();
        let interaction_changed = self.clear_primary_sidebar_preview_interaction();
        preview_changed || interaction_changed
    }

    /// Arms the hover-preview suppression latch. Call this whenever an
    /// explicit collapse fires (toggle button, shortcut, or drag-to-collapse)
    /// while the pointer may still be sitting inside the trigger zone the
    /// collapsed chrome renders at - otherwise the very next hover check
    /// reopens the preview the user just collapsed away from. See
    /// `PrimarySidebarPreview::suppress_until_pointer_exit` for the exact
    /// clear condition.
    pub(super) fn arm_primary_sidebar_preview_suppression(&mut self) {
        self.primary_sidebar_preview.suppress_until_pointer_exit();
    }

    pub(super) fn advance_primary_sidebar_preview(&mut self, now: Instant) -> bool {
        let pointer = self
            .window_focused
            .then_some(self.window_state.cursor_logical);
        self.update_primary_sidebar_preview_hover(pointer);
        let duration = self.primary_sidebar_preview.transition_duration();
        let needs_frame = self.primary_sidebar_preview.advance(now, duration);
        let interaction_changed = if self.primary_sidebar_preview_is_visible() {
            false
        } else {
            self.clear_primary_sidebar_preview_interaction()
        };
        needs_frame || interaction_changed
    }

    fn primary_sidebar_preview_regions(&self) -> PrimarySidebarPreviewRegions {
        let overlay =
            ProjectSidebar::transient_rect(self.frame_snapshot.layout.viewport, &self.app_state);
        let trigger = CollapsedSidebarChrome::layout(self.frame_snapshot.layout.top_bar).toggle;
        let menu_corridor = ProjectSidebar::menu_layout(overlay, &self.app_state).map(|menu| {
            let x = (overlay.max_x() - 8.0).max(overlay.origin.x);
            Rect::xywh(
                x,
                menu.rect.origin.y - 8.0,
                (menu.rect.max_x() - x + 8.0).max(0.0),
                menu.rect.size.y + 16.0,
            )
        });
        PrimarySidebarPreviewRegions::new(trigger, overlay, menu_corridor)
    }

    fn primary_sidebar_preview_context(&self) -> PrimarySidebarPreviewContext {
        let blocking_overlay_open = self.app_state.global_search.open
            || self.app_state.project_picker.open
            || self.app_state.session_menu.is_some()
            || self.app_state.session_copy_menu.is_some()
            || self.app_state.session_rename.is_some()
            || self.app_state.open_with.menu_open
            || self.app_state.composer.queue_menu.is_some()
            || self.app_state.composer.context_menu.is_some()
            || self.app_state.composer.footer_menu.is_some()
            || self.app_state.presentation.pinned_summary_overlay_open;
        PrimarySidebarPreviewContext {
            persistent_open: self.primary_sidebar_transition_is_present(),
            settings_active: matches!(self.app_state.presentation.route, ShellRoute::Settings(_)),
            blocking_overlay_open,
            reduced_motion: self.app_state.ui_preferences.reduced_motion,
        }
    }
}

fn preview_surface_contains(sidebar: Rect, menu: Option<Rect>, point: Point2D) -> bool {
    sidebar.contains(point) || menu.is_some_and(|menu| menu.contains(point))
}

fn preview_has_painted_surface(persistent_open: bool, progress: f32) -> bool {
    !persistent_open && progress > 0.0
}

#[cfg(test)]
mod tests {
    use super::{preview_has_painted_surface, preview_surface_contains};
    use jian_widgets::{Point2D, Rect};

    #[test]
    fn only_the_currently_translated_surface_captures_input() {
        let sidebar = Rect::xywh(-210.0, 0.0, 240.0, 900.0);

        assert!(preview_surface_contains(
            sidebar,
            None,
            Point2D::new(20.0, 300.0)
        ));
        assert!(!preview_surface_contains(
            sidebar,
            None,
            Point2D::new(40.0, 300.0)
        ));
    }

    #[test]
    fn zero_progress_never_exposes_an_input_surface() {
        assert!(!preview_has_painted_surface(false, 0.0));
        assert!(preview_has_painted_surface(false, f32::EPSILON));
        assert!(!preview_has_painted_surface(true, 1.0));
    }

    #[test]
    fn translated_menu_surface_is_included_without_expanding_sidebar_hit_area() {
        let sidebar = Rect::xywh(-30.0, 0.0, 240.0, 900.0);
        let menu = Rect::xywh(218.0, 120.0, 220.0, 154.0);

        assert!(preview_surface_contains(
            sidebar,
            Some(menu),
            Point2D::new(300.0, 180.0)
        ));
        assert!(!preview_surface_contains(
            sidebar,
            Some(menu),
            Point2D::new(300.0, 400.0)
        ));
    }
}
