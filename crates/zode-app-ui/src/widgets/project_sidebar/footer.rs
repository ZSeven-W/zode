use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::ZodeAppState;

use super::{paint::draw_label, paint::paint_icon_button, ProjectSidebar, SidebarLayout};
use crate::{paint_single_line, SemanticIcon, WidgetId, ZodeTheme, HELP_ID};

pub(super) fn paint_footer(
    painter: &mut dyn Painter,
    layout: &SidebarLayout,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    hovered: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    if layout.footer.size.y <= 0.0 {
        return;
    }
    painter.fill_rect(
        Rect::xywh(
            layout.footer.origin.x,
            layout.footer.origin.y,
            layout.footer.size.x,
            1.0,
        ),
        theme.tokens.border.with_alpha(0.72),
    );
    if ProjectSidebar::footer_selected(state) {
        painter.fill_round_rect(layout.profile, 0.0, theme.tokens.row_selected);
    }
    if layout.compact {
        paint_avatar(
            painter,
            Rect::xywh(
                layout.profile.origin.x + (layout.profile.size.x - 20.0) / 2.0,
                layout.profile.origin.y + (layout.profile.size.y - 20.0) / 2.0,
                20.0,
                20.0,
            ),
            &state.local_profile.display_name,
            theme,
        );
        paint_icon_button(
            painter,
            layout.help,
            SemanticIcon::Help,
            focused == Some(HELP_ID),
            hovered == Some(HELP_ID),
            theme,
        );
        return;
    }
    let avatar = Rect::xywh(
        layout.profile.origin.x + 16.0,
        layout.profile.origin.y + (layout.profile.size.y - 20.0) / 2.0,
        20.0,
        20.0,
    );
    paint_avatar(painter, avatar, &state.local_profile.display_name, theme);
    draw_label(
        painter,
        state.local_profile.display_name.as_str(),
        Rect::xywh(
            layout.profile.origin.x + 44.0,
            layout.profile.origin.y,
            (layout.profile.size.x - 52.0).max(0.0),
            layout.profile.size.y,
        ),
        12.0,
        500,
        theme.sidebar_foreground,
    );
    paint_icon_button(
        painter,
        layout.help,
        SemanticIcon::Help,
        focused == Some(HELP_ID),
        hovered == Some(HELP_ID),
        theme,
    );
}

fn paint_avatar(painter: &mut dyn Painter, rect: Rect, display_name: &str, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, rect.size.x / 2.0, theme.zode_purple);
    let initial = display_name
        .chars()
        .next()
        .map(|value| value.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "Z".into());
    paint_single_line(
        painter,
        &initial,
        rect,
        10.0,
        600,
        theme.tokens.primary_foreground,
        HorizontalAlign::Center,
    );
}
