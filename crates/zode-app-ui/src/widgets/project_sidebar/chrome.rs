use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect};
use zode_app_model::ZodeAppState;

use super::{paint::paint_icon_button, SidebarLayout, SIDEBAR_SEARCH_ID, SIDEBAR_TOGGLE_ID};
use crate::{paint_single_line, SemanticIcon, WidgetId, ZodeTheme};

pub(super) fn paint(
    painter: &mut dyn Painter,
    layout: &SidebarLayout,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    paint_brand(painter, layout.brand, layout.compact, theme);
    if layout.compact {
        return;
    }

    paint_icon_button(
        painter,
        layout.titlebar_toggle,
        SemanticIcon::Sidebar,
        focused == Some(SIDEBAR_TOGGLE_ID),
        hovered == Some(SIDEBAR_TOGGLE_ID),
        theme,
    );
    // Match the native titlebar without exposing inert controls. These remain
    // disabled until task-history navigation has a real command contract.
    paint_disabled_icon(painter, layout.titlebar_back, SemanticIcon::Back, theme);
    paint_disabled_icon(
        painter,
        layout.titlebar_forward,
        SemanticIcon::Forward,
        theme,
    );

    let search_active = state.global_search.open;
    if search_active {
        painter.fill_round_rect(layout.brand_search, 6.0, theme.sidebar_row_selected);
    } else if focused == Some(SIDEBAR_SEARCH_ID) || hovered == Some(SIDEBAR_SEARCH_ID) {
        painter.fill_round_rect(layout.brand_search, 6.0, theme.sidebar_row_hover);
    }
    let icon_size = 14.0;
    painter.stroke_svg_path(
        SemanticIcon::Search.path(),
        Point2D::new(
            layout.brand_search.origin.x + (layout.brand_search.size.x - icon_size) / 2.0,
            layout.brand_search.origin.y + (layout.brand_search.size.y - icon_size) / 2.0,
        ),
        icon_size,
        theme.sidebar_foreground,
        SemanticIcon::Search.stroke_width(),
    );
}

fn paint_disabled_icon(
    painter: &mut dyn Painter,
    rect: Rect,
    icon: SemanticIcon,
    theme: &ZodeTheme,
) {
    let size = 14.0;
    painter.stroke_svg_path(
        icon.path(),
        Point2D::new(
            rect.origin.x + (rect.size.x - size) / 2.0,
            rect.origin.y + (rect.size.y - size) / 2.0,
        ),
        size,
        theme.sidebar_disabled_foreground,
        icon.stroke_width(),
    );
}

fn paint_brand(painter: &mut dyn Painter, rect: Rect, compact: bool, theme: &ZodeTheme) {
    paint_single_line(
        painter,
        if compact { "Z" } else { "Zode" },
        rect,
        if compact { 14.0 } else { 17.0 },
        600,
        theme.sidebar_foreground,
        HorizontalAlign::Start,
    );
}
