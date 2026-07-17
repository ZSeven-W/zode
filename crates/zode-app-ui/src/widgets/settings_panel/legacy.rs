use jian_widgets::{Painter, Point2D, Rect};
use zode_app_model::ZodeAppState;
use zode_node_protocol::WorkspaceUri;

use super::{
    draw_text, paint_appearance, paint_card, SettingsPanel, APPEARANCE_ROW_HEIGHT, CARD_TOP,
};
use crate::ZodeTheme;

pub(super) fn paint(
    painter: &mut dyn Painter,
    content: Rect,
    state: &ZodeAppState,
    workspace_uri: Option<&WorkspaceUri>,
    offset: f32,
    theme: &ZodeTheme,
) {
    paint_appearance(painter, content, state, offset, theme);
    let rows = workspace_uri
        .map(|workspace| SettingsPanel::permission_rows(state, workspace))
        .unwrap_or_default();
    let top = content.origin.y + CARD_TOP + APPEARANCE_ROW_HEIGHT * 5.0 + 48.0 - offset;
    draw_text(
        painter,
        "项目权限",
        Point2D::new(content.origin.x, top - 18.0),
        13.0,
        600,
        theme.tokens.foreground,
    );
    let card = Rect::xywh(
        content.origin.x,
        top,
        content.size.x,
        (104.0 + rows.len() as f32 * 36.0).max(104.0),
    );
    paint_card(painter, card, theme);
    if rows.is_empty() {
        draw_text(
            painter,
            "无活动项目或已保存权限",
            Point2D::new(card.origin.x + 18.0, card.origin.y + 34.0),
            12.0,
            400,
            theme.tokens.muted_foreground,
        );
    }
}
