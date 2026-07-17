use jian_widgets::{components::radio::Radio, HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, LoadState, ZodeAppState};
use zode_node_protocol::{RuntimeOptions, SandboxMode, WorkspaceUri};

use super::row::{clip_to_viewport, paint_card, paint_divider, paint_heading};
use crate::{paint_single_line, stable_widget_id, RectExt, WidgetId, ZodeTheme};

pub(super) const PERMISSION_PRESET_HEIGHT: f32 = 72.0;
pub(super) const PERMISSION_WORKSPACE_HEIGHT: f32 = 52.0;
pub(super) const PERMISSION_ROW_HEIGHT: f32 = 42.0;

const SANDBOX_READ_ONLY_ID: WidgetId = WidgetId(8_201);
const SANDBOX_WORKSPACE_WRITE_ID: WidgetId = WidgetId(8_202);
const SANDBOX_OFF_ID: WidgetId = WidgetId(8_203);

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionPresetLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub visible_rect: Option<Rect>,
    pub label_rect: Rect,
    pub description_rect: Rect,
    pub control_rect: Rect,
    pub mode: SandboxMode,
    pub label: &'static str,
    pub description: &'static str,
    pub selected: bool,
    pub enabled: bool,
    pub command: Option<AppCommand>,
}

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

pub(super) fn preset_layouts(
    card: Rect,
    viewport: Rect,
    state: &ZodeAppState,
) -> Vec<PermissionPresetLayout> {
    let runtime = active_runtime_options(state);
    let enabled = runtime.is_some() && current_session_is_idle(state);
    [
        (
            SANDBOX_READ_ONLY_ID,
            SandboxMode::ReadOnly,
            "只读",
            "仅允许读取工作区文件",
        ),
        (
            SANDBOX_WORKSPACE_WRITE_ID,
            SandboxMode::WorkspaceWrite,
            "工作区写入",
            "允许读取并编辑当前工作区",
        ),
        (
            SANDBOX_OFF_ID,
            SandboxMode::Off,
            "完全访问",
            "关闭沙箱限制（高风险）",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (id, mode, label, description))| {
        let rect = Rect::xywh(
            card.origin.x,
            card.origin.y + index as f32 * PERMISSION_PRESET_HEIGHT,
            card.size.x,
            PERMISSION_PRESET_HEIGHT,
        );
        let control_rect = Rect::xywh(
            rect.max_x() - 38.0,
            rect.origin.y + (rect.size.y - 18.0) / 2.0,
            18.0,
            18.0,
        );
        let text_width = (control_rect.origin.x - rect.origin.x - 40.0).max(0.0);
        PermissionPresetLayout {
            id,
            rect,
            visible_rect: clip_to_viewport(rect, viewport),
            label_rect: Rect::xywh(rect.origin.x + 18.0, rect.origin.y + 8.0, text_width, 23.0),
            description_rect: Rect::xywh(
                rect.origin.x + 18.0,
                rect.origin.y + 31.0,
                text_width,
                23.0,
            ),
            control_rect,
            mode,
            label,
            description,
            selected: runtime.is_some_and(|runtime| runtime.sandbox_mode == mode),
            enabled,
            command: enabled.then(|| AppCommand::SetSandbox {
                mode,
                network: runtime.is_some_and(|runtime| runtime.sandbox_network),
            }),
        }
    })
    .collect()
}

