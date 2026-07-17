//! `SettingsCategory::ComputerUse` page: live TCC permission status (with a
//! "打开系统设置" action), the `computer` tool group's enable switch, the
//! "Any App" blanket-allowlist switch, and the allowed-apps list itself.
//!
//! Scope note: the allowlist is config-backed here, but nothing in the
//! approval gate consults it yet (see
//! `zode_core::config::ComputerConfig::any_app`'s doc comment) - this page
//! only reads/writes config and reports live permission grant state.

use jian_core::text_input::TextInputState;
use jian_widgets::{components::input::Input, HorizontalAlign, Painter, Rect};
use zode_app_model::{
    AppCommand, ComputerPermissionKind, ComputerPermissionState, LoadState, ZodeAppState,
};

use super::row::{
    clip_to_viewport, paint_card, paint_divider, paint_heading, paint_section_label,
    paint_setting_row, setting_row, SettingRowLayout, SettingRowSpec, APPEARANCE_ROW_HEIGHT,
    SECTION_TOP,
};
use crate::{paint_single_line, stable_widget_id, RectExt, WidgetId, ZodeTheme};

const COMPUTER_TOOL_ENABLED_ID: WidgetId = WidgetId(8_700);
const COMPUTER_ANY_APP_ID: WidgetId = WidgetId(8_701);
const COMPUTER_ACCESSIBILITY_OPEN_ID: WidgetId = WidgetId(8_702);
const COMPUTER_SCREEN_RECORDING_OPEN_ID: WidgetId = WidgetId(8_703);
pub const COMPUTER_ALLOWED_APP_INPUT_ID: WidgetId = WidgetId(8_704);
pub const COMPUTER_ALLOWED_APP_ADD_ID: WidgetId = WidgetId(8_705);

const SECTION_LABEL_HEIGHT: f32 = 24.0;
const SECTION_LABEL_GAP: f32 = 10.0;
const SECTION_GAP: f32 = 34.0;
const PERMISSION_ROW_HEIGHT: f32 = 56.0;
const ALLOWED_APP_ROW_HEIGHT: f32 = 42.0;
const ALLOWED_APP_ADD_ROW_HEIGHT: f32 = 48.0;
const BOTTOM_GAP: f32 = 24.0;
const ACTION_BUTTON_SIZE: (f32, f32) = (108.0, 28.0);
const ADD_BUTTON_WIDTH: f32 = 72.0;

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionStatusRowLayout {
    pub kind: ComputerPermissionKind,
    pub rect: Rect,
    pub label: &'static str,
    pub status_text: &'static str,
    pub action_rect: Rect,
    pub action_id: WidgetId,
    pub actionable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AllowedAppRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub visible_rect: Option<Rect>,
    pub app: String,
    pub remove_rect: Rect,
    pub remove_command: AppCommand,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputerUseLayout {
    pub status_card: Option<Rect>,
    pub status_message: Option<String>,
    pub permission_section_label: Rect,
    pub permission_card: Rect,
    pub permission_rows: Vec<PermissionStatusRowLayout>,
    pub access_section_label: Rect,
    pub access_card: Rect,
    pub access_rows: Vec<SettingRowLayout>,
    pub allowed_apps_section_label: Rect,
    pub allowed_apps_card: Rect,
    pub allowed_app_rows: Vec<AllowedAppRowLayout>,
    pub allowed_app_input_rect: Rect,
    pub allowed_app_add_rect: Rect,
    pub content_height: f32,
}

fn permission_label(state: ComputerPermissionState) -> &'static str {
    match state {
        ComputerPermissionState::Granted => "已允许",
        ComputerPermissionState::NotGranted => "未允许",
        ComputerPermissionState::Unsupported => "当前平台不支持",
    }
}

pub(super) fn content_height(state: &ZodeAppState) -> f32 {
    layout(Rect::xywh(0.0, 0.0, 768.0, 0.0), state, 0.0).content_height
}

