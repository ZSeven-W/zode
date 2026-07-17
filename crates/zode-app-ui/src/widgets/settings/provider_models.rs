use jian_widgets::{HorizontalAlign, Painter, Rect};
use zode_app_model::{
    AppCommand, LoadState, ProviderDraft, ProviderKindChoice, ProviderModelsStatus,
    ProviderSummary, ZodeAppState,
};

use super::row::{paint_card, paint_divider, paint_section_label};
use crate::{paint_single_line, stable_widget_id, RectExt, SemanticIcon, WidgetId, ZodeTheme};

mod paint_helpers;

use paint_helpers::{kind_label, paint_button, paint_input, status_label};

pub const PROVIDER_ID_INPUT_ID: WidgetId = WidgetId(8_601);
pub const PROVIDER_KIND_ANTHROPIC_ID: WidgetId = WidgetId(8_602);
pub const PROVIDER_KIND_OPENAI_ID: WidgetId = WidgetId(8_603);
pub const PROVIDER_KIND_OLLAMA_ID: WidgetId = WidgetId(8_604);
pub const PROVIDER_BASE_URL_INPUT_ID: WidgetId = WidgetId(8_605);
pub const PROVIDER_API_KEY_INPUT_ID: WidgetId = WidgetId(8_606);
pub const PROVIDER_MODEL_IDS_INPUT_ID: WidgetId = WidgetId(8_607);
pub const PROVIDER_DEFAULT_MODEL_INPUT_ID: WidgetId = WidgetId(8_608);
pub const PROVIDER_ADD_ID: WidgetId = WidgetId(8_609);
pub const PROVIDER_SAVE_ID: WidgetId = WidgetId(8_610);
pub const PROVIDER_CANCEL_ID: WidgetId = WidgetId(8_611);
const PROVIDER_KIND_FIELD_ID: WidgetId = WidgetId(8_612);

