use jian_widgets::{Painter, Rect};

use crate::{WorkspaceLayout, ZodeTheme};

pub struct WindowChrome;

impl WindowChrome {
    pub fn paint(
        painter: &mut dyn Painter,
        viewport: Rect,
        geometry: &WorkspaceLayout,
        theme: &ZodeTheme,
    ) {
        painter.fill_rect(viewport, theme.tokens.background);
        if geometry.sidebar.size.x > 0.0 {
            painter.fill_rect(geometry.sidebar, theme.sidebar);
        }
        painter.fill_rect(geometry.top_bar, theme.tokens.background);
        let separator_x = geometry.top_bar.origin.x;
        painter.stroke_line(
            jian_widgets::Point2D::new(separator_x, geometry.top_bar.origin.y),
            jian_widgets::Point2D::new(
                separator_x,
                geometry.top_bar.origin.y + geometry.top_bar.size.y,
            ),
            theme.tokens.border,
            1.0,
        );
    }
}
