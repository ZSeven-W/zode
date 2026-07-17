use jian_widgets::{Painter, Point2D, Rect};
use zode_app_model::ZodeAppState;

use super::ProjectSidebar;
use crate::{RectExt, WidgetId, ZodeTheme};

/// Paints the collapsed primary sidebar as a transient overlay without
/// changing the workspace layout or the native material split.
pub struct PrimarySidebarPreview;

impl PrimarySidebarPreview {
    /// Returns the sidebar's painted geometry for the current transition.
    ///
    /// Both paint and host-side input routing use this exact cubic easing so
    /// the clickable surface never gets ahead of the pixels sliding onscreen.
    pub fn translated_rect(sidebar: Rect, progress: f32) -> Rect {
        let progress = progress.clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        Rect::xywh(
            sidebar.origin.x - (1.0 - eased) * sidebar.size.x,
            sidebar.origin.y,
            sidebar.size.x,
            sidebar.size.y,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        painter: &mut dyn Painter,
        viewport: Rect,
        state: &ZodeAppState,
        focused: Option<WidgetId>,
        hovered: Option<WidgetId>,
        show_shortcuts: bool,
        progress: f32,
        theme: &ZodeTheme,
    ) {
        let progress = progress.clamp(0.0, 1.0);
        if progress <= 0.0 {
            return;
        }

        let sidebar = ProjectSidebar::transient_rect(viewport, state);
        let painted_sidebar = Self::translated_rect(sidebar, progress);
        let offset = Point2D::new(painted_sidebar.origin.x - sidebar.origin.x, 0.0);
        let background = if theme.uses_native_sidebar_material() {
            theme.tokens.background.with_alpha(0.97)
        } else {
            theme.sidebar
        };

        painter.save();
        painter.clip_rect(viewport);
        painter.translate(offset);
        painter.fill_drop_shadow(
            Rect::xywh(
                sidebar.origin.x,
                sidebar.origin.y + 2.0,
                sidebar.size.x,
                (sidebar.size.y - 4.0).max(0.0),
            ),
            0.0,
            22.0,
            theme.tokens.foreground.with_alpha(0.14),
        );
        painter.fill_rect(sidebar, background);
        painter.stroke_line(
            Point2D::new(sidebar.max_x(), sidebar.origin.y),
            Point2D::new(sidebar.max_x(), sidebar.max_y()),
            // `tokens.border` is already `foreground.with_alpha(0.08)`;
            // `with_alpha` replaces rather than multiplies, so chaining it
            // here discarded that 8% and recomposited near-black
            // `foreground` at 72%, painting a solid dark edge instead of a
            // soft one. Compute straight from `foreground` at a modest alpha.
            theme.tokens.foreground.with_alpha(0.16),
            1.0,
        );
        ProjectSidebar::paint_with_interaction(
            painter,
            sidebar,
            state,
            focused,
            hovered,
            show_shortcuts,
            theme,
        );
        ProjectSidebar::paint_hover_overlay(painter, sidebar, state, focused, hovered, theme);
        painter.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::PrimarySidebarPreview;
    use jian_widgets::Rect;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.000_1,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn translated_rect_uses_the_same_cubic_easing_as_paint() {
        let sidebar = Rect::xywh(0.0, 0.0, 240.0, 900.0);

        let hidden = PrimarySidebarPreview::translated_rect(sidebar, 0.0);
        assert_close(hidden.origin.x, -240.0);
        assert_close(hidden.size.x, 240.0);

        let halfway = PrimarySidebarPreview::translated_rect(sidebar, 0.5);
        assert_close(halfway.origin.x, -30.0);
        assert_close(halfway.size.x, 240.0);

        let open = PrimarySidebarPreview::translated_rect(sidebar, 1.0);
        assert_close(open.origin.x, 0.0);
        assert_close(open.size.x, 240.0);
    }

    #[test]
    fn translated_rect_clamps_transition_progress() {
        let sidebar = Rect::xywh(12.0, 8.0, 200.0, 500.0);

        assert_eq!(
            PrimarySidebarPreview::translated_rect(sidebar, -1.0),
            Rect::xywh(-188.0, 8.0, 200.0, 500.0)
        );
        assert_eq!(
            PrimarySidebarPreview::translated_rect(sidebar, 2.0),
            sidebar
        );
    }
}
