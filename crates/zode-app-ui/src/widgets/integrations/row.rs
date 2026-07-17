use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{
    Availability, IntegrationCategory, IntegrationEntry, IntegrationInstallState,
    IntegrationMutationState, ZodeAppState,
};

use crate::{paint_single_line, stable_widget_id, WidgetId, ZodeTheme};

#[derive(Debug, Clone, PartialEq)]
pub struct IntegrationRowLayout {
    pub id: WidgetId,
    pub source_id: String,
    pub name: String,
    pub description: String,
    pub monogram: String,
    pub status: &'static str,
    pub rect: Rect,
    pub action_id: WidgetId,
    pub action_rect: Rect,
    pub action_label: &'static str,
    pub action_enabled: bool,
    pub action_reason: Option<&'static str>,
}

impl IntegrationRowLayout {
    pub(super) fn new(entry: &IntegrationEntry, rect: Rect, state: &ZodeAppState) -> Option<Self> {
        let source_id = entry.source_id.clone()?;
        let action = action_state(entry, state);
        let action_width = 64.0_f32.min((rect.size.x * 0.24).max(0.0));
        Some(Self {
            id: widget_id(&source_id),
            action_id: action_widget_id(&source_id),
            source_id,
            name: entry.name.clone(),
            description: entry.description.clone(),
            monogram: entry.icon.label().to_owned(),
            status: status_label(entry),
            rect,
            action_rect: Rect::xywh(
                rect.origin.x + (rect.size.x - action_width - 8.0).max(0.0),
                rect.origin.y + (rect.size.y - 24.0) / 2.0,
                action_width,
                24.0,
            ),
            action_label: action.label,
            action_enabled: action.enabled,
            action_reason: action.reason,
        })
    }
}

pub(super) fn widget_id(source_id: &str) -> WidgetId {
    stable_widget_id(0x72, source_id)
}

pub(super) fn action_widget_id(source_id: &str) -> WidgetId {
    stable_widget_id(0x74, source_id)
}

pub(super) struct RowActionState {
    pub label: &'static str,
    pub enabled: bool,
    pub reason: Option<&'static str>,
}

pub(super) fn action_state(entry: &IntegrationEntry, state: &ZodeAppState) -> RowActionState {
    if let IntegrationMutationState::Updating { source_id, .. } =
        &state.presentation.integration_mutation
    {
        return RowActionState {
            label: if entry.source_id.as_ref() == Some(source_id) {
                "更新中"
            } else {
                "请稍候"
            },
            enabled: false,
            reason: Some("另一项集成状态正在保存"),
        };
    }
    if entry.install_state == IntegrationInstallState::Available {
        return RowActionState {
            label: "安装",
            enabled: false,
            reason: Some("当前节点没有可验证的集成安装 API"),
        };
    }
    if entry.category == IntegrationCategory::Capabilities {
        return RowActionState {
            label: "节点提供",
            enabled: false,
            reason: Some("节点能力不能在插件页修改"),
        };
    }
    let busy = state.current_session.as_ref().is_some_and(|session| {
        state
            .available_workspace_for_session(session)
            .is_some_and(|workspace| Some(workspace) == state.active_available_workspace())
            && (state.active_turns.contains_key(session)
                || state
                    .transcripts
                    .get(session)
                    .is_some_and(|transcript| transcript.busy))
    });
    RowActionState {
        label: if entry.availability == Availability::Disabled {
            "启用"
        } else {
            "停用"
        },
        enabled: !busy,
        reason: busy.then_some("当前任务运行中，请先停止"),
    }
}

fn status_label(entry: &IntegrationEntry) -> &'static str {
    if entry.availability == Availability::Disabled {
        "已停用"
    } else {
        entry.install_state.label()
    }
}

pub(super) fn paint(painter: &mut dyn Painter, row: &IntegrationRowLayout, theme: &ZodeTheme) {
    painter.save();
    painter.clip_round_rect(row.rect, 10.0);
    painter.fill_round_rect(row.rect, 10.0, theme.tokens.card);
    painter.stroke_round_rect(row.rect, 10.0, theme.tokens.border, 1.0);

    let icon_size = row.rect.size.y.clamp(0.0, 36.0);
    let icon = Rect::xywh(
        row.rect.origin.x + 10.0,
        row.rect.origin.y + (row.rect.size.y - icon_size) / 2.0,
        icon_size,
        icon_size,
    );
    painter.fill_round_rect(icon, 9.0, theme.tokens.muted);
    paint_single_line(
        painter,
        &row.monogram,
        icon,
        if row.monogram.chars().count() > 1 {
            11.0
        } else {
            14.0
        },
        650,
        theme.tokens.foreground,
        HorizontalAlign::Center,
    );

    let status_width = 58.0_f32.min((row.rect.size.x * 0.22).max(0.0));
    let status = Rect::xywh(
        (row.action_rect.origin.x - status_width - 6.0).max(row.rect.origin.x),
        row.rect.origin.y + (row.rect.size.y - 24.0) / 2.0,
        status_width,
        24.0,
    );
    painter.fill_round_rect(status, 12.0, theme.tokens.muted);
    paint_single_line(
        painter,
        row.status,
        status,
        11.0,
        500,
        if matches!(row.status, "已安装" | "已内置" | "已配置") {
            theme.success
        } else {
            theme.tokens.muted_foreground
        },
        HorizontalAlign::Center,
    );

    let text_x = icon.origin.x + icon.size.x + 10.0;
    let text_width = (status.origin.x - text_x - 8.0).max(0.0);
    paint_single_line(
        painter,
        &row.name,
        Rect::xywh(text_x, row.rect.origin.y + 8.0, text_width, 19.0),
        13.0,
        600,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        &row.description,
        Rect::xywh(text_x, row.rect.origin.y + 28.0, text_width, 16.0),
        11.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    painter.fill_round_rect(row.action_rect, 12.0, theme.tokens.muted);
    painter.stroke_round_rect(row.action_rect, 12.0, theme.tokens.border, 1.0);
    paint_single_line(
        painter,
        row.action_label,
        row.action_rect,
        11.0,
        550,
        if row.action_enabled {
            theme.tokens.foreground
        } else {
            theme.tokens.muted_foreground.with_alpha(0.64)
        },
        HorizontalAlign::Center,
    );
    painter.restore();
}