pub(super) fn paint_presets(
    painter: &mut dyn Painter,
    card: Rect,
    layouts: &[PermissionPresetLayout],
    theme: &ZodeTheme,
) {
    paint_card(painter, card, theme);
    for (index, layout) in layouts.iter().enumerate() {
        if index > 0 {
            paint_divider(
                painter,
                card,
                index as f32 * PERMISSION_PRESET_HEIGHT,
                theme,
            );
        }
        let label_color = if layout.enabled {
            theme.tokens.foreground
        } else {
            theme.tokens.muted_foreground.with_alpha(0.75)
        };
        paint_single_line(
            painter,
            layout.label,
            layout.label_rect,
            13.0,
            600,
            label_color,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            layout.description,
            layout.description_rect,
            11.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        Radio {
            selected: layout.selected,
            enabled: layout.enabled,
        }
        .paint(painter, layout.control_rect, &theme.tokens);
    }
}

pub(super) fn command_for_preset(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if !matches!(
        state.presentation.route,
        zode_app_model::ShellRoute::Settings(zode_app_model::SettingsCategory::General)
    ) {
        return None;
    }
    let mode = match id {
        SANDBOX_READ_ONLY_ID => SandboxMode::ReadOnly,
        SANDBOX_WORKSPACE_WRITE_ID => SandboxMode::WorkspaceWrite,
        SANDBOX_OFF_ID => SandboxMode::Off,
        _ => return None,
    };
    let runtime = active_runtime_options(state)?;
    current_session_is_idle(state).then_some(AppCommand::SetSandbox {
        mode,
        network: runtime.sandbox_network,
    })
}

pub(super) fn runtime_effort(state: &ZodeAppState) -> Option<&str> {
    active_runtime_options(state).and_then(|runtime| runtime.effort.as_deref())
}

pub(super) fn runtime_status(state: &ZodeAppState) -> &'static str {
    match state
        .current_session_presentation()
        .map(|presentation| &presentation.runtime_options)
    {
        Some(LoadState::Ready(_)) if current_session_is_idle(state) => "",
        Some(LoadState::Ready(_)) => "任务运行时不可更改",
        Some(LoadState::Loading) => "正在加载运行时权限",
        Some(LoadState::Failed(_)) => "运行时权限不可用",
        Some(LoadState::Idle) | None => "选择任务后加载运行时权限",
    }
}

fn active_runtime_options(state: &ZodeAppState) -> Option<&RuntimeOptions> {
    state
        .current_session_presentation()?
        .runtime_options
        .ready()
}

pub(super) fn runtime_options_available(state: &ZodeAppState) -> bool {
    active_runtime_options(state).is_some()
}

pub(super) fn current_session_is_idle(state: &ZodeAppState) -> bool {
    state
        .current_session
        .as_ref()
        .is_some_and(|session| !state.active_turns.contains_key(session))
}

pub(super) fn permission_rows(
    state: &ZodeAppState,
    workspace_uri: &WorkspaceUri,
) -> Vec<PermissionRow> {
    state
        .project_permissions
        .get(workspace_uri)
        .and_then(LoadState::ready)
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

pub(super) fn permission_widget_id(workspace_uri: &WorkspaceUri, tool: &str) -> WidgetId {
    stable_widget_id(0x50, &(workspace_uri, tool))
}

pub(super) fn permission_card_rect(content: Rect, row_count: usize, offset: f32) -> Rect {
    Rect::xywh(
        content.origin.x,
        content.origin.y + super::row::SECTION_TOP - offset,
        content.size.x,
        (PERMISSION_WORKSPACE_HEIGHT + row_count as f32 * PERMISSION_ROW_HEIGHT).max(104.0),
    )
}

pub(super) fn permission_row_layout(
    content: Rect,
    state: &ZodeAppState,
    workspace_uri: &WorkspaceUri,
) -> Vec<PermissionRowLayout> {
    let rows = permission_rows(state, workspace_uri);
    let offset = super::scroll_offset(content, state);
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
                id: permission_widget_id(workspace_uri, &row.tool),
                rect,
                visible_rect: clip_to_viewport(rect, content)?,
                tool: row.tool,
                revoke_command: row.revoke_command,
            })
        })
        .collect()
}

pub(super) fn paint_permission_page(
    painter: &mut dyn Painter,
    content: Rect,
    state: &ZodeAppState,
    workspace_uri: Option<&WorkspaceUri>,
    offset: f32,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, "权限", "项目权限", offset, theme);
    let rows = workspace_uri
        .map(|workspace| permission_rows(state, workspace))
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
    paint_single_line(
        painter,
        workspace_uri.map(WorkspaceUri::as_str).unwrap_or("未选择"),
        Rect::xywh(
            (workspace_row.origin.x + 150.0).min(workspace_row.max_x()),
            workspace_row.origin.y,
            (workspace_row.size.x - 168.0).max(0.0),
            workspace_row.size.y,
        ),
        12.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::End,
    );
    paint_divider(painter, card, PERMISSION_WORKSPACE_HEIGHT, theme);
    if rows.is_empty() {
        let empty_message = workspace_uri.map_or("未选择工作区", |workspace| {
            match state.project_permissions.get(workspace) {
                Some(LoadState::Loading) => "正在加载项目权限",
                Some(LoadState::Failed(_)) => "项目权限不可用",
                Some(LoadState::Idle) | None => "项目权限尚未加载",
                Some(LoadState::Ready(_)) => "未保存项目权限",
            }
        });
        paint_single_line(
            painter,
            empty_message,
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
        .map(|workspace| permission_row_layout(content, state, workspace))
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
