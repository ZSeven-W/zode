//! Inline "添加插件" overlay on the Plugins tab: a git spec + optional ref
//! field, submit/cancel, and an installing/success/error status line. Opened
//! by [`super::INTEGRATIONS_ADD_PLUGIN_ID`]; the actual install runs on the
//! desktop app's endpoint command pump (see `zode-app-model::PluginAddState`
//! and `zode-app::command_bridge::plugin_market`), so this widget only ever
//! reflects state that already lives on `ZodeAppState`.

use jian_core::text_input::TextInputState;
use jian_widgets::{components::input::Input, HorizontalAlign, Painter, Rect};
use zode_app_model::{PluginAddStatus, ZodeAppState};

use crate::theme::paint_elevated_surface;
use crate::{paint_single_line, Button, ButtonVariant, WidgetId, ZodeTheme};

pub const PLUGIN_ADD_SPEC_INPUT_ID: WidgetId = WidgetId(301);
pub const PLUGIN_ADD_REFERENCE_INPUT_ID: WidgetId = WidgetId(302);
pub const PLUGIN_ADD_SUBMIT_ID: WidgetId = WidgetId(303);
pub const PLUGIN_ADD_CANCEL_ID: WidgetId = WidgetId(304);

const PANEL_W: f32 = 420.0;
const PANEL_H: f32 = 258.0;
const PAD: f32 = 20.0;
const FIELD_H: f32 = 34.0;
const BUTTON_H: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PluginAddFormLayout {
    pub panel: Rect,
    pub spec_input: Rect,
    pub reference_input: Rect,
    pub status: Rect,
    pub cancel: Rect,
    pub submit: Rect,
    pub submit_enabled: bool,
}

impl PluginAddFormLayout {
    pub fn new(content: Rect, state: &ZodeAppState) -> Self {
        let panel = Rect::xywh(
            content.origin.x + ((content.size.x - PANEL_W).max(0.0) / 2.0),
            content.origin.y + 24.0,
            PANEL_W.min(content.size.x.max(0.0)),
            PANEL_H,
        );
        let spec_input = Rect::xywh(
            panel.origin.x + PAD,
            panel.origin.y + 56.0,
            panel.size.x - PAD * 2.0,
            FIELD_H,
        );
        let reference_input = Rect::xywh(
            panel.origin.x + PAD,
            spec_input.origin.y + FIELD_H + 34.0,
            panel.size.x - PAD * 2.0,
            FIELD_H,
        );
        let status = Rect::xywh(
            panel.origin.x + PAD,
            reference_input.origin.y + FIELD_H + 8.0,
            panel.size.x - PAD * 2.0,
            32.0,
        );
        let button_y = panel.origin.y + panel.size.y - BUTTON_H - PAD;
        let submit = Rect::xywh(
            panel.origin.x + panel.size.x - PAD - 88.0,
            button_y,
            88.0,
            BUTTON_H,
        );
        let cancel = Rect::xywh(submit.origin.x - 12.0 - 72.0, button_y, 72.0, BUTTON_H);
        let submit_enabled = !state.presentation.plugin_add.spec.trim().is_empty()
            && state.presentation.plugin_add.status != PluginAddStatus::Installing;
        Self {
            panel,
            spec_input,
            reference_input,
            status,
            cancel,
            submit,
            submit_enabled,
        }
    }
}

pub fn paint(
    painter: &mut dyn Painter,
    scrim: Rect,
    layout: &PluginAddFormLayout,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    painter.fill_rect(scrim, theme.tokens.background.with_alpha(0.5));
    paint_elevated_surface(painter, layout.panel, 14.0, theme);
    painter.fill_round_rect(layout.panel, 14.0, theme.tokens.card);
    painter.stroke_round_rect(layout.panel, 14.0, theme.tokens.border, 1.0);

    paint_single_line(
        painter,
        "添加插件",
        Rect::xywh(
            layout.panel.origin.x + PAD,
            layout.panel.origin.y + 16.0,
            layout.panel.size.x - PAD * 2.0,
            24.0,
        ),
        16.0,
        650,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );

    let add = &state.presentation.plugin_add;
    let spec_state = TextInputState::with_text(add.spec.clone());
    Input {
        state: &spec_state,
        placeholder: "owner/repo 或完整 Git URL",
        focused: focused == Some(PLUGIN_ADD_SPEC_INPUT_ID),
        font_size: 13.0,
        now_ms: 0,
        icon_d: None,
    }
    .paint(painter, layout.spec_input, &theme.tokens);

    let reference_state = TextInputState::with_text(add.reference.clone());
    Input {
        state: &reference_state,
        placeholder: "分支 / 提交（可选）",
        focused: focused == Some(PLUGIN_ADD_REFERENCE_INPUT_ID),
        font_size: 13.0,
        now_ms: 0,
        icon_d: None,
    }
    .paint(painter, layout.reference_input, &theme.tokens);

    let (status_text, status_color) = match &add.status {
        PluginAddStatus::Idle => (String::new(), theme.tokens.muted_foreground),
        PluginAddStatus::Installing => ("正在安装…".to_owned(), theme.tokens.muted_foreground),
        PluginAddStatus::Failed(message) => (message.clone(), theme.tokens.destructive),
    };
    if !status_text.is_empty() {
        paint_single_line(
            painter,
            &status_text,
            layout.status,
            12.0,
            450,
            status_color,
            HorizontalAlign::Start,
        );
    }

    Button::paint(
        painter,
        layout.cancel,
        8.0,
        "取消",
        None,
        ButtonVariant::Secondary,
        false,
        &theme.tokens,
    );
    Button::paint(
        painter,
        layout.submit,
        8.0,
        if add.status == PluginAddStatus::Installing {
            "安装中…"
        } else {
            "安装"
        },
        None,
        ButtonVariant::Primary,
        !layout.submit_enabled,
        &theme.tokens,
    );
}