pub(super) fn layout(content: Rect, state: &ZodeAppState, offset: f32) -> ComputerUseLayout {
    let top = content.origin.y + SECTION_TOP - offset;
    let Some(snapshot) = state.local_settings.computer.ready() else {
        let message = match &state.local_settings.computer {
            LoadState::Loading => "正在读取电脑操控设置…",
            LoadState::Failed(error) => return failed_layout(content, top, error.clone()),
            LoadState::Idle | LoadState::Ready(_) => "尚未读取电脑操控设置。",
        };
        return empty_layout(content, top, message.to_owned());
    };

    let permission_section_label =
        Rect::xywh(content.origin.x, top, content.size.x, SECTION_LABEL_HEIGHT);
    let permission_card_top = permission_section_label.max_y() + SECTION_LABEL_GAP;
    let permission_card = Rect::xywh(
        content.origin.x,
        permission_card_top,
        content.size.x,
        PERMISSION_ROW_HEIGHT * 2.0,
    );
    let permission_rows = vec![
        permission_status_row(
            permission_card,
            0,
            ComputerPermissionKind::Accessibility,
            "系统控制权限",
            snapshot.accessibility,
            COMPUTER_ACCESSIBILITY_OPEN_ID,
        ),
        permission_status_row(
            permission_card,
            1,
            ComputerPermissionKind::ScreenRecording,
            "录屏权限",
            snapshot.screen_recording,
            COMPUTER_SCREEN_RECORDING_OPEN_ID,
        ),
    ];

    let access_section_label = Rect::xywh(
        content.origin.x,
        permission_card.max_y() + SECTION_GAP,
        content.size.x,
        SECTION_LABEL_HEIGHT,
    );
    let access_card_top = access_section_label.max_y() + SECTION_LABEL_GAP;
    let access_card = Rect::xywh(
        content.origin.x,
        access_card_top,
        content.size.x,
        APPEARANCE_ROW_HEIGHT * 2.0,
    );
    let access_rows = vec![
        setting_row(SettingRowSpec {
            id: COMPUTER_TOOL_ENABLED_ID,
            rect: Rect::xywh(
                access_card.origin.x,
                access_card.origin.y,
                access_card.size.x,
                APPEARANCE_ROW_HEIGHT,
            ),
            viewport: content,
            label: "电脑操控工具",
            value: if snapshot.tool_group_enabled {
                "打开".into()
            } else {
                "关闭".into()
            },
            toggled: Some(snapshot.tool_group_enabled),
            enabled: true,
            command: Some(AppCommand::SetComputerToolEnabled(
                !snapshot.tool_group_enabled,
            )),
        }),
        setting_row(SettingRowSpec {
            id: COMPUTER_ANY_APP_ID,
            rect: Rect::xywh(
                access_card.origin.x,
                access_card.origin.y + APPEARANCE_ROW_HEIGHT,
                access_card.size.x,
                APPEARANCE_ROW_HEIGHT,
            ),
            viewport: content,
            label: "允许所有应用（Any App）",
            value: if snapshot.any_app { "打开" } else { "关闭" }.into(),
            toggled: Some(snapshot.any_app),
            enabled: true,
            command: Some(AppCommand::SetComputerAnyApp(!snapshot.any_app)),
        }),
    ];

    let allowed_apps_section_label = Rect::xywh(
        content.origin.x,
        access_card.max_y() + SECTION_GAP,
        content.size.x,
        SECTION_LABEL_HEIGHT,
    );
    let list_top = allowed_apps_section_label.max_y() + SECTION_LABEL_GAP;
    // Reserve one extra row's worth of height for the "no apps yet" message
    // when the list is empty, so it doesn't overlap the add row painted
    // right below it.
    let empty_message_rows = usize::from(snapshot.allowed_apps.is_empty());
    let allowed_app_row_count = snapshot.allowed_apps.len() + empty_message_rows;
    let allowed_apps_card = Rect::xywh(
        content.origin.x,
        list_top,
        content.size.x,
        ALLOWED_APP_ROW_HEIGHT * allowed_app_row_count as f32 + ALLOWED_APP_ADD_ROW_HEIGHT,
    );
    let allowed_app_rows = snapshot
        .allowed_apps
        .iter()
        .enumerate()
        .map(|(index, app)| {
            let rect = Rect::xywh(
                allowed_apps_card.origin.x,
                allowed_apps_card.origin.y + index as f32 * ALLOWED_APP_ROW_HEIGHT,
                allowed_apps_card.size.x,
                ALLOWED_APP_ROW_HEIGHT,
            );
            AllowedAppRowLayout {
                id: stable_widget_id(0x63, app),
                visible_rect: clip_to_viewport(rect, content),
                remove_rect: Rect::xywh(
                    rect.max_x() - 82.0,
                    rect.origin.y + (rect.size.y - 28.0) / 2.0,
                    64.0,
                    28.0,
                ),
                remove_command: AppCommand::RemoveComputerAllowedApp(app.clone()),
                rect,
                app: app.clone(),
            }
        })
        .collect();
    let add_row_y =
        allowed_apps_card.origin.y + ALLOWED_APP_ROW_HEIGHT * allowed_app_row_count as f32;
    let allowed_app_add_rect = Rect::xywh(
        allowed_apps_card.max_x() - ADD_BUTTON_WIDTH - 12.0,
        add_row_y + (ALLOWED_APP_ADD_ROW_HEIGHT - 30.0) / 2.0,
        ADD_BUTTON_WIDTH,
        30.0,
    );
    let allowed_app_input_rect = Rect::xywh(
        allowed_apps_card.origin.x + 12.0,
        add_row_y + (ALLOWED_APP_ADD_ROW_HEIGHT - 30.0) / 2.0,
        (allowed_app_add_rect.origin.x - allowed_apps_card.origin.x - 24.0).max(0.0),
        30.0,
    );

    // `content_height()` always probes with `content.origin.y == 0` and
    // `offset == 0.0`, so `allowed_apps_card.max_y()` already equals the
    // total height above the gap.
    let content_height = allowed_apps_card.max_y() + BOTTOM_GAP;
    ComputerUseLayout {
        status_card: None,
        status_message: None,
        permission_section_label,
        permission_card,
        permission_rows,
        access_section_label,
        access_card,
        access_rows,
        allowed_apps_section_label,
        allowed_apps_card,
        allowed_app_rows,
        allowed_app_input_rect,
        allowed_app_add_rect,
        content_height,
    }
}

