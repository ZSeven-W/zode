mod legacy;
mod rail;

use jian_widgets::{HorizontalAlign, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    AppCommand, ConnectionState, SettingsCategory, ShellPage, ShellRoute, ThemePreference,
    ZodeAppState,
};
use zode_node_protocol::WorkspaceUri;

use crate::{
    paint_single_line, stable_widget_id, RectExt, WidgetId, WorkspaceSnapshot, ZodeTheme,
    HIGH_CONTRAST_ID, REDUCED_MOTION_ID, THEME_DARK_ID, THEME_LIGHT_ID, THEME_SYSTEM_ID,
};

const CONTENT_WIDTH: f32 = 768.0;
const CONTENT_TOP: f32 = 70.0;
const CARD_TOP: f32 = 84.0;
const GENERAL_ROW_HEIGHT: f32 = 52.0;
const APPEARANCE_ROW_HEIGHT: f32 = 44.0;
const PERMISSION_WORKSPACE_HEIGHT: f32 = 52.0;
const PERMISSION_ROW_HEIGHT: f32 = 42.0;
const SETTINGS_GENERAL_CATEGORY_ID: WidgetId = WidgetId(80);
const SETTINGS_APPEARANCE_CATEGORY_ID: WidgetId = WidgetId(81);
const SETTINGS_PERMISSIONS_CATEGORY_ID: WidgetId = WidgetId(82);
const SETTINGS_KEYBOARD_SHORTCUTS_CATEGORY_ID: WidgetId = WidgetId(83);
const SETTINGS_ENVIRONMENT_CATEGORY_ID: WidgetId = WidgetId(84);

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
    pub fn page_layout(primary_surface: Rect) -> (Rect, Rect) {
        let horizontal_gutter = 32.0_f32.min(primary_surface.size.x);
        let width = CONTENT_WIDTH.min((primary_surface.size.x - horizontal_gutter).max(0.0));
        let x = primary_surface.origin.x + (primary_surface.size.x - width).max(0.0) / 2.0;
        let y = (primary_surface.origin.y + CONTENT_TOP).min(primary_surface.max_y());
        let content = Rect::xywh(x, y, width, (primary_surface.max_y() - y).max(0.0));
        (
            content,
            Rect::xywh(
                content.origin.x,
                content.origin.y + CARD_TOP,
                content.size.x,
                GENERAL_ROW_HEIGHT * 3.0,
            ),
        )
    }

    pub fn active_category(state: &ZodeAppState) -> SettingsCategory {
        match state.presentation.route {
            ShellRoute::Settings(category) => category,
            _ if state.shell.page == ShellPage::Settings => SettingsCategory::Appearance,
            _ => SettingsCategory::General,
        }
    }

    pub const fn category_widget_id(category: SettingsCategory) -> WidgetId {
        match category {
            SettingsCategory::General => SETTINGS_GENERAL_CATEGORY_ID,
            SettingsCategory::Appearance => SETTINGS_APPEARANCE_CATEGORY_ID,
            SettingsCategory::Permissions => SETTINGS_PERMISSIONS_CATEGORY_ID,
            SettingsCategory::KeyboardShortcuts => SETTINGS_KEYBOARD_SHORTCUTS_CATEGORY_ID,
            SettingsCategory::Environment => SETTINGS_ENVIRONMENT_CATEGORY_ID,
        }
    }

    /// Returns stable category geometry as
    /// `(id, rect, category, label, selected, available)` tuples.
    pub fn category_rows(
        rect: Rect,
        state: &ZodeAppState,
    ) -> Vec<(WidgetId, Rect, SettingsCategory, &'static str, bool, bool)> {
        let active = Self::active_category(state);
        [
            (SettingsCategory::General, "常规", true),
            (SettingsCategory::Appearance, "外观", true),
            (SettingsCategory::Permissions, "权限", true),
            (SettingsCategory::KeyboardShortcuts, "键盘快捷键", false),
            (SettingsCategory::Environment, "环境", false),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (category, label, available))| {
            (
                Self::category_widget_id(category),
                Rect::xywh(
                    rect.origin.x + 8.0,
                    rect.origin.y + 154.0 + index as f32 * 30.0,
                    (rect.size.x - 16.0).max(0.0),
                    30.0,
                ),
                category,
                label,
                category == active,
                available,
            )
        })
        .collect()
    }

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
        if Self::active_category(state) != SettingsCategory::Appearance {
            return Vec::new();
        }
        let offset = Self::scroll_offset(content, state);
        let card = appearance_card_rect(content, offset);
        [
            THEME_SYSTEM_ID,
            THEME_LIGHT_ID,
            THEME_DARK_ID,
            REDUCED_MOTION_ID,
            HIGH_CONTRAST_ID,
        ]
        .into_iter()
        .zip(Self::appearance_controls(state))
        .filter_map(|(id, control)| {
            let index = match id {
                THEME_SYSTEM_ID => 0,
                THEME_LIGHT_ID => 1,
                THEME_DARK_ID => 2,
                REDUCED_MOTION_ID => 3,
                HIGH_CONTRAST_ID => 4,
                _ => return None,
            };
            let rect = Rect::xywh(
                card.origin.x,
                card.origin.y + index as f32 * APPEARANCE_ROW_HEIGHT,
                card.size.x,
                APPEARANCE_ROW_HEIGHT,
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
        if Self::active_category(state) != SettingsCategory::Permissions {
            return Vec::new();
        }
        let rows = Self::permission_rows(state, workspace_uri);
        let offset = Self::scroll_offset(content, state);
        let card = permission_card_rect(content, rows.len(), offset);
        rows.into_iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let rect = Rect::xywh(
                    card.max_x() - 82.0,
                    card.origin.y
                        + PERMISSION_WORKSPACE_HEIGHT
                        + index as f32 * PERMISSION_ROW_HEIGHT
                        + 7.0,
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
        category_command_for_widget(id).or_else(|| {
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
        })
    }

    pub fn max_scroll_offset(content: Rect, state: &ZodeAppState) -> f32 {
        (settings_content_height(state) - content.size.y).max(0.0)
    }

    pub fn scroll_command(content: Rect, state: &ZodeAppState, delta: f32) -> AppCommand {
        let max_offset = Self::max_scroll_offset(content, state);
        let current = state.settings_scroll_offset.clamp(0.0, max_offset);
        AppCommand::SetSettingsScroll {
            offset: (current + delta).clamp(0.0, max_offset),
        }
    }

    fn scroll_offset(content: Rect, state: &ZodeAppState) -> f32 {
        let max_offset = Self::max_scroll_offset(content, state);
        state.settings_scroll_offset.clamp(0.0, max_offset)
    }

    pub fn paint_page(
        painter: &mut dyn Painter,
        snapshot: &WorkspaceSnapshot,
        state: &ZodeAppState,
        workspace_uri: Option<&WorkspaceUri>,
        theme: &ZodeTheme,
    ) {
        rail::paint(painter, snapshot.layout.sidebar, state, theme);
        let (content, _) = Self::page_layout(snapshot.layout.primary_surface);
        let offset = Self::scroll_offset(content, state);
        painter.save();
        painter.clip_rect(content);
        if !matches!(state.presentation.route, ShellRoute::Settings(_)) {
            legacy::paint(painter, content, state, workspace_uri, offset, theme);
            painter.restore();
            return;
        }
        match Self::active_category(state) {
            SettingsCategory::General => paint_general(painter, content, state, offset, theme),
            SettingsCategory::Appearance => {
                paint_appearance(painter, content, state, offset, theme)
            }
            SettingsCategory::Permissions => {
                paint_permissions(painter, content, state, workspace_uri, offset, theme)
            }
            SettingsCategory::KeyboardShortcuts => {
                paint_placeholder(painter, content, "键盘快捷键", "快捷键设置即将支持", theme)
            }
            SettingsCategory::Environment => {
                paint_placeholder(painter, content, "环境", "环境设置即将支持", theme)
            }
        }
        painter.restore();
    }
}

fn paint_general(
    painter: &mut dyn Painter,
    content: Rect,
    state: &ZodeAppState,
    offset: f32,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, "常规", "本地运行状态", offset, theme);
    let card = Rect::xywh(
        content.origin.x,
        content.origin.y + CARD_TOP - offset,
        content.size.x,
        GENERAL_ROW_HEIGHT * 3.0,
    );
    paint_card(painter, card, theme);
    let connection = match state.host.connection {
        ConnectionState::Local => "本地",
        ConnectionState::Connecting => "连接中",
        ConnectionState::Unavailable => "不可用",
    };
    let workspace = selected_workspace_uri(state)
        .map(WorkspaceUri::as_str)
        .unwrap_or("未选择");
    let capability_count = format!("{} 项", state.host.capabilities.capabilities.len());
    for (index, (label, value)) in [
        ("主机连接", connection),
        ("活动工作区", workspace),
        ("可用能力", capability_count.as_str()),
    ]
    .into_iter()
    .enumerate()
    {
        paint_value_row(painter, card, index, label, value, theme);
    }
}

fn paint_appearance(
    painter: &mut dyn Painter,
    content: Rect,
    state: &ZodeAppState,
    offset: f32,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, "外观", "主题与动态效果", offset, theme);
    let card = appearance_card_rect(content, offset);
    paint_card(painter, card, theme);
    let controls = SettingsPanel::appearance_control_layout(content, state);
    for (index, layout) in controls.into_iter().enumerate() {
        if index > 0 {
            paint_divider(painter, card, index as f32 * APPEARANCE_ROW_HEIGHT, theme);
        }
        paint_single_line(
            painter,
            &layout.control.label,
            Rect::xywh(
                layout.rect.origin.x + 18.0,
                layout.rect.origin.y,
                (layout.rect.size.x - 84.0).max(0.0),
                layout.rect.size.y,
            ),
            13.0,
            500,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        let indicator = Rect::xywh(
            layout.rect.max_x() - 48.0,
            layout.rect.origin.y + 13.0,
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
}

fn paint_permissions(
    painter: &mut dyn Painter,
    content: Rect,
    state: &ZodeAppState,
    workspace_uri: Option<&WorkspaceUri>,
    offset: f32,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, "权限", "项目权限", offset, theme);
    let rows = workspace_uri
        .map(|workspace| SettingsPanel::permission_rows(state, workspace))
        .unwrap_or_default();
    let card = permission_card_rect(content, rows.len(), offset);
    paint_card(painter, card, theme);
    let workspace_row = Rect::xywh(
        card.origin.x,
        card.origin.y,
        card.size.x,
        PERMISSION_WORKSPACE_HEIGHT,
    );
    paint_single_line(
        painter,
        "活动工作区",
        Rect::xywh(
            workspace_row.origin.x + 18.0,
            workspace_row.origin.y,
            (workspace_row.size.x - 36.0).max(0.0),
            workspace_row.size.y,
        ),
        13.0,
        500,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    draw_right_aligned(
        painter,
        workspace_uri.map(WorkspaceUri::as_str).unwrap_or("未选择"),
        workspace_row,
        theme,
    );
    paint_divider(painter, card, PERMISSION_WORKSPACE_HEIGHT, theme);

    if rows.is_empty() {
        paint_single_line(
            painter,
            if workspace_uri.is_some() {
                "未保存项目权限"
            } else {
                "未选择工作区"
            },
            Rect::xywh(
                card.origin.x + 18.0,
                card.origin.y + PERMISSION_WORKSPACE_HEIGHT,
                (card.size.x - 36.0).max(0.0),
                (card.size.y - PERMISSION_WORKSPACE_HEIGHT).max(0.0),
            ),
            12.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        return;
    }

    let layouts = workspace_uri
        .map(|workspace| SettingsPanel::permission_row_layout(content, state, workspace))
        .unwrap_or_default();
    for (index, layout) in layouts.into_iter().enumerate() {
        if index > 0 {
            paint_divider(
                painter,
                card,
                PERMISSION_WORKSPACE_HEIGHT + index as f32 * PERMISSION_ROW_HEIGHT,
                theme,
            );
        }
        let row_y =
            card.origin.y + PERMISSION_WORKSPACE_HEIGHT + index as f32 * PERMISSION_ROW_HEIGHT;
        let row = Rect::xywh(card.origin.x, row_y, card.size.x, PERMISSION_ROW_HEIGHT);
        paint_single_line(
            painter,
            &layout.tool,
            Rect::xywh(
                row.origin.x + 18.0,
                row.origin.y,
                (layout.rect.origin.x - row.origin.x - 30.0).max(0.0),
                row.size.y,
            ),
            12.0,
            500,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        painter.fill_round_rect(layout.rect, 7.0, theme.tokens.destructive.with_alpha(0.12));
        paint_single_line(
            painter,
            "撤销",
            layout.rect,
            11.0,
            600,
            theme.tokens.destructive,
            HorizontalAlign::Center,
        );
    }
}

fn paint_placeholder(
    painter: &mut dyn Painter,
    content: Rect,
    title: &str,
    message: &str,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, title, "即将支持", 0.0, theme);
    let card = Rect::xywh(
        content.origin.x,
        content.origin.y + CARD_TOP,
        content.size.x,
        136.0,
    );
    paint_card(painter, card, theme);
    draw_text(
        painter,
        message,
        Point2D::new(card.origin.x + 18.0, card.origin.y + 48.0),
        14.0,
        600,
        theme.tokens.foreground,
    );
    draw_text(
        painter,
        "接入真实本地状态后，这里会提供对应设置。",
        Point2D::new(card.origin.x + 18.0, card.origin.y + 76.0),
        12.0,
        400,
        theme.tokens.muted_foreground,
    );
}

fn paint_heading(
    painter: &mut dyn Painter,
    content: Rect,
    title: &str,
    section: &str,
    offset: f32,
    theme: &ZodeTheme,
) {
    draw_text(
        painter,
        title,
        Point2D::new(content.origin.x, content.origin.y + 32.0 - offset),
        24.0,
        650,
        theme.tokens.foreground,
    );
    draw_text(
        painter,
        section,
        Point2D::new(content.origin.x, content.origin.y + 66.0 - offset),
        13.0,
        600,
        theme.tokens.foreground,
    );
}

fn paint_value_row(
    painter: &mut dyn Painter,
    card: Rect,
    index: usize,
    label: &str,
    value: &str,
    theme: &ZodeTheme,
) {
    if index > 0 {
        paint_divider(painter, card, index as f32 * GENERAL_ROW_HEIGHT, theme);
    }
    let row = Rect::xywh(
        card.origin.x,
        card.origin.y + index as f32 * GENERAL_ROW_HEIGHT,
        card.size.x,
        GENERAL_ROW_HEIGHT,
    );
    paint_single_line(
        painter,
        label,
        Rect::xywh(
            row.origin.x + 18.0,
            row.origin.y,
            (row.size.x - 36.0).max(0.0),
            row.size.y,
        ),
        13.0,
        500,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    draw_right_aligned(painter, value, row, theme);
}

fn draw_right_aligned(painter: &mut dyn Painter, value: &str, row: Rect, theme: &ZodeTheme) {
    paint_single_line(
        painter,
        value,
        Rect::xywh(
            (row.origin.x + 150.0).min(row.max_x()),
            row.origin.y,
            (row.size.x - 168.0).max(0.0),
            row.size.y,
        ),
        12.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::End,
    );
}

fn paint_card(painter: &mut dyn Painter, rect: Rect, theme: &ZodeTheme) {
    painter.fill_round_rect(rect, 12.0, theme.tokens.card);
    painter.stroke_round_rect(rect, 12.0, theme.tokens.border, 1.0);
}

fn paint_divider(painter: &mut dyn Painter, card: Rect, y_offset: f32, theme: &ZodeTheme) {
    let y = card.origin.y + y_offset;
    painter.stroke_line(
        Point2D::new(card.origin.x + 16.0, y),
        Point2D::new(card.max_x() - 16.0, y),
        theme.tokens.border,
        1.0,
    );
}

fn appearance_card_rect(content: Rect, offset: f32) -> Rect {
    Rect::xywh(
        content.origin.x,
        content.origin.y + CARD_TOP - offset,
        content.size.x,
        APPEARANCE_ROW_HEIGHT * 5.0,
    )
}

fn permission_card_rect(content: Rect, row_count: usize, offset: f32) -> Rect {
    Rect::xywh(
        content.origin.x,
        content.origin.y + CARD_TOP - offset,
        content.size.x,
        (PERMISSION_WORKSPACE_HEIGHT + row_count as f32 * PERMISSION_ROW_HEIGHT).max(104.0),
    )
}

fn settings_content_height(state: &ZodeAppState) -> f32 {
    let card_height = match SettingsPanel::active_category(state) {
        SettingsCategory::General => GENERAL_ROW_HEIGHT * 3.0,
        SettingsCategory::Appearance => APPEARANCE_ROW_HEIGHT * 5.0,
        SettingsCategory::Permissions => {
            let count = SettingsPanel::active_workspace_uri(state)
                .map(|workspace| SettingsPanel::permission_rows(state, workspace).len())
                .unwrap_or_default();
            (PERMISSION_WORKSPACE_HEIGHT + count as f32 * PERMISSION_ROW_HEIGHT).max(104.0)
        }
        SettingsCategory::KeyboardShortcuts | SettingsCategory::Environment => 136.0,
    };
    CARD_TOP + card_height + 24.0
}

const fn category_command_for_widget(id: WidgetId) -> Option<AppCommand> {
    let category = match id {
        SETTINGS_GENERAL_CATEGORY_ID => SettingsCategory::General,
        SETTINGS_APPEARANCE_CATEGORY_ID => SettingsCategory::Appearance,
        SETTINGS_PERMISSIONS_CATEGORY_ID => SettingsCategory::Permissions,
        SETTINGS_KEYBOARD_SHORTCUTS_CATEGORY_ID => SettingsCategory::KeyboardShortcuts,
        SETTINGS_ENVIRONMENT_CATEGORY_ID => SettingsCategory::Environment,
        _ => return None,
    };
    Some(AppCommand::SelectSettingsCategory(category))
}

fn selected_workspace_uri(state: &ZodeAppState) -> Option<&WorkspaceUri> {
    state.active_available_workspace().or_else(|| {
        state
            .current_session
            .as_ref()
            .and_then(|session| state.available_workspace_for_session(session))
    })
}

fn clip_to_viewport(rect: Rect, viewport: Rect) -> Option<Rect> {
    let left = rect.origin.x.max(viewport.origin.x);
    let top = rect.origin.y.max(viewport.origin.y);
    let right = rect.max_x().min(viewport.max_x());
    let bottom = rect.max_y().min(viewport.max_y());
    (right > left && bottom > top).then(|| Rect::xywh(left, top, right - left, bottom - top))
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
