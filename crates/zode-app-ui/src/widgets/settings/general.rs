use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{AppCommand, ZodeAppState};

use super::{
    permissions::{
        paint_presets, preset_layouts, runtime_effort, runtime_status, PermissionPresetLayout,
        PERMISSION_PRESET_HEIGHT,
    },
    row::{
        paint_card, paint_divider, paint_heading, paint_section_label, paint_setting_row,
        setting_row, SettingRowLayout, SettingRowSpec, FIRST_SECTION_TOP, GENERAL_ROW_HEIGHT,
        SECTION_TOP,
    },
};
use crate::{paint_single_line, RectExt, WidgetId, ZodeTheme};

const GENERAL_SECTION_LABEL_GAP: f32 = 46.0;
const GENERAL_SECTION_GAP: f32 = 84.0;
const GENERAL_BOTTOM_GAP: f32 = 6.0;
const GENERAL_ROW_COUNT: usize = 10;
const SPEED_SETTING_ID: WidgetId = WidgetId(8_306);
const TASK_SUGGESTIONS_SETTING_ID: WidgetId = WidgetId(8_307);
const SIDEBAR_TASKS_SETTING_ID: WidgetId = WidgetId(8_302);

type GeneralRowDescriptor = (&'static str, String, Option<bool>, bool, Option<AppCommand>);

pub(super) const fn content_height() -> f32 {
    SECTION_TOP
        + PERMISSION_PRESET_HEIGHT * 3.0
        + GENERAL_SECTION_GAP
        + GENERAL_ROW_HEIGHT * GENERAL_ROW_COUNT as f32
        + GENERAL_BOTTOM_GAP
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneralSettingsLayout {
    pub permission_card: Rect,
    pub permission_status_rect: Rect,
    pub permission_presets: Vec<PermissionPresetLayout>,
    pub general_section_label: Rect,
    pub general_card: Rect,
    pub general_rows: Vec<SettingRowLayout>,
    pub content_height: f32,
}

pub(super) fn layout(content: Rect, state: &ZodeAppState, offset: f32) -> GeneralSettingsLayout {
    let permission_card = Rect::xywh(
        content.origin.x,
        content.origin.y + SECTION_TOP - offset,
        content.size.x,
        PERMISSION_PRESET_HEIGHT * 3.0,
    );
    let permission_status_rect = Rect::xywh(
        content.origin.x + content.size.x * 0.42,
        content.origin.y + FIRST_SECTION_TOP - offset,
        content.size.x * 0.58,
        24.0,
    );
    let permission_presets = preset_layouts(permission_card, content, state);
    let general_section_label = Rect::xywh(
        content.origin.x,
        permission_card.max_y() + GENERAL_SECTION_LABEL_GAP,
        content.size.x,
        24.0,
    );
    let general_card = Rect::xywh(
        content.origin.x,
        permission_card.max_y() + GENERAL_SECTION_GAP,
        content.size.x,
        GENERAL_ROW_HEIGHT * GENERAL_ROW_COUNT as f32,
    );
    let speed = runtime_effort(state).map_or_else(
        || "未加载".to_owned(),
        |effort| match effort.to_ascii_lowercase().as_str() {
            "minimal" | "low" => "低".to_owned(),
            "medium" | "standard" => "标准".to_owned(),
            "high" => "高".to_owned(),
            _ => effort.to_owned(),
        },
    );
    let speed_command = next_effort(state).map(|effort| AppCommand::SetEffort(effort.into()));
    let descriptors: [GeneralRowDescriptor; GENERAL_ROW_COUNT] = [
        ("默认文件打开目标", "即将支持".into(), None, false, None),
        ("语言", "即将支持".into(), None, false, None),
        (
            "侧边栏任务列表",
            if state.sidebar.tasks_expanded {
                "展开"
            } else {
                "折叠"
            }
            .into(),
            Some(state.sidebar.tasks_expanded),
            true,
            Some(AppCommand::SetSidebarTasksExpanded(
                !state.sidebar.tasks_expanded,
            )),
        ),
        ("底部面板", "即将支持".into(), None, false, None),
        ("默认终端位置", "即将支持".into(), None, false, None),
        ("运行时防止系统休眠", "即将支持".into(), None, false, None),
        ("速度", speed, None, speed_command.is_some(), speed_command),
        (
            "建议提示",
            if state.ui_preferences.task_suggestions {
                "开启"
            } else {
                "关闭"
            }
            .into(),
            Some(state.ui_preferences.task_suggestions),
            true,
            Some(AppCommand::SetTaskSuggestions(
                !state.ui_preferences.task_suggestions,
            )),
        ),
        (
            "从其他 AI 应用导入工作内容",
            "导入即将支持".into(),
            None,
            false,
            None,
        ),
        ("打开源许可证", "查看即将支持".into(), None, false, None),
    ];
    let general_rows = descriptors
        .into_iter()
        .enumerate()
        .map(|(index, (label, value, toggled, enabled, command))| {
            setting_row(SettingRowSpec {
                id: WidgetId(8_300 + index as u64),
                rect: Rect::xywh(
                    general_card.origin.x,
                    general_card.origin.y + index as f32 * GENERAL_ROW_HEIGHT,
                    general_card.size.x,
                    GENERAL_ROW_HEIGHT,
                ),
                viewport: content,
                label,
                value,
                toggled,
                enabled,
                command,
            })
        })
        .collect();
    let content_height = content_height();
    GeneralSettingsLayout {
        permission_card,
        permission_status_rect,
        permission_presets,
        general_section_label,
        general_card,
        general_rows,
        content_height,
    }
}

pub(super) fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if id == TASK_SUGGESTIONS_SETTING_ID {
        return Some(AppCommand::SetTaskSuggestions(
            !state.ui_preferences.task_suggestions,
        ));
    }
    if id == SIDEBAR_TASKS_SETTING_ID {
        return Some(AppCommand::SetSidebarTasksExpanded(
            !state.sidebar.tasks_expanded,
        ));
    }
    (id == SPEED_SETTING_ID)
        .then(|| next_effort(state))
        .flatten()
        .map(|effort| AppCommand::SetEffort(effort.into()))
}

fn next_effort(state: &ZodeAppState) -> Option<&'static str> {
    if !super::permissions::runtime_options_available(state)
        || !super::permissions::current_session_is_idle(state)
    {
        return None;
    }
    Some(match runtime_effort(state)?.to_ascii_lowercase().as_str() {
        "minimal" | "low" => "medium",
        "medium" | "standard" => "high",
        "high" => "low",
        _ => "medium",
    })
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    content: Rect,
    layout: &GeneralSettingsLayout,
    state: &ZodeAppState,
    offset: f32,
    theme: &ZodeTheme,
) {
    paint_heading(painter, content, "常规", "权限", offset, theme);
    let status = runtime_status(state);
    if !status.is_empty() {
        paint_single_line(
            painter,
            status,
            layout.permission_status_rect,
            11.0,
            450,
            theme.tokens.muted_foreground,
            HorizontalAlign::End,
        );
    }
    paint_presets(
        painter,
        layout.permission_card,
        &layout.permission_presets,
        theme,
    );
    paint_section_label(painter, layout.general_section_label, "常规", theme);
    paint_card(painter, layout.general_card, theme);
    for (index, row) in layout.general_rows.iter().enumerate() {
        if index > 0 {
            paint_divider(
                painter,
                layout.general_card,
                index as f32 * GENERAL_ROW_HEIGHT,
                theme,
            );
        }
        paint_setting_row(painter, row, theme);
    }
}