fn permission_status_row(
    card: Rect,
    index: usize,
    kind: ComputerPermissionKind,
    label: &'static str,
    state: ComputerPermissionState,
    action_id: WidgetId,
) -> PermissionStatusRowLayout {
    let rect = Rect::xywh(
        card.origin.x,
        card.origin.y + index as f32 * PERMISSION_ROW_HEIGHT,
        card.size.x,
        PERMISSION_ROW_HEIGHT,
    );
    let actionable = state == ComputerPermissionState::NotGranted;
    PermissionStatusRowLayout {
        kind,
        action_rect: Rect::xywh(
            rect.max_x() - ACTION_BUTTON_SIZE.0 - 18.0,
            rect.origin.y + (rect.size.y - ACTION_BUTTON_SIZE.1) / 2.0,
            ACTION_BUTTON_SIZE.0,
            ACTION_BUTTON_SIZE.1,
        ),
        action_id,
        actionable,
        rect,
        label,
        status_text: permission_label(state),
    }
}

fn empty_layout(content: Rect, top: f32, message: String) -> ComputerUseLayout {
    let card = Rect::xywh(content.origin.x, top, content.size.x, 92.0);
    ComputerUseLayout {
        status_card: Some(card),
        status_message: Some(message),
        permission_section_label: Rect::ZERO,
        permission_card: Rect::ZERO,
        permission_rows: Vec::new(),
        access_section_label: Rect::ZERO,
        access_card: Rect::ZERO,
        access_rows: Vec::new(),
        allowed_apps_section_label: Rect::ZERO,
        allowed_apps_card: Rect::ZERO,
        allowed_app_rows: Vec::new(),
        allowed_app_input_rect: Rect::ZERO,
        allowed_app_add_rect: Rect::ZERO,
        // `content_height()` always probes with `content.origin.y == 0`, so
        // `card.max_y()` already equals the total height above the gap.
        content_height: card.max_y() + BOTTOM_GAP,
    }
}

fn failed_layout(content: Rect, top: f32, error: String) -> ComputerUseLayout {
    empty_layout(content, top, format!("读取失败：{error}"))
}