const HEADER_HEIGHT: f32 = 70.0;
const SECTION_LABEL_HEIGHT: f32 = 28.0;
const PROVIDER_ROW_HEIGHT: f32 = 60.0;
const EMPTY_HEIGHT: f32 = 92.0;
const FIELD_ROW_HEIGHT: f32 = 56.0;
const MODEL_ROW_HEIGHT: f32 = 40.0;
const SECTION_GAP: f32 = 32.0;
const BOTTOM_GAP: f32 = 28.0;
/// Width of the 编辑/删除 row-action buttons. `paint_button` reserves 38px for
/// an icon plus padding, so the label area is `ROW_BUTTON_WIDTH - 38`; two CJK
/// glyphs at 12px need ~24px, so this leaves comfortable slack instead of the
/// old 52px (14px of label space) that clipped both labels.
const ROW_BUTTON_WIDTH: f32 = 72.0;
const ROW_BUTTON_HEIGHT: f32 = 28.0;
const ROW_BUTTON_GAP: f32 = 10.0;
const ROW_BUTTON_MARGIN: f32 = 16.0;

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRowLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub edit_rect: Rect,
    pub remove_id: WidgetId,
    pub remove_rect: Rect,
    pub summary: ProviderSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderFieldLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: &'static str,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderKindLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub label: &'static str,
    pub kind: ProviderKindChoice,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderModelLayout {
    pub id: WidgetId,
    pub rect: Rect,
    pub model_id: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderEditorLayout {
    pub section_label: Rect,
    pub fields_card: Rect,
    pub fields: Vec<ProviderFieldLayout>,
    pub kinds: Vec<ProviderKindLayout>,
    pub model_section_label: Rect,
    pub models_card: Option<Rect>,
    pub models: Vec<ProviderModelLayout>,
    pub save_rect: Rect,
    pub cancel_rect: Rect,
    pub status_rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderModelsLayout {
    pub title: Rect,
    pub subtitle: Rect,
    pub list_section_label: Rect,
    pub add_rect: Rect,
    pub providers_card: Rect,
    pub providers: Vec<ProviderRowLayout>,
    pub empty_message: Option<&'static str>,
    pub editor: Option<ProviderEditorLayout>,
}

pub fn provider_widget_id(provider_id: &str) -> WidgetId {
    stable_widget_id(0xB0, provider_id)
}

pub fn provider_remove_widget_id(provider_id: &str) -> WidgetId {
    stable_widget_id(0xB1, provider_id)
}

pub fn provider_model_widget_id(model_id: &str) -> WidgetId {
    stable_widget_id(0xB2, model_id)
}

pub(super) fn content_height(state: &ZodeAppState) -> f32 {
    let provider_count = state
        .provider_models
        .load
        .ready()
        .map(Vec::len)
        .unwrap_or_default();
    let list_height = if provider_count == 0 {
        EMPTY_HEIGHT
    } else {
        provider_count as f32 * PROVIDER_ROW_HEIGHT
    };
    let mut height = HEADER_HEIGHT + SECTION_LABEL_HEIGHT + list_height + BOTTOM_GAP;
    if let Some(draft) = state.provider_models.editor.as_ref() {
        height += SECTION_GAP
            + SECTION_LABEL_HEIGHT
            + FIELD_ROW_HEIGHT * 5.0
            + SECTION_GAP
            + SECTION_LABEL_HEIGHT
            + (draft.model_ids.len().max(1) as f32 * MODEL_ROW_HEIGHT)
            + 58.0;
    }
    height
}

pub(super) fn layout(content: Rect, state: &ZodeAppState, offset: f32) -> ProviderModelsLayout {
    let title = Rect::xywh(
        content.origin.x,
        content.origin.y - 8.0 - offset,
        content.size.x,
        36.0,
    );
    let subtitle = Rect::xywh(
        content.origin.x,
        content.origin.y + 28.0 - offset,
        content.size.x,
        22.0,
    );
    let list_section_label = Rect::xywh(
        content.origin.x,
        content.origin.y + HEADER_HEIGHT - offset,
        content.size.x,
        SECTION_LABEL_HEIGHT,
    );
    let add_rect = Rect::xywh(
        content.max_x() - 132.0,
        list_section_label.origin.y - 4.0,
        132.0,
        32.0,
    );
    let providers = state
        .provider_models
        .load
        .ready()
        .cloned()
        .unwrap_or_default();
    let providers_height = if providers.is_empty() {
        EMPTY_HEIGHT
    } else {
        providers.len() as f32 * PROVIDER_ROW_HEIGHT
    };
    let providers_card = Rect::xywh(
        content.origin.x,
        list_section_label.max_y() + 10.0,
        content.size.x,
        providers_height,
    );
    let provider_rows = providers
        .into_iter()
        .enumerate()
        .map(|(index, summary)| provider_row(providers_card, index, summary))
        .collect::<Vec<_>>();
    let empty_message = match &state.provider_models.load {
        LoadState::Idle => Some("正在等待读取 Provider 配置。"),
        LoadState::Loading => Some("正在读取 Provider 与模型…"),
        LoadState::Failed(_) => Some("无法读取 Provider 配置，请稍后重试。"),
        LoadState::Ready(providers) if providers.is_empty() => {
            Some("尚未配置 Provider。添加后即可在会话中选择模型。")
        }
        LoadState::Ready(_) => None,
    };
    let editor =
        state.provider_models.editor.as_ref().map(|draft| {
            editor_layout(content, providers_card.max_y() + SECTION_GAP, draft, state)
        });
    ProviderModelsLayout {
        title,
        subtitle,
        list_section_label,
        add_rect,
        providers_card,
        providers: provider_rows,
        empty_message,
        editor,
    }
}

fn provider_row(card: Rect, index: usize, summary: ProviderSummary) -> ProviderRowLayout {
    let rect = Rect::xywh(
        card.origin.x,
        card.origin.y + index as f32 * PROVIDER_ROW_HEIGHT,
        card.size.x,
        PROVIDER_ROW_HEIGHT,
    );
    let remove_rect = Rect::xywh(
        rect.max_x() - ROW_BUTTON_MARGIN - ROW_BUTTON_WIDTH,
        rect.origin.y + 16.0,
        ROW_BUTTON_WIDTH,
        ROW_BUTTON_HEIGHT,
    );
    let edit_rect = Rect::xywh(
        remove_rect.origin.x - ROW_BUTTON_GAP - ROW_BUTTON_WIDTH,
        remove_rect.origin.y,
        ROW_BUTTON_WIDTH,
        ROW_BUTTON_HEIGHT,
    );
    ProviderRowLayout {
        id: provider_widget_id(&summary.provider_id),
        rect,
        edit_rect,
        remove_id: provider_remove_widget_id(&summary.provider_id),
        remove_rect,
        summary,
    }
}

fn editor_layout(
    viewport: Rect,
    y: f32,
    draft: &ProviderDraft,
    state: &ZodeAppState,
) -> ProviderEditorLayout {
    let section_label = Rect::xywh(viewport.origin.x, y, viewport.size.x, SECTION_LABEL_HEIGHT);
    let fields_card = Rect::xywh(
        viewport.origin.x,
        section_label.max_y() + 10.0,
        viewport.size.x,
        FIELD_ROW_HEIGHT * 5.0,
    );
    let specs = [
        (
            PROVIDER_ID_INPUT_ID,
            "Provider ID",
            draft.provider_id.clone(),
        ),
        (PROVIDER_KIND_FIELD_ID, "类型", String::new()),
        (
            PROVIDER_BASE_URL_INPUT_ID,
            "Base URL",
            draft.base_url.clone().unwrap_or_default(),
        ),
        (
            PROVIDER_API_KEY_INPUT_ID,
            "API Key",
            if state.provider_models.credential_configured {
                "已配置".into()
            } else {
                "输入密钥".into()
            },
        ),
        (
            PROVIDER_MODEL_IDS_INPUT_ID,
            "模型",
            draft.model_ids.join(", "),
        ),
    ];
    let fields = specs
        .into_iter()
        .enumerate()
        .map(|(index, (id, label, value))| {
            let row = Rect::xywh(
                fields_card.origin.x,
                fields_card.origin.y + index as f32 * FIELD_ROW_HEIGHT,
                fields_card.size.x,
                FIELD_ROW_HEIGHT,
            );
            ProviderFieldLayout {
                id,
                rect: Rect::xywh(
                    row.origin.x + 178.0,
                    row.origin.y + 10.0,
                    (row.size.x - 196.0).max(0.0),
                    36.0,
                ),
                label,
                value,
            }
        })
        .collect::<Vec<_>>();
    let kind_field = &fields[1];
    let kind_width = kind_field.rect.size.x / 3.0;
    let kinds = [
        (
            PROVIDER_KIND_ANTHROPIC_ID,
            "Anthropic",
            ProviderKindChoice::Anthropic,
        ),
        (
            PROVIDER_KIND_OPENAI_ID,
            "OpenAI",
            ProviderKindChoice::OpenAi,
        ),
        (
            PROVIDER_KIND_OLLAMA_ID,
            "Ollama",
            ProviderKindChoice::Ollama,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (id, label, kind))| {
        let rect = Rect::xywh(
            kind_field.rect.origin.x + index as f32 * kind_width,
            kind_field.rect.origin.y,
            kind_width,
            kind_field.rect.size.y,
        );
        ProviderKindLayout {
            id,
            rect,
            label,
            kind,
            selected: draft.kind == kind,
        }
    })
    .collect();
    let model_section_label = Rect::xywh(
        viewport.origin.x,
        fields_card.max_y() + SECTION_GAP,
        viewport.size.x,
        SECTION_LABEL_HEIGHT,
    );
    let models_card = (!draft.model_ids.is_empty()).then(|| {
        Rect::xywh(
            viewport.origin.x,
            model_section_label.max_y() + 10.0,
            viewport.size.x,
            draft.model_ids.len() as f32 * MODEL_ROW_HEIGHT,
        )
    });
    let models = models_card
        .map(|card| {
            draft
                .model_ids
                .iter()
                .enumerate()
                .map(|(index, model_id)| {
                    let rect = Rect::xywh(
                        card.origin.x,
                        card.origin.y + index as f32 * MODEL_ROW_HEIGHT,
                        card.size.x,
                        MODEL_ROW_HEIGHT,
                    );
                    ProviderModelLayout {
                        id: provider_model_widget_id(model_id),
                        rect,
                        model_id: model_id.clone(),
                        selected: draft.default_model.as_ref() == Some(model_id),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let actions_y = models_card
        .map(RectExt::max_y)
        .unwrap_or_else(|| model_section_label.max_y() + 10.0)
        + 14.0;
    let save_rect = Rect::xywh(viewport.max_x() - 92.0, actions_y, 92.0, 36.0);
    let cancel_rect = Rect::xywh(save_rect.origin.x - 104.0, actions_y, 92.0, 36.0);
    ProviderEditorLayout {
        section_label,
        fields_card,
        fields,
        kinds,
        model_section_label,
        models_card,
        models,
        save_rect,
        cancel_rect,
        status_rect: Rect::xywh(
            viewport.origin.x,
            actions_y,
            (cancel_rect.origin.x - viewport.origin.x - 16.0).max(0.0),
            36.0,
        ),
    }
}

pub(super) fn command_for_widget(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if id == PROVIDER_ADD_ID {
        return Some(AppCommand::BeginProviderEdit {
            draft: ProviderDraft {
                kind: ProviderKindChoice::Anthropic,
                ..ProviderDraft::default()
            },
            credential_configured: false,
        });
    }
    if id == PROVIDER_SAVE_ID {
        return (!state.provider_models.status.is_busy())
            .then_some(AppCommand::RequestSaveProvider);
    }
    if id == PROVIDER_CANCEL_ID {
        return (!state.provider_models.status.is_busy()).then_some(AppCommand::CancelProviderEdit);
    }
    for (candidate, kind) in [
        (PROVIDER_KIND_ANTHROPIC_ID, ProviderKindChoice::Anthropic),
        (PROVIDER_KIND_OPENAI_ID, ProviderKindChoice::OpenAi),
        (PROVIDER_KIND_OLLAMA_ID, ProviderKindChoice::Ollama),
    ] {
        if id == candidate {
            return Some(AppCommand::SetProviderDraftKind(kind));
        }
    }
    let providers = state.provider_models.load.ready()?;
    if let Some(summary) = providers
        .iter()
        .find(|summary| provider_widget_id(&summary.provider_id) == id)
    {
        return Some(AppCommand::BeginProviderEdit {
            draft: ProviderDraft::from(summary),
            credential_configured: summary.credential_configured,
        });
    }
    if let Some(summary) = providers
        .iter()
        .find(|summary| provider_remove_widget_id(&summary.provider_id) == id)
    {
        return Some(AppCommand::RequestRemoveProvider {
            provider_id: summary.provider_id.clone(),
        });
    }
    let draft = state.provider_models.editor.as_ref()?;
    draft.model_ids.iter().find_map(|model_id| {
        (provider_model_widget_id(model_id) == id)
            .then(|| AppCommand::SelectProviderDefaultModel(model_id.clone()))
    })
}

pub(super) fn paint(
    painter: &mut dyn Painter,
    content: Rect,
    state: &ZodeAppState,
    offset: f32,
    focused: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    let layout = layout(content, state, offset);
    paint_single_line(
        painter,
        "Provider 与模型",
        layout.title,
        24.0,
        650,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    paint_single_line(
        painter,
        "连接模型服务，并管理每个会话可选择的模型。",
        layout.subtitle,
        13.0,
        400,
        theme.tokens.muted_foreground,
        HorizontalAlign::Start,
    );
    paint_section_label(painter, layout.list_section_label, "已配置", theme);
    paint_button(
        painter,
        layout.add_rect,
        "添加 Provider",
        SemanticIcon::Plus,
        false,
        false,
        theme,
    );
    paint_card(painter, layout.providers_card, theme);
    if let Some(message) = layout.empty_message {
        paint_single_line(
            painter,
            message,
            Rect::xywh(
                layout.providers_card.origin.x + 18.0,
                layout.providers_card.origin.y,
                (layout.providers_card.size.x - 36.0).max(0.0),
                layout.providers_card.size.y,
            ),
            12.0,
            450,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }
    for (index, row) in layout.providers.iter().enumerate() {
        if index > 0 {
            paint_divider(
                painter,
                layout.providers_card,
                index as f32 * PROVIDER_ROW_HEIGHT,
                theme,
            );
        }
        paint_provider_row(painter, row, theme);
    }
    if let (Some(editor), Some(draft)) = (
        layout.editor.as_ref(),
        state.provider_models.editor.as_ref(),
    ) {
        paint_editor(painter, editor, draft, state, focused, theme);
    }
}

fn paint_provider_row(painter: &mut dyn Painter, row: &ProviderRowLayout, theme: &ZodeTheme) {
    let text_width = (row.edit_rect.origin.x - row.rect.origin.x - 24.0).max(0.0);
    paint_single_line(
        painter,
        &row.summary.provider_id,
        Rect::xywh(
            row.rect.origin.x + 18.0,
            row.rect.origin.y + 7.0,
            text_width,
            22.0,
        ),
        13.0,
        600,
        theme.tokens.foreground,
        HorizontalAlign::Start,
    );
    let default_model = row
        .summary
        .default_model
        .as_deref()
        .unwrap_or("未选择默认模型");
    let detail = format!(
        "{} · {} 个模型 · {} · {}",
        kind_label(row.summary.kind),
        row.summary.models.len(),
        if row.summary.credential_configured {
            "凭据已配置"
        } else {
            "凭据未配置"
        },
        default_model,
    );
    paint_single_line(
        painter,
        &detail,
        Rect::xywh(
            row.rect.origin.x + 18.0,
            row.rect.origin.y + 29.0,
            text_width,
            20.0,
        ),
        11.0,
        400,
        if row.summary.credential_configured {
            theme.tokens.muted_foreground
        } else {
            theme.warning
        },
        HorizontalAlign::Start,
    );
    paint_button(
        painter,
        row.edit_rect,
        "编辑",
        SemanticIcon::Edit,
        false,
        false,
        theme,
    );
    paint_button(
        painter,
        row.remove_rect,
        "删除",
        SemanticIcon::Delete,
        true,
        false,
        theme,
    );
}

fn paint_editor(
    painter: &mut dyn Painter,
    layout: &ProviderEditorLayout,
    draft: &ProviderDraft,
    state: &ZodeAppState,
    focused: Option<WidgetId>,
    theme: &ZodeTheme,
) {
    paint_section_label(
        painter,
        layout.section_label,
        if draft.provider_id.is_empty() {
            "添加 Provider"
        } else {
            "编辑 Provider"
        },
        theme,
    );
    paint_card(painter, layout.fields_card, theme);
    for (index, field) in layout.fields.iter().enumerate() {
        if index > 0 {
            paint_divider(
                painter,
                layout.fields_card,
                index as f32 * FIELD_ROW_HEIGHT,
                theme,
            );
        }
        let row_y = layout.fields_card.origin.y + index as f32 * FIELD_ROW_HEIGHT;
        paint_single_line(
            painter,
            field.label,
            Rect::xywh(
                layout.fields_card.origin.x + 18.0,
                row_y,
                148.0,
                FIELD_ROW_HEIGHT,
            ),
            13.0,
            550,
            theme.tokens.foreground,
            HorizontalAlign::Start,
        );
        if field.id != PROVIDER_KIND_FIELD_ID {
            paint_input(
                painter,
                field.rect,
                &field.value,
                field.id == PROVIDER_API_KEY_INPUT_ID,
                focused == Some(field.id),
                state.provider_models.credential_configured,
                state.provider_models.pending_secret_len,
                theme,
            );
        }
    }
    for kind in &layout.kinds {
        painter.fill_round_rect(
            kind.rect,
            7.0,
            if kind.selected {
                theme.tokens.row_selected
            } else {
                theme.tokens.card
            },
        );
        if kind.selected {
            painter.stroke_round_rect(kind.rect, 7.0, theme.tokens.border, 1.0);
        }
        paint_single_line(
            painter,
            kind.label,
            kind.rect,
            12.0,
            if kind.selected { 600 } else { 450 },
            if kind.selected {
                theme.tokens.foreground
            } else {
                theme.tokens.muted_foreground
            },
            HorizontalAlign::Center,
        );
    }
    paint_section_label(
        painter,
        layout.model_section_label,
        "新会话默认模型（全局）",
        theme,
    );
    if let Some(card) = layout.models_card {
        paint_card(painter, card, theme);
        for (index, model) in layout.models.iter().enumerate() {
            if index > 0 {
                paint_divider(painter, card, index as f32 * MODEL_ROW_HEIGHT, theme);
            }
            paint_single_line(
                painter,
                &model.model_id,
                Rect::xywh(
                    model.rect.origin.x + 18.0,
                    model.rect.origin.y,
                    (model.rect.size.x - 58.0).max(0.0),
                    model.rect.size.y,
                ),
                12.0,
                if model.selected { 600 } else { 450 },
                theme.tokens.foreground,
                HorizontalAlign::Start,
            );
            if model.selected {
                painter.stroke_svg_path(
                    SemanticIcon::Check.path(),
                    jian_widgets::Point2D::new(
                        model.rect.max_x() - 34.0,
                        model.rect.origin.y + 12.0,
                    ),
                    16.0,
                    theme.success,
                    1.25,
                );
            }
        }
    } else {
        paint_single_line(
            painter,
            "先输入至少一个模型 ID。",
            Rect::xywh(
                layout.model_section_label.origin.x,
                layout.model_section_label.max_y() + 8.0,
                layout.model_section_label.size.x,
                40.0,
            ),
            12.0,
            450,
            theme.tokens.muted_foreground,
            HorizontalAlign::Start,
        );
    }
    paint_single_line(
        painter,
        status_label(&state.provider_models.status),
        layout.status_rect,
        11.0,
        450,
        if matches!(
            state.provider_models.status,
            ProviderModelsStatus::Failed { .. }
        ) {
            theme.warning
        } else {
            theme.tokens.muted_foreground
        },
        HorizontalAlign::Start,
    );
    paint_button(
        painter,
        layout.cancel_rect,
        "取消",
        SemanticIcon::Close,
        false,
        state.provider_models.status.is_busy(),
        theme,
    );
    paint_button(
        painter,
        layout.save_rect,
        "保存",
        SemanticIcon::Check,
        false,
        state.provider_models.status.is_busy(),
        theme,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Button;
    use jian_widgets::{Point2D, TextLayout};

    /// Bare-bones `Painter` sufficient to call `Button::min_width`, which
    /// only needs `measure_text_weighted` (a default trait method - no
    /// override required here).
    struct MeasuringPainter;

    impl Painter for MeasuringPainter {
        fn begin_frame(&mut self) {}
        fn end_frame(&mut self) {}
        fn fill_rect(&mut self, _rect: Rect, _color: jian_widgets::Color) {}
        fn stroke_rect(&mut self, _rect: Rect, _color: jian_widgets::Color, _width: f32) {}
        fn draw_text(&mut self, _layout: &TextLayout, _origin: Point2D) {}
        fn clip_rect(&mut self, _rect: Rect) {}
        fn stroke_line(
            &mut self,
            _from: Point2D,
            _to: Point2D,
            _color: jian_widgets::Color,
            _width: f32,
        ) {
        }
        fn fill_round_rect(&mut self, _rect: Rect, _radius: f32, _color: jian_widgets::Color) {}
        fn stroke_round_rect(
            &mut self,
            _rect: Rect,
            _radius: f32,
            _color: jian_widgets::Color,
            _width: f32,
        ) {
        }
        fn stroke_svg_path(
            &mut self,
            _d: &str,
            _top_left: Point2D,
            _size: f32,
            _color: jian_widgets::Color,
            _width: f32,
        ) {
        }
        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _offset: Point2D) {}
        fn resize(&mut self, _width: u32, _height: u32) {}
        fn dpi_scale(&self) -> f32 {
            1.0
        }
    }

    #[test]
    fn row_button_width_never_falls_below_what_its_labels_actually_measure() {
        // `ROW_BUTTON_WIDTH` used to be a hand-picked constant that silently
        // clipped both row-action labels (see its doc comment's history).
        // Tying it to `Button::min_width` - the same width formula the
        // shared component now uses - turns "hope it's wide enough" into a
        // checked invariant.
        let mut painter = MeasuringPainter;
        for label in ["编辑", "删除"] {
            let needed = Button::min_width(&mut painter, label, Some(SemanticIcon::Edit));
            assert!(
                ROW_BUTTON_WIDTH >= needed,
                "ROW_BUTTON_WIDTH {ROW_BUTTON_WIDTH} is narrower than {label:?} needs ({needed})"
            );
        }
    }
}
