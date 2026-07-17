use jian_widgets::{Painter, Point2D, Rect, TextLayout};
use zode_app_model::{AppCommand, ThemePreference, ZodeAppState};
use zode_node_protocol::WorkspaceUri;

use crate::{
    stable_widget_id, RectExt, WidgetId, WorkspaceSnapshot, ZodeTheme, HIGH_CONTRAST_ID,
    REDUCED_MOTION_ID, THEME_DARK_ID, THEME_LIGHT_ID, THEME_SYSTEM_ID,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRow {
    pub tool: String,
    pub revoke_command: AppCommand,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub visible_rect: Rect,
    pub tool: String,
    pub revoke_command: AppCommand,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingControl {
    pub label: String,
    pub selected: bool,
    pub command: AppCommand,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingControlLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub visible_rect: Rect,
    pub control: SettingControl,
}

pub struct SettingsPanel;

impl SettingsPanel {
    pub fn active_workspace_uri(state: &ZodeAppState) -> Option<&WorkspaceUri> {
        state
            .active_available_workspace()
            .or_else(|| {
                state
                    .current_session
                    .as_ref()
                    .and_then(|session| state.available_workspace_for_session(session))
            })
            .or_else(|| {
                state
                    .projects
                    .iter()
                    .find(|project| project.available)
                    .map(|project| &project.workspace_uri)
            })
    }

    pub fn appearance_controls(state: &ZodeAppState) -> Vec<SettingControl> {
        vec![
            SettingControl {
                label: "跟随系统".into(),
                selected: state.ui_preferences.theme == ThemePreference::System,
                command: AppCommand::SetThemePreference(ThemePreference::System),
            },
            SettingControl {
                label: "浅色".into(),
                selected: state.ui_preferences.theme == ThemePreference::Light,
                command: AppCommand::SetThemePreference(ThemePreference::Light),
            },
            SettingControl {
                label: "深色".into(),
                selected: state.ui_preferences.theme == ThemePreference::Dark,
                command: AppCommand::SetThemePreference(ThemePreference::Dark),
            },
            SettingControl {
                label: "减少动画".into(),
                selected: state.ui_preferences.reduced_motion,
                command: AppCommand::SetReducedMotion(!state.ui_preferences.reduced_motion),
            },
            SettingControl {
                label: "高对比度".into(),
                selected: state.ui_preferences.high_contrast,
                command: AppCommand::SetHighContrast(!state.ui_preferences.high_contrast),
            },
        ]
    }

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

    pub fn permission_widget_id(workspace_uri: &WorkspaceUri, tool: &str) -> WidgetId {
        stable_widget_id(0x50, &(workspace_uri, tool))
    }

    pub fn appearance_control_layout(
        content: Rect,
        state: &ZodeAppState,
    ) -> Vec<SettingControlLayout> {
        let row_count = Self::permission_rows_for_active_workspace(state).len();
        let offset = Self::scroll_offset(content, state, row_count);
        [
            THEME_SYSTEM_ID,
            THEME_LIGHT_ID,
            THEME_DARK_ID,
            REDUCED_MOTION_ID,
            HIGH_CONTRAST_ID,
        ]
        .into_iter()
        .zip(Self::appearance_controls(state))
        .enumerate()
        .filter_map(|(index, (id, control))| {
            let rect = Rect::xywh(
                content.origin.x,
                content.origin.y + 128.0 + index as f32 * 36.0 - offset,
                content.size.x,
                34.0,
            );
            Some(SettingControlLayout {
                id,
                rect,
                visible_rect: clip_to_viewport(rect, content)?,
                control,
            })
        })
        .collect()
    }

    /// Returns the exact revoke-button geometry consumed by paint, hit testing,
    /// keyboard navigation, and accessibility.
    pub fn permission_row_layout(
        content: Rect,
        state: &ZodeAppState,
        workspace_uri: &WorkspaceUri,
    ) -> Vec<PermissionRowLayout> {
        let rows = Self::permission_rows(state, workspace_uri);
        let offset = Self::scroll_offset(content, state, rows.len());
        let card = Self::permission_card_rect(content, rows.len(), offset);
        rows.into_iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let rect = Rect::xywh(
                    card.max_x() - 82.0,
                    card.origin.y + 48.0 + index as f32 * 38.0,
                    64.0,
                    28.0,
                );
                Some(PermissionRowLayout {
                    id: Self::permission_widget_id(workspace_uri, &row.tool),
                    rect,
                    visible_rect: clip_to_viewport(rect, content)?,
                    tool: row.tool,
                    revoke_command: row.revoke_command,
                })
            })
            .collect()
    }

    pub fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
        state
            .project_permissions
            .iter()
            .find_map(|(workspace, tools)| {
                tools.iter().find_map(|tool| {
                    (Self::permission_widget_id(workspace, tool) == id).then(|| {
                        AppCommand::RevokeProjectPermission {
                            workspace_uri: workspace.clone(),
                            tool: tool.clone(),
                        }
                    })
                })
            })
    }

    pub fn max_scroll_offset(content: Rect, state: &ZodeAppState) -> f32 {
        let row_count = Self::permission_rows_for_active_workspace(state).len();
        (settings_content_height(row_count) - content.size.y).max(0.0)
    }

    pub fn scroll_command(content: Rect, state: &ZodeAppState, delta: f32) -> AppCommand {
        let max_offset = Self::max_scroll_offset(content, state);
        let current = state.settings_scroll_offset.clamp(0.0, max_offset);
        AppCommand::SetSettingsScroll {
            offset: (current + delta).clamp(0.0, max_offset),
        }
    }

    fn permission_rows_for_active_workspace(state: &ZodeAppState) -> Vec<PermissionRow> {
        Self::active_workspace_uri(state)
            .map(|workspace| Self::permission_rows(state, workspace))
            .unwrap_or_default()
    }

    fn scroll_offset(content: Rect, state: &ZodeAppState, row_count: usize) -> f32 {
        let max_offset = (settings_content_height(row_count) - content.size.y).max(0.0);
        state.settings_scroll_offset.clamp(0.0, max_offset)
    }

    fn permission_card_rect(content: Rect, row_count: usize, offset: f32) -> Rect {
        let height = (96.0 + row_count as f32 * 38.0).max(128.0);
        Rect::xywh(
            content.origin.x,
            content.origin.y + 360.0 - offset,
            content.size.x,
            height,
        )
    }

    pub fn paint_page(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        workspace_uri: Option<&WorkspaceUri>,
        theme: &ZodeTheme,
    ) {
        let layout = &snapshot.layout;
        paint_category_rail(painter, layout.sidebar, theme);
        let content = layout.transcript;
        let permission_rows = workspace_uri
            .map(|workspace| Self::permission_rows(state, workspace))
            .unwrap_or_default();
        let offset = Self::scroll_offset(content, state, permission_rows.len());
        painter.save();
        painter.clip_rect(content);
        draw_text(
            painter,
            "外观",
            Point2D::new(content.origin.x, content.origin.y + 42.0 - offset),
            24.0,
            650,
            theme.tokens.foreground,
        );

        let appearance_card = Rect::xywh(
            content.origin.x,
            content.origin.y + 70.0 - offset,
            content.size.x,
            260.0,
        );
        paint_card(painter, appearance_card, theme);
        draw_text(
            painter,
            "主题与动态效果",
            Point2D::new(
                appearance_card.origin.x + 18.0,
                appearance_card.origin.y + 28.0,
            ),
            14.0,
            600,
            theme.tokens.foreground,
        );
        for layout in Self::appearance_control_layout(content, state) {
            let control_rect = layout.rect;
            draw_text(
                painter,
                &layout.control.label,
                Point2D::new(control_rect.origin.x + 18.0, control_rect.origin.y + 20.0),
                13.0,
                500,
                theme.tokens.foreground,
            );
            let indicator = Rect::xywh(
                control_rect.max_x() - 48.0,
                control_rect.origin.y + 5.0,
                30.0,
                18.0,
            );
            painter.fill_round_rect(
                indicator,
                9.0,
                if layout.control.selected {
                    theme.zode_purple
                } else {
                    theme.tokens.muted
                },
            );
        }

        let permission_card = Self::permission_card_rect(content, permission_rows.len(), offset);
        paint_card(painter, permission_card, theme);
        draw_text(
            painter,
            "项目权限",
            Point2D::new(
                permission_card.origin.x + 18.0,
                permission_card.origin.y + 30.0,
            ),
            14.0,
            600,
            theme.tokens.foreground,
        );
        if permission_rows.is_empty() {
            draw_text(
                painter,
                "无活动项目或已保存权限",
                Point2D::new(
                    permission_card.origin.x + 18.0,
                    permission_card.origin.y + 66.0,
                ),
                12.0,
                400,
                theme.tokens.muted_foreground,
            );
        } else {
            let layouts = workspace_uri
                .map(|workspace| Self::permission_row_layout(content, state, workspace))
                .unwrap_or_default();
            for layout in layouts {
                draw_text(
                    painter,
                    &layout.tool,
                    Point2D::new(permission_card.origin.x + 18.0, layout.rect.origin.y + 18.0),
                    12.0,
                    500,
                    theme.tokens.foreground,
                );
                painter.fill_round_rect(
                    layout.rect,
                    7.0,
                    theme.tokens.destructive.with_alpha(0.12),
                );
                draw_text(
                    painter,
                    "撤销",
                    Point2D::new(layout.rect.origin.x + 18.0, layout.rect.origin.y + 19.0),
                    11.0,
                    600,
                    theme.tokens.destructive,
                );
            }
        }
        painter.restore();
    }
}

