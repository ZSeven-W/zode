use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::ZodeAppState;

use super::{
    permissions::{
        paint_presets, preset_layouts, runtime_effort, runtime_status, PermissionPresetLayout,
        PERMISSION_PRESET_HEIGHT,
    },
    row::{
        paint_card, paint_divider, paint_heading, paint_section_label, paint_setting_row,
        setting_row, SettingRowLayout, GENERAL_ROW_HEIGHT, SECTION_TOP,
    },
};
use crate::{paint_single_line, RectExt, WidgetId, ZodeTheme};

const GENERAL_SECTION_GAP: f32 = 52.0;
const GENERAL_ROW_COUNT: usize = 10;

pub(super) const fn content_height() -> f32 {
    SECTION_TOP
        + PERMISSION_PRESET_HEIGHT * 3.0
        + GENERAL_SECTION_GAP
        + GENERAL_ROW_HEIGHT * GENERAL_ROW_COUNT as f32
        + 24.0
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
        content.origin.y + 50.0 - offset,
        content.size.x * 0.58,
        24.0,
    );
    let permission_presets = preset_layouts(permission_card, content, state);
    let general_section_label = Rect::xywh(
        content.origin.x,
        permission_card.max_y() + 20.0,
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
    let descriptors: [(&'static str, String); GENERAL_ROW_COUNT] = [
        ("默认文件打开目标", "即将支持".into()),
        ("语言", "中文（中国）".into()),
        ("在菜单栏中显示", "即将支持".into()),
        ("底部面板", "即将支持".into()),
        ("默认终端位置", "即将支持".into()),
        ("运行时防止系统休眠", "即将支持".into()),
        ("速度", speed),
        ("建议提示", "即将支持".into()),
        ("从其他 AI 应用导入工作内容", "导入即将支持".into()),
        ("打开源许可证", "查看即将支持".into()),
    ];
    let general_rows = descriptors
        .into_iter()
        .enumerate()
        .map(|(index, (label, value))| {
            setting_row(
                WidgetId(8_300 + index as u64),
                Rect::xywh(
                    general_card.origin.x,
                    general_card.origin.y + index as f32 * GENERAL_ROW_HEIGHT,
                    general_card.size.x,
                    GENERAL_ROW_HEIGHT,
                ),
                content,
                label,
                value,
                false,
                None,
            )
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
