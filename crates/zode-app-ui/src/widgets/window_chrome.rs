use jian_widgets::{Painter, Rect};

use crate::{RectExt, WorkspaceLayout, ZodeTheme};

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
            paint_sidebar_material_edge(painter, geometry.sidebar, theme);
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

/// Approximate the inner edge produced by macOS sidebar material.
///
/// The live Softbuffer presenter is opaque RGB, so it cannot expose a native
/// `NSVisualEffectView` behind the raster. A short translucent ramp preserves
/// the same visual separation without changing the window or Jian renderer.
fn paint_sidebar_material_edge(painter: &mut dyn Painter, sidebar: Rect, theme: &ZodeTheme) {
    if !theme.uses_sidebar_material() || sidebar.size.x < 8.0 {
        return;
    }

    const EDGE_ALPHA: [f32; 8] = [0.004, 0.008, 0.016, 0.016, 0.024, 0.024, 0.035, 0.09];
    let edge_x = sidebar.max_x() - EDGE_ALPHA.len() as f32;
    for (index, alpha) in EDGE_ALPHA.into_iter().enumerate() {
        painter.fill_rect(
            Rect::xywh(edge_x + index as f32, sidebar.origin.y, 1.0, sidebar.size.y),
            jian_widgets::Color::BLACK.with_alpha(alpha),
        );
    }
}