fn settings_content_height(permission_count: usize) -> f32 {
    360.0 + (96.0 + permission_count as f32 * 38.0).max(128.0) + 24.0
}

fn clip_to_viewport(rect: Rect, viewport: Rect) -> Option<Rect> {
    let left = rect.origin.x.max(viewport.origin.x);
    let top = rect.origin.y.max(viewport.origin.y);
    let right = rect.max_x().min(viewport.max_x());
    let bottom = rect.max_y().min(viewport.max_y());
    (right > left && bottom > top).then(|| Rect::xywh(left, top, right - left, bottom - top))
}

fn paint_category_rail(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
    if rect.size.x <= 0.0 {
        return;
    }
    draw_text(
        painter,
        "设置",
        Point2D::new(rect.origin.x + 16.0, rect.origin.y + 72.0),
        18.0,
        650,
        theme.sidebar_foreground,
    );
    painter.fill_round_rect(
        Rect::xywh(
            rect.origin.x + 8.0,
            rect.origin.y + 138.0,
            rect.size.x - 16.0,
            34.0,
        ),
        10.0,
        theme.tokens.row_selected,
    );
    draw_text(
        painter,
        "外观",
        Point2D::new(rect.origin.x + 18.0, rect.origin.y + 160.0),
        13.0,
        600,
        theme.sidebar_foreground,
    );
    draw_text(
        painter,
        "项目权限",
        Point2D::new(rect.origin.x + 18.0, rect.origin.y + 202.0),
        13.0,
        500,
        theme.sidebar_foreground,
    );
}

fn paint_card(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, 12.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 12.0, theme.tokens.border, 1.0);
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
