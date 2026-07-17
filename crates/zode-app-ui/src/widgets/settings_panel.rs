use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::{AppCommand, ZodeAppState};
use zode_node_protocol::WorkspaceUri;

use crate::ZodeTheme;

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRow {
    pub tool: String,
    pub revoke_command: AppCommand,
}

pub struct SettingsPanel;

impl SettingsPanel {
    pub fn permission_rows(
        state: &ZodeAppState,
        workspace_uri: &WorkspaceUri,
    ) -> Vec<PermissionRow> {
        state
            .project_permissions
            .get(workspace_uri)
            .into_iter()
            .flatten()
            .map(|tool| PermissionRow {
                tool: tool.clone(),
                revoke_command: AppCommand::RevokeProjectPermission {
                    workspace_uri: workspace_uri.clone(),
                    tool: tool.clone(),
                },
            })
            .collect()
    }

    pub fn paint(
        painter: &mut dyn Painter,
        rect: Rect,
        state: &ZodeAppState,
        workspace_uri: &WorkspaceUri,
        theme: &ZodeTheme,
    ) {
        draw_text(
            painter,
            "项目权限",
            Point2D::new(rect.origin.x, rect.origin.y + 24.0),
            16.0,
            600,
            theme.tokens.foreground,
        );
        for (index, row) in Self::permission_rows(state, workspace_uri)
            .iter()
            .enumerate()
        {
            let y = rect.origin.y + 48.0 + index as f32 * 38.0;
            painter.fill_round_rect(
                Rect::xywh(rect.origin.x, y, rect.size.x, 30.0),
                7.0,
                theme.tokens.muted,
            );
            draw_text(
                painter,
                &row.tool,
                Point2D::new(rect.origin.x + 10.0, y + 20.0),
                12.0,
                500,
                theme.tokens.foreground,
            );
        }
    }
}

fn draw_text(
    painter: &mut dyn Painter,
    text: &str,
    origin: Point2D,
    size: f32,
    weight: u16,
    color: jian_widgets::Color,
) {
    let layout = TextLayout::single_run(text, "system-ui", size, color.to_jian(), Point2D::ZERO)
        .with_font_weight(weight);
    painter.draw_text(&layout, origin);
}