pub(super) fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if !matches!(
        state.presentation.route,
        zode_app_model::ShellRoute::Settings(zode_app_model::SettingsCategory::ComputerUse)
    ) {
        return None;
    }
    let snapshot = state.local_settings.computer.ready()?;
    if id == COMPUTER_TOOL_ENABLED_ID {
        return Some(AppCommand::SetComputerToolEnabled(
            !snapshot.tool_group_enabled,
        ));
    }
    if id == COMPUTER_ANY_APP_ID {
        return Some(AppCommand::SetComputerAnyApp(!snapshot.any_app));
    }
    if id == COMPUTER_ACCESSIBILITY_OPEN_ID
        && snapshot.accessibility == ComputerPermissionState::NotGranted
    {
        return Some(AppCommand::OpenComputerUsePermissionSettings(
            ComputerPermissionKind::Accessibility,
        ));
    }
    if id == COMPUTER_SCREEN_RECORDING_OPEN_ID
        && snapshot.screen_recording == ComputerPermissionState::NotGranted
    {
        return Some(AppCommand::OpenComputerUsePermissionSettings(
            ComputerPermissionKind::ScreenRecording,
        ));
    }
    if id == COMPUTER_ALLOWED_APP_ADD_ID {
        return Some(AppCommand::AddComputerAllowedApp(
            state.computer_use.allowed_app_input.clone(),
        ));
    }
    snapshot
        .allowed_apps
        .iter()
        .find(|app| stable_widget_id(0x63, *app) == id)
        .map(|app| AppCommand::RemoveComputerAllowedApp(app.clone()))
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    content: Rect,
    layout: &ComputerUseLayout,
    state: &ZodeAppState,
    offset: f32,
    focused: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    paint_heading(
        painter,
        content,
        "电脑操控",
        "本机能力与访问范围",
        offset,
        theme,
    );
    if let (Some(card), Some(message)) = (layout.status_card, &layout.status_message) {
        paint_card(painter, card, theme);
        paint_single_line(
            painter,
            message,
            Rect::xywh(
                card.origin.x + 18.0,
                card.origin.y,
                (card.size.x - 36.0).max(0.0),
                card.size.y,
            ),
            12.0,
            450,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
        return;
    }

    paint_section_label(painter, layout.permission_section_label, "权限状态", theme);
    paint_card(painter, layout.permission_card, theme);
    for (index, row) in layout.permission_rows.iter().enumerate() {
        if index > 0 {
            paint_divider(
                painter,
                layout.permission_card,
                index as f32 * PERMISSION_ROW_HEIGHT,
                theme,
            );
        }
        paint_single_line(
            painter,
            row.label,
            Rect::xywh(
                row.rect.origin.x + 18.0,
                row.rect.origin.y + 8.0,
                (row.action_rect.origin.x - row.rect.origin.x - 30.0).max(0.0),
                20.0,
            ),
            13.0,
            550,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        paint_single_line(
            painter,
            row.status_text,
            Rect::xywh(
                row.rect.origin.x + 18.0,
                row.rect.origin.y + 30.0,
                (row.action_rect.origin.x - row.rect.origin.x - 30.0).max(0.0),
                18.0,
            ),
            12.0,
            400,
            if row.actionable {
                theme.tokens.destructive
            } else {
                theme.tokens.muted_foreground
            },
            HorizontalAlign::Start,
        );
        if row.actionable {
            painter.fill_round_rect(row.action_rect, 8.0, theme.tokens.accent.with_alpha(0.14));
            paint_single_line(
                painter,
                "打开系统设置",
                row.action_rect,
                11.0,
                600,
                theme.tokens.accent,
                HorizontalAlign::Center,
            );
        }
    }

    paint_section_label(
        painter,
        layout.access_section_label,
        "工具与访问范围",
        theme,
    );
    paint_card(painter, layout.access_card, theme);
    for (index, row) in layout.access_rows.iter().enumerate() {
        if index > 0 {
            paint_divider(
                painter,
                layout.access_card,
                index as f32 * APPEARANCE_ROW_HEIGHT,
                theme,
            );
        }
        paint_setting_row(painter, row, theme);
    }

    paint_section_label(
        painter,
        layout.allowed_apps_section_label,
        "允许操作的应用（Any App 关闭时生效）",
        theme,
    );
    paint_card(painter, layout.allowed_apps_card, theme);
    for (index, row) in layout.allowed_app_rows.iter().enumerate() {
        if index > 0 {
            paint_divider(
                painter,
                layout.allowed_apps_card,
                index as f32 * ALLOWED_APP_ROW_HEIGHT,
                theme,
            );
        }
        paint_single_line(
            painter,
            &row.app,
            Rect::xywh(
                row.rect.origin.x + 18.0,
                row.rect.origin.y,
                (row.remove_rect.origin.x - row.rect.origin.x - 30.0).max(0.0),
                row.rect.size.y,
            ),
            12.0,
            500,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        painter.fill_round_rect(
            row.remove_rect,
            7.0,
            theme.tokens.destructive.with_alpha(0.12),
        );
        paint_single_line(
            painter,
            "移除",
            row.remove_rect,
            11.0,
            600,
            theme.tokens.destructive,
            HorizontalAlign::Center,
        );
    }
    if layout.allowed_app_rows.is_empty() {
        paint_single_line(
            painter,
            "尚未添加任何应用；关闭 Any App 后，将仅允许此处列出的应用。",
            Rect::xywh(
                layout.allowed_apps_card.origin.x + 18.0,
                layout.allowed_apps_card.origin.y,
                (layout.allowed_apps_card.size.x - 36.0).max(0.0),
                ALLOWED_APP_ROW_HEIGHT,
            ),
            12.0,
            400,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }
    let input_state = TextInputState::with_text(state.computer_use.allowed_app_input.clone());
    Input {
        state: &input_state,
        placeholder: "应用名称，如 Notes",
        focused: focused == Some(COMPUTER_ALLOWED_APP_INPUT_ID),
        font_size: 13.0,
        now_ms: 0,
        icon_d: None,
    }
    .paint(painter, layout.allowed_app_input_rect, &theme.tokens);
    let add_enabled = !state.computer_use.allowed_app_input.trim().is_empty();
    painter.fill_round_rect(
        layout.allowed_app_add_rect,
        8.0,
        if add_enabled {
            theme.tokens.accent
        } else {
            theme.tokens.muted_foreground.with_alpha(0.2)
        },
    );
    paint_single_line(
        painter,
        "添加",
        layout.allowed_app_add_rect,
        12.0,
        600,
        if add_enabled {
            theme.tokens.accent_foreground
        } else {
            theme.tokens.muted_foreground
        },
        HorizontalAlign::Center,
    );
}
